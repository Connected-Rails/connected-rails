//! On-board electrical system and drive control (plan ch. 8).
//!
//! No SPICE: a directed state graph of switches and loads. What matters is the order
//! (battery → pantograph → main switch → auxiliaries) and, behind it, the drive model
//! from [`crate::drive`], which this module ticks.

use crate::brakes::approach;
use crate::drive::{
    AsyncMotor, DieselElectric, DieselEngine, DriveMode, DriveSpec, DynamicBrake, ElectricMotor,
    Governor, HydrodynamicBrake, HydrostaticDrive, MAX_CIRCUITS, MAX_DRIVES, MechanicalGearbox,
    MotorGroup, SeriesMotor, Starter, Thermal, TractionSpec, Transmission, quantise,
};
use crate::steam::{SteamControls, SteamLoco, SteamState};
use serde::{Deserialize, Serialize};
use std::f64::consts::TAU;

/// The supply systems are line data as much as vehicle data, so both sides name the same
/// enum: [`track_model::PowerSystem`], stated per track section by
/// [`track_model::TrackNetwork::electrification_at`].
pub use track_model::PowerSystem as SupplySystem;

fn default_rise_time() -> f64 {
    5.0
}

/// The vehicle's own electrical system, as distinct from its traction chains: what it
/// collects power with, and what keeps it alive while it is shut down.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PowerSupply {
    /// What the vehicle's current collectors are built for, most usual first. More than
    /// one is a multi-system vehicle — and in the diagram it is exactly that: one
    /// pantograph block per system, which is how the real ones are built too.
    #[serde(default = "default_systems")]
    pub systems: Vec<SupplySystem>,
    /// Time the pantograph takes to rise [s].
    #[serde(default = "default_rise_time")]
    pub rise_time: f64,
    /// Battery voltage [V]; 0 = no battery, and then nothing starts at all.
    #[serde(default)]
    pub battery_voltage: f64,
    /// Battery capacity [Ah].
    #[serde(default)]
    pub battery_capacity: f64,
    /// Own supply that stands in for the contact line [V]; 0 = none. A test rig, or a
    /// vehicle whose diagram has a `voltage-source` instead of a pantograph.
    #[serde(default)]
    pub source_voltage: f64,
}

fn default_systems() -> Vec<SupplySystem> {
    vec![SupplySystem::default()]
}

impl Default for PowerSupply {
    fn default() -> Self {
        Self {
            systems: default_systems(),
            rise_time: default_rise_time(),
            battery_voltage: 110.0,
            battery_capacity: 250.0,
            source_voltage: 0.0,
        }
    }
}

impl PowerSupply {
    /// The system the vehicle is most at home under — what the pantograph rises for by
    /// default and what a diagram with a single pantograph block says.
    pub fn system(&self) -> SupplySystem {
        self.systems.first().copied().unwrap_or_default()
    }

    /// Can the vehicle work under `line`? A section with no wire answers no, and so does
    /// one carrying a system this vehicle was not built for.
    pub fn accepts(&self, line: track_model::Electrification) -> bool {
        line.is_some_and(|system| self.systems.contains(&system))
    }

    /// Charge the battery holds when full [As].
    pub fn battery_charge(&self) -> f64 {
        self.battery_capacity.max(0.0) * 3600.0
    }

    /// Current the standing load takes off the battery [A] — control circuits, lighting,
    /// the compressor's own contactor. A rough figure, but enough that a loco left with
    /// the battery on overnight will not start in the morning.
    pub fn standing_load(&self) -> f64 {
        12.0
    }

    /// Current cranking a diesel engine takes [A].
    pub fn cranking_load(&self) -> f64 {
        450.0
    }
}

/// Below this share of its charge the battery will no longer crank an engine.
pub const BATTERY_CRANKING_MINIMUM: f64 = 0.15;

/// Nominal voltage of the German railway power network [V] — 15 kV 16.7 Hz.
pub const NOMINAL_LINE_VOLTAGE: f64 = 15_000.0;
/// Frequency of the German railway power network [Hz].
pub const LINE_FREQUENCY: f64 = 16.7;

/// Serde default for the range selector: a vehicle without a two-range gearbox is
/// always in the road gear, and so is one whose save file predates the selector.
fn yes() -> bool {
    true
}

/// State of one traction chain. Everything here belongs to a single drive — a vehicle
/// with two engines has two of these; what the whole vehicle shares stays in
/// [`TractionState`].
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct DriveState {
    /// Power controller of this chain: −1 … +1 (negative = dynamic brake). Chains on the
    /// shared handle get it copied in from [`TractionState::notch`] every step.
    pub notch: f64,
    /// Switched on by the driver. A shut-down chain delivers nothing.
    pub enabled: bool,
    /// Current tap changer notch (only `TapChanger`).
    pub step: f64,
    /// Diesel engine running.
    pub engine_running: bool,
    /// Remaining cranking time [s].
    pub start_timer: f64,
    /// Current tractive effort [N], positive = traction, negative = dynamic brake.
    pub force: f64,
    /// Engine speed [1/min] (diesel with an engine map).
    pub engine_rpm: f64,
    /// Fuel rack 0…1 (diesel with an engine map).
    pub engine_fill: f64,
    /// Engaged hydraulic circuit (diesel-hydraulic).
    pub circuit: usize,
    /// Filling of the hydraulic circuits 0…1.
    pub circuit_fill: [f64; MAX_CIRCUITS],
    /// Speed ratio ν of the engaged circuit — the transmission's working point.
    pub circuit_nu: f64,
    /// Road gear engaged (as opposed to the shunting gear of a two-range gearbox). A
    /// vehicle without one stays in it. See [`TractionState::road_gear`].
    #[serde(default = "yes")]
    pub road_gear: bool,
    /// Filling of the hydrodynamic brake 0…1.
    pub retarder_fill: f64,
    /// Engaged gear of a mechanical gearbox.
    #[serde(default)]
    pub gear: usize,
    /// Clutch of a mechanical gearbox, 0 = out, 1 = fully in.
    #[serde(default)]
    pub clutch: f64,
    /// Time left of the gear change in progress [s].
    #[serde(default)]
    pub shift_timer: f64,
    /// Swash plate of a hydrostatic drive, 0…1.
    #[serde(default)]
    pub displacement: f64,
    /// Armature current [A] (series-wound drive).
    pub motor_current: f64,
    /// Field stage in use as a share of the full field (series-wound drive).
    pub field: f64,
    /// Braking force the dynamic brake is actually delivering [N], positive.
    pub dynamic_force: f64,
    /// Ramped force of the electric brake of a diesel-electric drive [N] — its own state,
    /// because `dynamic_force` also carries the retarder's share.
    pub electric_brake: f64,
    /// Contactor position of a resistance start, continuous between the notches.
    #[serde(default)]
    pub contactor: f64,
    /// Motor grouping currently in effect.
    #[serde(default)]
    pub group: MotorGroup,
    /// Series resistance currently in the motor string [Ω].
    #[serde(default)]
    pub starting_resistance: f64,
    /// Load regulator of a diesel-electric drive: generator voltage as a share of the
    /// highest one (0…1).
    #[serde(default)]
    pub regulator: f64,
    /// Generator voltage [V] (diesel-electric).
    #[serde(default)]
    pub generator_voltage: f64,
    /// Slip of the induction motors (three-phase drive).
    #[serde(default)]
    pub slip: f64,
    /// Temperature of the traction motors [°C].
    #[serde(default)]
    pub motor_temp: f64,
    /// Temperature of the starting resistors [°C].
    #[serde(default)]
    pub resistor_temp: f64,
    /// Temperature of the braking resistors [°C].
    #[serde(default)]
    pub brake_resistor_temp: f64,
    /// Blower demand 0…1 the cooling system is being asked for.
    #[serde(default)]
    pub blower: f64,
    /// Boiler and fire of a steam locomotive. `None` until the chain is first stepped.
    #[serde(default)]
    pub steam: Option<SteamState>,
}

