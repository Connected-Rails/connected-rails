//! Air brake (plan ch. 7).
//!
//! Real pneumatics are modelled, not a brake-force slider:
//! the brake pipe as a chain of nodes along the train, one KE control valve per vehicle
//! (three-pressure system) with control chamber, auxiliary reservoir and brake cylinder.

use crate::G;
use crate::train::Train;
use serde::{Deserialize, Serialize};

/// Nominal operating pressure of the brake pipe [bar].
pub const PIPE_NOMINAL: f64 = 5.0;
/// Charging pressure during the release surge [bar].
pub const PIPE_OVERCHARGE: f64 = 5.4;
/// Pressure drop at which the control valve responds [bar].
pub const RESPONSE_DROP: f64 = 0.3;
/// Pressure drop for a full service application [bar].
pub const FULL_SERVICE_DROP: f64 = 1.5;

/// Brake position (changeover handle on the vehicle).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BrakePosition {
    /// Freight train, long transition times.
    G,
    /// Passenger train.
    #[default]
    P,
    /// Rapid brake position, higher brake force in the upper speed range.
    R,
    /// R with magnetic track brake.
    RMg,
}

impl BrakePosition {
    /// Filling time of the brake cylinder (0 → 95 %) [s].
    pub fn apply_time(self) -> f64 {
        match self {
            BrakePosition::G => 22.0,
            _ => 4.0,
        }
    }

    /// Release time of the brake cylinder [s].
    pub fn release_time(self) -> f64 {
        match self {
            BrakePosition::G => 50.0,
            _ => 17.0,
        }
    }

    pub fn has_mg(self) -> bool {
        matches!(self, BrakePosition::RMg)
    }

    /// Force bonus in the R range above 60 km/h.
    pub fn high_speed_factor(self, v_kmh: f64) -> f64 {
        match self {
            BrakePosition::R | BrakePosition::RMg if v_kmh > 60.0 => 1.35,
            _ => 1.0,
        }
    }
}

/// Brake type — determines how the friction coefficient varies with speed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BrakeKind {
    /// Block brake (cast iron): friction coefficient drops sharply with speed.
    #[default]
    Block,
    /// Disc brake: nearly constant friction coefficient.
    Disc,
}

impl BrakeKind {
    /// Friction factor relative to standstill.
    /// ponytail: two smooth curves instead of Karwatzki tables — good enough for braking
    /// distances within a few percent; add real pad characteristic maps per type when
    /// fine-tuning against the braking table is due.
    pub fn friction_factor(self, v_kmh: f64) -> f64 {
        let v = v_kmh.abs();
        match self {
            BrakeKind::Block => 1.0 / (1.0 + 0.011 * v),
            BrakeKind::Disc => 1.0 / (1.0 + 0.003 * v),
        }
    }
}

/// Brake equipment of a vehicle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrakeSpec {
    pub kind: BrakeKind,
    pub position: BrakePosition,
    /// Braked weight [t] — basis of the braked weight percentage.
    pub brake_weight: f64,
    /// Brake force at full brake cylinder pressure and standstill [N].
    pub max_force: f64,
    /// Highest brake cylinder pressure [bar].
    pub max_cylinder: f64,
    /// Volume ratio brake cylinder / auxiliary reservoir (exhaustibility).
    pub cylinder_to_reservoir: f64,
    /// Magnetic track brake fitted.
    #[serde(default)]
    pub has_mg: bool,
    /// Force of the magnetic track brake [N].
    #[serde(default)]
    pub mg_force: f64,
    /// Direct brake fitted — powered vehicles only.
    #[serde(default)]
    pub has_direct: bool,
    /// Spring-applied parking / hand brake force [N].
    #[serde(default)]
    pub parking_force: f64,
}

impl BrakeSpec {
    /// Derive the brake equipment from the braked weight.
    ///
    /// The factor is calibrated against the braking table: a train with a braked weight
    /// percentage of 100 comes to a stand from 100 km/h with emergency braking in the
    /// order of 500 m (see test `schnellbremsung_aus_100_kmh`).
    pub fn from_brake_weight(brake_weight_t: f64, kind: BrakeKind) -> Self {
        Self {
            kind,
            position: BrakePosition::P,
            brake_weight: brake_weight_t,
            max_force: brake_weight_t * 1000.0 * G * 0.145,
            max_cylinder: 3.8,
            cylinder_to_reservoir: 0.35,
            has_mg: false,
            mg_force: 0.0,
            has_direct: false,
            parking_force: 0.0,
        }
    }

