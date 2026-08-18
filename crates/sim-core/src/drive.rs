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

/// Traction chains a vehicle can carry. Four covers the multi-engine railcars (BR 245)
/// and every dual-mode loco; like [`MAX_CIRCUITS`] it keeps the drive state `Copy` and
/// allocation-free, which the determinism test relies on.
pub const MAX_DRIVES: usize = 4;

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
    /// Thermal behaviour of the motors; `None` = they never get hot.
    #[serde(default)]
    pub thermal: Option<Thermal>,
}

impl SeriesMotor {
    /// Flux linkage kΦ [V·s] at armature current `i` [A] and field factor `field`.
    fn flux(&self, i: f64, field: f64) -> f64 {
        self.flux_constant * i / (1.0 + i / self.saturation_current.max(1.0)) * field
    }

    /// Armature current [A] at terminal voltage `u` [V] and angular velocity `omega`
    /// [rad/s], with `r_ext` [Ω] of starting resistance in the same string.
    ///
    /// `U = I·(R + R_ext) + kΦ(I)·ω` grows strictly monotonically in `I`, so bisection
    /// converges; 30 halvings resolve the search range to well below a milliampere.
    fn current(&self, u: f64, omega: f64, field: f64, r_ext: f64) -> f64 {
        let r = self.resistance + r_ext.max(0.0);
        let (mut lo, mut hi) = (0.0, self.max_current * 4.0);
        for _ in 0..30 {
            let mid = 0.5 * (lo + hi);
            if mid * r + self.flux(mid, field) * omega < u {
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
        self.effort_with(v, ratio, field, 0.0)
    }

    /// The same with a starting resistance `r_ext` [Ω] in series with each motor. That is
    /// the whole of what a resistance start does: the resistor eats the voltage the back
    /// EMF is not yet eating, and the current stays where the contactors want it.
    pub fn effort_with(&self, v: f64, ratio: f64, field: f64, r_ext: f64) -> (f64, f64) {
        let radius = (self.wheel_diameter / 2.0).max(0.05);
        let omega = v.abs() / radius * self.gear_ratio;
        let u = self.max_voltage * ratio.clamp(0.0, 1.0);
        let current = self.current(u, omega, field, r_ext).min(self.max_current);
        let torque = self.flux(current, field) * current;
        let force = torque * self.gear_ratio / radius * self.count as f64 * self.efficiency;
        (force, current)
    }

    /// Heat the motors and a starting resistance put out at `current` [A] [W].
    pub fn losses(&self, current: f64, r_ext: f64) -> f64 {
        let r = self.resistance + r_ext.max(0.0);
        current * current * r * self.count.max(1) as f64
    }

    /// Best field stage at this operating point: the strongest field wins at a stand,
    /// the weakest one keeps the effort up at speed. Returns (force, current, field).
    pub fn best_effort(&self, v: f64, ratio: f64, power_limit: f64) -> (f64, f64, f64) {
        self.best_effort_with(v, ratio, power_limit, 0.0)
    }

    /// The same with a starting resistance in the string.
    pub fn best_effort_with(
        &self,
        v: f64,
        ratio: f64,
        power_limit: f64,
        r_ext: f64,
    ) -> (f64, f64, f64) {
        let mut best = (0.0, 0.0, 1.0);
        let steps: &[f64] = if self.field_steps.is_empty() {
            &[1.0]
        } else {
            &self.field_steps
        };
        for &field in steps {
            let (force, current) = self.effort_with(v, ratio, field, r_ext);
            // The transformer's continuous rating limits the power, not the motor.
            let force = force.min(power_limit / v.abs().max(0.5));
            if force > best.0 {
                best = (force, current, field);
            }
        }
        best
    }
}

/// How the traction motors of a series-wound drive are connected to each other.
///
/// The classic way of starting a DC drive: all motors in series first, so each one sees a
/// fraction of the line voltage, then regrouped as the train speeds up. Every regrouping is
/// a step in the tractive effort curve — the sawtooth of an old electric loco's data sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MotorGroup {
    /// All motors in one series string.
    Series,
    /// Two strings in parallel.
    SeriesParallel,
    /// Every motor across the full voltage.
    #[default]
    Parallel,
}

impl MotorGroup {
    /// How many motors share the supply voltage in one string.
    pub fn in_series(self, count: u32) -> f64 {
        let count = count.max(1) as f64;
        match self {
            MotorGroup::Series => count,
            MotorGroup::SeriesParallel => (count / 2.0).max(1.0),
            MotorGroup::Parallel => 1.0,
        }
    }

    /// i18n key of the grouping's name.
    pub fn key(self) -> &'static str {
        match self {
            MotorGroup::Series => "grp-series",
            MotorGroup::SeriesParallel => "grp-series-parallel",
            MotorGroup::Parallel => "grp-parallel",
        }
    }
}

