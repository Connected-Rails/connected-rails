//! Acceptance tests of the brake equipment and the drive models (plan ch. 7, 8) — headless.
//!
//! Everything here works on a whole train: the brakes of the individual vehicles are what
//! is simulated, not one lumped braking force.

use content::musterbahn;
use content::vehicles::{
    br52, br101, br110, br218, br232, freight_wagon, freight_wagon_k_valve, passenger_coach,
    railcar,
};
use sim_core::Sim;
use sim_core::brakes::{
    COMPRESSOR_CUT_IN, ControlValve, DriverBrakeValve, SPRING_RELEASE_PRESSURE,
};
use sim_core::safety::SafetyEquipment;
use sim_core::train::{Train, Vehicle, VehicleSpec};
use track_model::{EdgeId, TrackPosition};

fn new_sim() -> Sim {
    let line = musterbahn().compile().expect("line compiles");
    Sim::new(line.net, line.interlock, 1234)
}

/// Assembles a train from the given specs at the start of the line and powers it up.
fn train(sim: &mut Sim, specs: Vec<VehicleSpec>) -> usize {
    let head = TrackPosition::new(EdgeId(0), 100.0, 1);
    // Without train protection — these tests are about the brake, not about the PZB.
    let vehicles: Vec<Vehicle> = specs
        .into_iter()
        .map(|spec| {
            Vehicle::new(
                VehicleSpec {
                    safety: SafetyEquipment::None,
                    ..spec
                },
                head,
            )
        })
        .collect();
    let train = Train::assemble(vehicles, head, &sim.net);
    let index = sim.add_train(train);
    for v in &mut sim.trains[index].vehicles {
        if v.spec.powered() {
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

/// Changeover lever on the freight wagon: it moves the rigging, so the loaded wagon gets
/// the braked weight of its anscription while the cylinder sees the same pressure.
#[test]
fn the_changeover_lever_gives_the_loaded_wagon_its_full_brake() {
    let braked = |load_kg: f64| {
        let mut sim = new_sim();
        let t = train(&mut sim, vec![br101(), freight_wagon()]);
        sim.trains[t].vehicles[1].load = load_kg;
        // Charge the brake pipe, then apply. Position G takes its time.
        run(&mut sim, 60.0);
        sim.controls[t].brake_valve = DriverBrakeValve::Emergency;
        run(&mut sim, 60.0);
        let wagon = &sim.trains[t].vehicles[1];
        (wagon.brake.force, wagon.brake.cylinder)
    };

    let (empty_force, empty_cylinder) = braked(0.0);
    let (laden_force, laden_cylinder) = braked(57_000.0);
    assert!(empty_force > 1_000.0, "the wagon must brake at all");
    // 22 t braked weight empty, 55 t loaded — the anscription of an Eaos.
    assert!(
        (laden_force / empty_force - 55.0 / 22.0).abs() < 0.05,
        "empty {empty_force:.0} N → loaded {laden_force:.0} N"
    );
    assert!(
        (laden_cylinder - empty_cylinder).abs() < 0.01,
        "a lever is not a valve: {empty_cylinder:.2} vs {laden_cylinder:.2} bar"
    );
}

/// Weighing valve on the railcar: it throttles the cylinder pressure itself, and it does
/// it so that the braked weight percentage — the figure the brake sheet is written in —
/// stays where it belongs however full the vehicle is.
#[test]
fn the_weighing_valve_holds_the_brake_percentage_of_the_railcar() {
    let braked = |load_kg: f64| {
        let mut sim = new_sim();
        let t = train(&mut sim, vec![railcar()]);
        sim.trains[t].vehicles[0].load = load_kg;
        run(&mut sim, 60.0);
        let percentage = sim.trains[t].brake_percentage();
        sim.controls[t].brake_valve = DriverBrakeValve::Emergency;
        run(&mut sim, 20.0);
        (percentage, sim.trains[t].vehicles[0].brake.cylinder)
    };

    let (empty_percentage, empty_cylinder) = braked(0.0);
    let (full_percentage, full_cylinder) = braked(9_000.0);
    assert!(
        (empty_percentage - full_percentage).abs() < 0.5,
        "brake sheet must not move: {empty_percentage:.1} vs {full_percentage:.1} %"
    );
    assert!(
        full_cylinder > empty_cylinder * 1.1,
        "the valve must throttle: {empty_cylinder:.2} → {full_cylinder:.2} bar"
    );
}

#[test]
fn a_k_valve_releases_in_one_go_a_ke_valve_graduates() {
    let last_cylinder = |valve: ControlValve| {
        let mut sim = new_sim();
        let mut wagons = vec![br101()];
        for _ in 0..5 {
            wagons.push(if valve == ControlValve::KGp {
                freight_wagon_k_valve()
            } else {
                freight_wagon()
            });
        }
        let t = train(&mut sim, wagons);
        // Made up as a passenger train: the changeover handles go to P.
        sim.trains[t].set_brake_position(sim_core::brakes::BrakePosition::P);
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
        loco.traction.drives[0].dynamic_force > 50_000.0,
        "the regenerative brake must work at 120 km/h: {:.0} N",
        loco.traction.drives[0].dynamic_force
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
    assert!(sim.trains[t].vehicles[0].traction.any_engine_running());

    sim.controls[t].reverser = 1;
    sim.controls[t].throttle = 1.0;
    sim.controls[t].brake_valve = DriverBrakeValve::Release;
    run(&mut sim, 120.0);

    let loco = &sim.trains[t].vehicles[0];
    let kmh = sim.trains[t].speed_kmh();
    assert!(kmh > 70.0, "diesel-hydraulic train too slow: {kmh:.1} km/h");
    assert_eq!(
        loco.traction.drives[0].circuit, 1,
        "must have changed to the second converter"
    );
    assert!(
        loco.traction.drives[0].engine_rpm > 1300.0,
        "governor must hold the engine speed up: {:.0} 1/min",
        loco.traction.drives[0].engine_rpm
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
        unit.traction.drives[0].retarder_fill > 0.9,
        "the retarder must be filled: {:.2}",
        unit.traction.drives[0].retarder_fill
    );
    assert!(
        unit.traction.drives[0].dynamic_force > 20_000.0,
        "hydrodynamic braking force {:.0} N",
        unit.traction.drives[0].dynamic_force
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
            coach.brake.ep = Some(sim_core::brakes::EpBrake::default());
            specs.push(coach);
        }
        let t = train(&mut sim, specs);
        run(&mut sim, 5.0);
        sim.controls[t].ep_brake = ep;
        sim.controls[t].brake_valve = DriverBrakeValve::Service(1.5);
        run(&mut sim, 2.0);
        sim.trains[t]
            .vehicles
            .last()
            .unwrap()
            .brake
            .applied_cylinder()
    };
    let pneumatic = brake_after(false);
    let electric = brake_after(true);
    assert!(
        electric > pneumatic + 0.5,
        "the pre-controlled brake must be at the rear sooner: {pneumatic:.2} vs {electric:.2} bar"
    );
}

#[test]
fn a_closed_angle_cock_leaves_everything_behind_it_unbraked() {
    let mut sim = new_sim();
    let mut specs = vec![br101()];
    specs.extend(std::iter::repeat_with(freight_wagon).take(8));
    let t = train(&mut sim, specs);
    run(&mut sim, 20.0);
    // Someone has closed the cock between the third and the fourth wagon.
    sim.trains[t].vehicles[3].brake.cock_rear = false;
    sim.trains[t].vehicles[4].brake.cock_front = false;
    sim.controls[t].brake_valve = DriverBrakeValve::Service(1.5);
    run(&mut sim, 20.0);
    let front = sim.trains[t].vehicles[2].brake.applied_cylinder();
    let behind = sim.trains[t].vehicles[6].brake.applied_cylinder();
    assert!(front > 2.0, "the front must brake: {front:.2} bar");
    assert!(
        behind < 0.2,
        "behind the closed cock nothing may happen: {behind:.2} bar"
    );
}

#[test]
fn an_open_cock_at_the_end_of_the_train_will_not_let_it_charge() {
    let mut sim = new_sim();
    let mut specs = vec![br101()];
    specs.extend(std::iter::repeat_with(freight_wagon).take(4));
    let t = train(&mut sim, specs);
    // Dump the pipe, then try to charge it with the last cock left open.
    for v in &mut sim.trains[t].vehicles {
        v.brake.pipe = 0.0;
    }
    sim.trains[t].vehicles.last_mut().unwrap().brake.cock_rear = true;
    sim.controls[t].brake_valve = DriverBrakeValve::Release;
    run(&mut sim, 60.0);
    let rear = sim.trains[t].vehicles.last().unwrap().brake.pipe;
    assert!(rear < 2.0, "the pipe must not charge: {rear:.2} bar");

    // Shut it and the train charges as it should.
    sim.trains[t].vehicles.last_mut().unwrap().brake.cock_rear = false;
    run(&mut sim, 90.0);
    let rear = sim.trains[t].vehicles.last().unwrap().brake.pipe;
    assert!(rear > 4.5, "{rear:.2} bar");
}

#[test]
fn a_vacuum_braked_train_stops_on_its_own_numbers() {
    let mut sim = new_sim();
    let specs: Vec<VehicleSpec> = std::iter::once(br101())
        .chain(std::iter::repeat_with(passenger_coach).take(5))
        .map(|mut spec| {
            spec.brake = spec.brake.clone().as_vacuum();
            spec
        })
        .collect();
    let t = train(&mut sim, specs);
    // Charge the pipe to full vacuum first — the exhauster is a slow pump.
    run(&mut sim, 120.0);
    let charged = sim.trains[t].vehicles.last().unwrap().brake.pipe;
    assert!(
        charged > sim_core::brakes::VACUUM_NOMINAL * 0.9,
        "{charged:.2} bar of vacuum"
    );
    set_speed(&mut sim, t, 80.0);
    sim.controls[t].brake_valve = DriverBrakeValve::Service(1.5);
    run(&mut sim, 20.0);
    for v in &sim.trains[t].vehicles {
        assert!(
            v.brake.applied_cylinder() > 1.5,
            "{} did not brake: {:.2} bar",
            v.spec.name,
            v.brake.applied_cylinder()
        );
    }
    assert!(sim.trains[t].speed_kmh() < 80.0, "the train must slow down");
}

#[test]
fn the_diesel_electric_holds_its_power_across_the_speed_range() {
    let mut sim = new_sim();
    let t = train(&mut sim, vec![br232(), freight_wagon(), freight_wagon()]);
    sim.controls[t].engine_start = true;
    run(&mut sim, 20.0);
    sim.controls[t].engine_start = false;
    assert!(
        sim.trains[t].vehicles[0].traction.any_engine_running(),
        "the engine must be running"
    );
    sim.controls[t].reverser = 1;
    sim.controls[t].throttle = 1.0;

    let effort_at = |sim: &mut Sim, kmh: f64| {
        set_speed(sim, t, kmh);
        // Long enough for the load regulator to have travelled.
        for _ in 0..(15.0 / Sim::DT) as usize {
            sim.step(Sim::DT);
            set_speed(sim, t, kmh);
        }
        sim.trains[t].vehicles[0].traction.force
    };
    let start = effort_at(&mut sim, 5.0);
    let mid = effort_at(&mut sim, 40.0);
    let fast = effort_at(&mut sim, 100.0);
    assert!(
        start > mid && mid > fast,
        "{start:.0} / {mid:.0} / {fast:.0} N"
    );
    // A 2.2 MW machine: a few hundred kN at a stand.
    assert!(
        (150_000.0..420_000.0).contains(&start),
        "{start:.0} N at a stand"
    );
    // The regulator holds the power, so effort × speed stays in the same order of
    // magnitude once the current limit has let go.
    let p_mid = mid * 40.0 / 3.6;
    let p_fast = fast * 100.0 / 3.6;
    assert!(
        p_fast > p_mid * 0.6 && p_fast < p_mid * 1.6,
        "{p_mid:.0} vs {p_fast:.0} W"
    );
}

#[test]
fn the_steam_locomotive_makes_effort_out_of_its_boiler_and_runs_out_of_it() {
    let mut sim = new_sim();
    let mut specs = vec![br52()];
    specs.extend(std::iter::repeat_with(freight_wagon).take(10));
    let t = train(&mut sim, specs);
    run(&mut sim, 30.0);

    // Regulator open, long cutoff, damper open: the loco pulls.
    sim.controls[t].reverser = 1;
    sim.controls[t].steam.regulator = 1.0;
    sim.controls[t].steam.cutoff = 1.0;
    sim.controls[t].steam.damper = 1.0;
    run(&mut sim, 10.0);
    let effort = sim.trains[t].vehicles[0].traction.force;
    assert!(effort > 100_000.0, "{effort:.0} N");

    // Working it hard without firing empties the boiler.
    let before = sim.trains[t].vehicles[0].traction.drives[0]
        .steam
        .expect("boiler")
        .pressure;
    set_speed(&mut sim, t, 50.0);
    for _ in 0..(600.0 / Sim::DT) as usize {
        sim.step(Sim::DT);
        set_speed(&mut sim, t, 50.0);
    }
    let boiler = sim.trains[t].vehicles[0].traction.drives[0]
        .steam
        .expect("boiler");
    assert!(
        boiler.pressure < before,
        "{before:.1} → {:.1} bar",
        boiler.pressure
    );
    assert!(
        boiler.fire_mass < 260.0 * 0.6,
        "the fire must burn what is on the grate: {:.0} kg",
        boiler.fire_mass
    );

    // Firing and an injector bring it back.
    sim.controls[t].steam.regulator = 0.0;
    sim.controls[t].steam.blower = 1.0;
    sim.controls[t].steam.injector_left = 1.0;
    let low = boiler.pressure;
    for i in 0..(900.0 / Sim::DT) as usize {
        sim.controls[t].shovel = if i % (30.0 / Sim::DT) as usize == 0 {
            3.0
        } else {
            0.0
        };
        sim.step(Sim::DT);
    }
    let boiler = sim.trains[t].vehicles[0].traction.drives[0]
        .steam
        .expect("boiler");
    assert!(
        boiler.pressure > low,
        "{low:.1} → {:.1} bar",
        boiler.pressure
    );
    assert!(
        boiler.tender_water < 30_000.0,
        "the injector must draw water"
    );
    assert!(
        boiler.tender_coal < 10_000.0,
        "the fireman must have used coal"
    );
}

#[test]
fn a_pulled_emergency_valve_stops_the_train_whatever_the_driver_does() {
    let mut sim = new_sim();
    let mut loco = br101();
    loco.brake.has_emergency_valve = true;
    let mut specs = vec![loco];
    specs.extend(std::iter::repeat_with(passenger_coach).take(4));
    let t = train(&mut sim, specs);
    run(&mut sim, 30.0);
    set_speed(&mut sim, t, 100.0);

    // Driver's valve in release, emergency valve pulled.
    sim.controls[t].brake_valve = DriverBrakeValve::Release;
    sim.controls[t].emergency_valve = true;
    run(&mut sim, 15.0);
    let pipe = sim.trains[t].vehicles.last().unwrap().brake.pipe;
    assert!(pipe < 1.0, "the pipe must be vented: {pipe:.2} bar");
    for v in &sim.trains[t].vehicles {
        assert!(
            v.brake.applied_cylinder() > 2.0,
            "{} must brake: {:.2} bar",
            v.spec.name,
            v.brake.applied_cylinder()
        );
    }

    // Reset it and the train charges again.
    sim.controls[t].emergency_valve = false;
    run(&mut sim, 120.0);
    assert!(
        sim.trains[t].vehicles.last().unwrap().brake.pipe > 4.5,
        "the pipe must charge again"
    );
}

#[test]
fn a_vehicle_whose_pipe_stops_short_cannot_pass_the_brake_on() {
    let mut sim = new_sim();
    let mut blocked = freight_wagon();
    // A works vehicle with no brake pipe at its rear end.
    blocked.brake.pipe_rear = false;
    let specs = vec![
        br101(),
        freight_wagon(),
        blocked,
        freight_wagon(),
        freight_wagon(),
    ];
    let t = train(&mut sim, specs);
    run(&mut sim, 30.0);
    sim.controls[t].brake_valve = DriverBrakeValve::Emergency;
    run(&mut sim, 30.0);
    assert!(
        sim.trains[t].vehicles[1].brake.applied_cylinder() > 2.0,
        "in front of it the brake works"
    );
    assert!(
        sim.trains[t].vehicles[4].brake.applied_cylinder() < 0.2,
        "behind it nothing reaches the wagons"
    );
}

#[test]
fn more_sand_is_more_adhesion_up_to_a_point() {
    use sim_core::physics::{REFERENCE_SAND_RATE, adhesion_with_sand};
    use sim_core::train::RailCondition;
    let mu = |rate: f64| adhesion_with_sand(40.0, RailCondition::Wet, rate);
    assert!(mu(0.0) < mu(REFERENCE_SAND_RATE));
    assert!(mu(REFERENCE_SAND_RATE) < mu(REFERENCE_SAND_RATE * 1.4));
    // Past that the extra sand is thrown away.
    assert!((mu(REFERENCE_SAND_RATE * 1.4) - mu(REFERENCE_SAND_RATE * 4.0)).abs() < 1e-12);
}

/// The wire is a property of the line, and a locomotive works only under the system it
/// was built for. Everything else is how a loco ends up stranded at a system boundary.
mod electrification {
    use super::*;
    use sim_core::electric::SupplySystem;
    use track_model::{PowerSystem, StepProfile};

    /// Puts a BR 101 on the line and reports whether its main switch closes.
    fn runs_under(line: Option<PowerSystem>, built_for: &[SupplySystem]) -> bool {
        let mut sim = new_sim();
        sim.net.set_default_electrification(line);
        let mut loco = br101();
        loco.supply.systems = built_for.to_vec();
        let t = train(&mut sim, vec![loco, passenger_coach()]);
        run(&mut sim, 20.0);
        sim.trains[t].vehicles[0].traction.main_switch
    }

    #[test]
    fn a_locomotive_only_works_under_the_wire_it_was_built_for() {
        assert!(runs_under(
            Some(PowerSystem::Ac15kv),
            &[SupplySystem::Ac15kv]
        ));
        // The volts are there and the system is wrong — 25 kV would cook a 15 kV
        // transformer, and the switch stays open.
        assert!(!runs_under(
            Some(PowerSystem::Ac25kv),
            &[SupplySystem::Ac15kv]
        ));
        assert!(!runs_under(
            Some(PowerSystem::Dc1500v),
            &[SupplySystem::Ac15kv]
        ));
        // And a line under no wire at all runs nothing electric.
        assert!(!runs_under(None, &[SupplySystem::Ac15kv]));
    }

    #[test]
    fn a_multi_system_locomotive_runs_under_all_of_them() {
        let built = [SupplySystem::Ac15kv, SupplySystem::Ac25kv];
        assert!(runs_under(Some(PowerSystem::Ac15kv), &built));
        assert!(runs_under(Some(PowerSystem::Ac25kv), &built));
        assert!(!runs_under(Some(PowerSystem::Dc3kv), &built));
    }

    #[test]
    fn running_off_the_end_of_the_wire_drops_the_main_switch() {
        let mut sim = new_sim();
        // The first 500 m are wired, everything past it is not.
        let length = sim.net.edges()[0].length();
        assert!(length > 600.0, "the test needs a long first edge");
        sim.net.set_electrification(
            EdgeId(0),
            Some(StepProfile::new(vec![
                (0.0, Some(PowerSystem::Ac15kv)),
                (500.0, None),
            ])),
        );
        let t = train(&mut sim, vec![br101(), passenger_coach()]);
        run(&mut sim, 20.0);
        assert!(
            sim.trains[t].vehicles[0].traction.main_switch,
            "under the wire it works"
        );

        // Roll on past the end of the electrification.
        set_speed(&mut sim, t, 60.0);
        run(&mut sim, 40.0);
        assert!(
            sim.trains[t].vehicles[0].pos.s > 500.0,
            "the train has to leave the wired section"
        );
        assert!(
            !sim.trains[t].vehicles[0].traction.main_switch,
            "off the end of the wire the main switch drops"
        );
        assert_eq!(sim.trains[t].vehicles[0].traction.line_system, None);
    }
}
