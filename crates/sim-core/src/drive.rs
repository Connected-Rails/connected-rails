//! Drive models (plan ch. 8): what turns the wheels, parametrised from the data sheet.
//!
//! Four families live in [`TractionSpec`]:
//!
//! | variant | what it models |
//! |---|---|
//! | `Curve` | the simplified model — tractive effort read off the diagram over speed |
//! | `TapChanger` | transformer + tap changer with series-wound motors (BR 110/140) |
//! | `Converter` | three-phase drive behind a converter (BR 101/185/423) |
//! | `Diesel` | diesel engine with a hydraulic transmission (BR 218) |
//!
//! The detailed data — motor, engine map, transmission — is an `Option` on the variant.
//! Without it the variant falls back to the tractive effort hyperbola from
//! `max_force`/`max_power`, so a vehicle can start out coarse and grow data later without
//! changing its type.
//!
//! The contact line is German: 15 kV 16.7 Hz, see [`crate::electric`].

use serde::{Deserialize, Serialize};
use std::f64::consts::TAU;

/// Number of hydraulic circuits a transmission may have.
/// (Voith transmissions have at most three: starting converter, running converter, coupling.)
pub const MAX_CIRCUITS: usize = 4;

/// Linear interpolation over a table of `(x, y)` pairs sorted by `x`.
/// Outside the table the first / last value is held.
pub fn interpolate(points: &[(f64, f64)], x: f64) -> f64 {
    let Some(&(first_x, first_y)) = points.first() else {
        return 0.0;
    };
    if x <= first_x {
        return first_y;
    }
    for pair in points.windows(2) {
        let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
        if x <= x1 {
            let span = x1 - x0;
            let t = if span.abs() < 1e-9 {
                0.0
            } else {
                (x - x0) / span
            };
            return y0 + t * (y1 - y0);
        }
    }
    points[points.len() - 1].1
}

/// Quantises `x` (0…1) into `steps` steps.
///
/// `0` is continuous, `1` is plain on/off — that is exactly the range from a
/// quasi-continuously filled torque converter down to a switched one.
pub fn quantise(x: f64, steps: u32) -> f64 {
    let x = x.clamp(0.0, 1.0);
    if steps == 0 {
        x
    } else {
        (x * steps as f64).round() / steps as f64
    }
}

/// Series-wound traction motor (Reihenschlussmotor) — the data sheet's motor figures.
///
/// The characteristic follows from the machine equations, it is not a curve:
/// `U = I·R + kΦ(I)·ω` with a saturating flux `kΦ(I) = c·I/(1 + I/I_sat)`, torque
/// `M = kΦ(I)·I`. That is what makes a series motor pull hard at a stand and run away
/// unloaded — no table can express it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeriesMotor {
    /// Number of motors in the vehicle.
    pub count: u32,
    /// Resistance of armature and field winding together [Ω].
    pub resistance: f64,
    /// Machine constant `c` [V·s/A]: flux linkage per ampere in the unsaturated range.
    pub flux_constant: f64,
    /// Current at which the iron saturates [A].
    pub saturation_current: f64,
    /// Highest permissible current [A] — the current limit relay holds the notch back here.
    pub max_current: f64,
    /// Motor voltage at the top notch [V].
    pub max_voltage: f64,
    /// Field weakening stages as a share of the full field, strongest field first.
    pub field_steps: Vec<f64>,
    /// Gear ratio motor : wheelset.
    pub gear_ratio: f64,
    /// Wheel diameter [m].
    pub wheel_diameter: f64,
    /// Efficiency of gearing and motor.
    pub efficiency: f64,
}

impl SeriesMotor {
    /// Flux linkage kΦ [V·s] at armature current `i` [A] and field factor `field`.
    fn flux(&self, i: f64, field: f64) -> f64 {
        self.flux_constant * i / (1.0 + i / self.saturation_current.max(1.0)) * field
    }

