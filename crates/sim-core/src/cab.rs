//! Cab controls as simulation input (plan ch. 12).

use crate::brakes::DriverBrakeValve;
use crate::train::{Motion, Train};
use serde::{Deserialize, Serialize};

/// Edge detection for push buttons.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    last: bool,
}

impl Edge {
    /// `true` exactly in the step in which the button is pressed.
    pub fn rising(&mut self, now: bool) -> bool {
        let r = now && !self.last;
        self.last = now;
        r
    }

    /// `true` exactly in the step in which the button is released.
    pub fn falling(&mut self, now: bool) -> bool {
        let f = !now && self.last;
        self.last = now;
        f
    }

    pub fn held(&self) -> bool {
        self.last
    }
}

/// All control values of a cab. Buttons are to be read as "currently pressed";
/// the systems evaluate the edges themselves.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CabInputs {
    /// Reverser: −1 backwards, 0 off, +1 forwards.
    pub reverser: i8,
    /// Power controller −1 … +1 (negative = dynamic brake).
    pub throttle: f64,
    pub brake_valve: DriverBrakeValve,
    /// Direct (additional) brake 0 … 1.
    pub direct_brake: f64,
    /// Release button of the loco brake — releases the traction unit's own brake while the
    /// train brake stays applied.
    #[serde(default)]
    pub brake_release: bool,
    /// Parking brake set (spring-applied brake or hand brake).
    #[serde(default)]
    pub parking_brake: bool,
    /// Electrically transmitted, pre-controlled air brake switched on: the whole train
    /// applies at once instead of waiting for the pressure wave.
    #[serde(default)]
    pub ep_brake: bool,
    /// Starter button of the diesel engine.
    #[serde(default)]
    pub engine_start: bool,
    /// Range selector of a two-range gearbox: `true` = road gear, `false` = shunting gear.
    /// Only a shunter with the gearbox has it, and it only changes at a stand.
    #[serde(default = "on")]
    pub road_gear: bool,
    /// Emergency valve of the cab pulled. It vents the brake pipe and the driver's own
    /// valve cannot make it up — that is what "emergency" means.
    #[serde(default)]
    pub emergency_valve: bool,
    /// Regulator, cutoff, blower, damper, firehole and injectors of a steam locomotive.
    /// The cutoff carries the reverser: negative is back gear.
    #[serde(default)]
    pub steam: crate::steam::SteamControls,
    /// Shovelfuls waiting to go on the grate — the fireman's stroke, consumed by
    /// [`crate::steam::fire`] on the next step.
    #[serde(default)]
    pub shovel: f64,
    /// Door release, left and right in the direction of travel.
    #[serde(default)]
    pub door_release_left: bool,
    #[serde(default)]
    pub door_release_right: bool,
    /// Door close button.
    #[serde(default)]
    pub door_close: bool,
    pub sanding: bool,
    /// Sifa pedal/button.
    pub sifa: bool,
    pub pzb_acknowledge: bool,
    pub pzb_exempt: bool,
    pub pzb_override: bool,
    pub lzb_takeover: bool,
    pub lzb_end: bool,
    /// LZB function test button (acknowledges the test result).
    pub lzb_test: bool,
    pub horn: bool,
    /// AFB switched on.
    pub afb: bool,
    /// AFB target speed [km/h].
    pub afb_target: f64,
    /// Wiper switch: 0 off, 1 interval, 2 slow, 3 fast.
    #[serde(default)]
    pub wipers: u8,
    /// Headlights (Spitzensignal) on. Defaults to on — the AI never touches the
    /// switch, and a train without lights at night would be an operating error.
    #[serde(default = "on")]
    pub headlights: bool,
    /// Cab light on.
    #[serde(default)]
    pub cab_light: bool,
    /// Instrument backlighting 0 … 1 — its own dimmer, as most locos have one
    /// next to the cab lamp switch.
    #[serde(default)]
    pub instrument_light: f64,
    /// Softkeys next to the cab displays, read by the `display(ctx)` script hook.
    #[serde(default)]
    pub display_buttons: [bool; 8],
}

fn on() -> bool {
    true
}

impl Default for CabInputs {
    fn default() -> Self {
        Self {
            reverser: 0,
            throttle: 0.0,
            brake_valve: DriverBrakeValve::Release,
            direct_brake: 0.0,
            brake_release: false,
            parking_brake: false,
            ep_brake: false,
            engine_start: false,
            road_gear: true,
            emergency_valve: false,
            steam: crate::steam::SteamControls::default(),
            shovel: 0.0,
            door_release_left: false,
            door_release_right: false,
            door_close: false,
            sanding: false,
            sifa: false,
            pzb_acknowledge: false,
            pzb_exempt: false,
            pzb_override: false,
            lzb_takeover: false,
            lzb_end: false,
            lzb_test: false,
            horn: false,
            afb: false,
            afb_target: 0.0,
            wipers: 0,
            headlights: true,
            cab_light: false,
            instrument_light: 0.0,
            display_buttons: [false; 8],
        }
    }
}

