//! What a key or a controller button does, and where the player changes it.
//!
//! Bevy brings the raw devices and nothing above them: `ButtonInput<KeyCode>` for the
//! keyboard and a `Gamepad` component per connected controller, each holding its own
//! `ButtonInput<GamepadButton>` plus the analogue axes. There is no binding layer and no
//! rebinding UI in the engine, so this module is the thin one in between — a table of
//! actions with their default key and controller button, a resource holding what the
//! player has changed, and one `SystemParam` ([`Input`]) that every driving system asks
//! for instead of the keyboard.
//!
//! Two representations, because they answer different questions:
//!
//! * [`Bindings`] is the settings group. One string a line, `name key pad`, and only for
//!   what actually differs from the default — a settings file stays readable and a
//!   rebound key survives a new default for everything else.
//! * [`Binds`] is what the frame reads: one [`Bind`] per action, subscripted by
//!   `Action as usize`. Rebuilt from [`Bindings`] whenever that changes.
//!
//! Two kinds of row, because the desk has two kinds of control. An [`Action`] is a
//! button: it is pressed or it is not, and a key or a controller button works it. A
//! [`Lever`] has a *position*, and only an axis can hold one — a rate key nudges a notch,
//! a trigger holds it. The three levers that have a position are the power controller, the
//! driver's brake valve and the direct brake; each can be put on a stick axis or an analogue
//! trigger, and drives the lever absolutely from there.
//!
//! Levers are unbound out of the box. A bound one writes its lever every frame, so a
//! default binding would hold the brake valve at Release for everyone who has a pad plugged
//! in and never touches it.
//!
//! Looking around and walking are the exception in the other direction: the right stick
//! looks and the left one walks, always and unbindably. Those are not levers of the desk,
//! and a menu offering to bind "look up" to one axis of one of them would be answering a
//! question nobody asked.
//!
//! Multiplayer: nothing here crosses the wire. A binding decides which lever the local
//! player moves; what travels is the lever's position in `CabInputs`, exactly as before.

use bevy::ecs::system::SystemParam;
use bevy::input::gamepad::GamepadInput;
use bevy::prelude::*;
use bevy::reflect::FromReflect;
use bevy::reflect::enums::DynamicEnum;
use bevy::settings::{ReflectSettingsGroup, SettingsGroup};

/// Everything the player can press.
///
/// The order is the order of [`ACTIONS`] read out flat, which is also the order of the
/// controls page — `as usize` subscripts [`Binds`], so a lookup is an index rather than a
/// search. `actions_are_in_enum_order` holds the two together.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    // Driving.
    ThrottleUp,
    ThrottleDown,
    ThrottleOff,
    ReverserForward,
    ReverserNeutral,
    ReverserBack,
    RoadGear,
    Afb,
    AfbDown,
    AfbUp,
    // Brakes.
    BrakeApply,
    BrakeRelease,
    BrakeLap,
    BrakeFill,
    BrakeEmergency,
    DirectBrakeApply,
    DirectBrakeRelease,
    LocoBrakeRelease,
    ParkingBrake,
    EpBrake,
    Sanding,
    // Train protection.
    Sifa,
    PzbAcknowledge,
    PzbFree,
    PzbOverride,
    LzbTakeover,
    LzbEnd,
    LzbTest,
    TrainType,
    Horn,
    // Preparing the vehicle.
    Battery,
    Pantograph,
    MainSwitch,
    Compressor,
    EngineStart,
    Headlights,
    CabLight,
    InstrumentLightUp,
    InstrumentLightDown,
    Wipers,
    DoorLeft,
    DoorRight,
    DoorClose,
    // View and overlays.
    ViewCab,
    ViewOutside,
    ViewWayside,
    ViewWalk,
    LookLeft,
    LookRight,
    LookUp,
    LookDown,
    ZoomIn,
    ZoomOut,
    HelpOverlay,
    Diagnostics,
    HudMode,
    ModManager,
    Pause,
    // On foot.
    WalkForward,
    WalkBack,
    WalkLeft,
    WalkRight,
    WalkRun,
    WalkDoor,
}

