//! Sound table of a vehicle (plan ch. 13) — declarative, in the vehicle file.
//!
//! The model is the one Zusi uses: an entry has three parts.
//!
//! - **Trigger** — what starts the sound. [`Trigger::Loop`] is "no trigger": the sound runs
//!   continuously and only its volume and pitch move. That is the case every generated loop
//!   of [`crate::synth`] falls under.
//! - **Conditions** — state predicates that mute the sound or release it (a speed window,
//!   a brake pressure threshold).
//! - **Dependencies** — [`Curve`]s with support points that map a physical quantity onto
//!   volume and playback speed.
//!
//! The point of putting it here rather than in the app: **the mapping quantity → volume /
//! pitch is data, not code**. A continuous rolling noise and a discrete contactor click are
//! the same mechanism — the click has a trigger and no loop, the rolling noise a loop and no
//! trigger.
//!
//! `sim-core` therefore hands out no sound *events*. It exports a named set of state
//! quantities ([`SoundState`]); edge detection on them is the trigger, and that runs in the
//! app where the audio device is. A tap changer step is a number whose crossing fires the
//! contactor; brake squeal is a condition (speed window ∧ brake force) on a loop. Only what
//! does not follow from the vehicle state at all has to come from outside — rail joints,
//! which belong to the track, not to the train.
//!
//! One sound is normally several entries. A loop stretched over a whole speed range by its
//! playback rate drags its formants with it; three loops whose [`Curve::window`]s overlap
//! each stay near their own pitch and hand over to the next. [`default_table`] is built that
//! way, and a mod with recorded samples writes exactly the same shape.

use crate::cab::{CabControl, CabInputs};
use crate::safety::ProtectionOutput;
use crate::train::Vehicle;
use serde::{Deserialize, Serialize};

/// A named state quantity of the simulation that a sound can follow.
///
/// This list *is* the interface between simulation and audio. Everything a vehicle file may
/// depend on stands here, and nothing else — a mod cannot invent a quantity, because there
/// would be nothing behind it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Quantity {
    /// Driving speed [km/h], always positive.
    #[default]
    Speed,
    /// Distance travelled [m], monotonic — the reference for anything periodic.
    Distance,
    /// Diesel engine speed [1/min]; 0 without a combustion engine.
    EngineRpm,
    /// Tap changer position (0 … number of notches), running through continuously.
    TapChangerStep,
    /// Index of the engaged torque converter circuit.
    Circuit,
    /// Tractive effort at the rail [kN], absolute.
    TractiveEffort,
    /// Brake force acting [kN], absolute.
    BrakeEffort,
    /// Force of the dynamic brake alone [kN] — the electric or hydrodynamic brake acting
    /// through the drive, which is what a converter is heard doing while the train slows.
    DynamicBrake,
    /// Brake pipe [bar].
    BrakePipe,
    /// Brake cylinder [bar], automatic plus direct brake.
    BrakeCylinder,
    /// Main reservoir [bar].
    MainReservoir,
    /// Pressure change in pipe and cylinder [bar/s], absolute — air is heard when it moves.
    AirFlow,
    /// Slip speed of the driven axles [m/s].
    Slip,
    /// Power controller −1 … +1 (negative = dynamic brake).
    Throttle,
    /// Pantograph travel 0 … 1.
    Pantograph,
    /// Main switch closed (0/1).
    MainSwitch,
    /// Compressor delivering (0/1).
    Compressor,
    /// Widest door position 0 … 1.
    Doors,
    /// Horn operated (0/1).
    Horn,
    /// Any train protection or vigilance device demands an operation (0/1).
    Alert,
    /// The vigilance device (Sifa) sounds its warning (0/1) — the driver has to operate.
    VigilanceAlert,
    /// The train protection (PZB, LZB, …) sounds its horn (0/1): an acknowledgement is
    /// due, a supervision has tripped, or the system is being accepted or ended.
    ProtectionAlert,
    /// Roughness of the track type under the vehicle, 1.0 = welded main-line
    /// rail (see `TrackType::roughness`). The app fills it from the track;
    /// jointed or worn track sits above 1, slab track below.
    Roughness,
    /// Thunder over the vehicle: 1 the moment the clap arrives, rolling off
    /// after it — the far strike rolls for seconds, the near one cracks
    /// (`crate::weather::Strike::thunder`).
    Thunder,
    /// Rain falling on the vehicle (0/1). Filled in by the app like
    /// `Roughness` — the weather lives outside the vehicle state. A rain
    /// loop conditions or scales its volume on it.
    Rain,
    /// Position of a cab control, normalised 0 … 1 over its travel exactly as
    /// the 3D cab reads it ([`CabControl::get`]) — detents sit at the same
    /// values, so a threshold between two of them catches the click. This is
    /// how a lever or switch gets an operating sound: `Rises`/`Falls` for one
    /// edge, `Every` with the detent spacing for every notch passed.
    Control(CabControl),
}

impl Quantity {
    /// Every plain quantity, in the order the editor lists them. The editor
    /// appends `Control(…)` for each [`CabControl::ALL`] entry itself.
    pub const ALL: [Quantity; 25] = [
        Quantity::Speed,
        Quantity::Distance,
        Quantity::EngineRpm,
        Quantity::TapChangerStep,
        Quantity::Circuit,
        Quantity::TractiveEffort,
        Quantity::BrakeEffort,
        Quantity::DynamicBrake,
        Quantity::BrakePipe,
        Quantity::BrakeCylinder,
        Quantity::MainReservoir,
        Quantity::AirFlow,
        Quantity::Slip,
        Quantity::Throttle,
        Quantity::Pantograph,
        Quantity::MainSwitch,
        Quantity::Compressor,
        Quantity::Doors,
        Quantity::Horn,
        Quantity::Alert,
        Quantity::VigilanceAlert,
        Quantity::ProtectionAlert,
        Quantity::Roughness,
        Quantity::Rain,
        Quantity::Thunder,
    ];