/// The interactive 3D cab of a model: driver's eye and mouse-operable controls
/// (plan ch. 12). Pure data like [`crate::train::VehicleModel`], which carries it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CabSpec {
    /// Driver's eye in model space [m]: X right, Y above the rail head, −Z ahead.
    pub eye: [f32; 3],
    /// Mouse-operable controls.
    #[serde(default)]
    pub controls: Vec<CabControlSpec>,
}

impl Default for CabSpec {
    fn default() -> Self {
        Self {
            // The spot the hard-wired cab camera used before there was data for it.
            eye: [-0.6, 2.8, -8.0],
            controls: Vec::new(),
        }
    }
}

/// One interactive control: a glTF node bound to a simulation input — the
/// write-direction counterpart of a [`crate::train::Part`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CabControlSpec {
    /// Name of the glTF node; its whole subtree takes the mouse.
    pub node: String,
    /// The simulation input the control operates.
    pub input: CabControl,
    /// How the node moves between input 0 and 1.
    #[serde(default)]
    pub motion: Motion,
}

/// A screen in the cab: a texture rendered by the app onto a glTF node
/// (plan ch. 12 — MFA, EBuLa and the like).
///
/// Content, in order of what is easiest for a mod: the [`Widget`] list draws
/// itself with no code at all; a vehicle script with a `display(ctx)` hook
/// overrides it with a draw list and can build whole menu trees, its softkeys
/// being ordinary cab controls ([`CabControl::Display`]). Neither present —
/// the screen stays dark.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplaySpec {
    /// Name the script hook is asked for (`ctx.display`).
    pub name: String,
    /// glTF node whose meshes show the texture.
    pub node: String,
    /// Texture resolution [px].
    #[serde(default = "display_width")]
    pub width: u32,
    #[serde(default = "display_height")]
    pub height: u32,
    /// Code-free content, drawn unless the script takes over.
    #[serde(default)]
    pub widgets: Vec<Widget>,
    /// HTML/CSS/JS page below `mods/` (e.g. `example/displays/ebula.html`).
    /// When set it draws this screen alone; widgets and the Lua hook are the
    /// simpler paths for values and menus.
    #[serde(default)]
    pub html: Option<String>,
}

fn display_width() -> u32 {
    256
}

fn display_height() -> u32 {
    160
}

/// Where a display widget reads its value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DisplaySource {
    /// A sound-table quantity — speed, pressures, control positions
    /// ([`crate::sound::Quantity`], including `Control(…)`).
    Quantity(crate::sound::Quantity),
    /// A named indicator of the train protection (`mfa_v_soll`,
    /// `mfa_zielentfernung`, …); 0 while it is absent.
    Indicator(String),
}

/// One element of a code-free display. Coordinates in pixels from the top
/// left of the texture; colors are linear RGBA 0 … 1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Widget {
    /// Fixed text.
    Label {
        x: f32,
        y: f32,
        size: f32,
        text: String,
        #[serde(default = "white")]
        color: [f32; 4],
    },
    /// A value, scaled and formatted: `{value * scale:.decimals} {unit}`.
    Value {
        x: f32,
        y: f32,
        size: f32,
        source: DisplaySource,
        #[serde(default)]
        decimals: u8,
        #[serde(default)]
        unit: String,
        #[serde(default = "one_f64")]
        scale: f64,
        #[serde(default = "white")]
        color: [f32; 4],
    },
    /// A filled bar growing with the value from 0 to `max`.
    Bar {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        source: DisplaySource,
        max: f64,
        #[serde(default = "white")]
        color: [f32; 4],
    },
}

fn white() -> [f32; 4] {
    [1.0, 1.0, 1.0, 1.0]
}

fn one_f64() -> f64 {
    1.0
}