/// The controls page, by group: the heading, and under it every action with the name it
/// is stored under and what it carries when nothing has been rebound.
///
/// Most actions have no controller button — a desk has some sixty levers and a pad has
/// sixteen buttons, so the ones that get one are the ones a hand reaches for while the
/// train is moving. The rest are there to be bound by whoever wants them.
type Row = (Action, &'static str, KeyCode, Option<GamepadButton>);
pub const ACTIONS: [(&str, &[Row]); 6] = [
    (
        "ctl-group-driving",
        &[
            (
                Action::ThrottleUp,
                "throttle-up",
                KeyCode::KeyW,
                Some(GamepadButton::DPadUp),
            ),
            (
                Action::ThrottleDown,
                "throttle-down",
                KeyCode::KeyS,
                Some(GamepadButton::DPadDown),
            ),
            (Action::ThrottleOff, "throttle-off", KeyCode::KeyX, None),
            (
                Action::ReverserForward,
                "reverser-forward",
                KeyCode::KeyR,
                None,
            ),
            (
                Action::ReverserNeutral,
                "reverser-neutral",
                KeyCode::KeyT,
                None,
            ),
            (Action::ReverserBack, "reverser-back", KeyCode::KeyF, None),
            (Action::RoadGear, "road-gear", KeyCode::Backquote, None),
            (Action::Afb, "afb", KeyCode::Digit6, None),
            (Action::AfbDown, "afb-down", KeyCode::Digit7, None),
            (Action::AfbUp, "afb-up", KeyCode::Digit8, None),
        ],
    ),
    (
        "ctl-group-brakes",
        &[
            (
                Action::BrakeApply,
                "brake-apply",
                KeyCode::KeyD,
                Some(GamepadButton::RightTrigger2),
            ),
            (
                Action::BrakeRelease,
                "brake-release",
                KeyCode::KeyA,
                Some(GamepadButton::LeftTrigger2),
            ),
            (Action::BrakeLap, "brake-lap", KeyCode::KeyQ, None),
            (Action::BrakeFill, "brake-fill", KeyCode::KeyZ, None),
            (
                Action::BrakeEmergency,
                "brake-emergency",
                KeyCode::KeyE,
                None,
            ),
            (
                Action::DirectBrakeApply,
                "direct-brake-apply",
                KeyCode::KeyC,
                Some(GamepadButton::RightTrigger),
            ),
            (
                Action::DirectBrakeRelease,
                "direct-brake-release",
                KeyCode::KeyV,
                Some(GamepadButton::LeftTrigger),
            ),
            (
                Action::LocoBrakeRelease,
                "loco-brake-release",
                KeyCode::KeyL,
                None,
            ),
            (Action::ParkingBrake, "parking-brake", KeyCode::KeyP, None),
            (Action::EpBrake, "ep-brake", KeyCode::KeyO, None),
            (Action::Sanding, "sanding", KeyCode::KeyG, None),
        ],
    ),
    (
        "ctl-group-safety",
        &[
            (
                Action::Sifa,
                "sifa",
                KeyCode::Space,
                Some(GamepadButton::East),
            ),
            (
                Action::PzbAcknowledge,
                "pzb-acknowledge",
                KeyCode::PageDown,
                Some(GamepadButton::North),
            ),
            (Action::PzbFree, "pzb-free", KeyCode::End, None),
            (Action::PzbOverride, "pzb-override", KeyCode::Delete, None),
            (Action::LzbTakeover, "lzb-takeover", KeyCode::KeyN, None),
            (Action::LzbEnd, "lzb-end", KeyCode::KeyM, None),
            (Action::LzbTest, "lzb-test", KeyCode::KeyB, None),
            (Action::TrainType, "train-type", KeyCode::KeyU, None),
            (
                Action::Horn,
                "horn",
                KeyCode::KeyH,
                Some(GamepadButton::South),
            ),
        ],
    ),
    (
        "ctl-group-vehicle",
        &[
            (Action::Battery, "battery", KeyCode::Digit1, None),
            (Action::Pantograph, "pantograph", KeyCode::Digit2, None),
            (Action::MainSwitch, "main-switch", KeyCode::Digit3, None),
            (Action::Compressor, "compressor", KeyCode::Digit4, None),
            (Action::EngineStart, "engine-start", KeyCode::Digit5, None),
            (Action::Headlights, "headlights", KeyCode::Digit9, None),
            (Action::CabLight, "cab-light", KeyCode::Digit0, None),
            (
                Action::InstrumentLightUp,
                "instrument-light-up",
                KeyCode::Period,
                None,
            ),
            (
                Action::InstrumentLightDown,
                "instrument-light-down",
                KeyCode::Comma,
                None,
            ),
            (Action::Wipers, "wipers", KeyCode::KeyY, None),
            (Action::DoorLeft, "door-left", KeyCode::KeyJ, None),
            (Action::DoorRight, "door-right", KeyCode::KeyK, None),
            (Action::DoorClose, "door-close", KeyCode::KeyI, None),
        ],
    ),
    (
        "ctl-group-view",
        &[
            (Action::ViewCab, "view-cab", KeyCode::F1, None),
            (Action::ViewOutside, "view-outside", KeyCode::F2, None),
            (Action::ViewWayside, "view-wayside", KeyCode::F3, None),
            (
                Action::ViewWalk,
                "view-walk",
                KeyCode::F4,
                Some(GamepadButton::RightThumb),
            ),
            (Action::LookLeft, "look-left", KeyCode::ArrowLeft, None),
            (Action::LookRight, "look-right", KeyCode::ArrowRight, None),
            (Action::LookUp, "look-up", KeyCode::ArrowUp, None),
            (Action::LookDown, "look-down", KeyCode::ArrowDown, None),
            (Action::ZoomIn, "zoom-in", KeyCode::NumpadAdd, None),
            (Action::ZoomOut, "zoom-out", KeyCode::NumpadSubtract, None),
            (
                Action::HelpOverlay,
                "help-overlay",
                KeyCode::F5,
                Some(GamepadButton::Select),
            ),
            (Action::Diagnostics, "diagnostics", KeyCode::F6, None),
            (Action::HudMode, "hud-mode", KeyCode::F7, None),
            (Action::ModManager, "mod-manager", KeyCode::F9, None),
            (
                Action::Pause,
                "pause",
                KeyCode::Escape,
                Some(GamepadButton::Start),
            ),
        ],
    ),
    (
        "ctl-group-walk",
        &[
            (Action::WalkForward, "walk-forward", KeyCode::KeyW, None),
            (Action::WalkBack, "walk-back", KeyCode::KeyS, None),
            (Action::WalkLeft, "walk-left", KeyCode::KeyA, None),
            (Action::WalkRight, "walk-right", KeyCode::KeyD, None),
            (
                Action::WalkRun,
                "walk-run",
                KeyCode::ShiftLeft,
                Some(GamepadButton::LeftThumb),
            ),
            (
                Action::WalkDoor,
                "walk-door",
                KeyCode::KeyE,
                Some(GamepadButton::West),
            ),
        ],
    ),
];

/// Every row of [`ACTIONS`], flat and in enum order.
pub fn rows() -> impl Iterator<Item = &'static Row> {
    ACTIONS.iter().flat_map(|(_, group)| group.iter())
}

