//! Acceptance tests of the driving dynamics and the brake (plan ch. 6, 7, 18) — headless.

use content::musterbahn;
use content::vehicles::{br101, de_pzb_lzb, freight_wagon, passenger_coach, vehicle};
use sim_core::Sim;
use sim_core::brakes::DriverBrakeValve;
use sim_core::safety::SafetySystems;
use sim_core::safety::de::TrainType;
use sim_core::train::{Train, Vehicle};
use track_model::{EdgeId, TrackPosition};

/// Builds a train of BR 101 + n passenger coaches at the start of the line.
fn passenger_train(sim: &mut Sim, coaches: usize) -> usize {
    let head = TrackPosition::new(EdgeId(0), 100.0, 1);
    let mut vehicles = vec![vehicle(br101(), head, de_pzb_lzb(TrainType::O))];
    for _ in 0..coaches {
        vehicles.push(vehicle(passenger_coach(), head, SafetySystems::None));
    }
    let train = Train::assemble(vehicles, head, &sim.net);
    sim.add_train(train)
}

fn new_sim() -> Sim {
    let line = musterbahn().compile().expect("line compiles");
    Sim::new(line.net, line.interlock, 1234)
}

/// Make the vehicles ready for service (battery, pantograph, main switch).
fn power_up(sim: &mut Sim, train: usize) {
    for v in &mut sim.trains[train].vehicles {
        if v.spec.traction.is_some() {
            v.traction.battery = true;
            v.traction.pantograph_command = true;
            v.traction.main_switch_command = true;
        }
    }
    // The pantograph needs ~5 s.
    for _ in 0..1600 {
        sim.step(Sim::DT);
    }
}

fn set_speed(sim: &mut Sim, train: usize, kmh: f64) {
    for v in &mut sim.trains[train].vehicles {
        v.v = kmh / 3.6;
    }
}

/// Keep the Sifa quiet (otherwise it intervenes after 35 s).
fn hold_sifa(sim: &mut Sim, train: usize, pressed: bool) {
    sim.controls[train].sifa = pressed;
}

#[test]
fn coasting_test_follows_the_davis_curve() {
    let mut sim = new_sim();
    let t = passenger_train(&mut sim, 5);
    power_up(&mut sim, t);
    set_speed(&mut sim, t, 120.0);
    sim.controls[t].brake_valve = DriverBrakeValve::Release;

    // Coast for 60 s; operate the Sifa alternately.
    let mut v_last = sim.trains[t].speed_kmh();
    for i in 0..12_000 {
        hold_sifa(&mut sim, t, (i / 200) % 2 == 0);
        sim.step(Sim::DT);
    }
    let v_end = sim.trains[t].speed_kmh();
    assert!(v_end < v_last, "train must coast down");

    // Target deceleration from Davis: a = R(v)/m_inertial, on a straight line.
    let train = &sim.trains[t];
    // Target value at the mean speed of the coasting run.
    let v_mean = (120.0 + v_end) / 2.0 / 3.6;
    let r: f64 = train
        .vehicles
        .iter()
        .map(|v| v.spec.davis.resistance(v_mean))
        .sum();
    let m: f64 = train.vehicles.iter().map(|v| v.inertial_mass()).sum();
    let a_target = r / m;
    let a_actual = (120.0 - v_end) / 3.6 / 60.0;
    assert!(
        (a_actual - a_target).abs() / a_target < 0.15,
        "coasting deceleration {a_actual:.4} vs Davis {a_target:.4} m/s²"
    );
    v_last = v_end;
    assert!(v_last > 90.0, "coast-down far too strong: {v_last} km/h");
}

#[test]
fn emergency_braking_from_100_kmh_matches_the_brake_table() {
    let mut sim = new_sim();
    let t = passenger_train(&mut sim, 5);
    power_up(&mut sim, t);
    set_speed(&mut sim, t, 100.0);

    let brh = sim.trains[t].brake_percentage();
    assert!(
        (100.0..=160.0).contains(&brh),
        "brake percentage implausible: {brh}"
    );

    let start = sim.runtime[t].odometer;
    sim.controls[t].brake_valve = DriverBrakeValve::Emergency;
    for i in 0..24_000 {
        hold_sifa(&mut sim, t, (i / 200) % 2 == 0);
        sim.step(Sim::DT);
        if sim.trains[t].speed_kmh() < 0.5 {
            break;
        }
    }
    let distance = sim.runtime[t].odometer - start;
    assert!(sim.trains[t].speed_kmh() < 1.0, "train must be at a stand");
    // Brake table: at ~130 brake percent the emergency braking distance from 100 km/h
    // is in the order of 400–500 m. Generous tolerance, but not arbitrary.
    assert!(
        (300.0..=650.0).contains(&distance),
        "emergency braking distance {distance:.0} m outside the expected range"
    );
}

