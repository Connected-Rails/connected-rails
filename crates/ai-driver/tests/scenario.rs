//! Acceptance: playable scenario with scoring (plan ch. 11.4, M7 criterion).

use ai_driver::AiDriver;
use content::vehicles::{br101, passenger_coach};
use content::{musterbahn, re_4711, to_musterstadt};
use sim_core::Sim;
use sim_core::scenario::{Action, Event, Scenario, Trigger};
use sim_core::train::{RailCondition, Train, Vehicle};
use track_model::{EdgeId, TrackPosition};

fn scenario_sim(start: TrackPosition) -> (Sim, usize) {
    let line = musterbahn().compile().unwrap();
    let mut sim = Sim::new(line.net, line.interlock, 99);
    let mut vehicles = vec![Vehicle::new(br101(), start)];
    for _ in 0..4 {
        vehicles.push(Vehicle::new(passenger_coach(), start));
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
    sim.set_scenario(to_musterstadt(), re_4711());
    (sim, t)
}

#[test]
fn events_fire_one_after_another() {
    let (mut sim, t) = scenario_sim(TrackPosition::new(EdgeId(0), 100.0, 1));
    let mut ai = AiDriver::new(re_4711());

    // Nothing has happened before the departure.
    sim.step(Sim::DT);
    assert!(sim.scenario.fired_at("abfahrt").is_none());

    for _ in 0..2_000 {
        sim.step(Sim::DT);
    }
    assert!(sim.scenario.fired_at("abfahrt").is_some(), "time trigger");
    assert_eq!(sim.scenario.messages.len(), 1);
    assert!(sim.scenario.messages[0].announcement);

    // Drive past km 1.2 → position trigger, then the chained rain event.
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
        .expect("position trigger");
    let rain = sim
        .scenario
        .fired_at("regen")
        .expect("chained event trigger");
    assert!(
        (rain - block - 30.0).abs() < 0.5,
        "delay observed: {block} → {rain}"
    );
    assert_eq!(sim.trains[t].rail, RailCondition::Wet, "weather applied");
}

#[test]
fn scenario_finishes_successfully_and_is_scored() {
    // The report reads back in the display language.
    i18n::set_language("en");
    // Start shortly before the destination so the test runs in seconds.
    let (mut sim, t) = scenario_sim(TrackPosition::new(EdgeId(2), 1000.0, 1));
    let mut ai = AiDriver::new(re_4711());

    for _ in 0..120_000 {
        ai.drive(&mut sim, t, Sim::DT);
        sim.step(Sim::DT);
        if sim.scenario.is_finished() {
            break;
        }
    }

    let outcome = sim.scenario.outcome.clone().expect("scenario finished");
    assert!(outcome.success, "outcome: {}", outcome.reason);

    let report = sim.score.report(sim.scenario.bonus);
    assert_eq!(sim.score.stops.len(), 1, "stop was scored");
    let stop = &sim.score.stops[0];
    assert!(
        stop.position_error.abs() < 60.0,
        "stopping position error {:.1} m",
        stop.position_error
    );
    assert_eq!(sim.score.forced_brakes, 0, "no forced braking");
    assert!(sim.score.energy_kwh > 0.0, "energy use recorded");
    assert!(
        report.total > 0 && report.total <= report.base,
        "score {} implausible",
        report.total
    );
    assert!(
        report.summary().contains("Score:"),
        "summary: {}",
        report.summary()
    );
}

#[test]
fn forced_braking_costs_points() {
    // The report reads back in the display language.
    i18n::set_language("en");
    let (mut sim, t) = scenario_sim(TrackPosition::new(EdgeId(0), 100.0, 1));
    // Never operate the Sifa → forced braking after 35 s.
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
            .any(|i| i.reason.contains("forced brake application")),
        "deduction missing: {report:?}"
    );
    assert!(report.total < report.base);
    // The scenario reports the forced braking as well.
    assert!(sim.scenario.fired_at("zwangsbremsung").is_some());
}

#[test]
fn exceeding_the_maximum_speed_is_counted() {
    // The report reads back in the display language.
    i18n::set_language("en");
    let (mut sim, _t) = scenario_sim(TrackPosition::new(EdgeId(1), 100.0, 1));
    // Section 1 permits 130 km/h — we set 170 km/h.
    for v in &mut sim.trains[0].vehicles {
        v.v = 170.0 / 3.6;
    }
    for _ in 0..400 {
        sim.step(Sim::DT);
    }
    assert!(sim.score.overspeed_seconds > 1.0);
    assert!(sim.score.max_overspeed > 30.0);
    let report = sim.score.report(0);
    assert!(report.items.iter().any(|i| i.reason.contains("too fast")));
}

#[test]
fn custom_scenarios_run_from_ron() {
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
                module: None,
            ),
            (
                name: "ende",
                trigger: After(event: "start", delay: 2.0),
                actions: [Finish(success: true, reason: "fertig")],
                once: true,
                module: None,
            ),
        ],
    )"#;
    let scenario = Scenario::from_ron(text).expect("RON readable");
    assert_eq!(scenario.events.len(), 2);

    let (mut sim, _t) = scenario_sim(TrackPosition::new(EdgeId(0), 100.0, 1));
    sim.set_scenario(scenario, re_4711());
    for _ in 0..1000 {
        sim.step(Sim::DT);
    }
    assert!(sim.scenario.is_finished());
    assert_eq!(sim.scenario.bonus, 10);
    assert_eq!(sim.scenario.messages.len(), 2, "message + score notice");
}

#[test]
fn switch_and_route_actions_are_wired() {
    // Actions without matching infrastructure must not crash.
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
                module: None,
            }],
            script: None,
            timetable: None,
            line: None,
            module: None,
        },
        re_4711(),
    );
    for _ in 0..10 {
        sim.step(Sim::DT);
    }
    assert!(sim.scenario.fired_at("stellen").is_some());
}