impl Action {
    /// The name in the settings file; the message key is this with `ctl-` in front.
    pub fn name(self) -> &'static str {
        rows().nth(self as usize).map_or("", |row| row.1)
    }

    /// The action stored under `name`, if it is still one.
    fn named(name: &str) -> Option<Action> {
        rows().find(|row| row.1 == name).map(|row| row.0)
    }
}

/// The controls of the desk that have a position rather than a direction — the ones a
/// stick or a trigger can hold where a key can only nudge.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lever {
    /// −1 … 1: full electric brake through coasting to full power.
    Throttle,
    /// 0 … 1.5 bar of brake pipe drop. Nothing is Release, anything above it a service
    /// application; the axis has no detent for lap, fill or emergency, which is why the
    /// keys for those keep working (`ui::player_input`).
    BrakeValve,
    /// 0 … 1 of the direct (additional) brake.
    DirectBrake,
}

/// The lever rows of the controls page, in enum order — `Lever as usize` subscripts them
/// exactly as `Action as usize` does the buttons.
type LeverRow = (Lever, &'static str);
pub const LEVERS: &[LeverRow] = &[
    (Lever::Throttle, "lever-throttle"),
    (Lever::BrakeValve, "lever-brake-valve"),
    (Lever::DirectBrake, "lever-direct-brake"),
];

impl Lever {
    /// The name in the settings file; the message key is this with `ctl-` in front.
    pub fn name(self) -> &'static str {
        LEVERS.get(self as usize).map_or("", |row| row.1)
    }

    fn named(name: &str) -> Option<Lever> {
        LEVERS.iter().find(|row| row.1 == name).map(|row| row.0)
    }
}

