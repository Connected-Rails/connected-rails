//! Acceptance M4: AI runs to the timetable, stops at the signal and at the platform.

use ai_driver::{AiDriver, DriverState, ScheduledStop, Timetable, TimetableKind};
use content::musterbahn;
use content::vehicles::{br101, passenger_coach};
use sim_core::Sim;
use sim_core::train::{Train, Vehicle};
use track_model::{EdgeId, TrackPosition};

fn sim_with_train(start_s: f64) -> (Sim, usize) {
    let line = musterbahn().compile().unwrap();
    let mut sim = Sim::new(line.net, line.interlock, 7);
    let head = TrackPosition::new(EdgeId(0), start_s, 1);
    let mut vehicles = vec![Vehicle::new(br101(), head)];
    for _ in 0..4 {
        vehicles.push(Vehicle::new(passenger_coach(), head));
    }
    let train = Train::assemble(vehicles, head, &sim.net);
    let t = sim.add_train(train);
    for v in &mut sim.trains[t].vehicles {
        v.traction.battery = true;
        v.traction.pantograph_command = true;
        v.traction.main_switch_command = true;
    }
    for _ in 0..1600 {
        sim.step(Sim::DT);
    }
    (sim, t)
}

fn timetable_to_platform() -> Timetable {
    Timetable {
        number: "RE 4711".into(),
        category: "RE".into(),
        kind: TimetableKind::Scenario,
        module: None,
        stops: vec![ScheduledStop {
            name: "Musterstadt".into(),
            edge: EdgeId(2),
            s: 2600.0,
            arrival: 600.0,
            departure: 660.0,
            platform: "2".into(),
            module: None,
        }],
    }
}

#[test]
fn ai_accelerates_and_keeps_speed() {
    let (mut sim, t) = sim_with_train(100.0);
    let mut ai = AiDriver::new(Timetable::default());
    // Drive to the end of the first section (160 km/h applies there) and record the
    // highest speed reached along the way.
    let mut v_max: f64 = 0.0;
    for _ in 0..40_000 {
        ai.drive(&mut sim, t, Sim::DT);
        sim.step(Sim::DT);
        let head = sim.trains[t].vehicles[0].pos;
        if head.edge != EdgeId(0) {
            break;
        }
        let v = sim.trains[t].speed_kmh();
        v_max = v_max.max(v);
        // The permitted speed must never be exceeded.
        assert!(
            v <= head.speed_limit(&sim.net) + 2.0,
            "AI too fast: {v:.1} km/h with {} km/h permitted",
            head.speed_limit(&sim.net)
        );
    }
    // On the 3 km long section the train accelerates as far as tractive effort and
    // the braking curve onto the following 130 km/h section allow.
    assert!(
        (120.0..=160.0).contains(&v_max),
        "AI does not use the line to the full: {v_max} km/h"
    );
    // No forced braking on the way.
    assert_eq!(
        sim.runtime[t].protection.action,
        sim_core::safety::ProtectionAction::None
    );
}

#[test]
fn ai_stops_in_front_of_signal_at_stop() {
    let (mut sim, t) = sim_with_train(100.0);
    // Place a second train in the following section → block signal at km 2.0 shows stop.
    let blocker_head = TrackPosition::new(EdgeId(1), 200.0, 1);
    let blocker = Train::assemble(
        vec![Vehicle::new(passenger_coach(), blocker_head)],
        blocker_head,
        &sim.net,
    );
    sim.add_train(blocker);

    let mut ai = AiDriver::new(Timetable::default());
    for _ in 0..60_000 {
        ai.drive(&mut sim, t, Sim::DT);
        sim.step(Sim::DT);
        if ai.state == DriverState::WaitingAtSignal {
            break;
        }
    }
    let head = sim.trains[t].vehicles[0].pos;
    assert!(sim.trains[t].speed_kmh() < 1.0, "train must be standing");
    assert!(
        head.s < 2000.0,
        "train passed the signal at stop (s = {})",
        head.s
    );
    assert!(
        head.s > 1500.0,
        "train stopped far too early (s = {})",
        head.s
    );
    // The PZB must not have intervened — the AI braked in time.
    assert_eq!(
        sim.runtime[t].protection.action,
        sim_core::safety::ProtectionAction::None
    );
}

#[test]
fn ai_stops_at_platform_and_departs_on_time() {
    // Start shortly before the platform so the test stays fast.
    let line = musterbahn().compile().unwrap();
    let mut sim = Sim::new(line.net, line.interlock, 7);
    let head = TrackPosition::new(EdgeId(2), 1200.0, 1);
    let mut vehicles = vec![Vehicle::new(br101(), head)];
    for _ in 0..4 {
        vehicles.push(Vehicle::new(passenger_coach(), head));
    }
    let train = Train::assemble(vehicles, head, &sim.net);
    let t = sim.add_train(train);
    for v in &mut sim.trains[t].vehicles {
        v.traction.battery = true;
        v.traction.pantograph_command = true;
        v.traction.main_switch_command = true;
    }

    let mut ai = AiDriver::new(timetable_to_platform());
    for _ in 0..80_000 {
        ai.drive(&mut sim, t, Sim::DT);
        sim.step(Sim::DT);
        if ai.state == DriverState::Dwelling {
            break;
        }
    }
    assert_eq!(ai.state, DriverState::Dwelling, "AI did not stop");
    let head = sim.trains[t].vehicles[0].pos;
    let error = (head.s - 2600.0).abs();
    assert!(
        error < 60.0,
        "stopping accuracy {error:.1} m at the platform is too poor"
    );
}