/// Starting equipment of a DC drive: resistors that are cut out step by step, and the
/// regroupings between them.
///
/// Without it a series-wound drive is nailed to the tap changer's voltage; with it the
/// contactor sequence of a DC loco or tramcar can be written down as it really runs —
/// notch up, resistors out, regroup, resistors out again, field weakening last.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Starter {
    /// Series resistance per step [Ω], strongest first, ending at 0. One entry per
    /// contactor notch; the last one is the resistance-free running position.
    pub resistor_steps: Vec<f64>,
    /// Groupings in the order they are taken up.
    pub groups: Vec<MotorGroup>,
    /// Time per contactor step [s].
    pub step_time: f64,
    /// A chopper replaces the resistors: the voltage is set continuously instead of in
    /// steps, and nothing is burnt in a resistor bank.
    #[serde(default)]
    pub chopper: bool,
    /// Thermal behaviour of the resistor bank; `None` = the resistors never get hot.
    #[serde(default)]
    pub thermal: Option<Thermal>,
}

impl Default for Starter {
    fn default() -> Self {
        Self {
            resistor_steps: vec![1.6, 1.1, 0.75, 0.5, 0.3, 0.15, 0.0],
            groups: vec![MotorGroup::Series, MotorGroup::Parallel],
            step_time: 1.2,
            chopper: false,
            thermal: None,
        }
    }
}

impl Starter {
    /// Number of contactor positions: every grouping runs through every resistor step.
    pub fn positions(&self) -> usize {
        (self.resistor_steps.len().max(1)) * (self.groups.len().max(1))
    }

    /// Grouping and series resistance at contactor position `pos`.
    pub fn at(&self, pos: usize) -> (MotorGroup, f64) {
        let steps = self.resistor_steps.len().max(1);
        let group = *self
            .groups
            .get(pos / steps)
            .or_else(|| self.groups.last())
            .unwrap_or(&MotorGroup::Parallel);
        let resistance = self
            .resistor_steps
            .get(pos % steps)
            .copied()
            .unwrap_or(0.0)
            .max(0.0);
        (group, resistance)
    }

    /// Position the drive should be on at demand `notch` (0…1) — the driver's handle
    /// commands a position, the contactors walk towards it with `step_time`.
    pub fn target(&self, notch: f64) -> f64 {
        (notch.clamp(0.0, 1.0) * (self.positions().saturating_sub(1)) as f64).max(0.0)
    }
}

/// Thermal behaviour of a component that turns electricity into heat — traction motors,
/// braking resistors, starting resistors.
///
/// One lumped mass with a cooling term. What it buys is the reason a rheostatic brake
/// cannot be held for ever and why a loco that has been slogging derates: the resistor bank
/// is a heat store, not a sink.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Thermal {
    /// Heat capacity of the mass [J/K].
    pub heat_capacity: f64,
    /// Heat the cooling carries off per kelvin above ambient [W/K], with the blower running.
    pub cooling: f64,
    /// Share of `cooling` left with the blower off (natural convection).
    #[serde(default)]
    pub natural_share: f64,
    /// Temperature at which derating starts [°C].
    pub warn_temp: f64,
    /// Temperature at which nothing is delivered any more [°C].
    pub max_temp: f64,
    /// Ambient temperature [°C].
    pub ambient: f64,
}

impl Default for Thermal {
    fn default() -> Self {
        Self {
            heat_capacity: 120_000.0,
            cooling: 900.0,
            natural_share: 0.15,
            warn_temp: 250.0,
            max_temp: 400.0,
            ambient: 20.0,
        }
    }
}

impl Thermal {
    /// New temperature [°C] after `dt` with `heat_w` going in and the blower at `blower`
    /// (0…1).
    pub fn step(&self, temp: f64, heat_w: f64, blower: f64, dt: f64) -> f64 {
        let share = self.natural_share.clamp(0.0, 1.0);
        let cooling = self.cooling * (share + (1.0 - share) * blower.clamp(0.0, 1.0));
        let out = cooling * (temp - self.ambient);
        temp + (heat_w - out) / self.heat_capacity.max(1.0) * dt
    }

    /// Factor on the deliverable effort at `temp` — 1 up to the warning temperature, then
    /// linearly down to 0 at the limit.
    pub fn derate(&self, temp: f64) -> f64 {
        let span = (self.max_temp - self.warn_temp).max(1.0);
        (1.0 - (temp - self.warn_temp) / span).clamp(0.0, 1.0)
    }

    /// Starting temperature of a cold vehicle.
    pub fn cold(&self) -> f64 {
        self.ambient
    }
}

/// Three-phase induction motor (Asynchronmotor) behind a traction converter.
///
/// The torque follows Kloss's equation `M(s) = 2·M_K / (s/s_K + s_K/s)`, and the converter
/// sets the stator frequency. That is what produces the three ranges of a modern tractive
/// effort curve by itself, instead of the `v_pullout` fudge: constant effort while the
/// converter still has voltage in hand, constant power in the field-weakening range, and
/// the pull-out torque bending the curve down with 1/v² at the top.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AsyncMotor {
    /// Number of motors in the vehicle.
    pub count: u32,
    /// Pole pairs.
    pub pole_pairs: u32,
    /// Rated torque per motor [N·m].
    pub rated_torque: f64,
    /// Pull-out torque as a multiple of the rated torque (2.2…3).
    pub pullout_ratio: f64,
    /// Slip at the pull-out torque.
    pub pullout_slip: f64,
    /// Stator frequency at which the converter reaches its full voltage [Hz]; above it the
    /// field weakens.
    pub rated_frequency: f64,
    /// Highest stator frequency the converter can put out [Hz].
    pub max_frequency: f64,
    /// Gear ratio motor : wheelset.
    pub gear_ratio: f64,
    /// Wheel diameter [m].
    pub wheel_diameter: f64,
    /// Efficiency of converter, motor and gearing together.
    pub efficiency: f64,
    /// Thermal behaviour of the motors.
    #[serde(default)]
    pub thermal: Option<Thermal>,
}