/// What a row of the controls page binds — the two kinds of row in one type, so the page,
/// the capture and the menu state need only one of everything.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bindable {
    Button(Action),
    Lever(Lever),
}

/// The controller inputs a lever may be put on: the six standard axes, and the four
/// triggers, which Bevy keeps among the buttons but reads as an analogue value all the
/// same. `Other(u8)` is left out — a row that cannot be named cannot be stored.
fn candidates() -> impl Iterator<Item = GamepadInput> {
    const TRIGGERS: [GamepadButton; 4] = [
        GamepadButton::LeftTrigger,
        GamepadButton::LeftTrigger2,
        GamepadButton::RightTrigger,
        GamepadButton::RightTrigger2,
    ];
    GamepadAxis::all()
        .into_iter()
        .map(GamepadInput::Axis)
        .chain(TRIGGERS.into_iter().map(GamepadInput::Button))
}

/// What one action answers to. Either half may be empty — a control nobody uses can be
/// unbound entirely, and most of the desk has no controller button to begin with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Bind {
    pub key: Option<KeyCode>,
    pub pad: Option<GamepadButton>,
}

/// What everything answers to: one [`Bind`] per action, one controller input per lever,
/// each subscripted by its enum. Rebuilt from [`Bindings`] whenever that changes, and read
/// by [`Input`] every frame.
#[derive(Resource, Clone, PartialEq, Eq, Debug)]
pub struct Binds {
    buttons: Vec<Bind>,
    levers: Vec<Option<GamepadInput>>,
}

impl Default for Binds {
    fn default() -> Self {
        Self {
            buttons: rows()
                .map(|(_, _, key, pad)| Bind {
                    key: Some(*key),
                    pad: *pad,
                })
                .collect(),
            // Nothing: a bound lever writes its control every frame, so a default would
            // hold the brake valve at Release for anyone with a pad plugged in.
            levers: vec![None; LEVERS.len()],
        }
    }
}

impl Binds {
    pub fn get(&self, action: Action) -> Bind {
        self.buttons
            .get(action as usize)
            .copied()
            .unwrap_or_default()
    }

    /// Not `set` — `ResMut` carries one of its own, and a two-argument call through it
    /// would only ever be a puzzle.
    pub fn bind(&mut self, action: Action, bind: Bind) {
        if let Some(slot) = self.buttons.get_mut(action as usize) {
            *slot = bind;
        }
    }

    pub fn lever(&self, lever: Lever) -> Option<GamepadInput> {
        self.levers.get(lever as usize).copied().flatten()
    }

    pub fn bind_lever(&mut self, lever: Lever, input: Option<GamepadInput>) {
        if let Some(slot) = self.levers.get_mut(lever as usize) {
            *slot = input;
        }
    }
}

/// The bindings as the settings file keeps them.
///
/// A list of lines rather than a field per action: sixty fields would be sixty places to
/// forget, and the enum is the list already. Only what differs from the default is
/// written, so the group is empty on a fresh install and a new default reaches everyone
/// who never touched that row.
#[derive(Resource, SettingsGroup, Reflect, Clone, Debug, Default, PartialEq)]
#[reflect(Resource, SettingsGroup, Default)]
#[settings_group(group = "controls")]
pub struct Bindings {
    /// `name key pad` for a button row and `name input` for a lever —
    /// `throttle-up KeyW DPadUp`, `lever-brake-valve RightTrigger2`, with `-` where
    /// nothing is bound. The names are the ones Bevy's own enums carry.
    pub binds: Vec<String>,
}

