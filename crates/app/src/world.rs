//! Building the simulation out of the loaded mods: line, trains, scenario.
//!
//! Split out of `setup` because the dedicated server (`net::run_dedicated`) needs exactly
//! this and nothing that follows it — no terrain, no scenery, no window. Client and server
//! must land on the same world, so both go through here.

use crate::services::{self, DayRun, Dispatch};
use crate::{arg, spawn_consist, spawn_train};
use ai_driver::{AiDriver, ScheduledStop, Timetable, TimetableKind};
use bevy::prelude::*;
use content::vehicles::{br101, passenger_coach};
use content::{LineSource, musterbahn, musterbahn_day, re_4711, to_musterstadt};
use mod_runtime::ModRuntime;
use sim_core::Sim;
use sim_core::consist::ConsistSource;
use sim_core::day::{Date, OperatingDay};
use sim_core::scenario::Scenario;
use sim_core::weather::WeatherChoice;
use track_model::{EdgeId, TrackPosition};

/// The key the built-in operating day is known and seeded under — the counterpart of the
/// built-in line, for the run picker's first row.
pub const BUILTIN_DAY: &str = "musterbahn";

/// Everything the run is built out of, before anything is drawn.
pub struct World {
    pub sim: Sim,
    /// Train the player drives in single player; in multiplayer the server reassigns it.
    pub player: usize,
    pub drivers: Vec<(usize, AiDriver)>,
    pub line: LineSource,
    /// The operating day the run comes out of — `None` for a scenario or a free run.
    /// With one, `crate::dispatch_services` keeps putting its services on the line as
    /// their hour comes (plan ch. 11).
    pub day: Option<DayRun>,
    /// How far that day has been dispatched. Built here rather than at the first frame,
    /// so the world a client joins is the world the server built.
    pub dispatch: Dispatch,
}

/// Which service of which operating day the player takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceRef {
    /// `"<mod>:<file stem>"`, or [`BUILTIN_DAY`].
    pub day: String,
    /// Index into [`OperatingDay::services`](sim_core::day::OperatingDay::services).
    pub index: usize,
}

/// The plan behind an id: a mod's `days/*.ron`, or the built-in Musterbahn day.
pub fn resolve_day(mods: &ModRuntime, id: &str) -> Option<OperatingDay> {
    mods.mods
        .days
        .get(id)
        .cloned()
        .or_else(|| (id == BUILTIN_DAY).then(musterbahn_day))
}

/// Fingerprint of the world, so client and server can tell they built the same one.
/// Line name and the consists are what a mismatch shows up in first.
pub fn fingerprint(line: &str, sim: &Sim) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |x: u64| {
        h ^= x;
        h = h.wrapping_mul(0x100_0000_01b3);
    };
    for byte in line.as_bytes() {
        mix(u64::from(*byte));
    }
    mix(sim.trains.len() as u64);
    for train in &sim.trains {
        mix(train.vehicles.len() as u64);
        // Variant and load decide which model and which mass a vehicle has. The
        // server builds its world from the scenario and a client from its menu, so
        // the two can disagree here — and a disagreement over liveries and loads is
        // to be refused at join, not discovered by two players describing different
        // trains to each other. Nothing replicates them: they belong to the consist,
        // and the consist is the server's (CLAUDE.md, ch. 20).
        for vehicle in &train.vehicles {
            mix(vehicle.variant.map_or(u64::MAX, |v| v as u64));
            mix(vehicle.load_index.map_or(u64::MAX, |l| l as u64));
        }
    }
    h
}

/// The date `--date 2026-10-03` names, if it names one.
pub fn date_arg() -> Option<Date> {
    let parts: Vec<i64> = arg("--date")?
        .split('-')
        .filter_map(|p| p.parse().ok())
        .collect();
    match parts[..] {
        [year, month, day] => Some(Date {
            year: year as i32,
            month: month as u32,
            day: day as u32,
        }),
        _ => None,
    }
}

