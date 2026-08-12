//! Längsdynamik des Zugverbands (Plan Kap. 6).
//!
//! Jedes Fahrzeug ist ein Massenpunkt auf dem Gleis; Nachbarn sind über Feder-Dämpfer
//! mit Spiel gekoppelt. Integration: semi-implizites Euler mit fester Schrittweite.

use crate::G;
use crate::train::{RailCondition, Train, Vehicle};
use track_model::{AdvanceError, PassedDevice, TrackNetwork};

/// Ergebnis eines Physikschritts.
#[derive(Debug, Default, Clone)]
pub struct StepReport {
    /// Überfahrene Streckengeräte, je Fahrzeugindex.
    pub passed: Vec<(usize, PassedDevice)>,
    /// Zug ist an einem nicht befahrbaren Knoten aufgelaufen.
    pub blocked: Option<AdvanceError>,
    /// Gerissene Kupplungen (Index in `Train::couplers`).
    pub broken_couplers: Vec<usize>,
}

/// Kraftschlussbeiwert nach Curtius/Kniffler, mit Schienenzustand und Sanden.
pub fn adhesion_coefficient(v_kmh: f64, rail: RailCondition, sanding: bool) -> f64 {
    let base = 7.5 / (v_kmh.abs() + 44.0) + 0.161;
    let sand = if sanding { 1.25 } else { 1.0 };
    base * rail.factor() * sand
}

/// Bogenwiderstand nach Röckl [N] für Masse `m` [kg] und Krümmung `k` [1/m].
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

/// Ein Schritt der Längsdynamik.
///
/// `dt` sollte klein genug für die Kupplungssteifigkeit sein (200 Hz sind für
/// Schraubenkupplungen ausreichend); Aufrufer kann substepping betreiben.
pub fn step(train: &mut Train, net: &TrackNetwork, dt: f64) -> StepReport {
    let mut report = StepReport::default();
    let n = train.vehicles.len();
    if n == 0 {
        return report;
    }

    // 1. Kupplungskräfte aus Auslenkung und Relativgeschwindigkeit.
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

    // 2. Kräfte je Fahrzeug und Integration der Geschwindigkeit.
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

        // Widerstände wirken immer gegen die Bewegung, Bremse ebenso.
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
            // Stillstand: Haftreibung/Bremse halten, solange die Restkraft kleiner ist.
            if force.abs() <= opposing {
                force = 0.0;
            } else {
                force -= force.signum() * opposing;
            }
        }

        let a = force / veh.inertial_mass();
        let v_new = veh.v + a * dt;
        // Nulldurchgang durch Bremskraft nicht überschießen lassen.
        veh.v = if dir != 0.0 && v_new * dir < 0.0 && traction.abs() < opposing {
            0.0
        } else {
            v_new
        };
    }

    // 3. Positionen fortschreiben.
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

/// Zugkraft nach Kraftschlussgrenze; erzeugt Schleudern und regelt es ggf. ab.
fn transmit_traction(veh: &mut Vehicle, rail: RailCondition, dt: f64) -> f64 {
    let demand = veh.traction.force.max(0.0);
    if demand <= 0.0 {
        veh.slip = (veh.slip - 3.0 * dt).max(0.0);
        return veh.traction.force.min(0.0).max(-limit(veh, rail));
    }
    let limit = limit(veh, rail);
    if demand > limit && limit > 0.0 {
        // Überschusskraft beschleunigt die Treibradsätze → Schlupf wächst.
        veh.slip += (demand - limit) / veh.inertial_mass() * dt;
        let mut transmitted = limit * 0.9; // Gleitreibung < Haftreibung
        if veh.spec.slip_control && veh.slip > 0.3 {
            // Schleuderschutz nimmt Zugkraft zurück, bis der Schlupf abgebaut ist.
            transmitted *= 0.6;
        }
        transmitted
    } else {
        veh.slip = (veh.slip - 3.0 * dt).max(0.0);
        demand
    }
}

/// Bremskraft nach Kraftschlussgrenze inkl. Blending mit der elektrischen Bremse.
fn brake_force(veh: &mut Vehicle, rail: RailCondition, dt: f64) -> f64 {
    // Blending: die E-Bremse ersetzt die Druckluftbremse am Triebfahrzeug, sie addiert
    // sich nicht dazu (sonst würde das Tfz im Zugverband überbremst).
    let electric = (-veh.traction.force).max(0.0);
    let mut f = veh.brake.force.max(electric);
    // Magnetschienenbremse wirkt schienengebunden, nicht über den Radkraftschluss.
    let adhesion_bound = adhesion_coefficient(veh.v * 3.6, rail, veh.sanding) * veh.mass() * G;
    let mg = if veh.brake.mg_applied {
        veh.spec.brake.mg_force
    } else {
        0.0
    };
    let wheel = (f - mg).max(0.0);
    if wheel > adhesion_bound {
        // Gleiten: der Gleitschutz löst die Bremse kurz an.
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