impl Bindings {
    /// The rebound rows of `binds`, in the order the controls page lists them.
    pub fn of(binds: &Binds) -> Self {
        let default = Binds::default();
        let buttons = rows()
            .map(|row| row.0)
            .filter(|action| binds.get(*action) != default.get(*action))
            .map(|action| {
                let bind = binds.get(action);
                format!(
                    "{} {} {}",
                    action.name(),
                    bind.key.map_or_else(|| "-".into(), |k| name(&k)),
                    bind.pad.map_or_else(|| "-".into(), |p| name(&p)),
                )
            });
        let levers = LEVERS
            .iter()
            .map(|row| row.0)
            .filter(|lever| binds.lever(*lever).is_some())
            .map(|lever| {
                format!(
                    "{} {}",
                    lever.name(),
                    binds.lever(lever).map_or_else(|| "-".into(), input_name),
                )
            });
        Self {
            binds: buttons.chain(levers).collect(),
        }
    }

    /// The defaults with these lines applied. A line naming an action or a key that no
    /// longer exists is dropped rather than fought over — the row keeps its default.
    pub fn binds(&self) -> Binds {
        let mut binds = Binds::default();
        for line in &self.binds {
            let mut words = line.split_whitespace();
            let Some(row) = words.next() else { continue };
            if let Some(lever) = Lever::named(row) {
                binds.bind_lever(lever, words.next().and_then(parse_input));
            } else if let Some(action) = Action::named(row) {
                binds.bind(
                    action,
                    Bind {
                        key: words.next().and_then(parse),
                        pad: words.next().and_then(parse),
                    },
                );
            }
        }
        binds
    }
}

/// The name a key or a button is stored under. `Debug` and the variant name of the
/// reflected enum are the same string, which is what lets [`parse`] read it back —
/// `round_trip` is the test that keeps them the same.
fn name(value: &impl std::fmt::Debug) -> String {
    format!("{value:?}")
}

/// The key or button of that name. Bevy's input enums are reflected, so a unit variant
/// can be built from its name without a serde feature and without a table of our own.
fn parse<T: FromReflect>(name: &str) -> Option<T> {
    T::from_reflect(&DynamicEnum::new(name, ()))
}

/// A controller input under the flat name of whichever half of it it is: `LeftStickY`,
/// `RightTrigger2`. `GamepadInput`'s own `{:?}` wraps that in `Axis(…)`/`Button(…)`, which
/// is a tuple variant and therefore neither readable in the file nor buildable by name.
fn input_name(input: GamepadInput) -> String {
    match input {
        GamepadInput::Axis(axis) => name(&axis),
        GamepadInput::Button(button) => name(&button),
    }
}

/// The other way round. The two enums share no variant name, so the flat name is enough
/// to tell which of them it belongs to.
fn parse_input(name: &str) -> Option<GamepadInput> {
    parse::<GamepadAxis>(name)
        .map(GamepadInput::Axis)
        .or_else(|| parse::<GamepadButton>(name).map(GamepadInput::Button))
}

/// What a rebinding needs: the bindings in both of their shapes, and the controllers a
/// new one may come from. One parameter rather than three, because a Bevy system takes
/// sixteen and the menu already has fifteen of its own.
#[derive(SystemParam)]
pub struct Rebind<'w, 's> {
    pub binds: ResMut<'w, Binds>,
    pub bindings: ResMut<'w, Bindings>,
    pub pads: Query<'w, 's, &'static Gamepad>,
}

/// The keyboard and the controllers behind the bindings — what a driving system asks for
/// instead of `ButtonInput<KeyCode>`.
///
/// Every connected pad answers for every action: two people cannot drive one train, so
/// there is nothing to be gained by asking which of them pressed.
#[derive(SystemParam)]
pub struct Input<'w, 's> {
    binds: Res<'w, Binds>,
    keys: Res<'w, ButtonInput<KeyCode>>,
    pads: Query<'w, 's, &'static Gamepad>,
}

impl Input<'_, '_> {
    /// Held down — for the levers that move while a key is held.
    pub fn pressed(&self, action: Action) -> bool {
        let bind = self.binds.get(action);
        bind.key.is_some_and(|key| self.keys.pressed(key))
            || bind
                .pad
                .is_some_and(|button| self.pads.iter().any(|pad| pad.pressed(button)))
    }

    /// Pressed this frame — for the switches that flip once per press.
    pub fn just_pressed(&self, action: Action) -> bool {
        let bind = self.binds.get(action);
        bind.key.is_some_and(|key| self.keys.just_pressed(key))
            || bind
                .pad
                .is_some_and(|button| self.pads.iter().any(|pad| pad.just_pressed(button)))
    }