    /// Armature current [A] at terminal voltage `u` [V] and angular velocity `omega` [rad/s].
    ///
    /// `U = I·R + kΦ(I)·ω` grows strictly monotonically in `I`, so bisection converges;
    /// 30 halvings resolve the search range to well below a milliampere.
    fn current(&self, u: f64, omega: f64, field: f64) -> f64 {
        let (mut lo, mut hi) = (0.0, self.max_current * 4.0);
        for _ in 0..30 {
            let mid = 0.5 * (lo + hi);
            if mid * self.resistance + self.flux(mid, field) * omega < u {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        0.5 * (lo + hi)
    }

    /// Tractive effort [N] and armature current [A] at speed `v` [m/s], with the tap
    /// changer at `ratio` (0…1) of the full voltage and the field at `field`.
    pub fn effort(&self, v: f64, ratio: f64, field: f64) -> (f64, f64) {
        let radius = (self.wheel_diameter / 2.0).max(0.05);
        let omega = v.abs() / radius * self.gear_ratio;
        let u = self.max_voltage * ratio.clamp(0.0, 1.0);
        let current = self.current(u, omega, field).min(self.max_current);
        let torque = self.flux(current, field) * current;
        let force = torque * self.gear_ratio / radius * self.count as f64 * self.efficiency;
        (force, current)
    }

    /// Best field stage at this operating point: the strongest field wins at a stand,
    /// the weakest one keeps the effort up at speed. Returns (force, current, field).
    pub fn best_effort(&self, v: f64, ratio: f64, power_limit: f64) -> (f64, f64, f64) {
        let mut best = (0.0, 0.0, 1.0);
        let steps: &[f64] = if self.field_steps.is_empty() {
            &[1.0]
        } else {
            &self.field_steps
        };
        for &field in steps {
            let (force, current) = self.effort(v, ratio, field);
            // The transformer's continuous rating limits the power, not the motor.
            let force = force.min(power_limit / v.abs().max(0.5));
            if force > best.0 {
                best = (force, current, field);
            }
        }
        best
    }
}

/// Dynamic brake of an electric drive (plan ch. 7: "electric brake with blending").
///
/// Rheostatic on a series-wound drive (the motors feed braking resistors), regenerative on
/// a three-phase drive (the converter feeds back into the contact line, so it needs the
/// main switch closed and line voltage present).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DynamicBrake {
    /// Highest braking force [N].
    pub max_force: f64,
    /// Highest braking power [W].
    pub max_power: f64,
    /// Below this speed the brake fades out [km/h] — it cannot hold a train at a stand.
    pub fade_out_kmh: f64,
    /// Feeds back into the contact line instead of into resistors.
    pub regenerative: bool,
    /// Rise time from zero to full braking force [s].
    pub ramp_time: f64,
}

impl DynamicBrake {
    /// Braking force available at speed `v` [m/s].
    pub fn available(&self, v: f64) -> f64 {
        let kmh = v.abs() * 3.6;
        let fade = if self.fade_out_kmh > 0.0 {
            (kmh / self.fade_out_kmh).clamp(0.0, 1.0)
        } else {
            1.0
        };
        self.max_force.min(self.max_power / v.abs().max(0.5)) * fade
    }
}

/// Governor of a diesel engine.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Governor {
    /// Speed-governed: the power controller sets a target engine speed, the governor holds
    /// it against the load by opening the fuel rack. German main line diesel locos.
    Speed {
        /// Notches of the power controller; 0 = continuous.
        steps: u32,
        /// Droop: share of the rated speed the set speed sags by between no load and full
        /// rack. 0 is an isochronous governor, the original is 3…5 %.
        #[serde(default)]
        droop: f64,
    },
    /// Fill-governed: the power controller sets the fuel rack directly, the engine speed
    /// follows from the load. Shunting locos and railcars with mechanical injection pumps.
    Fill,
}

/// Diesel engine — the map from the data sheet plus its governor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DieselEngine {
    /// Idling speed [1/min].
    pub idle_rpm: f64,
    /// Rated speed [1/min].
    pub rated_rpm: f64,
    /// Speed at which the overspeed governor cuts in [1/min].
    pub max_rpm: f64,
    /// Full load torque over engine speed [(1/min, N·m)] — the data sheet's map.
    pub torque_curve: Vec<(f64, f64)>,
    pub governor: Governor,
    /// Moment of inertia of the rotating parts incl. flywheel [kg·m²].
    pub inertia: f64,
    /// Time for the fuel rack to travel from idle to full load [s].
    pub response_time: f64,
}

impl DieselEngine {
    /// Full load torque [N·m] at engine speed `rpm`.
    pub fn full_load_torque(&self, rpm: f64) -> f64 {
        interpolate(&self.torque_curve, rpm).max(0.0)
    }
}

/// What sits in a hydraulic circuit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CircuitKind {
    /// Torque converter: multiplies the torque, the multiplication falls with the speed ratio.
    Converter,
    /// Fluid coupling: transmits the torque one to one, absorption falls with the slip.
    Coupling,
}

