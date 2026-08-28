//! Putting an operating day's services on the line (plan ch. 11).
//!
//! A [`sim_core::day::OperatingDay`] is a whole day of workings, and only a handful of
//! them are under way at any one minute. This is what turns the plan into trains: as a
//! service's hour comes it claims a unit and the AI takes it out; when it is over the
//! unit is stabled, and the next service that needs the same stock takes that one rather
//! than a new one. Over a looping 24-hour day that keeps the train list at the size of
//! the busiest minute instead of the day's whole service count.
//!
//! **Multiplayer.** Which services are out is a pure function of the clock
//! ([`OperatingDay::active`]), the order they claim in is their departure order, and the
//! stabled units are handed back out in the order they came in — so the server and every
//! client build the same train list, at the same indices, without a message about it.
//! What stays the server's is the *driving*: [`crate::drive_ai`] does not run on a client,
//! which is why [`dispatch`] hands the new trains back rather than driving them itself.

use ai_driver::AiDriver;
use ai_driver::shunt::{ShuntJob, ShuntMove, ShuntTarget};
use bevy::prelude::*;
use sim_core::Sim;
use sim_core::brakes::DriverBrakeValve;
use sim_core::cab::CabInputs;
use sim_core::consist::{ShuntWay, Spawn};
use sim_core::day::{OperatingDay, RunSetup, Service};
use sim_core::timetable::DAY;
use sim_core::train::VehicleSpec;
use sim_core::yard::YardKind;
use std::collections::BTreeMap;
use track_model::{EdgeId, TrackPosition};

/// The timetable run: which plan it comes out of, what the player set for it, and which
/// of its services is theirs.
#[derive(Resource, Debug, Clone)]
pub struct DayRun {
    /// Key of the plan, `"<mod>:<file stem>"` — what the generated weather is seeded
    /// from, so the same service on the same date brings the same sky everywhere.
    pub id: String,
    pub day: OperatingDay,
    pub setup: RunSetup,
    /// Index of the service the player drives.
    pub service: usize,
}

/// Which train each service is using, and which units are standing idle.
#[derive(Resource, Debug, Default)]
pub struct Dispatch {
    /// Train per service; `None` = the service is not out at the moment.
    trains: Vec<Option<usize>>,
    /// Stabled units, oldest first: the train, the vehicle it is headed by and how many
    /// vehicles are behind it. A service only takes one that is made of what it needs.
    free: Vec<(usize, Option<String>, usize)>,
}

impl Dispatch {
    /// A dispatch for `run` in which the player's own service is already out: the train
    /// they were put in is the one their working is being driven by.
    pub fn new(run: &DayRun, player: usize) -> Self {
        let mut trains = vec![None; run.day.services.len()];
        if let Some(slot) = trains.get_mut(run.service) {
            *slot = Some(player);
        }
        Self {
            trains,
            free: Vec::new(),
        }
    }

    /// The service a train is out on, if it is out on one — what says which working the
    /// player has just taken over (`crate::crew`).
    pub fn service_of(&self, train: usize) -> Option<usize> {
        self.trains.iter().position(|out| *out == Some(train))
    }
}

/// A service that has just been put on the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dispatched {
    pub service: usize,
    pub train: usize,
    /// The train was created for this service and has nothing drawn for it yet.
    pub fresh: bool,
}

/// What one dispatching step changed — the caller has to give the new workings a driver
/// and take the driver off the ones that are over.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changes {
    pub started: Vec<Dispatched>,
    /// Trains whose working has ended. Nobody is in the cab of these any more.
    pub released: Vec<usize>,
}

