//! Vehicle database (plan ch. 15): RON files + built-in reference vehicles.

use sim_core::brakes::{BrakeKind, BrakePosition, BrakeSpec, ControlValve, SlipProtection};
use sim_core::doors::DoorSystem;
use sim_core::drive::{
    Circuit, CircuitKind, DieselEngine, Governor, HydrodynamicBrake, SeriesMotor, TractionSpec,
    Transmission,
};
use sim_core::safety::SafetyEquipment;
use sim_core::safety::de::{PzbVariant, SifaKind, TrainType};
use sim_core::train::{CouplerSpec, Davis, STANDARD_GAUGE, VehicleSpec};

/// Loads a vehicle definition from RON.
pub fn load_vehicle(ron_text: &str) -> Result<VehicleSpec, ron::error::SpannedError> {
    ron::from_str(ron_text)
}

/// Serializes a vehicle definition (for the editor).
pub fn save_vehicle(spec: &VehicleSpec) -> String {
    ron::ser::to_string_pretty(spec, ron::ser::PrettyConfig::default()).expect("serializable")
}

/// BR 101 — three-phase loco with converter, LZB/PZB, electric brake.
pub fn br101() -> VehicleSpec {
    VehicleSpec {
        name: "BR 101".into(),
        length: 19.1,
        mass_empty: 84_000.0,
        rotating_mass_factor: 0.20,
        // Davis parameters roughly from the data sheet (a in N, b in N/(m/s), c in N/(m/s)²).
        davis: Davis {
            a: 2_200.0,
            b: 60.0,
            c: 6.5,
        },
        brake: BrakeSpec::from_brake_weight(90.0, BrakeKind::Disc)
            .with_position(BrakePosition::R)
            // Two cylinder pressure stages, changed over by speed; pre-controlled,
            // with an air supplement brake behind the regenerative brake.
            .as_traction_unit(ControlValve::KeL2a, 90_000.0),
        traction: Some(TractionSpec::Converter {
            max_force: 300_000.0,
            max_power: 6_400_000.0,
            v_max: 220.0,
            brake_force: 150_000.0,
            brake_power: 2_600_000.0,
            ramp_time: 2.5,
            // Above this speed the pull-out torque of the induction motors takes over.
            v_pullout: 150.0,
            regenerative: true,
            brake_fade_kmh: 10.0,
        }),
        coupler: CouplerSpec::screw(),
        adhesive_mass_fraction: 1.0,
        slip_protection: SlipProtection::CreepControl,
        gauge: STANDARD_GAUGE,
        v_max: 220.0,
        axles: 4,
        // Bo'Bo', 2.65 m axle base per bogie.
        axle_base_sum: 5.3,
        // The Davis parameters are calibrated as a whole; cw·A stays open for mods.
        cw_a: None,
        curve_resistance_factor: 1.0,
        max_payload: 0.0,
        tilt_angle_deg: 0.0,
        passenger_doors: false,
        // PZB 90 V2.0 plus LZB 80 — the equipment of a high-speed capable main line loco.
        safety: SafetyEquipment::De {
            pzb: Some(PzbVariant::Pzb90V20),
            lzb: true,
            sifa: Some(SifaKind::TimeTime),
            train_type: TrainType::O,
        },
        // Door blocking of the hauled IC coaches, operated from the loco.
        doors: DoorSystem::Tb0,
        hunting: 0.0,
        script: None,
        model: None,
    }
}

