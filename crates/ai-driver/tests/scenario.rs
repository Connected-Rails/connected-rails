//! Abnahme: durchspielbares Szenario mit Wertung (Plan Kap. 11.4, M7-Kriterium).

use ai_driver::AiDriver;
use content::vehicles::{br101, de_pzb_lzb, passenger_coach, vehicle};
use content::{musterbahn, nach_musterstadt, re_4711};
use sim_core::Sim;
use sim_core::safety::SafetySystems;
use sim_core::safety::de::TrainType;
use sim_core::scenario::{Action, Event, Scenario, Trigger};
use sim_core::train::{RailCondition, Train};
use track_model::{EdgeId, TrackPosition};

fn scenario_sim(start: TrackPosition) -> (Sim, usize) {
    let line = musterbahn().compile().unwrap();
    let mut sim = Sim::new(line.net, line.interlock, 99);
    let mut vehicles = vec![vehicle(br101(), start, de_pzb_lzb(TrainType::O))];
    for _ in 0..4 {
        vehicles.push(vehicle(passenger_coach(), start, SafetySystems::None));
    }
    let train = Train::assemble(vehicles, start, &sim.net);
    let t = sim.add_train(train);
    for v in &mut sim.trains[t].vehicles {
        if v.is_powered() {
            v.traction.battery = true;
            v.traction.pantograph_command = true;
            v.traction.main_switch_command = true;
            v.traction.pantograph = 1.0;
        }
    }
    sim.set_scenario(nach_musterstadt(), re_4711());
    (sim, t)
}

#[test]
fn ereignisse_loesen_der_reihe_nach_aus() {
    let (mut sim, t) = scenario_sim(TrackPosition::new(EdgeId(0), 100.0, 1));
    let mut ai = AiDriver::new(re_4711());

    // Vor der Abfahrt ist noch nichts passiert.
    sim.step(Sim::DT);
    assert!(sim.scenario.fired_at("abfahrt").is_none());

    for _ in 0..2_000 {
        sim.step(Sim::DT);
    }
    assert!(sim.scenario.fired_at("abfahrt").is_some(), "Zeitauslöser");
    assert_eq!(sim.scenario.messages.len(), 1);
    assert!(sim.scenario.messages[0].announcement);

    // Fahren bis hinter km 1,2 → Positionsauslöser, danach der verkettete Regen.
    for _ in 0..60_000 {
        ai.drive(&mut sim, t, Sim::DT);
        sim.step(Sim::DT);
        if sim.scenario.fired_at("regen").is_some() {
            break;
        }
    }
    let block = sim
        .scenario
        .fired_at("block_frei")
        .expect("Positionsauslöser");
    let regen = sim
        .scenario
        .fired_at("regen")
        .expect("verketteter Auslöser");
    assert!(
        (regen - block - 30.0).abs() < 0.5,
        "Verzögerung eingehalten: {block} → {regen}"
    );
    assert_eq!(sim.trains[t].rail, RailCondition::Wet, "Wetteraktion wirkt");
}

#[test]
fn szenario_wird_erfolgreich_beendet_und_bewertet() {
    // Kurz vor dem Ziel starten, damit der Test in Sekunden läuft.
    let (mut sim, t) = scenario_sim(TrackPosition::new(EdgeId(2), 1000.0, 1));
    let mut ai = AiDriver::new(re_4711());

    for _ in 0..120_000 {
        ai.drive(&mut sim, t, Sim::DT);
        sim.step(Sim::DT);
        if sim.scenario.is_finished() {
            break;
        }
    }

    let outcome = sim.scenario.outcome.clone().expect("Szenario beendet");
    assert!(outcome.success, "Ausgang: {}", outcome.reason);

    let report = sim.score.report(sim.scenario.bonus);
    assert_eq!(sim.score.stops.len(), 1, "Halt wurde gewertet");
    let stop = &sim.score.stops[0];
    assert!(
        stop.position_error.abs() < 60.0,
        "Halteplatzabweichung {:.1} m",
        stop.position_error
    );
    assert_eq!(sim.score.forced_brakes, 0, "keine Zwangsbremsung");
    assert!(sim.score.energy_kwh > 0.0, "Energieverbrauch erfasst");
    assert!(
        report.total > 0 && report.total <= report.base,
        "Punktzahl {} unplausibel",
        report.total
    );
    assert!(
        report.summary().contains("Wertung:"),
        "Zusammenfassung: {}",
        report.summary()
    );
}