    /// Range a slider offers for this quantity — what the vehicle editor's sound preview
    /// scrubs over.
    ///
    /// Not a limit the simulation respects: a value outside it is legal and a curve may
    /// have support points beyond. It is the range worth dragging through by hand, so a
    /// pressure slider does not spend its travel between 0 and 6 bar on the way to 10 000.
    pub fn range(self) -> std::ops::RangeInclusive<f64> {
        match self {
            Quantity::Speed => 0.0..=400.0,
            Quantity::Distance => 0.0..=1_000.0,
            Quantity::EngineRpm => 0.0..=3_000.0,
            Quantity::TapChangerStep => 0.0..=40.0,
            Quantity::Circuit => 0.0..=3.0,
            Quantity::TractiveEffort | Quantity::BrakeEffort | Quantity::DynamicBrake => {
                0.0..=600.0
            }
            Quantity::BrakePipe | Quantity::BrakeCylinder => 0.0..=6.0,
            Quantity::MainReservoir => 0.0..=12.0,
            Quantity::AirFlow => 0.0..=3.0,
            Quantity::Slip => 0.0..=10.0,
            Quantity::Throttle => -1.0..=1.0,
            Quantity::Roughness => 0.0..=3.0,
            // Everything else is a normalised position or a flag.
            _ => 0.0..=1.0,
        }
    }

    /// i18n key of the label — `snd-quantity-speed` and so on.
    pub fn key(self) -> &'static str {
        match self {
            Quantity::Speed => "snd-quantity-speed",
            Quantity::Distance => "snd-quantity-distance",
            Quantity::EngineRpm => "snd-quantity-engine-rpm",
            Quantity::TapChangerStep => "snd-quantity-tap-changer-step",
            Quantity::Circuit => "snd-quantity-circuit",
            Quantity::TractiveEffort => "snd-quantity-tractive-effort",
            Quantity::BrakeEffort => "snd-quantity-brake-effort",
            Quantity::DynamicBrake => "snd-quantity-dynamic-brake",
            Quantity::BrakePipe => "snd-quantity-brake-pipe",
            Quantity::BrakeCylinder => "snd-quantity-brake-cylinder",
            Quantity::MainReservoir => "snd-quantity-main-reservoir",
            Quantity::AirFlow => "snd-quantity-air-flow",
            Quantity::Slip => "snd-quantity-slip",
            Quantity::Throttle => "snd-quantity-throttle",
            Quantity::Pantograph => "snd-quantity-pantograph",
            Quantity::MainSwitch => "snd-quantity-main-switch",
            Quantity::Compressor => "snd-quantity-compressor",
            Quantity::Doors => "snd-quantity-doors",
            Quantity::Horn => "snd-quantity-horn",
            Quantity::Alert => "snd-quantity-alert",
            Quantity::VigilanceAlert => "snd-quantity-vigilance-alert",
            Quantity::ProtectionAlert => "snd-quantity-protection-alert",
            Quantity::Roughness => "snd-quantity-roughness",
            Quantity::Rain => "snd-quantity-rain",
            Quantity::Thunder => "snd-quantity-thunder",
            // The control labels the cab editor already has.
            Quantity::Control(control) => control.key(),
        }
    }
}

/// One reading of every quantity of one vehicle.
///
/// Built once per frame per vehicle; the previous one is kept so that triggers have an edge
/// to detect and `AirFlow` a difference to form.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SoundState {
    pub speed: f64,
    pub distance: f64,
    pub engine_rpm: f64,
    pub tap_changer_step: f64,
    pub circuit: f64,
    pub tractive_effort: f64,
    pub brake_effort: f64,
    #[serde(default)]
    pub dynamic_brake: f64,
    pub brake_pipe: f64,
    pub brake_cylinder: f64,
    #[serde(default)]
    pub main_reservoir: f64,
    pub air_flow: f64,
    pub slip: f64,
    pub throttle: f64,
    pub pantograph: f64,
    pub main_switch: f64,
    pub compressor: f64,
    pub doors: f64,
    pub horn: f64,
    pub alert: f64,
    #[serde(default)]
    pub vigilance_alert: f64,
    #[serde(default)]
    pub protection_alert: f64,
    /// The cab inputs as the driver set them — [`Quantity::Control`] reads the
    /// pure-cab controls straight from here.
    #[serde(default)]
    pub cab: CabInputs,
    /// Positions of the vehicle-level switches, normalised like
    /// [`CabControl::get`]. `compressor` above is "delivering", this is the
    /// switch — an operating click must not wait for the pressure governor.
    #[serde(default)]
    pub battery: f64,
    #[serde(default)]
    pub pantograph_switch: f64,
    #[serde(default)]
    pub main_switch_position: f64,
    #[serde(default)]
    pub compressor_switch: f64,
    #[serde(default)]
    pub train_type: f64,
    /// AFB target 0 … 1 over the vehicle's `v_max`.
    #[serde(default)]
    pub afb_target: f64,
    /// Track roughness under the vehicle, neutral 1.0. [`Self::sample`] sets
    /// the neutral value; the app overwrites it from the track network, which
    /// the sampler deliberately does not see.
    #[serde(default = "neutral_roughness")]
    pub roughness: f64,
    /// Rain on the vehicle (0/1), filled in by the app like `roughness`.
    #[serde(default)]
    pub rain: f64,
    /// Thunder, 1 as the clap arrives and rolling off after it.
    #[serde(default)]
    pub thunder: f64,
}

fn neutral_roughness() -> f64 {
    1.0
}

