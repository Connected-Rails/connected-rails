//! Longitudinal dynamics of the train consist (plan ch. 6).
//!
//! Every vehicle is a point mass on the track; neighbours are coupled by spring-dampers
//! with slack. Integration: semi-implicit Euler with a fixed step size.

use crate::G;
use crate::brakes::SlipProtection;
use crate::train::{AxleState, RailCondition, Train, Vehicle};
use track_model::{AdvanceError, PassedDevice, TrackNetwork};

/// Result of a physics step.
#[derive(Debug, Default, Clone)]
pub struct StepReport {
    /// Trackside devices that were passed, per vehicle index.
    pub passed: Vec<(usize, PassedDevice)>,
    /// The train has run up against a node that cannot be passed.
    pub blocked: Option<AdvanceError>,
    /// Broken couplers (index into `Train::couplers`).
    pub broken_couplers: Vec<usize>,
}

/// Adhesion coefficient after Curtius/Kniffler, with rail condition and sanding.
///
/// `sanding` is the plain on/off case at the reference rate; [`adhesion_with_sand`] takes
/// the vehicle's own sand rate, which is what the sander block sets.
pub fn adhesion_coefficient(v_kmh: f64, rail: RailCondition, sanding: bool) -> f64 {
    let rate = if sanding { REFERENCE_SAND_RATE } else { 0.0 };
    adhesion_with_sand(v_kmh, rail, rate)
}

/// Sand rate [kg/min] the 25 % bonus of [`adhesion_coefficient`] is calibrated for.
pub const REFERENCE_SAND_RATE: f64 = 4.0;

/// The same with the vehicle's own sand rate [kg/min]. More sand helps, but not without
/// end — past about twice the reference rate the extra sand is simply thrown away.
pub fn adhesion_with_sand(v_kmh: f64, rail: RailCondition, sand_rate: f64) -> f64 {
    let base = 7.5 / (v_kmh.abs() + 44.0) + 0.161;
    let sand = 1.0 + 0.25 * (sand_rate.max(0.0) / REFERENCE_SAND_RATE).min(1.4);
    base * rail.factor() * sand
}

/// Adhesion of this vehicle right now — its sand rate where it is sanding.
fn vehicle_adhesion(veh: &Vehicle, rail: RailCondition) -> f64 {
    let rate = if veh.sanding { veh.spec.sand_rate } else { 0.0 };
    adhesion_with_sand(veh.v * 3.6, rail, rate)
}

/// How much worse the leading axle has it than the ones behind it.
///
/// The first axle runs on the rail as it finds it — damp, dusty, greasy — and wipes it as
/// it goes; every axle behind it runs on a rail that has already been cleaned. That is why
/// the leading axle of a locomotive is the one that spins, and it is the whole reason
/// modelling axles separately buys anything: with the same coefficient everywhere and the
/// torque shared by weight, every driven axle would reach its limit in the same instant.
///
/// The factors are normalised against the vehicle's own load distribution, so the total
/// adhesive force is exactly what it was before the axles were told apart — only its
/// distribution changes.
///
/// ponytail: one exponential instead of a contamination model. The shape is what the
/// measurements agree on (first axle 10–30 % down, recovered within three or four axles);
/// the exact figure depends on things no data sheet states.
pub fn rail_cleaning(axles: &[AxleState]) -> Vec<f64> {
    /// How far down the leading axle is before the normalisation.
    const DEPTH: f64 = 0.18;
    /// Axles it takes to recover, as the decay constant of the exponential.
    const RECOVERY: f64 = 1.5;

    let raw: Vec<f64> = (0..axles.len())
        .map(|i| 1.0 - DEPTH * (-(i as f64) / RECOVERY).exp())
        .collect();
    let mean: f64 = axles
        .iter()
        .zip(&raw)
        .map(|(a, f)| a.spec.load_share * f)
        .sum();
    if mean <= 0.0 {
        return vec![1.0; axles.len()];
    }
    raw.into_iter().map(|f| f / mean).collect()
}

