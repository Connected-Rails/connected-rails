//! Longitudinal dynamics of the train consist (plan ch. 6).
//!
//! Every vehicle is a point mass on the track; neighbours are coupled by spring-dampers
//! with slack. Integration: semi-implicit Euler with a fixed step size.

use crate::G;
use crate::brakes::SlipProtection;
use crate::train::{RailCondition, Train, Vehicle};
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
pub fn adhesion_coefficient(v_kmh: f64, rail: RailCondition, sanding: bool) -> f64 {
    let base = 7.5 / (v_kmh.abs() + 44.0) + 0.161;
    let sand = if sanding { 1.25 } else { 1.0 };
    base * rail.factor() * sand
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
        veh.v = if dir != 0.0 && v_new * dir < 0.0 && traction.abs() < opposing {
            0.0
        } else {
            v_new
        };
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
        }
    }
    report
}

/// Tractive effort limited by adhesion; produces wheel slip and answers it the way the
/// vehicle's wheel slip protection would (plan ch. 6).
fn transmit_traction(veh: &mut Vehicle, rail: RailCondition, dt: f64) -> f64 {
    let protection = veh.spec.slip_protection;
    let demand = veh.traction.force.max(0.0);
    if demand <= 0.0 {
        veh.slip = (veh.slip - 3.0 * dt).max(0.0);
        return veh.traction.force.min(0.0).max(-limit(veh, rail));
    }
    let limit = limit(veh, rail);
    if demand > limit && limit > 0.0 {
        // Excess force accelerates the driven wheelsets → slip grows.
        veh.slip += (demand - limit) / veh.inertial_mass() * dt;
        let mut transmitted = limit * 0.9; // sliding friction < static friction
        match protection {
            SlipProtection::None => {}
            // The wheel slip brake takes the spinning wheelset down. That costs effort, but
            // it catches the slip in a fraction of the time.
            SlipProtection::SlipBrake if veh.slip > 0.1 => {
                transmitted *= 0.8;
                veh.slip = (veh.slip - 9.0 * dt).max(0.0);
            }
            // Throttling: the drive cuts its own effort right back and feels its way up again.
            SlipProtection::TractionCutback if veh.slip > 0.3 => transmitted *= 0.6,
            // Creep control does not avoid the slip, it lives in it — and therefore stays
            // right at the maximum of the adhesion curve.
            SlipProtection::CreepControl => {
                transmitted = limit * 0.98;
                veh.slip = veh.slip.min(0.15);
            }
            _ => {}
        }
        transmitted
    } else {
        veh.slip = (veh.slip - 3.0 * dt).max(0.0);
        demand
    }
}

/// Brake force limited by adhesion, including blending with the dynamic brake.
fn brake_force(veh: &mut Vehicle, rail: RailCondition, dt: f64) -> f64 {
    let mu = adhesion_coefficient(veh.v * 3.6, rail, veh.sanding);
    // The dynamic brake only acts on the driven axles, so it has only their adhesion.
    let dynamic = veh
        .brake
        .dynamic_force
        .min(mu * veh.adhesive_mass() * G * veh.spec.slip_protection.adhesion_bonus());
    // Blending: an air supplement brake adds to the dynamic brake (the pneumatic part has
    // already been reduced by it), otherwise the dynamic brake replaces the air brake on
    // the powered vehicle — else the loco would be overbraked within the consist.
    let mut f = if veh.spec.brake.supplement_brake {
        veh.brake.force + dynamic
    } else {
        veh.brake.force.max(dynamic)
    };
    // The magnetic track brake acts on the rail, not through the wheel adhesion.
    let adhesion_bound = mu * veh.mass() * G;
    let mg = if veh.brake.mg_applied {
        veh.spec.brake.mg_force
    } else {
        0.0
    };
    let wheel = (f - mg).max(0.0);
    if wheel > adhesion_bound {
        // Wheel slide: the wheel slide protection briefly releases the brake.
        veh.slip -= (wheel - adhesion_bound) / veh.inertial_mass() * dt;
        f = if veh.spec.slip_protection.protects() {
            adhesion_bound * 0.85 + mg
        } else {
            adhesion_bound * 0.7 + mg
        };
    } else if veh.slip < 0.0 {
        veh.slip = (veh.slip + 3.0 * dt).min(0.0);
    }
    f
}

fn limit(veh: &Vehicle, rail: RailCondition) -> f64 {
    adhesion_coefficient(veh.v * 3.6, rail, veh.sanding)
        * veh.adhesive_mass()
        * G
        * veh.spec.slip_protection.adhesion_bonus()
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