/// All quantities at rest — except the roughness, whose neutral value is 1.0:
/// a state without track information must not read as unnaturally smooth rail.
impl Default for SoundState {
    fn default() -> Self {
        Self {
            speed: 0.0,
            distance: 0.0,
            engine_rpm: 0.0,
            tap_changer_step: 0.0,
            circuit: 0.0,
            tractive_effort: 0.0,
            brake_effort: 0.0,
            dynamic_brake: 0.0,
            brake_pipe: 0.0,
            brake_cylinder: 0.0,
            main_reservoir: 0.0,
            air_flow: 0.0,
            slip: 0.0,
            throttle: 0.0,
            pantograph: 0.0,
            main_switch: 0.0,
            compressor: 0.0,
            doors: 0.0,
            horn: 0.0,
            alert: 0.0,
            vigilance_alert: 0.0,
            protection_alert: 0.0,
            cab: CabInputs::default(),
            battery: 0.0,
            pantograph_switch: 0.0,
            main_switch_position: 0.0,
            compressor_switch: 0.0,
            train_type: 0.0,
            afb_target: 0.0,
            roughness: neutral_roughness(),
            rain: 0.0,
            thunder: 0.0,
        }
    }
}

impl SoundState {
    pub fn get(&self, quantity: Quantity) -> f64 {
        match quantity {
            Quantity::Speed => self.speed,
            Quantity::Distance => self.distance,
            Quantity::EngineRpm => self.engine_rpm,
            Quantity::TapChangerStep => self.tap_changer_step,
            Quantity::Circuit => self.circuit,
            Quantity::TractiveEffort => self.tractive_effort,
            Quantity::BrakeEffort => self.brake_effort,
            Quantity::DynamicBrake => self.dynamic_brake,
            Quantity::BrakePipe => self.brake_pipe,
            Quantity::BrakeCylinder => self.brake_cylinder,
            Quantity::MainReservoir => self.main_reservoir,
            Quantity::AirFlow => self.air_flow,
            Quantity::Slip => self.slip,
            Quantity::Throttle => self.throttle,
            Quantity::Pantograph => self.pantograph,
            Quantity::MainSwitch => self.main_switch,
            Quantity::Compressor => self.compressor,
            Quantity::Doors => self.doors,
            Quantity::Horn => self.horn,
            Quantity::Alert => self.alert,
            Quantity::VigilanceAlert => self.vigilance_alert,
            Quantity::ProtectionAlert => self.protection_alert,
            Quantity::Roughness => self.roughness,
            Quantity::Rain => self.rain,
            Quantity::Thunder => self.thunder,
            Quantity::Control(control) => match control {
                CabControl::AfbTarget => self.afb_target,
                CabControl::Battery => self.battery,
                CabControl::Pantograph => self.pantograph_switch,
                CabControl::MainSwitch => self.main_switch_position,
                CabControl::Compressor => self.compressor_switch,
                CabControl::TrainType => self.train_type,
                other => other.get_inputs(&self.cab).unwrap_or(0.0),
            },
        }
    }

    /// Writes one quantity, the inverse of [`Self::get`] — `false` for a quantity this
    /// state cannot set by itself.
    ///
    /// The simulator never needs this: it samples a vehicle and reads the result. The
    /// vehicle editor does — its sound preview lets the author scrub a quantity and hear
    /// what the entry makes of it, which is the whole point of a preview.
    ///
    /// The exception is [`Quantity::Control`] for a control that lives in [`CabInputs`]:
    /// writing one goes through [`CabControl::set`], which needs a whole [`Train`] to scale
    /// against. Those are reported rather than silently ignored, so the editor can say so.
    ///
    /// [`Train`]: crate::train::Train
    pub fn set(&mut self, quantity: Quantity, value: f64) -> bool {
        match quantity {
            Quantity::Speed => self.speed = value,
            Quantity::Distance => self.distance = value,
            Quantity::EngineRpm => self.engine_rpm = value,
            Quantity::TapChangerStep => self.tap_changer_step = value,
            Quantity::Circuit => self.circuit = value,
            Quantity::TractiveEffort => self.tractive_effort = value,
            Quantity::BrakeEffort => self.brake_effort = value,
            Quantity::DynamicBrake => self.dynamic_brake = value,
            Quantity::BrakePipe => self.brake_pipe = value,
            Quantity::BrakeCylinder => self.brake_cylinder = value,
            Quantity::MainReservoir => self.main_reservoir = value,
            Quantity::AirFlow => self.air_flow = value,
            Quantity::Slip => self.slip = value,
            Quantity::Throttle => self.throttle = value,
            Quantity::Pantograph => self.pantograph = value,
            Quantity::MainSwitch => self.main_switch = value,
            Quantity::Compressor => self.compressor = value,
            Quantity::Doors => self.doors = value,
            Quantity::Horn => self.horn = value,
            Quantity::Alert => self.alert = value,
            Quantity::VigilanceAlert => self.vigilance_alert = value,
            Quantity::ProtectionAlert => self.protection_alert = value,
            Quantity::Roughness => self.roughness = value,
            Quantity::Rain => self.rain = value,
            Quantity::Thunder => self.thunder = value,
            Quantity::Control(control) => match control {
                CabControl::AfbTarget => self.afb_target = value,
                CabControl::Battery => self.battery = value,
                CabControl::Pantograph => self.pantograph_switch = value,
                CabControl::MainSwitch => self.main_switch_position = value,
                CabControl::Compressor => self.compressor_switch = value,
                CabControl::TrainType => self.train_type = value,
                _ => return false,
            },
        }
        true
    }