impl Default for AsyncMotor {
    fn default() -> Self {
        Self {
            count: 4,
            pole_pairs: 2,
            rated_torque: 5_800.0,
            pullout_ratio: 2.6,
            pullout_slip: 0.14,
            rated_frequency: 60.0,
            max_frequency: 160.0,
            gear_ratio: 2.5,
            wheel_diameter: 1.25,
            efficiency: 0.9,
            thermal: None,
        }
    }
}

impl AsyncMotor {
    /// Rotor frequency [Hz] at road speed `v` [m/s].
    pub fn rotor_frequency(&self, v: f64) -> f64 {
        let radius = (self.wheel_diameter / 2.0).max(0.05);
        v.abs() / radius * self.gear_ratio * self.pole_pairs.max(1) as f64 / TAU
    }

    /// Torque per motor [N·m] at slip `s` with the flux at `flux` (1 = full field).
    pub fn torque(&self, s: f64, flux: f64) -> f64 {
        let s_k = self.pullout_slip.max(1e-3);
        let s = s.clamp(-4.0, 4.0);
        if s.abs() < 1e-6 {
            return 0.0;
        }
        // Pull-out torque scales with the square of the flux; the shape of the curve does not.
        let m_k = self.pullout_ratio.max(1.0) * self.rated_torque * flux * flux;
        2.0 * m_k / (s / s_k + s_k / s)
    }

    /// Flux the converter can still hold at stator frequency `f_s` [Hz] — full up to the
    /// rated frequency, then falling with 1/f because the voltage has run out.
    pub fn flux(&self, f_s: f64) -> f64 {
        if f_s <= self.rated_frequency.max(1e-3) {
            1.0
        } else {
            (self.rated_frequency / f_s).clamp(0.0, 1.0)
        }
    }

    /// Highest tractive effort [N] at speed `v` [m/s], and the slip it is reached at.
    ///
    /// The converter is free to put the slip wherever it likes up to the pull-out point;
    /// what limits it is the stator frequency it can generate and the flux left at it.
    pub fn best_effort(&self, v: f64) -> (f64, f64) {
        let radius = (self.wheel_diameter / 2.0).max(0.05);
        let f_r = self.rotor_frequency(v);
        let s_k = self.pullout_slip.max(1e-3);
        // Driving means the stator runs ahead of the rotor: f_s = f_r/(1 − s).
        let f_s = (f_r / (1.0 - s_k)).min(self.max_frequency.max(1.0));
        if f_s <= 0.0 {
            return (0.0, 0.0);
        }
        // The slip that is actually left once the frequency ceiling bites.
        let slip = if f_s > 0.0 {
            ((f_s - f_r) / f_s).clamp(0.0, s_k)
        } else {
            0.0
        };
        let torque = self.torque(slip, self.flux(f_s));
        let force =
            torque * self.gear_ratio / radius * self.count.max(1) as f64 * self.efficiency.max(0.1);
        (force.max(0.0), slip)
    }

    /// Tractive effort [N] the converter delivers at `demand` (0…1) of the available torque.
    pub fn effort(&self, v: f64, demand: f64) -> f64 {
        self.best_effort(v).0 * demand.clamp(0.0, 1.0)
    }

    /// Heat the motors put out at effort `force` [N] and speed `v` [m/s] [W].
    pub fn losses(&self, force: f64, v: f64) -> f64 {
        let mechanical = force.abs() * v.abs();
        mechanical * (1.0 / self.efficiency.clamp(0.1, 1.0) - 1.0)
    }
}

/// What a diesel-electric drive drives its wheels with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ElectricMotor {
    /// Series-wound DC motors behind a generator and a rectifier — the classic arrangement.
    Dc(SeriesMotor),
    /// Three-phase motors behind an inverter (modern diesel-electrics).
    Ac(AsyncMotor),
}

impl ElectricMotor {
    /// Tractive effort [N] at speed `v` with the supply at `voltage_ratio` (0…1) and the
    /// field at `field` — the AC branch ignores the field and takes the ratio as a torque
    /// demand, which is exactly what its converter does.
    pub fn effort(&self, v: f64, voltage_ratio: f64, field: f64, resistance: f64) -> (f64, f64) {
        match self {
            ElectricMotor::Dc(motor) => motor.effort_with(v, voltage_ratio, field, resistance),
            ElectricMotor::Ac(motor) => (motor.effort(v, voltage_ratio), 0.0),
        }
    }

