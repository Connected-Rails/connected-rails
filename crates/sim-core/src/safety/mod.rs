//! Train protection: country-neutral abstraction + country packages (plan ch. 9).
//!
//! Every train protection system is a state machine with defined inputs/outputs.
//! The vehicle side only knows [`TrainProtectionSystem`]; which systems a vehicle carries
//! is stated in the vehicle database.

pub mod de;

use crate::cab::CabInputs;
use serde::{Deserialize, Serialize};
use track_model::DeviceKind;

/// What the train protection commands the vehicle to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ProtectionAction {
    #[default]
    None,
    /// Forced braking as a service application (e.g. LZB service braking).
    ForcedServiceBrake,
    /// Forced braking as an emergency application (PZB, Sifa).
    EmergencyBrake,
    /// Traction cut-off only.
    TractionCutOff,
}

impl ProtectionAction {
    /// The stricter of two requests.
    pub fn max(self, other: Self) -> Self {
        use ProtectionAction::*;
        let rank = |a: Self| match a {
            None => 0,
            TractionCutOff => 1,
            ForcedServiceBrake => 2,
            EmergencyBrake => 3,
        };
        if rank(other) > rank(self) {
            other
        } else {
            self
        }
    }
}

/// State of an indicator lamp / a display in the cab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LampState {
    #[default]
    Off,
    On,
    Blinking,
}

/// A display of the train protection (indicator lamp or numeric value for MFA/EBuLa).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Indicator {
    pub name: &'static str,
    pub lamp: LampState,
    /// Numeric value for display instruments (v-Soll, v-Ziel, distance to target).
    pub value: Option<f64>,
}

impl Indicator {
    pub fn lamp(name: &'static str, on: bool) -> Self {
        Self {
            name,
            lamp: if on { LampState::On } else { LampState::Off },
            value: None,
        }
    }

    pub fn state(name: &'static str, lamp: LampState) -> Self {
        Self {
            name,
            lamp,
            value: None,
        }
    }

    pub fn value(name: &'static str, value: f64) -> Self {
        Self {
            name,
            lamp: LampState::Off,
            value: Some(value),
        }
    }
}

/// Output of a train protection system after one step.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProtectionOutput {
    pub action: ProtectionAction,
    /// Supervision speed [km/h], if the system prescribes one.
    pub speed_limit: Option<f64>,
    /// Target speed [km/h] (LZB/ETCS).
    pub target_speed: Option<f64>,
    /// Distance to target [m] (LZB/ETCS).
    pub target_distance: Option<f64>,
    /// The system demands an operation (for sound: horn/forced braking).
    pub alert: bool,
}

impl ProtectionOutput {
    /// Combine two outputs (several systems on one vehicle).
    pub fn merge(self, other: Self) -> Self {
        Self {
            action: self.action.max(other.action),
            speed_limit: min_option(self.speed_limit, other.speed_limit),
            target_speed: min_option(self.target_speed, other.target_speed),
            target_distance: min_option(self.target_distance, other.target_distance),
            alert: self.alert || other.alert,
        }
    }
}

fn min_option(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (x, None) => x,
        (None, y) => y,
    }
}

/// A trackside device passed by a vehicle carrying an antenna.
#[derive(Debug, Clone, PartialEq)]
pub struct TracksideEvent {
    pub device: DeviceKind,
    /// Payload as RON text (see `TracksideDevice::payload`).
    pub payload: String,
    /// How far behind the vehicle antenna the device now lies [m].
    pub s_offset: f64,
    /// Activation — for signal-dependent magnets the interlocking decides
    /// (1000 Hz only with Vr0/Vr2, 2000 Hz only with Hp0).
    pub active: bool,
}

/// Vehicle state as far as the train protection needs it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SafetyTrainState {
    /// Speed [km/h], magnitude.
    pub v_kmh: f64,
    /// Monotonically increasing distance travelled [m] — reference for all distance
    /// supervisions.
    pub odometer: f64,
    /// Permitted speed at the current location [km/h].
    pub line_speed: f64,
    /// Braking active (for the release logic).
    pub braking: bool,
    /// Train length [m] — the LZB needs it: outside CIR-ELKE a speed rise only takes
    /// effect once the rear of the train has passed the point of change.
    pub train_length: f64,
}

impl SafetyTrainState {
    pub fn standstill(&self) -> bool {
        self.v_kmh < 0.5
    }
}

/// Phase of the function test (Funktionsprüfung) that every modern train protection
/// system runs when it is switched on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SelfTestPhase {
    /// Lamp test — every indicator is lit.
    Lamps,
    /// The device tests itself, all indicators are dark.
    Running,
    /// Result present, waiting for the driver's acknowledgement.
    AwaitAck,
    /// Test passed, the system is operational.
    #[default]
    Passed,
}

/// Function test of a train protection system (plan 9.3/9.4).
///
/// It only runs at a standstill: rolling away during the test freezes it, and the
/// forced braking therefore stays applied.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SelfTest {
    phase: SelfTestPhase,
    timer: f64,
    /// Duration of the lamp test [s].
    pub t_lamps: f64,
    /// Duration of the internal test [s].
    pub t_running: f64,
}

impl Default for SelfTest {
    fn default() -> Self {
        Self::passed()
    }
}