/// One hydraulic circuit of the transmission.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Circuit {
    pub kind: CircuitKind,
    /// Gear ratio turbine : transmission output for this circuit.
    pub ratio: f64,
    /// Torque ratio µ at stall (ν = 0). Starting converter 2.5…4, running converter ~2,
    /// fluid coupling 1.
    pub stall_ratio: f64,
    /// Speed ratio ν = n_turbine/n_pump at which µ has fallen to 1 (the coupling point).
    pub coupling_nu: f64,
    /// Absorption λ of the pump wheel at ν = 0 [N·m/(rad/s)²] at full filling:
    /// `M_pump = λ(ν)·ω²·fill`. Set it so that the pump absorbs the engine's rated torque
    /// at rated speed.
    pub absorption: f64,
    /// Trend of λ over the speed ratio: `λ(ν) = absorption·(1 + slope·ν)`. 0 keeps λ
    /// constant, which nails a speed-governed engine to one speed parabola for the whole
    /// converter range; the original wanders, and this number is how far.
    #[serde(default)]
    pub absorption_slope: f64,
    /// Change up to the next circuit above this speed [km/h]. The last circuit ignores it.
    pub shift_up_kmh: f64,
    /// Primary influence: at the zero notch the change point sits this much lower [km/h],
    /// linearly over the notch. A BR 216 changes into the coupling around 35 km/h earlier
    /// in notch 10 than in notch 15. 0 = the change point depends on speed alone.
    #[serde(default)]
    pub shift_primary_kmh: f64,
}

impl Circuit {
    /// Torque ratio µ at speed ratio `nu`.
    pub fn torque_ratio(self, nu: f64) -> f64 {
        match self.kind {
            CircuitKind::Converter => {
                let slope = (self.stall_ratio - 1.0) / self.coupling_nu.max(0.05);
                (self.stall_ratio - slope * nu.max(0.0)).max(0.0)
            }
            CircuitKind::Coupling => 1.0,
        }
    }

    /// Torque the pump wheel takes from the engine [N·m].
    pub fn pump_torque(self, omega_engine: f64, nu: f64, fill: f64) -> f64 {
        let lambda = self.absorption * (1.0 + self.absorption_slope * nu.max(0.0)).max(0.0);
        let base = lambda * omega_engine * omega_engine * fill;
        match self.kind {
            CircuitKind::Converter => base,
            // A coupling only absorbs what it slips.
            CircuitKind::Coupling => base * (1.0 - nu).clamp(0.0, 1.0),
        }
    }
}

/// Hydraulic transmission: circuits that are engaged by filling and emptying them.
///
/// Changing gear is not a clutch operation — the outgoing circuit runs empty while the
/// incoming one fills, which is why the change takes about a second and why a partly
/// filled circuit is a perfectly good way of setting part load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transmission {
    /// Circuits in the order they are engaged; at most [`MAX_CIRCUITS`].
    pub circuits: Vec<Circuit>,
    /// Steps of the filling between empty and full: 0 = continuous, 1 = fill/empty only,
    /// larger values are the multi-stage partial filling of the original.
    pub fill_steps: u32,
    /// Time to fill a circuit [s].
    pub fill_time: f64,
    /// Time to empty a circuit [s]; 0 takes [`Transmission::fill_time`]. Emptying is the
    /// faster of the two, and the difference is what the change point feels like: the
    /// outgoing circuit lets go before the incoming one has taken hold.
    #[serde(default)]
    pub drain_time: f64,
    /// Hysteresis of the change points [km/h]: the change back down happens this much
    /// below the change-up point, so the transmission does not hunt on a gradient.
    pub hysteresis_kmh: f64,
    /// Final drive: transmission output : wheelset.
    pub final_ratio: f64,
    /// Wheel diameter [m].
    pub wheel_diameter: f64,
    /// Number of transmissions in the vehicle.
    pub count: u32,
    /// Mechanical efficiency of the gearing behind the circuit.
    pub efficiency: f64,
}

impl Transmission {
    /// Speed ratio ν and tractive effort per unit of pump torque, for circuit `index`.
    pub fn geometry(&self, index: usize, v: f64, omega_engine: f64) -> (f64, f64) {
        let circuit = self.circuits[index];
        let radius = (self.wheel_diameter / 2.0).max(0.05);
        // Rolling backwards, the turbine stands still as far as the converter is concerned:
        // stall, and that is exactly where it delivers its maximum torque.
        let omega_wheel = (v / radius).max(0.0);
        let total = circuit.ratio * self.final_ratio;
        let omega_turbine = omega_wheel * total;
        let nu = if omega_engine > 1.0 {
            omega_turbine / omega_engine
        } else {
            0.0
        };
        (nu, total / radius * self.efficiency)
    }

