//! Who is at the controls (plan ch. 11).
//!
//! A driver is responsible for exactly one train at a time, or for none. [`Duty`] holds
//! which one; `PlayerTrain` only says where the player is standing, and the two come apart
//! the moment they get up and walk. That split is the whole of the arbitration between the
//! player and the AI: [`crate::drive_ai`] drives every train it has a driver for **except**
//! the one on duty, and `ui::player_input` moves the levers only of that same one. There is
//! never a moment where both are on them.
//!
//! Three things happen, and they are all the player walking:
//!
//! * **Getting out** hands the train over. The AI takes the working on from the stop that
//!   is actually next (`services::driver_for`), or, where the train has no working at all,
//!   the train is simply secured — nobody leaves a train on the running line with the
//!   brakes released.
//! * **Walking into another train** is boarding it (`walk::walk_player`); that alone makes
//!   the player a passenger, not a driver.
//! * **Taking over** is a key of its own, pressed in the cab. It is refused with a reason
//!   rather than silently ignored, because "the button did nothing" is the worst answer a
//!   simulator can give.
//!
//! **Multiplayer.** A client may not simply decide it drives a train — the server owns the
//! world and has to be able to say no (CLAUDE.md ch. 20). On a client the key sends a
//! [`TakeOver`](crate::net::TakeOver) and changes nothing; what the server grants comes
//! back as the `Welcome` that already exists for joining, and that is what moves the duty.
//! On a host and in single player the answer is given here directly.

use bevy::prelude::*;
use sim_core::Sim;
use sim_core::score::ScoreKeeper;

use crate::bindings::{Action, Input};
use crate::services::{self, DayRun, Dispatch};
use crate::ui::{CameraMode, CameraState};
use crate::walk::{Place, Walker};
use crate::{AiDrivers, PlayerTrain, SimResource, net};

/// The train the player is in charge of. `None` = they are a passenger, on the platform,
/// or have handed their working over.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Duty(pub Option<usize>);

/// Why a train may not be taken over. Every one of them is shown to the player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Not in the train — the levers are not worked from the platform.
    NotAboard,
    /// Out of service, or no longer a train at all.
    OutOfService,
    /// Nothing in it that pulls. A rake of coaches is driven by whatever is on the front.
    NothingToDriveWith,
    /// A scenario's train belongs to the scenario: its events, its scoring and its
    /// ending all name that one train. Swapping is a timetable run's freedom.
    ScenarioTrain,
    /// Somebody else is driving it.
    AnotherDriver,
}

impl Refusal {
    /// The message key it is shown under.
    pub fn key(self) -> &'static str {
        match self {
            Refusal::NotAboard => "crew-not-aboard",
            Refusal::OutOfService => "crew-out-of-service",
            Refusal::NothingToDriveWith => "crew-nothing-to-drive",
            Refusal::ScenarioTrain => "crew-scenario-train",
            Refusal::AnotherDriver => "crew-another-driver",
        }
    }
}

/// Whether the player may take `train` over.
///
/// `timetable` says a run out of an operating day is under way — only then may the player
/// change trains at all. Without one there is exactly one train that is theirs: the one
/// the scenario or the free run put them in.
pub fn may_take_over(
    sim: &Sim,
    train: usize,
    walker: &Walker,
    timetable: bool,
    taken: bool,
) -> Result<(), Refusal> {
    // `None` is the seat itself: they never got up.
    if matches!(walker.place, Some(Place::Outside { .. })) {
        return Err(Refusal::NotAboard);
    }
    let Some(consist) = sim.trains.get(train) else {
        return Err(Refusal::OutOfService);
    };
    if consist.stabled || consist.vehicles.is_empty() {
        return Err(Refusal::OutOfService);
    }
    // Whose train it is comes before what it is made of: a scenario's answer is the same
    // whether the thing standing there could be driven or not.
    if !timetable && train != sim.scenario.scenario.player_train {
        return Err(Refusal::ScenarioTrain);
    }
    if !consist.vehicles.iter().any(|vehicle| vehicle.is_powered()) {
        return Err(Refusal::NothingToDriveWith);
    }
    if taken {
        return Err(Refusal::AnotherDriver);
    }
    Ok(())
}

