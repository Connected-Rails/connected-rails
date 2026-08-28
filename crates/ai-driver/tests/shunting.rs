//! Acceptance: the AI driver works a shunt job — draws up, sets back onto a rake,
//! couples, uncouples and finishes at a stand (plan ch. 11).
//!
//! Nothing here reaches into the world: the driver writes cab inputs, the simulation does
//! the rest, and the coupling gear is worked through `CabInputs::shunt` like a button on
//! the desk.

use ai_driver::{AiDriver, DriverState, ShuntJob, ShuntMove, ShuntTarget};
use content::musterbahn;
use content::vehicles::{br101, freight_wagon};
use sim_core::Sim;
use sim_core::shunt::SHUNTING_SPEED_KMH;
use sim_core::train::{Train, Vehicle};
use track_model::{EdgeId, TrackPosition};

/// The loco alone at `s`, and a rake of three wagons standing behind it at `rake`.
fn depot() -> (Sim, usize, usize) {
    let line = musterbahn().compile().expect("line compiles");
    let mut sim = Sim::new(line.net, line.interlock, 11);

    let head = TrackPosition::new(EdgeId(0), 500.0, 1);
    let loco = Train::assemble(vec![Vehicle::new(br101(), head)], head, &sim.net);
    let a = sim.add_train(loco);

    let rake_head = TrackPosition::new(EdgeId(0), 460.0, 1);
    let wagons: Vec<Vehicle> = (0..3)
        .map(|_| Vehicle::new(freight_wagon(), rake_head))
        .collect();
    let rake = Train::assemble(wagons, rake_head, &sim.net);
    let b = sim.add_train(rake);

    // Ready for service, and the brake charged so the loco can move at all.
    for train in [a, b] {
        for v in &mut sim.trains[train].vehicles {
            v.traction.battery = true;
            v.traction.pantograph_command = true;
            v.traction.main_switch_command = true;
            v.brake.pipe = 5.0;
            v.brake.aux_reservoir = 5.0;
            v.brake.main_reservoir = 9.0;
        }
    }
    for _ in 0..2_000 {
        sim.step(Sim::DT);
    }
    (sim, a, b)
}

fn at(s: f64) -> ShuntTarget {
    ShuntTarget::At {
        edge: EdgeId(0),
        s,
        module: None,
    }
}

/// The whole job: set back onto the rake, couple, draw it forward, leave it and stand.
#[test]
fn the_ai_sets_back_onto_a_rake_couples_draws_it_up_and_leaves_it_again() {
    let (mut sim, loco, rake) = depot();
    let job = ShuntJob {
        name: "Rangierfahrt Musterstadt".into(),
        moves: vec![
            ShuntMove::SetBack(at(460.0)),
            ShuntMove::Couple,
            ShuntMove::DrawUp(at(900.0)),
            ShuntMove::Uncouple(0),
            ShuntMove::Stand,
        ],
    };
    let mut driver = AiDriver::shunting(job);

    let mut top_speed: f64 = 0.0;
    let mut coupled_at = None;
    for step in 0..200_000 {
        driver.drive(&mut sim, loco, Sim::DT);
        sim.step(Sim::DT);
        top_speed = top_speed.max(sim.trains[loco].speed_kmh().abs());
        if coupled_at.is_none() && sim.trains[loco].vehicles.len() == 4 {
            coupled_at = Some(step);
        }
        if driver.shunt.as_ref().is_some_and(|s| !s.active()) {
            break;
        }
    }

    let shunt = driver.shunt.as_ref().expect("the job is the driver's");
    assert!(!shunt.active(), "the job was worked to the end");
    assert_eq!(driver.state, DriverState::Shunting);
    assert!(coupled_at.is_some(), "the rake was picked up");

    // The loco left the rake behind again: one vehicle in its own consist, three in the
    // part it uncoupled, and the slot the rake started in is the empty one it was coupled
    // away into.
    assert_eq!(sim.trains[loco].vehicles.len(), 1);
    assert!(sim.trains[rake].vehicles.is_empty());
    assert!(sim.trains[rake].stabled);
    let left_behind = sim
        .trains
        .iter()
        .position(|t| t.vehicles.len() == 3)
        .expect("the rake stands somewhere");
    assert_ne!(left_behind, loco);
    // Nothing lost a slot: three trains for two starting consists and one split.
    assert_eq!(sim.trains.len(), 3);
    assert_eq!(sim.runtime.len(), 3);
    assert_eq!(sim.controls.len(), 3);

    // It was drawn up the line, not left where it was picked up.
    let head = sim.trains[loco].head().expect("a head");
    assert!(head.s > 850.0, "drew up to the mark, at {}", head.s);

    // And all of it at shunting speed.
    assert!(
        top_speed <= SHUNTING_SPEED_KMH + 1.0,
        "shunting speed exceeded: {top_speed:.1} km/h"
    );
}

/// A job whose target the line does not have stops the train instead of running it on to
/// nowhere.
#[test]
fn a_job_with_a_target_the_line_does_not_have_stands_still() {
    let (mut sim, loco, _) = depot();
    let job = ShuntJob {
        name: "kaputt".into(),
        moves: vec![ShuntMove::SetBack(ShuntTarget::Yard(
            "gibt es nicht".into(),
        ))],
    };
    let mut driver = AiDriver::shunting(job);
    let start = sim.trains[loco].head().expect("a head").s;
    for _ in 0..20_000 {
        driver.drive(&mut sim, loco, Sim::DT);
        sim.step(Sim::DT);
    }
    assert!(
        (sim.trains[loco].head().expect("a head").s - start).abs() < 1.0,
        "the train stayed where it was"
    );
    assert!(!driver.shunt.as_ref().expect("job").active());
}