    /// Time to empty a circuit [s] — [`Transmission::fill_time`] where none is given.
    pub fn drain_time(&self) -> f64 {
        if self.drain_time > 0.0 {
            self.drain_time
        } else {
            self.fill_time
        }
    }

    /// Change-up speed of circuit `index` [km/h] with the power controller at `demand`.
    ///
    /// Two numbers, not one: the primary influence pulls the change point down at a low
    /// notch, so the change speed is a plane over (v, notch) rather than a line over v.
    pub fn shift_up_kmh(&self, index: usize, demand: f64) -> f64 {
        let circuit = self.circuits[index];
        (circuit.shift_up_kmh - (1.0 - demand.clamp(0.0, 1.0)) * circuit.shift_primary_kmh).max(0.0)
    }

    /// Circuit the change schedule asks for at `kmh`, ignoring hysteresis — for the
    /// steady-state curve, not for the running transmission.
    pub fn circuit_at(&self, kmh: f64, demand: f64) -> usize {
        let count = self.circuits.len().min(MAX_CIRCUITS);
        (0..count)
            .take_while(|&i| i + 1 < count && kmh > self.shift_up_kmh(i, demand))
            .count()
    }

    /// Steady-state tractive effort [N] at full filling and full notch — the tractive
    /// effort curve of the data sheet, the one a fit is made against.
    ///
    /// The engine settles where its full load torque meets what the pump absorbs: the
    /// governor holds the rated speed as long as there is torque to spare, below that the
    /// engine lugs down onto the balance point. That balance is the whole reason a
    /// hydraulic drive cannot be read off a curve.
    pub fn steady_force(&self, engine: &DieselEngine, v: f64) -> f64 {
        let count = self.circuits.len().min(MAX_CIRCUITS);
        if count == 0 {
            return 0.0;
        }
        let v = v.abs();
        let index = self.circuit_at(v * 3.6, 1.0);
        let circuit = self.circuits[index];
        let transmissions = self.count.max(1) as f64;
        let working_point = |rpm: f64| {
            let omega = rpm * TAU / 60.0;
            let (nu, per_torque) = self.geometry(index, v, omega);
            let pump = circuit.pump_torque(omega, nu, 1.0) * transmissions;
            (pump, nu, per_torque)
        };
        // The pump takes ω², the engine map is nearly flat — so the balance is unique and
        // bisection finds it.
        let (mut lo, mut hi) = (engine.idle_rpm, engine.rated_rpm.max(engine.idle_rpm));
        if working_point(hi).0 > engine.full_load_torque(hi) {
            for _ in 0..40 {
                let mid = 0.5 * (lo + hi);
                if working_point(mid).0 > engine.full_load_torque(mid) {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
        } else {
            lo = hi;
        }
        let (pump, nu, per_torque) = working_point(lo);
        pump * circuit.torque_ratio(nu) * per_torque
    }

    /// A starting parameter set out of five numbers of the data sheet: starting tractive
    /// effort, top speed, rated engine speed, rated torque and wheel diameter.
    ///
    /// The converter figures cannot be computed back out of a given tractive effort curve,
    /// so fitting against the plot is the only way there is. This only puts the fit within
    /// reach of it — kind, stall ratio and coupling point stay as they are set.
    pub fn suggest(&self, engine: &DieselEngine, start_force: f64, v_max: f64) -> Self {
        let mut out = self.clone();
        let count = out.circuits.len().min(MAX_CIRCUITS);
        if count == 0 {
            return out;
        }
        let omega = engine.rated_rpm.max(1.0) * TAU / 60.0;
        let rated_torque = engine.full_load_torque(engine.rated_rpm).max(1.0);
        let radius = (out.wheel_diameter / 2.0).max(0.05);
        let final_ratio = out.final_ratio.max(0.01);
        let efficiency = out.efficiency.max(0.1);
        // λ so that the pumps together take the engine's rated torque at rated speed.
        let absorption = rated_torque / (omega * omega) / out.count.max(1) as f64;
        // The first circuit has to make the starting effort at stall, the last one has to
        // still be turning at the top speed; the ones in between are spaced geometrically.
        let ratio_first = start_force * radius
            / (rated_torque * out.circuits[0].stall_ratio.max(0.1) * final_ratio * efficiency);
        let ratio_last = out.circuits[count - 1].coupling_nu.max(0.05) * omega * radius
            / ((v_max / 3.6).max(1.0) * final_ratio);
        let ratio_first = ratio_first.max(1e-3);
        for (i, circuit) in out.circuits.iter_mut().enumerate().take(count) {
            let t = if count > 1 {
                i as f64 / (count - 1) as f64
            } else {
                0.0
            };
            circuit.absorption = absorption;
            circuit.ratio = ratio_first * (ratio_last.max(1e-3) / ratio_first).powf(t);
            // Change up once the circuit has reached its coupling point at rated speed —
            // past it a converter only loses efficiency.
            circuit.shift_up_kmh = if i + 1 < count {
                circuit.coupling_nu.max(0.05) * omega * radius / (circuit.ratio * final_ratio) * 3.6
            } else {
                0.0
            };
        }
        out
    }
}

/// Hydrodynamic brake (retarder) in the transmission.
///
/// A water brake: the rotor throws the filling against the stator, the energy leaves as
/// heat through the cooler. Braking torque grows with the square of the speed, so it is
/// strong at speed and useless at a stand.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HydrodynamicBrake {
    /// Absorption λ of the rotor [N·m/(rad/s)²] at full filling.
    pub absorption: f64,
    /// Gear ratio rotor : wheelset.
    pub ratio: f64,
    /// Wheel diameter [m].
    pub wheel_diameter: f64,
    /// Highest braking force [N] the transmission may take.
    pub max_force: f64,
    /// Highest heat the cooler can carry off [W].
    pub max_power: f64,
    /// Time to fill or empty the brake [s].
    pub fill_time: f64,
    /// Below this speed the brake fades out [km/h].
    pub fade_out_kmh: f64,
}

impl HydrodynamicBrake {
    /// Braking force [N] at speed `v` [m/s] and filling `fill`.
    pub fn force(&self, v: f64, fill: f64) -> f64 {
        let radius = (self.wheel_diameter / 2.0).max(0.05);
        let omega_rotor = v.abs() / radius * self.ratio;
        let torque = self.absorption * omega_rotor * omega_rotor * fill.clamp(0.0, 1.0);
        let force = torque * self.ratio / radius;
        let kmh = v.abs() * 3.6;
        let fade = if self.fade_out_kmh > 0.0 {
            (kmh / self.fade_out_kmh).clamp(0.0, 1.0)
        } else {
            1.0
        };
        force
            .min(self.max_force)
            .min(self.max_power / v.abs().max(0.5))
            * fade
    }
}

fn default_ramp_time() -> f64 {
    2.0
}

/// Traction chain of a powered vehicle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TractionSpec {
    /// Simplified drive model: tractive effort over speed, straight off the data sheet's
    /// diagram. Knows nothing about motors or gearboxes — and therefore behaves the same
    /// whatever the load is.
    Curve {
        /// Tractive effort [(km/h, N)], interpolated linearly.
        force: Vec<(f64, f64)>,
        v_max: f64,
        /// Dynamic braking force [(km/h, N)]; empty = no dynamic brake.
        #[serde(default)]
        brake: Vec<(f64, f64)>,
        #[serde(default = "default_ramp_time")]
        ramp_time: f64,
    },
    /// Transformer with tap changer and series-wound motors (BR 110/140/141).
    TapChanger {
        /// Number of notches.
        steps: u32,
        /// Starting tractive effort [N].
        max_force: f64,
        /// Continuous power at the wheel [W].
        max_power: f64,
        /// Maximum speed [km/h].
        v_max: f64,
        /// Time per notch [s].
        step_time: f64,
        /// Motor data. Without it the effort follows the hyperbola from
        /// `max_force`/`max_power`.
        #[serde(default)]
        motor: Option<SeriesMotor>,
        /// Rheostatic brake. Most tap changer locos have none.
        #[serde(default)]
        dynamic_brake: Option<DynamicBrake>,
    },
    /// Three-phase drive with converter (BR 101/185/423, ICE).
    Converter {
        max_force: f64,
        max_power: f64,
        v_max: f64,
        /// Highest dynamic brake force [N].
        brake_force: f64,
        /// Power of the dynamic brake [W].
        brake_power: f64,
        /// Rise time from 0 to full force [s].
        ramp_time: f64,
        /// Speed from which the pull-out torque limits the effort [km/h]: above it the
        /// tractive effort falls with 1/v² instead of 1/v. 0 = no such limit.
        #[serde(default)]
        v_pullout: f64,
        /// The brake feeds back into the contact line and needs line voltage for it.
        #[serde(default)]
        regenerative: bool,
        /// Below this speed the dynamic brake fades out [km/h].
        #[serde(default)]
        brake_fade_kmh: f64,
    },
    /// Diesel drive (BR 218 hydraulic, BR 648).
    Diesel {
        max_force: f64,
        max_power: f64,
        v_max: f64,
        /// Time from idle to full load [s] — only used without an engine map.
        ramp_time: f64,
        /// Cranking time of the engine [s].
        start_time: f64,
        /// Engine map and governor. Without it the effort follows the hyperbola.
        #[serde(default)]
        engine: Option<DieselEngine>,
        /// Hydraulic transmission. Needs `engine`; without it the drive stays simplified.
        #[serde(default)]
        transmission: Option<Transmission>,
        /// Hydrodynamic brake in the transmission.
        #[serde(default)]
        hydrodynamic_brake: Option<HydrodynamicBrake>,
    },
}