    pub fn with_position(mut self, position: BrakePosition) -> Self {
        self.position = position;
        self
    }

    pub fn with_direct_brake(mut self) -> Self {
        self.has_direct = true;
        self
    }

    pub fn with_mg(mut self, force: f64) -> Self {
        self.has_mg = true;
        self.mg_force = force;
        self.position = BrakePosition::RMg;
        self
    }
}

/// Runtime state of a vehicle's brake. All pressures in bar (gauge pressure).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BrakeState {
    /// Brake pipe at this vehicle.
    pub pipe: f64,
    /// Control chamber (reference pressure of the control valve).
    pub control_reservoir: f64,
    /// Auxiliary reservoir (R reservoir).
    pub aux_reservoir: f64,
    /// Brake cylinder pressure from the automatic brake.
    pub cylinder: f64,
    /// Brake cylinder pressure from the direct brake.
    pub direct_cylinder: f64,
    /// Main reservoir (powered vehicles only).
    pub main_reservoir: f64,
    /// Magnetic track brake applied.
    pub mg_applied: bool,
    /// Spring-applied parking / hand brake applied.
    pub parking_applied: bool,
    /// Current brake force [N] (output to the longitudinal dynamics).
    pub force: f64,
}

impl BrakeState {
    pub fn new(spec: &BrakeSpec) -> Self {
        let _ = spec;
        Self {
            pipe: PIPE_NOMINAL,
            control_reservoir: PIPE_NOMINAL,
            aux_reservoir: PIPE_NOMINAL,
            cylinder: 0.0,
            direct_cylinder: 0.0,
            main_reservoir: 9.0,
            mg_applied: false,
            parking_applied: false,
            force: 0.0,
        }
    }

    /// Released?
    pub fn released(&self) -> bool {
        self.cylinder < 0.15 && self.direct_cylinder < 0.15
    }
}

/// Position of the driver's brake valve.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum DriverBrakeValve {
    /// Filling position (release surge, brake pipe above nominal pressure).
    Fill,
    /// Running position — brake pipe at nominal pressure, kept level.
    Release,
    /// Lap position — no connection, pressure stays where it is.
    Lap,
    /// Service application with a pressure drop [bar] below nominal pressure.
    Service(f64),
    /// Emergency braking.
    Emergency,
}

impl DriverBrakeValve {
    /// Target pressure of the brake pipe at the driver's brake valve [bar].
    pub fn target_pressure(self) -> Option<f64> {
        match self {
            DriverBrakeValve::Fill => Some(PIPE_OVERCHARGE),
            DriverBrakeValve::Release => Some(PIPE_NOMINAL),
            DriverBrakeValve::Lap => None,
            DriverBrakeValve::Service(drop) => {
                Some((PIPE_NOMINAL - drop.clamp(0.0, FULL_SERVICE_DROP)).max(3.4))
            }
            DriverBrakeValve::Emergency => Some(0.0),
        }
    }

    /// Flow towards the target value [bar/s]: charging slower than venting,
    /// emergency braking very fast.
    pub fn flow_rate(self) -> f64 {
        match self {
            DriverBrakeValve::Fill => 1.2,
            DriverBrakeValve::Release => 0.5,
            DriverBrakeValve::Lap => 0.0,
            DriverBrakeValve::Service(_) => 0.6,
            DriverBrakeValve::Emergency => 6.0,
        }
    }
}

/// Conductance between two neighbouring vehicles [1/s].
///
/// ponytail: node model instead of a pipe PDE (plan 7). The pressure drop therefore
/// propagates diffusively instead of as a wave towards the rear — order and delay are
/// qualitatively right (a long freight train brakes later at the rear), the exact
/// propagation speed is not. Upgrade path: method of characteristics per pipe section.
pub const PIPE_CONDUCTANCE: f64 = 6.0;

/// One simulation step of the whole brake system of a train.
pub fn step(train: &mut Train, valve: DriverBrakeValve, direct: f64, dt: f64) {
    update_pipe(train, valve, dt);
    let cab = train.cab;
    let v_kmh = train.speed_kmh().abs();
    for (i, veh) in train.vehicles.iter_mut().enumerate() {
        update_control_valve(&mut veh.brake, &veh.spec.brake, dt);
        if veh.spec.brake.has_direct && i == cab {
            let target = direct.clamp(0.0, 1.0) * veh.spec.brake.max_cylinder;
            approach(&mut veh.brake.direct_cylinder, target, 2.0, dt);
        }
        veh.brake.mg_applied = veh.spec.brake.has_mg
            && veh.spec.brake.position.has_mg()
            && v_kmh > 50.0
            && veh.brake.pipe < PIPE_NOMINAL - 1.0;
        veh.brake.force = brake_force(&veh.spec.brake, &veh.brake, v_kmh);
    }
}