/// One dispatching step: stables what the clock has finished with, puts out what it has
/// reached, and hands back what has been put out so the caller can give it a driver and
/// something to draw.
///
/// `protected` are the trains somebody is driving — the player's own, and on a server
/// every train a client has taken over. They are left alone whatever the plan says about
/// their hours: a unit is never stabled out from under the driver sitting in it. A train
/// that has been handed back to the AI is not protected and goes back into the rotation
/// like any other (`crate::crew`).
pub fn dispatch(
    sim: &mut Sim,
    run: &DayRun,
    state: &mut Dispatch,
    protected: &[usize],
    vehicles: &BTreeMap<String, VehicleSpec>,
    fallback: &VehicleSpec,
) -> Changes {
    if state.trains.len() != run.day.services.len() {
        *state = Dispatch::new(run, protected.first().copied().unwrap_or(0));
    }
    let clock = sim.clock();
    let active = run.day.active(clock);

    // Stable first, so a unit that has just come in can form the next working out.
    let mut changes = Changes::default();
    for i in 0..state.trains.len() {
        if active.contains(&i) {
            continue;
        }
        let Some(train) = state.trains[i] else {
            continue;
        };
        if protected.contains(&train) {
            continue;
        }
        state.trains[i] = None;
        let service = run.day.services[i].clone();
        stable(sim, train, &service);
        state
            .free
            .push((train, service.vehicle.clone(), service.cars));
        changes.released.push(train);
    }

    for i in active {
        if state.trains[i].is_some() {
            continue;
        }
        let service = &run.day.services[i];
        let head = spawn_on(sim, &service.origin, &service.number);
        let stock = (service.vehicle.clone(), service.cars);
        let (train, fresh) = match state
            .free
            .iter()
            .position(|(_, vehicle, cars)| (vehicle.clone(), *cars) == stock)
        {
            Some(k) => {
                let (train, _, _) = state.free.remove(k);
                place(sim, train, head);
                (train, false)
            }
            None => {
                let spec = service
                    .vehicle
                    .as_ref()
                    .and_then(|id| vehicles.get(id))
                    .cloned()
                    .unwrap_or_else(|| fallback.clone());
                (crate::spawn_train(sim, head, service.cars, spec), true)
            }
        };
        sim.trains[train].stabled = false;
        sim.trains[train].number = service.number.clone();
        state.trains[i] = Some(train);
        changes.started.push(Dispatched {
            service: i,
            train,
            fresh,
        });
    }
    changes
}

/// A driver for a working, seeded to the stop it is actually at and given the move that
/// puts the stock away afterwards.
///
/// [`AiDriver::new`] starts at the first stop of the timetable, which is right for a
/// working that has not left yet and wrong for one picked up halfway: the driver would
/// brake for a platform it passed twenty minutes ago. `clock` is the wall clock
/// \[s since local midnight\].
///
/// Where the service names a road, the driver is given a shunt job as well —
/// [`AiDriver::with_shunt`] works it once the last stop has been made, so the unit is
/// *driven* into its siding instead of appearing in it. It is best effort: the window
/// closing places the unit there whatever the driver managed (see [`stable`]).
pub fn driver_for(service: &Service, clock: f64) -> AiDriver {
    let mut driver = AiDriver::new(service.timetable());
    driver.next_stop = next_stop(service, clock);
    match stable_job(service) {
        Some(job) => driver.with_shunt(job),
        None => driver,
    }
}

/// The move that takes a working's stock to the road it is left on, if it has one.
pub fn stable_job(service: &Service) -> Option<ShuntJob> {
    let road = service.stable_at.clone()?;
    let target = ShuntTarget::Yard(road.clone());
    Some(ShuntJob {
        name: format!("{} → {road}", service.number),
        moves: vec![
            match service.stable_way {
                ShuntWay::SetBack => ShuntMove::SetBack(target),
                ShuntWay::DrawUp => ShuntMove::DrawUp(target),
            },
            ShuntMove::Stand,
        ],
    })
}

/// The first stop of `service` whose departure is still ahead at `clock`.
///
/// "Ahead" is less than half a day in front — the way a wall clock tells later from
/// earlier without knowing the date. Past the last stop it falls back to the first: a
/// daily timetable starts over rather than ending.
fn next_stop(service: &Service, clock: f64) -> usize {
    service
        .stops
        .iter()
        .position(|stop| (stop.departure - clock).rem_euclid(DAY) < DAY / 2.0)
        .unwrap_or(0)
}

