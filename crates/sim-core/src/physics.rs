//! Longitudinal dynamics of the train consist (plan ch. 6).
//!
//! Every vehicle is a point mass on the track; neighbours are coupled by spring-dampers
//! with slack. Integration: semi-implicit Euler with a fixed step size.

use crate::G;
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
pub fn curve_resistance(m: f64, k: f64) -> f64 {
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
    m * G / 1000.0 * specific
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
        let resistance = veh.spec.davis.resistance(veh.v);
        let grade = veh.mass() * G * pose.grade / 1000.0;
        let curve = curve_resistance(veh.mass(), pose.curvature);

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

/// Tractive effort limited by adhesion; produces wheel slip and reduces it if necessary.
fn transmit_traction(veh: &mut Vehicle, rail: RailCondition, dt: f64) -> f64 {
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
        if veh.spec.slip_control && veh.slip > 0.3 {
            // Wheel slip protection reduces the tractive effort until the slip is gone.
            transmitted *= 0.6;
        }
        transmitted
    } else {
        veh.slip = (veh.slip - 3.0 * dt).max(0.0);
        demand
    }
}

/// Brake force limited by adhesion, including blending with the dynamic brake.
fn brake_force(veh: &mut Vehicle, rail: RailCondition, dt: f64) -> f64 {
    // Blending: the dynamic brake replaces the air brake on the powered vehicle, it does
    // not add to it (otherwise the loco would be overbraked within the consist).
    let electric = (-veh.traction.force).max(0.0);
    let mut f = veh.brake.force.max(electric);
    // The magnetic track brake acts on the rail, not through the wheel adhesion.
    let adhesion_bound = adhesion_coefficient(veh.v * 3.6, rail, veh.sanding) * veh.mass() * G;
    let mg = if veh.brake.mg_applied {
        veh.spec.brake.mg_force
    } else {
        0.0
    };
    let wheel = (f - mg).max(0.0);
    if wheel > adhesion_bound {
        // Wheel slide: the wheel slide protection briefly releases the brake.
        veh.slip -= (wheel - adhesion_bound) / veh.inertial_mass() * dt;
        f = if veh.spec.slip_control {
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
    adhesion_coefficient(veh.v * 3.6, rail, veh.sanding) * veh.adhesive_mass() * G
}