/// Pressure equalisation in the brake pipe including the driver's brake valve.
fn update_pipe(train: &mut Train, valve: DriverBrakeValve, dt: f64) {
    let n = train.vehicles.len();
    let pressures: Vec<f64> = train.vehicles.iter().map(|v| v.brake.pipe).collect();
    for i in 0..n {
        let mut flow = 0.0;
        if i > 0 {
            flow += PIPE_CONDUCTANCE * (pressures[i - 1] - pressures[i]);
        }
        if i + 1 < n {
            flow += PIPE_CONDUCTANCE * (pressures[i + 1] - pressures[i]);
        }
        // Consumption by the control valve while recharging the auxiliary reservoir.
        let veh = &train.vehicles[i];
        if veh.brake.aux_reservoir < veh.brake.pipe {
            flow -= 0.15 * (veh.brake.pipe - veh.brake.aux_reservoir);
        }
        let p = &mut train.vehicles[i].brake.pipe;
        *p = (*p + flow * dt).clamp(0.0, PIPE_OVERCHARGE);
    }
    // The driver's brake valve acts at the occupied cab.
    if let Some(target) = valve.target_pressure() {
        let cab = train.cab.min(n.saturating_sub(1));
        let p = &mut train.vehicles[cab].brake.pipe;
        approach(p, target, valve.flow_rate(), dt);
    }
}

/// KE control valve: three-pressure system with control chamber, auxiliary reservoir
/// and brake cylinder.
fn update_control_valve(state: &mut BrakeState, spec: &BrakeSpec, dt: f64) {
    // The control chamber follows the brake pipe only while releasing/charging (and never
    // beyond nominal pressure, otherwise the release surge would "overcharge" the brake).
    if state.pipe >= state.control_reservoir {
        approach(
            &mut state.control_reservoir,
            state.pipe.min(PIPE_NOMINAL),
            0.35,
            dt,
        );
    }
    // The auxiliary reservoir is recharged from the brake pipe.
    if state.pipe > state.aux_reservoir {
        approach(&mut state.aux_reservoir, state.pipe, 0.15, dt);
    }

    let drop = state.control_reservoir - state.pipe;
    let target = if drop <= RESPONSE_DROP {
        0.0
    } else {
        // Full cylinder pressure at the full service pressure drop.
        let ratio = spec.max_cylinder / (FULL_SERVICE_DROP - RESPONSE_DROP);
        ((drop - RESPONSE_DROP) * ratio).min(spec.max_cylinder)
    };
    // Exhaustibility: the cylinder can never be charged beyond the auxiliary reservoir.
    let target = target.min(state.aux_reservoir);

    let rate = if target > state.cylinder {
        // 0 → 95 % in apply_time.
        spec.max_cylinder / spec.position.apply_time() * 3.0
    } else {
        spec.max_cylinder / spec.position.release_time() * 3.0
    };
    let before = state.cylinder;
    approach(&mut state.cylinder, target, rate, dt);
    // Air consumption from the auxiliary reservoir.
    let delta = state.cylinder - before;
    if delta > 0.0 {
        state.aux_reservoir = (state.aux_reservoir - delta * spec.cylinder_to_reservoir).max(0.0);
    }
}

/// Brake force of a vehicle [N].
fn brake_force(spec: &BrakeSpec, state: &BrakeState, v_kmh: f64) -> f64 {
    let cylinder = state.cylinder.max(state.direct_cylinder);
    let mut f = cylinder / spec.max_cylinder
        * spec.max_force
        * spec.kind.friction_factor(v_kmh)
        * spec.position.high_speed_factor(v_kmh);
    if state.mg_applied {
        f += spec.mg_force;
    }
    if state.parking_applied {
        f += spec.parking_force;
    }
    f.max(0.0)
}

/// Moves `value` towards `target` at a maximum rate of `rate` [unit/s].
pub(crate) fn approach(value: &mut f64, target: f64, rate: f64, dt: f64) {
    let max_step = rate * dt;
    let diff = target - *value;
    *value += diff.clamp(-max_step, max_step);
}
