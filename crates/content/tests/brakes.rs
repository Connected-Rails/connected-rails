//! Acceptance tests of the brake equipment and the drive models (plan ch. 7, 8) — headless.
//!
//! Everything here works on a whole train: the brakes of the individual vehicles are what
//! is simulated, not one lumped braking force.

use content::musterbahn;
use content::vehicles::{
    br101, br110, br218, freight_wagon, freight_wagon_k_valve, passenger_coach, railcar, vehicle,
};
use sim_core::Sim;
use sim_core::brakes::{
    COMPRESSOR_CUT_IN, ControlValve, DriverBrakeValve, SPRING_RELEASE_PRESSURE,
};
use sim_core::safety::SafetySystems;
use sim_core::train::{Train, Vehicle, VehicleSpec};
use track_model::{EdgeId, TrackPosition};

fn new_sim() -> Sim {
    let line = musterbahn().compile().expect("line compiles");
    Sim::new(line.net, line.interlock, 1234)
}

/// Assembles a train from the given specs at the start of the line and powers it up.
fn train(sim: &mut Sim, specs: Vec<VehicleSpec>) -> usize {
    let head = TrackPosition::new(EdgeId(0), 100.0, 1);
    let vehicles: Vec<Vehicle> = specs
        .into_iter()
        .map(|spec| vehicle(spec, head, SafetySystems::None))
        .collect();
    let train = Train::assemble(vehicles, head, &sim.net);
    let index = sim.add_train(train);
    for v in &mut sim.trains[index].vehicles {
        if v.spec.traction.is_some() {
            v.traction.battery = true;
            v.traction.pantograph_command = true;
            v.traction.main_switch_command = true;
            v.traction.pantograph = 1.0;
            v.traction.compressor = true;
        }
    }
    index
}

fn run(sim: &mut Sim, seconds: f64) {
    for _ in 0..(seconds / Sim::DT) as usize {
        sim.step(Sim::DT);
    }
}

fn set_speed(sim: &mut Sim, t: usize, kmh: f64) {
    for v in &mut sim.trains[t].vehicles {
        v.v = kmh / 3.6;
    }
}

#[test]
fn the_equalising_device_holds_the_pressure_in_lap() {
    let with_angleicher = |on: bool| {
        let mut sim = new_sim();
        let mut loco = br101();
        loco.brake.angleicher = on;
        let t = train(&mut sim, vec![loco, passenger_coach(), passenger_coach()]);
        run(&mut sim, 5.0);
        sim.controls[t].brake_valve = DriverBrakeValve::Service(0.6);
        run(&mut sim, 8.0);
        let lapped = sim.trains[t].vehicles[0].brake.pipe;
        sim.controls[t].brake_valve = DriverBrakeValve::Lap;
        run(&mut sim, 120.0);
        (lapped, sim.trains[t].vehicles[0].brake.pipe)
    };

    let (lapped, held) = with_angleicher(true);
    assert!(
        (held - lapped).abs() < 0.05,
        "the equalising device must hold {lapped:.2} bar, it is at {held:.2} bar"
    );

    // Without it, the leakage lets the pressure sink and the brake creeps on by itself.
    let (lapped, sunk) = with_angleicher(false);
    assert!(
        sunk < lapped - 0.1,
        "without an equalising device the pressure must sink: {lapped:.2} → {sunk:.2} bar"
    );
}

#[test]
fn a_k_valve_releases_in_one_go_a_ke_valve_graduates() {
    let last_cylinder = |valve: ControlValve| {
        let mut sim = new_sim();
        let mut wagons = vec![br101()];
        for _ in 0..5 {
            let mut w = if valve == ControlValve::KGp {
                freight_wagon_k_valve()
            } else {
                freight_wagon()
            };
            w.brake.position = sim_core::brakes::BrakePosition::P;
            wagons.push(w);
        }
        let t = train(&mut sim, wagons);
        run(&mut sim, 5.0);
        // Apply, then take one step of the application back again.
        sim.controls[t].brake_valve = DriverBrakeValve::Service(1.0);
        run(&mut sim, 20.0);
        let applied = sim.trains[t].vehicles[3].brake.cylinder;
        sim.controls[t].brake_valve = DriverBrakeValve::Service(0.5);
        run(&mut sim, 20.0);
        (applied, sim.trains[t].vehicles[3].brake.cylinder)
    };

    let (applied, graduated) = last_cylinder(ControlValve::KeGp);
    assert!(applied > 2.0, "KE valve must apply: {applied:.2} bar");
    assert!(
        graduated > 0.3 && graduated < applied,
        "a KE valve releases in steps: {applied:.2} → {graduated:.2} bar"
    );

    let (applied, released) = last_cylinder(ControlValve::KGp);
    assert!(applied > 2.0, "K valve must apply: {applied:.2} bar");
    assert!(
        released < 0.2,
        "a K valve is single-release and empties completely: {applied:.2} → {released:.2} bar"
    );
}