    /// Highest effort the motor can make at `v` with the supply fully open.
    pub fn max_effort(&self, v: f64) -> f64 {
        match self {
            ElectricMotor::Dc(motor) => motor.best_effort(v, 1.0, f64::INFINITY).0,
            ElectricMotor::Ac(motor) => motor.best_effort(v).0,
        }
    }

    pub fn thermal(&self) -> Option<Thermal> {
        match self {
            ElectricMotor::Dc(motor) => motor.thermal,
            ElectricMotor::Ac(motor) => motor.thermal,
        }
    }
}

/// Generator (or alternator) and load regulator of a diesel-electric drive.
///
/// The load regulator is what makes the type behave the way it does: the driver's handle
/// asks for an engine power, and the regulator adjusts the generator's excitation until the
/// generator takes exactly that much off the engine. The wheels then get whatever voltage
/// and current that power happens to work out to — constant power, limited by the highest
/// current at a stand and by the highest voltage at speed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DieselElectric {
    /// Electrical power the generator can deliver at the full notch [W].
    pub generator_power: f64,
    /// Efficiency of generator and rectifier together.
    pub generator_efficiency: f64,
    /// Highest generator voltage [V].
    pub max_voltage: f64,
    /// Highest generator current [A].
    pub max_current: f64,
    /// Time the load regulator takes to travel its full range [s].
    pub regulator_time: f64,
    /// The traction motors behind it.
    pub motor: ElectricMotor,
    /// Blower of the traction motors: share of the cooling that runs with the engine
    /// (0 = only when the drive is working).
    #[serde(default)]
    pub blower_idle_share: f64,
}

impl Default for DieselElectric {
    fn default() -> Self {
        Self {
            generator_power: 1_800_000.0,
            generator_efficiency: 0.94,
            max_voltage: 1_200.0,
            max_current: 4_000.0,
            regulator_time: 3.0,
            motor: ElectricMotor::Dc(SeriesMotor {
                count: 6,
                resistance: 0.028,
                flux_constant: 0.021,
                saturation_current: 900.0,
                max_current: 1_100.0,
                max_voltage: 1_200.0,
                field_steps: vec![1.0, 0.7, 0.5],
                gear_ratio: 4.4,
                wheel_diameter: 1.016,
                efficiency: 0.92,
                thermal: None,
            }),
            blower_idle_share: 0.2,
        }
    }
}

