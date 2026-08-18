//! Signal graph of a vehicle: the wiring between the physical blocks.
//!
//! The block diagram is not only pipes and shafts — a good half of a real vehicle is the
//! control logic between them: a PID that holds a set speed, a characteristic that turns a
//! speed into a blower demand, a notched controller that steps its output, a rate of change
//! that a load-shedding relay watches. Without these the only way to express any of it is a
//! script.
//!
//! [`SignalProgram`] is what [`crate::blocks::bake`] compiles that part of the diagram into:
//! a flat list of operations in evaluation order, plus which of them feed which sink. It is
//! evaluated once per simulation step, before the drive — so what it writes takes effect in
//! the same step the driver's lever moved.
//!
//! It is deliberately not a general expression language. Every operation is a block that
//! exists in the palette, the list is acyclic by construction (an operation may only read
//! ones before it), and it holds no memory beyond the few state values named below. What
//! needs more than that is what the `script` block is for.

use serde::{Deserialize, Serialize};

use crate::drive::interpolate;

/// A reading the signal graph can take from the vehicle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SignalInput {
    /// Power controller of the cab, −1 … +1.
    #[default]
    Throttle,
    /// Brake demand of the driver's brake valve, 0…1.
    BrakeDemand,
    /// Direct brake handle, 0…1.
    DirectBrake,
    /// Speed [m/s], signed.
    Speed,
    /// Speed [km/h], magnitude — what a cab instrument shows.
    SpeedKmh,
    /// Set speed of the cruise control [km/h].
    TargetSpeedKmh,
    /// Brake cylinder pressure [bar].
    CylinderPressure,
    /// Brake pipe pressure [bar].
    PipePressure,
    /// Main reservoir pressure [bar].
    MainReservoir,
    /// Armature or motor current of the first chain [A].
    MotorCurrent,
    /// Engine speed of the first chain [1/min].
    EngineRpm,
    /// Hottest component of the first chain [°C].
    Temperature,
    /// Tractive effort of the vehicle [N], signed.
    TractiveEffort,
    /// Reverser: −1, 0 or +1.
    Reverser,
    /// 1 while the vehicle is being sanded.
    Sanding,
}

impl SignalInput {
    /// i18n key of the reading's name.
    pub fn key(self) -> &'static str {
        match self {
            SignalInput::Throttle => "sig-in-throttle",
            SignalInput::BrakeDemand => "sig-in-brake",
            SignalInput::DirectBrake => "sig-in-direct",
            SignalInput::Speed => "sig-in-speed",
            SignalInput::SpeedKmh => "sig-in-speed-kmh",
            SignalInput::TargetSpeedKmh => "sig-in-target-speed",
            SignalInput::CylinderPressure => "sig-in-cylinder",
            SignalInput::PipePressure => "sig-in-pipe",
            SignalInput::MainReservoir => "sig-in-main-res",
            SignalInput::MotorCurrent => "sig-in-current",
            SignalInput::EngineRpm => "sig-in-rpm",
            SignalInput::Temperature => "sig-in-temp",
            SignalInput::TractiveEffort => "sig-in-effort",
            SignalInput::Reverser => "sig-in-reverser",
            SignalInput::Sanding => "sig-in-sanding",
        }
    }

    /// Stable id used in the vehicle file and in the block parameter.
    pub fn id(self) -> &'static str {
        match self {
            SignalInput::Throttle => "throttle",
            SignalInput::BrakeDemand => "brake",
            SignalInput::DirectBrake => "direct",
            SignalInput::Speed => "speed",
            SignalInput::SpeedKmh => "speed-kmh",
            SignalInput::TargetSpeedKmh => "target-speed",
            SignalInput::CylinderPressure => "cylinder",
            SignalInput::PipePressure => "pipe",
            SignalInput::MainReservoir => "main-res",
            SignalInput::MotorCurrent => "current",
            SignalInput::EngineRpm => "rpm",
            SignalInput::Temperature => "temp",
            SignalInput::TractiveEffort => "effort",
            SignalInput::Reverser => "reverser",
            SignalInput::Sanding => "sanding",
        }
    }

    pub const ALL: [SignalInput; 15] = [
        SignalInput::Throttle,
        SignalInput::BrakeDemand,
        SignalInput::DirectBrake,
        SignalInput::Speed,
        SignalInput::SpeedKmh,
        SignalInput::TargetSpeedKmh,
        SignalInput::CylinderPressure,
        SignalInput::PipePressure,
        SignalInput::MainReservoir,
        SignalInput::MotorCurrent,
        SignalInput::EngineRpm,
        SignalInput::Temperature,
        SignalInput::TractiveEffort,
        SignalInput::Reverser,
        SignalInput::Sanding,
    ];

    pub fn from_id(id: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|i| i.id() == id)
            .unwrap_or(SignalInput::Throttle)
    }
}