#[test]
fn freight_train_brakes_later_at_the_rear() {
    let mut sim = new_sim();
    let head = TrackPosition::new(EdgeId(0), 100.0, 1);
    let mut vehicles: Vec<Vehicle> = vec![vehicle(br101(), head, SafetySystems::None)];
    for _ in 0..25 {
        vehicles.push(vehicle(freight_wagon(), head, SafetySystems::None));
    }
    let train = Train::assemble(vehicles, head, &sim.net);
    let t = sim.add_train(train);
    set_speed(&mut sim, t, 60.0);

    sim.controls[t].brake_valve = DriverBrakeValve::Emergency;
    // Brake for half a second and watch the pressure wave.
    for _ in 0..100 {
        sim.step(Sim::DT);
    }
    let front = sim.trains[t].vehicles[1].brake.pipe;
    let back = sim.trains[t].vehicles.last().unwrap().brake.pipe;
    assert!(
        back > front + 0.2,
        "rear pressure ({back:.2} bar) must lag behind the front one ({front:.2} bar)"
    );
    // And in the end the whole train brakes — brake position G needs ~ 22 s for that.
    for _ in 0..12_000 {
        sim.step(Sim::DT);
    }
    let back_cyl = sim.trains[t].vehicles.last().unwrap().brake.cylinder;
    assert!(
        back_cyl > 3.0,
        "the last coach must have applied: {back_cyl}"
    );
}

#[test]
fn starting_on_the_gradient_and_adhesion_limit() {
    let mut sim = new_sim();
    // Place it on the 8 ‰ climb in the third section.
    let head = TrackPosition::new(EdgeId(2), 1000.0, 1);
    let mut vehicles = vec![vehicle(br101(), head, SafetySystems::None)];
    for _ in 0..8 {
        vehicles.push(vehicle(passenger_coach(), head, SafetySystems::None));
    }
    let train = Train::assemble(vehicles, head, &sim.net);
    let t = sim.add_train(train);
    power_up(&mut sim, t);

    // Without tractive effort the train rolls backwards.
    sim.controls[t].brake_valve = DriverBrakeValve::Release;
    for _ in 0..2000 {
        sim.step(Sim::DT);
    }
    assert!(
        sim.trains[t].speed() < -0.05,
        "on the gradient without tractive effort the train must roll back: {} m/s",
        sim.trains[t].speed()
    );

    // With full tractive effort it starts.
    sim.controls[t].reverser = 1;
    sim.controls[t].throttle = 1.0;
    for _ in 0..6000 {
        sim.step(Sim::DT);
    }
    assert!(
        sim.trains[t].speed_kmh() > 5.0,
        "starting on the gradient failed: {} km/h",
        sim.trains[t].speed_kmh()
    );

    // Adhesion: the loco never transmits more than µ·m·g.
    let loco = &sim.trains[t].vehicles[0];
    let mu = sim_core::physics::adhesion_coefficient(loco.v * 3.6, sim.trains[t].rail, false);
    let limit = mu * loco.adhesive_mass() * sim_core::G;
    assert!(
        loco.tractive_effort <= limit * 1.05,
        "transmitted tractive effort {} N above the adhesion limit {} N",
        loco.tractive_effort,
        limit
    );
}

#[test]
fn train_stretches_when_starting() {
    let mut sim = new_sim();
    let t = passenger_train(&mut sim, 5);
    power_up(&mut sim, t);
    sim.controls[t].reverser = 1;
    sim.controls[t].throttle = 0.6;
    let mut max_tension: f64 = 0.0;
    for _ in 0..3000 {
        sim.step(Sim::DT);
        max_tension = max_tension.max(sim.trains[t].couplers[0].force);
    }
    assert!(max_tension > 0.0, "the first coupler must go into tension");
    // Coupler slack: the rear vehicles start moving later.
    assert!(
        sim.trains[t].vehicles[0].x > sim.trains[t].vehicles[5].x,
        "train must be stretched"
    );
    assert!(
        sim.trains[t].couplers[0].extension > 0.0,
        "coupler extended"
    );
    // No coupler may break during a normal start.
    assert!(sim.trains[t].couplers.iter().all(|c| !c.broken));
}

#[test]
fn determinism_two_runs_same_hash() {
    let run = || {
        let mut sim = new_sim();
        let t = passenger_train(&mut sim, 5);
        power_up(&mut sim, t);
        sim.controls[t].reverser = 1;
        for i in 0..4000 {
            sim.controls[t].throttle = if i < 2000 { 0.8 } else { 0.0 };
            sim.controls[t].brake_valve = if i > 3000 {
                DriverBrakeValve::Service(0.8)
            } else {
                DriverBrakeValve::Release
            };
            hold_sifa(&mut sim, t, (i / 200) % 2 == 0);
            sim.step(Sim::DT);
        }
        sim.state_hash()
    };
    assert_eq!(run(), run(), "the same seed must yield an identical state");
}

#[test]
fn save_load_roundtrip_preserves_state() {
    let mut sim = new_sim();
    let t = passenger_train(&mut sim, 3);
    power_up(&mut sim, t);
    sim.controls[t].reverser = 1;
    sim.controls[t].throttle = 0.7;
    for _ in 0..2000 {
        sim.step(Sim::DT);
    }
    let hash = sim.state_hash();
    let text = ron::ser::to_string_pretty(&sim, ron::ser::PrettyConfig::default()).unwrap();
    let mut restored: Sim = ron::from_str(&text).expect("Sim readable");
    restored.net.finish();
    assert_eq!(restored.state_hash(), hash);

    // And continuing to simulate yields the same as the original.
    for _ in 0..500 {
        sim.step(Sim::DT);
        restored.step(Sim::DT);
    }
    assert_eq!(restored.state_hash(), sim.state_hash());
}
