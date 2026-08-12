//! Electrics, drive and start-up procedure (plan ch. 8).
//!
//! No SPICE: a directed state graph of switches and loads. What matters is the order
//! (battery → pantograph → main switch → auxiliaries) and the characteristic curve of
//! the traction chain.

use crate::brakes::approach;
use serde::{Deserialize, Serialize};

/// Nominal voltage of the German railway power network [V].
pub const NOMINAL_LINE_VOLTAGE: f64 = 15_000.0;
/// From this contact wire voltage upwards the main switch may close [V].
pub const MIN_LINE_VOLTAGE: f64 = 10_000.0;

/// Traction chain of a powered vehicle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TractionSpec {
    /// Transformer with tap changer (older electric loco, e.g. BR 110/140).
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
    },
    /// Diesel drive (BR 218 hydraulic, BR 648).
    Diesel {
        max_force: f64,
        max_power: f64,
        v_max: f64,
        /// Time from idle to full load [s].
        ramp_time: f64,
        /// Cranking time of the engine [s].
        start_time: f64,
    },
}

impl TractionSpec {
    pub fn v_max(&self) -> f64 {
        match self {
            TractionSpec::TapChanger { v_max, .. }
            | TractionSpec::Converter { v_max, .. }
            | TractionSpec::Diesel { v_max, .. } => *v_max,
        }
    }

    /// Available tractive effort at speed `v` [m/s] — tractive effort hyperbola.
    pub fn available_force(&self, v: f64) -> f64 {
        let (max_force, max_power, v_max) = match self {
            TractionSpec::TapChanger {
                max_force,
                max_power,
                v_max,
                ..
            }
            | TractionSpec::Converter {
                max_force,
                max_power,
                v_max,
                ..
            }
            | TractionSpec::Diesel {
                max_force,
                max_power,
                v_max,
                ..
            } => (*max_force, *max_power, *v_max),
        };
        let av = v.abs();
        if av > v_max / 3.6 {
            return 0.0;
        }
        // Below the transition speed constant force, above it constant power.
        max_force.min(max_power / av.max(0.5))
    }

    /// Available dynamic brake force at `v` [m/s].
    pub fn available_brake_force(&self, v: f64) -> f64 {
        match self {
            TractionSpec::Converter {
                brake_force,
                brake_power,
                ..
            } => brake_force.min(brake_power / v.abs().max(0.5)),
            // Tap changer locos of class 110 have no dynamic brake, nor does diesel v1.
            _ => 0.0,
        }
    }
}

/// State of the on-board electrical system and drive of a vehicle.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TractionState {
    pub battery: bool,
    /// Command "raise pantograph".
    pub pantograph_command: bool,
    /// Raise state of the pantograph 0…1 (travel time ~ 5 s).
    pub pantograph: f64,
    /// Command "main switch on".
    pub main_switch_command: bool,
    pub main_switch: bool,
    /// Contact wire voltage at the pantograph [V] — set by the line
    /// (0 in neutral sections or without catenary).
    pub line_voltage: f64,
    /// Power controller: −1 … +1 (negative = dynamic brake).
    pub notch: f64,
    /// Current tap changer notch (only `TapChanger`).
    pub step: f64,
    /// Diesel engine running.
    pub engine_running: bool,
    /// Remaining cranking time [s].
    pub start_timer: f64,
    /// Air compressor switched on.
    pub compressor: bool,
    /// Train line (heating) switched on.
    pub train_line: bool,
    /// Current tractive effort [N], positive = traction, negative = dynamic brake.
    pub force: f64,
}

impl Default for TractionState {
    fn default() -> Self {
        Self {
            battery: false,
            pantograph_command: false,
            pantograph: 0.0,
            main_switch_command: false,
            main_switch: false,
            line_voltage: 0.0,
            notch: 0.0,
            step: 0.0,
            engine_running: false,
            start_timer: 0.0,
            compressor: false,
            train_line: false,
            force: 0.0,
        }
    }
}

impl TractionState {
    /// Started up and ready to run?
    pub fn ready(&self) -> bool {
        self.battery && (self.main_switch || self.engine_running)
    }
}

/// One simulation step for the on-board electrical system and drive of a vehicle.
pub fn step(state: &mut TractionState, spec: &TractionSpec, v: f64, dt: f64) {
    update_power(state, spec, dt);

    let electric_ok = state.main_switch && state.line_voltage >= MIN_LINE_VOLTAGE;
    let powered = match spec {
        TractionSpec::Diesel { .. } => state.engine_running,
        _ => electric_ok,
    };

    if !powered {
        // Force decays, the tap changer runs back to the zero notch.
        approach(&mut state.force, 0.0, 1.0e6, dt);
        approach(&mut state.step, 0.0, 5.0, dt);
        return;
    }

    let notch = state.notch.clamp(-1.0, 1.0);
    match spec {
        TractionSpec::TapChanger {
            steps, step_time, ..
        } => {
            // The tap changer runs notch by notch — the power controller only sets the target.
            let target = notch.max(0.0) * *steps as f64;
            approach(&mut state.step, target, 1.0 / step_time.max(0.01), dt);
            state.force = state.step / *steps as f64 * spec.available_force(v);
        }
        TractionSpec::Converter { ramp_time, .. } => {
            let target = if notch >= 0.0 {
                notch * spec.available_force(v)
            } else {
                notch * spec.available_brake_force(v)
            };
            let rate = spec.available_force(v).max(1.0) / ramp_time.max(0.1);
            approach(&mut state.force, target, rate, dt);
        }
        TractionSpec::Diesel { ramp_time, .. } => {
            let target = notch.max(0.0) * spec.available_force(v);
            let rate = spec.available_force(v).max(1.0) / ramp_time.max(0.1);
            approach(&mut state.force, target, rate, dt);
        }
    }
}

/// Start-up chain: battery → pantograph → main switch (plan 8, start-up procedure).
fn update_power(state: &mut TractionState, spec: &TractionSpec, dt: f64) {
    if !state.battery {
        state.pantograph_command = false;
        state.main_switch_command = false;
        state.compressor = false;
    }

    // The pantograph needs ~ 5 s to rise and ~ 3 s to lower.
    let target = if state.pantograph_command && state.battery {
        1.0
    } else {
        0.0
    };
    let rate = if target > state.pantograph {
        1.0 / 5.0
    } else {
        1.0 / 3.0
    };
    approach(&mut state.pantograph, target, rate, dt);

    // Main switch: only with contact wire voltage present, drops out on loss of voltage
    // (neutral section!).
    let contact = state.pantograph > 0.98;
    let voltage_ok = contact && state.line_voltage >= MIN_LINE_VOLTAGE;
    state.main_switch = state.main_switch_command && state.battery && voltage_ok;

    if let TractionSpec::Diesel { start_time, .. } = spec {
        if state.start_timer > 0.0 {
            state.start_timer -= dt;
            if state.start_timer <= 0.0 {
                state.engine_running = true;
                state.start_timer = 0.0;
            }
        }
        let _ = start_time;
    }
}

/// Crank the diesel engine (needs the battery).
pub fn start_engine(state: &mut TractionState, spec: &TractionSpec) {
    if let TractionSpec::Diesel { start_time, .. } = spec
        && state.battery
        && !state.engine_running
        && state.start_timer <= 0.0
    {
        state.start_timer = *start_time;
    }
}