/// Where a computed value goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SignalSink {
    /// Replaces the power controller for the drives, −1 … +1. This is how a cruise control
    /// built out of blocks takes over the handle.
    #[default]
    Throttle,
    /// Adds to the brake demand, 0…1.
    BrakeDemand,
    /// Sands while the value is above 0.5.
    Sanding,
    /// Blower demand of the cooling system, 0…1.
    Blower,
    /// Free value the cab displays can read (`aux0`…`aux3`).
    Aux(u8),
}

impl SignalSink {
    pub fn key(self) -> &'static str {
        match self {
            SignalSink::Throttle => "sig-out-throttle",
            SignalSink::BrakeDemand => "sig-out-brake",
            SignalSink::Sanding => "sig-out-sanding",
            SignalSink::Blower => "sig-out-blower",
            SignalSink::Aux(_) => "sig-out-aux",
        }
    }

    pub fn id(self) -> String {
        match self {
            SignalSink::Throttle => "throttle".to_string(),
            SignalSink::BrakeDemand => "brake".to_string(),
            SignalSink::Sanding => "sanding".to_string(),
            SignalSink::Blower => "blower".to_string(),
            SignalSink::Aux(n) => format!("aux{n}"),
        }
    }

    pub fn from_id(id: &str) -> Self {
        match id {
            "brake" => SignalSink::BrakeDemand,
            "sanding" => SignalSink::Sanding,
            "blower" => SignalSink::Blower,
            "aux0" => SignalSink::Aux(0),
            "aux1" => SignalSink::Aux(1),
            "aux2" => SignalSink::Aux(2),
            "aux3" => SignalSink::Aux(3),
            _ => SignalSink::Throttle,
        }
    }
}

/// How two values are combined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Combine {
    #[default]
    Add,
    Subtract,
    Multiply,
    Min,
    Max,
}

impl Combine {
    pub fn apply(self, a: f64, b: f64) -> f64 {
        match self {
            Combine::Add => a + b,
            Combine::Subtract => a - b,
            Combine::Multiply => a * b,
            Combine::Min => a.min(b),
            Combine::Max => a.max(b),
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Combine::Add => "add",
            Combine::Subtract => "sub",
            Combine::Multiply => "mul",
            Combine::Min => "min",
            Combine::Max => "max",
        }
    }

    pub fn from_id(id: &str) -> Self {
        match id {
            "sub" => Combine::Subtract,
            "mul" => Combine::Multiply,
            "min" => Combine::Min,
            "max" => Combine::Max,
            _ => Combine::Add,
        }
    }
}

/// One operation of the signal graph. Every operand is the index of an operation *before*
/// this one, which is what makes the list acyclic without a check at run time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SignalOp {
    /// A reading from the vehicle.
    Read(SignalInput),
    /// A fixed number.
    Const(f64),
    /// Piecewise linear characteristic — the value converter of the diagram.
    Curve {
        input: usize,
        points: Vec<(f64, f64)>,
    },
    /// Two values combined.
    Combine { a: usize, b: usize, how: Combine },
    /// Clamped to a range.
    Clamp { input: usize, min: f64, max: f64 },
    /// PID controller on the error `setpoint − input`.
    Pid {
        input: usize,
        setpoint: usize,
        kp: f64,
        ki: f64,
        kd: f64,
        min: f64,
        max: f64,
    },
    /// Notched controller: the output steps towards the input at a limited rate, and lands
    /// only on one of `steps` positions. `steps == 0` is continuous.
    Transition {
        input: usize,
        steps: u32,
        /// Full range per second.
        rate: f64,
    },
    /// Rate of change of the input per second, low-pass filtered with `smoothing` [s].
    Rate { input: usize, smoothing: f64 },
    /// `a` while the control value is below the threshold, `b` above it — with a little
    /// hysteresis, so a value sitting on the threshold does not chatter.
    Switch {
        control: usize,
        a: usize,
        b: usize,
        threshold: f64,
        hysteresis: f64,
    },
}