impl DriveState {
    /// A chain as it sits before the driver touches anything: switched on, shut down, cold.
    pub fn new() -> Self {
        Self {
            enabled: true,
            road_gear: true,
            field: 1.0,
            motor_temp: AMBIENT_TEMP,
            resistor_temp: AMBIENT_TEMP,
            brake_resistor_temp: AMBIENT_TEMP,
            ..Self::default()
        }
    }

    /// Hottest component of the chain [°C] — what a temperature gauge in the cab shows.
    pub fn peak_temp(&self) -> f64 {
        self.motor_temp
            .max(self.resistor_temp)
            .max(self.brake_resistor_temp)
    }
}

/// Temperature a vehicle starts out at [°C].
pub const AMBIENT_TEMP: f64 = 20.0;

/// Runs a thermal model one step and returns the derating factor it leaves behind.
fn heat(thermal: Option<&Thermal>, temp: &mut f64, heat_w: f64, blower: f64, dt: f64) -> f64 {
    match thermal {
        Some(thermal) => {
            *temp = thermal.step(*temp, heat_w, blower, dt);
            thermal.derate(*temp)
        }
        None => {
            approach(temp, AMBIENT_TEMP, 5.0, dt);
            1.0
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
    /// What the line above this vehicle is electrified with, straight off the track
    /// (`None` = no wire, or a neutral section). The voltage alone cannot decide whether
    /// the main switch may close: 25 kV is plenty of volts for a 15 kV loco and still the
    /// wrong system.
    #[serde(default)]
    pub line_system: track_model::Electrification,
    /// Range selector of a two-range gearbox: `true` = road gear, `false` = shunting gear.
    /// Only a vehicle with `Transmission::shunting_ratio` has one, and the change only
    /// takes at a stand — the dog clutch cannot be shifted under load.
    #[serde(default = "yes")]
    pub road_gear: bool,
    /// Shared power controller: −1 … +1 (negative = dynamic brake). Commands every chain
    /// with `throttle == 0`; chains with their own handle read `DriveState::notch`.
    pub notch: f64,
    /// The machine stands in back gear: its effort pushes the train against its own
    /// direction of travel.
    ///
    /// The drive models only ever work out *how hard* the machine pulls — that is the
    /// same number in either gear. Which way it pulls is the reverser's business, and
    /// [`crate::physics`] puts the sign on at the rail. Without it a train could not set
    /// back, and setting back is half of shunting (plan ch. 11).
    #[serde(default)]
    pub back_gear: bool,
    /// Air compressor switched on.
    pub compressor: bool,
    /// Charge left in the battery, 0…1.
    #[serde(default)]
    pub battery_charge: f64,
    /// Train line (heating) switched on.
    pub train_line: bool,
    /// Regulator, cutoff, blower, damper and injectors of a steam locomotive — written by
    /// the cab in the same way as `notch`, so the boiler needs no path of its own into here.
    #[serde(default)]
    pub steam_controls: SteamControls,
    /// Total tractive effort of the vehicle [N], the sum over the chains that are
    /// delivering. Positive = traction, negative = dynamic brake.
    pub force: f64,
    /// Selected power source. On a dual-mode vehicle only the chains of this mode work;
    /// where every chain agrees it is simply that mode.
    #[serde(default)]
    pub mode: DriveMode,
    /// State of each traction chain, in the order of `VehicleSpec::drives`. Entries past
    /// the vehicle's chain count are unused.
    #[serde(default)]
    pub drives: [DriveState; MAX_DRIVES],
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
            line_system: None,
            notch: 0.0,
            back_gear: false,
            road_gear: true,
            compressor: false,
            // A vehicle is put into service with a charged battery.
            battery_charge: 1.0,
            train_line: false,
            force: 0.0,
            mode: DriveMode::default(),
            steam_controls: SteamControls::default(),
            drives: [DriveState::new(); MAX_DRIVES],
        }
    }
}

impl TractionState {
    /// Started up and ready to run?
    pub fn ready(&self) -> bool {
        self.battery && (self.main_switch || self.any_engine_running())
    }

    /// Is any diesel engine of the vehicle running?
    pub fn any_engine_running(&self) -> bool {
        self.drives.iter().any(|d| d.engine_running)
    }

    /// The chains of the selected mode that the driver has switched on, as indices into
    /// `VehicleSpec::drives`.
    pub fn active<'a>(&'a self, specs: &'a [DriveSpec]) -> impl Iterator<Item = usize> + 'a {
        specs
            .iter()
            .enumerate()
            .take(MAX_DRIVES)
            .filter(move |(i, s)| s.mode == self.mode && self.drives[*i].enabled)
            .map(|(i, _)| i)
    }
}

/// One simulation step for the on-board electrical system and every traction chain of a
/// vehicle. `state.force` comes out as the sum over the chains.
pub fn step(state: &mut TractionState, specs: &[DriveSpec], supply: &PowerSupply, v: f64, dt: f64) {
    update_vehicle_power(state, specs, supply, dt);

    let count = specs.len().min(MAX_DRIVES);
    let (notch, mode, main_switch, steam_controls, road_gear) = (
        state.notch,
        state.mode,
        state.main_switch,
        state.steam_controls,
        state.road_gear,
    );
    let mut total = 0.0;
    for (drive, chain) in specs.iter().zip(state.drives.iter_mut()) {
        // A chain of the mode that is not selected is dead, whatever its own switch says.
        let selected = drive.mode == mode;
        // Chains on the shared handle follow the cab's power controller.
        if drive.throttle == 0 {
            chain.notch = notch;
        }
        // The range change is a dog clutch: it goes in at a stand and nowhere else.
        if v.abs() < 0.3 {
            chain.road_gear = road_gear;
        }
        let live = selected && chain.enabled;
        step_drive(
            chain,
            &drive.traction,
            main_switch,
            live,
            &steam_controls,
            v,
            dt,
        );
        // `force` already carries the dynamic brake as a negative effort — the blending in
        // `brakes` reads `dynamic_force` separately.
        total += chain.force;
    }
    // Chains the vehicle does not have must not keep old force around.
    for chain in &mut state.drives[count..] {
        *chain = DriveState::new();
    }
    state.force = total;
}