/// Hands `train` back: the AI takes the working on, or the train is secured where it has
/// no working to take on.
pub fn hand_over(
    sim: &mut Sim,
    drivers: &mut AiDrivers,
    run: Option<&DayRun>,
    dispatch: &Dispatch,
    train: usize,
) {
    let clock = sim.clock();
    let working = run.and_then(|run| {
        dispatch
            .service_of(train)
            .and_then(|index| run.day.services.get(index))
    });
    match working {
        Some(service) => {
            let ai = services::driver_for(service, clock);
            match drivers.0.iter_mut().find(|(driven, _)| *driven == train) {
                Some(slot) => slot.1 = ai,
                None => drivers.0.push((train, ai)),
            }
            message(sim, "crew-handed-over", &sim.trains[train].number.clone());
        }
        // No working, so nobody is coming to take it on: it stays where it is, braked.
        None => {
            drivers.0.retain(|(driven, _)| *driven != train);
            services::secure(sim, train);
            message(sim, "crew-secured", &sim.trains[train].number.clone());
        }
    }
}

/// Puts the player on the levers of `train`: the AI lets go, and the run follows the
/// driver — from here on they are scored against this working, not the one they left.
pub fn take_over(
    sim: &mut Sim,
    drivers: &mut AiDrivers,
    duty: &mut Duty,
    run: Option<&DayRun>,
    dispatch: &Dispatch,
    train: usize,
) {
    duty.0 = Some(train);
    drivers.0.retain(|(driven, _)| *driven != train);
    sim.scenario.scenario.player_train = train;
    if let Some(service) = run.and_then(|run| {
        dispatch
            .service_of(train)
            .and_then(|index| run.day.services.get(index))
    }) {
        sim.score = ScoreKeeper::new(train, service.timetable());
    }
    message(sim, "crew-took-over", &sim.trains[train].number.clone());
}

/// Says what happened, in the same place the dispatcher's messages appear.
fn message(sim: &mut Sim, key: &str, train: &str) {
    let time = sim.time;
    let text = if train.is_empty() {
        i18n::t!(key)
    } else {
        i18n::t!(key, train = train.to_string())
    };
    sim.scenario.message(time, text, false);
}