#[test]
fn zwangsbremsung_kostet_punkte() {
    let (mut sim, t) = scenario_sim(TrackPosition::new(EdgeId(0), 100.0, 1));
    // Sifa nie bedienen → nach 35 s Zwangsbremsung.
    sim.controls[t].reverser = 1;
    sim.controls[t].throttle = 0.5;
    for _ in 0..12_000 {
        sim.step(Sim::DT);
        if sim.score.forced_brakes > 0 {
            break;
        }
    }
    assert_eq!(sim.score.forced_brakes, 1);
    let report = sim.score.report(sim.scenario.bonus);
    assert!(
        report
            .items
            .iter()
            .any(|i| i.reason.contains("Zwangsbremsung")),
        "Abzug fehlt: {report:?}"
    );
    assert!(report.total < report.base);
    // Das Szenario meldet die Zwangsbremsung ebenfalls.
    assert!(sim.scenario.fired_at("zwangsbremsung").is_some());
}

#[test]
fn ueberschreitung_der_hoechstgeschwindigkeit_wird_gezaehlt() {
    let (mut sim, _t) = scenario_sim(TrackPosition::new(EdgeId(1), 100.0, 1));
    // Abschnitt 1 erlaubt 130 km/h — wir setzen 170 km/h.
    for v in &mut sim.trains[0].vehicles {
        v.v = 170.0 / 3.6;
    }
    for _ in 0..400 {
        sim.step(Sim::DT);
    }
    assert!(sim.score.overspeed_seconds > 1.0);
    assert!(sim.score.max_overspeed > 30.0);
    let report = sim.score.report(0);
    assert!(report.items.iter().any(|i| i.reason.contains("zu schnell")));
}

#[test]
fn eigene_szenarien_laufen_aus_ron() {
    let text = r#"(
        name: "Testfahrt",
        description: "Selbst geschriebenes Szenario",
        player_train: 0,
        events: [
            (
                name: "start",
                trigger: Time(1.0),
                actions: [Message("Los geht's"), Score(points: 10, reason: "Pünktlich losgefahren")],
                once: true,
            ),
            (
                name: "ende",
                trigger: After(event: "start", delay: 2.0),
                actions: [Finish(success: true, reason: "fertig")],
                once: true,
            ),
        ],
    )"#;
    let scenario = Scenario::from_ron(text).expect("RON lesbar");
    assert_eq!(scenario.events.len(), 2);

    let (mut sim, _t) = scenario_sim(TrackPosition::new(EdgeId(0), 100.0, 1));
    sim.set_scenario(scenario, re_4711());
    for _ in 0..1000 {
        sim.step(Sim::DT);
    }
    assert!(sim.scenario.is_finished());
    assert_eq!(sim.scenario.bonus, 10);
    assert_eq!(sim.scenario.messages.len(), 2, "Meldung + Wertungshinweis");
}

#[test]
fn weichen_und_fahrstrassenaktionen_sind_verdrahtet() {
    // Aktionen ohne passende Infrastruktur dürfen nicht abstürzen.
    let (mut sim, _t) = scenario_sim(TrackPosition::new(EdgeId(0), 100.0, 1));
    sim.set_scenario(
        Scenario {
            name: "Stellwerkstest".into(),
            description: String::new(),
            player_train: 0,
            events: vec![Event {
                name: "stellen".into(),
                trigger: Trigger::Time(0.0),
                actions: vec![
                    Action::SetSwitch {
                        node: track_model::NodeId(0),
                        position: track_model::SwitchPosition::Diverging,
                    },
                    Action::RequestRoute(sim_core::interlock::RouteId(0)),
                    Action::ReleaseRoute(sim_core::interlock::RouteId(0)),
                ],
                once: true,
            }],
        },
        re_4711(),
    );
    for _ in 0..10 {
        sim.step(Sim::DT);
    }
    assert!(sim.scenario.fired_at("stellen").is_some());
}