/// One simulation step of a single traction chain.
///
/// `live` is false for a chain that is switched off or belongs to the mode that is not
/// selected — it then coasts down exactly like one without power.
#[allow(clippy::too_many_arguments)]
fn step_drive(
    state: &mut DriveState,
    spec: &TractionSpec,
    main_switch: bool,
    live: bool,
    steam_controls: &SteamControls,
    v: f64,
    dt: f64,
) {
    update_power(state, spec, dt);

    // A boiler does not stop being a boiler because the regulator is shut — the fire burns,
    // the injectors feed and the safety valves lift whether the loco is "live" or not.
    if let TractionSpec::Steam { loco, .. } = spec {
        step_steam(state, loco, steam_controls, live, v, dt);
        return;
    }

    // The main switch has already checked that the voltage is one this vehicle can work
    // with (see `update_vehicle_power`); here it is enough that it is closed.
    let electric_ok = main_switch;
    let powered = live
        && match spec {
            TractionSpec::Diesel { .. } => state.engine_running,
            TractionSpec::Steam { .. } => true,
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
        approach(&mut state.contactor, 0.0, 5.0, dt);
        approach(&mut state.regulator, 0.0, 1.0, dt);
        state.dynamic_force = 0.0;
        state.electric_brake = 0.0;
        state.motor_current = 0.0;
        state.generator_voltage = 0.0;
        state.slip = 0.0;
        state.blower = 0.0;
        approach(&mut state.motor_temp, AMBIENT_TEMP, 1.0, dt);
        approach(&mut state.resistor_temp, AMBIENT_TEMP, 2.0, dt);
        approach(&mut state.brake_resistor_temp, AMBIENT_TEMP, 2.0, dt);
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
            max_force,
            motor,
            starter,
            dynamic_brake,
            ..
        } => {
            let steps = (*steps).max(1) as f64;
            let target = notch.clamp(0.0, 1.0) * steps;
            approach(&mut state.step, target, 1.0 / step_time.max(0.01), dt);
            let ratio = state.step / steps;
            match (motor, starter) {
                // Contactor drive: the resistors and the grouping set the working point, the
                // transformer (if there is one) only scales it.
                (Some(motor), Some(starter)) => {
                    step_starter(state, motor, starter, ratio, *max_power, v, notch, dt);
                    state.force = state.force.min(*max_force);
                }
                // With motor data the machine equations decide, not a curve.
                (Some(motor), None) => {
                    let (force, current, field) = motor.best_effort(v, ratio, *max_power);
                    let derate = heat(
                        motor.thermal.as_ref(),
                        &mut state.motor_temp,
                        motor.losses(current, 0.0),
                        state.blower,
                        dt,
                    );
                    state.force = force * derate;
                    state.motor_current = current;
                    state.field = field;
                    state.group = MotorGroup::Parallel;
                    state.starting_resistance = 0.0;
                }
                (None, _) => {
                    state.force = ratio * spec.available_force(v);
                    state.motor_current = 0.0;
                    state.field = 1.0;
                }
            }
            state.blower = (state.force.abs() / max_force.max(1.0)).clamp(0.0, 1.0);
            apply_dynamic_brake(state, dynamic_brake.as_ref(), electric_ok, v, notch, dt);
        }
        TractionSpec::Converter {
            brake_force,
            brake_power,
            brake_fade_kmh,
            ramp_time,
            regenerative,
            max_force,
            max_power,
            motor,
            ..
        } => {
            let brake = DynamicBrake {
                max_force: *brake_force,
                max_power: *brake_power,
                fade_out_kmh: *brake_fade_kmh,
                regenerative: *regenerative,
                ramp_time: *ramp_time,
                thermal: None,
            };
            if notch >= 0.0 {
                let available = match motor {
                    Some(motor) => step_async(
                        state,
                        motor,
                        v,
                        dt,
                        (*max_force).min(max_power / v.abs().max(0.5)),
                    ),
                    None => spec.available_force(v),
                };
                let rate = available.max(1.0) / ramp_time.max(0.1);
                approach(&mut state.force, notch * available, rate, dt);
                state.dynamic_force = 0.0;
            } else {
                state.force = 0.0;
                state.slip = 0.0;
                apply_dynamic_brake(state, Some(&brake), electric_ok, v, notch, dt);
            }
            state.blower = (state.force.abs() / max_force.max(1.0)).clamp(0.0, 1.0);
        }
        TractionSpec::Diesel {
            ramp_time,
            engine,
            transmission,
            electric,
            hydrodynamic_brake,
            dynamic_brake,
            ..
        } => {
            step_diesel(
                state,
                spec,
                engine.as_ref(),
                transmission.as_deref(),
                electric.as_ref(),
                hydrodynamic_brake.as_ref(),
                dynamic_brake.as_ref(),
                *ramp_time,
                v,
                notch,
                dt,
            );
        }
        // Handled before the power check — a boiler runs on whatever the driver left it on.
        TractionSpec::Steam { .. } => {}
    }
}

/// Resistance start: the contactors walk towards the position the handle asks for, the
/// grouping and the series resistance follow from where they have got to.
///
/// A chopper drive takes the same road without the steps — the resistance is simply the
/// one the chopper's duty cycle stands in for, and nothing is burnt.
#[allow(clippy::too_many_arguments)]
fn step_starter(
    state: &mut DriveState,
    motor: &SeriesMotor,
    starter: &Starter,
    supply_ratio: f64,
    max_power: f64,
    v: f64,
    notch: f64,
    dt: f64,
) {
    let target = starter.target(notch.clamp(0.0, 1.0));
    let rate = 1.0 / starter.step_time.max(0.02);
    if starter.chopper {
        // No contactors to walk: the chopper follows the handle at once.
        approach(&mut state.contactor, target, rate * 8.0, dt);
    } else {
        approach(&mut state.contactor, target, rate, dt);
    }

    let pos = state.contactor.round().max(0.0) as usize;
    let (group, mut resistance) = starter.at(pos);
    if starter.chopper {
        // Between two positions the chopper interpolates instead of jumping.
        let (_, next) = starter.at((pos + 1).min(starter.positions().saturating_sub(1)));
        let t = (state.contactor - pos as f64).clamp(-1.0, 1.0);
        resistance = if t >= 0.0 {
            resistance + (next - resistance) * t
        } else {
            resistance
        };
    }
    state.group = group;
    state.starting_resistance = resistance;

    // Motors in one string share the supply voltage and the resistance in it. The last
    // grouping is the reference the motor's rated voltage is stated for.
    let reference = starter
        .groups
        .last()
        .copied()
        .unwrap_or(MotorGroup::Parallel)
        .in_series(motor.count);
    let in_series = group.in_series(motor.count);
    let ratio = supply_ratio * reference / in_series;
    let r_ext = resistance / in_series;

    let (force, current, field) = motor.best_effort_with(v, ratio, max_power, r_ext);
    // Everything the resistor drops is heat; the motors add their own copper losses.
    let resistor_heat = current * current * resistance;
    let motor_heat = motor.losses(current, 0.0);
    let resistor_derate = heat(
        starter.thermal.as_ref(),
        &mut state.resistor_temp,
        resistor_heat,
        state.blower,
        dt,
    );
    let motor_derate = heat(
        motor.thermal.as_ref(),
        &mut state.motor_temp,
        motor_heat,
        state.blower,
        dt,
    );
    state.force = force * resistor_derate.min(motor_derate);
    state.motor_current = current;
    state.field = field;
}