/// Curve resistance after Röckl [N] for mass `m` [kg] and curvature `k` [1/m].
///
/// `axle_base_sum` [m] is the total axle base of the vehicle; it is what forces the axles
/// in a curve. Röckl's constants are calibrated for a bogie vehicle with about 5 m
/// ([`REFERENCE_AXLE_BASE`](crate::train::REFERENCE_AXLE_BASE)), so the value is scaled
/// against that reference; 0 means "not stated" and leaves Röckl untouched.
///
/// ponytail: linear scaling instead of a flange friction model — the empirical
/// wheel/rail part of Röckl dominates anyway. A real model needs the bogie geometry
/// (axle base per bogie, pivot spacing), which no data sheet states either.
pub fn curve_resistance(m: f64, k: f64, axle_base_sum: f64) -> f64 {
    let radius = if k.abs() < 1e-9 {
        return 0.0;
    } else {
        1.0 / k.abs()
    };
    let specific = if radius >= 300.0 {
        650.0 / (radius - 55.0)
    } else {
        500.0 / (radius - 30.0).max(1.0)
    };
    let scale = if axle_base_sum > 0.0 {
        (axle_base_sum / crate::train::REFERENCE_AXLE_BASE).clamp(0.5, 2.0)
    } else {
        1.0
    };
    m * G / 1000.0 * specific * scale
}

/// One step of the longitudinal dynamics.
///
/// `dt` should be small enough for the coupler stiffness (200 Hz is sufficient for
/// screw couplers); the caller may use substepping.
pub fn step(train: &mut Train, net: &TrackNetwork, dt: f64) -> StepReport {
    let mut report = StepReport::default();
    let n = train.vehicles.len();
    if n == 0 {
        return report;
    }

    // 1. Coupler forces from deflection and relative speed.
    for i in 0..n - 1 {
        if train.couplers[i].broken {
            train.couplers[i].force = 0.0;
            continue;
        }
        let (a, b) = (&train.vehicles[i], &train.vehicles[i + 1]);
        let nominal = (a.spec.length + b.spec.length) / 2.0;
        let extension = (a.x - b.x) - nominal;
        let spec = a.spec.coupler;
        let half_slack = spec.slack / 2.0;
        let mut force = if extension > half_slack {
            spec.draw_stiffness * (extension - half_slack)
        } else if extension < -half_slack {
            spec.buffer_stiffness * (extension + half_slack)
        } else {
            0.0
        };
        if force != 0.0 {
            force += spec.damping * (a.v - b.v);
        }
        train.couplers[i].extension = extension;
        if force.abs() > spec.breaking_force {
            train.couplers[i].broken = true;
            report.broken_couplers.push(i);
            force = 0.0;
        }
        train.couplers[i].force = force;
    }

    // 2. Forces per vehicle and integration of the speed.
    for i in 0..n {
        let coupler_front = if i > 0 {
            train.couplers[i - 1].force
        } else {
            0.0
        };
        let coupler_rear = if i + 1 < n {
            train.couplers[i].force
        } else {
            0.0
        };
        let rail = train.rail;
        let veh = &mut train.vehicles[i];
        let pose = veh.pos.pose(net);

        let traction = transmit_traction(veh, rail, dt);
        let braking = brake_force(veh, rail, dt);
        veh.tractive_effort = traction;
        veh.brake_effort = braking;
        let resistance = veh.spec.resistance(veh.v);
        let grade = veh.mass() * G * pose.grade / 1000.0;
        let curve = curve_resistance(veh.mass(), pose.curvature, veh.spec.axle_base_sum)
            * veh.spec.curve_resistance_factor;

        // Resistances always act against the motion, and so does the brake.
        let dir = if veh.v.abs() < 1e-4 {
            0.0
        } else {
            veh.v.signum()
        };
        let mut force = traction - grade + coupler_front - coupler_rear;
        let opposing = resistance + curve + braking;
        if dir != 0.0 {
            force -= dir * opposing;
        } else {
            // Standstill: static friction/brake hold as long as the residual force is smaller.
            if force.abs() <= opposing {
                force = 0.0;
            } else {
                force -= force.signum() * opposing;
            }
        }

        let a = force / veh.inertial_mass();
        let v_new = veh.v + a * dt;
        // Do not let the brake force overshoot through zero.
        let v_old = veh.v;
        veh.v = if dir != 0.0 && v_new * dir < 0.0 && traction.abs() < opposing {
            0.0
        } else {
            v_new
        };
        // What was actually applied, not what the forces asked for — the standstill hold
        // and the zero crossing above are part of the answer the network extrapolates with.
        veh.a = (veh.v - v_old) / dt;
    }

    // 3. Advance the positions.
    for i in 0..n {
        let veh = &mut train.vehicles[i];
        let dx = veh.v * dt;
        veh.x += dx;
        let mut passed = Vec::new();
        if let Err(e) = veh.pos.advance(net, dx, &mut passed) {
            report.blocked = Some(e);
            veh.v = 0.0;
        }
        for p in passed {
            report.passed.push((i, p));
        }
    }
    if report.blocked.is_some() {
        for v in &mut train.vehicles {
            v.v = 0.0;
            v.a = 0.0;
        }
    }
    report
}

