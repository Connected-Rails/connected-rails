//! On-board electrical system and drive control (plan ch. 8).
//!
//! No SPICE: a directed state graph of switches and loads. What matters is the order
//! (battery → pantograph → main switch → auxiliaries) and, behind it, the drive model
//! from [`crate::drive`], which this module ticks.

use crate::brakes::approach;
use crate::drive::{
    DieselEngine, DynamicBrake, Governor, HydrodynamicBrake, MAX_CIRCUITS, TractionSpec,
    Transmission, quantise,
};
use serde::{Deserialize, Serialize};
use std::f64::consts::TAU;

/// Nominal voltage of the German railway power network [V] — 15 kV 16.7 Hz.
pub const NOMINAL_LINE_VOLTAGE: f64 = 15_000.0;
/// Frequency of the German railway power network [Hz].
pub const LINE_FREQUENCY: f64 = 16.7;
/// From this contact wire voltage upwards the main switch may close [V].
pub const MIN_LINE_VOLTAGE: f64 = 10_000.0;

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
    /// Engine speed [1/min] (diesel with an engine map).
    #[serde(default)]
    pub engine_rpm: f64,
    /// Fuel rack 0…1 (diesel with an engine map).
    #[serde(default)]
    pub engine_fill: f64,
    /// Engaged hydraulic circuit (diesel-hydraulic).
    #[serde(default)]
    pub circuit: usize,
    /// Filling of the hydraulic circuits 0…1.
    #[serde(default)]
    pub circuit_fill: [f64; MAX_CIRCUITS],
    /// Speed ratio ν of the engaged circuit — the transmission's working point.
    #[serde(default)]
    pub circuit_nu: f64,
    /// Filling of the hydrodynamic brake 0…1.
    #[serde(default)]
    pub retarder_fill: f64,
    /// Armature current [A] (series-wound drive).
    #[serde(default)]
    pub motor_current: f64,
    /// Field stage in use as a share of the full field (series-wound drive).
    #[serde(default)]
    pub field: f64,
    /// Braking force the dynamic brake is actually delivering [N], positive.
    #[serde(default)]
    pub dynamic_force: f64,
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
            engine_rpm: 0.0,
            engine_fill: 0.0,
            circuit: 0,
            circuit_fill: [0.0; MAX_CIRCUITS],
            circuit_nu: 0.0,
            retarder_fill: 0.0,
            motor_current: 0.0,
            field: 1.0,
            dynamic_force: 0.0,
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
        // Force decays, the tap changer runs back to the zero notch, the circuits empty.
        approach(&mut state.force, 0.0, 1.0e6, dt);
        approach(&mut state.step, 0.0, 5.0, dt);
        for fill in &mut state.circuit_fill {
            approach(fill, 0.0, 1.0, dt);
        }
        approach(&mut state.retarder_fill, 0.0, 1.0, dt);
        state.dynamic_force = 0.0;
        state.motor_current = 0.0;
        if !matches!(spec, TractionSpec::Diesel { .. }) {
            state.engine_rpm = 0.0;
        }
        return;
    }

    let notch = state.notch.clamp(-1.0, 1.0);
    match spec {
        TractionSpec::Curve {
            ramp_time, brake, ..
        } => {
            let target = if notch >= 0.0 {
                notch * spec.available_force(v)
            } else if brake.is_empty() {
                0.0
            } else {
                notch * spec.available_brake_force(v)
            };
            let rate = spec.available_force(v).max(1.0) / ramp_time.max(0.1);
            approach(&mut state.force, target, rate, dt);
        }
        TractionSpec::TapChanger {
            steps,
            step_time,
            max_power,
            motor,
            dynamic_brake,
            ..
        } => {
            let steps = (*steps).max(1) as f64;
            let target = notch.clamp(0.0, 1.0) * steps;
            approach(&mut state.step, target, 1.0 / step_time.max(0.01), dt);
            let ratio = state.step / steps;
            match motor {
                // With motor data the machine equations decide, not a curve.
                Some(motor) => {
                    let (force, current, field) = motor.best_effort(v, ratio, *max_power);
                    state.force = force;
                    state.motor_current = current;
                    state.field = field;
                }
                None => {
                    state.force = ratio * spec.available_force(v);
                    state.motor_current = 0.0;
                    state.field = 1.0;
                }
            }
            apply_dynamic_brake(state, dynamic_brake.as_ref(), electric_ok, v, notch, dt);
        }
        TractionSpec::Converter {
            brake_force,
            brake_power,
            brake_fade_kmh,
            ramp_time,
            regenerative,
            ..
        } => {
            let brake = DynamicBrake {
                max_force: *brake_force,
                max_power: *brake_power,
                fade_out_kmh: *brake_fade_kmh,
                regenerative: *regenerative,
                ramp_time: *ramp_time,
            };
            if notch >= 0.0 {
                let rate = spec.available_force(v).max(1.0) / ramp_time.max(0.1);
                approach(&mut state.force, notch * spec.available_force(v), rate, dt);
                state.dynamic_force = 0.0;
            } else {
                state.force = 0.0;
                apply_dynamic_brake(state, Some(&brake), electric_ok, v, notch, dt);
            }
        }
        TractionSpec::Diesel {
            ramp_time,
            engine,
            transmission,
            hydrodynamic_brake,
            ..
        } => {
            step_diesel(
                state,
                spec,
                engine.as_ref(),
                transmission.as_ref(),
                hydrodynamic_brake.as_ref(),
                *ramp_time,
                v,
                notch,
                dt,
            );
        }
    }
}