/// Steam locomotive: the boiler runs whatever the driver is doing, the regulator only
/// decides how much of it goes into the cylinders.
fn step_steam(
    state: &mut DriveState,
    loco: &SteamLoco,
    controls: &SteamControls,
    live: bool,
    v: f64,
    dt: f64,
) {
    let boiler = state.steam.get_or_insert_with(|| SteamState::new(loco));
    // A shut-down chain is a locomotive standing with the regulator shut — the fire still
    // burns and the injectors still work.
    let controls = if live {
        *controls
    } else {
        SteamControls {
            regulator: 0.0,
            ..*controls
        }
    };
    let force = crate::steam::step(loco, boiler, &controls, v, dt);
    state.force = force;
    state.dynamic_force = 0.0;
    // The gauges of the cab read the chain's own fields, so the boiler shows up in the same
    // places a diesel's engine speed does.
    state.engine_rpm = v.abs() / (std::f64::consts::PI * loco.wheel_diameter.max(0.1)) * 60.0;
    state.engine_fill = controls.regulator.clamp(0.0, 1.0);
    state.engine_running = boiler.pressure > 0.5;
}

/// Three-phase drive: the converter puts the slip where the torque is and the machine does
/// the rest. Returns the effort available at this speed [N].
fn step_async(state: &mut DriveState, motor: &AsyncMotor, v: f64, dt: f64, ceiling: f64) -> f64 {
    let (force, slip) = motor.best_effort(v);
    state.slip = slip;
    let derate = heat(
        motor.thermal.as_ref(),
        &mut state.motor_temp,
        motor.losses(state.force, v),
        state.blower,
        dt,
    );
    (force * derate).min(ceiling)
}