/// Puts a working's unit away when its hours are over.
///
/// A service that names a road is put on it: a **stabling road** holds the unit where it
/// can be seen and where it occupies the track like any other train, so it is secured
/// rather than switched off; a **portal** is the edge of the module and swallows it
/// altogether. A service that names none leaves its unit standing at its terminus and
/// takes it out of service on the spot, which is what a plan that says nothing means.
///
/// The unit is *placed* rather than shunted there. Driving the move is a shunt job the
/// content can ask for (`ai_driver::shunt`); the dispatching itself has to stay a function
/// of the clock, or a client and the server would end up with different trains on the line
/// (see the module docs).
fn stable(sim: &mut Sim, train: usize, service: &Service) {
    let Some(road) = service.stable_at.clone() else {
        sim.trains[train].stabled = true;
        return;
    };
    let portal = sim
        .yard(&road)
        .is_some_and(|yard| yard.kind == YardKind::Portal);
    if let Err(e) = sim.place_at(train, &road) {
        warn!(
            "service {}: cannot put its stock on {road} ({e:?}) — stabled where it stands",
            service.number
        );
        sim.trains[train].stabled = true;
        return;
    }
    if portal {
        // Off the module: the unit is gone, not standing anywhere to be seen.
        if let Err(e) = sim.withdraw(train, &road) {
            warn!("service {}: {road} did not take it ({e:?})", service.number);
            sim.trains[train].stabled = true;
        }
    } else {
        // It is still on the line, so it may not be left with the brakes released.
        secure(sim, train);
    }
}

/// Leaves a train safe with nobody in the cab: power off, reverser out, a full service
/// application on. Anything left standing on the running line with released brakes is a
/// runaway waiting for a gradient.
pub fn secure(sim: &mut Sim, train: usize) {
    if let Some(cab) = sim.controls.get_mut(train) {
        *cab = CabInputs {
            brake_valve: DriverBrakeValve::Service(1.5),
            ..CabInputs::default()
        };
    }
}

/// Where a spawn point puts the head of a train.
///
/// A plan and a line are two files that can disagree — a day written for one module,
/// started with `--line` on another, names edges or roads that line has never had. That is
/// a mod's mistake and not a crash: it comes back as a warning, and the train goes on the
/// first edge instead.
pub fn spawn_on(sim: &Sim, at: &Spawn, what: &str) -> TrackPosition {
    match at.position(sim) {
        Some(position) => position,
        None => {
            warn!("{what}: {at:?} is not on this line — starting at the beginning instead");
            TrackPosition::new(EdgeId(0), 200.0, 1)
        }
    }
}

/// Puts a stabled unit back on the line at `head`, standing.
fn place(sim: &mut Sim, train: usize, head: TrackPosition) {
    let net = std::mem::take(&mut sim.net);
    sim.trains[train].place_head_at(head, &net);
    sim.net = net;
    for vehicle in &mut sim.trains[train].vehicles {
        vehicle.v = 0.0;
        vehicle.a = 0.0;
    }
    // Whatever the last driver left on the levers is not this one's problem.
    sim.controls[train] = sim_core::cab::CabInputs::default();
}

#[cfg(test)]
mod tests {
    use super::*;
    use content::musterbahn;
    use sim_core::day::{Date, Service};
    use sim_core::timetable::ScheduledStop;
    use sim_core::yard::Yard;

    fn service(number: &str, from: f64, to: f64, s: f64) -> Service {
        let stop = |name: &str, at: f64, time: f64| ScheduledStop {
            name: name.into(),
            edge: EdgeId(0),
            s: at,
            arrival: time,
            departure: time,
            platform: "1".into(),
            module: None,
        };
        Service {
            number: number.into(),
            category: "RB".into(),
            description: String::new(),
            vehicle: None,
            cars: 2,
            origin: Spawn::at(EdgeId(0), s, 1),
            stable_at: None,
            stable_way: ShuntWay::SetBack,
            playable: true,
            module: None,
            stops: vec![stop("A", s, from), stop("B", 2_600.0, to)],
        }
    }

    /// Player train plus a day of three services, only two of which ever overlap.
    fn run() -> (Sim, DayRun, usize) {
        let line = musterbahn().compile().expect("line compiles");
        let mut sim = Sim::new(line.net, line.interlock, 1);
        let player = crate::spawn_train(
            &mut sim,
            TrackPosition::new(EdgeId(0), 200.0, 1),
            2,
            content::vehicles::br101(),
        );
        let day = OperatingDay {
            name: "Test".into(),
            services: vec![
                service("RB 1", 3_600.0, 4_200.0, 200.0),
                service("RB 2", 7_200.0, 7_800.0, 400.0),
                service("RB 3", 7_200.0, 7_800.0, 600.0),
            ],
            ..Default::default()
        };
        let run = DayRun {
            id: "test:day".into(),
            day,
            setup: RunSetup {
                date: Date::default(),
                ..Default::default()
            },
            service: 0,
        };
        (sim, run, player)
    }