impl TractionSpec {
    pub fn v_max(&self) -> f64 {
        match self {
            TractionSpec::Curve { v_max, .. }
            | TractionSpec::TapChanger { v_max, .. }
            | TractionSpec::Converter { v_max, .. }
            | TractionSpec::Diesel { v_max, .. } => *v_max,
        }
    }

    /// Available tractive effort at speed `v` [m/s] — the nominal characteristic, without
    /// the drive's own state. The AI driver and the HUD plan with it.
    pub fn available_force(&self, v: f64) -> f64 {
        let av = v.abs();
        if av > self.v_max() / 3.6 {
            return 0.0;
        }
        match self {
            TractionSpec::Curve { force, .. } => interpolate(force, av * 3.6).max(0.0),
            TractionSpec::Converter {
                max_force,
                max_power,
                v_pullout,
                ..
            } => {
                let base = max_force.min(max_power / av.max(0.5));
                // Above the pull-out speed the breakdown torque takes over: F ~ 1/v².
                if *v_pullout > 0.0 && av * 3.6 > *v_pullout {
                    let ratio = *v_pullout / (av * 3.6);
                    base * ratio
                } else {
                    base
                }
            }
            TractionSpec::TapChanger {
                max_force,
                max_power,
                ..
            } => max_force.min(max_power / av.max(0.5)),
            TractionSpec::Diesel {
                max_force,
                max_power,
                engine,
                transmission,
                ..
            } => match (engine, transmission) {
                // With engine and transmission the curve is not a hyperbola but what the
                // converters make of the engine map — change points and all.
                (Some(engine), Some(transmission)) => {
                    transmission.steady_force(engine, av).min(*max_force)
                }
                _ => max_force.min(max_power / av.max(0.5)),
            },
        }
    }