/// A driver with a timetable takes its shunt job up only once the last stop is made — the
/// job is what happens *after* the working, not instead of it.
#[test]
fn a_shunt_job_waits_for_the_timetable_to_finish() {
    let (mut sim, loco, _) = depot();
    let timetable = ai_driver::Timetable {
        number: "Lz 1".into(),
        stops: vec![ai_driver::ScheduledStop {
            name: "Musterstadt".into(),
            edge: EdgeId(0),
            s: 2_000.0,
            arrival: 60.0,
            departure: 90.0,
            platform: String::new(),
            module: None,
        }],
        ..ai_driver::Timetable::default()
    };
    let job = ShuntJob {
        name: "abstellen".into(),
        moves: vec![ShuntMove::Stand],
    };
    let mut driver = AiDriver::new(timetable).with_shunt(job);
    // While the timetable still has a stop the driver runs to it, not to the job.
    for _ in 0..2_000 {
        driver.drive(&mut sim, loco, Sim::DT);
        sim.step(Sim::DT);
    }
    assert_ne!(driver.state, DriverState::Shunting);
    assert!(driver.shunt.as_ref().expect("job").active());
    assert!(
        sim.trains[loco].speed_kmh() > 1.0,
        "it is running to its stop"
    );

    // Once the working is over, the job is taken up.
    driver.state = DriverState::Finished;
    driver.drive(&mut sim, loco, Sim::DT);
    assert_eq!(driver.state, DriverState::Shunting);
}

/// Rangierfahrt and Zugfahrt are two different movements, and a train changes from one to
/// the other by passing a signal (Ril 301): out of the siding past Sh 1 it is a shunting
/// movement, and past the starting signal at Hp 1 it is a train again.
///
/// The whole point of the distinction is what it does to the driving: as a shunt it is held
/// to 25 km/h and stopped by a main signal that has nothing to say to it; as a train it
/// takes the line speed.
#[test]
fn a_shunting_movement_becomes_a_train_movement_by_passing_the_signal() {
    use sim_core::interlock::{Route, RouteId, Signal, SignalId, SignalKind};
    use sim_core::shunt::Movement;
    use track_model::{DeviceId, DeviceKind, Facing, TracksideDevice};

    let line = musterbahn().compile().expect("line compiles");
    let mut sim = Sim::new(line.net, line.interlock, 12);

    // Two signals on the first edge: a Sperrsignal at 600 m and a main signal at 900 m.
    let sperr_device = sim.net.add_device(TracksideDevice {
        id: DeviceId(0),
        kind: DeviceKind::Signal,
        edge: EdgeId(0),
        s: 600.0,
        facing: Facing::Forward,
        lateral_offset: 3.0,
        payload: "()".into(),
    });
    let main_device = sim.net.add_device(TracksideDevice {
        id: DeviceId(0),
        kind: DeviceKind::Signal,
        edge: EdgeId(0),
        s: 900.0,
        facing: Facing::Forward,
        lateral_offset: 3.0,
        payload: "()".into(),
    });
    let mut sperr = Signal::new(SignalId(0), SignalKind::Shunting, sperr_device);
    sperr.requires_route = true;
    let sperr = sim.interlock.add_signal(sperr);
    let main = sim
        .interlock
        .add_signal(Signal::new(SignalId(0), SignalKind::Main, main_device));
    // A shunting route out of the Sperrsignal, so it shows Sh 1.
    let route = sim
        .interlock
        .add_route(Route::new(RouteId(0), sperr, main).shunting());
    let mut interlock = std::mem::take(&mut sim.interlock);
    assert!(interlock.request_route(route, &mut sim.net));
    sim.interlock = interlock;

    let head = TrackPosition::new(EdgeId(0), 500.0, 1);
    let train = sim.add_train(Train::assemble(
        vec![Vehicle::new(br101(), head)],
        head,
        &sim.net,
    ));
    for v in &mut sim.trains[train].vehicles {
        v.traction.battery = true;
        v.traction.pantograph_command = true;
        v.traction.main_switch_command = true;
        v.brake.pipe = 5.0;
        v.brake.aux_reservoir = 5.0;
        v.brake.main_reservoir = 9.0;
    }
    for _ in 0..2_000 {
        sim.step(Sim::DT);
    }
    assert_eq!(
        sim.trains[train].movement,
        Movement::Train,
        "it has passed nothing yet"
    );

    // Drive it forward past the Sperrsignal showing Sh 1.
    sim.controls[train].reverser = 1;
    sim.controls[train].throttle = 0.3;
    sim.controls[train].brake_valve = sim_core::brakes::DriverBrakeValve::Release;
    let mut passed_shunting = false;
    for _ in 0..40_000 {
        sim.step(Sim::DT);
        if sim.trains[train].movement == Movement::Shunt {
            passed_shunting = true;
            break;
        }
    }
    assert!(passed_shunting, "Sh 1 made it a shunting movement");

    // And on past the main signal, which is showing proceed: it is a train again.
    let mut passed_main = false;
    for _ in 0..40_000 {
        sim.step(Sim::DT);
        if sim.trains[train].movement == Movement::Train {
            passed_main = true;
            break;
        }
    }
    assert!(passed_main, "Hp 1 made it a train movement again");
    assert!(
        sim.interlock.signal(main).aspect.main == Some(sim_core::interlock::MainAspect::Proceed)
    );
}