impl SelfTest {
    /// A device that has already been tested (vehicle set up before the scenario starts).
    pub fn passed() -> Self {
        Self {
            phase: SelfTestPhase::Passed,
            timer: 0.0,
            t_lamps: 2.0,
            t_running: 3.0,
        }
    }

    /// Starts the test — the system switches on.
    pub fn start() -> Self {
        Self {
            phase: SelfTestPhase::Lamps,
            ..Self::passed()
        }
    }

    pub fn restart(&mut self) {
        self.phase = SelfTestPhase::Lamps;
        self.timer = 0.0;
    }

    pub fn phase(&self) -> SelfTestPhase {
        self.phase
    }

    pub fn is_passed(&self) -> bool {
        self.phase == SelfTestPhase::Passed
    }

    /// During the lamp test every indicator of the system is lit.
    pub fn lamp_test(&self) -> bool {
        self.phase == SelfTestPhase::Lamps
    }

    /// Advances the test. `ack` is the acknowledgement button of the system
    /// (PZB: Wachsam, LZB: test button). Returns `true` once the test is passed.
    pub fn step(&mut self, dt: f64, train: &SafetyTrainState, ack: bool) -> bool {
        if self.phase == SelfTestPhase::Passed {
            return true;
        }
        // The test requires a standstill; if the train moves it does not progress.
        if !train.standstill() {
            return false;
        }
        self.timer += dt;
        match self.phase {
            SelfTestPhase::Lamps if self.timer >= self.t_lamps => {
                self.phase = SelfTestPhase::Running;
                self.timer = 0.0;
            }
            SelfTestPhase::Running if self.timer >= self.t_running => {
                self.phase = SelfTestPhase::AwaitAck;
                self.timer = 0.0;
            }
            SelfTestPhase::AwaitAck if ack => {
                self.phase = SelfTestPhase::Passed;
                self.timer = 0.0;
            }
            _ => {}
        }
        self.phase == SelfTestPhase::Passed
    }
}

/// Country-neutral interface of every train protection system.
pub trait TrainProtectionSystem {
    fn update(
        &mut self,
        dt: f64,
        train: &SafetyTrainState,
        cab: &CabInputs,
        events: &[TracksideEvent],
    ) -> ProtectionOutput;

    /// Indicator lamps / displays for the cab.
    fn indicators(&self) -> Vec<Indicator>;

    /// Isolating switch.
    fn isolate(&mut self, isolated: bool);

    fn is_isolated(&self) -> bool;

    /// Short name for debug overlays.
    fn name(&self) -> &'static str;
}

/// Train protection equipment of a vehicle.
///
/// Country packages are compile-time Rust (plan 9.1); hence an enum instead of
/// `Vec<Box<dyn …>>` — that way cloning and serialising (save/load, replays) stay possible
/// without extra code.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
// ponytail: the German package is a few hundred bytes larger than the empty variant, so
// every coach carries them unused. Boxing would cost an allocation per vehicle and the
// `Copy` of the inner types — not worth it below thousands of vehicles per train.
#[allow(clippy::large_enum_variant)]
pub enum SafetySystems {
    /// Vehicle without train protection (coach).
    #[default]
    None,
    /// German package: Sifa, Indusi/PZB (I 54 … PZB 90 V2.0, ÖBB PZB 60), LZB 80.
    De(de::DeSafety),
}

/// Train protection **fitted** to a vehicle — the declarative part that belongs in the
/// vehicle database. [`SafetySystems`] is the running state built from it, so which systems
/// a train carries follows from its vehicles, not from a run-time option.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SafetyEquipment {
    /// Vehicle without train protection (coach, freight wagon).
    #[default]
    None,
    /// German package. `pzb: None` with `lzb: true` is a vehicle that may only run under
    /// LZB guidance; whether the LZB actually works also depends on the line (conductor
    /// cable).
    De {
        #[serde(default)]
        pzb: Option<de::PzbVariant>,
        #[serde(default)]
        lzb: bool,
        #[serde(default)]
        sifa: Option<de::SifaKind>,
        /// Train category the PZB starts in (the driver sets it from the brake sheet).
        #[serde(default)]
        train_type: de::TrainType,
    },
}

impl SafetyEquipment {
    /// Builds the running systems from the fitment.
    pub fn build(self) -> SafetySystems {
        match self {
            SafetyEquipment::None => SafetySystems::None,
            SafetyEquipment::De {
                pzb,
                lzb,
                sifa,
                train_type,
            } => SafetySystems::De(de::DeSafety {
                sifa: sifa.map(de::Sifa::with_kind),
                pzb: pzb.map(|v| de::Pzb::with_variant(v, train_type)),
                lzb: lzb.then(de::Lzb80::new),
            }),
        }
    }
}

impl SafetySystems {
    pub fn update(
        &mut self,
        dt: f64,
        train: &SafetyTrainState,
        cab: &CabInputs,
        events: &[TracksideEvent],
    ) -> ProtectionOutput {
        match self {
            SafetySystems::None => ProtectionOutput::default(),
            SafetySystems::De(de) => de.update(dt, train, cab, events),
        }
    }

    pub fn indicators(&self) -> Vec<Indicator> {
        match self {
            SafetySystems::None => Vec::new(),
            SafetySystems::De(de) => de.indicators(),
        }
    }

    /// Switching on (setting the vehicle up) starts the function tests.
    pub fn power_on(&mut self) {
        match self {
            SafetySystems::None => {}
            SafetySystems::De(de) => de.power_on(),
        }
    }
}