    /// Available dynamic brake force at `v` [m/s] — including the hydrodynamic brake of a
    /// diesel-hydraulic drive.
    pub fn available_brake_force(&self, v: f64) -> f64 {
        match self {
            TractionSpec::Curve { brake, .. } => interpolate(brake, v.abs() * 3.6).max(0.0),
            TractionSpec::TapChanger { dynamic_brake, .. } => {
                dynamic_brake.map_or(0.0, |b| b.available(v))
            }
            TractionSpec::Converter {
                brake_force,
                brake_power,
                brake_fade_kmh,
                ..
            } => {
                let fade = if *brake_fade_kmh > 0.0 {
                    (v.abs() * 3.6 / brake_fade_kmh).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                brake_force.min(brake_power / v.abs().max(0.5)) * fade
            }
            TractionSpec::Diesel {
                hydrodynamic_brake, ..
            } => hydrodynamic_brake.map_or(0.0, |b| b.force(v, 1.0)),
        }
    }

    /// Does the drive have a dynamic brake at all?
    pub fn has_dynamic_brake(&self) -> bool {
        match self {
            TractionSpec::Curve { brake, .. } => !brake.is_empty(),
            TractionSpec::TapChanger { dynamic_brake, .. } => dynamic_brake.is_some(),
            TractionSpec::Converter { brake_force, .. } => *brake_force > 0.0,
            TractionSpec::Diesel {
                hydrodynamic_brake, ..
            } => hydrodynamic_brake.is_some(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn motor() -> SeriesMotor {
        SeriesMotor {
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
        }
    }

    #[test]
    fn series_motor_pulls_hardest_at_a_stand() {
        let m = motor();
        let (f0, i0) = m.effort(0.0, 1.0, 1.0);
        let (f100, i100) = m.effort(100.0 / 3.6, 1.0, 1.0);
        assert!(
            f0 > f100,
            "series motor: {f0:.0} N at a stand vs {f100:.0} N"
        );
        // The current limit relay holds at a stand, at speed the back EMF does it by itself.
        assert!((i0 - m.max_current).abs() < 1.0);
        assert!(i100 < i0);
        // Order of magnitude of a BR 110: 250–300 kN starting effort.
        assert!((200_000.0..320_000.0).contains(&f0), "{f0:.0} N");
    }

    #[test]
    fn field_weakening_extends_the_speed_range() {
        let m = motor();
        let v = 120.0 / 3.6;
        let full = m.effort(v, 1.0, 1.0).0;
        let weak = m.effort(v, 1.0, 0.7).0;
        assert!(
            weak > full,
            "weakening the field must help at speed: {weak:.0} vs {full:.0} N"
        );
    }

    fn engine() -> DieselEngine {
        DieselEngine {
            idle_rpm: 600.0,
            rated_rpm: 1500.0,
            max_rpm: 1650.0,
            torque_curve: vec![
                (600.0, 9_000.0),
                (1000.0, 13_500.0),
                (1500.0, 13_115.0),
                (1650.0, 11_500.0),
            ],
            governor: Governor::Speed {
                steps: 0,
                droop: 0.04,
            },
            inertia: 60.0,
            response_time: 1.0,
        }
    }

    /// The BR 218's transmission, as `content` has it.
    fn transmission() -> Transmission {
        Transmission {
            circuits: vec![
                Circuit {
                    kind: CircuitKind::Converter,
                    ratio: 3.93,
                    stall_ratio: 2.4,
                    coupling_nu: 0.85,
                    absorption: 0.53,
                    absorption_slope: 0.15,
                    shift_up_kmh: 72.0,
                    shift_primary_kmh: 25.0,
                },
                Circuit {
                    kind: CircuitKind::Converter,
                    ratio: 1.50,
                    stall_ratio: 1.9,
                    coupling_nu: 0.85,
                    absorption: 0.53,
                    absorption_slope: 0.15,
                    shift_up_kmh: 0.0,
                    shift_primary_kmh: 0.0,
                },
            ],
            fill_steps: 0,
            fill_time: 1.2,
            drain_time: 0.7,
            hysteresis_kmh: 10.0,
            final_ratio: 1.0,
            wheel_diameter: 1.0,
            count: 1,
            efficiency: 0.95,
        }
    }

    #[test]
    fn torque_converter_multiplies_at_stall_and_stops_at_the_coupling_point() {
        let c = Circuit {
            kind: CircuitKind::Converter,
            ratio: 3.93,
            stall_ratio: 2.4,
            coupling_nu: 0.85,
            absorption: 0.53,
            absorption_slope: 0.0,
            shift_up_kmh: 72.0,
            shift_primary_kmh: 0.0,
        };
        assert!((c.torque_ratio(0.0) - 2.4).abs() < 1e-9);
        assert!((c.torque_ratio(0.85) - 1.0).abs() < 1e-9);
        // Efficiency µ·ν never leaves the physically possible range.
        for i in 0..=85 {
            let nu = i as f64 / 100.0;
            assert!(c.torque_ratio(nu) * nu < 1.0, "efficiency > 1 at ν = {nu}");
        }
    }

    #[test]
    fn a_coupling_stops_absorbing_when_it_stops_slipping() {
        let c = Circuit {
            kind: CircuitKind::Coupling,
            ratio: 1.0,
            stall_ratio: 1.0,
            coupling_nu: 1.0,
            absorption: 0.5,
            absorption_slope: 0.0,
            shift_up_kmh: 0.0,
            shift_primary_kmh: 0.0,
        };
        assert!(c.pump_torque(150.0, 0.0, 1.0) > 0.0);
        assert!(c.pump_torque(150.0, 1.0, 1.0).abs() < 1e-9);
    }

    #[test]
    fn absorption_walks_with_the_speed_ratio() {
        let mut c = transmission().circuits[0];
        c.absorption_slope = 0.0;
        assert!((c.pump_torque(150.0, 0.0, 1.0) - c.pump_torque(150.0, 0.8, 1.0)).abs() < 1e-9);
        // With a trend the pump takes more towards the coupling point, which is what pulls
        // the engine speed off its parabola.
        let c = transmission().circuits[0];
        assert!(c.pump_torque(150.0, 0.8, 1.0) > c.pump_torque(150.0, 0.0, 1.0) * 1.1);
    }

    #[test]
    fn the_primary_influence_changes_up_earlier_at_a_low_notch() {
        let t = transmission();
        assert!((t.shift_up_kmh(0, 1.0) - 72.0).abs() < 1e-9);
        assert!((t.shift_up_kmh(0, 0.0) - 47.0).abs() < 1e-9);
        // 60 km/h is the running converter at part load and the starting converter at full.
        assert_eq!(t.circuit_at(60.0, 0.2), 1);
        assert_eq!(t.circuit_at(60.0, 1.0), 0);
    }

    #[test]
    fn the_steady_curve_falls_and_shows_the_change_point() {
        let (engine, t) = (engine(), transmission());
        let force = |kmh: f64| t.steady_force(&engine, kmh / 3.6);
        // Starting effort of a BR 218: around 235 kN.
        assert!(
            (200_000.0..280_000.0).contains(&force(0.0)),
            "{:.0} N at a stand",
            force(0.0)
        );
        assert!(force(0.0) > force(60.0) && force(60.0) > force(140.0));
        // The change into the running converter costs effort — that is the hole the driver
        // feels, and the reason the curve cannot be one hyperbola.
        assert!(
            force(74.0) < force(70.0),
            "{:.0} → {:.0} N over the change point",
            force(70.0),
            force(74.0)
        );
    }

    #[test]
    fn the_suggestion_lands_on_the_data_sheet_figures() {
        // Five numbers in, the BR 218's hand-fitted set out — near enough to fit from.
        let engine = engine();
        let suggested = transmission().suggest(&engine, 235_000.0, 140.0);
        let first = suggested.circuits[0];
        assert!(
            (first.absorption - 0.53).abs() < 0.01,
            "λ {}",
            first.absorption
        );
        assert!((first.ratio - 3.93).abs() < 0.05, "ratio {}", first.ratio);
        assert!(
            (50.0..80.0).contains(&first.shift_up_kmh),
            "change point {:.0} km/h",
            first.shift_up_kmh
        );
        // The last circuit still turns at the top speed, and it is the shortest one.
        let last = suggested.circuits[1];
        assert!(last.ratio < first.ratio);
        assert_eq!(last.shift_up_kmh, 0.0);
        assert!(suggested.steady_force(&engine, 0.0) > 150_000.0);
    }

    #[test]
    fn filling_steps_run_from_on_off_to_continuous() {
        assert_eq!(quantise(0.4, 1), 0.0);
        assert_eq!(quantise(0.6, 1), 1.0);
        assert_eq!(quantise(0.4, 4), 0.5);
        assert_eq!(quantise(0.37, 0), 0.37);
    }

    #[test]
    fn the_simplified_model_reads_its_curve() {
        let spec = TractionSpec::Curve {
            force: vec![(0.0, 300_000.0), (100.0, 150_000.0), (200.0, 60_000.0)],
            v_max: 200.0,
            brake: vec![(0.0, 0.0), (50.0, 150_000.0)],
            ramp_time: 2.0,
        };
        assert!((spec.available_force(0.0) - 300_000.0).abs() < 1.0);
        assert!((spec.available_force(50.0 / 3.6) - 225_000.0).abs() < 1.0);
        assert_eq!(spec.available_force(220.0 / 3.6), 0.0);
        assert!((spec.available_brake_force(25.0 / 3.6) - 75_000.0).abs() < 1.0);
    }

    #[test]
    fn the_pull_out_range_bends_the_three_phase_curve_down() {
        let spec = TractionSpec::Converter {
            max_force: 300_000.0,
            max_power: 6_400_000.0,
            v_max: 220.0,
            brake_force: 150_000.0,
            brake_power: 2_600_000.0,
            ramp_time: 2.5,
            v_pullout: 150.0,
            regenerative: true,
            brake_fade_kmh: 10.0,
        };
        let hyperbola = |kmh: f64| 6_400_000.0 / (kmh / 3.6);
        // Below the pull-out speed it is the plain constant-power hyperbola.
        assert!((spec.available_force(120.0 / 3.6) - hyperbola(120.0)).abs() < 1.0);
        // Above it, less.
        assert!(spec.available_force(200.0 / 3.6) < hyperbola(200.0) * 0.8);
    }

    #[test]
    fn the_retarder_is_useless_at_a_stand_and_capped_at_speed() {
        let b = HydrodynamicBrake {
            absorption: 0.8,
            ratio: 4.0,
            wheel_diameter: 1.0,
            max_force: 120_000.0,
            max_power: 2_000_000.0,
            fill_time: 1.0,
            fade_out_kmh: 10.0,
        };
        assert_eq!(b.force(0.0, 1.0), 0.0);
        assert!(b.force(80.0 / 3.6, 1.0) > b.force(20.0 / 3.6, 1.0));
        assert!(b.force(160.0 / 3.6, 1.0) <= b.max_force);
    }

    #[test]
    fn interpolation_holds_the_ends() {
        let table = [(10.0, 1.0), (20.0, 2.0)];
        assert_eq!(interpolate(&table, 0.0), 1.0);
        assert_eq!(interpolate(&table, 15.0), 1.5);
        assert_eq!(interpolate(&table, 99.0), 2.0);
        assert_eq!(interpolate(&[], 5.0), 0.0);
    }
}