    /// Where a lever's axis stands, or `None` where nothing is bound to it. Half travel
    /// on the first pad that has the input — two people cannot drive one train.
    pub fn lever(&self, lever: Lever) -> Option<f32> {
        let input = self.binds.lever(lever)?;
        self.pads.iter().find_map(|pad| pad.get(input))
    }

    /// The right stick, for looking around: x to the right, y up. Not bindable.
    pub fn look(&self) -> Vec2 {
        self.pads
            .iter()
            .map(Gamepad::right_stick)
            .find(|s| *s != Vec2::ZERO)
            .unwrap_or(Vec2::ZERO)
    }

    /// The left stick, for walking: x to the right, y ahead. Not bindable.
    pub fn walk(&self) -> Vec2 {
        self.pads
            .iter()
            .map(Gamepad::left_stick)
            .find(|s| *s != Vec2::ZERO)
            .unwrap_or(Vec2::ZERO)
    }
}

/// The controller input that has just been moved, for a lever row waiting to be bound.
///
/// Half travel from rest, which every candidate has at zero — enough to tell a deliberate
/// push from a stick worn loose, and reached by any trigger pulled on purpose.
pub fn moved(pads: &Query<&Gamepad>) -> Option<GamepadInput> {
    candidates().find(|input| {
        pads.iter()
            .any(|pad| pad.get(*input).is_some_and(|v| v.abs() > 0.5))
    })
}

/// What a controller input is called on the controls page — the same flat name it is
/// stored under, which is already what the pad prints beside it.
pub fn input_label(input: GamepadInput) -> String {
    match input {
        GamepadInput::Button(button) => pad_label(button),
        GamepadInput::Axis(axis) => name(&axis),
    }
}

/// What a key is called on the controls page and in the key help: the prefix nobody says
/// out loud taken off, and the handful that are a symbol rather than a word written as
/// the symbol.
pub fn key_label(key: KeyCode) -> String {
    let symbol = match key {
        KeyCode::ArrowLeft => "←",
        KeyCode::ArrowRight => "→",
        KeyCode::ArrowUp => "↑",
        KeyCode::ArrowDown => "↓",
        KeyCode::Backquote => "^",
        KeyCode::Period => ".",
        KeyCode::Comma => ",",
        KeyCode::Minus => "−",
        KeyCode::Equal => "=",
        KeyCode::NumpadAdd => "Num +",
        KeyCode::NumpadSubtract => "Num −",
        KeyCode::PageUp => "PgUp",
        KeyCode::PageDown => "PgDn",
        KeyCode::Delete => "Del",
        KeyCode::Escape => "Esc",
        KeyCode::Insert => "Ins",
        KeyCode::ShiftLeft => "Shift",
        KeyCode::ShiftRight => "Shift R",
        KeyCode::ControlLeft => "Ctrl",
        KeyCode::ControlRight => "Ctrl R",
        KeyCode::AltLeft => "Alt",
        KeyCode::AltRight => "Alt Gr",
        _ => "",
    };
    if !symbol.is_empty() {
        return symbol.to_string();
    }
    let name = name(&key);
    name.strip_prefix("Key")
        .or_else(|| name.strip_prefix("Digit"))
        .unwrap_or(&name)
        .to_string()
}

/// What a controller button is called. Bevy names the four of the action pad by compass
/// point, which is precise and tells a player nothing — they carry the letters printed on
/// them instead. The rest already say what they are.
///
/// ponytail: the Xbox letters, not the PlayStation shapes. Fira Mono has no ✕ ○ △ □, and a
/// second font for four glyphs is not worth what it costs.
pub fn pad_label(button: GamepadButton) -> String {
    match button {
        GamepadButton::South => "A".to_string(),
        GamepadButton::East => "B".to_string(),
        GamepadButton::North => "Y".to_string(),
        GamepadButton::West => "X".to_string(),
        other => name(&other),
    }
}

/// Keeps [`Binds`] in step with what the settings file holds.
///
/// The group itself is registered in `settings::plugin`, which has to happen before
/// `SettingsPlugin` is added — this one only resolves what that loaded.
pub fn plugin(app: &mut App) {
    app.init_resource::<Binds>()
        .add_systems(Update, resolve.run_if(resource_changed::<Bindings>));
}