    /// Reads the state of one vehicle.
    ///
    /// `protection` is the merged output of the train protection of the whole train, `cab`
    /// the desk that drives it — both are train-level, everything else belongs to the
    /// vehicle itself. `previous` and `dt` are only needed for `AirFlow`; without them it
    /// stays 0.
    pub fn sample(
        vehicle: &Vehicle,
        cab: &CabInputs,
        protection: &ProtectionOutput,
        previous: Option<&SoundState>,
        dt: f64,
    ) -> Self {
        let pipe = vehicle.brake.pipe;
        let cylinder = vehicle.brake.cylinder + vehicle.brake.direct_cylinder;
        let air_flow = match previous {
            Some(prev) if dt > 0.0 => {
                ((pipe - prev.brake_pipe).abs() + (cylinder - prev.brake_cylinder).abs()) / dt
            }
            _ => 0.0,
        };
        // ponytail: the first chain speaks for the vehicle — one sound source per vehicle,
        // so a two-engine railcar is heard through its engine 1. Split it up as soon as
        // sounds are placed per chain.
        let drive = vehicle.traction.drives[0];
        Self {
            speed: vehicle.v.abs() * 3.6,
            distance: vehicle.x.abs(),
            engine_rpm: drive.engine_rpm,
            tap_changer_step: drive.step,
            circuit: drive.circuit as f64,
            tractive_effort: vehicle.tractive_effort.abs() / 1000.0,
            brake_effort: vehicle.brake_effort.abs() / 1000.0,
            dynamic_brake: vehicle.brake.dynamic_force.abs() / 1000.0,
            brake_pipe: pipe,
            brake_cylinder: cylinder,
            main_reservoir: vehicle.brake.main_reservoir,
            air_flow,
            slip: vehicle.slip.abs(),
            throttle: cab.throttle,
            pantograph: vehicle.traction.pantograph,
            main_switch: f64::from(vehicle.traction.main_switch),
            compressor: f64::from(vehicle.brake.compressor_running && vehicle.traction.compressor),
            doors: vehicle.doors.left.travel.max(vehicle.doors.right.travel),
            horn: f64::from(cab.horn),
            alert: f64::from(protection.alert),
            vigilance_alert: f64::from(protection.vigilance_alert),
            protection_alert: f64::from(protection.protection_alert),
            cab: *cab,
            battery: f64::from(vehicle.traction.battery),
            pantograph_switch: f64::from(vehicle.traction.pantograph_command),
            main_switch_position: f64::from(vehicle.traction.main_switch_command),
            compressor_switch: f64::from(vehicle.traction.compressor),
            train_type: {
                use crate::safety::de::TrainType;
                match vehicle.safety.train_type() {
                    Some(TrainType::O) | None => 0.0,
                    Some(TrainType::M) => 0.5,
                    Some(TrainType::U) => 1.0,
                }
            },
            afb_target: {
                let v_max = if vehicle.spec.v_max > 0.0 {
                    vehicle.spec.v_max
                } else {
                    160.0
                };
                (cab.afb_target / v_max).clamp(0.0, 1.0)
            },
            roughness: neutral_roughness(),
            rain: 0.0,
            thunder: 0.0,
        }
    }
}

/// A dependency: support points that map a quantity onto volume or playback speed.
///
/// Between the points it interpolates linearly, beyond the ends it holds the last value —
/// so a curve of two points is a ramp with a floor and a ceiling, which is what most of them
/// are. An empty curve is neutral (1.0), not silent: that way an entry can carry a quantity
/// before anyone has drawn its points.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Curve {
    pub quantity: Quantity,
    /// `(quantity value, result)`, ascending in the first element.
    pub points: Vec<(f64, f64)>,
}

impl Curve {
    /// A ramp from `(x0, y0)` to `(x1, y1)` — the shape nearly every dependency has.
    pub fn ramp(quantity: Quantity, x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Self {
            quantity,
            points: vec![(x0, y0), (x1, y1)],
        }
    }

    /// A crossfade window: silent below `x0`, `peak` between `x1` and `x2`, silent again
    /// above `x3`.
    ///
    /// This is what layering a sound is made of. One recorded loop stretched over a whole
    /// speed range drags its formants along with the playback rate and arrives at the top
    /// as a toy train; three loops whose windows overlap each stay near their own pitch.
    /// Give the neighbours a shared flank — layer A's `x2 … x3` is layer B's `x0 … x1` —
    /// and the sum stays roughly `peak` right through the handover.
    pub fn window(quantity: Quantity, x0: f64, x1: f64, x2: f64, x3: f64, peak: f64) -> Self {
        Self {
            quantity,
            points: vec![(x0, 0.0), (x1, peak), (x2, peak), (x3, 0.0)],
        }
    }

    /// Value at `x`.
    pub fn at(&self, x: f64) -> f64 {
        let Some(&(first_x, first_y)) = self.points.first() else {
            return 1.0;
        };
        if x <= first_x {
            return first_y;
        }
        for pair in self.points.windows(2) {
            let ((ax, ay), (bx, by)) = (pair[0], pair[1]);
            if x <= bx {
                // Two points at the same x are a step, not a division by zero.
                if bx <= ax {
                    return by;
                }
                return ay + (by - ay) * (x - ax) / (bx - ax);
            }
        }
        self.points.last().map(|p| p.1).unwrap_or(1.0)
    }

    pub fn eval(&self, state: &SoundState) -> f64 {
        self.at(state.get(self.quantity))
    }
}

/// A condition: the quantity has to lie inside the window, otherwise the sound stays silent.
///
/// Brake squeal is one of these, not an event — `Speed` in 3 … 25 km/h ∧ `BrakeEffort`
/// above its threshold, on a loop.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Condition {
    pub quantity: Quantity,
    pub min: f64,
    pub max: f64,
}

impl Condition {
    pub fn holds(&self, state: &SoundState) -> bool {
        (self.min..=self.max).contains(&state.get(self.quantity))
    }
}