/// Dynamic brake: the effort follows the demand with the drive's ramp time.
fn apply_dynamic_brake(
    state: &mut DriveState,
    brake: Option<&DynamicBrake>,
    electric_ok: bool,
    v: f64,
    notch: f64,
    dt: f64,
) {
    let Some(brake) = brake else {
        state.dynamic_force = 0.0;
        approach(&mut state.brake_resistor_temp, AMBIENT_TEMP, 5.0, dt);
        return;
    };
    // A regenerative brake needs somewhere to put the energy — without line voltage it is
    // out of action, exactly like in a neutral section.
    let available = if brake.regenerative && !electric_ok {
        0.0
    } else {
        brake.available(v)
    };
    // A rheostatic brake puts everything it takes off the train into the resistor bank, and
    // a bank that has run hot cannot take any more.
    let derate = heat(
        brake.thermal.as_ref(),
        &mut state.brake_resistor_temp,
        if brake.regenerative {
            0.0
        } else {
            state.dynamic_force * v.abs()
        },
        state.blower,
        dt,
    );
    let available = available * derate;
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
    state: &mut DriveState,
    spec: &TractionSpec,
    engine: Option<&DieselEngine>,
    transmission: Option<&Transmission>,
    electric: Option<&DieselElectric>,
    retarder: Option<&HydrodynamicBrake>,
    dynamic_brake: Option<&DynamicBrake>,
    ramp_time: f64,
    v: f64,
    notch: f64,
    dt: f64,
) {
    // The hydrodynamic brake is independent of the engine — it only needs a turning wheel.
    let retarder_force = match retarder {
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
    // Electric brake of a diesel-electric drive: motors into the braking resistors. A
    // regenerative flag stays dead — a diesel loco has no line to feed back into.
    let electric_force = match dynamic_brake {
        Some(brake) if !brake.regenerative => {
            let available = brake.available(v);
            let demand = (-notch).clamp(0.0, 1.0) * available;
            let rate = available.max(1.0) / brake.ramp_time.max(0.1);
            approach(&mut state.electric_brake, demand, rate, dt);
            state.electric_brake = state.electric_brake.min(available);
            state.electric_brake
        }
        _ => {
            state.electric_brake = 0.0;
            0.0
        }
    };
    let brake_force = retarder_force + electric_force;
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

    // Gearbox and hydrostatic drive come straight off the spec — the signature carries
    // enough already.
    let (gearbox, hydrostatic) = match spec {
        TractionSpec::Diesel {
            gearbox,
            hydrostatic,
            ..
        } => (gearbox.as_ref(), hydrostatic.as_ref()),
        _ => (None, None),
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

    let max_force = match spec {
        TractionSpec::Diesel { max_force, .. } => *max_force,
        _ => f64::INFINITY,
    };
    let (load, force) = match (transmission, electric, gearbox, hydrostatic) {
        (Some(transmission), ..) => {
            step_transmission(state, transmission, engine, demand, v, omega, dt)
        }
        (None, Some(electric), ..) => {
            // The motors would pull harder than the running gear may; `max_force` is the
            // figure the works plate carries and it caps them.
            let (load, force) = step_diesel_electric(state, electric, demand, v, omega, dt);
            (load, force.min(max_force))
        }
        (None, None, Some(gearbox), _) => {
            let (load, force) = step_gearbox(state, gearbox, engine, demand, v, dt);
            (load, force.min(max_force))
        }
        (None, None, None, Some(hydrostatic)) => {
            let (load, force) = step_hydrostatic(state, hydrostatic, engine, demand, v, omega, dt);
            (load, force.min(max_force))
        }
        (None, None, None, None) => {
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

/// Slip [1/min] at which the clutch is passing everything its lining can hold.
const CLUTCH_FULL_SLIP: f64 = 60.0;

/// Mechanical gearbox: clutch, gear change and the hole each change tears in the effort.
///
/// The driver goes by the engine speed, as he does in a railbus — up at the top of the
/// range, down when the engine starts to labour. Nothing multiplies the torque, so getting
/// away from a stand is the clutch slipping and nothing else.
///
/// Returns (torque taken from the engine [N·m], tractive effort at the wheel [N]).
fn step_gearbox(
    state: &mut DriveState,
    gearbox: &MechanicalGearbox,
    engine: &DieselEngine,
    demand: f64,
    v: f64,
    dt: f64,
) -> (f64, f64) {
    let count = gearbox.gears.len();
    if count == 0 {
        return (0.0, 0.0);
    }
    state.gear = state.gear.min(count - 1);

    if state.shift_timer > 0.0 {
        state.shift_timer -= dt;
    } else if state.gear + 1 < count && state.engine_rpm > gearbox.shift_up_rpm {
        state.gear += 1;
        state.shift_timer = gearbox.shift_time;
    } else if state.gear > 0 && state.engine_rpm < gearbox.shift_down_rpm {
        state.gear -= 1;
        state.shift_timer = gearbox.shift_time;
    }
    let gear = state.gear;
    let sync_rpm = gearbox.sync_rpm(gear, v);

    // The clutch is out while the gear is being changed. Getting away, it comes in with the
    // engine speed — nothing below the take-up speed, everything at rated speed, which is
    // what a centrifugal clutch does and what a driver's foot does. Once the gear turns the
    // engine faster than its idle the clutch is simply in, and that gives the engine brake.
    let rolling = sync_rpm > engine.idle_rpm;
    let take_up = engine.idle_rpm * 1.3;
    let by_speed =
        ((state.engine_rpm - take_up) / (engine.rated_rpm - take_up).max(1.0)).clamp(0.0, 1.0);
    let target = if state.shift_timer > 0.0 {
        0.0
    } else if rolling {
        1.0
    } else if demand > 0.02 {
        by_speed
    } else {
        0.0
    };
    approach(
        &mut state.clutch,
        target,
        1.0 / gearbox.clutch_time.max(0.05),
        dt,
    );

    // Torque across the clutch grows with the slip until the lining passes all it can hold.
    // ponytail: linear up to CLUTCH_FULL_SLIP, flat above — a friction model per lining
    // would need figures nobody has.
    let slip = (state.engine_rpm - sync_rpm) / CLUTCH_FULL_SLIP;
    let torque = state.clutch * gearbox.clutch_torque * slip.clamp(-1.0, 1.0);
    let radius = (gearbox.wheel_diameter / 2.0).max(0.05);
    let force = torque * gearbox.total_ratio(gear) * gearbox.efficiency / radius;

    // Stalling it is part of the deal: with the clutch in, the load can drag the engine
    // below the speed at which it keeps itself alight.
    if state.clutch > 0.05 && state.engine_rpm < engine.idle_rpm * 0.6 {
        state.engine_running = false;
        state.engine_rpm = 0.0;
    }
    (torque, force)
}

/// Hydrostatic drive: the swash plate follows the controller, the relief valve caps the
/// effort at a stand and the engine's power caps it above.
///
/// Returns (torque taken from the engine [N·m], tractive effort at the wheel [N]).
fn step_hydrostatic(
    state: &mut DriveState,
    drive: &HydrostaticDrive,
    engine: &DieselEngine,
    demand: f64,
    v: f64,
    omega_engine: f64,
    dt: f64,
) -> (f64, f64) {
    approach(
        &mut state.displacement,
        demand.clamp(0.0, 1.0),
        1.0 / drive.response_time.max(0.05),
        dt,
    );
    // Limiting-load control: the swash plate goes back as the engine starts to labour, so
    // the drive settles on a working point instead of pulling its engine down and dying.
    let held = ((state.engine_rpm - engine.idle_rpm)
        / (engine.rated_rpm - engine.idle_rpm).max(1.0))
    .clamp(0.0, 1.0);
    let power = engine.full_load_torque(state.engine_rpm) * omega_engine * held;
    let force = drive.force(power, v, state.displacement);
    // What the pump takes off the engine. At a stand the oil goes over the relief valve
    // instead of into motion, so the load is figured at walking pace rather than at zero.
    let load = if omega_engine > 1.0 {
        force * v.abs().max(1.0) / (omega_engine * drive.efficiency.max(0.1))
    } else {
        0.0
    };
    (load, force)
}

/// Diesel-electric drive: the load regulator holds the generator on the power the notch
/// asks for, and the motors take whatever voltage and current that works out to.
///
/// Returns (torque taken from the engine [N·m], tractive effort at the wheel [N]).
fn step_diesel_electric(
    state: &mut DriveState,
    electric: &DieselElectric,
    demand: f64,
    v: f64,
    omega_engine: f64,
    dt: f64,
) -> (f64, f64) {
    // Field weakening only helps a DC drive; an inverter has no field stage to pick.
    let field = match &electric.motor {
        ElectricMotor::Dc(motor) => {
            let power = demand * electric.generator_power;
            let ratio = state.regulator;
            let (_, _, field) = motor.best_effort(v, ratio, power.max(1.0));
            field
        }
        ElectricMotor::Ac(_) => 1.0,
    };
    state.field = field;

    let power = demand * electric.generator_power;
    let target = electric.regulator_ratio(v, power, field);
    approach(
        &mut state.regulator,
        target,
        1.0 / electric.regulator_time.max(0.05),
        dt,
    );
    let (force, current) = electric.effort(v, state.regulator, field);
    state.motor_current = current;
    state.generator_voltage = state.regulator * electric.max_voltage;
    if let ElectricMotor::Ac(motor) = &electric.motor {
        state.slip = motor.best_effort(v).1;
    }

    // Blower: with the engine running it turns anyway, and harder the more the drive works.
    let working = (force.abs() * v.abs() / electric.generator_power.max(1.0)).clamp(0.0, 1.0);
    state.blower = electric.blower_idle_share.clamp(0.0, 1.0).max(working);
    let losses = match &electric.motor {
        ElectricMotor::Dc(motor) => motor.losses(current, 0.0),
        ElectricMotor::Ac(motor) => motor.losses(force, v),
    };
    let derate = heat(
        electric.motor.thermal().as_ref(),
        &mut state.motor_temp,
        losses,
        state.blower,
        dt,
    );
    let force = force * derate;

    // What the generator takes off the engine: the electrical power plus its own losses.
    let electrical = force * v.abs() / electric.generator_efficiency.clamp(0.1, 1.0);
    let load = if omega_engine > 1.0 {
        electrical / omega_engine
    } else {
        0.0
    };
    (load, force)
}

/// Hydraulic transmission: change point with hysteresis, filling, torque conversion.
/// Returns (torque taken from the engine [N·m], tractive effort at the wheel [N]).
fn step_transmission(
    state: &mut DriveState,
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
    // effort at the change point. A speed-controlled transmission fills once and stays
    // full; there the notch moves the engine, not the oil.
    let target_fill = if transmission.speed_controlled {
        f64::from(demand > 0.02)
    } else {
        quantise(demand, transmission.fill_steps)
    };
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
        let (nu, force_per_torque) = transmission.geometry(i, v, omega_engine, state.road_gear);
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
/// Battery, pantograph and main switch — the part of the electrical system the whole
/// vehicle shares, whatever its chains are.
fn update_vehicle_power(
    state: &mut TractionState,
    specs: &[DriveSpec],
    supply: &PowerSupply,
    dt: f64,
) {
    // A battery that has run flat is the same thing as no battery: nothing switches on.
    let flat = supply.battery_voltage > 0.0 && state.battery_charge <= 0.0;
    if flat {
        state.battery = false;
    }
    if !state.battery {
        state.pantograph_command = false;
        state.main_switch_command = false;
        state.compressor = false;
    }

    // The pantograph needs its rise time to go up and rather less to come down. A shoe
    // does neither — it is simply there.
    let target = if state.pantograph_command && state.battery {
        1.0
    } else {
        0.0
    };
    if supply.system().is_third_rail() {
        state.pantograph = target;
    } else {
        let rise = 1.0 / supply.rise_time.max(0.1);
        let rate = if target > state.pantograph {
            rise
        } else {
            rise * 5.0 / 3.0
        };
        approach(&mut state.pantograph, target, rate, dt);
    }

    // Main switch: only over a wire this vehicle is built for, and it drops out on loss of
    // voltage (neutral section!) or where the system changes under it. A 15 kV loco under
    // 25 kV keeps its switch open, and rightly so — the volts are there, the system is not.
    let contact = state.pantograph > 0.98;
    let system_ok = supply.accepts(state.line_system)
        && state.line_voltage >= state.line_system.map_or(f64::INFINITY, |s| s.minimum());
    let own_supply = supply.source_voltage >= supply.system().minimum();
    state.main_switch =
        state.main_switch_command && state.battery && (contact && system_ok || own_supply);

    // Battery: the standing load drains it while nothing is charging, and anything that is
    // running charges it back up.
    if supply.battery_voltage > 0.0 && supply.battery_capacity > 0.0 {
        let charging = state.main_switch || state.any_engine_running();
        let amps = if charging {
            // Recharges in roughly an hour of running.
            -supply.battery_capacity
        } else if state.battery {
            supply.standing_load()
        } else {
            0.0
        };
        let share = amps * dt / supply.battery_charge();
        state.battery_charge = (state.battery_charge - share).clamp(0.0, 1.0);
    }

    // A vehicle whose chains all agree has no mode selector; keep the mode on whatever it
    // can actually run on, so a diesel railcar does not sit dead in `Electric`.
    if let Some(first) = specs.first()
        && !specs.iter().any(|s| s.mode == state.mode)
    {
        state.mode = first.mode;
    }
}

/// Cranking and shutdown of one chain's diesel engine.
fn update_power(state: &mut DriveState, spec: &TractionSpec, dt: f64) {
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

/// Crank the diesel engine of one chain (needs the battery, which the caller checks).
pub fn start_engine(state: &mut DriveState, spec: &TractionSpec, battery: bool) {
    if let TractionSpec::Diesel { start_time, .. } = spec
        && battery
        && !state.engine_running
        && state.start_timer <= 0.0
    {
        state.start_timer = *start_time;
    }
}

/// Cranking takes far more out of the battery than standing does — call it once per step
/// while an engine is turning over. Returns false when there is not enough left to try.
pub fn crank_battery(state: &mut TractionState, supply: &PowerSupply, dt: f64) -> bool {
    if supply.battery_voltage <= 0.0 || supply.battery_capacity <= 0.0 {
        return true;
    }
    if state.battery_charge < BATTERY_CRANKING_MINIMUM {
        return false;
    }
    let share = supply.cranking_load() * dt / supply.battery_charge();
    state.battery_charge = (state.battery_charge - share).max(0.0);
    true
}

/// Shut the diesel engine down.
pub fn stop_engine(state: &mut DriveState) {
    state.engine_running = false;
    state.start_timer = 0.0;
    state.engine_rpm = 0.0;
    state.engine_fill = 0.0;
}

#[cfg(test)]
mod supply_tests {
    use super::*;

    fn drive() -> DriveSpec {
        DriveSpec::new(TractionSpec::Converter {
            max_force: 300_000.0,
            max_power: 6_400_000.0,
            v_max: 220.0,
            brake_force: 0.0,
            brake_power: 0.0,
            ramp_time: 2.5,
            v_pullout: 0.0,
            regenerative: false,
            brake_fade_kmh: 0.0,
            motor: None,
        })
    }

    /// Runs a vehicle up under the wire of `line` and reports what it ended up with.
    fn live(supply: &PowerSupply, line: track_model::Electrification) -> TractionState {
        let mut state = TractionState {
            battery: true,
            pantograph_command: true,
            main_switch_command: true,
            line_system: line,
            line_voltage: line.map_or(0.0, |s| s.voltage()),
            ..TractionState::default()
        };
        for _ in 0..(200 * 20) {
            step(&mut state, &[drive()], supply, 0.0, 1.0 / 200.0);
        }
        state
    }

    fn supply_for(systems: &[SupplySystem]) -> PowerSupply {
        PowerSupply {
            systems: systems.to_vec(),
            ..PowerSupply::default()
        }
    }

    #[test]
    fn the_main_switch_only_closes_under_a_system_the_vehicle_is_built_for() {
        let de = supply_for(&[SupplySystem::Ac15kv]);
        assert!(live(&de, Some(SupplySystem::Ac15kv)).main_switch);
        // Plenty of volts, wrong system — and 25 kV would fry a 15 kV transformer.
        assert!(!live(&de, Some(SupplySystem::Ac25kv)).main_switch);
        assert!(!live(&de, Some(SupplySystem::Dc1500v)).main_switch);
        // No wire at all is no wire at all.
        assert!(!live(&de, None).main_switch);

        let nl = supply_for(&[SupplySystem::Dc1500v]);
        assert!(live(&nl, Some(SupplySystem::Dc1500v)).main_switch);
        assert!(!live(&nl, Some(SupplySystem::Ac15kv)).main_switch);
    }

    #[test]
    fn a_multi_system_vehicle_works_under_every_system_it_carries_a_head_for() {
        let supply = supply_for(&[
            SupplySystem::Ac15kv,
            SupplySystem::Ac25kv,
            SupplySystem::Dc3kv,
        ]);
        for system in [
            SupplySystem::Ac15kv,
            SupplySystem::Ac25kv,
            SupplySystem::Dc3kv,
        ] {
            assert!(live(&supply, Some(system)).main_switch, "{system:?}");
        }
        assert!(!live(&supply, Some(SupplySystem::Dc1500v)).main_switch);
        // The first one is what the vehicle is at home under.
        assert_eq!(supply.system(), SupplySystem::Ac15kv);
    }

    #[test]
    fn a_third_rail_shoe_needs_no_rise_time() {
        let supply = supply_for(&[SupplySystem::ThirdRail]);
        let mut state = TractionState {
            battery: true,
            pantograph_command: true,
            main_switch_command: true,
            line_system: Some(SupplySystem::ThirdRail),
            line_voltage: SupplySystem::ThirdRail.voltage(),
            ..TractionState::default()
        };
        step(&mut state, &[drive()], &supply, 0.0, 1.0 / 200.0);
        assert_eq!(state.pantograph, 1.0);
        assert!(state.main_switch);
    }

    #[test]
    fn a_voltage_source_stands_in_for_the_contact_line() {
        let supply = PowerSupply {
            source_voltage: 15_000.0,
            ..PowerSupply::default()
        };
        // No pantograph up, no wire over the track — and the switch still closes.
        let mut state = TractionState {
            battery: true,
            main_switch_command: true,
            ..TractionState::default()
        };
        step(&mut state, &[drive()], &supply, 0.0, 1.0 / 200.0);
        assert!(state.main_switch);
    }

    #[test]
    fn the_battery_runs_down_when_it_is_left_on_and_charges_when_something_runs() {
        let supply = PowerSupply::default();
        let mut state = TractionState {
            battery: true,
            ..TractionState::default()
        };
        // Standing overnight: 12 A out of 250 Ah is about 20 hours.
        for _ in 0..(200 * 3600 * 8) {
            step(&mut state, &[drive()], &supply, 0.0, 1.0 / 200.0);
        }
        assert!(
            (0.4..0.8).contains(&state.battery_charge),
            "{:.2} left",
            state.battery_charge
        );
        // Raise the pantograph under the wire and close the switch: it charges again.
        state.pantograph_command = true;
        state.main_switch_command = true;
        state.line_system = Some(SupplySystem::Ac15kv);
        state.line_voltage = SupplySystem::Ac15kv.voltage();
        for _ in 0..(200 * 1800) {
            step(&mut state, &[drive()], &supply, 0.0, 1.0 / 200.0);
        }
        assert!((state.battery_charge - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_flat_battery_will_not_crank_and_switches_everything_off() {
        let supply = PowerSupply::default();
        let mut state = TractionState {
            battery: true,
            battery_charge: 0.05,
            ..TractionState::default()
        };
        assert!(!crank_battery(&mut state, &supply, 1.0 / 200.0));
        state.battery_charge = 0.0;
        step(&mut state, &[drive()], &supply, 0.0, 1.0 / 200.0);
        assert!(!state.battery, "a flat battery is no battery");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drive::{Circuit, CircuitKind, SeriesMotor};

    /// Every test here drives a vehicle with one chain on the shared power controller —
    /// these two shadow the real functions so the tests read as they did before the split.
    fn step(state: &mut TractionState, spec: &TractionSpec, v: f64, dt: f64) {
        super::step(
            state,
            &[DriveSpec::new(spec.clone())],
            &PowerSupply::default(),
            v,
            dt,
        );
    }

    fn start_engine(state: &mut TractionState, spec: &TractionSpec) {
        let battery = state.battery;
        super::start_engine(&mut state.drives[0], spec, battery);
    }

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
            transmission: Some(Box::new(Transmission {
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
            })),
            electric: None,
            gearbox: None,
            hydrostatic: None,
            hydrodynamic_brake: Some(HydrodynamicBrake {
                absorption: 0.35,
                ratio: 4.0,
                wheel_diameter: 1.0,
                max_force: 100_000.0,
                max_power: 1_500_000.0,
                fill_time: 1.5,
                fade_out_kmh: 12.0,
            }),
            dynamic_brake: None,
        }
    }

    /// A railbus: 150 kW petrol-sized diesel, four gears, friction clutch.
    fn diesel_mechanical() -> TractionSpec {
        TractionSpec::Diesel {
            max_force: 25_000.0,
            max_power: 150_000.0,
            v_max: 90.0,
            ramp_time: 4.0,
            start_time: 5.0,
            engine: Some(DieselEngine {
                idle_rpm: 600.0,
                rated_rpm: 1900.0,
                max_rpm: 2100.0,
                torque_curve: vec![(600.0, 500.0), (1400.0, 780.0), (1900.0, 750.0)],
                governor: Governor::Fill,
                inertia: 6.0,
                response_time: 0.6,
            }),
            transmission: None,
            electric: None,
            gearbox: Some(Box::new(MechanicalGearbox {
                gears: vec![5.5, 3.0, 1.8, 1.0],
                final_ratio: 3.0,
                wheel_diameter: 0.9,
                efficiency: 0.95,
                clutch_torque: 1_200.0,
                clutch_time: 1.0,
                shift_time: 1.5,
                shift_up_rpm: 1800.0,
                shift_down_rpm: 900.0,
            })),
            hydrostatic: None,
            hydrodynamic_brake: None,
            dynamic_brake: None,
        }
    }

    /// A small shunter on a hydrostatic drive.
    fn diesel_hydrostatic() -> TractionSpec {
        TractionSpec::Diesel {
            max_force: 60_000.0,
            max_power: 250_000.0,
            v_max: 40.0,
            ramp_time: 4.0,
            start_time: 5.0,
            engine: Some(DieselEngine {
                idle_rpm: 700.0,
                rated_rpm: 2000.0,
                max_rpm: 2200.0,
                torque_curve: vec![(700.0, 900.0), (1500.0, 1_300.0), (2000.0, 1_200.0)],
                governor: Governor::Speed {
                    steps: 0,
                    droop: 0.03,
                },
                inertia: 8.0,
                response_time: 0.8,
            }),
            transmission: None,
            electric: None,
            gearbox: None,
            hydrostatic: Some(HydrostaticDrive {
                max_force: 60_000.0,
                efficiency: 0.8,
                response_time: 1.5,
            }),
            hydrodynamic_brake: None,
            dynamic_brake: None,
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
        assert!(state.drives[0].engine_running);
        assert!(
            (560.0..660.0).contains(&state.drives[0].engine_rpm),
            "idle {:.0} 1/min",
            state.drives[0].engine_rpm
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
        assert!(state.drives[0].circuit_nu.abs() < 0.05);
        assert!(
            state.drives[0].engine_rpm > 1300.0,
            "{:.0} 1/min",
            state.drives[0].engine_rpm
        );
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
        assert_eq!(
            state.drives[0].circuit, 1,
            "must be in the running converter"
        );
        // Just below the change-up point it stays there — that is the hysteresis.
        for _ in 0..400 {
            step(&mut state, &spec, 68.0 / 3.6, 1.0 / 200.0);
        }
        assert_eq!(
            state.drives[0].circuit, 1,
            "no hunting inside the hysteresis"
        );
        // Well below it, it changes back.
        for _ in 0..400 {
            step(&mut state, &spec, 50.0 / 3.6, 1.0 / 200.0);
        }
        assert_eq!(state.drives[0].circuit, 0);
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
        assert!(state.drives[0].dynamic_force > 20_000.0);
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
        assert!(state.drives[0].circuit_fill.iter().all(|fill| *fill < 0.01));
    }

    #[test]
    fn the_gearbox_gets_away_on_its_clutch_and_changes_up() {
        let spec = diesel_mechanical();
        let mut state = running(&spec);
        state.notch = 1.0;
        // Getting away: the clutch slips, and that slip is the tractive effort.
        let mut v: f64 = 0.0;
        for _ in 0..200 {
            step(&mut state, &spec, v, 1.0 / 200.0);
        }
        let drive = state.drives[0];
        assert_eq!(drive.gear, 0, "must get away in first gear");
        assert!(drive.clutch > 0.0 && drive.clutch <= 1.0);
        assert!(state.force > 5_000.0, "effort {:.0} N", state.force);

        // Running up through the gears — the schedule goes by engine speed.
        for step_index in 0..12_000 {
            v = (step_index as f64 / 12_000.0) * 80.0 / 3.6;
            step(&mut state, &spec, v, 1.0 / 200.0);
        }
        assert!(
            state.drives[0].gear >= 2,
            "still in gear {}",
            state.drives[0].gear + 1
        );
    }

    #[test]
    fn a_mechanical_gearbox_can_be_stalled() {
        let mut spec = diesel_mechanical();
        if let TractionSpec::Diesel {
            gearbox: Some(g), ..
        } = &mut spec
        {
            // One gear, so there is nothing to change down into: braking to a stand with
            // the clutch still in drags the engine below its idle and kills it — which is
            // exactly what happens to a driver who forgets to declutch.
            g.gears = vec![1.0];
        }
        let mut state = running(&spec);
        state.notch = 0.3;
        for _ in 0..1200 {
            step(&mut state, &spec, 40.0 / 3.6, 1.0 / 200.0);
        }
        assert!(
            state.drives[0].clutch > 0.9,
            "clutch should be in when rolling"
        );
        // Braked to a stand inside a second.
        for i in 0..200 {
            let v = (40.0 / 3.6) * (1.0 - i as f64 / 200.0);
            step(&mut state, &spec, v, 1.0 / 200.0);
        }
        for _ in 0..200 {
            step(&mut state, &spec, 0.0, 1.0 / 200.0);
        }
        assert!(!state.drives[0].engine_running, "engine survived");
    }

    #[test]
    fn the_hydrostatic_drive_is_flat_then_hyperbolic() {
        let spec = diesel_hydrostatic();
        let force_at = |kmh: f64| {
            let mut state = running(&spec);
            state.notch = 1.0;
            for _ in 0..2000 {
                step(&mut state, &spec, kmh / 3.6, 1.0 / 200.0);
            }
            state.force
        };
        // Pressure-limited at the bottom, power-limited at the top.
        let low = force_at(2.0);
        let high = force_at(30.0);
        assert!(
            low > 55_000.0,
            "relief valve should cap at 60 kN: {low:.0} N"
        );
        assert!(high < low * 0.5, "{low:.0} N → {high:.0} N over speed");
        assert!(high > 5_000.0, "still pulling: {high:.0} N");
    }

    #[test]
    fn the_shunting_gear_pulls_harder_and_only_changes_at_a_stand() {
        let mut spec = diesel_hydraulic();
        if let TractionSpec::Diesel {
            transmission: Some(t),
            ..
        } = &mut spec
        {
            // Twice the final drive: the shunting gear of a V 90 in round figures.
            t.shunting_ratio = t.final_ratio * 2.0;
        }
        let force_in = |road_gear: bool| {
            let mut state = running(&spec);
            state.notch = 1.0;
            state.drives[0].road_gear = road_gear;
            for _ in 0..1200 {
                step(&mut state, &spec, 10.0 / 3.6, 1.0 / 200.0);
            }
            state.force
        };
        assert!(
            force_in(false) > force_in(true) * 1.5,
            "{:.0} kN shunting vs {:.0} kN road",
            force_in(false) / 1000.0,
            force_in(true) / 1000.0
        );

        // Under way the dog clutch stays where it is, however the driver turns the switch.
        let mut state = running(&spec);
        state.notch = 1.0;
        state.road_gear = false;
        for _ in 0..200 {
            step(&mut state, &spec, 40.0 / 3.6, 1.0 / 200.0);
        }
        assert!(state.drives[0].road_gear, "changed under way");
        for _ in 0..200 {
            step(&mut state, &spec, 0.0, 1.0 / 200.0);
        }
        assert!(!state.drives[0].road_gear, "did not change at a stand");
    }

    #[test]
    fn a_speed_controlled_transmission_stays_full_at_a_low_notch() {
        let filling = diesel_hydraulic();
        let mut mekydro = diesel_hydraulic();
        if let TractionSpec::Diesel {
            transmission: Some(t),
            ..
        } = &mut mekydro
        {
            t.speed_controlled = true;
        }
        let fill_at = |spec: &TractionSpec| {
            let mut state = running(spec);
            state.notch = 0.3;
            for _ in 0..1200 {
                step(&mut state, spec, 20.0 / 3.6, 1.0 / 200.0);
            }
            (
                state.drives[0].circuit_fill[state.drives[0].circuit],
                state.force,
            )
        };
        let (voith_fill, voith_force) = fill_at(&filling);
        let (mekydro_fill, mekydro_force) = fill_at(&mekydro);
        // The Mekydro's converter knows full or empty; the notch moves the engine instead.
        assert!(mekydro_fill > 0.99, "filling {mekydro_fill:.2}");
        assert!(voith_fill < 0.5, "filling {voith_fill:.2}");
        assert!(
            mekydro_force > voith_force,
            "{:.0} kN speed-controlled vs {:.0} kN by filling",
            mekydro_force / 1000.0,
            voith_force / 1000.0
        );
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
            (b.drives[0].engine_rpm - 1050.0).abs() < 5.0,
            "{:.0} 1/min",
            b.drives[0].engine_rpm
        );
        assert!(
            a.drives[0].engine_rpm < b.drives[0].engine_rpm - 10.0,
            "droop {:.0} vs isochronous {:.0} 1/min",
            a.drives[0].engine_rpm,
            b.drives[0].engine_rpm
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
        let loaded = state.drives[0].engine_rpm;
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
                thermal: None,
            }),
            starter: None,
            dynamic_brake: None,
        };
        let mut state = TractionState {
            battery: true,
            pantograph: 1.0,
            pantograph_command: true,
            main_switch_command: true,
            line_voltage: NOMINAL_LINE_VOLTAGE,
            line_system: Some(SupplySystem::Ac15kv),
            ..Default::default()
        };
        state.notch = 1.0;
        // One step time gets nowhere near the top notch.
        for _ in 0..160 {
            step(&mut state, &spec, 0.0, 1.0 / 200.0);
        }
        assert!(
            state.drives[0].step < 28.0,
            "tap changer at {:.1}",
            state.drives[0].step
        );
        for _ in 0..8000 {
            step(&mut state, &spec, 0.0, 1.0 / 200.0);
        }
        assert!((state.drives[0].step - 28.0).abs() < 0.1);
        assert!(state.force > 200_000.0, "{:.0} N", state.force);
        assert!(state.drives[0].motor_current <= 1600.0 + 1.0);
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
            motor: None,
        };
        let mut state = TractionState {
            battery: true,
            pantograph: 1.0,
            pantograph_command: true,
            main_switch_command: true,
            line_voltage: NOMINAL_LINE_VOLTAGE,
            line_system: Some(SupplySystem::Ac15kv),
            notch: -1.0,
            ..Default::default()
        };
        for _ in 0..1000 {
            step(&mut state, &spec, 120.0 / 3.6, 1.0 / 200.0);
        }
        assert!(state.drives[0].dynamic_force > 50_000.0);
        // Neutral section: the main switch drops out and the brake goes with it.
        // The wire runs out — a neutral section, or the end of the electrification.
        state.line_voltage = 0.0;
        state.line_system = None;
        for _ in 0..1000 {
            step(&mut state, &spec, 120.0 / 3.6, 1.0 / 200.0);
        }
        assert!(
            state.drives[0].dynamic_force < 1.0,
            "{:.0} N",
            state.drives[0].dynamic_force
        );
    }
}