/// The stored lines into the array the frame subscripts.
fn resolve(bindings: Res<Bindings>, mut binds: ResMut<Binds>) {
    let resolved = bindings.binds();
    // Written through the guard: the key help redraws on a changed `Binds`, and a
    // settings save that changed nothing must not make it flicker.
    binds.set_if_neq(resolved);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Action as usize` subscripts [`Binds`], so the enum and the table have to read in
    /// the same order — every lookup in the simulator depends on it.
    #[test]
    fn actions_are_in_enum_order() {
        for (index, row) in rows().enumerate() {
            assert_eq!(row.0 as usize, index, "{} is out of order", row.1);
        }
        assert_eq!(Binds::default().buttons.len(), rows().count());
        for (index, row) in LEVERS.iter().enumerate() {
            assert_eq!(row.0 as usize, index, "{} is out of order", row.1);
        }
    }

    /// Two rows sharing a name would overwrite each other in the settings file.
    #[test]
    fn names_are_unique() {
        let mut names: Vec<&str> = rows().map(|row| row.1).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count);
    }

    /// The file keeps `{:?}` names and reads them back by reflection — if the two ever
    /// drift apart, every stored binding silently falls back to its default.
    #[test]
    fn round_trip() {
        for (_, _, key, pad) in rows() {
            assert_eq!(parse::<KeyCode>(&name(key)), Some(*key));
            if let Some(pad) = pad {
                assert_eq!(parse::<GamepadButton>(&name(pad)), Some(*pad));
            }
        }
        assert_eq!(parse::<KeyCode>("NoSuchKey"), None);
    }

    /// A lever's axis goes through the same file as a key, under a name of its own, and
    /// every input a lever may be put on survives the round trip.
    #[test]
    fn a_lever_keeps_its_axis_through_the_file() {
        for input in candidates() {
            assert_eq!(parse_input(&input_name(input)), Some(input));
        }

        let mut binds = Binds::default();
        assert!(
            Bindings::of(&binds).binds.is_empty(),
            "no lever is bound out of the box — a bound one writes its lever every frame"
        );

        binds.bind_lever(
            Lever::BrakeValve,
            Some(GamepadInput::Button(GamepadButton::RightTrigger2)),
        );
        binds.bind_lever(
            Lever::Throttle,
            Some(GamepadInput::Axis(GamepadAxis::LeftStickY)),
        );
        let stored = Bindings::of(&binds);
        assert_eq!(
            stored.binds,
            [
                "lever-throttle LeftStickY",
                "lever-brake-valve RightTrigger2"
            ]
        );
        assert_eq!(stored.binds(), binds);
    }

    /// Only what was changed is written, and what is written comes back the same.
    #[test]
    fn only_the_changed_rows_are_stored() {
        let mut binds = Binds::default();
        assert!(Bindings::of(&binds).binds.is_empty());

        binds.bind(
            Action::Horn,
            Bind {
                key: Some(KeyCode::KeyO),
                pad: None,
            },
        );
        // Unbound entirely — the pair of `None`s has to survive the file as well.
        binds.bind(Action::Sanding, Bind::default());
        let stored = Bindings::of(&binds);
        // In the order the page reads, which is the order of the groups: sanding before horn.
        assert_eq!(stored.binds, ["sanding - -", "horn KeyO -"]);
        assert_eq!(stored.binds(), binds);
    }

    /// A settings file from an older or newer build names actions and keys this one does
    /// not have. Those rows keep their default instead of taking the whole file down.
    #[test]
    fn unknown_rows_are_ignored() {
        let bindings = Bindings {
            binds: vec![
                "no-such-action KeyO -".into(),
                "horn NoSuchKey -".into(),
                "sanding KeyO -".into(),
            ],
        };
        let binds = bindings.binds();
        assert_eq!(
            binds.get(Action::Horn).key,
            None,
            "an unreadable key unbinds"
        );
        assert_eq!(binds.get(Action::Sanding).key, Some(KeyCode::KeyO));
        assert_eq!(
            binds.get(Action::ThrottleUp),
            Binds::default().get(Action::ThrottleUp)
        );
    }
}