    #[test]
    fn a_service_takes_a_train_when_its_hour_comes_and_gives_it_back_after() {
        let (mut sim, run, player) = run();
        let mut state = Dispatch::new(&run, player);
        let mods = BTreeMap::new();
        let loco = content::vehicles::br101();

        // Half past midnight: nothing but the player is out.
        sim.start.hour = 0;
        sim.start.minute = 30;
        assert!(
            dispatch(&mut sim, &run, &mut state, &[player], &mods, &loco)
                .started
                .is_empty()
        );
        assert_eq!(sim.trains.len(), 1);

        // Two hours in, both of the other services are due — two new trains.
        sim.time = 2.0 * 3_600.0 - 30.0 * 60.0;
        let started = dispatch(&mut sim, &run, &mut state, &[player], &mods, &loco).started;
        assert_eq!(started.len(), 2);
        assert!(started.iter().all(|d| d.fresh));
        assert_eq!(sim.trains.len(), 3);
        assert_eq!(sim.trains[1].number, "RB 2");
        assert!(!sim.trains[1].stabled);
        // Dispatching again changes nothing — a service claims once.
        assert!(
            dispatch(&mut sim, &run, &mut state, &[player], &mods, &loco)
                .started
                .is_empty()
        );

        // Their hours over, both are stabled and off the line.
        sim.time = 4.0 * 3_600.0;
        assert!(
            dispatch(&mut sim, &run, &mut state, &[player], &mods, &loco)
                .started
                .is_empty()
        );
        assert!(sim.trains[1].stabled && sim.trains[2].stabled);
        assert_eq!(state.trains[1], None);
    }

    #[test]
    fn a_working_that_names_a_road_leaves_its_stock_on_it() {
        let (mut sim, mut run, player) = run();
        // A siding on the far edge of the Musterbahn, and the portal the line already has.
        sim.yards.push(Yard {
            name: "Abstellgleis".into(),
            kind: YardKind::Stabling,
            at: TrackPosition::new(EdgeId(2), 2_000.0, -1),
            length: 200.0,
        });
        run.day.services[1].stable_at = Some("Abstellgleis".into());
        run.day.services[2].stable_at = Some("Portal Ost".into());
        let mut state = Dispatch::new(&run, player);
        let mods = BTreeMap::new();
        let loco = content::vehicles::br101();

        sim.start.hour = 0;
        sim.time = 2.0 * 3_600.0;
        let started = dispatch(&mut sim, &run, &mut state, &[player], &mods, &loco).started;
        assert_eq!(started.len(), 2);
        let (siding, portal) = (started[0].train, started[1].train);

        sim.time = 4.0 * 3_600.0;
        let changes = dispatch(&mut sim, &run, &mut state, &[player], &mods, &loco);
        assert_eq!(changes.released.len(), 2, "both workings are over");

        // The one with a siding stands in it, on the line and with its brakes on …
        assert!(!sim.trains[siding].stabled, "it is really standing there");
        let head = sim.trains[siding].head_position();
        assert_eq!(head.edge, EdgeId(2));
        assert!((head.s - 2_000.0).abs() < 1.0, "at {} m", head.s);
        assert!(
            matches!(
                sim.controls[siding].brake_valve,
                sim_core::brakes::DriverBrakeValve::Service(_)
            ),
            "left with the brakes released"
        );
        // … and the one sent to a portal has left the module altogether.
        assert!(sim.trains[portal].stabled);
    }

    #[test]
    fn a_road_that_is_not_there_leaves_the_unit_where_it_stands() {
        let (mut sim, mut run, player) = run();
        run.day.services[1].stable_at = Some("Gibt es nicht".into());
        let mut state = Dispatch::new(&run, player);
        let mods = BTreeMap::new();
        let loco = content::vehicles::br101();
        sim.start.hour = 0;
        sim.time = 2.0 * 3_600.0;
        let train = dispatch(&mut sim, &run, &mut state, &[player], &mods, &loco).started[0].train;
        sim.time = 4.0 * 3_600.0;
        dispatch(&mut sim, &run, &mut state, &[player], &mods, &loco);
        // A mod's mistake is a warning and the old behaviour, not a panic.
        assert!(sim.trains[train].stabled);
    }