/// Builds line, trains and either a scenario or a service out of an operating day.
/// `selection` comes from the menu; CLI flags win over it, so the documented command line
/// invocations stay non-interactive.
pub fn build(mods: &mut ModRuntime, selection: &crate::menu::Selection) -> World {
    let scenario_id = arg("--scenario").or_else(|| selection.scenario_id.clone());
    // `--day example:beispieltag --service 2` takes a service out of an operating day the
    // way `--scenario` takes a scenario, and beats what the menu picked.
    let service_ref = arg("--day")
        .map(|day| ServiceRef {
            day,
            index: arg("--service")
                .and_then(|index| index.parse().ok())
                .unwrap_or_default(),
        })
        .or_else(|| selection.service.clone());
    let plan = service_ref.as_ref().and_then(|reference| {
        let plan = resolve_day(mods, &reference.day);
        if plan.is_none() {
            warn!("operating day {} not found", reference.day);
        }
        plan.filter(|plan| {
            let has = !plan.services.is_empty();
            if !has {
                warn!("operating day {} has no services", reference.day);
            }
            has
        })
    });
    let line_ref = arg("--line")
        .or_else(|| selection.line_ref.clone())
        .or_else(|| {
            scenario_id
                .as_ref()
                .and_then(|id| mods.mods.scenarios.get(id))
                .and_then(|s| s.line.clone())
        })
        .or_else(|| plan.as_ref().and_then(|plan| plan.line.clone()));
    let resolved = line_ref.and_then(|id| match mods.mods.resolve_line(&id) {
        Ok(composed) => {
            for note in &composed.notes {
                info!("{id}: {note}");
            }
            Some(composed)
        }
        Err(e) => {
            warn!("line {id}: {e} — using the example line");
            None
        }
    });
    let modded = resolved.is_some();
    let module_offsets = resolved
        .as_ref()
        .map(|c| c.offsets.clone())
        .unwrap_or_default();
    let line_source = resolved.map(|c| c.line).unwrap_or_else(musterbahn);
    let mut line = line_source.compile().expect("line compiles");
    for warning in mods
        .mods
        .apply_signal_types(&line_source, &mut line.interlock)
    {
        warn!("{}: {warning}", line_source.name);
    }
    // Track types: specs behind the names, and the superstructure speed cap
    // merged into the one profile AI, LZB, HUD and scoring read.
    for warning in mods.mods.apply_track_types(&mut line.net) {
        warn!("{}: {warning}", line_source.name);
    }
    let mut sim = Sim::new(line.net, line.interlock, 2024);
    // Stabling roads and portals: line content, resolved onto the graph by the compile and
    // qualified against the module offsets with the rest of it (plan ch. 11).
    sim.yards = line.yards;

    // Vehicle from menu selection or CLI flag.
    let loco = arg("--loco")
        .or_else(|| selection.loco_id.clone())
        .and_then(|id| match mods.mods.vehicles.get(&id) {
            Some(spec) => Some(spec.clone()),
            None => {
                warn!("vehicle {id} not found — using the BR 101");
                None
            }
        })
        .unwrap_or_else(br101);

    // The scenario is read before anything is put on the line, because it may say what
    // stands there (`Scenario::consists`, plan ch. 11).
    let scenario = scenario_id.as_ref().and_then(|id| {
        let mut scenario = mods.mods.scenarios.get(id).cloned();
        if scenario.is_none() {
            warn!("scenario {id} not found");
        }
        if let Some(scenario) = scenario.as_mut() {
            for warning in mod_runtime::qualify_scenario(scenario, &module_offsets) {
                warn!("scenario {id}: {warning}");
            }
        }
        scenario
    });

    // A timetable run: the player takes one service out of an operating day, and the
    // day's other workings go on the line around them (plan ch. 11). The player's own
    // train comes first, so it keeps index 0 whichever run this is.
    let mut drivers = Vec::new();
    let mut dispatch = Dispatch::default();
    let mut day = None;
    let player = if let (Some(reference), Some(mut plan)) = (service_ref, plan) {
        for warning in mod_runtime::qualify_day(&mut plan, &module_offsets) {
            warn!("day {}: {warning}", reference.day);
        }
        let index = if reference.index < plan.services.len() {
            reference.index
        } else {
            warn!(
                "operating day {} has no service {} — taking the first",
                reference.day, reference.index
            );
            0
        };
        let service = plan.services[index].clone();
        // What the player set in the run picker; without a picker (a CLI run) the plan's
        // own date and weather, with `--date` still able to move the date.
        let mut setup = selection.setup.unwrap_or_else(|| plan.setup());
        if let Some(date) = date_arg() {
            setup.date = date;
        }
        // The service's own vehicle where it names one — the timetable says what runs.
        let head_vehicle = service
            .vehicle
            .as_ref()
            .and_then(|id| match mods.mods.vehicles.get(id) {
                Some(spec) => Some(spec.clone()),
                None => {
                    warn!("service {}: vehicle {id} not found", service.number);
                    None
                }
            })
            .unwrap_or_else(|| loco.clone());
        let head = services::spawn_on(&sim, &service.origin, &service.number);
        let player = spawn_train(&mut sim, head, service.cars, head_vehicle);
        sim.trains[player].vehicles[0].variant = selection.variant;

        // The run's clock, its timetable and its scoring. A service carries no events, so
        // the scenario around it is a shell: the score keeper wants a name, and the sky
        // and the sun want the start.
        let (from, to) = service.route();
        let mut shell = Scenario {
            // The run's name is where it goes; the train number is the timetable's, and
            // the HUD already prints that beside it.
            name: format!("{from} – {to}"),
            description: service.description.clone(),
            start: plan.start_time(index, setup.date),
            player_train: player,
            ..Scenario::default()
        };
        // Nothing reads it back — it says where the run's timetable came from, for a
        // script or a saved run that wants to know.
        shell.timetable = Some(reference.day.clone());
        sim.set_scenario(shell, service.timetable());

        let run = DayRun {
            id: reference.day.clone(),
            day: plan,
            setup,
            service: index,
        };
        apply_weather(&mut sim, &run);
        info!(
            "operating day {}: {} of {} services, driving {} at {:02}:{:02} on {:02}.{:02}.{}",
            run.id,
            index + 1,
            run.day.services.len(),
            service.number,
            sim.start.hour,
            sim.start.minute,
            setup.date.day,
            setup.date.month,
            setup.date.year,
        );
        // Everything else the plan has out at that minute, put on the line before the
        // first frame — a client that joins has to find the same trains the server has.
        dispatch = Dispatch::new(&run, player);
        // The fallback is the built-in vehicle, never the player's pick: an AI service
        // that names no vehicle has to be made of the same thing on every machine, and
        // the menu's choice is one player's (see `dispatch_services`).
        let fallback = br101();
        let changes = services::dispatch(
            &mut sim,
            &run,
            &mut dispatch,
            &[player],
            &mods.mods.vehicles,
            &fallback,
        );
        let clock = sim.clock();
        for started in changes.started {
            drivers.push((
                started.train,
                services::driver_for(&run.day.services[started.service], clock),
            ));
        }
        // The stock the plan has standing about from the first minute — units in the
        // sidings, a rake waiting to be collected. It goes on after the player's train,
        // so their index stays what the run picker promised.
        spawn_consists(
            &mut sim,
            mods,
            &run.day.consists,
            &mut drivers,
            &reference.day,
        );
        day = Some(run);
        player
    } else if let Some(scenario) = scenario.as_ref().filter(|s| !s.consists.is_empty()) {
        // The scenario says what stands on the line, and which of them is the player's.
        // The list's order is the order the indices run in, so it is what the events
        // address — the menu's vehicle is not asked, because the scenario has answered.
        let trains = spawn_consists(
            &mut sim,
            mods,
            &scenario.consists,
            &mut drivers,
            scenario_id.as_deref().unwrap_or_default(),
        );
        let player = trains
            .get(scenario.player_train)
            .copied()
            .unwrap_or_else(|| {
                warn!(
                    "scenario {:?}: no consist {} to drive — taking the first",
                    scenario_id, scenario.player_train
                );
                trains[0]
            });
        sim.trains[player].vehicles[0].variant =
            selection.variant.or(sim.trains[player].vehicles[0].variant);
        player
    } else {
        let player = spawn_train(&mut sim, TrackPosition::new(EdgeId(0), 200.0, 1), 5, loco);
        // The livery picked in the menu. It rides on the vehicle, not on a render
        // component, so it is part of the consist the fingerprint below compares.
        sim.trains[player].vehicles[0].variant = selection.variant;
        player
    };

    // Second train, timetable and scenario belong to the example line — a modded line
    // brings its own scenario or none at all, and a timetable run brings its whole day.
    if !modded && day.is_none() {
        let ai_train = spawn_train(
            &mut sim,
            TrackPosition::new(EdgeId(1), 400.0, 1),
            3,
            br101(),
        );
        drivers.push((
            ai_train,
            AiDriver::new(Timetable {
                number: "RB 20".into(),
                category: "RB".into(),
                kind: TimetableKind::Scenario,
                module: None,
                stops: vec![ScheduledStop {
                    name: "Musterstadt".into(),
                    edge: EdgeId(2),
                    s: 2600.0,
                    arrival: 300.0,
                    departure: 360.0,
                    platform: "1".into(),
                    module: None,
                }],
            }),
        ));

        // Load the scenario with timetable and scoring (plan 11.4).
        let mut scenario = to_musterstadt();
        scenario.player_train = player;
        sim.set_scenario(scenario, re_4711());
    }

    // `--scenario <mod>:<name>` runs a scenario out of a mod. A `timetable/*.ron` the
    // scenario references adds stop scoring; without one only the scenario points count.
    // A scenario that could not be read has been reported where it was read.
    if let (Some(id), Some(mut scenario)) = (scenario_id, scenario) {
        scenario.player_train = player;
        let timetable = scenario
            .timetable
            .as_deref()
            .and_then(|name| {
                let timetable = mods.mods.timetables.get(name).cloned();
                if timetable.is_none() {
                    warn!("scenario {id}: timetable {name:?} not found");
                }
                timetable
            })
            .map(|mut timetable| {
                for warning in mod_runtime::qualify_timetable(&mut timetable, &module_offsets) {
                    warn!("scenario {id}: {warning}");
                }
                timetable
            })
            .unwrap_or_else(|| sim_core::timetable::Timetable {
                // The train's own number where the scenario gave it one, so the
                // HUD reads "Lz 77400 · Rangierfahrt" rather than the name twice.
                number: sim.trains[player].number.clone(),
                ..default()
            });
        sim.set_scenario(scenario, timetable);
    }

    // Line and scenario hooks: `on_load` now, `on_frame` every frame (plan 19.7).
    mods.begin(&mut sim, &line_source);
    World {
        sim,
        player,
        drivers,
        line: line_source,
        day,
        dispatch,
    }
}