/// BR 110 — older electric loco with transformer and tap changer, PZB, no electric brake.
pub fn br110() -> VehicleSpec {
    VehicleSpec {
        name: "BR 110".into(),
        length: 16.4,
        mass_empty: 85_000.0,
        rotating_mass_factor: 0.22,
        davis: Davis {
            a: 2_400.0,
            b: 65.0,
            c: 7.0,
        },
        brake: BrakeSpec::from_brake_weight(85.0, BrakeKind::Block)
            .with_position(BrakePosition::P)
            .as_traction_unit(ControlValve::KeGp, 85_000.0),
        traction: Some(TractionSpec::TapChanger {
            steps: 28,
            max_force: 275_000.0,
            max_power: 3_620_000.0,
            v_max: 150.0,
            step_time: 0.8,
            // Four series-wound motors; the characteristic follows from the machine
            // equations, so the loco pulls hard at a stand and thins out with speed.
            motor: Some(SeriesMotor {
                count: 4,
                resistance: 0.05,
                flux_constant: 0.0289,
                saturation_current: 600.0,
                max_current: 1600.0,
                max_voltage: 1000.0,
                field_steps: vec![1.0, 0.85, 0.7],
                gear_ratio: 2.17,
                wheel_diameter: 1.25,
                efficiency: 0.95,
            }),
            // The BR 110 has no electric brake.
            dynamic_brake: None,
        }),
        coupler: CouplerSpec::screw(),
        adhesive_mass_fraction: 1.0,
        // Older loco: the wheel slip brake takes the spinning wheelset down.
        slip_protection: SlipProtection::SlipBrake,
        gauge: STANDARD_GAUGE,
        v_max: 150.0,
        axles: 4,
        // Bo'Bo', 3.4 m axle base per bogie.
        axle_base_sum: 6.8,
        cw_a: None,
        curve_resistance_factor: 1.0,
        max_payload: 0.0,
        tilt_angle_deg: 0.0,
        passenger_doors: false,
        // Indusi I 60R — the retrofitted interim build, no LZB on board.
        safety: SafetyEquipment::De {
            pzb: Some(PzbVariant::I60R),
            lzb: false,
            sifa: Some(SifaKind::TimeTime),
            train_type: TrainType::O,
        },
        doors: DoorSystem::Tb0,
        hunting: 0.0,
        script: None,
        model: None,
    }
}

/// BR 218 — diesel loco with hydraulic transmission.
pub fn br218() -> VehicleSpec {
    VehicleSpec {
        name: "BR 218".into(),
        length: 16.4,
        mass_empty: 79_000.0,
        rotating_mass_factor: 0.18,
        davis: Davis {
            a: 2_500.0,
            b: 70.0,
            c: 7.2,
        },
        brake: BrakeSpec::from_brake_weight(78.0, BrakeKind::Block)
            .with_position(BrakePosition::P)
            .as_traction_unit(ControlValve::KeTm, 78_000.0),
        traction: Some(TractionSpec::Diesel {
            max_force: 235_000.0,
            max_power: 1_840_000.0,
            v_max: 140.0,
            ramp_time: 4.0,
            start_time: 8.0,
            // Speed-governed engine: the power controller sets the engine speed, the
            // governor holds it against the load.
            engine: Some(DieselEngine {
                idle_rpm: 600.0,
                rated_rpm: 1500.0,
                max_rpm: 1650.0,
                torque_curve: vec![
                    (600.0, 9_000.0),
                    (1000.0, 13_500.0),
                    (1500.0, 13_115.0),
                    (1650.0, 11_500.0),
                ],
                governor: Governor::Speed { steps: 0 },
                inertia: 60.0,
                response_time: 1.0,
            }),
            // Two converters, changed over by filling and emptying — the starting
            // converter multiplies almost two and a half times at stall.
            transmission: Some(Transmission {
                circuits: vec![
                    Circuit {
                        kind: CircuitKind::Converter,
                        ratio: 3.93,
                        stall_ratio: 2.4,
                        coupling_nu: 0.85,
                        absorption: 0.53,
                        shift_up_kmh: 72.0,
                    },
                    Circuit {
                        kind: CircuitKind::Converter,
                        ratio: 1.50,
                        stall_ratio: 1.9,
                        coupling_nu: 0.85,
                        absorption: 0.53,
                        shift_up_kmh: 0.0,
                    },
                ],
                // Quasi-continuous filling: the converter is the power control.
                fill_steps: 0,
                fill_time: 1.2,
                hysteresis_kmh: 10.0,
                final_ratio: 1.0,
                wheel_diameter: 1.0,
                count: 1,
                efficiency: 0.95,
            }),
            hydrodynamic_brake: None,
        }),
        coupler: CouplerSpec::screw(),
        adhesive_mass_fraction: 1.0,
        slip_protection: SlipProtection::SlipBrake,
        gauge: STANDARD_GAUGE,
        v_max: 140.0,
        axles: 4,
        // B'B', 2.8 m axle base per bogie.
        axle_base_sum: 5.6,
        cw_a: None,
        curve_resistance_factor: 1.0,
        max_payload: 0.0,
        tilt_angle_deg: 0.0,
        passenger_doors: false,
        safety: SafetyEquipment::De {
            pzb: Some(PzbVariant::Pzb90V20),
            lzb: false,
            sifa: Some(SifaKind::TimeTime),
            train_type: TrainType::M,
        },
        doors: DoorSystem::Tb0,
        hunting: 0.0,
        script: None,
        model: None,
    }
}

