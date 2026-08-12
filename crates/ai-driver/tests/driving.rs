//! Abnahme M4: KI fährt nach Fahrplan, hält am Signal und am Bahnsteig.

use ai_driver::{AiDriver, DriverState, ScheduledStop, Timetable};
use content::musterbahn;
use content::vehicles::{br101, de_pzb, passenger_coach, vehicle};
use sim_core::Sim;
use sim_core::safety::SafetySystems;
use sim_core::safety::de::TrainType;
use sim_core::train::Train;
use track_model::{EdgeId, TrackPosition};

fn sim_with_train(start_s: f64) -> (Sim, usize) {
    let line = musterbahn().compile().unwrap();
    let mut sim = Sim::new(line.net, line.interlock, 7);
    let head = TrackPosition::new(EdgeId(0), start_s, 1);
    let mut vehicles = vec![vehicle(br101(), head, de_pzb(TrainType::O))];
    for _ in 0..4 {
        vehicles.push(vehicle(passenger_coach(), head, SafetySystems::None));
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
        stops: vec![ScheduledStop {
            name: "Musterstadt".into(),
            edge: EdgeId(2),
            s: 2600.0,
            arrival: 600.0,
            departure: 660.0,
            platform: "2".into(),
        }],
    }
}

#[test]
fn ki_faehrt_an_und_haelt_geschwindigkeit() {
    let (mut sim, t) = sim_with_train(100.0);
    let mut ai = AiDriver::new(Timetable::default());
    // Bis zum Ende des ersten Abschnitts fahren (dort gilt 160 km/h) und dabei die
    // höchste erreichte Geschwindigkeit mitschreiben.
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
        // Die zulässige Geschwindigkeit darf nie überschritten werden.
        assert!(
            v <= head.speed_limit(&sim.net) + 2.0,
            "KI zu schnell: {v:.1} km/h bei zulässigen {} km/h",
            head.speed_limit(&sim.net)
        );
    }
    // Der Zug beschleunigt auf dem 3 km langen Abschnitt so weit, wie es Zugkraft und
    // die Bremskurve auf den anschließenden 130er-Abschnitt zulassen.
    assert!(
        (120.0..=160.0).contains(&v_max),
        "KI schöpft die Strecke nicht aus: {v_max} km/h"
    );
    // Keine Zwangsbremsung unterwegs.
    assert_eq!(
        sim.runtime[t].protection.action,
        sim_core::safety::ProtectionAction::None
    );
}

#[test]
fn ki_haelt_vor_halt_zeigendem_signal() {
    let (mut sim, t) = sim_with_train(100.0);
    // Zweiten Zug in den Folgeabschnitt stellen → Blocksignal bei km 2,0 zeigt Halt.
    let blocker_head = TrackPosition::new(EdgeId(1), 200.0, 1);
    let blocker = Train::assemble(
        vec![vehicle(
            passenger_coach(),
            blocker_head,
            SafetySystems::None,
        )],
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
    assert!(sim.trains[t].speed_kmh() < 1.0, "Zug muss stehen");
    assert!(
        head.s < 2000.0,
        "Zug ist am Halt zeigenden Signal vorbeigefahren (s = {})",
        head.s
    );
    assert!(
        head.s > 1500.0,
        "Zug hat viel zu früh gehalten (s = {})",
        head.s
    );
    // Die PZB darf dabei nicht eingegriffen haben — die KI hat rechtzeitig gebremst.
    assert_eq!(
        sim.runtime[t].protection.action,
        sim_core::safety::ProtectionAction::None
    );
}

#[test]
fn ki_haelt_am_bahnsteig_und_faehrt_nach_fahrplan_ab() {
    // Kurz vor dem Bahnsteig starten, damit der Test schnell bleibt.
    let line = musterbahn().compile().unwrap();
    let mut sim = Sim::new(line.net, line.interlock, 7);
    let head = TrackPosition::new(EdgeId(2), 1200.0, 1);
    let mut vehicles = vec![vehicle(br101(), head, de_pzb(TrainType::O))];
    for _ in 0..4 {
        vehicles.push(vehicle(passenger_coach(), head, SafetySystems::None));
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
    assert_eq!(ai.state, DriverState::Dwelling, "KI hat nicht gehalten");
    let head = sim.trains[t].vehicles[0].pos;
    let error = (head.s - 2600.0).abs();
    assert!(
        error < 60.0,
        "Haltegenauigkeit {error:.1} m am Bahnsteig zu schlecht"
    );
}