/// Every simulation input a cab control can be bound to — the write-direction
/// counterpart of [`crate::sound::Quantity`]: a closed registry instead of
/// hand-wired fields, so editor and app can enumerate and map them.
///
/// Values are normalised to 0…1 over the control's travel; [`CabControl::set`]
/// translates into the native units and [`CabControl::get`] back. Whether a
/// control is a push button, a latching switch or a continuous lever follows
/// from the input itself ([`CabControl::momentary`], [`CabControl::positions`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CabControl {
    /// Power controller −1 … +1 (0 = full dynamic brake, 1 = full power).
    Throttle,
    /// Reverser: 0 = backwards, ½ = off, 1 = forwards.
    Reverser,
    /// Driver's brake valve along its quadrant: fill – release – lap –
    /// service range – emergency.
    BrakeValve,
    /// Direct (additional) brake 0 … 1.
    DirectBrake,
    /// AFB target speed 0 … `v_max`.
    AfbTarget,
    Sifa,
    PzbAcknowledge,
    PzbExempt,
    PzbOverride,
    LzbTakeover,
    LzbEnd,
    LzbTest,
    Horn,
    Sanding,
    BrakeRelease,
    EngineStart,
    /// Range selector of a two-range gearbox: shunting gear – road gear, standstill only.
    RoadGear,
    DoorReleaseLeft,
    DoorReleaseRight,
    DoorClose,
    ParkingBrake,
    EpBrake,
    /// AFB on/off.
    Afb,
    Battery,
    /// Pantograph command (the raise state follows on its own).
    Pantograph,
    MainSwitch,
    Compressor,
    /// Train type switch (Zugartschalter): O – M – U, standstill only.
    TrainType,
    /// Wiper switch: off – interval – slow – fast.
    Wipers,
    /// Headlights (Spitzensignal) on/off.
    Headlights,
    /// Cab light on/off.
    CabLight,
    /// Instrument backlighting, continuous over its dimmer 0 … 1.
    InstrumentLight,
    /// Softkey next to a cab display (0 … 7) — the `display(ctx)` script hook
    /// reads it, which is how a screen gets nested menus.
    Display(u8),
}

/// Driver's brake valve position ↔ normalised lever travel. The service range
/// is continuous; everything else is a detent along the same quadrant.
const VALVE_FILL: f64 = 0.0;
const VALVE_RELEASE: f64 = 0.15;
const VALVE_LAP: f64 = 0.3;
const VALVE_SERVICE_START: f64 = 0.4;
const VALVE_SERVICE_END: f64 = 0.9;
const VALVE_EMERGENCY: f64 = 1.0;
/// Largest service application [bar] — the same limit the keyboard uses.
const FULL_SERVICE_DROP: f64 = 1.5;

fn valve_to_axis(valve: DriverBrakeValve) -> f64 {
    match valve {
        DriverBrakeValve::Fill => VALVE_FILL,
        DriverBrakeValve::Release => VALVE_RELEASE,
        DriverBrakeValve::Lap => VALVE_LAP,
        DriverBrakeValve::Service(drop) => {
            VALVE_SERVICE_START
                + (drop / FULL_SERVICE_DROP).clamp(0.0, 1.0)
                    * (VALVE_SERVICE_END - VALVE_SERVICE_START)
        }
        DriverBrakeValve::Emergency => VALVE_EMERGENCY,
    }
}

fn axis_to_valve(axis: f64) -> DriverBrakeValve {
    // Detents win everything up to halfway towards the next position.
    if axis < (VALVE_FILL + VALVE_RELEASE) / 2.0 {
        DriverBrakeValve::Fill
    } else if axis < (VALVE_RELEASE + VALVE_LAP) / 2.0 {
        DriverBrakeValve::Release
    } else if axis < (VALVE_LAP + VALVE_SERVICE_START) / 2.0 {
        DriverBrakeValve::Lap
    } else if axis < (VALVE_SERVICE_END + VALVE_EMERGENCY) / 2.0 {
        let t = ((axis - VALVE_SERVICE_START) / (VALVE_SERVICE_END - VALVE_SERVICE_START))
            .clamp(0.0, 1.0);
        DriverBrakeValve::Service(t * FULL_SERVICE_DROP)
    } else {
        DriverBrakeValve::Emergency
    }
}

/// Proportional band of the AFB [km/h]: full power that far below the target,
/// full dynamic brake that far above it. The air brake blends in over the same
/// width beyond the dynamic band, up to a full service application.
const AFB_BAND: f64 = 10.0;

/// What the AFB commands for one step: the power controller, and the driver's
/// brake valve wherever the dynamic brake alone is not enough.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AfbCommand {
    pub throttle: f64,
    pub valve: DriverBrakeValve,
}