/// What starts a sound.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum Trigger {
    /// No trigger: the sound loops and is modulated. Zusi's "without trigger" checkbox.
    #[default]
    Loop,
    /// The quantity crosses `threshold` upwards.
    Rises { quantity: Quantity, threshold: f64 },
    /// The quantity crosses `threshold` downwards.
    Falls { quantity: Quantity, threshold: f64 },
    /// The quantity crosses a multiple of `interval` — a tap changer notch
    /// (`TapChangerStep`, 1.0) or a rail joint (`Distance`, 30.0).
    ///
    /// ponytail: rail joints out of a distance interval rather than out of the track. Zusi
    /// has the route builder place them, with an editor function that inserts one every
    /// x metres — this is that function at run time. A `DeviceKind::RailJoint` on the edge
    /// replaces the interval as soon as joints are to sit where the track says.
    Every { quantity: Quantity, interval: f64 },
}

impl Trigger {
    /// `true` in exactly the frame in which the trigger fires.
    pub fn fires(&self, now: &SoundState, previous: &SoundState) -> bool {
        match *self {
            Trigger::Loop => false,
            Trigger::Rises {
                quantity,
                threshold,
            } => previous.get(quantity) < threshold && now.get(quantity) >= threshold,
            Trigger::Falls {
                quantity,
                threshold,
            } => previous.get(quantity) >= threshold && now.get(quantity) < threshold,
            Trigger::Every { quantity, interval } => {
                if interval <= 0.0 {
                    return false;
                }
                let step = |value: f64| (value / interval).floor();
                step(now.get(quantity)) != step(previous.get(quantity))
            }
        }
    }
}

/// One entry of the sound table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SoundSpec {
    /// Free-form name — what the entry is called in the editor. Not user-visible in the
    /// simulator, so it is not translated.
    pub name: String,
    /// Sample: `"<mod>/assets/<file>"` below the `mods/` directory, or `"synth:<name>"` for
    /// one of the loops the app generates at start-up.
    pub file: String,
    #[serde(default)]
    pub trigger: Trigger,
    /// All of them have to hold.
    #[serde(default)]
    pub conditions: Vec<Condition>,
    /// Volume 0 … 1; without a curve the sound plays at full volume.
    #[serde(default)]
    pub volume: Option<Curve>,
    /// Multiplied into the volume, one factor per curve — how a second
    /// quantity scales an entry whose volume already follows a first one:
    /// the rolling noise follows the speed and is scaled by the track
    /// roughness. Empty means no scaling.
    #[serde(default)]
    pub factors: Vec<Curve>,
    /// Playback speed; without a curve the sample plays at its own pitch.
    #[serde(default)]
    pub pitch: Option<Curve>,
    /// Placed in the world: attenuated by distance and Doppler-shifted. Off means the cab
    /// hears it at a constant place — buzzer, Sifa, the driver's own instruments.
    #[serde(default)]
    pub positional: bool,
}

/// Bounds of the playback speed. Below this a loop turns into a rumble, above it into a
/// whistle, and neither is what the curve author meant.
pub const PITCH_RANGE: std::ops::RangeInclusive<f64> = 0.1..=4.0;

impl SoundSpec {
    /// A continuously running entry — the case without a trigger.
    pub fn is_loop(&self) -> bool {
        self.trigger == Trigger::Loop
    }

    /// Volume and playback speed at this state. Volume is 0 while a condition fails.
    pub fn level(&self, state: &SoundState) -> (f64, f64) {
        let pitch = self
            .pitch
            .as_ref()
            .map(|c| c.eval(state))
            .unwrap_or(1.0)
            .clamp(*PITCH_RANGE.start(), *PITCH_RANGE.end());
        if !self.conditions.iter().all(|c| c.holds(state)) {
            return (0.0, pitch);
        }
        let volume = self.volume.as_ref().map(|c| c.eval(state)).unwrap_or(1.0)
            * self.factors.iter().map(|c| c.eval(state)).product::<f64>();
        (volume.clamp(0.0, 1.0), pitch)
    }

    /// Every quantity this entry looks at, in the order it looks at them and without
    /// repeats — trigger, then conditions, then volume, factors and pitch.
    ///
    /// The vehicle editor's preview asks for this: those are exactly the sliders that
    /// change what the entry does, and nothing else is worth putting on the screen.
    pub fn quantities(&self) -> Vec<Quantity> {
        let trigger = match self.trigger {
            Trigger::Loop => None,
            Trigger::Rises { quantity, .. }
            | Trigger::Falls { quantity, .. }
            | Trigger::Every { quantity, .. } => Some(quantity),
        };
        let mut seen = Vec::new();
        for quantity in trigger
            .into_iter()
            .chain(self.conditions.iter().map(|c| c.quantity))
            .chain(self.volume.iter().map(|c| c.quantity))
            .chain(self.factors.iter().map(|c| c.quantity))
            .chain(self.pitch.iter().map(|c| c.quantity))
        {
            if !seen.contains(&quantity) {
                seen.push(quantity);
            }
        }
        seen
    }

    /// `true` when the entry has to be started in this frame — trigger fired and every
    /// condition holds.
    pub fn fires(&self, now: &SoundState, previous: &SoundState) -> bool {
        self.trigger.fires(now, previous) && self.conditions.iter().all(|c| c.holds(now))
    }
}