#[test]
fn the_spring_brake_applies_when_the_air_runs_out() {
    let mut sim = new_sim();
    let t = train(&mut sim, vec![br101(), passenger_coach()]);
    run(&mut sim, 10.0);
    assert!(
        !sim.trains[t].vehicles[0].brake.parking_applied,
        "with air in the main reservoir the spring brake is held off"
    );

    // Compressor off and the main reservoir empty — that is a loco left standing overnight.
    sim.trains[t].vehicles[0].traction.compressor = false;
    sim.trains[t].vehicles[0].brake.main_reservoir = 1.0;
    run(&mut sim, 10.0);
    let loco = &sim.trains[t].vehicles[0];
    assert!(
        loco.brake.parking_applied,
        "spring chamber {:.2} bar — the brake must have applied",
        loco.brake.spring_chamber
    );
    assert!(loco.brake.force > 50_000.0, "{:.0} N", loco.brake.force);

    // The compressor charges up again and the brake releases — a loco needs minutes for it.
    sim.trains[t].vehicles[0].traction.compressor = true;
    run(&mut sim, 200.0);
    let loco = &sim.trains[t].vehicles[0];
    assert!(loco.brake.main_reservoir > SPRING_RELEASE_PRESSURE);
    assert!(!loco.brake.parking_applied);
}

#[test]
fn the_compressor_keeps_the_main_reservoir_between_its_switching_pressures() {
    let mut sim = new_sim();
    let t = train(
        &mut sim,
        vec![br101(), passenger_coach(), passenger_coach()],
    );
    sim.trains[t].vehicles[0].brake.main_reservoir = 7.0;
    run(&mut sim, 120.0);
    let loco = &sim.trains[t].vehicles[0];
    assert!(
        loco.brake.main_reservoir > COMPRESSOR_CUT_IN,
        "compressor must charge up: {:.2} bar",
        loco.brake.main_reservoir
    );
    // Air consumption is accounted for, not assumed away.
    assert!(
        loco.brake.air_consumed > 0.0,
        "leakage and brake alone consume air"
    );

    // A brake application costs air out of the main reservoir.
    let before = sim.trains[t].vehicles[0].brake.air_consumed;
    sim.controls[t].brake_valve = DriverBrakeValve::Service(1.5);
    run(&mut sim, 20.0);
    sim.controls[t].brake_valve = DriverBrakeValve::Release;
    run(&mut sim, 40.0);
    assert!(
        sim.trains[t].vehicles[0].brake.air_consumed > before + 50.0,
        "an application and release must be measurable in the air consumption"
    );
}

#[test]
fn the_air_supplement_brake_fills_up_what_the_dynamic_brake_lacks() {
    let mut sim = new_sim();
    let t = train(
        &mut sim,
        vec![br101(), passenger_coach(), passenger_coach()],
    );
    run(&mut sim, 5.0);
    set_speed(&mut sim, t, 120.0);
    sim.controls[t].reverser = 1;
    sim.controls[t].throttle = -1.0;
    sim.controls[t].brake_valve = DriverBrakeValve::Service(1.0);
    run(&mut sim, 10.0);

    let loco = &sim.trains[t].vehicles[0];
    let coach = &sim.trains[t].vehicles[1];
    assert!(
        loco.traction.dynamic_force > 50_000.0,
        "the regenerative brake must work at 120 km/h: {:.0} N",
        loco.traction.dynamic_force
    );
    // The pneumatic part gets out of the way — the loco's cylinder force is what is left
    // over after the dynamic brake, and the coach behind brakes purely pneumatically.
    assert!(
        loco.brake.force < coach.brake.force,
        "loco (pneumatic) {:.0} N vs coach {:.0} N",
        loco.brake.force,
        coach.brake.force
    );
    // Together they still deliver at least what the coach does per tonne.
    assert!(loco.brake_effort > coach.brake.force * 0.8);
}

#[test]
fn every_wagon_of_the_train_has_its_own_brake_state() {
    let mut sim = new_sim();
    let mut specs = vec![br101()];
    for _ in 0..20 {
        specs.push(freight_wagon());
    }
    let t = train(&mut sim, specs);
    run(&mut sim, 5.0);
    set_speed(&mut sim, t, 60.0);
    sim.controls[t].brake_valve = DriverBrakeValve::Service(1.5);
    // Brake position G: the wagons need about 22 s to fill, and the rear needs longer still.
    run(&mut sim, 10.0);

    let cylinders: Vec<f64> = sim.trains[t]
        .vehicles
        .iter()
        .map(|v| v.brake.cylinder)
        .collect();
    // The pressure wave runs to the rear: the front is applying, the rear is not yet.
    assert!(
        cylinders[1] > cylinders[20] + 0.15,
        "front {:.2} bar vs rear {:.2} bar",
        cylinders[1],
        cylinders[20]
    );
    // And the forces differ accordingly — no lumped braking force anywhere.
    let forces: Vec<f64> = sim.trains[t]
        .vehicles
        .iter()
        .map(|v| v.brake_effort)
        .collect();
    assert!(forces[1] > forces[20]);
    // Emptying an auxiliary reservoir in the middle of the train leaves the neighbours alone.
    sim.trains[t].vehicles[10].brake.aux_reservoir = 0.0;
    run(&mut sim, 10.0);
    let exhausted = sim.trains[t].vehicles[10].brake.cylinder;
    let neighbour = sim.trains[t].vehicles[11].brake.cylinder;
    assert!(
        exhausted < neighbour,
        "an exhausted wagon brakes less than its neighbour: {exhausted:.2} vs {neighbour:.2} bar"
    );
}

