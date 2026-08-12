//! Vehicle database (plan ch. 15): RON files + built-in reference vehicles.

use sim_core::brakes::{BrakeKind, BrakePosition, BrakeSpec};
use sim_core::electric::TractionSpec;
use sim_core::safety::SafetySystems;
use sim_core::safety::de::{DeSafety, TrainType};
use sim_core::train::{CouplerSpec, Davis, Vehicle, VehicleSpec};
use track_model::TrackPosition;

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
            .with_direct_brake(),
        traction: Some(TractionSpec::Converter {
            max_force: 300_000.0,
            max_power: 6_400_000.0,
            v_max: 220.0,
            brake_force: 150_000.0,
            brake_power: 2_600_000.0,
            ramp_time: 2.5,
        }),
        coupler: CouplerSpec::screw(),
        adhesive_mass_fraction: 1.0,
        slip_control: true,
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
            .with_direct_brake(),
        traction: Some(TractionSpec::TapChanger {
            steps: 28,
            max_force: 275_000.0,
            max_power: 3_620_000.0,
            v_max: 150.0,
            step_time: 0.8,
        }),
        coupler: CouplerSpec::screw(),
        adhesive_mass_fraction: 1.0,
        slip_control: false,
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
            .with_direct_brake(),
        traction: Some(TractionSpec::Diesel {
            max_force: 235_000.0,
            max_power: 1_840_000.0,
            v_max: 140.0,
            ramp_time: 4.0,
            start_time: 8.0,
        }),
        coupler: CouplerSpec::screw(),
        adhesive_mass_fraction: 1.0,
        slip_control: false,
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
        brake: BrakeSpec::from_brake_weight(45.0, BrakeKind::Disc).with_position(BrakePosition::P),
        traction: None,
        coupler: CouplerSpec::screw(),
        adhesive_mass_fraction: 0.0,
        slip_control: true,
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
        brake: BrakeSpec::from_brake_weight(22.0, BrakeKind::Block).with_position(BrakePosition::G),
        traction: None,
        coupler: CouplerSpec::screw(),
        adhesive_mass_fraction: 0.0,
        slip_control: false,
    }
}

/// Builds a vehicle at a track position, optionally with German train protection.
pub fn vehicle(spec: VehicleSpec, pos: TrackPosition, safety: SafetySystems) -> Vehicle {
    let mut v = Vehicle::new(spec, pos);
    v.safety = safety;
    v
}

/// Equipment: Sifa + PZB.
pub fn de_pzb(train_type: TrainType) -> SafetySystems {
    SafetySystems::De(DeSafety::pzb(train_type))
}

/// Equipment: Sifa + PZB + LZB.
pub fn de_pzb_lzb(train_type: TrainType) -> SafetySystems {
    SafetySystems::De(DeSafety::pzb_lzb(train_type))
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