/// Dynamic brake: the effort follows the demand with the drive's ramp time.
fn apply_dynamic_brake(
    state: &mut TractionState,
    brake: Option<&DynamicBrake>,
    electric_ok: bool,
    v: f64,
    notch: f64,
    dt: f64,
) {
    let Some(brake) = brake else {
        state.dynamic_force = 0.0;
        return;
    };
    // A regenerative brake needs somewhere to put the energy — without line voltage it is
    // out of action, exactly like in a neutral section.
    let available = if brake.regenerative && !electric_ok {
        0.0
    } else {
        brake.available(v)
    };
    let demand = (-notch).clamp(0.0, 1.0) * available;
    let rate = available.max(1.0) / brake.ramp_time.max(0.1);
    approach(&mut state.dynamic_force, demand, rate, dt);
    state.dynamic_force = state.dynamic_force.min(available);
    if notch < 0.0 {
        state.force = -state.dynamic_force;
    }
}

/// Diesel drive. With an engine map and a transmission this is a torque balance between
/// engine and pump wheel; without them the notch scales the hyperbola as before.
#[allow(clippy::too_many_arguments)]
fn step_diesel(
    state: &mut TractionState,
    spec: &TractionSpec,
    engine: Option<&DieselEngine>,
    transmission: Option<&Transmission>,
    retarder: Option<&HydrodynamicBrake>,
    ramp_time: f64,
    v: f64,
    notch: f64,
    dt: f64,
) {
    // The hydrodynamic brake is independent of the engine — it only needs a turning wheel.
    let brake_force = match retarder {
        Some(retarder) => {
            let demand = (-notch).clamp(0.0, 1.0);
            approach(
                &mut state.retarder_fill,
                demand,
                1.0 / retarder.fill_time.max(0.05),
                dt,
            );
            retarder.force(v, state.retarder_fill)
        }
        None => {
            state.retarder_fill = 0.0;
            0.0
        }
    };
    state.dynamic_force = brake_force;

    let Some(engine) = engine else {
        let target = notch.max(0.0) * spec.available_force(v);
        let rate = spec.available_force(v).max(1.0) / ramp_time.max(0.1);
        approach(&mut state.force, target, rate, dt);
        if brake_force > 0.0 && notch < 0.0 {
            state.force = -brake_force;
        }
        return;
    };

    let demand = notch.max(0.0);
    // Speed governor: the notch is a target engine speed, the governor holds it by opening
    // the rack. Fill governor: the notch *is* the rack, the speed follows from the load.
    let idle_help = ((engine.idle_rpm - state.engine_rpm) * 0.01).clamp(0.0, 1.0);
    let commanded = match engine.governor {
        Governor::Fill => demand.max(idle_help),
        Governor::Speed { steps, droop } => {
            // Droop lets the set speed sag with the rack, so the engine speed in the
            // converter range follows the load instead of standing still.
            let target_rpm = engine.idle_rpm
                + quantise(demand, steps) * (engine.rated_rpm - engine.idle_rpm)
                - droop * engine.rated_rpm * state.engine_fill;
            // A mechanical governor integrates the speed error onto the rack.
            let gain = 1.0 / (engine.response_time.max(0.1) * 100.0);
            (state.engine_fill + (target_rpm - state.engine_rpm) * gain * dt)
                .clamp(0.0, 1.0)
                .max(idle_help)
        }
    };
    approach(
        &mut state.engine_fill,
        commanded,
        1.0 / engine.response_time.max(0.05),
        dt,
    );

    let omega = state.engine_rpm * TAU / 60.0;
    let full_load = engine.full_load_torque(state.engine_rpm);
    // Auxiliaries and internal friction pull the engine down when the rack is closed.
    let drag = full_load * 0.08;

    let (load, force) = match transmission {
        Some(transmission) => step_transmission(state, transmission, engine, demand, v, omega, dt),
        None => {
            let target = demand * spec.available_force(v);
            let rate = spec.available_force(v).max(1.0) / ramp_time.max(0.1);
            approach(&mut state.force, target, rate, dt);
            // Without a transmission the load follows the delivered power.
            let load = if omega > 1.0 {
                state.force * v.abs() / omega
            } else {
                0.0
            };
            (load, state.force)
        }
    };

    // Torque balance of the engine — this is what makes it lug down under load.
    let torque = state.engine_fill * full_load - drag - load;
    let d_rpm = torque / engine.inertia.max(1.0) * dt * 60.0 / TAU;
    state.engine_rpm = (state.engine_rpm + d_rpm).clamp(0.0, engine.max_rpm);

    state.force = if notch < 0.0 && brake_force > 0.0 {
        -brake_force
    } else {
        force
    };
}