    #[test]
    fn a_driver_taking_a_working_over_halfway_starts_at_the_stop_ahead() {
        let (_, run, _) = run();
        let service = &run.day.services[0];
        // Before it leaves: the first stop.
        assert_eq!(driver_for(service, 3_500.0).next_stop, 0);
        // Under way between the two: the second, not the platform it has left.
        assert_eq!(driver_for(service, 3_900.0).next_stop, 1);
        // And past the last one it starts over — a daily timetable has no end.
        assert_eq!(driver_for(service, 5_000.0).next_stop, 0);
    }

    #[test]
    fn the_next_day_takes_the_stabled_units_rather_than_new_ones() {
        let (mut sim, run, player) = run();
        let mut state = Dispatch::new(&run, player);
        let mods = BTreeMap::new();
        let loco = content::vehicles::br101();

        sim.start.hour = 0;
        sim.start.minute = 0;
        sim.time = 2.0 * 3_600.0;
        dispatch(&mut sim, &run, &mut state, &[player], &mods, &loco);
        assert_eq!(sim.trains.len(), 3);
        sim.time = 4.0 * 3_600.0;
        dispatch(&mut sim, &run, &mut state, &[player], &mods, &loco);

        // Twenty-four hours on the plan starts over, and it starts over with the same
        // two units rather than two more.
        sim.time = 26.0 * 3_600.0;
        let started = dispatch(&mut sim, &run, &mut state, &[player], &mods, &loco).started;
        assert_eq!(started.len(), 2);
        assert!(started.iter().all(|d| !d.fresh), "the stock was reused");
        assert_eq!(sim.trains.len(), 3, "and no train was added for it");
        // It stands at its origin again, at a stand.
        let head = sim.trains[started[0].train].head_position();
        assert!((head.s - 400.0).abs() < 1.0, "at {} m", head.s);
        assert_eq!(sim.trains[started[0].train].speed(), 0.0);
    }

    #[test]
    fn a_stabled_train_is_off_the_line() {
        let (mut sim, _, _) = run();
        // The player's train stands on edge 0, so the section holding it reads occupied.
        sim.step(Sim::DT);
        assert!(
            sim.interlock.sections.iter().any(|s| s.occupied),
            "a train on the line occupies it"
        );
        // Out of service, and the interlocking stops seeing it there.
        sim.trains[0].stabled = true;
        sim.step(Sim::DT);
        assert!(sim.interlock.sections.iter().all(|s| !s.occupied));
    }

    #[test]
    fn two_peers_dispatch_the_same_trains_off_the_same_clock() {
        // The same plan and the same clock readings, but one peer looks at the clock
        // twice as often — which is what two machines at different frame rates do.
        let readings = [
            vec![0.0, 3_600.0, 7_200.0, 14_400.0, 93_600.0],
            vec![
                0.0, 900.0, 1_800.0, 3_600.0, 5_400.0, 7_200.0, 9_000.0, 14_400.0, 90_000.0,
                93_600.0,
            ],
        ];
        let mut peers = Vec::new();
        for times in readings {
            let (mut sim, run, player) = run();
            let mut state = Dispatch::new(&run, player);
            for time in times {
                sim.time = time;
                dispatch(
                    &mut sim,
                    &run,
                    &mut state,
                    &[player],
                    &BTreeMap::new(),
                    &content::vehicles::br101(),
                );
            }
            peers.push((sim, state));
        }
        // Same trains, at the same indices, in the same services.
        assert_eq!(peers[0].0.trains.len(), peers[1].0.trains.len());
        assert_eq!(peers[0].1.trains, peers[1].1.trains);
        assert!(
            peers[0].1.trains.iter().any(|t| t.is_none()),
            "and the day has moved on"
        );
    }

    #[test]
    fn the_player_keeps_their_train_whatever_the_clock_says() {
        let (mut sim, run, player) = run();
        let mut state = Dispatch::new(&run, player);
        // Long past their service's last arrival.
        sim.time = 12.0 * 3_600.0;
        dispatch(
            &mut sim,
            &run,
            &mut state,
            &[player],
            &BTreeMap::new(),
            &content::vehicles::br101(),
        );
        assert!(!sim.trains[player].stabled);
        assert_eq!(state.trains[run.service], Some(player));
    }
}