/// Puts the trains a scenario or an operating day declares on the line, in the order they
/// are declared — which is the order their indices run in.
///
/// A consist that names a timetable is driven to it by the AI. One that names none stands
/// where it was put: stock in a siding, a rake waiting to be collected, a light engine on
/// shed. Naming a **portal** as the spawn puts it at the edge of the modelled railway,
/// which is how a train comes out of a part of the line that was never built.
fn spawn_consists(
    sim: &mut Sim,
    mods: &ModRuntime,
    sources: &[ConsistSource],
    drivers: &mut Vec<(usize, AiDriver)>,
    what: &str,
) -> Vec<usize> {
    let mut built = Vec::new();
    for source in sources {
        let head = services::spawn_on(sim, &source.at, &source.number);
        let vehicles: Vec<_> = source
            .each_vehicle()
            .map(|(id, variant)| {
                let spec = mods.mods.vehicles.get(id).cloned().unwrap_or_else(|| {
                    warn!("{what}: consist {}: vehicle {id} not found", source.number);
                    passenger_coach()
                });
                (spec, variant)
            })
            .collect();
        let train = spawn_consist(
            sim,
            head,
            vehicles.iter().map(|(spec, _)| spec.clone()).collect(),
            source.prepared,
        );
        for (vehicle, (_, variant)) in sim.trains[train].vehicles.iter_mut().zip(&vehicles) {
            vehicle.variant = *variant;
        }
        sim.trains[train].number = source.number.clone();
        // A timetable makes it a working and a shunt job makes it a pilot; with both, the
        // job is worked once the last stop has been made. With neither it stands where it
        // was put, and a train left standing has its brakes applied.
        let timetable = source
            .timetable
            .as_deref()
            .and_then(|name| match mods.mods.timetables.get(name) {
                Some(timetable) => Some(timetable.clone()),
                None => {
                    warn!(
                        "{what}: consist {}: timetable {name} not found",
                        source.number
                    );
                    None
                }
            })
            .map(|mut timetable| {
                for warning in mod_runtime::qualify_timetable(&mut timetable, &Default::default()) {
                    warn!("{what}: consist {}: {warning}", source.number);
                }
                timetable
            });
        match (timetable, source.shunt.clone()) {
            (Some(timetable), Some(job)) => {
                drivers.push((train, AiDriver::new(timetable).with_shunt(job)))
            }
            (Some(timetable), None) => drivers.push((train, AiDriver::new(timetable))),
            (None, Some(job)) => drivers.push((train, AiDriver::shunting(job))),
            (None, None) => services::secure(sim, train),
        }
        built.push(train);
    }
    built
}