/// Tractive effort limited by adhesion, axle by axle (plan ch. 6).
///
/// Every driven axle gets its share of the demand and has its own adhesion limit from the
/// weight it carries. That is why a locomotive on a greasy rail loses *an* axle rather
/// than all of them: the others keep pulling, and what the driver feels is the effort
/// stepping down, not vanishing.
///
/// The wheel slip protection answers per axle too, which is what the three kinds actually
/// differ in — the slip brake takes the one spinning wheelset down, the cutback throttles
/// the whole drive because it has only one handle to throttle, and creep control holds
/// every axle at the maximum of its own adhesion curve.
fn transmit_traction(veh: &mut Vehicle, rail: RailCondition, dt: f64) -> f64 {
    let protection = veh.spec.slip_protection;
    let mu = vehicle_adhesion(veh, rail) * protection.adhesion_bonus();
    let cleaning = rail_cleaning(&veh.axles);
    let mass = veh.mass();
    let inertia = veh.inertial_mass();
    let demand = veh.traction.force;

    // Dynamic braking through the traction motors runs through `brake_force`; here the
    // negative case only has to stay inside the driven axles' adhesion.
    if demand <= 0.0 {
        let mut total = 0.0;
        for (i, axle) in veh.axles.iter_mut().enumerate() {
            axle.tractive_effort = 0.0;
            axle.slip -= axle.slip.signum() * (3.0 * dt).min(axle.slip.abs());
            if axle.spec.driven {
                total += mu * cleaning[i] * mass * axle.spec.load_share * G;
            }
        }
        veh.slip = peak_slip(&veh.axles);
        return demand.max(-total);
    }

    // The drive shares its effort over the driven axles by the weight they carry — a
    // common shaft would equalise the torque, and the load share is what that comes to.
    let driven: f64 = veh
        .axles
        .iter()
        .filter(|a| a.spec.driven)
        .map(|a| a.spec.load_share)
        .sum();
    if driven <= 0.0 {
        veh.slip = 0.0;
        return 0.0;
    }

    // A cutback has one handle for the whole drive, so it needs the worst axle first.
    let cutback = protection == SlipProtection::TractionCutback
        && veh.axles.iter().any(|a| a.spec.driven && a.slip > 0.3);

    let mut total = 0.0;
    for (i, axle) in veh.axles.iter_mut().enumerate() {
        if !axle.spec.driven {
            axle.tractive_effort = 0.0;
            continue;
        }
        let share = axle.spec.load_share / driven;
        let mut want = demand * share;
        if cutback {
            want *= 0.6;
        }
        let limit = mu * cleaning[i] * mass * axle.spec.load_share * G;
        let transmitted = if want > limit && limit > 0.0 {
            // Excess force accelerates this wheelset — the slip is its own.
            axle.slip += (want - limit) / (inertia * share.max(1e-6)) * dt;
            let mut transmitted = limit * 0.9; // sliding friction < static friction
            match protection {
                SlipProtection::None => {}
                // The wheel slip brake takes the spinning wheelset down. That costs effort
                // on that axle, but it catches the slip in a fraction of the time.
                SlipProtection::SlipBrake if axle.slip > 0.1 => {
                    transmitted *= 0.8;
                    axle.slip = (axle.slip - 9.0 * dt).max(0.0);
                }
                SlipProtection::TractionCutback => {}
                // Creep control does not avoid the slip, it lives in it — and therefore
                // stays right at the maximum of the adhesion curve.
                SlipProtection::CreepControl => {
                    transmitted = limit * 0.98;
                    axle.slip = axle.slip.min(0.15);
                }
                _ => {}
            }
            transmitted
        } else {
            axle.slip = (axle.slip - 3.0 * dt).max(0.0);
            want
        };
        axle.tractive_effort = transmitted;
        total += transmitted;
    }
    veh.slip = peak_slip(&veh.axles);
    total
}