impl SignalOp {
    /// Highest operand index this operation reads; `None` for a leaf.
    fn max_operand(&self) -> Option<usize> {
        match self {
            SignalOp::Read(_) | SignalOp::Const(_) => None,
            SignalOp::Curve { input, .. }
            | SignalOp::Clamp { input, .. }
            | SignalOp::Transition { input, .. }
            | SignalOp::Rate { input, .. } => Some(*input),
            SignalOp::Combine { a, b, .. } => Some((*a).max(*b)),
            SignalOp::Pid {
                input, setpoint, ..
            } => Some((*input).max(*setpoint)),
            SignalOp::Switch { control, a, b, .. } => Some((*control).max(*a).max(*b)),
        }
    }
}

/// The compiled signal graph of a vehicle.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SignalProgram {
    /// Operations in evaluation order.
    pub ops: Vec<SignalOp>,
    /// Which operation feeds which sink.
    pub outputs: Vec<(SignalSink, usize)>,
}

impl SignalProgram {
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty() || self.outputs.is_empty()
    }

    /// Is every operand an operation that comes earlier, and every output in range?
    ///
    /// `bake` produces the list in topological order, so this only ever fails on a file
    /// that was edited by hand — but a cycle would be an endless loop at 200 Hz, so it is
    /// worth the two lines.
    pub fn is_well_formed(&self) -> bool {
        self.ops
            .iter()
            .enumerate()
            .all(|(i, op)| op.max_operand().is_none_or(|m| m < i))
            && self.outputs.iter().all(|(_, i)| *i < self.ops.len())
    }
}

/// Readings the signal graph is evaluated against — filled in by the caller once per step.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SignalReadings {
    pub throttle: f64,
    pub brake_demand: f64,
    pub direct_brake: f64,
    pub speed: f64,
    pub target_speed_kmh: f64,
    pub cylinder: f64,
    pub pipe: f64,
    pub main_reservoir: f64,
    pub motor_current: f64,
    pub engine_rpm: f64,
    pub temperature: f64,
    pub tractive_effort: f64,
    pub reverser: f64,
    pub sanding: bool,
}

impl SignalReadings {
    fn get(&self, input: SignalInput) -> f64 {
        match input {
            SignalInput::Throttle => self.throttle,
            SignalInput::BrakeDemand => self.brake_demand,
            SignalInput::DirectBrake => self.direct_brake,
            SignalInput::Speed => self.speed,
            SignalInput::SpeedKmh => self.speed.abs() * 3.6,
            SignalInput::TargetSpeedKmh => self.target_speed_kmh,
            SignalInput::CylinderPressure => self.cylinder,
            SignalInput::PipePressure => self.pipe,
            SignalInput::MainReservoir => self.main_reservoir,
            SignalInput::MotorCurrent => self.motor_current,
            SignalInput::EngineRpm => self.engine_rpm,
            SignalInput::Temperature => self.temperature,
            SignalInput::TractiveEffort => self.tractive_effort,
            SignalInput::Reverser => self.reverser,
            SignalInput::Sanding => f64::from(u8::from(self.sanding)),
        }
    }
}

/// What the signal graph wrote this step.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct SignalOutputs {
    /// `Some` = the graph is commanding the power controller.
    pub throttle: Option<f64>,
    /// Added to the driver's brake demand.
    pub brake_demand: f64,
    pub sanding: bool,
    /// `Some` = the graph is commanding the blower.
    pub blower: Option<f64>,
    /// Free values for the cab displays.
    pub aux: [f64; 4],
}

/// Per-operation memory. One number is enough for every operation that has any: the PID
/// keeps its integral, the transition its position, the rate its last input, the switch
/// which side it is on.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SignalState {
    memory: Vec<f64>,
    aux: Vec<f64>,
    /// Value of every operation after the last step — what a debug view of the diagram shows.
    #[serde(skip)]
    values: Vec<f64>,
}

impl SignalState {
    pub fn new(program: &SignalProgram) -> Self {
        Self {
            memory: vec![0.0; program.ops.len()],
            aux: vec![0.0; program.ops.len()],
            values: vec![0.0; program.ops.len()],
        }
    }

    /// Value of operation `index` after the last step — for the editor's live view.
    pub fn value(&self, index: usize) -> f64 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    fn fit(&mut self, len: usize) {
        self.memory.resize(len, 0.0);
        self.aux.resize(len, 0.0);
        self.values.resize(len, 0.0);
    }
}