/// Puts the weather the run picker asked for into the run.
///
/// Dynamic weather is *generated*, not placed: from here on the sky is a function of the
/// clock and the seed, which is what lets it keep moving for a whole 24-hour day and what
/// makes it the same sky on every machine. A named weather is placed, ground and all — a
/// player who asked for snow means the run to start in it, not to run into it five
/// minutes later.
fn apply_weather(sim: &mut Sim, run: &DayRun) {
    let clock = sim.clock();
    match (run.setup.weather, run.setup.dynamic(&run.id)) {
        (_, Some(dynamic)) => sim.weather.generate(dynamic, clock),
        (WeatherChoice::Fixed(preset), None) => sim.weather.place(preset.weather(), 0.0),
        (WeatherChoice::Dynamic, None) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::Selection;
    use sim_core::consist::{ConsistSource, Formation, Spawn};
    use sim_core::day::{Date, RunSetup};
    use sim_core::timetable::TimetableKind;
    use sim_core::weather::{Precip, Preset};

    /// A run picked out of the built-in operating day, set up as the picker would.
    fn timetable_run(setup: RunSetup) -> World {
        let mut mods = ModRuntime::load("../../mods");
        let day = musterbahn_day();
        // The 08:12 out of Musterbach.
        let index = day
            .services
            .iter()
            .position(|service| service.departure() == 8.0 * 3_600.0 + 720.0)
            .expect("the plan has an 08:12");
        build(
            &mut mods,
            &Selection {
                service: Some(ServiceRef {
                    day: BUILTIN_DAY.into(),
                    index,
                }),
                setup: Some(setup),
                ..Selection::default()
            },
        )
    }

    #[test]
    fn a_timetable_run_starts_at_its_service_on_the_day_that_was_set() {
        let world = timetable_run(RunSetup {
            date: Date {
                year: 2026,
                month: 1,
                day: 9,
            },
            weather: sim_core::weather::WeatherChoice::Fixed(Preset::Snow),
        });
        let sim = &world.sim;
        // Two minutes before the service leaves, on the date the player set — not the
        // plan's own August date.
        assert_eq!((sim.start.hour, sim.start.minute), (8, 10));
        assert_eq!(
            (sim.start.year, sim.start.month, sim.start.day),
            (2026, 1, 9)
        );
        // The service's own timetable is what the run is scored against, and it is read
        // around the clock rather than from the start of the run.
        assert_eq!(sim.score.timetable.kind, TimetableKind::Daily);
        assert_eq!(sim.score.timetable.stops.len(), 2);
        assert!(sim.score.timetable.number.starts_with("RB "));
        // The train stands at the service's origin, facing the way it leaves.
        let head = sim.trains[world.player].head_position();
        assert_eq!(head.edge, track_model::EdgeId(0));
        assert_eq!(head.dir, 1);
        // A named weather is placed, ground and all: the run begins in the snow rather
        // than driving into it.
        assert_eq!(sim.weather.now.precip, Precip::Snow);
        assert!(sim.weather.snow > 0.0);
        assert!(sim.weather.dynamic.is_none(), "nothing is generating it");
        // And the day is there for the dispatcher to carry on with.
        let run = world.day.expect("a timetable run carries its plan");
        assert_eq!(run.id, BUILTIN_DAY);
        assert_eq!(run.day.services.len(), musterbahn_day().services.len());
    }

    #[test]
    fn a_dynamic_day_generates_its_weather_out_of_the_date() {
        let january = RunSetup {
            date: Date {
                year: 2026,
                month: 1,
                day: 9,
            },
            weather: sim_core::weather::WeatherChoice::Dynamic,
        };
        let world = timetable_run(january);
        let dynamic = world
            .sim
            .weather
            .dynamic
            .expect("the sky makes itself from here on");
        assert_eq!(dynamic.month, 1);
        assert_eq!(dynamic.seed, january.seed(BUILTIN_DAY));
        // The run starts in the weather of its own hour, not the plan's first.
        assert_eq!(world.sim.weather.now, dynamic.at(world.sim.clock()));
        // Same service, same date: the same day of weather, on every machine.
        assert_eq!(
            timetable_run(january).sim.weather.now,
            world.sim.weather.now
        );
        // Another date is another day.
        let july = RunSetup {
            date: Date {
                year: 2026,
                month: 7,
                day: 9,
            },
            ..january
        };
        assert_ne!(
            timetable_run(july)
                .sim
                .weather
                .dynamic
                .expect("dynamic")
                .seed,
            dynamic.seed
        );
    }

    /// The shipped example content, end to end: the evening working runs, and when its
    /// hours are over its unit is standing in the siding the plan names.
    #[test]
    fn the_example_day_puts_its_evening_working_in_the_siding() {
        let mut mods = ModRuntime::load("../../mods");
        let plan = mods.mods.days["example:beispieltag"].clone();
        let morning = plan
            .services
            .iter()
            .position(|service| service.number == "RB 30001")
            .expect("the morning working");
        let evening = plan
            .services
            .iter()
            .position(|service| service.number == "RB 30015")
            .expect("the evening working");
        let mut world = build(
            &mut mods,
            &Selection {
                line_ref: Some("example:beispielstrecke".into()),
                service: Some(ServiceRef {
                    day: "example:beispieltag".into(),
                    index: morning,
                }),
                ..Selection::default()
            },
        );
        let run = world.day.take().expect("a timetable run");
        let mut dispatch = std::mem::take(&mut world.dispatch);
        let loco = br101();
        let mut step = |sim: &mut Sim, at: f64| {
            sim.time = at - sim.start.seconds();
            services::dispatch(
                sim,
                &run,
                &mut dispatch,
                &[world.player],
                &mods.mods.vehicles,
                &loco,
            )
        };

        // A quarter past nine in the evening: the last passenger working is out.
        let started = step(&mut world.sim, 21.0 * 3_600.0 + 20.0 * 60.0).started;
        let train = started
            .iter()
            .find(|d| d.service == evening)
            .expect("the evening working was put on the line")
            .train;
        assert!(!world.sim.trains[train].stabled);

        // Twenty to ten: past the end of its window — a working with a road to go to is
        // given the longer `SHUNT_TAIL` to drive there — and its stock is in the siding.
        let changes = step(&mut world.sim, 21.0 * 3_600.0 + 40.0 * 60.0);
        assert!(changes.released.contains(&train), "it was given back");
        let consist = &world.sim.trains[train];
        assert!(
            !consist.stabled,
            "a unit in a siding is really standing there"
        );
        let road = world.sim.yard("Abstellgleis 1").expect("the siding").at;
        let head = consist.head_position();
        assert_eq!(head.edge, road.edge, "on the siding, not at the platform");
        assert!((head.s - road.s).abs() < 1.0, "at {} m", head.s);
        // And left with the brakes on, because it is on the track like any other train.
        assert!(matches!(
            world.sim.controls[train].brake_valve,
            sim_core::brakes::DriverBrakeValve::Service(_)
        ));
    }

    #[test]
    fn a_scenario_puts_its_own_trains_on_the_line() {
        let mut mods = ModRuntime::load("../../mods");
        let coach = mods
            .mods
            .vehicles
            .keys()
            .next()
            .expect("the example mod ships a vehicle")
            .clone();
        mods.mods.scenarios.insert(
            "test:consists".into(),
            Scenario {
                name: "Rangieren".into(),
                line: Some("example:beispielstrecke".into()),
                consists: vec![
                    ConsistSource {
                        number: "Lt 1".into(),
                        vehicles: vec![Formation::single(&coach)],
                        at: Spawn::Yard("Abstellgleis 1".into()),
                        prepared: true,
                        timetable: None,
                        shunt: None,
                        module: None,
                    },
                    ConsistSource {
                        number: "Wagengruppe".into(),
                        vehicles: vec![Formation::several(&coach, 3)],
                        at: Spawn::at(track_model::EdgeId(0), 1_000.0, 1),
                        prepared: false,
                        timetable: None,
                        shunt: None,
                        module: None,
                    },
                ],
                // The second one is the player's, which is the whole point of the field.
                player_train: 1,
                ..Scenario::default()
            },
        );
        let world = build(
            &mut mods,
            &Selection {
                scenario_id: Some("test:consists".into()),
                ..Selection::default()
            },
        );
        assert_eq!(
            world.sim.trains.len(),
            2,
            "the scenario said what stands there"
        );
        assert_eq!(world.player, 1, "and which of them is the player's");
        assert_eq!(world.sim.trains[0].number, "Lt 1");
        assert_eq!(world.sim.trains[1].vehicles.len(), 3);
        // The first stands on the road it named …
        let road = world.sim.yard("Abstellgleis 1").expect("the siding").at;
        assert_eq!(world.sim.trains[0].head_position().edge, road.edge);
        // … and neither is driven by anybody, so both are left braked.
        assert!(world.drivers.is_empty());
        assert!(matches!(
            world.sim.controls[0].brake_valve,
            sim_core::brakes::DriverBrakeValve::Service(_)
        ));
        // A consist that is not prepared is a cold engine.
        assert!(!world.sim.trains[1].vehicles[0].traction.battery);
    }

    /// The shipped shunting scenario, end to end: the light engine stands in the siding in
    /// front of a Sperrsignal at Sh 0, and the interlocking sets the shunting route out of
    /// it by itself — which is the signalman answering a movement that has drawn up.
    #[test]
    fn the_example_shunting_scenario_is_let_out_of_its_siding() {
        use sim_core::interlock::SignalKind;
        let mut mods = ModRuntime::load("../../mods");
        let mut world = build(
            &mut mods,
            &Selection {
                scenario_id: Some("example:rangierfahrt".into()),
                ..Selection::default()
            },
        );
        let sperr = world
            .sim
            .interlock
            .signals
            .iter()
            .find(|signal| signal.kind == SignalKind::Shunting)
            .expect("the example line has a Sperrsignal")
            .id;

        world.sim.step(Sim::DT);
        assert!(
            !world.sim.interlock.signal(sperr).aspect.permits_shunting(),
            "it rests at Sh 0"
        );
        assert_eq!(
            world.sim.interlock.signal(sperr).aspect.main,
            None,
            "a Sperrsignal has no main aspect at all"
        );

        // The turnout takes six seconds to lie over; give it a little more than that.
        for _ in 0..2_000 {
            world.sim.step(Sim::DT);
        }
        assert!(
            world.sim.interlock.signal(sperr).aspect.permits_shunting(),
            "Sh 1 — the shunting route out of the siding was set"
        );
        // And it is still no road for a train: Sh 1 says nothing to one.
        assert_eq!(
            world
                .sim
                .interlock
                .signal_speed(sperr, sim_core::shunt::Movement::Train),
            Some(sim_core::shunt::SHUNTING_SPEED_KMH),
            "a Sperrsignal showing Sh 1 lets anything past at shunting speed"
        );
    }

    #[test]
    fn a_service_out_of_a_portal_appears_at_it() {
        let mut mods = ModRuntime::load("../../mods");
        let plan = mods.mods.days["example:beispieltag"].clone();
        let freight = plan
            .services
            .iter()
            .position(|service| service.number == "Gz 51230")
            .expect("the night freight");
        assert_eq!(
            plan.services[freight].origin.yard(),
            Some("Portal Ost"),
            "it comes off the unbuilt railway in the east"
        );
        // Its stock is put at the portal, not made up in the middle of the line.
        let world = build(
            &mut mods,
            &Selection {
                line_ref: Some("example:beispielstrecke".into()),
                service: Some(ServiceRef {
                    day: "example:beispieltag".into(),
                    index: freight,
                }),
                ..Selection::default()
            },
        );
        let portal = world.sim.yard("Portal Ost").expect("the portal").at;
        let head = world.sim.trains[world.player].head_position();
        assert_eq!(head.edge, portal.edge);
        assert!((head.s - portal.s).abs() < 1.0, "at {} m", head.s);
    }

    #[test]
    fn a_working_with_a_road_is_given_the_move_to_it() {
        let mods = ModRuntime::load("../../mods");
        let plan = &mods.mods.days["example:beispieltag"];
        let evening = plan
            .services
            .iter()
            .find(|service| service.number == "RB 30015")
            .expect("the evening working");
        let job = services::stable_job(evening).expect("it has a road to go to");
        assert!(matches!(
            job.moves.first(),
            Some(ai_driver::shunt::ShuntMove::DrawUp(
                ai_driver::shunt::ShuntTarget::Yard(road)
            )) if road == "Abstellgleis 1"
        ));
        assert_eq!(job.moves.last(), Some(&ai_driver::shunt::ShuntMove::Stand));
        // The driver works its timetable first and the job after the last stop.
        let driver = services::driver_for(evening, 0.0);
        assert!(driver.shunt.is_some());
        // A working that names no road is given no job at all.
        let morning = plan
            .services
            .iter()
            .find(|service| service.number == "RB 30001")
            .expect("the morning working");
        assert!(services::stable_job(morning).is_none());
        assert!(services::driver_for(morning, 0.0).shunt.is_none());
    }

    #[test]
    fn the_plans_own_standing_stock_is_there_from_the_first_minute() {
        let mut mods = ModRuntime::load("../../mods");
        let world = build(
            &mut mods,
            &Selection {
                line_ref: Some("example:beispielstrecke".into()),
                service: Some(ServiceRef {
                    day: "example:beispieltag".into(),
                    index: 0,
                }),
                ..Selection::default()
            },
        );
        // The player's train, plus whatever the plan has standing about.
        assert!(
            world
                .sim
                .trains
                .iter()
                .any(|train| train.number == "Übergabe 62701"),
            "the plan's standing stock was not put on the line"
        );
    }

    /// A consist may carry a shunt job and nothing else: no timetable, no working — a
    /// pilot that exists to move stock about, driven by the AI on its own.
    #[test]
    fn a_consist_may_be_nothing_but_a_shunt_job() {
        let mut mods = ModRuntime::load("../../mods");
        let plan = mods.mods.days["example:beispieltag"].clone();
        let pilot = plan
            .consists
            .iter()
            .find(|consist| consist.shunt.is_some())
            .expect("the example day has a shunt job of its own");
        assert!(pilot.timetable.is_none(), "and no timetable at all");
        let world = build(
            &mut mods,
            &Selection {
                line_ref: Some("example:beispielstrecke".into()),
                service: Some(ServiceRef {
                    day: "example:beispieltag".into(),
                    index: 0,
                }),
                ..Selection::default()
            },
        );
        let train = world
            .sim
            .trains
            .iter()
            .position(|train| train.number == pilot.number)
            .expect("it is on the line");
        let driver = world
            .drivers
            .iter()
            .find(|(driven, _)| *driven == train)
            .map(|(_, driver)| driver)
            .expect("and it has a driver");
        assert!(driver.shunt.is_some(), "who was given the job");
        assert!(driver.timetable.stops.is_empty(), "and nothing else");
    }

    #[test]
    fn a_free_run_is_untouched_by_any_of_it() {
        let mut mods = ModRuntime::load("../../mods");
        let world = build(&mut mods, &Selection::default());
        assert!(world.day.is_none());
        // The example line's own demo scenario, exactly as before.
        assert!(!world.sim.scenario.scenario.events.is_empty());
        assert!(world.sim.weather.dynamic.is_none());
    }
}