/// Passenger coach (n-Wagen/Bnrz), disc brake, brake position P.
pub fn passenger_coach() -> VehicleSpec {
    VehicleSpec {
        name: "Reisezugwagen".into(),
        length: 26.4,
        mass_empty: 42_000.0,
        rotating_mass_factor: 0.05,
        davis: Davis {
            a: 900.0,
            b: 22.0,
            c: 4.8,
        },
        brake: BrakeSpec::from_brake_weight(45.0, BrakeKind::Disc)
            .with_position(BrakePosition::P)
            .with_valve(ControlValve::KeGpr),
        traction: None,
        coupler: CouplerSpec::screw(),
        adhesive_mass_fraction: 0.0,
        slip_protection: SlipProtection::CreepControl,
        gauge: STANDARD_GAUGE,
        v_max: 160.0,
        axles: 4,
        // Two Minden-Deutz bogies, 2.5 m axle base each.
        axle_base_sum: 5.0,
        cw_a: None,
        curve_resistance_factor: 1.0,
        // Passengers and luggage — the usual assumption is 5 t per coach.
        max_payload: 5_000.0,
        tilt_angle_deg: 0.0,
        passenger_doors: true,
        // A hauled coach carries no train protection and no door control of its own.
        safety: SafetyEquipment::None,
        doors: DoorSystem::None,
        hunting: 0.0,
        script: None,
        model: None,
    }
}

/// Open freight wagon (Eaos), block brake, brake position G.
pub fn freight_wagon() -> VehicleSpec {
    VehicleSpec {
        name: "Güterwagen Eaos".into(),
        length: 14.0,
        mass_empty: 21_000.0,
        rotating_mass_factor: 0.06,
        davis: Davis {
            a: 700.0,
            b: 18.0,
            c: 5.5,
        },
        brake: BrakeSpec::from_brake_weight(22.0, BrakeKind::Block)
            .with_position(BrakePosition::G)
            .with_valve(ControlValve::KeGp),
        traction: None,
        coupler: CouplerSpec::screw(),
        adhesive_mass_fraction: 0.0,
        slip_protection: SlipProtection::None,
        gauge: STANDARD_GAUGE,
        v_max: 100.0,
        axles: 4,
        // Y25 bogies, 1.8 m axle base each.
        axle_base_sum: 3.6,
        cw_a: None,
        curve_resistance_factor: 1.0,
        max_payload: 57_000.0,
        tilt_angle_deg: 0.0,
        passenger_doors: false,
        safety: SafetyEquipment::None,
        doors: DoorSystem::None,
        hunting: 0.0,
        script: None,
        model: None,
    }
}

/// Older freight wagon with a K control valve: graduated application, but single-release —
/// once released it must be recharged before it can brake properly again.
pub fn freight_wagon_k_valve() -> VehicleSpec {
    let mut spec = freight_wagon();
    spec.name = "Güterwagen (K-Ventil)".into();
    spec.brake.valve = ControlValve::KGp;
    spec
}