/// AFB (plan 9.4): target speed controller of the occupied vehicle. Returns
/// what replaces the driver's levers for this step, or `None` while the AFB is
/// off, not fitted, or the reverser stands in neutral. Under LZB guidance the
/// LZB's v-soll caps the dial, so the train follows the braking curve on its
/// own; forced braking still wins downstream, exactly as it does against the
/// driver's lever.
///
/// The brake blending works as it does at the prototype: the dynamic brake is
/// preferential, and the air brake supplements it once the speed excess runs
/// past the dynamic band — immediately on a train whose drive has no dynamic
/// brake. A brake application by the driver overrides the AFB: its traction is
/// cut, and the commanded valve never brakes less than the driver's own lever.
pub fn afb_control(train: &Train, cab: &CabInputs) -> Option<AfbCommand> {
    let seat = train.vehicles.get(train.cab)?;
    if !seat.spec.afb || !cab.afb || cab.reverser == 0 {
        return None;
    }
    let v_soll = train.vehicles.iter().find_map(|v| v.safety.lzb_v_soll());
    let target = v_soll.map_or(cab.afb_target, |v| cab.afb_target.min(v));
    let error = target - train.speed_kmh().abs();
    let driver_brakes = matches!(
        cab.brake_valve,
        DriverBrakeValve::Service(_) | DriverBrakeValve::Emergency
    );
    let throttle = if driver_brakes {
        (error / AFB_BAND).clamp(-1.0, 0.0)
    } else {
        (error / AFB_BAND).clamp(-1.0, 1.0)
    };
    let dynamic_band = if train.vehicles.iter().any(|v| v.spec.has_dynamic_brake()) {
        AFB_BAND
    } else {
        0.0
    };
    let excess = -error - dynamic_band;
    let valve = if excess > 0.0 {
        let drop = (excess / AFB_BAND).clamp(0.0, 1.0) * FULL_SERVICE_DROP;
        max_by_braking(cab.brake_valve, DriverBrakeValve::Service(drop))
    } else {
        cab.brake_valve
    };
    Some(AfbCommand { throttle, valve })
}

/// The valve command that brakes harder: the AFB never releases a brake the
/// driver applied, and its own demand wins over a released lever.
fn max_by_braking(a: DriverBrakeValve, b: DriverBrakeValve) -> DriverBrakeValve {
    let rank = |v: DriverBrakeValve| match v {
        DriverBrakeValve::Fill => -1.0,
        DriverBrakeValve::Release => 0.0,
        DriverBrakeValve::Lap => 0.1,
        DriverBrakeValve::Service(d) => 1.0 + d,
        DriverBrakeValve::Emergency => f64::INFINITY,
    };
    if rank(b) > rank(a) { b } else { a }
}

/// AFB scale: the running-gear limit of the occupied vehicle, or 160 km/h
/// when the spec does not state one.
fn afb_scale(train: &Train) -> f64 {
    train
        .vehicles
        .get(train.cab)
        .map(|v| v.spec.v_max)
        .filter(|v| *v > 0.0)
        .unwrap_or(160.0)
}

impl CabControl {
    pub const ALL: [CabControl; 40] = [
        CabControl::Throttle,
        CabControl::Reverser,
        CabControl::BrakeValve,
        CabControl::DirectBrake,
        CabControl::AfbTarget,
        CabControl::Sifa,
        CabControl::PzbAcknowledge,
        CabControl::PzbExempt,
        CabControl::PzbOverride,
        CabControl::LzbTakeover,
        CabControl::LzbEnd,
        CabControl::LzbTest,
        CabControl::Horn,
        CabControl::Sanding,
        CabControl::BrakeRelease,
        CabControl::EngineStart,
        CabControl::RoadGear,
        CabControl::DoorReleaseLeft,
        CabControl::DoorReleaseRight,
        CabControl::DoorClose,
        CabControl::ParkingBrake,
        CabControl::EpBrake,
        CabControl::Afb,
        CabControl::Battery,
        CabControl::Pantograph,
        CabControl::MainSwitch,
        CabControl::Compressor,
        CabControl::TrainType,
        CabControl::Wipers,
        CabControl::Headlights,
        CabControl::CabLight,
        CabControl::InstrumentLight,
        CabControl::Display(0),
        CabControl::Display(1),
        CabControl::Display(2),
        CabControl::Display(3),
        CabControl::Display(4),
        CabControl::Display(5),
        CabControl::Display(6),
        CabControl::Display(7),
    ];