/// Brake force limited by adhesion, axle by axle, including blending with the dynamic
/// brake. An axle that starts to slide is released on its own, which is what a wheel slide
/// protection does and why it saves the wheel flat on that axle alone.
fn brake_force(veh: &mut Vehicle, rail: RailCondition, dt: f64) -> f64 {
    let mu = vehicle_adhesion(veh, rail);
    let cleaning = rail_cleaning(&veh.axles);
    let mass = veh.mass();
    let inertia = veh.inertial_mass();
    // The dynamic brake only acts on the driven axles, so it has only their adhesion.
    let driven: f64 = veh
        .axles
        .iter()
        .filter(|a| a.spec.driven)
        .map(|a| a.spec.load_share)
        .sum();
    let dynamic = veh
        .brake
        .dynamic_force
        .min(mu * mass * driven * G * veh.spec.slip_protection.adhesion_bonus());
    // Blending: an air supplement brake adds to the dynamic brake (the pneumatic part has
    // already been reduced by it), otherwise the dynamic brake replaces the air brake on
    // the powered vehicle — else the loco would be overbraked within the consist.
    let demand = if veh.spec.brake.supplement_brake {
        veh.brake.force + dynamic
    } else {
        veh.brake.force.max(dynamic)
    };
    // The magnetic track brake acts on the rail, not through the wheel adhesion, so it is
    // taken out before the axles are asked and put back afterwards.
    let mg = if veh.brake.mg_applied {
        veh.spec.brake.mg_force
    } else {
        0.0
    };
    let wheel_demand = (demand - mg).max(0.0);
    let protects = veh.spec.slip_protection.protects();

    let mut total = 0.0;
    for (i, axle) in veh.axles.iter_mut().enumerate() {
        let want = wheel_demand * axle.spec.load_share;
        let limit = mu * cleaning[i] * mass * axle.spec.load_share * G;
        let applied = if want > limit && limit > 0.0 {
            // Wheel slide on this axle: its protection briefly releases this brake.
            axle.slip -= (want - limit) / (inertia * axle.spec.load_share.max(1e-6)) * dt;
            if protects { limit * 0.85 } else { limit * 0.7 }
        } else {
            if axle.slip < 0.0 {
                axle.slip = (axle.slip + 3.0 * dt).min(0.0);
            }
            want
        };
        axle.brake_effort = applied;
        total += applied;
    }
    veh.slip = peak_slip(&veh.axles);
    total + mg
}

/// The axle the vehicle is worst off on — what the HUD, the sound and the scoring read,
/// because a train with one axle spinning is a train that is spinning.
fn peak_slip(axles: &[AxleState]) -> f64 {
    axles.iter().map(|a| a.slip).fold(0.0, |worst: f64, slip| {
        if slip.abs() > worst.abs() {
            slip
        } else {
            worst
        }
    })
}

#[cfg(test)]
mod axle_tests {
    use super::*;
    use crate::brakes::{BrakeKind, BrakeSpec};
    use crate::train::{AxleSpec, Vehicle, VehicleSpec};
    use track_model::{EdgeId, TrackPosition};