/// Hydraulic transmission: change point with hysteresis, filling, torque conversion.
/// Returns (torque taken from the engine [N·m], tractive effort at the wheel [N]).
fn step_transmission(
    state: &mut TractionState,
    transmission: &Transmission,
    engine: &DieselEngine,
    demand: f64,
    v: f64,
    omega_engine: f64,
    dt: f64,
) -> (f64, f64) {
    let count = transmission.circuits.len().min(MAX_CIRCUITS);
    if count == 0 {
        return (0.0, 0.0);
    }
    let kmh = v.abs() * 3.6;

    // Change point. Up when the engaged circuit has run out, down only after the
    // hysteresis — otherwise the transmission hunts on every gradient. Both points move
    // with the notch, that is the primary influence.
    let mut circuit = state.circuit.min(count - 1);
    if circuit + 1 < count && kmh > transmission.shift_up_kmh(circuit, demand) {
        circuit += 1;
    } else if circuit > 0
        && kmh < transmission.shift_up_kmh(circuit - 1, demand) - transmission.hysteresis_kmh
    {
        circuit -= 1;
    }
    state.circuit = circuit;

    // Filling is the power control: quantised into as many steps as the original has.
    // The change itself needs no clutch — the old circuit runs empty while the new one
    // fills, and it does so at its own rate, which is what tears the hole in the tractive
    // effort at the change point.
    let target_fill = quantise(demand, transmission.fill_steps);
    let fill_rate = 1.0 / transmission.fill_time.max(0.05);
    let drain_rate = 1.0 / transmission.drain_time().max(0.05);
    for (i, fill) in state.circuit_fill.iter_mut().enumerate().take(count) {
        let target = if i == circuit { target_fill } else { 0.0 };
        let rate = if target > *fill {
            fill_rate
        } else {
            drain_rate
        };
        approach(fill, target, rate, dt);
    }

    let mut pump_torque = 0.0;
    let mut force = 0.0;
    for i in 0..count {
        let fill = state.circuit_fill[i];
        if fill <= 1e-3 {
            continue;
        }
        let (nu, force_per_torque) = transmission.geometry(i, v, omega_engine);
        let element = transmission.circuits[i];
        let pump = element.pump_torque(omega_engine, nu, fill);
        pump_torque += pump;
        force += pump * element.torque_ratio(nu) * force_per_torque;
        if i == circuit {
            state.circuit_nu = nu;
        }
    }
    let count_factor = transmission.count.max(1) as f64;

    // A fluid transmission cannot stall the engine: if the pump takes more than the engine
    // has, the engine drags down — that is the torque balance in `step_diesel`. Below idle
    // the converter would kill it, so the model lets the circuit slip instead.
    let stall_guard = if state.engine_rpm < engine.idle_rpm * 0.6 {
        (state.engine_rpm / (engine.idle_rpm * 0.6)).clamp(0.0, 1.0)
    } else {
        1.0
    };

    (
        pump_torque * count_factor * stall_guard,
        force * count_factor * stall_guard,
    )
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

    if let TractionSpec::Diesel { engine, .. } = spec {
        if state.start_timer > 0.0 {
            state.start_timer -= dt;
            if state.start_timer <= 0.0 {
                state.engine_running = true;
                state.start_timer = 0.0;
                if let Some(engine) = engine {
                    state.engine_rpm = engine.idle_rpm;
                }
            }
        }
        if !state.engine_running {
            state.engine_rpm = 0.0;
            state.engine_fill = 0.0;
        }
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

/// Shut the diesel engine down.
pub fn stop_engine(state: &mut TractionState) {
    state.engine_running = false;
    state.start_timer = 0.0;
    state.engine_rpm = 0.0;
    state.engine_fill = 0.0;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drive::{Circuit, CircuitKind, SeriesMotor};

    fn diesel_hydraulic() -> TractionSpec {
        TractionSpec::Diesel {
            max_force: 235_000.0,
            max_power: 1_840_000.0,
            v_max: 140.0,
            ramp_time: 4.0,
            start_time: 8.0,
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
                governor: Governor::Speed {
                    steps: 0,
                    droop: 0.04,
                },
                inertia: 60.0,
                response_time: 1.0,
            }),
            transmission: Some(Transmission {
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
            }),
            hydrodynamic_brake: Some(HydrodynamicBrake {
                absorption: 0.35,
                ratio: 4.0,
                wheel_diameter: 1.0,
                max_force: 100_000.0,
                max_power: 1_500_000.0,
                fill_time: 1.5,
                fade_out_kmh: 12.0,
            }),
        }
    }

    fn running(spec: &TractionSpec) -> TractionState {
        let mut state = TractionState {
            battery: true,
            ..Default::default()
        };
        start_engine(&mut state, spec);
        for _ in 0..2000 {
            step(&mut state, spec, 0.0, 1.0 / 200.0);
        }
        state
    }

    #[test]
    fn the_engine_idles_after_starting() {
        let spec = diesel_hydraulic();
        let state = running(&spec);
        assert!(state.engine_running);
        assert!(
            (560.0..660.0).contains(&state.engine_rpm),
            "idle {:.0} 1/min",
            state.engine_rpm
        );
        assert!(state.force.abs() < 1.0, "no effort at the zero notch");
    }

    #[test]
    fn full_notch_at_a_stand_gives_the_starting_effort() {
        let spec = diesel_hydraulic();
        let mut state = running(&spec);
        state.notch = 1.0;
        for _ in 0..1200 {
            step(&mut state, &spec, 0.0, 1.0 / 200.0);
        }
        assert!(
            (180_000.0..300_000.0).contains(&state.force),
            "starting effort {:.0} N",
            state.force
        );
        // The converter is at stall and the governor holds the engine near rated speed.
        assert!(state.circuit_nu.abs() < 0.05);
        assert!(state.engine_rpm > 1300.0, "{:.0} 1/min", state.engine_rpm);
    }

    #[test]
    fn the_transmission_changes_up_with_hysteresis() {
        let spec = diesel_hydraulic();
        let mut state = running(&spec);
        state.notch = 1.0;
        // Accelerate past the change point.
        for _ in 0..400 {
            step(&mut state, &spec, 80.0 / 3.6, 1.0 / 200.0);
        }
        assert_eq!(state.circuit, 1, "must be in the running converter");
        // Just below the change-up point it stays there — that is the hysteresis.
        for _ in 0..400 {
            step(&mut state, &spec, 68.0 / 3.6, 1.0 / 200.0);
        }
        assert_eq!(state.circuit, 1, "no hunting inside the hysteresis");
        // Well below it, it changes back.
        for _ in 0..400 {
            step(&mut state, &spec, 50.0 / 3.6, 1.0 / 200.0);
        }
        assert_eq!(state.circuit, 0);
    }

    #[test]
    fn the_effort_falls_off_towards_the_top_speed() {
        let spec = diesel_hydraulic();
        let mut state = running(&spec);
        state.notch = 1.0;
        let mut effort = Vec::new();
        for kmh in [10.0, 40.0, 100.0, 135.0] {
            for _ in 0..600 {
                step(&mut state, &spec, kmh / 3.6, 1.0 / 200.0);
            }
            effort.push(state.force);
        }
        assert!(
            effort[0] > effort[3],
            "effort must fall: {:?}",
            effort.iter().map(|f| f / 1000.0).collect::<Vec<_>>()
        );
        // Power at the wheel stays inside the engine's rating.
        let power = effort[2] * 100.0 / 3.6;
        assert!(power < 1_900_000.0, "wheel power {power:.0} W");
    }

    #[test]
    fn partial_filling_gives_partial_effort() {
        let spec = diesel_hydraulic();
        let mut full = running(&spec);
        let mut half = full;
        full.notch = 1.0;
        half.notch = 0.4;
        for _ in 0..1200 {
            step(&mut full, &spec, 20.0 / 3.6, 1.0 / 200.0);
            step(&mut half, &spec, 20.0 / 3.6, 1.0 / 200.0);
        }
        assert!(
            half.force < full.force * 0.8,
            "partial filling {:.0} N vs full {:.0} N",
            half.force,
            full.force
        );
        assert!(half.force > 0.0);
    }

    #[test]
    fn the_hydrodynamic_brake_answers_the_negative_notch() {
        let spec = diesel_hydraulic();
        let mut state = running(&spec);
        state.notch = -1.0;
        for _ in 0..1000 {
            step(&mut state, &spec, 100.0 / 3.6, 1.0 / 200.0);
        }
        assert!(state.force < -20_000.0, "retarder {:.0} N", state.force);
        assert!(state.dynamic_force > 20_000.0);
    }

    /// The one thing that tells a hydraulic drive from a stepped gearbox with a soft jolt:
    /// the outgoing converter is empty before the incoming one has taken hold.
    #[test]
    fn the_change_point_tears_a_hole_in_the_tractive_effort() {
        let spec = diesel_hydraulic();
        let mut state = running(&spec);
        state.notch = 1.0;
        for _ in 0..1200 {
            step(&mut state, &spec, 70.0 / 3.6, 1.0 / 200.0);
        }
        let before = state.force;
        let mut lowest = f64::INFINITY;
        for _ in 0..400 {
            step(&mut state, &spec, 74.0 / 3.6, 1.0 / 200.0);
            lowest = lowest.min(state.force);
        }
        for _ in 0..1200 {
            step(&mut state, &spec, 74.0 / 3.6, 1.0 / 200.0);
        }
        let after = state.force;
        assert!(
            lowest < before * 0.8 && lowest < after * 0.8,
            "{:.0} → {:.0} → {:.0} kN over the change point",
            before / 1000.0,
            lowest / 1000.0,
            after / 1000.0
        );
    }

    #[test]
    fn an_empty_converter_does_not_drag() {
        let spec = diesel_hydraulic();
        let mut state = running(&spec);
        state.notch = 1.0;
        for _ in 0..1200 {
            step(&mut state, &spec, 60.0 / 3.6, 1.0 / 200.0);
        }
        assert!(state.force > 50_000.0);
        // Coasting: nothing in the circuits, so nothing holds the train back either.
        state.notch = 0.0;
        for _ in 0..1200 {
            step(&mut state, &spec, 60.0 / 3.6, 1.0 / 200.0);
        }
        assert!(state.force.abs() < 1.0, "drag {:.0} N", state.force);
        assert!(state.circuit_fill.iter().all(|fill| *fill < 0.01));
    }

    #[test]
    fn droop_lets_the_engine_speed_sag_under_load() {
        let with_droop = diesel_hydraulic();
        let mut isochronous = diesel_hydraulic();
        if let TractionSpec::Diesel {
            engine: Some(engine),
            ..
        } = &mut isochronous
        {
            engine.governor = Governor::Speed {
                steps: 0,
                droop: 0.0,
            };
        }
        // Half notch, so the governor has rack left over — at full notch the converter
        // saturates it and both engines lug down the same way.
        let (mut a, mut b) = (running(&with_droop), running(&isochronous));
        a.notch = 0.5;
        b.notch = 0.5;
        for _ in 0..4000 {
            step(&mut a, &with_droop, 0.0, 1.0 / 200.0);
            step(&mut b, &isochronous, 0.0, 1.0 / 200.0);
        }
        // The isochronous governor holds its set speed of 600 + 0.5·900 exactly.
        assert!(
            (b.engine_rpm - 1050.0).abs() < 5.0,
            "{:.0} 1/min",
            b.engine_rpm
        );
        assert!(
            a.engine_rpm < b.engine_rpm - 10.0,
            "droop {:.0} vs isochronous {:.0} 1/min",
            a.engine_rpm,
            b.engine_rpm
        );
    }

    #[test]
    fn a_fill_governed_engine_lugs_down_under_load() {
        let mut spec = diesel_hydraulic();
        if let TractionSpec::Diesel { engine, .. } = &mut spec
            && let Some(engine) = engine
        {
            engine.governor = Governor::Fill;
        }
        let mut state = running(&spec);
        state.notch = 1.0;
        for _ in 0..600 {
            step(&mut state, &spec, 0.0, 1.0 / 200.0);
        }
        let loaded = state.engine_rpm;
        // With the rack wide open at stall the converter holds the engine below rated speed.
        assert!(loaded > 600.0, "engine must not stall: {loaded:.0} 1/min");
        assert!(state.force > 100_000.0);
    }

    #[test]
    fn the_tap_changer_runs_notch_by_notch() {
        let spec = TractionSpec::TapChanger {
            steps: 28,
            max_force: 275_000.0,
            max_power: 3_620_000.0,
            v_max: 150.0,
            step_time: 0.8,
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
            dynamic_brake: None,
        };
        let mut state = TractionState {
            battery: true,
            pantograph: 1.0,
            pantograph_command: true,
            main_switch_command: true,
            line_voltage: NOMINAL_LINE_VOLTAGE,
            ..Default::default()
        };
        state.notch = 1.0;
        // One step time gets nowhere near the top notch.
        for _ in 0..160 {
            step(&mut state, &spec, 0.0, 1.0 / 200.0);
        }
        assert!(state.step < 28.0, "tap changer at {:.1}", state.step);
        for _ in 0..8000 {
            step(&mut state, &spec, 0.0, 1.0 / 200.0);
        }
        assert!((state.step - 28.0).abs() < 0.1);
        assert!(state.force > 200_000.0, "{:.0} N", state.force);
        assert!(state.motor_current <= 1600.0 + 1.0);
    }

    #[test]
    fn a_regenerative_brake_is_dead_without_line_voltage() {
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
        let mut state = TractionState {
            battery: true,
            pantograph: 1.0,
            pantograph_command: true,
            main_switch_command: true,
            line_voltage: NOMINAL_LINE_VOLTAGE,
            notch: -1.0,
            ..Default::default()
        };
        for _ in 0..1000 {
            step(&mut state, &spec, 120.0 / 3.6, 1.0 / 200.0);
        }
        assert!(state.dynamic_force > 50_000.0);
        // Neutral section: the main switch drops out and the brake goes with it.
        state.line_voltage = 0.0;
        for _ in 0..1000 {
            step(&mut state, &spec, 120.0 / 3.6, 1.0 / 200.0);
        }
        assert!(state.dynamic_force < 1.0, "{:.0} N", state.dynamic_force);
    }
}
