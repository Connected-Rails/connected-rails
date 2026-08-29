//! The Sifa and the train protection reach the sound table on quantities of their own —
//! the regression for the split of `Alert` into `VigilanceAlert` and `ProtectionAlert`,
//! driven headless over the example line's magnets.
//!
//! The sounds these quantities carry are conditions, not events: the horn sounds **from**
//! an influence **until** the driver acknowledges it. Pressing the acknowledgement button
//! with nothing pending is silent, which is what the tests below pin down.

use content::LineSource;
use content::vehicles::{br101, passenger_coach};
use sim_core::Sim;
use sim_core::safety::ProtectionAction;
use sim_core::sound::SoundState;
use sim_core::train::{Train, Vehicle};
use track_model::{EdgeId, TrackPosition};

/// The line the simulator runs by default, so what the tests hear is what a run hears: a
/// Ks main signal at km 2.0 guarding the section the train itself stands in and therefore
/// at stop, its distant at km 1.0 showing Ks 2 with the 1000 Hz magnet live, and the
/// 500 Hz magnet at km 1.75 behind it.
const LINE: &str = include_str!("../../../mods/example/lines/beispielstrecke.ron");

/// A BR 101 with a coach, powered up, at the west portal.
fn example_run() -> (Sim, usize) {
    let source: LineSource = ron::from_str(LINE).expect("the example line parses");
    let line = source.compile().expect("the example line compiles");
    let mut sim = Sim::new(line.net, line.interlock, 1234);
    let head = TrackPosition::new(EdgeId(0), 300.0, 1);
    let vehicles = vec![
        Vehicle::new(br101(), head),
        Vehicle::new(passenger_coach(), head),
    ];
    let train = Train::assemble(vehicles, head, &sim.net);
    let t = sim.add_train(train);
    for v in &mut sim.trains[t].vehicles {
        if v.spec.powered() {
            v.traction.battery = true;
            v.traction.pantograph_command = true;
            v.traction.main_switch_command = true;
        }
    }
    for _ in 0..1600 {
        sim.step(Sim::DT);
    }
    (sim, t)
}

fn sound(sim: &Sim, t: usize) -> SoundState {
    SoundState::sample(
        &sim.trains[t].vehicles[0],
        &sim.controls[t],
        &sim.runtime[t].protection,
        None,
        0.0,
    )
}

/// One step with the acknowledgement button down, then released.
fn acknowledge(sim: &mut Sim, t: usize) {
    sim.controls[t].pzb_acknowledge = true;
    sim.step(Sim::DT);
    sim.controls[t].pzb_acknowledge = false;
}

/// Runs at `kmh` until the protection demands something, keeping the Sifa quiet. Returns
/// where that was, or `None` if `limit_s` was reached first.
fn run_to_the_next_demand(sim: &mut Sim, t: usize, kmh: f64, limit_s: f64) -> Option<f64> {
    for step in 0..(200 * 300) {
        // The Sifa wants an operation every half minute; a short press every twenty
        // seconds keeps it quiet, so what is raised is the protection's alone.
        sim.controls[t].sifa = step % 4000 < 100;
        for v in &mut sim.trains[t].vehicles {
            v.v = kmh / 3.6;
        }
        sim.step(Sim::DT);
        assert!(
            !sim.runtime[t].protection.vigilance_alert,
            "the Sifa was operated"
        );
        if sim.runtime[t].protection.protection_alert {
            return Some(sim.trains[t].vehicles[0].pos.s);
        }
        if sim.trains[t].vehicles[0].pos.s > limit_s {
            return None;
        }
    }
    None
}

#[test]
fn the_1000_hz_magnet_sounds_the_horn_until_it_is_acknowledged() {
    let (mut sim, t) = example_run();

    // Nothing is pending at the start: pressing the button is silent.
    acknowledge(&mut sim, t);
    assert_eq!(sound(&sim, t).protection_alert, 0.0);

    let s = run_to_the_next_demand(&mut sim, t, 100.0, 1100.0)
        .expect("the 1000 Hz magnet at the distant signal demands an acknowledgement");
    assert!((990.0..1010.0).contains(&s), "at km 1.0, not at {s:.0} m");

    let state = sound(&sim, t);
    assert_eq!(state.protection_alert, 1.0);
    assert_eq!(state.vigilance_alert, 0.0, "the Sifa is a sound of its own");
    assert_eq!(state.alert, 1.0, "the combined quantity follows both");

    // Acknowledged within the four seconds: the horn stops, and no braking follows.
    acknowledge(&mut sim, t);
    assert_eq!(
        sound(&sim, t).protection_alert,
        0.0,
        "the horn falls silent"
    );
    assert_ne!(
        sim.runtime[t].protection.action,
        ProtectionAction::EmergencyBrake
    );
    // And the supervision it started stays: the horn is over, the curve is not.
    assert!(sim.runtime[t].protection.speed_limit.is_some());
}

#[test]
fn driving_on_after_the_acknowledgement_trips_the_curve_and_sounds_again() {
    let (mut sim, t) = example_run();
    run_to_the_next_demand(&mut sim, t, 100.0, 1100.0).expect("the 1000 Hz influence");
    acknowledge(&mut sim, t);

    // Acknowledging buys the driver the braking curve, not the right to hold his speed:
    // 165 → 85 km/h over 23 s, so 100 km/h is exceeded a good 500 m past the magnet.
    let s = run_to_the_next_demand(&mut sim, t, 100.0, 1740.0)
        .expect("the braking curve trips at an unchanged 100 km/h");
    assert!(
        (1100.0..1740.0).contains(&s),
        "on the curve behind the magnet, not at {s:.0} m"
    );
    assert_eq!(
        sim.runtime[t].protection.action,
        ProtectionAction::EmergencyBrake
    );

    // The forced braking holds the demand until it is acknowledged at a stand — which is
    // what makes the buzzer a loop rather than a one-shot.
    for _ in 0..(200 * 120) {
        sim.step(Sim::DT);
        if sim.trains[t].vehicles[0].v.abs() < 1e-3 {
            break;
        }
    }
    assert_eq!(sound(&sim, t).protection_alert, 1.0, "still sounding");
    acknowledge(&mut sim, t);
    assert_eq!(sound(&sim, t).protection_alert, 0.0, "released at a stand");
}

#[test]
fn an_unattended_sifa_sounds_the_vigilance_device_and_not_the_protection() {
    let (mut sim, t) = example_run();
    let mut raised = None;
    for _ in 0..(200 * 60) {
        for v in &mut sim.trains[t].vehicles {
            v.v = 60.0 / 3.6;
        }
        sim.step(Sim::DT);
        if sim.runtime[t].protection.vigilance_alert {
            raised = Some(sim.trains[t].vehicles[0].pos.s);
            break;
        }
    }
    let s = raised.expect("the Sifa sounds when nobody operates it");
    assert!(s < 1000.0, "before the magnet at km 1.0, not at {s:.0} m");

    let state = sound(&sim, t);
    assert_eq!(state.vigilance_alert, 1.0);
    assert_eq!(state.protection_alert, 0.0);
    assert_eq!(state.alert, 1.0);
}