impl DieselElectric {
    /// Voltage ratio (0…1) the load regulator settles on so the generator takes `power` [W]
    /// off the engine at speed `v` [m/s], with the field at `field`.
    ///
    /// The motor's demand grows monotonically with the voltage, so bisection finds the
    /// working point; the ceiling is the generator's own voltage limit.
    pub fn regulator_ratio(&self, v: f64, power: f64, field: f64) -> f64 {
        let target = power.max(0.0) * self.generator_efficiency.clamp(0.1, 1.0);
        let draw = |ratio: f64| {
            let (force, current) = self.motor.effort(v, ratio, field, 0.0);
            // Electrical power the motors take: mechanical output plus their own losses.
            let mechanical = force * v.abs();
            let electrical = match &self.motor {
                ElectricMotor::Dc(motor) => {
                    let per = ratio * motor.max_voltage * current;
                    per * motor.count.max(1) as f64
                }
                ElectricMotor::Ac(motor) => mechanical / motor.efficiency.clamp(0.1, 1.0),
            };
            (electrical.max(mechanical), current)
        };
        let (mut lo, mut hi) = (0.0, 1.0);
        if draw(hi).0 <= target {
            return 1.0;
        }
        for _ in 0..30 {
            let mid = 0.5 * (lo + hi);
            let (electrical, current) = draw(mid);
            if electrical < target && current < self.max_current {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        0.5 * (lo + hi)
    }

    /// Tractive effort [N] at `v` with the load regulator at `ratio` and the field at `field`.
    pub fn effort(&self, v: f64, ratio: f64, field: f64) -> (f64, f64) {
        self.motor.effort(v, ratio, field, 0.0)
    }

    /// Steady tractive effort [N] at the full notch — the data sheet's curve.
    pub fn steady_force(&self, v: f64) -> f64 {
        let field = match &self.motor {
            ElectricMotor::Dc(motor) => motor.field_steps.last().copied().unwrap_or(1.0),
            ElectricMotor::Ac(_) => 1.0,
        };
        let ratio = self.regulator_ratio(v, self.generator_power, field);
        let (force, _) = self.effort(v, ratio, field);
        force
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
    /// Thermal behaviour of the braking resistors. A rheostatic brake that is held long
    /// enough fades out — that is what this models. `None` = the bank never gets hot;
    /// a regenerative brake has nothing to heat up anyway.
    #[serde(default)]
    pub thermal: Option<Thermal>,
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
    /// Final drive of the shunting gear, where the vehicle has a two-range gearbox behind
    /// the transmission (V 60, V 90 and most heavy shunters): more tractive effort, less
    /// speed. 0 = no such gearbox. The change is a dog clutch and only takes at a stand.
    #[serde(default)]
    pub shunting_ratio: f64,
    /// Wheel diameter [m].
    pub wheel_diameter: f64,
    /// Number of transmissions in the vehicle.
    pub count: u32,
    /// The power comes from the engine speed, not from the filling: the circuit is filled
    /// as soon as the controller leaves the zero notch and stays full. That is how a
    /// Mekydro works, whose converter knows only full or empty and whose gears sit behind
    /// it; a Voith with filling control (the default) sets its part load in the circuit.
    #[serde(default)]
    pub speed_controlled: bool,
    /// Mechanical efficiency of the gearing behind the circuit.
    pub efficiency: f64,
}

impl Transmission {
    /// Final drive in effect: the shunting gear where one is fitted and engaged.
    pub fn final_ratio(&self, road_gear: bool) -> f64 {
        if !road_gear && self.shunting_ratio > 0.0 {
            self.shunting_ratio
        } else {
            self.final_ratio
        }
    }

    /// Speed ratio ν and tractive effort per unit of pump torque, for circuit `index`.
    pub fn geometry(&self, index: usize, v: f64, omega_engine: f64, road_gear: bool) -> (f64, f64) {
        let circuit = self.circuits[index];
        let radius = (self.wheel_diameter / 2.0).max(0.05);
        // Rolling backwards, the turbine stands still as far as the converter is concerned:
        // stall, and that is exactly where it delivers its maximum torque.
        let omega_wheel = (v / radius).max(0.0);
        let total = circuit.ratio * self.final_ratio(road_gear);
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
            // The nominal curve is the one of the data sheet: road gear.
            let (nu, per_torque) = self.geometry(index, v, omega, true);
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

/// Mechanical gearbox with a friction clutch — the drive of the small shunters (Köf I
/// and II) and the railbuses (VT 95/98).
///
/// Nothing multiplies the torque here: the tractive effort is the engine's torque times
/// the gear, and getting away from a stand is the clutch slipping. That also means the
/// engine hangs rigidly on the wheels once the clutch is in — the speed follows the road
/// speed, not the notch — and that it can be stalled.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MechanicalGearbox {
    /// Gear ratios engine : gearbox output, first gear first.
    pub gears: Vec<f64>,
    /// Final drive: gearbox output : wheelset.
    pub final_ratio: f64,
    /// Wheel diameter [m].
    pub wheel_diameter: f64,
    /// Mechanical efficiency of the gearing.
    pub efficiency: f64,
    /// Torque the clutch can hold before it slips [N·m].
    pub clutch_torque: f64,
    /// Time the clutch takes over its full travel [s].
    pub clutch_time: f64,
    /// Time a gear change takes — clutch out, gear, clutch in [s].
    pub shift_time: f64,
    /// Change up once the engine reaches this speed [1/min].
    pub shift_up_rpm: f64,
    /// Change down once it falls below this one [1/min].
    pub shift_down_rpm: f64,
}

impl MechanicalGearbox {
    /// Total ratio engine : wheelset of gear `index`.
    pub fn total_ratio(&self, index: usize) -> f64 {
        self.gears.get(index).copied().unwrap_or(1.0) * self.final_ratio
    }

    /// Engine speed [1/min] the gear ties to road speed `v` [m/s] with the clutch in.
    pub fn sync_rpm(&self, index: usize, v: f64) -> f64 {
        let radius = (self.wheel_diameter / 2.0).max(0.05);
        v.abs() / radius * self.total_ratio(index) * 60.0 / TAU
    }

    /// Steady tractive effort [N] at full load in the gear the schedule picks for `v` —
    /// the nominal curve, without clutch or change.
    pub fn steady_force(&self, engine: &DieselEngine, v: f64) -> f64 {
        let count = self.gears.len();
        if count == 0 {
            return 0.0;
        }
        // The gear the driver would be in: the highest one still turning below the
        // change-up speed.
        let index = (0..count)
            .take_while(|&i| i + 1 < count && self.sync_rpm(i, v) > self.shift_up_rpm)
            .count();
        let rpm = self
            .sync_rpm(index, v)
            .clamp(engine.idle_rpm, engine.rated_rpm);
        let radius = (self.wheel_diameter / 2.0).max(0.05);
        engine.full_load_torque(rpm) * self.total_ratio(index) * self.efficiency / radius
    }
}

/// Hydrostatic drive: a variable-displacement pump on the engine, a hydraulic motor on the
/// axle. Stepless, so there is nothing to change — the swash plate does what a gearbox
/// does. Modern small shunters and road-rail vehicles are built this way.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HydrostaticDrive {
    /// Tractive effort the pressure relief valve allows [N] — the flat part of the curve.
    pub max_force: f64,
    /// Overall efficiency of pump, lines and motor.
    pub efficiency: f64,
    /// Time the swash plate takes over its full travel [s].
    pub response_time: f64,
}

impl HydrostaticDrive {
    /// Tractive effort [N] at speed `v` [m/s] and displacement `displacement` (0…1), with
    /// `power` [W] available at the pump. Pressure-limited at a stand, power-limited above.
    pub fn force(&self, power: f64, v: f64, displacement: f64) -> f64 {
        let limit = self.max_force * displacement.clamp(0.0, 1.0);
        // Below walking pace the relief valve, not the power, sets the effort.
        let by_power = power * self.efficiency / v.abs().max(1.0);
        limit.min(by_power)
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

/// Where a drive takes its power from. A vehicle whose drives disagree is a dual-mode
/// vehicle and gets a mode selector; one where they all agree has none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DriveMode {
    /// Contact line over the pantograph.
    #[default]
    Electric,
    /// Diesel engine carried on board.
    Diesel,
    /// Boiler and fire carried on board.
    Steam,
}

impl DriveMode {
    /// Translation key of the mode's name.
    pub fn key(self) -> &'static str {
        match self {
            DriveMode::Electric => "drv-mode-electric",
            DriveMode::Diesel => "drv-mode-diesel",
            DriveMode::Steam => "drv-mode-steam",
        }
    }
}

/// One traction chain of a vehicle, with what it needs to run and who commands it.
///
/// A vehicle carries a list of these: two diesel engines, four traction motors, or a
/// diesel and an electric chain side by side on a dual-mode loco.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DriveSpec {
    /// The chain itself.
    pub traction: TractionSpec,
    /// Power source it needs. Defaults to what the chain implies.
    #[serde(default)]
    pub mode: DriveMode,
    /// Which power controller commands it: `0` is the shared one every cab has, `1..`
    /// picks the separate handle of that number. The vehicle builder decides — one lever
    /// for the whole loco, or one per engine.
    #[serde(default)]
    pub throttle: u8,
    /// Name in the cab and in the editor ("Engine 1"). Empty = numbered by position.
    #[serde(default)]
    pub name: String,
}

impl DriveSpec {
    /// A chain on the shared power controller, in the mode its type implies — what a
    /// single-drive vehicle has always been.
    pub fn new(traction: TractionSpec) -> Self {
        Self {
            mode: traction.implied_mode(),
            traction,
            throttle: 0,
            name: String::new(),
        }
    }
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
        /// Starting resistors and motor groupings. A tap changer loco has none — its
        /// transformer already sets the voltage — but a DC loco, a tramcar or a shunter
        /// with a contactor drum is nothing else, and then `steps` is the contactor drum
        /// rather than the transformer.
        #[serde(default)]
        starter: Option<Starter>,
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
        /// Induction motor data. With it the three ranges of the curve come out of the
        /// machine instead of out of `v_pullout`, and `max_force`/`max_power` only cap it.
        #[serde(default)]
        motor: Option<AsyncMotor>,
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
        /// Boxed, like the gearbox and the boiler: the drive paths are the bulky part of
        /// this enum and every variant would carry their size.
        #[serde(default)]
        transmission: Option<Box<Transmission>>,
        /// Generator, load regulator and traction motors of a diesel-electric drive.
        /// Mutually exclusive with `transmission` — a locomotive has one or the other.
        #[serde(default)]
        electric: Option<DieselElectric>,
        /// Mechanical gearbox with a friction clutch (small shunters, railbuses). Boxed
        /// for the same reason the boiler is: it would otherwise blow up every variant.
        #[serde(default)]
        gearbox: Option<Box<MechanicalGearbox>>,
        /// Hydrostatic drive (modern small shunters, road-rail vehicles).
        #[serde(default)]
        hydrostatic: Option<HydrostaticDrive>,
        /// Hydrodynamic brake in the transmission.
        #[serde(default)]
        hydrodynamic_brake: Option<HydrodynamicBrake>,
        /// Electric brake of a diesel-electric drive: the traction motors feed braking
        /// resistors. Independent of the transmission path — a `regenerative` flag is
        /// ignored, a diesel loco has no line to feed back into.
        #[serde(default)]
        dynamic_brake: Option<DynamicBrake>,
    },
    /// Steam locomotive (see [`crate::steam`]). Everything about it is in the boiler, so
    /// the variant carries nothing else but the top speed.
    Steam {
        loco: Box<crate::steam::SteamLoco>,
        v_max: f64,
    },
}

impl TractionSpec {
    /// Power source the chain implies. `Curve` is the abstract one and could be either —
    /// it counts as electric, and a diesel railcar built from it says so in `DriveSpec`.
    pub fn implied_mode(&self) -> DriveMode {
        match self {
            TractionSpec::Diesel { .. } => DriveMode::Diesel,
            TractionSpec::Steam { .. } => DriveMode::Steam,
            _ => DriveMode::Electric,
        }
    }