/// Diesel railcar (BR 648 type): two fill-governed engines, one torque converter and one
/// fluid coupling each, hydrodynamic brake in the transmission, disc and magnetic brake.
pub fn railcar() -> VehicleSpec {
    VehicleSpec {
        name: "Dieseltriebwagen".into(),
        length: 41.8,
        mass_empty: 63_000.0,
        rotating_mass_factor: 0.12,
        davis: Davis {
            a: 1_300.0,
            b: 40.0,
            c: 6.0,
        },
        brake: BrakeSpec::from_brake_weight(66.0, BrakeKind::Disc)
            .with_mg(60_000.0)
            .as_traction_unit(ControlValve::KeGpr, 60_000.0),
        traction: Some(TractionSpec::Diesel {
            max_force: 65_000.0,
            max_power: 630_000.0,
            v_max: 120.0,
            ramp_time: 3.0,
            start_time: 6.0,
            // Fill-governed: the power controller is the fuel rack, the engine speed
            // follows from the load.
            engine: Some(DieselEngine {
                idle_rpm: 800.0,
                rated_rpm: 2100.0,
                max_rpm: 2300.0,
                torque_curve: vec![
                    (800.0, 1_000.0),
                    (1200.0, 1_500.0),
                    (1600.0, 1_550.0),
                    (2100.0, 1_432.0),
                    (2300.0, 1_250.0),
                ],
                governor: Governor::Fill,
                inertia: 8.0,
                response_time: 0.8,
            }),
            transmission: Some(Transmission {
                circuits: vec![
                    Circuit {
                        kind: CircuitKind::Converter,
                        ratio: 3.0,
                        stall_ratio: 2.8,
                        coupling_nu: 0.85,
                        absorption: 0.0296,
                        shift_up_kmh: 85.0,
                    },
                    // Above the change point a fluid coupling takes over — practically a
                    // direct drive, which is why the engine speed then follows the road.
                    Circuit {
                        kind: CircuitKind::Coupling,
                        ratio: 2.41,
                        stall_ratio: 1.0,
                        coupling_nu: 1.0,
                        absorption: 1.0,
                        shift_up_kmh: 0.0,
                    },
                ],
                // Five filling stages instead of continuous — the notches of the original.
                fill_steps: 5,
                fill_time: 0.9,
                hysteresis_kmh: 12.0,
                final_ratio: 1.0,
                wheel_diameter: 0.77,
                count: 2,
                efficiency: 0.95,
            }),
            hydrodynamic_brake: Some(HydrodynamicBrake {
                absorption: 0.046,
                ratio: 4.0,
                wheel_diameter: 0.77,
                max_force: 40_000.0,
                max_power: 500_000.0,
                fill_time: 1.0,
                fade_out_kmh: 15.0,
            }),
        }),
        coupler: CouplerSpec::center_buffer(),
        // Two of six axles driven.
        adhesive_mass_fraction: 0.34,
        slip_protection: SlipProtection::CreepControl,
        gauge: STANDARD_GAUGE,
        v_max: 120.0,
        axles: 6,
        axle_base_sum: 5.6,
        cw_a: None,
        curve_resistance_factor: 1.0,
        max_payload: 9_000.0,
        tilt_angle_deg: 0.0,
        passenger_doors: true,
        // Modern railcar: PZB 90, time-distance Sifa, automatic door closing.
        safety: SafetyEquipment::De {
            pzb: Some(PzbVariant::Pzb90V20),
            lzb: false,
            sifa: Some(SifaKind::TimeDistance),
            train_type: TrainType::M,
        },
        doors: DoorSystem::Tav,
        hunting: 0.0,
        script: None,
        model: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vehicle_specs_roundtrip_through_ron() {
        for spec in [
            br101(),
            br110(),
            br218(),
            passenger_coach(),
            freight_wagon(),
        ] {
            let text = save_vehicle(&spec);
            let back = load_vehicle(&text).expect("RON readable");
            assert_eq!(back.name, spec.name);
            assert_eq!(back.mass_empty, spec.mass_empty);
            assert_eq!(back.traction, spec.traction);
            assert_eq!(back.brake, spec.brake);
        }
    }

    #[test]
    fn brake_weights_are_plausible() {
        // For passenger coaches the brake weight exceeds the empty mass (brake percentage > 100).
        let coach = passenger_coach();
        assert!(coach.brake.brake_weight * 1000.0 > coach.mass_empty);
        // A freight wagon in G brakes more weakly than its mass.
        let wagon = freight_wagon();
        assert!(wagon.brake.brake_weight * 1000.0 <= wagon.mass_empty * 1.1);
    }
}