    /// A Bo'2' vehicle: two driven axles leading, two carrying — and the driven pair
    /// deliberately made to carry more, as a locomotive's do.
    fn vehicle() -> Vehicle {
        let spec = VehicleSpec {
            mass_empty: 80_000.0,
            axles: 4,
            adhesive_mass_fraction: 0.6,
            brake: BrakeSpec::from_brake_weight(60.0, BrakeKind::Disc),
            running_gear: vec![
                AxleSpec {
                    driven: true,
                    load_share: 0.3,
                },
                AxleSpec {
                    driven: true,
                    load_share: 0.3,
                },
                AxleSpec {
                    driven: false,
                    load_share: 0.2,
                },
                AxleSpec {
                    driven: false,
                    load_share: 0.2,
                },
            ],
            ..VehicleSpec::default()
        };
        Vehicle::new(spec, TrackPosition::new(EdgeId(0), 0.0, 1))
    }

    #[test]
    fn the_layout_reproduces_the_adhesive_mass_a_data_sheet_states() {
        for (axles, fraction) in [(4u8, 1.0), (6, 1.0), (4, 0.5), (10, 0.5), (2, 0.0)] {
            let layout = AxleSpec::layout(axles, fraction);
            assert_eq!(layout.len(), axles as usize);
            let total: f64 = layout.iter().map(|a| a.load_share).sum();
            assert!((total - 1.0).abs() < 1e-12, "{axles}/{fraction}: {total}");
            let driven: f64 = layout
                .iter()
                .filter(|a| a.driven)
                .map(|a| a.load_share)
                .sum();
            assert!(
                (driven - fraction).abs() < 1e-12,
                "{axles} axles at {fraction}: {driven}"
            );
        }
        // A count that does not divide the fraction evenly still lands on it exactly.
        let odd = AxleSpec::layout(5, 0.55);
        let driven: f64 = odd.iter().filter(|a| a.driven).map(|a| a.load_share).sum();
        assert!((driven - 0.55).abs() < 1e-12, "{driven}");
        assert_eq!(AxleSpec::layout(0, 1.0).len(), 0);
    }

    #[test]
    fn only_the_driven_axles_take_traction() {
        let mut veh = vehicle();
        veh.v = 10.0;
        veh.traction.force = 60_000.0;
        transmit_traction(&mut veh, RailCondition::Dry, 1.0 / 200.0);
        assert!(veh.axles[0].tractive_effort > 0.0);
        assert!(veh.axles[1].tractive_effort > 0.0);
        assert_eq!(veh.axles[2].tractive_effort, 0.0);
        assert_eq!(veh.axles[3].tractive_effort, 0.0);
        // Shared by the weight they carry, so equally loaded axles pull equally.
        assert!((veh.axles[0].tractive_effort - veh.axles[1].tractive_effort).abs() < 1e-9);
    }

    #[test]
    fn the_leading_axle_runs_on_a_dirtier_rail_than_the_ones_behind_it() {
        let veh = vehicle();
        let cleaning = rail_cleaning(&veh.axles);
        assert_eq!(cleaning.len(), 4);
        for pair in cleaning.windows(2) {
            assert!(pair[0] < pair[1], "{cleaning:?}");
        }
        // …and the vehicle as a whole has exactly the adhesion it had before the axles
        // were told apart, so nothing that was calibrated against it moves.
        let mean: f64 = veh
            .axles
            .iter()
            .zip(&cleaning)
            .map(|(a, f)| a.spec.load_share * f)
            .sum();
        assert!((mean - 1.0).abs() < 1e-12, "{mean}");
    }

    /// A vehicle with all four axles alike and driven, so only the rail decides.
    fn loco() -> Vehicle {
        let spec = VehicleSpec {
            mass_empty: 80_000.0,
            axles: 4,
            adhesive_mass_fraction: 1.0,
            brake: BrakeSpec::from_brake_weight(60.0, BrakeKind::Disc),
            ..VehicleSpec::default()
        };
        Vehicle::new(spec, TrackPosition::new(EdgeId(0), 0.0, 1))
    }