    /// i18n key of the label (same pattern as [`crate::sound::Quantity::key`]).
    pub fn key(self) -> &'static str {
        match self {
            CabControl::Throttle => "cab-input-throttle",
            CabControl::Reverser => "cab-input-reverser",
            CabControl::BrakeValve => "cab-input-brake-valve",
            CabControl::DirectBrake => "cab-input-direct-brake",
            CabControl::AfbTarget => "cab-input-afb-target",
            CabControl::Sifa => "cab-input-sifa",
            CabControl::PzbAcknowledge => "cab-input-pzb-acknowledge",
            CabControl::PzbExempt => "cab-input-pzb-exempt",
            CabControl::PzbOverride => "cab-input-pzb-override",
            CabControl::LzbTakeover => "cab-input-lzb-takeover",
            CabControl::LzbEnd => "cab-input-lzb-end",
            CabControl::LzbTest => "cab-input-lzb-test",
            CabControl::Horn => "cab-input-horn",
            CabControl::Sanding => "cab-input-sanding",
            CabControl::BrakeRelease => "cab-input-brake-release",
            CabControl::EngineStart => "cab-input-engine-start",
            CabControl::DoorReleaseLeft => "cab-input-door-release-left",
            CabControl::DoorReleaseRight => "cab-input-door-release-right",
            CabControl::DoorClose => "cab-input-door-close",
            CabControl::ParkingBrake => "cab-input-parking-brake",
            CabControl::EpBrake => "cab-input-ep-brake",
            CabControl::Afb => "cab-input-afb",
            CabControl::Battery => "cab-input-battery",
            CabControl::Pantograph => "cab-input-pantograph",
            CabControl::MainSwitch => "cab-input-main-switch",
            CabControl::Compressor => "cab-input-compressor",
            CabControl::RoadGear => "cab-input-road-gear",
            CabControl::TrainType => "cab-input-train-type",
            CabControl::Wipers => "cab-input-wipers",
            CabControl::Headlights => "cab-input-headlights",
            CabControl::CabLight => "cab-input-cab-light",
            CabControl::InstrumentLight => "cab-input-instrument-light",
            CabControl::Display(n) => match n {
                0 => "cab-input-display-1",
                1 => "cab-input-display-2",
                2 => "cab-input-display-3",
                3 => "cab-input-display-4",
                4 => "cab-input-display-5",
                5 => "cab-input-display-6",
                6 => "cab-input-display-7",
                _ => "cab-input-display-8",
            },
        }
    }

    /// Held while the mouse button is down, springs back on release (push buttons).
    pub fn momentary(self) -> bool {
        matches!(
            self,
            CabControl::Sifa
                | CabControl::PzbAcknowledge
                | CabControl::PzbExempt
                | CabControl::PzbOverride
                | CabControl::LzbTakeover
                | CabControl::LzbEnd
                | CabControl::LzbTest
                | CabControl::Horn
                | CabControl::Sanding
                | CabControl::BrakeRelease
                | CabControl::EngineStart
                | CabControl::DoorReleaseLeft
                | CabControl::DoorReleaseRight
                | CabControl::DoorClose
                | CabControl::Display(_)
        )
    }

    /// Number of discrete positions a click or scroll steps through; 0 = continuous.
    pub fn positions(self) -> u8 {
        match self {
            CabControl::Throttle
            | CabControl::BrakeValve
            | CabControl::DirectBrake
            | CabControl::AfbTarget
            | CabControl::InstrumentLight => 0,
            CabControl::Reverser | CabControl::TrainType => 3,
            CabControl::Wipers => 4,
            _ => 2,
        }
    }

    /// Travel of one scroll-wheel notch on a continuous control.
    pub fn scroll_step(self) -> f64 {
        match self {
            CabControl::DirectBrake => 0.1,
            _ => 0.05,
        }
    }

    /// Value of the inputs that live in [`CabInputs`] alone, normalised to
    /// 0…1; `None` for the vehicle-level switches, which need the train.
    /// The sound table reads control positions through this.
    pub fn get_inputs(self, cab: &CabInputs) -> Option<f64> {
        let value = match self {
            CabControl::Throttle => (cab.throttle + 1.0) / 2.0,
            CabControl::Reverser => f64::from(cab.reverser + 1) / 2.0,
            CabControl::BrakeValve => valve_to_axis(cab.brake_valve),
            CabControl::DirectBrake => cab.direct_brake,
            CabControl::Sifa => f64::from(cab.sifa),
            CabControl::PzbAcknowledge => f64::from(cab.pzb_acknowledge),
            CabControl::PzbExempt => f64::from(cab.pzb_exempt),
            CabControl::PzbOverride => f64::from(cab.pzb_override),
            CabControl::LzbTakeover => f64::from(cab.lzb_takeover),
            CabControl::LzbEnd => f64::from(cab.lzb_end),
            CabControl::LzbTest => f64::from(cab.lzb_test),
            CabControl::Horn => f64::from(cab.horn),
            CabControl::Sanding => f64::from(cab.sanding),
            CabControl::BrakeRelease => f64::from(cab.brake_release),
            CabControl::EngineStart => f64::from(cab.engine_start),
            CabControl::RoadGear => f64::from(cab.road_gear),
            CabControl::DoorReleaseLeft => f64::from(cab.door_release_left),
            CabControl::DoorReleaseRight => f64::from(cab.door_release_right),
            CabControl::DoorClose => f64::from(cab.door_close),
            CabControl::ParkingBrake => f64::from(cab.parking_brake),
            CabControl::EpBrake => f64::from(cab.ep_brake),
            CabControl::Afb => f64::from(cab.afb),
            CabControl::Wipers => f64::from(cab.wipers.min(3)) / 3.0,
            CabControl::Headlights => f64::from(cab.headlights),
            CabControl::CabLight => f64::from(cab.cab_light),
            CabControl::InstrumentLight => cab.instrument_light,
            CabControl::Display(n) => f64::from(cab.display_buttons[usize::from(n) % 8]),
            CabControl::AfbTarget
            | CabControl::Battery
            | CabControl::Pantograph
            | CabControl::MainSwitch
            | CabControl::Compressor
            | CabControl::TrainType => return None,
        };
        Some(value)
    }

    /// Current value, normalised to 0…1 over the control's travel.
    pub fn get(self, train: &Train, cab: &CabInputs) -> f64 {
        if let Some(value) = self.get_inputs(cab) {
            return value;
        }
        let powered = |f: fn(&crate::electric::TractionState) -> bool| {
            train
                .vehicles
                .iter()
                .find(|v| v.is_powered())
                .map(|v| f64::from(f(&v.traction)))
                .unwrap_or(0.0)
        };
        match self {
            CabControl::AfbTarget => (cab.afb_target / afb_scale(train)).clamp(0.0, 1.0),
            CabControl::Battery => powered(|t| t.battery),
            CabControl::Pantograph => powered(|t| t.pantograph_command),
            CabControl::MainSwitch => powered(|t| t.main_switch_command),
            CabControl::Compressor => powered(|t| t.compressor),
            CabControl::TrainType => {
                use crate::safety::de::TrainType;
                match train.vehicles.iter().find_map(|v| v.safety.train_type()) {
                    Some(TrainType::O) | None => 0.0,
                    Some(TrainType::M) => 0.5,
                    Some(TrainType::U) => 1.0,
                }
            }
            // Everything else came out of `get_inputs` above.
            _ => unreachable!("get_inputs covers the pure cab inputs"),
        }
    }

    /// Applies a normalised 0…1 value to the simulation. Discrete inputs snap
    /// to the nearest position; the vehicle switches (battery, pantograph, …)
    /// act on every powered vehicle, exactly like the keyboard.
    pub fn set(self, train: &mut Train, cab: &mut CabInputs, value: f64) {
        fn powered(train: &mut Train, mut set: impl FnMut(&mut crate::electric::TractionState)) {
            for v in train.vehicles.iter_mut().filter(|v| v.is_powered()) {
                set(&mut v.traction);
            }
        }
        let value = value.clamp(0.0, 1.0);
        let on = value >= 0.5;
        match self {
            CabControl::Throttle => cab.throttle = value * 2.0 - 1.0,
            CabControl::Reverser => cab.reverser = (value * 2.0).round() as i8 - 1,
            CabControl::BrakeValve => cab.brake_valve = axis_to_valve(value),
            CabControl::DirectBrake => cab.direct_brake = value,
            CabControl::AfbTarget => cab.afb_target = (value * afb_scale(train)).round(),
            CabControl::Sifa => cab.sifa = on,
            CabControl::PzbAcknowledge => cab.pzb_acknowledge = on,
            CabControl::PzbExempt => cab.pzb_exempt = on,
            CabControl::PzbOverride => cab.pzb_override = on,
            CabControl::LzbTakeover => cab.lzb_takeover = on,
            CabControl::LzbEnd => cab.lzb_end = on,
            CabControl::LzbTest => cab.lzb_test = on,
            CabControl::Horn => cab.horn = on,
            CabControl::Sanding => cab.sanding = on,
            CabControl::BrakeRelease => cab.brake_release = on,
            CabControl::EngineStart => cab.engine_start = on,
            CabControl::RoadGear => cab.road_gear = on,
            CabControl::DoorReleaseLeft => cab.door_release_left = on,
            CabControl::DoorReleaseRight => cab.door_release_right = on,
            CabControl::DoorClose => cab.door_close = on,
            CabControl::ParkingBrake => cab.parking_brake = on,
            CabControl::EpBrake => cab.ep_brake = on,
            CabControl::Afb => cab.afb = on,
            CabControl::Wipers => cab.wipers = (value * 3.0).round() as u8,
            CabControl::Headlights => cab.headlights = on,
            CabControl::CabLight => cab.cab_light = on,
            CabControl::InstrumentLight => cab.instrument_light = value,
            CabControl::Display(n) => cab.display_buttons[usize::from(n) % 8] = on,
            CabControl::Battery => {
                for v in train.vehicles.iter_mut().filter(|v| v.is_powered()) {
                    // Switching the battery on sets the train protection up,
                    // exactly like the keyboard path (plan 9.3/9.4).
                    if on && !v.traction.battery {
                        v.safety.power_on();
                    }
                    v.traction.battery = on;
                }
            }
            CabControl::Pantograph => powered(train, |t| t.pantograph_command = on),
            CabControl::MainSwitch => powered(train, |t| t.main_switch_command = on),
            CabControl::Compressor => powered(train, |t| t.compressor = on),
            CabControl::TrainType => {
                use crate::safety::de::TrainType;
                let wanted = match (value * 2.0).round() as u8 {
                    0 => TrainType::O,
                    1 => TrainType::M,
                    _ => TrainType::U,
                };
                let speed = train.speed();
                for v in &mut train.vehicles {
                    v.safety.set_train_type(wanted, speed);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::train::{RailCondition, Train, Vehicle, VehicleSpec};
    use track_model::{EdgeId, TrackPosition};

    fn train() -> Train {
        let spec = VehicleSpec {
            v_max: 200.0,
            drives: vec![crate::drive::DriveSpec::new(
                crate::drive::TractionSpec::Curve {
                    force: vec![(0.0, 100_000.0)],
                    v_max: 200.0,
                    brake: vec![],
                    ramp_time: 5.0,
                },
            )],
            legacy_traction: None,
            ..VehicleSpec::default()
        };
        Train {
            vehicles: vec![Vehicle::new(spec, TrackPosition::new(EdgeId(0), 0.0, 1))],
            couplers: vec![],
            cab: 0,
            rail: RailCondition::Dry,
            number: String::new(),
            doors: Default::default(),
        }
    }

    #[test]
    fn every_control_reports_what_was_set() {
        let mut train = train();
        let mut cab = CabInputs::default();
        for control in CabControl::ALL {
            for value in [0.0, 1.0] {
                control.set(&mut train, &mut cab, value);
                if control == CabControl::TrainType {
                    continue; // no PZB fitted — the switch does not exist
                }
                assert_eq!(control.get(&train, &cab), value, "{control:?} at {value}");
            }
        }
    }

    #[test]
    fn brake_valve_axis_round_trips_every_position() {
        for valve in [
            DriverBrakeValve::Fill,
            DriverBrakeValve::Release,
            DriverBrakeValve::Lap,
            DriverBrakeValve::Service(0.75),
            DriverBrakeValve::Emergency,
        ] {
            assert_eq!(axis_to_valve(valve_to_axis(valve)), valve, "{valve:?}");
        }
    }

    #[test]
    fn discrete_controls_snap() {
        let mut train = train();
        let mut cab = CabInputs::default();
        CabControl::Reverser.set(&mut train, &mut cab, 0.9);
        assert_eq!(cab.reverser, 1);
        CabControl::Reverser.set(&mut train, &mut cab, 0.4);
        assert_eq!(cab.reverser, 0);
        CabControl::BrakeValve.set(&mut train, &mut cab, VALVE_RELEASE + 0.01);
        assert_eq!(cab.brake_valve, DriverBrakeValve::Release);
    }

    #[test]
    fn afb_holds_the_dial_speed() {
        let mut train = train();
        train.vehicles[0].spec.afb = true;
        let mut cab = CabInputs {
            afb: true,
            afb_target: 100.0,
            reverser: 1,
            ..CabInputs::default()
        };
        // Standing well below the dial: full power.
        assert_eq!(afb_control(&train, &cab).unwrap().throttle, 1.0);
        // Above the dial: dynamic brake.
        train.vehicles[0].v = 110.0 / 3.6;
        assert!(afb_control(&train, &cab).unwrap().throttle < 0.0);
        // Off, reverser in neutral, or not fitted: the driver's levers stay in charge.
        cab.afb = false;
        assert_eq!(afb_control(&train, &cab), None);
        cab.afb = true;
        cab.reverser = 0;
        assert_eq!(afb_control(&train, &cab), None);
        cab.reverser = 1;
        train.vehicles[0].spec.afb = false;
        assert_eq!(afb_control(&train, &cab), None);
    }

    #[test]
    fn afb_blends_the_air_brake_in() {
        let mut train = train();
        train.vehicles[0].spec.afb = true;
        // Give the drive a dynamic brake, so the air brake is the supplement.
        if let crate::drive::TractionSpec::Curve { brake, .. } =
            &mut train.vehicles[0].spec.drives[0].traction
        {
            *brake = vec![(0.0, 100_000.0)];
        }
        let mut cab = CabInputs {
            afb: true,
            afb_target: 100.0,
            reverser: 1,
            ..CabInputs::default()
        };
        // Inside the dynamic band the air stays out of it.
        train.vehicles[0].v = 105.0 / 3.6;
        assert_eq!(afb_control(&train, &cab).unwrap().valve, cab.brake_valve);
        // Past the band the air brake supplements the saturated dynamic brake.
        train.vehicles[0].v = 115.0 / 3.6;
        let cmd = afb_control(&train, &cab).unwrap();
        assert_eq!(cmd.throttle, -1.0);
        assert_eq!(cmd.valve, DriverBrakeValve::Service(0.75));
        // Far past it: a full service application, never an emergency one.
        train.vehicles[0].v = 140.0 / 3.6;
        assert_eq!(
            afb_control(&train, &cab).unwrap().valve,
            DriverBrakeValve::Service(FULL_SERVICE_DROP)
        );
        // A deeper application by the driver is never released, and it cuts the
        // AFB's traction.
        cab.brake_valve = DriverBrakeValve::Service(1.2);
        train.vehicles[0].v = 115.0 / 3.6;
        assert_eq!(
            afb_control(&train, &cab).unwrap().valve,
            DriverBrakeValve::Service(1.2)
        );
        train.vehicles[0].v = 0.0;
        let cmd = afb_control(&train, &cab).unwrap();
        assert_eq!(cmd.throttle, 0.0);
        assert_eq!(cmd.valve, DriverBrakeValve::Service(1.2));
        // Without a dynamic brake the air brake steps in right at the target.
        if let crate::drive::TractionSpec::Curve { brake, .. } =
            &mut train.vehicles[0].spec.drives[0].traction
        {
            *brake = Vec::new();
        }
        cab.brake_valve = DriverBrakeValve::Release;
        train.vehicles[0].v = 105.0 / 3.6;
        assert_eq!(
            afb_control(&train, &cab).unwrap().valve,
            DriverBrakeValve::Service(0.75)
        );
    }

    #[test]
    fn lzb_v_soll_caps_the_afb_dial() {
        use crate::safety::de::lzb::{Lzb80, LzbTelegram};
        use crate::safety::{
            SafetySystems, SafetyTrainState, TracksideEvent, TrainProtectionSystem,
        };
        use track_model::DeviceKind;

        // Drive an LZB into guidance: telegram received, takeover acknowledged.
        let mut lzb = Lzb80::new();
        let telegram = LzbTelegram {
            permitted_speed: 60.0,
            target_speed: 60.0,
            target_distance: 12_000.0,
            end_of_authority: false,
            length: 1000.0,
            block_mode: Default::default(),
            cir_elke: false,
        };
        let state = SafetyTrainState::default();
        let event = TracksideEvent {
            device: DeviceKind::LineConductor,
            payload: ron::to_string(&telegram).unwrap(),
            s_offset: 0.0,
            active: true,
        };
        lzb.update(0.1, &state, &CabInputs::default(), &[event]);
        let takeover = CabInputs {
            lzb_takeover: true,
            ..CabInputs::default()
        };
        lzb.update(0.1, &state, &takeover, &[]);
        assert!(lzb.is_guiding());

        let mut train = train();
        train.vehicles[0].spec.afb = true;
        train.vehicles[0].safety = SafetySystems::De(crate::safety::de::DeSafety {
            sifa: None,
            pzb: None,
            lzb: Some(lzb),
        });
        let cab = CabInputs {
            afb: true,
            afb_target: 160.0,
            reverser: 1,
            ..CabInputs::default()
        };
        // Rolling at the LZB's v-soll: the dial says 160, the LZB says 60 — no power.
        train.vehicles[0].v = 60.0 / 3.6;
        assert!(afb_control(&train, &cab).unwrap().throttle <= 0.0);
        // Well above it: full braking, air brake included.
        train.vehicles[0].v = 90.0 / 3.6;
        let cmd = afb_control(&train, &cab).unwrap();
        assert_eq!(cmd.throttle, -1.0);
        assert_eq!(cmd.valve, DriverBrakeValve::Service(FULL_SERVICE_DROP));
    }

    #[test]
    fn cab_spec_survives_ron() {
        let spec = CabSpec {
            eye: [-0.6, 2.6, -7.5],
            controls: vec![CabControlSpec {
                node: "lever_throttle".into(),
                input: CabControl::Throttle,
                motion: Motion::Rotate {
                    axis: [1.0, 0.0, 0.0],
                    degrees: -40.0,
                },
            }],
        };
        let text = ron::ser::to_string(&spec).unwrap();
        let back: CabSpec = ron::from_str(&text).unwrap();
        assert_eq!(back, spec);
    }
}
