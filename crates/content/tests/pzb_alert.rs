//! The Sifa and the PZB reach the sound table on their own quantities — the regression
//! for the split of `Alert` into `VigilanceAlert` and `ProtectionAlert`, run headless over
//! the Musterbahn's 1000 Hz magnet.

use content::musterbahn;
use content::vehicles::{br101, passenger_coach};
use sim_core::Sim;
use sim_core::sound::SoundState;
use sim_core::train::{Train, Vehicle};
use track_model::{EdgeId, TrackPosition};

/// A BR 101 with a coach, powered up and rolling at 80 km/h from the start of the line.
fn rolling_train(sim: &mut Sim) -> usize {
    let head = TrackPosition::new(EdgeId(0), 100.0, 1);
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
    for v in &mut sim.trains[t].vehicles {
        v.v = 80.0 / 3.6;
    }
    t
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

#[test]
fn a_1000_hz_magnet_raises_the_protection_alert_until_it_is_acknowledged() {
    let line = musterbahn().compile().expect("line compiles");
    let mut sim = Sim::new(line.net, line.interlock, 1234);
    let t = rolling_train(&mut sim);

    let mut raised = None;
    for step in 0..(200 * 120) {
        // The Sifa wants a fresh operation every half minute: half a second every twenty
        // seconds keeps it quiet, so what is raised below is the PZB's alone.
        sim.controls[t].sifa = step % 4000 < 100;
        sim.step(Sim::DT);
        let protection = &sim.runtime[t].protection;
        assert!(!protection.vigilance_alert, "the Sifa was operated");
        if protection.protection_alert {
            raised = Some((sim.runtime[t].odometer, protection.alert));
            break;
        }
    }
    let (odometer, any) = raised.expect("the 1000 Hz magnet demands an acknowledgement");
    assert!(any, "`alert` follows `protection_alert`");
    assert!(
        (850.0..1000.0).contains(&odometer),
        "raised at the magnet at km 1.0, not after {odometer:.0} m"
    );
    let state = sound(&sim, t);
    assert_eq!(state.protection_alert, 1.0);
    assert_eq!(state.vigilance_alert, 0.0);
    assert_eq!(state.alert, 1.0);

    // Acknowledged: the demand is gone with the next step, and so is the sound.
    sim.controls[t].pzb_acknowledge = true;
    sim.step(Sim::DT);
    assert!(!sim.runtime[t].protection.protection_alert);
    assert_eq!(sound(&sim, t).protection_alert, 0.0);
}

#[test]
fn an_unattended_sifa_raises_the_vigilance_alert_and_not_the_protections() {
    let line = musterbahn().compile().expect("line compiles");
    let mut sim = Sim::new(line.net, line.interlock, 1234);
    let t = rolling_train(&mut sim);

    let mut raised = None;
    for _ in 0..(200 * 60) {
        sim.step(Sim::DT);
        if sim.runtime[t].protection.vigilance_alert {
            raised = Some(sim.runtime[t].odometer);
            break;
        }
    }
    let odometer = raised.expect("the Sifa sounds when nobody operates it");
    assert!(odometer < 850.0, "before the magnet, not after {odometer:.0} m");
    let state = sound(&sim, t);
    assert_eq!(state.vigilance_alert, 1.0);
    assert_eq!(state.protection_alert, 0.0);
    assert_eq!(state.alert, 1.0);
}