/// Evaluates the program once.
pub fn step(
    program: &SignalProgram,
    state: &mut SignalState,
    readings: &SignalReadings,
    dt: f64,
) -> SignalOutputs {
    let mut out = SignalOutputs::default();
    if program.is_empty() {
        return out;
    }
    state.fit(program.ops.len());
    let mut values = std::mem::take(&mut state.values);
    values.clear();
    values.resize(program.ops.len(), 0.0);

    for (i, op) in program.ops.iter().enumerate() {
        // An operand that points forwards would be a cycle; a malformed file gets 0 rather
        // than a panic, which is what every other loader in here does too.
        let read = |values: &Vec<f64>, index: usize| -> f64 {
            if index < i { values[index] } else { 0.0 }
        };
        values[i] = match op {
            SignalOp::Read(input) => readings.get(*input),
            SignalOp::Const(value) => *value,
            SignalOp::Curve { input, points } => interpolate(points, read(&values, *input)),
            SignalOp::Combine { a, b, how } => how.apply(read(&values, *a), read(&values, *b)),
            SignalOp::Clamp { input, min, max } => read(&values, *input).clamp(*min, max.max(*min)),
            SignalOp::Pid {
                input,
                setpoint,
                kp,
                ki,
                kd,
                min,
                max,
            } => {
                let error = read(&values, *setpoint) - read(&values, *input);
                let integral = state.memory[i] + error * dt;
                let derivative = if dt > 0.0 {
                    (error - state.aux[i]) / dt
                } else {
                    0.0
                };
                let raw = kp * error + ki * integral + kd * derivative;
                let clamped = raw.clamp(*min, max.max(*min));
                // Anti-windup: the integral only keeps growing while the output is free.
                if (raw - clamped).abs() < 1e-9 || ki.abs() < 1e-12 {
                    state.memory[i] = integral;
                }
                state.aux[i] = error;
                clamped
            }
            SignalOp::Transition { input, steps, rate } => {
                let target = read(&values, *input);
                let rate = rate.max(1e-6);
                let mut pos = state.memory[i];
                let delta = (target - pos).clamp(-rate * dt, rate * dt);
                pos += delta;
                state.memory[i] = pos;
                if *steps == 0 {
                    pos
                } else {
                    let steps = *steps as f64;
                    (pos * steps).round() / steps
                }
            }
            SignalOp::Rate { input, smoothing } => {
                let value = read(&values, *input);
                let raw = if dt > 0.0 {
                    (value - state.aux[i]) / dt
                } else {
                    0.0
                };
                state.aux[i] = value;
                let tau = smoothing.max(0.0);
                let alpha = if tau > 0.0 { (dt / tau).min(1.0) } else { 1.0 };
                state.memory[i] += (raw - state.memory[i]) * alpha;
                state.memory[i]
            }
            SignalOp::Switch {
                control,
                a,
                b,
                threshold,
                hysteresis,
            } => {
                let control = read(&values, *control);
                let on = state.memory[i] > 0.5;
                let half = hysteresis.abs() / 2.0;
                let on = if on {
                    control > threshold - half
                } else {
                    control > threshold + half
                };
                state.memory[i] = f64::from(u8::from(on));
                if on {
                    read(&values, *b)
                } else {
                    read(&values, *a)
                }
            }
        };
    }

    for (sink, index) in &program.outputs {
        let value = values.get(*index).copied().unwrap_or(0.0);
        match sink {
            SignalSink::Throttle => out.throttle = Some(value.clamp(-1.0, 1.0)),
            SignalSink::BrakeDemand => out.brake_demand += value.clamp(0.0, 1.0),
            SignalSink::Sanding => out.sanding |= value > 0.5,
            SignalSink::Blower => out.blower = Some(value.clamp(0.0, 1.0)),
            SignalSink::Aux(n) => {
                if let Some(slot) = out.aux.get_mut(*n as usize) {
                    *slot = value;
                }
            }
        }
    }

    state.values = values;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn readings(speed_kmh: f64, target_kmh: f64) -> SignalReadings {
        SignalReadings {
            speed: speed_kmh / 3.6,
            target_speed_kmh: target_kmh,
            ..Default::default()
        }
    }

    /// A cruise control out of three blocks: read the speed, read the set speed, PID
    /// between them onto the power controller.
    fn cruise() -> SignalProgram {
        SignalProgram {
            ops: vec![
                SignalOp::Read(SignalInput::SpeedKmh),
                SignalOp::Read(SignalInput::TargetSpeedKmh),
                SignalOp::Pid {
                    input: 0,
                    setpoint: 1,
                    kp: 0.1,
                    ki: 0.02,
                    kd: 0.0,
                    min: -1.0,
                    max: 1.0,
                },
            ],
            outputs: vec![(SignalSink::Throttle, 2)],
        }
    }

    #[test]
    fn a_pid_out_of_blocks_opens_up_below_its_set_speed_and_closes_above_it() {
        let program = cruise();
        assert!(program.is_well_formed());
        let mut state = SignalState::new(&program);
        let out = step(&program, &mut state, &readings(60.0, 100.0), 0.005);
        assert!(out.throttle.unwrap() > 0.5, "{:?}", out.throttle);
        let mut state = SignalState::new(&program);
        let out = step(&program, &mut state, &readings(120.0, 100.0), 0.005);
        assert!(out.throttle.unwrap() < 0.0, "{:?}", out.throttle);
    }

    #[test]
    fn the_integral_stops_winding_up_once_the_output_is_hard_over() {
        let program = cruise();
        let mut state = SignalState::new(&program);
        for _ in 0..2000 {
            step(&program, &mut state, &readings(0.0, 100.0), 0.005);
        }
        // Wound up without a limit the integral would be 100·10 s = 1000.
        assert!(state.memory[2] < 60.0, "integral {}", state.memory[2]);
        // And it comes back down inside a few seconds once the speed is there.
        for _ in 0..2000 {
            step(&program, &mut state, &readings(101.0, 100.0), 0.005);
        }
        assert!(state.memory[2] < 20.0, "integral {}", state.memory[2]);
    }

    #[test]
    fn a_transition_lands_on_its_notches_and_takes_its_time() {
        let program = SignalProgram {
            ops: vec![
                SignalOp::Read(SignalInput::Throttle),
                SignalOp::Transition {
                    input: 0,
                    steps: 4,
                    rate: 0.5,
                },
            ],
            outputs: vec![(SignalSink::Aux(0), 1)],
        };
        let mut state = SignalState::new(&program);
        let readings = SignalReadings {
            throttle: 1.0,
            ..Default::default()
        };
        let out = step(&program, &mut state, &readings, 0.5);
        // Half a second at half a range per second is a quarter of the way — notch 1 of 4.
        assert!((out.aux[0] - 0.25).abs() < 1e-9, "{}", out.aux[0]);
        for _ in 0..10 {
            step(&program, &mut state, &readings, 0.5);
        }
        assert!((state.value(1) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_switch_needs_its_hysteresis_to_come_back() {
        let program = SignalProgram {
            ops: vec![
                SignalOp::Read(SignalInput::SpeedKmh),
                SignalOp::Const(0.0),
                SignalOp::Const(1.0),
                SignalOp::Switch {
                    control: 0,
                    a: 1,
                    b: 2,
                    threshold: 50.0,
                    hysteresis: 10.0,
                },
            ],
            outputs: vec![(SignalSink::Aux(0), 3)],
        };
        let mut state = SignalState::new(&program);
        assert_eq!(
            step(&program, &mut state, &readings(52.0, 0.0), 0.1).aux[0],
            0.0
        );
        assert_eq!(
            step(&program, &mut state, &readings(58.0, 0.0), 0.1).aux[0],
            1.0
        );
        // Back through the threshold is not enough — it has to fall past the lower edge.
        assert_eq!(
            step(&program, &mut state, &readings(48.0, 0.0), 0.1).aux[0],
            1.0
        );
        assert_eq!(
            step(&program, &mut state, &readings(40.0, 0.0), 0.1).aux[0],
            0.0
        );
    }

    #[test]
    fn a_curve_reads_its_table_and_a_rate_differentiates() {
        let program = SignalProgram {
            ops: vec![
                SignalOp::Read(SignalInput::SpeedKmh),
                SignalOp::Curve {
                    input: 0,
                    points: vec![(0.0, 0.2), (100.0, 1.0)],
                },
                SignalOp::Rate {
                    input: 0,
                    smoothing: 0.0,
                },
            ],
            outputs: vec![(SignalSink::Blower, 1), (SignalSink::Aux(1), 2)],
        };
        let mut state = SignalState::new(&program);
        step(&program, &mut state, &readings(0.0, 0.0), 0.1);
        let out = step(&program, &mut state, &readings(50.0, 0.0), 0.1);
        assert!((out.blower.unwrap() - 0.6).abs() < 1e-9);
        assert!((out.aux[1] - 500.0).abs() < 1e-6, "{}", out.aux[1]);
    }

    #[test]
    fn a_forward_reference_is_rejected_instead_of_looping() {
        let program = SignalProgram {
            ops: vec![SignalOp::Clamp {
                input: 1,
                min: 0.0,
                max: 1.0,
            }],
            outputs: vec![(SignalSink::Throttle, 0)],
        };
        assert!(!program.is_well_formed());
        let mut state = SignalState::new(&program);
        // Evaluating it anyway must terminate and read the missing operand as zero.
        assert_eq!(
            step(&program, &mut state, &readings(0.0, 0.0), 0.1).throttle,
            Some(0.0)
        );
    }
}