    pub fn v_max(&self) -> f64 {
        match self {
            TractionSpec::Curve { v_max, .. }
            | TractionSpec::TapChanger { v_max, .. }
            | TractionSpec::Converter { v_max, .. }
            | TractionSpec::Diesel { v_max, .. }
            | TractionSpec::Steam { v_max, .. } => *v_max,
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
                motor,
                ..
            } => {
                // With motor data the machine draws the curve; `max_force`/`max_power` only
                // cap what the converter is rated for.
                if let Some(motor) = motor {
                    return motor
                        .best_effort(av)
                        .0
                        .min(*max_force)
                        .min(max_power / av.max(0.5));
                }
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
                motor,
                starter,
                ..
            } => {
                // A contactor drive runs its motors at full line voltage once the resistors
                // are out; what the curve looks like is then the motor's business.
                if let (Some(motor), Some(_)) = (motor, starter) {
                    return motor
                        .best_effort(av, 1.0, *max_power)
                        .0
                        .min(*max_force)
                        .min(max_power / av.max(0.5));
                }
                max_force.min(max_power / av.max(0.5))
            }
            TractionSpec::Diesel {
                max_force,
                max_power,
                engine,
                transmission,
                electric,
                gearbox,
                hydrostatic,
                ..
            } => {
                // With engine and transmission the curve is not a hyperbola but what the
                // converters make of the engine map — change points and all.
                if let (Some(engine), Some(transmission)) = (engine, transmission) {
                    return transmission.steady_force(engine, av).min(*max_force);
                }
                // Mechanical gearbox: torque times gear, gear by gear.
                if let (Some(engine), Some(gearbox)) = (engine, gearbox) {
                    return gearbox.steady_force(engine, av).min(*max_force);
                }
                // Hydrostatic: flat at the relief valve, hyperbolic above it.
                if let Some(hydrostatic) = hydrostatic {
                    return hydrostatic.force(*max_power, av, 1.0).min(*max_force);
                }
                // Diesel-electric: the load regulator holds the power, the motors decide
                // where the current limit takes over from it.
                if let Some(electric) = electric {
                    return electric.steady_force(av).min(*max_force);
                }
                max_force.min(max_power / av.max(0.5))
            }
            // At working pressure, full regulator and the longest cutoff — the figure the
            // works plate carries. What is actually there depends on the boiler.
            TractionSpec::Steam { loco, .. } => {
                loco.tractive_effort(loco.working_pressure, 1.0, loco.max_cutoff)
            }
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
                hydrodynamic_brake,
                dynamic_brake,
                ..
            } => {
                hydrodynamic_brake.map_or(0.0, |b| b.force(v, 1.0))
                    + dynamic_brake.map_or(0.0, |b| b.available(v))
            }
            // A steam locomotive brakes with its train brake and nothing else.
            TractionSpec::Steam { .. } => 0.0,
        }
    }

    /// Does the drive have a dynamic brake at all?
    pub fn has_dynamic_brake(&self) -> bool {
        match self {
            TractionSpec::Curve { brake, .. } => !brake.is_empty(),
            TractionSpec::TapChanger { dynamic_brake, .. } => dynamic_brake.is_some(),
            TractionSpec::Converter { brake_force, .. } => *brake_force > 0.0,
            TractionSpec::Diesel {
                hydrodynamic_brake,
                dynamic_brake,
                ..
            } => hydrodynamic_brake.is_some() || dynamic_brake.is_some(),
            TractionSpec::Steam { .. } => false,
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
            thermal: None,
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
            shunting_ratio: 0.0,
            wheel_diameter: 1.0,
            count: 1,
            speed_controlled: false,
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
            motor: None,
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

    fn async_motor() -> AsyncMotor {
        // Roughly a BR 101: four motors, 6.4 MW at the wheel.
        AsyncMotor {
            count: 4,
            pole_pairs: 2,
            rated_torque: 7_600.0,
            pullout_ratio: 2.6,
            pullout_slip: 0.14,
            rated_frequency: 45.0,
            max_frequency: 180.0,
            gear_ratio: 2.1,
            wheel_diameter: 1.25,
            efficiency: 0.92,
            thermal: None,
        }
    }

    #[test]
    fn the_induction_motor_draws_the_three_ranges_by_itself() {
        let m = async_motor();
        let f = |kmh: f64| m.best_effort(kmh / 3.6).0;
        // Constant tractive effort while the converter still has voltage in hand.
        assert!(
            (f(10.0) - f(40.0)).abs() / f(10.0) < 0.05,
            "{:.0} vs {:.0} N",
            f(10.0),
            f(40.0)
        );
        // Field weakening: the effort falls, the power stays roughly where it was.
        let p = |kmh: f64| f(kmh) * kmh / 3.6;
        assert!(f(160.0) < f(80.0));
        assert!(
            p(160.0) > p(80.0) * 0.6,
            "{:.0} vs {:.0} W",
            p(160.0),
            p(80.0)
        );
        // And past the pull-out point it falls away faster than 1/v.
        assert!(f(250.0) * 250.0 < f(120.0) * 120.0 * 0.9);
    }

    #[test]
    fn kloss_peaks_at_the_pull_out_slip() {
        let m = async_motor();
        let peak = m.torque(m.pullout_slip, 1.0);
        for s in [0.02, 0.05, 0.25, 0.5, 1.0] {
            assert!(m.torque(s, 1.0) <= peak + 1e-9, "slip {s}");
        }
        assert!((peak - m.pullout_ratio * m.rated_torque).abs() < 1.0);
        assert_eq!(m.torque(0.0, 1.0), 0.0);
    }

    #[test]
    fn a_starting_resistance_holds_the_current_down_and_costs_effort() {
        let m = motor();
        let (free, i_free) = m.effort_with(0.0, 1.0, 1.0, 0.0);
        let (damped, i_damped) = m.effort_with(0.0, 1.0, 1.0, 0.6);
        assert!(i_damped < i_free, "{i_damped:.0} vs {i_free:.0} A");
        assert!(damped < free);
        // Cutting the resistors out step by step is what the contactor sequence is for:
        // no step ever costs effort, and the last one lands on the resistance-free figure.
        // It stops rising once the current limit relay has hold of it — which is exactly
        // why the last few notches of a real contactor drive feel like nothing at all.
        let steps = Starter::default().resistor_steps;
        let mut last = 0.0;
        for r in &steps {
            let (force, _) = m.effort_with(0.0, 1.0, 1.0, *r);
            assert!(
                force >= last - 1e-9,
                "cutting out {r} Ω must not cost effort"
            );
            last = force;
        }
        assert!((last - free).abs() < 1e-9);
        let (first, _) = m.effort_with(0.0, 1.0, 1.0, steps[0]);
        assert!(first < last * 0.5, "{first:.0} vs {last:.0} N");
    }

    #[test]
    fn the_contactor_positions_walk_through_resistors_and_groupings() {
        let starter = Starter::default();
        assert_eq!(starter.positions(), 14);
        assert_eq!(starter.at(0), (MotorGroup::Series, 1.6));
        assert_eq!(starter.at(6), (MotorGroup::Series, 0.0));
        assert_eq!(starter.at(7), (MotorGroup::Parallel, 1.6));
        assert_eq!(starter.at(13), (MotorGroup::Parallel, 0.0));
        // Past the end it stays on the last position rather than panicking.
        assert_eq!(starter.at(99).0, MotorGroup::Parallel);
        assert!((starter.target(1.0) - 13.0).abs() < 1e-9);
        assert_eq!(starter.target(0.0), 0.0);
    }

    #[test]
    fn a_grouping_shares_the_voltage_between_its_motors() {
        assert_eq!(MotorGroup::Series.in_series(4), 4.0);
        assert_eq!(MotorGroup::SeriesParallel.in_series(4), 2.0);
        assert_eq!(MotorGroup::Parallel.in_series(4), 1.0);
        // A single motor cannot be split up.
        assert_eq!(MotorGroup::SeriesParallel.in_series(1), 1.0);
    }

    #[test]
    fn the_load_regulator_holds_the_power_and_the_current_limit_takes_over() {
        let de = DieselElectric::default();
        let f = |kmh: f64| de.steady_force(kmh / 3.6);
        // A 1.8 MW diesel-electric: a few hundred kN at a stand, falling with speed.
        assert!(f(0.0) > f(40.0) && f(40.0) > f(100.0));
        // Constant power over the middle range — within the accuracy of a motor that also
        // has a current limit and a field to weaken.
        let p = |kmh: f64| f(kmh) * kmh / 3.6;
        assert!(
            p(80.0) > p(40.0) * 0.7 && p(80.0) < p(40.0) * 1.4,
            "{:.0} vs {:.0} W",
            p(40.0),
            p(80.0)
        );
        // Half the notch is half the power, not half the effort.
        let half = de.regulator_ratio(60.0 / 3.6, de.generator_power * 0.5, 1.0);
        let full = de.regulator_ratio(60.0 / 3.6, de.generator_power, 1.0);
        assert!(half < full, "{half} vs {full}");
    }

    #[test]
    fn a_hot_resistor_bank_stops_taking_energy() {
        let t = Thermal::default();
        assert_eq!(t.derate(t.ambient), 1.0);
        assert_eq!(t.derate(t.warn_temp), 1.0);
        assert_eq!(t.derate(t.max_temp), 0.0);
        assert!((t.derate((t.warn_temp + t.max_temp) / 2.0) - 0.5).abs() < 1e-9);
        // 500 kW into the bank heats it past its limit inside a few minutes…
        let mut temp = t.cold();
        for _ in 0..(200 * 400) {
            temp = t.step(temp, 500_000.0, 1.0, 1.0 / 200.0);
        }
        assert!(temp > t.max_temp, "{temp:.0} °C");
        // …and the blower brings it back down again.
        for _ in 0..(200 * 900) {
            temp = t.step(temp, 0.0, 1.0, 1.0 / 200.0);
        }
        assert!(temp < t.warn_temp, "{temp:.0} °C");
    }

    #[test]
    fn the_blower_is_what_makes_the_difference() {
        let t = Thermal::default();
        let run = |blower: f64| {
            let mut temp = t.cold();
            for _ in 0..(200 * 300) {
                temp = t.step(temp, 150_000.0, blower, 1.0 / 200.0);
            }
            temp
        };
        assert!(
            run(0.0) > run(1.0) + 50.0,
            "{:.0} vs {:.0}",
            run(0.0),
            run(1.0)
        );
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