/// The player getting up, walking away, and sitting down somewhere else.
///
/// It runs before `drive_ai`, so a train handed over in this frame is driven by the AI in
/// the same one and never coasts for a frame with nobody on the levers.
// A Bevy system takes its resources as parameters — the argument count says nothing here.
#[allow(clippy::too_many_arguments)]
pub fn crew_change(
    input: Input,
    mut sim: ResMut<SimResource>,
    mut duty: ResMut<Duty>,
    mut drivers: ResMut<AiDrivers>,
    mut requests: MessageWriter<net::TakeOverRequest>,
    player: Res<PlayerTrain>,
    walker: Res<Walker>,
    camera: Res<CameraState>,
    dispatch: Res<Dispatch>,
    run: Option<Res<DayRun>>,
    host: Option<Res<net::Host>>,
    role: Option<Res<net::Role>>,
) {
    // The dedicated server has no player of its own standing anywhere.
    if role.as_deref() == Some(&net::Role::Server) {
        return;
    }
    let run = run.as_deref();
    let sim = &mut sim.0;

    // Off the train, or into another one: either way the working they were on is not
    // theirs any more. This is the "get out and the AI takes over" of the whole feature —
    // it needs no key, because walking away is the act.
    if let Some(driving) = duty.0
        && (matches!(walker.place, Some(Place::Outside { .. })) || player.0 != driving)
    {
        duty.0 = None;
        hand_over(sim, &mut drivers, run, &dispatch, driving);
    }

    if !input.just_pressed(Action::TakeOver) {
        return;
    }
    // The same key gives a train back — a driver who wants to stretch their legs should
    // not have to walk out of the door to do it.
    if let Some(driving) = duty.0 {
        duty.0 = None;
        hand_over(sim, &mut drivers, run, &dispatch, driving);
        return;
    }
    // The detached cameras are nobody standing anywhere — the key belongs to the person in
    // the train, whether they are at the desk or still walking up the aisle to it.
    if camera.mode == CameraMode::Outside || camera.mode == CameraMode::Wayside {
        return;
    }
    let train = player.0;
    let taken = host
        .as_deref()
        .is_some_and(|host| host.is_player_driven(train));
    match may_take_over(sim, train, &walker, run.is_some(), taken) {
        Ok(()) if role.as_deref() == Some(&net::Role::Client) => {
            // The server owns the world: ask, and wait for the `Welcome` that answers.
            requests.write(net::TakeOverRequest(train as u16));
        }
        Ok(()) => take_over(sim, &mut drivers, &mut duty, run, &dispatch, train),
        Err(refusal) => {
            let time = sim.time;
            sim.scenario.message(time, i18n::t!(refusal.key()), false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::train::Vehicle;
    use track_model::{EdgeId, TrackPosition};

    /// The player's train, plus a rake of coaches with nothing on the front.
    fn sim() -> Sim {
        let line = content::musterbahn().compile().expect("line compiles");
        let mut sim = Sim::new(line.net, line.interlock, 1);
        crate::spawn_train(
            &mut sim,
            TrackPosition::new(EdgeId(0), 200.0, 1),
            2,
            content::vehicles::br101(),
        );
        let at = TrackPosition::new(EdgeId(2), 500.0, 1);
        let coaches = (0..3)
            .map(|_| Vehicle::new(content::vehicles::passenger_coach(), at))
            .collect();
        sim.add_train(sim_core::train::Train::assemble(coaches, at, &sim.net));
        sim
    }

    fn aboard() -> Walker {
        Walker {
            place: Some(Place::Aboard {
                vehicle: 0,
                eye: Vec3::ZERO,
            }),
            fall: 0.0,
        }
    }

    #[test]
    fn the_levers_are_not_worked_from_the_platform() {
        let sim = sim();
        let outside = Walker {
            place: Some(Place::Outside {
                eye: world_coords::EcefPos::default(),
            }),
            fall: 0.0,
        };
        assert_eq!(
            may_take_over(&sim, 0, &outside, true, false),
            Err(Refusal::NotAboard)
        );
        // In the cab, on a timetable run, it is granted.
        assert_eq!(may_take_over(&sim, 0, &aboard(), true, false), Ok(()));
    }

    #[test]
    fn a_rake_of_coaches_is_driven_by_nobody() {
        let sim = sim();
        assert_eq!(
            may_take_over(&sim, 1, &aboard(), true, false),
            Err(Refusal::NothingToDriveWith)
        );
    }

    #[test]
    fn a_train_out_of_service_is_not_taken_over() {
        let mut sim = sim();
        sim.trains[0].stabled = true;
        assert_eq!(
            may_take_over(&sim, 0, &aboard(), true, false),
            Err(Refusal::OutOfService)
        );
        assert_eq!(
            may_take_over(&sim, 9, &aboard(), true, false),
            Err(Refusal::OutOfService)
        );
    }

    #[test]
    fn a_scenario_keeps_the_player_in_its_own_train() {
        let mut sim = sim();
        sim.scenario.scenario.player_train = 0;
        // Without a timetable run there is one train that is theirs, and it is that one.
        assert_eq!(may_take_over(&sim, 0, &aboard(), false, false), Ok(()));
        assert_eq!(
            may_take_over(&sim, 1, &aboard(), false, false),
            Err(Refusal::ScenarioTrain)
        );
    }

    #[test]
    fn a_train_somebody_else_drives_stays_theirs() {
        let sim = sim();
        assert_eq!(
            may_take_over(&sim, 0, &aboard(), true, true),
            Err(Refusal::AnotherDriver)
        );
    }

    /// The whole thing as it is actually driven: an app with the system in it, a walker
    /// who gets out, and the take-over key.
    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<crate::bindings::Binds>()
            .init_resource::<CameraState>()
            .init_resource::<Dispatch>()
            .insert_resource(Walker {
                place: Some(Place::Aboard {
                    vehicle: 0,
                    eye: Vec3::ZERO,
                }),
                fall: 0.0,
            })
            .insert_resource(PlayerTrain(0))
            .insert_resource(Duty(Some(0)))
            .insert_resource(AiDrivers(Vec::new()))
            .insert_resource(SimResource(sim()))
            .add_message::<net::TakeOverRequest>()
            .add_systems(Update, crew_change);
        app.update();
        app
    }

    fn press(app: &mut App, code: KeyCode) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(code);
        app.update();
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset(code);
        keys.clear();
    }

    #[test]
    fn getting_out_hands_the_train_over() {
        let mut app = app();
        assert_eq!(app.world().resource::<Duty>().0, Some(0));
        // Off the train and onto the platform.
        app.world_mut().resource_mut::<Walker>().place = Some(Place::Outside {
            eye: world_coords::EcefPos::default(),
        });
        app.update();
        assert_eq!(
            app.world().resource::<Duty>().0,
            None,
            "the levers are not worked from the platform"
        );
        // Nothing came to take it on — this train has no working — so it stands braked.
        let sim = &app.world().resource::<SimResource>().0;
        assert!(matches!(
            sim.controls[0].brake_valve,
            sim_core::brakes::DriverBrakeValve::Service(_)
        ));
        // And the player is told what happened.
        assert!(!sim.scenario.messages.is_empty());
    }

    #[test]
    fn walking_into_another_train_hands_the_first_one_back() {
        let mut app = app();
        // `walk_player` boards the other train by moving `PlayerTrain`; the duty does not
        // follow it, because riding in a train is not driving it.
        app.world_mut().resource_mut::<PlayerTrain>().0 = 1;
        app.update();
        assert_eq!(app.world().resource::<Duty>().0, None);
    }

    #[test]
    fn the_key_takes_a_train_over_and_gives_it_back() {
        let mut app = app();
        // Hand it over first, so there is something to take.
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.world().resource::<Duty>().0, None);
        // And take it again.
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.world().resource::<Duty>().0, Some(0));
    }

    #[test]
    fn a_refused_take_over_says_why_and_changes_nothing() {
        let mut app = app();
        app.world_mut().resource_mut::<Duty>().0 = None;
        // Another train, boarded — and no operating day under way, so it is not his to
        // take: without a plan there is exactly one train that is.
        app.world_mut().resource_mut::<PlayerTrain>().0 = 1;
        app.update();
        let before = app
            .world()
            .resource::<SimResource>()
            .0
            .scenario
            .messages
            .len();
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.world().resource::<Duty>().0, None, "still nobody's");
        let sim = &app.world().resource::<SimResource>().0;
        assert_eq!(sim.scenario.messages.len(), before + 1, "and it said why");
        assert_eq!(
            sim.scenario.messages.last().map(|m| m.text.as_str()),
            Some(i18n::t!(Refusal::ScenarioTrain.key()).as_str())
        );
    }

    #[test]
    fn a_train_with_no_working_is_left_braked_rather_than_driven_away() {
        let mut sim = sim();
        let mut drivers = AiDrivers(Vec::new());
        hand_over(&mut sim, &mut drivers, None, &Dispatch::default(), 0);
        assert!(drivers.0.is_empty(), "nobody was sent to drive it");
        assert!(matches!(
            sim.controls[0].brake_valve,
            sim_core::brakes::DriverBrakeValve::Service(_)
        ));
        assert_eq!(sim.controls[0].throttle, 0.0);
    }
}