/// The table a vehicle without one of its own runs on: the generated sources of
/// [`crate::synth`], wired the way a mod would wire recorded ones.
///
/// It is what a mod would write into its vehicle file, only in code — so the table format is
/// exercised by every vehicle, not just by the one that brings samples. Two mechanisms are
/// worth reading off it:
///
/// - **Layers.** Rolling noise and traction are three entries each, their volume curves
///   overlapping [`Curve::window`]s over speed or engine speed. Only one or two are audible
///   at a time, and each stays near its own pitch. That is how a sample-based vehicle is
///   built too — swap the six `synth:` names for six recordings and nothing else changes.
/// - **Factors.** With the window occupying the volume curve, how *loud* the layer is comes
///   out of [`SoundSpec::factors`]: tractive effort for the electric, engine speed for the
///   diesel, track roughness on top of the rolling noise.
pub fn default_table() -> Vec<SoundSpec> {
    let entry = |name: &str, file: &str, volume: Curve| SoundSpec {
        name: name.into(),
        file: file.into(),
        trigger: Trigger::Loop,
        conditions: Vec::new(),
        volume: Some(volume),
        factors: Vec::new(),
        pitch: None,
        positional: true,
    };
    // One band of a crossfaded loop: the window says when it is heard, the pitch ramp keeps
    // it inside `pitch` over its own stretch of the quantity. The volume curve is the
    // window, so how loud the sound is at all belongs in `factors`.
    let layer = |name: &str, quantity: Quantity, window: [f64; 4], pitch: [f64; 2]| SoundSpec {
        name: name.into(),
        file: format!("synth:{name}"),
        trigger: Trigger::Loop,
        conditions: Vec::new(),
        volume: Some(Curve::window(
            quantity, window[0], window[1], window[2], window[3], 1.0,
        )),
        factors: Vec::new(),
        pitch: Some(Curve::ramp(
            quantity, window[0], pitch[0], window[3], pitch[1],
        )),
        positional: true,
    };
    // Rough or jointed superstructure is audibly louder than welded main-line rail. The
    // same factor sits on every rolling layer and on the rail joints.
    let roughness = || Curve::ramp(Quantity::Roughness, 0.5, 0.75, 2.0, 1.4);
    // How loud the rolling noise is at all; the layers only decide which of them says it.
    let rolling_level = || Curve::ramp(Quantity::Speed, 0.0, 0.0, 60.0, 0.55);
    let effort = || Curve::ramp(Quantity::TractiveEffort, 0.0, 0.0, 250.0, 0.45);
    let rpm_level = || Curve::ramp(Quantity::EngineRpm, 350.0, 0.25, 2250.0, 0.55);
    let electric = || Condition {
        quantity: Quantity::EngineRpm,
        min: 0.0,
        max: 0.0,
    };
    let diesel = || Condition {
        quantity: Quantity::EngineRpm,
        min: 1.0,
        max: f64::INFINITY,
    };

    vec![
        // Rolling noise in three bands over speed: the rumble of the running gear at
        // shunting speed hands over to the hiss of the wheel on the rail at line speed.
        SoundSpec {
            factors: vec![rolling_level(), roughness()],
            ..layer(
                "rolling-low",
                Quantity::Speed,
                [0.0, 12.0, 35.0, 60.0],
                [0.85, 1.25],
            )
        },
        SoundSpec {
            factors: vec![rolling_level(), roughness()],
            ..layer(
                "rolling-mid",
                Quantity::Speed,
                [35.0, 60.0, 95.0, 130.0],
                [0.85, 1.25],
            )
        },
        SoundSpec {
            factors: vec![rolling_level(), roughness()],
            ..layer(
                "rolling-high",
                Quantity::Speed,
                [95.0, 130.0, 260.0, 400.0],
                [0.85, 1.3],
            )
        },
        // Traction, electric: the converter whine follows the motor, and that follows the
        // wheel. The condition keeps it off a running diesel engine.
        SoundSpec {
            conditions: vec![electric()],
            factors: vec![effort()],
            ..layer(
                "traction-low",
                Quantity::Speed,
                [0.0, 5.0, 40.0, 70.0],
                [0.85, 1.25],
            )
        },
        SoundSpec {
            conditions: vec![electric()],
            factors: vec![effort()],
            ..layer(
                "traction-mid",
                Quantity::Speed,
                [40.0, 70.0, 130.0, 180.0],
                [0.85, 1.25],
            )
        },
        SoundSpec {
            conditions: vec![electric()],
            factors: vec![effort()],
            ..layer(
                "traction-high",
                Quantity::Speed,
                [130.0, 180.0, 300.0, 400.0],
                [0.85, 1.3],
            )
        },
        // Traction, diesel: the same three loops, but heard by engine speed. Two sets of
        // entries with one condition each is how Zusi splits a sound that follows different
        // quantities in different states.
        SoundSpec {
            name: "traction-diesel-low".into(),
            conditions: vec![diesel()],
            factors: vec![rpm_level()],
            ..layer(
                "traction-low",
                Quantity::EngineRpm,
                [0.0, 350.0, 750.0, 1050.0],
                [0.85, 1.25],
            )
        },
        SoundSpec {
            name: "traction-diesel-mid".into(),
            conditions: vec![diesel()],
            factors: vec![rpm_level()],
            ..layer(
                "traction-mid",
                Quantity::EngineRpm,
                [750.0, 1050.0, 1500.0, 1850.0],
                [0.85, 1.25],
            )
        },
        SoundSpec {
            name: "traction-diesel-high".into(),
            conditions: vec![diesel()],
            factors: vec![rpm_level()],
            ..layer(
                "traction-high",
                Quantity::EngineRpm,
                [1500.0, 1850.0, 2250.0, 2800.0],
                [0.85, 1.3],
            )
        },
        entry(
            "air",
            "synth:air",
            Curve::ramp(Quantity::AirFlow, 0.0, 0.0, 0.33, 0.5),
        ),
        entry(
            "compressor",
            "synth:compressor",
            Curve::ramp(Quantity::Compressor, 0.0, 0.0, 1.0, 0.3),
        ),
        entry(
            "horn",
            "synth:horn",
            Curve::ramp(Quantity::Horn, 0.0, 0.0, 1.0, 0.7),
        ),
        // The buzzer sits in the cab, not on the vehicle: it must not fade with distance.
        SoundSpec {
            positional: false,
            ..entry(
                "buzzer",
                "synth:buzzer",
                Curve::ramp(Quantity::Alert, 0.0, 0.0, 1.0, 0.35),
            )
        },
        // Rail joints — the same table, only with a trigger instead of a loop.
        SoundSpec {
            name: "rail-joint".into(),
            file: "synth:joint".into(),
            trigger: Trigger::Every {
                quantity: Quantity::Distance,
                interval: 30.0,
            },
            conditions: vec![Condition {
                quantity: Quantity::Speed,
                min: 3.0,
                max: f64::INFINITY,
            }],
            volume: Some(Curve::ramp(Quantity::Speed, 3.0, 0.12, 120.0, 0.35)),
            factors: vec![roughness()],
            pitch: Some(Curve::ramp(Quantity::Speed, 3.0, 0.8, 160.0, 1.4)),
            positional: true,
        },
        // Tap changer contactors: the notch runs through continuously, every whole one
        // passed is a contactor. A vehicle without a tap changer never leaves 0.
        SoundSpec {
            name: "tap-changer".into(),
            file: "synth:contactor".into(),
            trigger: Trigger::Every {
                quantity: Quantity::TapChangerStep,
                interval: 1.0,
            },
            conditions: Vec::new(),
            volume: Some(Curve::ramp(Quantity::TapChangerStep, 0.0, 0.4, 1.0, 0.4)),
            factors: Vec::new(),
            pitch: None,
            positional: true,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(speed: f64) -> SoundState {
        SoundState {
            speed,
            ..SoundState::default()
        }
    }

    #[test]
    fn a_curve_interpolates_and_holds_beyond_its_ends() {
        let curve = Curve::ramp(Quantity::Speed, 0.0, 0.0, 100.0, 1.0);
        assert_eq!(curve.at(-10.0), 0.0);
        assert_eq!(curve.at(0.0), 0.0);
        assert!((curve.at(50.0) - 0.5).abs() < 1e-12);
        assert_eq!(curve.at(100.0), 1.0);
        assert_eq!(curve.at(500.0), 1.0, "held, not extrapolated");
        // Three points, and a step where two share an x.
        let stepped = Curve {
            quantity: Quantity::Speed,
            points: vec![(0.0, 0.0), (10.0, 0.0), (10.0, 1.0)],
        };
        assert_eq!(stepped.at(5.0), 0.0);
        assert_eq!(stepped.at(10.0), 0.0);
        assert_eq!(stepped.at(11.0), 1.0);
        // An empty curve is neutral, so a half-edited entry is audible rather than silent.
        assert_eq!(
            Curve {
                quantity: Quantity::Speed,
                points: Vec::new()
            }
            .at(42.0),
            1.0
        );
    }

    #[test]
    fn conditions_mute_the_sound_but_leave_the_pitch() {
        let spec = SoundSpec {
            name: "squeal".into(),
            file: "synth:air".into(),
            trigger: Trigger::Loop,
            conditions: vec![Condition {
                quantity: Quantity::Speed,
                min: 3.0,
                max: 25.0,
            }],
            volume: Some(Curve::ramp(Quantity::Speed, 0.0, 0.5, 100.0, 0.5)),
            factors: Vec::new(),
            pitch: Some(Curve::ramp(Quantity::Speed, 0.0, 1.0, 100.0, 2.0)),
            positional: true,
        };
        assert_eq!(spec.level(&state(0.0)).0, 0.0, "below the window");
        assert_eq!(spec.level(&state(60.0)).0, 0.0, "above the window");
        assert_eq!(spec.level(&state(10.0)).0, 0.5, "inside");
        // The pitch is computed even while muted — the sound does not jump when it comes in.
        assert!((spec.level(&state(50.0)).1 - 1.5).abs() < 1e-12);
    }

    #[test]
    fn triggers_fire_on_the_edge_only() {
        let rises = Trigger::Rises {
            quantity: Quantity::Speed,
            threshold: 10.0,
        };
        assert!(rises.fires(&state(10.0), &state(9.0)));
        assert!(!rises.fires(&state(20.0), &state(15.0)), "already above");
        assert!(!rises.fires(&state(5.0), &state(15.0)));

        let falls = Trigger::Falls {
            quantity: Quantity::Speed,
            threshold: 10.0,
        };
        assert!(falls.fires(&state(9.0), &state(10.0)));
        assert!(!falls.fires(&state(9.0), &state(8.0)));

        // Rail joints: one per interval, no matter how many frames it takes to cross it.
        let joints = Trigger::Every {
            quantity: Quantity::Distance,
            interval: 30.0,
        };
        let at = |d: f64| SoundState {
            distance: d,
            ..SoundState::default()
        };
        assert!(joints.fires(&at(30.5), &at(29.5)));
        assert!(!joints.fires(&at(31.0), &at(30.5)));
        assert!(joints.fires(&at(61.0), &at(59.0)));
        // A standing train hears nothing, and an interval of 0 does not divide by zero.
        assert!(!joints.fires(&at(30.5), &at(30.5)));
        assert!(
            !Trigger::Every {
                quantity: Quantity::Distance,
                interval: 0.0
            }
            .fires(&at(100.0), &at(0.0))
        );
        // A loop has no edge — it runs.
        assert!(!Trigger::Loop.fires(&state(50.0), &state(0.0)));
    }

    /// A trigger fires only while the conditions hold — that is what keeps rail joints out
    /// of a standing train.
    #[test]
    fn a_trigger_still_obeys_its_conditions() {
        let table = default_table();
        let joint = table
            .iter()
            .find(|s| s.name == "rail-joint")
            .expect("present");
        let moving = |d: f64, v: f64| SoundState {
            distance: d,
            speed: v,
            ..SoundState::default()
        };
        assert!(joint.fires(&moving(30.5, 40.0), &moving(29.5, 40.0)));
        assert!(
            !joint.fires(&moving(30.5, 1.0), &moving(29.5, 1.0)),
            "shunting"
        );
    }

    /// The generated loops are the same data a mod would write — and the electric and
    /// diesel traction entries must not sound at the same time.
    #[test]
    fn the_default_table_covers_the_generated_loops() {
        let table = default_table();
        for name in [
            "rolling-low",
            "rolling-mid",
            "rolling-high",
            "traction-low",
            "traction-diesel-low",
            "air",
            "compressor",
            "horn",
            "buzzer",
        ] {
            let entry = table.iter().find(|s| s.name == name).expect(name);
            assert!(entry.is_loop(), "{name} runs without a trigger");
            assert!(entry.file.starts_with("synth:"), "{name}");
        }
        // Every source the table names has to exist, or the vehicle plays silence with
        // nothing but a warning in the log.
        for entry in &table {
            let generated = entry.file.strip_prefix("synth:").expect(&entry.file);
            assert!(crate::synth::synth(generated).is_some(), "{generated}");
        }

        let sums = |state: &SoundState, prefix: &str| -> f64 {
            table
                .iter()
                .filter(|e| e.name.starts_with(prefix))
                .map(|e| e.level(state).0)
                .sum()
        };
        let running = SoundState {
            engine_rpm: 1500.0,
            tractive_effort: 200.0,
            ..SoundState::default()
        };
        assert_eq!(sums(&running, "traction-low"), 0.0, "electric bands silent");
        assert!(sums(&running, "traction-diesel") > 0.0);
        let electric_loco = SoundState {
            speed: 60.0,
            tractive_effort: 200.0,
            ..SoundState::default()
        };
        assert!(sums(&electric_loco, "traction-mid") > 0.0);
        assert_eq!(sums(&electric_loco, "traction-diesel"), 0.0);
    }

    /// The point of layering: over the whole speed range at least one rolling band is
    /// audible, and the bands together stay near one level instead of dipping in the
    /// handover or doubling up in the overlap.
    #[test]
    fn the_rolling_bands_crossfade_without_a_gap() {
        let table = default_table();
        let bands: Vec<&SoundSpec> = table
            .iter()
            .filter(|e| e.name.starts_with("rolling-"))
            .collect();
        assert_eq!(bands.len(), 3);
        // The windows are the layer selection — their sum is what must stay flat. How loud
        // the rolling noise is at all is a factor on top and rises with the speed, so it is
        // deliberately not part of this.
        for kmh in 12..=260 {
            let at = state(f64::from(kmh));
            let sum: f64 = bands
                .iter()
                .map(|b| b.volume.as_ref().expect("windowed").eval(&at))
                .sum();
            assert!((0.95..=1.05).contains(&sum), "{kmh} km/h: {sum}");
        }
        // And the level itself never drops out between two bands.
        for kmh in 12..=260 {
            let at = state(f64::from(kmh));
            let sum: f64 = bands.iter().map(|b| b.level(&at).0).sum();
            assert!(sum > 0.1, "a hole at {kmh} km/h: {sum}");
        }
        // And no band is resampled far enough to change its character.
        for band in bands {
            for kmh in 0..=400 {
                let (_, pitch) = band.level(&state(f64::from(kmh)));
                assert!((0.8..=1.35).contains(&pitch), "{}: {pitch}", band.name);
            }
        }
    }

    /// A cab control is a sound quantity: the button edge fires a trigger, and
    /// a two-position switch clicks on both edges through `Every`.
    #[test]
    fn cab_controls_fire_sound_triggers() {
        let idle = SoundState::default();
        let mut pressed = SoundState::default();
        pressed.cab.sifa = true;
        let press = Trigger::Rises {
            quantity: Quantity::Control(CabControl::Sifa),
            threshold: 0.5,
        };
        assert!(press.fires(&pressed, &idle));
        assert!(!press.fires(&pressed, &pressed), "no edge, no click");

        let on = SoundState {
            battery: 1.0,
            ..SoundState::default()
        };
        let toggle = Trigger::Every {
            quantity: Quantity::Control(CabControl::Battery),
            interval: 1.0,
        };
        assert!(toggle.fires(&on, &idle), "switching on clicks");
        assert!(toggle.fires(&idle, &on), "switching off clicks too");
    }

    /// A factor curve scales the volume multiplicatively — the track's
    /// roughness makes the same rolling entry louder or quieter.
    #[test]
    fn factors_scale_the_volume() {
        let rolling = default_table()
            .into_iter()
            .find(|e| e.name == "rolling-mid")
            .expect("rolling entry exists");
        let mut smooth = state(60.0);
        smooth.roughness = 0.5;
        let mut rough = state(60.0);
        rough.roughness = 2.0;
        let neutral = state(60.0); // Default roughness is the neutral 1.0.
        let base = rolling.level(&neutral).0;
        assert!(rolling.level(&smooth).0 < base);
        assert!(rolling.level(&rough).0 > base);
        // The factor scales, the pitch stays the speed's business.
        assert_eq!(rolling.level(&rough).1, rolling.level(&neutral).1);
    }

    /// Playback speed stays inside a range a speaker can render.
    #[test]
    fn the_pitch_is_bounded() {
        let spec = SoundSpec {
            name: "wild".into(),
            file: "synth:traction".into(),
            trigger: Trigger::Loop,
            conditions: Vec::new(),
            volume: None,
            factors: Vec::new(),
            pitch: Some(Curve::ramp(Quantity::Speed, 0.0, -5.0, 100.0, 99.0)),
            positional: false,
        };
        assert_eq!(spec.level(&state(0.0)).1, *PITCH_RANGE.start());
        assert_eq!(spec.level(&state(100.0)).1, *PITCH_RANGE.end());
    }
}