#[test]
fn the_diesel_hydraulic_loco_starts_a_train_and_changes_up() {
    let mut sim = new_sim();
    let t = train(
        &mut sim,
        vec![
            br218(),
            passenger_coach(),
            passenger_coach(),
            passenger_coach(),
        ],
    );
    // Crank the engine.
    sim.controls[t].engine_start = true;
    run(&mut sim, 0.1);
    sim.controls[t].engine_start = false;
    run(&mut sim, 15.0);
    assert!(sim.trains[t].vehicles[0].traction.engine_running);

    sim.controls[t].reverser = 1;
    sim.controls[t].throttle = 1.0;
    sim.controls[t].brake_valve = DriverBrakeValve::Release;
    run(&mut sim, 120.0);

    let loco = &sim.trains[t].vehicles[0];
    let kmh = sim.trains[t].speed_kmh();
    assert!(kmh > 70.0, "diesel-hydraulic train too slow: {kmh:.1} km/h");
    assert_eq!(
        loco.traction.circuit, 1,
        "must have changed to the second converter"
    );
    assert!(
        loco.traction.engine_rpm > 1300.0,
        "governor must hold the engine speed up: {:.0} 1/min",
        loco.traction.engine_rpm
    );
}

#[test]
fn the_railcar_brakes_hydrodynamically_before_the_air_brake() {
    let mut sim = new_sim();
    let t = train(&mut sim, vec![railcar()]);
    sim.controls[t].engine_start = true;
    run(&mut sim, 0.1);
    sim.controls[t].engine_start = false;
    run(&mut sim, 12.0);
    set_speed(&mut sim, t, 100.0);
    sim.controls[t].reverser = 1;
    sim.controls[t].throttle = -1.0;
    run(&mut sim, 5.0);

    let unit = &sim.trains[t].vehicles[0];
    assert!(
        unit.traction.retarder_fill > 0.9,
        "the retarder must be filled: {:.2}",
        unit.traction.retarder_fill
    );
    assert!(
        unit.traction.dynamic_force > 20_000.0,
        "hydrodynamic braking force {:.0} N",
        unit.traction.dynamic_force
    );
    let before = sim.trains[t].speed_kmh();
    run(&mut sim, 10.0);
    assert!(
        sim.trains[t].speed_kmh() < before - 5.0,
        "it must slow down"
    );
}

#[test]
fn the_tap_changer_loco_pulls_harder_at_a_stand_than_at_speed() {
    let effort_at = |kmh: f64| {
        let mut sim = new_sim();
        let t = train(&mut sim, vec![br110(), passenger_coach()]);
        run(&mut sim, 8.0);
        set_speed(&mut sim, t, kmh);
        sim.controls[t].reverser = 1;
        sim.controls[t].throttle = 1.0;
        // The tap changer needs 28 × 0.8 s for the whole range.
        for _ in 0..(30.0 / Sim::DT) as usize {
            sim.step(Sim::DT);
            for v in &mut sim.trains[t].vehicles {
                v.v = kmh / 3.6;
            }
        }
        sim.trains[t].vehicles[0].traction.force
    };
    let standing = effort_at(0.0);
    let running = effort_at(120.0);
    assert!(
        standing > running * 1.5,
        "series motor: {standing:.0} N at a stand vs {running:.0} N at 120 km/h"
    );
    assert!(
        (200_000.0..320_000.0).contains(&standing),
        "{standing:.0} N"
    );
}

#[test]
fn the_pre_controlled_brake_applies_the_whole_train_at_once() {
    let brake_after = |ep: bool| {
        let mut sim = new_sim();
        let mut specs = vec![br101()];
        for _ in 0..8 {
            let mut coach = passenger_coach();
            // ep equipment on the coaches as well — otherwise there is nothing to control.
            coach.brake.pilot_controlled = true;
            specs.push(coach);
        }
        let t = train(&mut sim, specs);
        run(&mut sim, 5.0);
        sim.controls[t].ep_brake = ep;
        sim.controls[t].brake_valve = DriverBrakeValve::Service(1.5);
        run(&mut sim, 2.0);
        sim.trains[t].vehicles.last().unwrap().brake.cylinder
    };
    let pneumatic = brake_after(false);
    let electric = brake_after(true);
    assert!(
        electric > pneumatic + 0.5,
        "the pre-controlled brake must be at the rear sooner: {pneumatic:.2} vs {electric:.2} bar"
    );
}