    /// The point of the whole thing: the axle that has lost the rail loses *its* effort,
    /// and the ones behind it go on pulling.
    #[test]
    fn the_leading_axle_spins_first_and_the_others_keep_pulling() {
        let mut veh = loco();
        veh.v = 5.0;
        // Between the leading axle's limit and the trailing one's.
        veh.traction.force = 215_000.0;
        for _ in 0..200 {
            transmit_traction(&mut veh, RailCondition::Dry, 1.0 / 200.0);
        }
        assert!(
            veh.axles[0].slip > 0.05,
            "the leading axle must spin: {:.3} m/s",
            veh.axles[0].slip
        );
        assert_eq!(veh.axles[3].slip, 0.0, "the last axle must hold");
        assert!(
            veh.axles[3].tractive_effort > veh.axles[0].tractive_effort,
            "{:.0} vs {:.0} N",
            veh.axles[3].tractive_effort,
            veh.axles[0].tractive_effort
        );
        // The vehicle reports the worst axle, which is what the HUD and the sound read.
        assert_eq!(veh.slip, veh.axles[0].slip);
        // And it still pulls: losing one axle is not losing the locomotive.
        let total: f64 = veh.axles.iter().map(|a| a.tractive_effort).sum();
        assert!(total > 150_000.0, "{total:.0} N");
    }

    #[test]
    fn the_wheel_slide_protection_releases_the_sliding_axle_alone() {
        let mut veh = loco();
        veh.spec.slip_protection = crate::brakes::SlipProtection::CreepControl;
        veh.v = 20.0;
        // Between the leading axle's limit and the trailing one's, again.
        veh.brake.force = 170_000.0;
        for _ in 0..200 {
            brake_force(&mut veh, RailCondition::Dry, 1.0 / 200.0);
        }
        assert!(
            veh.axles[0].slip < -0.01,
            "the leading axle must slide: {:.3} m/s",
            veh.axles[0].slip
        );
        assert_eq!(veh.axles[3].slip, 0.0, "the last axle must hold");
        assert!(
            veh.axles[3].brake_effort > veh.axles[0].brake_effort,
            "the sliding axle is the one that gets released"
        );
    }

    #[test]
    fn a_vehicle_that_states_nothing_still_gets_axles() {
        let spec = VehicleSpec {
            axles: 4,
            adhesive_mass_fraction: 1.0,
            ..VehicleSpec::default()
        };
        let veh = Vehicle::new(spec, TrackPosition::new(EdgeId(0), 0.0, 1));
        assert_eq!(veh.axles.len(), 4);
        assert!(veh.axles.iter().all(|a| a.spec.driven));
        assert!((veh.adhesive_mass() - veh.mass()).abs() < 1e-9);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::train::{AIR_DENSITY, REFERENCE_AXLE_BASE};

    #[test]
    fn curve_resistance_grows_with_the_axle_base() {
        let k = 1.0 / 500.0;
        let reference = curve_resistance(40_000.0, k, REFERENCE_AXLE_BASE);
        // Not stated == reference vehicle.
        assert!((curve_resistance(40_000.0, k, 0.0) - reference).abs() < 1e-9);
        // A two-axle wagon with a short axle base forces less, a long one more.
        assert!(curve_resistance(40_000.0, k, 3.0) < reference);
        assert!(curve_resistance(40_000.0, k, 8.0) > reference);
        // Straight track has no curve resistance.
        assert_eq!(curve_resistance(40_000.0, 0.0, 5.0), 0.0);
    }

    #[test]
    fn cw_a_replaces_the_quadratic_davis_term() {
        let mut spec = crate::train::VehicleSpec {
            cw_a: None,
            ..crate::train::VehicleSpec::default()
        };
        spec.davis = crate::train::Davis {
            a: 1_000.0,
            b: 10.0,
            c: 5.0,
        };
        let v = 40.0;
        assert_eq!(spec.resistance(v), 1_000.0 + 10.0 * v + 5.0 * v * v);

        spec.cw_a = Some(10.0);
        let expected = 1_000.0 + 10.0 * v + 0.5 * AIR_DENSITY * 10.0 * v * v;
        assert!((spec.resistance(v) - expected).abs() < 1e-9);
    }
}
