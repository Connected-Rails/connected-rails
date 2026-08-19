//! The head-up display of the running simulator (plan 16.3).
//!
//! # What it is for
//!
//! The driver's desk is the instrument — the HUD only says what a driver could read off
//! it without leaning forward, plus the two things no desk shows: what the run is
//! supposed to do, and what the line ahead is about to ask for. Everything else the
//! simulation knows is diagnostics and lives on F6, where it does not sit between the
//! player and the track.
//!
//! # Two materials, not five cards
//!
//! Everything on screen is either **hardware** or **overlay**, and the two never look
//! alike. Hardware is the instrument panel at the bottom and the lamp housing beside it:
//! a lighter surface, a lit top edge, a shadow underneath, round instruments with needles
//! that turn. Overlay is the run and the systems at the top: type on a wash that fades
//! into the picture, with no frame at all. A screen of identically rounded, identically
//! weighted panels is what a generated layout looks like; a cab does not have five of the
//! same box, it has instruments where you read and labels where you glance.
//!
//! # The zones
//!
//! Read in the order the eye moves. Bottom centre is the **desk**: the speedometer, the
//! Doppelmanometer for brake pipe and main reservoir, the brake cylinder gauge, and
//! beside them what the levers are doing. Bottom left is the **train protection**, where
//! the lamps sit in a German cab. Bottom right is the **look-ahead**, signed the way the
//! line signs it — an Lf 7 board for a restriction, the disc of Hp 0 for a stop. Top left
//! is the **run**, top right the **systems**, and the top centre stays free for the only
//! text that may interrupt: scenario messages, and the banner saying the protection has
//! taken over.
//!
//! # How it is built
//!
//! Every mark is drawn by `glyphs` when the run starts — dial faces, needles, the ten
//! pictograms of the annunciators — so there is no asset directory and no icon set whose
//! idea of a pantograph we would have to live with.
//!
//! The tree is spawned once and refilled in place. A figure carries [`Readout`], a bar
//! [`Gauge`], a pointer [`Needle`], an indicator lamp [`Lamp`], an annunciator [`Chip`],
//! and a part that does not apply to every vehicle [`Block`]; one loop per kind fills them
//! from [`Frame`], which reads the simulation once. Nothing is respawned per frame.
//!
//! What is *shown* follows the vehicle: the AFB row on a vehicle fitted with one, the LZB
//! lamps where an LZB is, the look-ahead when something is coming. A block with nothing
//! to say collapses instead of printing zeroes.
//!
//! Every node is [`Pickable::IGNORE`]: the cab's controls are picked in 3D through the
//! display, and an overlay that swallowed clicks would put the desk out of reach.
//!
//! Colour follows `theme` — monochrome, red for danger, amber for attention. The
//! instruments are the documented exception, and the reason the rule exists: 1000 Hz has
//! to be amber, 500 Hz red, and on the Doppelmanometer the red needle is the main
//! reservoir and the pale one the brake pipe, because that is what the needles mean in a
//! cab. Colour that carries a meaning the driver already knows is not decoration.

use bevy::ecs::system::SystemParam;
use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::ui::widget::NodeImageMode;
use bevy::ui::{BackgroundGradient, ColorStop, LinearGradient};
use i18n::{decimal, t};
use sim_core::brakes::DriverBrakeValve;
use sim_core::drive::TractionSpec;
use sim_core::safety::{Indicator, LampState, ProtectionAction, SafetySystems, SelfTestPhase};
use sim_core::scenario::Message;
use sim_core::timetable::{DAY, ScheduledStop};
use sim_core::train::{Train, Vehicle};
use sim_core::{Sim, TrainRuntime};

use crate::cab::CabMouse;
use crate::glyphs::{self, Icon};
use crate::settings::{Gameplay, HudMode};
use crate::streaming::TerrainStreamer;
use crate::theme::{
    ACCENT, BRAND, CHIP, Face, Fonts, TEXT, TEXT_BRIGHT, TEXT_DIM, TEXT_FAINT, TEXT_MID, TRACK,
    WARN, text,
};
use crate::{PlayerTrain, SimResource, TerrainInfo, ViewDistance};

// ---------------------------------------------------------------------------------
// Surfaces and metrics
// ---------------------------------------------------------------------------------

/// The face of an instrument: darker than the panel it is let into, so a dial reads as a
/// hole in the hardware rather than a circle painted on it.
const FACE: Color = Color::srgba(0.031, 0.033, 0.039, 0.94);
/// The instrument panel the dials are let into.
const PANEL: Color = Color::srgba(0.106, 0.110, 0.125, 0.94);
/// The lit top edge of a panel — one hairline, which is what gives it a thickness.
const PANEL_LIP: Color = Color::srgba(0.66, 0.68, 0.74, 0.38);
/// The wash the two overlay zones sit on, at its darkest.
const WASH: Color = Color::srgba(0.024, 0.024, 0.031, 0.88);
/// The rail of the timetable ribbon, and the hairline over its footer.
const RAIL: Color = Color::srgba(0.48, 0.49, 0.55, 0.60);
/// The well an unlit annunciator or lamp sits in.
const WELL: Color = Color::srgba(0.129, 0.133, 0.149, 0.85);
/// An unlit lamp glass. Dark, but not the panel — glass is never quite black.
const GLASS_DARK: Color = Color::srgba(0.180, 0.184, 0.196, 0.9);
/// Rules inside a panel.
const EDGE: Color = Color::srgba(0.353, 0.353, 0.400, 0.32);

/// Distance from the edge of the screen, and the gap between two zones.
const MARGIN: f32 = 22.0;
const GAP: f32 = 12.0;
/// The corner the hardware is cut with. Small — a machined edge, not a card.
const RADIUS: f32 = 5.0;

/// The timetable ribbon: how many stops it holds, the height of one line, and where the
/// rail runs in it [px].
const STOP_ROWS: usize = 4;
const ROW: f32 = 28.0;
const RAIL_WIDTH: f32 = 17.0;

/// Diameter of the three instruments [px]. The speedometer is read at a glance and the
/// other two are checked, which is the whole reason they differ in size.
const SPEEDO: f32 = 172.0;
const MANOMETER: f32 = 104.0;

/// Full scale of the air gauges [bar]: brake pipe and main reservoir share the
/// Doppelmanometer, the brake cylinder has its own and a shorter scale.
const MANOMETER_MAX: f64 = 10.0;
const CYLINDER_MAX: f64 = 6.0;
/// Below this the main reservoir is low enough to be worth saying so [bar].
const LOW_RESERVOIR: f64 = 6.5;
/// Traction motor temperature from which the heat annunciator lights [°C].
const HOT_MOTOR: f64 = 140.0;

/// How many scenario messages stand at once, and for how long [s].
const MESSAGES: usize = 3;
const MESSAGE_LIFE: f64 = 24.0;

/// How far the look-ahead reads down the line [m].
const LOOKAHEAD: f64 = 4000.0;

/// Blinks per second of a flashing indicator lamp: the rate of a German cab, slow enough
/// to read the legend between two flashes.
const BLINK_HZ: f64 = 1.4;

// ---------------------------------------------------------------------------------
// The tree's components
// ---------------------------------------------------------------------------------

/// Root of the whole display.
#[derive(Component)]
pub struct Hud;

/// The drawn instruments and pictograms, and the scale the speedometer was drawn for. Built once when the
/// run starts — the dial face depends on the vehicle's maximum speed, so it cannot be
/// made before there is a vehicle.
#[derive(Resource)]
pub struct Drawings {
    /// Full scale of the speedometer [km/h], rounded up to a mark.
    pub speed_scale: f64,
    speedo_face: Handle<Image>,
    air_face: Handle<Image>,
    cylinder_face: Handle<Image>,
    needle: Handle<Image>,
    fine_needle: Handle<Image>,
    marker: Handle<Image>,
    lamp: Handle<Image>,
    board: Handle<Image>,
    disc: Handle<Image>,
    wedge: Handle<Image>,
    icons: Vec<Handle<Image>>,
}

impl Drawings {
    /// Draws everything the HUD is made of. `v_max` is the running-gear limit of the
    /// vehicle the driver sits in.
    pub fn draw(images: &mut Assets<Image>, v_max: f64) -> Self {
        // A speedometer has a scale, not a range that follows the line: round up to the
        // next mark so the last figure on the face is a round one.
        let speed_scale = ((v_max.max(40.0) / 20.0).ceil() * 20.0).min(400.0);
        let majors = (speed_scale / 20.0) as u32;
        Self {
            speed_scale,
            speedo_face: images.add(glyphs::dial_face(360, majors, 2)),
            air_face: images.add(glyphs::dial_face(224, MANOMETER_MAX as u32, 2)),
            cylinder_face: images.add(glyphs::dial_face(224, CYLINDER_MAX as u32, 2)),
            needle: images.add(glyphs::needle(360, 0.020, 0.345)),
            fine_needle: images.add(glyphs::needle(224, 0.016, 0.330)),
            marker: images.add(glyphs::marker(360)),
            lamp: images.add(glyphs::lamp_glass(64)),
            board: images.add(glyphs::speed_board(128)),
            disc: images.add(glyphs::stop_disc(128)),
            wedge: images.add(glyphs::here(48)),
            icons: Icon::ALL
                .into_iter()
                .map(|i| images.add(glyphs::icon(i, 44)))
                .collect(),
        }
    }
}

/// A text node refilled every frame.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum Readout {
    // The run, top left.
    Clock,
    Delay,
    Service,
    /// One line of the timetable ribbon, by row: 0 is the stop behind, 1 the next one,
    /// 2 and 3 the ones after it.
    StopName(usize),
    StopTime(usize),
    StopPlatform(usize),
    /// How far the next stop still is — written beside the wedge, which is the one place
    /// on the ribbon where "from here to there" is what the reader is looking at.
    LegDistance,
    Score,
    // Systems, top right — three rows the drive labels itself.
    DriveLabel(usize),
    Drive(usize),
    // The desk, bottom centre.
    Speed,
    SpeedLimit,
    Supervision,
    Power,
    BrakeValve,
    Effort,
    Pipe,
    Reservoir,
    Cylinder,
    Reverser,
    Afb,
    Odometer,
    // Train protection, bottom left.
    Protection,
    PzbNote,
    CategoryLamp,
    LzbPermitted,
    LzbTarget,
    LzbDistance,
    // Look-ahead, bottom right.
    AheadSpeed,
    AheadDistance,
    // The interruptions.
    Alert,
    Message(usize),
    Hover,
    // F6.
    Diagnostics,
}

/// A bar that grows.
#[derive(Component, Clone, Copy)]
pub struct Gauge(Meter);

/// A pointer that turns: the needle of an instrument, or a marker at its rim.
#[derive(Component, Clone, Copy)]
pub struct Needle(Meter);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Meter {
    Speed,
    Limit,
    Supervision,
    Power,
    Pipe,
    Reservoir,
    Cylinder,
    Ahead,
}

/// An indicator lamp of the train protection: a glass and a legend under it, both of
/// which take the lamp's own colour when it lights. `name` is what
/// [`SafetySystems::indicators`] reports it under; the empty name is the PZB's train
/// category lamp, whose legend *is* its name and changes with the category.
#[derive(Component, Clone, Copy)]
pub struct Lamp {
    name: &'static str,
    tone: Color,
}

/// An annunciator of the driver's desk, drawn as its pictogram. Unlike a lamp it is not a
/// signal, so it stays bone white — amber is kept for the ones where being lit is itself
/// the news.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum Chip {
    Battery,
    Pantograph,
    MainSwitch,
    Compressor,
    Parking,
    Sanding,
    Doors,
    Lights,
    Slip,
    Heat,
}

impl Chip {
    /// The annunciators in the order they stand on the panel, with the drawing and the
    /// name each one carries in the key sheet.
    const ALL: [(Chip, Icon, &'static str); 10] = [
        (Chip::Battery, Icon::Battery, "hud-chip-battery"),
        (Chip::Pantograph, Icon::Pantograph, "hud-chip-pantograph"),
        (Chip::MainSwitch, Icon::MainSwitch, "hud-chip-main-switch"),
        (Chip::Compressor, Icon::Compressor, "hud-chip-compressor"),
        (Chip::Parking, Icon::Parking, "hud-chip-parking"),
        (Chip::Sanding, Icon::Sanding, "hud-chip-sanding"),
        (Chip::Doors, Icon::Doors, "hud-chip-doors"),
        (Chip::Lights, Icon::Lights, "hud-chip-lights"),
        (Chip::Slip, Icon::Slip, "hud-chip-slip"),
        (Chip::Heat, Icon::Heat, "hud-chip-hot"),
    ];
}

/// A part of the tree that not every vehicle, or not every moment, has anything to say in.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum Block {
    /// The two overlay zones and the wash they share — everything the reduced display
    /// leaves out, because it informs rather than instruments.
    Journey,
    Systems,
    TopWash,
    /// The timetable ribbon, its four rows, and the wedge that marks where the train is.
    Ribbon,
    StopRow(usize),
    Wedge,
    Score,
    DriveRow(usize),
    Afb,
    /// The protection housing, its LZB lamp row, and the MFA figures under them.
    Safety,
    Lzb,
    LzbValues,
    /// The marker for a supervised speed only exists while something supervises one.
    Supervision,
    /// The look-ahead, and which of the two signs it shows.
    Ahead,
    AheadBoard,
    AheadStop,
    /// The banner, the message column, the name of the hovered control.
    Alert,
    Message(usize),
    Hover,
    /// The two overlays.
    Help,
    Diagnostics,
}

/// The step `--hud <step>` asked for. It is kept apart from the setting on purpose: the
/// settings file is written on exit whether anything was changed or not, so a run that
/// overrode the setting would leave the override behind in the player's preferences.
#[derive(Resource, Clone, Copy)]
pub struct HudOverride(pub HudMode);

/// The step in force: what the command line asked for, otherwise what the player set.
fn mode(gameplay: &Gameplay, over: Option<&HudOverride>) -> HudMode {
    over.map(|o| o.0).unwrap_or(gameplay.hud)
}

/// Which of the two overlays are open. Both are shut by default: a HUD that opens with a
/// wall of key bindings is a manual, and the game is not one.
#[derive(Resource, Default)]
pub struct Overlays {
    pub help: bool,
    pub diagnostics: bool,
}

// ---------------------------------------------------------------------------------
// The key help (F5)
// ---------------------------------------------------------------------------------

/// The keyboard, by group. Full operability from the keyboard is the principle the input
/// follows (`ui::player_input`), so this table is where the desk says what it can do. It
/// is written out rather than derived because the order it reads in is a driver's order,
/// not the order the code happens to poll the keys in.
const HELP: [(&str, &[(&str, &str)]); 5] = [
    (
        "hud-help-driving",
        &[
            ("W / S", "hud-key-throttle"),
            ("X", "hud-key-throttle-off"),
            ("R / T / F", "hud-key-reverser"),
            ("^", "hud-key-range"),
            ("6", "hud-key-afb"),
            ("7 / 8", "hud-key-afb-target"),
        ],
    ),
    (
        "hud-help-brakes",
        &[
            ("A / D", "hud-key-brake"),
            ("Q", "hud-key-lap"),
            ("Z", "hud-key-fill"),
            ("E", "hud-key-emergency"),
            ("C / V", "hud-key-direct"),
            ("L", "hud-key-release"),
            ("P", "hud-key-parking"),
            ("O", "hud-key-ep"),
            ("G", "hud-key-sand"),
        ],
    ),
    (
        "hud-help-safety",
        &[
            ("Space", "hud-key-sifa"),
            ("PgDn", "hud-key-acknowledge"),
            ("End", "hud-key-free"),
            ("Del", "hud-key-override"),
            ("N / M / B", "hud-key-lzb"),
            ("U", "hud-key-train-type"),
            ("H", "hud-key-horn"),
        ],
    ),
    (
        "hud-help-vehicle",
        &[
            ("1 – 4", "hud-key-prepare"),
            ("5", "hud-key-starter"),
            ("9 / 0", "hud-key-lamps"),
            (", / .", "hud-key-dimmer"),
            ("Y", "hud-key-wipers"),
            ("J / K / I", "hud-key-doors"),
        ],
    ),
    (
        "hud-help-view",
        &[
            ("F1 – F4", "hud-key-cameras"),
            ("← → ↑ ↓", "hud-key-look"),
            ("Num + −", "hud-key-zoom"),
            ("WASD", "hud-key-walk"),
            ("F5 / F6", "hud-key-overlays"),
            ("F7", "hud-key-hud"),
            ("F9", "hud-key-mods"),
            ("Esc", "hud-key-pause"),
        ],
    ),
];

// ---------------------------------------------------------------------------------
// Building the tree
// ---------------------------------------------------------------------------------

/// Every node of the HUD hangs off its parent and is transparent to the mouse — the two
/// things true of all of them, in one place.
fn child(commands: &mut Commands, parent: Entity, bundle: impl Bundle) -> Entity {
    commands
        .spawn((bundle, ChildOf(parent), Pickable::IGNORE))
        .id()
}

/// A node stretched over its whole parent — every layer of an instrument is one of these.
fn cover() -> Node {
    Node {
        position_type: PositionType::Absolute,
        top: Val::Px(0.0),
        left: Val::Px(0.0),
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        ..default()
    }
}

/// A picture, stretched to whatever node it is given.
fn picture(image: &Handle<Image>, tone: Color) -> ImageNode {
    ImageNode {
        image: image.clone(),
        color: tone,
        image_mode: NodeImageMode::Stretch,
        ..default()
    }
}

/// The shape a hardware panel has unless the caller says otherwise. Spread over rather
/// than defaulted inside [`hardware`], so what the caller writes always wins.
fn panel_node() -> Node {
    Node {
        flex_direction: FlexDirection::Column,
        padding: UiRect::all(Val::Px(12.0)),
        row_gap: Val::Px(7.0),
        ..default()
    }
}

/// A piece of hardware: the instrument panel, the lamp housing, the sign at the rim of
/// the line. A lit top edge and a shadow underneath are what give it a thickness — that,
/// and the fact that nothing else on the screen has them.
fn hardware(commands: &mut Commands, parent: Entity, node: Node) -> Entity {
    let panel = child(
        commands,
        parent,
        (
            Node {
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..node
            },
            BackgroundColor(PANEL),
            BoxShadow::from(ShadowStyle {
                color: Color::srgba(0.0, 0.0, 0.0, 0.55),
                x_offset: Val::ZERO,
                y_offset: Val::Px(3.0),
                spread_radius: Val::Px(1.0),
                blur_radius: Val::Px(14.0),
            }),
        ),
    );
    child(
        commands,
        panel,
        (
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(RADIUS),
                right: Val::Px(RADIUS),
                height: Val::Px(1.0),
                ..default()
            },
            BackgroundColor(PANEL_LIP),
        ),
    );
    panel
}

/// An overlay zone: type on the world and nothing else. No surface, no border, no
/// corner — the run and the systems are read, not operated, and a box around them would
/// put them on the same footing as the instruments. What makes them legible over a bright
/// sky is the shadow every figure carries ([`label`]), not a panel behind them.
fn overlay(commands: &mut Commands, parent: Entity, node: Node) -> Entity {
    child(
        commands,
        parent,
        Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Px(MARGIN)),
            row_gap: Val::Px(5.0),
            ..node
        },
    )
}

/// Type on the HUD is read over whatever the world is doing behind it, so all of it
/// carries a shadow. On the dark hardware the shadow is invisible; over a bright sky it is
/// what keeps a thin figure legible — and it is what lets the two overlay zones do without
/// a surface of their own entirely.
fn label(fonts: &Fonts, content: String, face: Face, size: f32, color: Color) -> impl Bundle {
    (
        text(fonts, content, face, size, color),
        TextShadow {
            offset: Vec2::new(0.0, 1.0),
            color: Color::srgba(0.0, 0.0, 0.0, 0.80),
        },
    )
}

/// The heading of a group: a small caps label, a rule filling the rest of the line, and
/// optionally a readout at the far end.
fn heading(
    commands: &mut Commands,
    fonts: &Fonts,
    parent: Entity,
    key: &str,
    trailing: Option<Readout>,
) {
    let row = child(
        commands,
        parent,
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Px(8.0),
            ..default()
        },
    );
    child(
        commands,
        row,
        label(
            fonts,
            t!(key).to_uppercase(),
            Face::Semibold,
            10.0,
            TEXT_MID,
        ),
    );
    child(
        commands,
        row,
        (
            Node {
                flex_grow: 1.0,
                height: Val::Px(1.0),
                ..default()
            },
            // The rule has to carry over a bright sky as well as over the hardware, so it
            // is the ribbon rail rather than the fainter edge of a panel.
            BackgroundColor(RAIL),
        ),
    );
    if let Some(readout) = trailing {
        child(
            commands,
            row,
            (
                label(fonts, String::new(), Face::Mono, 10.0, TEXT_MID),
                readout,
            ),
        );
    }
}

/// A labelled figure: the name on the left in the proportional face, the value on the
/// right in the fixed one.
fn row(
    commands: &mut Commands,
    fonts: &Fonts,
    parent: Entity,
    name: &str,
    readout: Readout,
    size: f32,
) -> Entity {
    let row = child(
        commands,
        parent,
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            column_gap: Val::Px(12.0),
            ..default()
        },
    );
    child(
        commands,
        row,
        label(fonts, t!(name), Face::Sans, size - 2.0, TEXT_MID),
    );
    child(
        commands,
        row,
        (
            label(fonts, String::new(), Face::Mono, size, TEXT_BRIGHT),
            readout,
        ),
    );
    row
}

/// The same, but with the label filled in every frame — the drive rows, where what is
/// measured depends on what is under the floor.
fn drive_row(commands: &mut Commands, fonts: &Fonts, parent: Entity, index: usize) {
    let row = child(
        commands,
        parent,
        (
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                column_gap: Val::Px(12.0),
                ..default()
            },
            Block::DriveRow(index),
        ),
    );
    child(
        commands,
        row,
        (
            label(fonts, String::new(), Face::Sans, 11.0, TEXT_MID),
            Readout::DriveLabel(index),
        ),
    );
    child(
        commands,
        row,
        (
            label(fonts, String::new(), Face::Mono, 12.0, TEXT_BRIGHT),
            Readout::Drive(index),
        ),
    );
}

// ---------------------------------------------------------------------------------
// The instruments
// ---------------------------------------------------------------------------------

/// A round instrument, let into the panel: the sunken face, the drawn scale, the figures
/// around it. The pointers are hung on afterwards, so one dial can carry two needles.
fn dial(
    commands: &mut Commands,
    fonts: &Fonts,
    parent: Entity,
    face: &Handle<Image>,
    diameter: f32,
    labels: &[(f32, String)],
    label_size: f32,
) -> Entity {
    let dial = child(
        commands,
        parent,
        (
            Node {
                width: Val::Px(diameter),
                height: Val::Px(diameter),
                flex_shrink: 0.0,
                border_radius: BorderRadius::all(Val::Percent(50.0)),
                ..default()
            },
            BackgroundColor(FACE),
            // The instrument sits *in* the panel: the shadow falls inwards from the rim,
            // which is a spread of nothing and a blur of something.
            BoxShadow::from(ShadowStyle {
                color: Color::srgba(0.0, 0.0, 0.0, 0.45),
                x_offset: Val::ZERO,
                y_offset: Val::Px(1.0),
                spread_radius: Val::Px(-2.0),
                blur_radius: Val::Px(6.0),
            }),
        ),
    );
    child(commands, dial, (cover(), picture(face, TEXT_DIM)));
    // The figures of the scale, each in a box of its own so the digits stay centred on
    // the mark rather than growing away from it.
    let radius = 0.305;
    for (fraction, figure) in labels {
        let at = glyphs::dial_point(*fraction, radius);
        let cell = child(
            commands,
            dial,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(at.x * 100.0),
                top: Val::Percent(at.y * 100.0),
                width: Val::Px(30.0),
                height: Val::Px(14.0),
                margin: UiRect::new(Val::Px(-15.0), Val::ZERO, Val::Px(-7.0), Val::ZERO),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
        );
        child(
            commands,
            cell,
            label(fonts, figure.clone(), Face::Mono, label_size, TEXT_MID),
        );
    }
    dial
}

/// A pointer on a dial. The drawing points at twelve o'clock and the whole square is
/// rotated, so the spindle is the middle of the instrument by construction.
fn pointer(
    commands: &mut Commands,
    dial: Entity,
    image: &Handle<Image>,
    meter: Meter,
    tone: Color,
) -> Entity {
    child(
        commands,
        dial,
        (
            cover(),
            picture(image, tone),
            UiTransform::default(),
            Needle(meter),
        ),
    )
}

/// The spindle the needles turn on, and the digital reading under it.
fn hub(commands: &mut Commands, size: f32, dial: Entity) {
    child(
        commands,
        dial,
        (
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(50.0),
                top: Val::Percent(50.0),
                width: Val::Px(size),
                height: Val::Px(size),
                margin: UiRect::new(
                    Val::Px(-size / 2.0),
                    Val::ZERO,
                    Val::Px(-size / 2.0),
                    Val::ZERO,
                ),
                border_radius: BorderRadius::all(Val::Percent(50.0)),
                ..default()
            },
            BackgroundColor(TEXT_MID),
        ),
    );
}

/// A figure written on the face of an instrument, `drop` of the diameter below its middle.
fn on_face(
    commands: &mut Commands,
    fonts: &Fonts,
    dial: Entity,
    readout: Readout,
    size: f32,
    drop: f32,
) {
    let cell = child(
        commands,
        dial,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Percent(drop * 100.0),
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
    );
    child(
        commands,
        cell,
        (
            label(fonts, String::new(), Face::Mono, size, TEXT_BRIGHT),
            readout,
        ),
    );
}

/// A bar on a track — what is left of the bars now that the pressures are on dials: the
/// power controller, and how close the look-ahead's restriction has come.
fn bar(commands: &mut Commands, parent: Entity, meter: Meter, height: f32) {
    let track = child(
        commands,
        parent,
        (
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(height),
                border_radius: BorderRadius::all(Val::Px(height / 2.0)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(TRACK),
        ),
    );
    child(
        commands,
        track,
        (
            Node {
                width: Val::Percent(0.0),
                height: Val::Percent(100.0),
                border_radius: BorderRadius::all(Val::Px(height / 2.0)),
                ..default()
            },
            BackgroundColor(ACCENT),
            Gauge(meter),
        ),
    );
}

/// An annunciator: the pictogram on a dark well, lit when the thing is on.
fn annunciator(
    commands: &mut Commands,
    drawings: &Drawings,
    parent: Entity,
    kind: Chip,
    icon: Icon,
) {
    let well = child(
        commands,
        parent,
        (
            Node {
                width: Val::Px(34.0),
                height: Val::Px(34.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
            BackgroundColor(WELL),
        ),
    );
    child(
        commands,
        well,
        (
            Node {
                width: Val::Px(23.0),
                height: Val::Px(23.0),
                ..default()
            },
            picture(&drawings.icons[icon as usize], TEXT_FAINT),
            kind,
        ),
    );
}

/// An indicator lamp: round glass with its legend under it, both lighting together in
/// the lamp's own colour. A legend that lights with the glass is what tells a driver
/// which lamp came on without reading any of the others.
fn lamp(
    commands: &mut Commands,
    fonts: &Fonts,
    drawings: &Drawings,
    parent: Entity,
    name: &'static str,
    tone: Color,
    legend: &str,
) {
    let cell = child(
        commands,
        parent,
        (
            Node {
                width: Val::Px(46.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(3.0),
                ..default()
            },
            Lamp { name, tone },
        ),
    );
    child(
        commands,
        cell,
        (
            Node {
                width: Val::Px(21.0),
                height: Val::Px(21.0),
                ..default()
            },
            picture(&drawings.lamp, GLASS_DARK),
        ),
    );
    let label = child(
        commands,
        cell,
        label(fonts, legend.to_string(), Face::Semibold, 9.0, TEXT_FAINT),
    );
    // The category lamp's legend is the category itself, so it is a readout like any other.
    if name.is_empty() {
        commands.entity(label).insert(Readout::CategoryLamp);
    }
}

// ---------------------------------------------------------------------------------
// The screen
// ---------------------------------------------------------------------------------

/// Builds the whole display. Called once, when the run starts.
pub fn spawn_hud(commands: &mut Commands, fonts: &Fonts, drawings: &Drawings) {
    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::SpaceBetween,
                ..default()
            },
            Visibility::Inherited,
            Pickable::IGNORE,
            Hud,
        ))
        .id();

    build_top(commands, fonts, drawings, root);
    build_bottom(commands, fonts, drawings, root);
    // The diagnostics first: the key sheet is a scrim over the whole display, and a panel
    // spawned after it would sit on top of the scrim rather than under it.
    build_diagnostics(commands, fonts, root);
    build_help(commands, fonts, drawings, root);
}

/// The run, the messages and the systems — the band across the top.
fn build_top(commands: &mut Commands, fonts: &Fonts, drawings: &Drawings, root: Entity) {
    let top = child(
        commands,
        root,
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Start,
            justify_content: JustifyContent::SpaceBetween,
            ..default()
        },
    );
    // One wash across the whole width, dark at the top edge of the screen and gone a
    // couple of hundred pixels down. It is what makes grey type legible against a bright
    // sky without putting a box around either zone — the only edge it has is the edge of
    // the screen, which is no edge at all.
    child(
        commands,
        top,
        (
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Px(230.0),
                ..default()
            },
            Block::TopWash,
            BackgroundGradient::from(LinearGradient::to_bottom(vec![
                ColorStop::percent(WASH, 0.0),
                ColorStop::percent(WASH.with_alpha(0.60), 50.0),
                ColorStop::percent(WASH.with_alpha(0.0), 100.0),
            ])),
        ),
    );

    // --- The run, as the line diagram of its own timetable.
    let journey = overlay(
        commands,
        top,
        Node {
            width: Val::Px(342.0),
            flex_shrink: 0.0,
            ..default()
        },
    );
    commands.entity(journey).insert(Block::Journey);
    build_journey(commands, fonts, drawings, journey);

    // --- The messages: the middle column, empty most of the time.
    let messages = child(
        commands,
        top,
        Node {
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            padding: UiRect::top(Val::Px(MARGIN)),
            row_gap: Val::Px(4.0),
            ..default()
        },
    );
    for index in 0..MESSAGES {
        let line = child(
            commands,
            messages,
            (
                Node {
                    max_width: Val::Px(560.0),
                    padding: UiRect::axes(Val::Px(13.0), Val::Px(5.0)),
                    border_radius: BorderRadius::all(Val::Px(RADIUS)),
                    ..default()
                },
                BackgroundColor(WASH),
                Block::Message(index),
            ),
        );
        child(
            commands,
            line,
            (
                label(fonts, String::new(), Face::Sans, 13.0, TEXT),
                Readout::Message(index),
            ),
        );
    }

    // --- The systems.
    let systems = overlay(
        commands,
        top,
        Node {
            width: Val::Px(300.0),
            flex_shrink: 0.0,
            align_items: AlignItems::End,
            ..default()
        },
    );
    let title = child(
        commands,
        systems,
        Node {
            width: Val::Percent(100.0),
            margin: UiRect::bottom(Val::Px(2.0)),
            ..default()
        },
    );
    heading(commands, fonts, title, "hud-systems", None);
    let chips = child(
        commands,
        systems,
        Node {
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::Wrap,
            justify_content: JustifyContent::End,
            // Five to a row, twice — a block of ten in one line would read as a toolbar.
            max_width: Val::Px(5.0 * 34.0 + 4.0 * 5.0),
            column_gap: Val::Px(5.0),
            row_gap: Val::Px(5.0),
            margin: UiRect::bottom(Val::Px(6.0)),
            ..default()
        },
    );
    for (kind, icon, _) in Chip::ALL {
        annunciator(commands, drawings, chips, kind, icon);
    }
    let rows = child(
        commands,
        systems,
        Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(3.0),
            ..default()
        },
    );
    for index in 0..3 {
        drive_row(commands, fonts, rows, index);
    }
    commands.entity(systems).insert(Block::Systems);
}

/// The run, top left: the clock, how the train stands against its timetable, and the
/// timetable itself as the line diagram every railway draws its route with.
///
/// A list of "next stop / platform / departure" rows says where the train goes next; a
/// **ribbon** says where it *is*. That is the whole reason for the shape: the rail down
/// the left carries the stops in order, the wedge sits between the one behind and the one
/// ahead with the remaining distance beside it, and the next stop is the only thing on
/// the block set large. The rows have fixed roles — row 0 is always the stop behind, row
/// 1 always the next — so their weight is built once rather than switched every frame.
fn build_journey(commands: &mut Commands, fonts: &Fonts, drawings: &Drawings, journey: Entity) {
    let head = child(
        commands,
        journey,
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Baseline,
            column_gap: Val::Px(11.0),
            ..default()
        },
    );
    child(
        commands,
        head,
        (
            label(fonts, String::new(), Face::Mono, 27.0, TEXT_BRIGHT),
            Readout::Clock,
        ),
    );
    // Punctuality is a state, not a measurement: it sits in a well of its own so the eye
    // finds it without reading it, and only its type carries the colour.
    let chip = child(
        commands,
        head,
        (
            Node {
                padding: UiRect::axes(Val::Px(7.0), Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
            BackgroundColor(WELL),
        ),
    );
    child(
        commands,
        chip,
        (
            label(fonts, String::new(), Face::Mono, 11.0, TEXT_MID),
            Readout::Delay,
        ),
    );
    child(
        commands,
        journey,
        (
            label(fonts, String::new(), Face::Sans, 12.0, TEXT),
            Readout::Service,
        ),
    );

    let table = child(
        commands,
        journey,
        (
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                margin: UiRect::top(Val::Px(9.0)),
                ..default()
            },
            Block::Ribbon,
        ),
    );
    heading(commands, fonts, table, "hud-timetable", None);
    let ribbon = child(
        commands,
        journey,
        (
            Node {
                flex_direction: FlexDirection::Column,
                margin: UiRect::top(Val::Px(4.0)),
                ..default()
            },
            Block::Ribbon,
        ),
    );
    stop_row(commands, fonts, ribbon, 0);
    // The wedge always stands directly above the next stop — before the first stop of the
    // run that is the top of the ribbon, which is exactly where the train is.
    let wedge = child(
        commands,
        ribbon,
        (
            Node {
                height: Val::Px(26.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                ..default()
            },
            Block::Wedge,
        ),
    );
    let rail_cell = rail_cell(commands, wedge);
    child(
        commands,
        rail_cell,
        (
            Node {
                width: Val::Px(11.0),
                height: Val::Px(11.0),
                ..default()
            },
            picture(&drawings.wedge, ACCENT),
        ),
    );
    child(
        commands,
        wedge,
        (
            label(fonts, String::new(), Face::Mono, 11.0, ACCENT),
            Readout::LegDistance,
        ),
    );
    for index in 1..STOP_ROWS {
        stop_row(commands, fonts, ribbon, index);
    }

    let footer = child(
        commands,
        journey,
        (
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(5.0),
                margin: UiRect::top(Val::Px(9.0)),
                ..default()
            },
            Block::Score,
        ),
    );
    child(
        commands,
        footer,
        (
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(1.0),
                ..default()
            },
            BackgroundColor(RAIL),
        ),
    );
    row(commands, fonts, footer, "hud-score", Readout::Score, 13.0);
}

/// The left-hand cell of a line on the ribbon: the segment of rail that runs through it,
/// and whatever sits on that rail. Every line carries its own segment, so the rail is
/// continuous however many stops the timetable has — and it runs on past the first and
/// the last, which is what a route diagram does: the line continues, the window does not.
fn rail_cell(commands: &mut Commands, line: Entity) -> Entity {
    let cell = child(
        commands,
        line,
        Node {
            width: Val::Px(RAIL_WIDTH),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
    );
    child(
        commands,
        cell,
        (
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Percent(50.0),
                width: Val::Px(2.0),
                height: Val::Percent(100.0),
                margin: UiRect::left(Val::Px(-1.0)),
                ..default()
            },
            BackgroundColor(RAIL),
        ),
    );
    cell
}

/// One stop on the ribbon. Row 1 is the next one and the only stop set large; the one
/// behind steps back, the ones after it sit between the two.
fn stop_row(commands: &mut Commands, fonts: &Fonts, ribbon: Entity, index: usize) {
    let next = index == 1;
    let (tone, dot, size) = match index {
        0 => (TEXT_FAINT, TEXT_FAINT, 6.0),
        1 => (TEXT_BRIGHT, ACCENT, 11.0),
        _ => (TEXT_MID, TEXT_DIM, 6.0),
    };
    let line = child(
        commands,
        ribbon,
        (
            Node {
                height: Val::Px(if next { ROW + 4.0 } else { ROW }),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                ..default()
            },
            Block::StopRow(index),
        ),
    );
    let rail_cell = rail_cell(commands, line);
    child(
        commands,
        rail_cell,
        (
            Node {
                width: Val::Px(size),
                height: Val::Px(size),
                border_radius: BorderRadius::all(Val::Percent(50.0)),
                ..default()
            },
            BackgroundColor(dot),
        ),
    );
    let name = child(
        commands,
        line,
        Node {
            flex_grow: 1.0,
            overflow: Overflow::clip(),
            ..default()
        },
    );
    child(
        commands,
        name,
        (
            label(
                fonts,
                String::new(),
                if next { Face::Semibold } else { Face::Sans },
                if next { 15.0 } else { 13.0 },
                tone,
            ),
            Readout::StopName(index),
        ),
    );
    // Times in the fixed face and right-aligned: the column is what makes a timetable
    // readable down the page rather than across one line.
    let time = child(
        commands,
        line,
        Node {
            width: Val::Px(52.0),
            justify_content: JustifyContent::End,
            ..default()
        },
    );
    child(
        commands,
        time,
        (
            label(
                fonts,
                String::new(),
                Face::Mono,
                if next { 13.0 } else { 11.0 },
                tone,
            ),
            Readout::StopTime(index),
        ),
    );
    let platform = child(
        commands,
        line,
        Node {
            width: Val::Px(38.0),
            justify_content: JustifyContent::End,
            ..default()
        },
    );
    child(
        commands,
        platform,
        (
            label(fonts, String::new(), Face::Mono, 11.0, TEXT_MID),
            Readout::StopPlatform(index),
        ),
    );
}

/// Protection, the desk and the look-ahead — the band across the bottom, with the banner
/// and the hovered control's name stacked above it.
fn build_bottom(commands: &mut Commands, fonts: &Fonts, drawings: &Drawings, root: Entity) {
    let bottom = child(
        commands,
        root,
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            padding: UiRect::all(Val::Px(MARGIN)),
            row_gap: Val::Px(GAP),
            ..default()
        },
    );

    // --- The banner: the protection has taken the train over, and that has to be read
    // before anything else on the screen.
    let alert = child(
        commands,
        bottom,
        (
            Node {
                padding: UiRect::axes(Val::Px(20.0), Val::Px(7.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            BackgroundColor(BRAND),
            Block::Alert,
        ),
    );
    child(
        commands,
        alert,
        (
            label(fonts, String::new(), Face::Semibold, 16.0, TEXT_BRIGHT),
            Readout::Alert,
        ),
    );

    // --- The name of the cab control under the cursor.
    let hover = child(
        commands,
        bottom,
        (
            Node {
                padding: UiRect::axes(Val::Px(10.0), Val::Px(3.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            BackgroundColor(WASH),
            Block::Hover,
        ),
    );
    child(
        commands,
        hover,
        (
            label(fonts, String::new(), Face::Mono, 12.0, TEXT),
            Readout::Hover,
        ),
    );

    // The three pieces of the bottom band. The side cells grow from nothing and keep the
    // desk in the middle of the screen whatever they hold.
    let band = child(
        commands,
        bottom,
        Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::End,
            column_gap: Val::Px(GAP),
            ..default()
        },
    );
    let left = child(
        commands,
        band,
        Node {
            flex_basis: Val::Px(0.0),
            flex_grow: 1.0,
            justify_content: JustifyContent::Start,
            ..default()
        },
    );
    build_safety(commands, fonts, drawings, left);
    build_desk(commands, fonts, drawings, band);
    let right = child(
        commands,
        band,
        Node {
            flex_basis: Val::Px(0.0),
            flex_grow: 1.0,
            justify_content: JustifyContent::End,
            ..default()
        },
    );
    build_ahead(commands, fonts, drawings, right);
}

/// The lamp housing of the train protection, in the order the lamps sit on a German desk.
fn build_safety(commands: &mut Commands, fonts: &Fonts, drawings: &Drawings, parent: Entity) {
    let safety = hardware(
        commands,
        parent,
        Node {
            padding: UiRect::new(Val::Px(13.0), Val::Px(13.0), Val::Px(10.0), Val::Px(11.0)),
            row_gap: Val::Px(9.0),
            ..panel_node()
        },
    );
    commands.entity(safety).insert(Block::Safety);
    heading(
        commands,
        fonts,
        safety,
        "hud-protection",
        Some(Readout::Protection),
    );

    let pzb = child(
        commands,
        safety,
        Node {
            flex_direction: FlexDirection::Row,
            column_gap: Val::Px(2.0),
            ..default()
        },
    );
    lamp(
        commands,
        fonts,
        drawings,
        pzb,
        "pzb_1000hz",
        WARN,
        "1000 Hz",
    );
    lamp(commands, fonts, drawings, pzb, "pzb_500hz", BRAND, "500 Hz");
    lamp(
        commands,
        fonts,
        drawings,
        pzb,
        "pzb_befehl",
        BRAND,
        "Befehl",
    );
    lamp(commands, fonts, drawings, pzb, "", ACCENT, "");
    lamp(commands, fonts, drawings, pzb, "sifa", WARN, "Sifa");

    let lzb = child(
        commands,
        safety,
        (
            Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(2.0),
                ..default()
            },
            Block::Lzb,
        ),
    );
    lamp(commands, fonts, drawings, lzb, "lzb_ue", ACCENT, "Ü");
    lamp(commands, fonts, drawings, lzb, "lzb_g", ACCENT, "G");
    lamp(commands, fonts, drawings, lzb, "lzb_ende", WARN, "Ende");
    lamp(commands, fonts, drawings, lzb, "lzb_b", WARN, "B");
    lamp(commands, fonts, drawings, lzb, "lzb_v40", ACCENT, "V40");
    lamp(
        commands,
        fonts,
        drawings,
        lzb,
        "lzb_stoerung",
        BRAND,
        "Stör",
    );

    // The MFA's three figures, which exist only while the LZB is guiding.
    let values = child(
        commands,
        safety,
        (
            Node {
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::SpaceBetween,
                column_gap: Val::Px(14.0),
                ..default()
            },
            Block::LzbValues,
        ),
    );
    for (name, readout) in [
        ("hud-v-permitted", Readout::LzbPermitted),
        ("hud-v-target", Readout::LzbTarget),
        ("hud-target-distance", Readout::LzbDistance),
    ] {
        let cell = child(
            commands,
            values,
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(1.0),
                ..default()
            },
        );
        child(
            commands,
            cell,
            label(
                fonts,
                t!(name).to_uppercase(),
                Face::Semibold,
                9.0,
                TEXT_DIM,
            ),
        );
        child(
            commands,
            cell,
            (
                label(fonts, String::new(), Face::Mono, 14.0, TEXT_BRIGHT),
                readout,
            ),
        );
    }
    child(
        commands,
        safety,
        (
            label(fonts, String::new(), Face::Sans, 11.0, WARN),
            Readout::PzbNote,
        ),
    );
}

/// The desk: the three instruments, and beside them what the levers are doing.
fn build_desk(commands: &mut Commands, fonts: &Fonts, drawings: &Drawings, parent: Entity) {
    let desk = hardware(
        commands,
        parent,
        Node {
            flex_direction: FlexDirection::Row,
            flex_shrink: 0.0,
            align_items: AlignItems::End,
            column_gap: Val::Px(18.0),
            padding: UiRect::new(Val::Px(18.0), Val::Px(20.0), Val::Px(14.0), Val::Px(12.0)),
            ..panel_node()
        },
    );

    // --- The speedometer. Ticks every 10, figures every 40 where the scale is long
    // enough to need thinning out — which is how a cab's speedometer is engraved.
    let scale = drawings.speed_scale;
    let majors = (scale / 20.0).round() as u32;
    let every = if majors > 8 { 2 } else { 1 };
    let labels: Vec<(f32, String)> = (0..=majors)
        .step_by(every)
        .map(|i| (i as f32 / majors as f32, format!("{:.0}", i as f64 * 20.0)))
        .collect();
    let speed = column(commands, desk);
    let dial_speed = dial(
        commands,
        fonts,
        speed,
        &drawings.speedo_face,
        SPEEDO,
        &labels,
        10.0,
    );
    // The line's limit first, the supervised speed over it — the stricter of the two is
    // the one that must not be hidden by the other.
    pointer(commands, dial_speed, &drawings.marker, Meter::Limit, WARN);
    let supervision = pointer(
        commands,
        dial_speed,
        &drawings.marker,
        Meter::Supervision,
        BRAND,
    );
    commands.entity(supervision).insert(Block::Supervision);
    on_face(commands, fonts, dial_speed, Readout::Speed, 26.0, 0.58);
    let unit = child(
        commands,
        dial_speed,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Percent(74.0),
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
    );
    child(
        commands,
        unit,
        label(fonts, t!("hud-unit-kmh"), Face::Semibold, 9.0, TEXT_DIM),
    );
    pointer(
        commands,
        dial_speed,
        &drawings.needle,
        Meter::Speed,
        TEXT_BRIGHT,
    );
    hub(commands, 13.0, dial_speed);
    let under_speed = under(commands, speed, SPEEDO);
    child(
        commands,
        under_speed,
        (
            label(fonts, String::new(), Face::Mono, 11.0, TEXT_MID),
            Readout::SpeedLimit,
        ),
    );
    child(
        commands,
        under_speed,
        (
            label(fonts, String::new(), Face::Mono, 11.0, TEXT_MID),
            Readout::Supervision,
        ),
    );

    // --- The Doppelmanometer: brake pipe and main reservoir on one face, as in the cab.
    let air_labels: Vec<(f32, String)> = (0..=(MANOMETER_MAX as u32))
        .step_by(2)
        .map(|i| (i as f32 / MANOMETER_MAX as f32, format!("{i}")))
        .collect();
    let air = column(commands, desk);
    let dial_air = dial(
        commands,
        fonts,
        air,
        &drawings.air_face,
        MANOMETER,
        &air_labels,
        8.0,
    );
    pointer(
        commands,
        dial_air,
        &drawings.fine_needle,
        Meter::Reservoir,
        BRAND,
    );
    pointer(
        commands,
        dial_air,
        &drawings.fine_needle,
        Meter::Pipe,
        TEXT_BRIGHT,
    );
    hub(commands, 9.0, dial_air);
    let under_air = under(commands, air, MANOMETER);
    child(
        commands,
        under_air,
        (
            label(fonts, String::new(), Face::Mono, 11.0, TEXT_BRIGHT),
            Readout::Pipe,
        ),
    );
    child(
        commands,
        under_air,
        (
            label(fonts, String::new(), Face::Mono, 11.0, BRAND),
            Readout::Reservoir,
        ),
    );

    // --- The brake cylinder, on its own gauge and its own shorter scale.
    let cylinder_labels: Vec<(f32, String)> = (0..=(CYLINDER_MAX as u32))
        .step_by(2)
        .map(|i| (i as f32 / CYLINDER_MAX as f32, format!("{i}")))
        .collect();
    let cyl = column(commands, desk);
    let dial_cyl = dial(
        commands,
        fonts,
        cyl,
        &drawings.cylinder_face,
        MANOMETER,
        &cylinder_labels,
        8.0,
    );
    pointer(
        commands,
        dial_cyl,
        &drawings.fine_needle,
        Meter::Cylinder,
        WARN,
    );
    hub(commands, 9.0, dial_cyl);
    let under_cyl = under(commands, cyl, MANOMETER);
    child(
        commands,
        under_cyl,
        (
            label(fonts, String::new(), Face::Mono, 11.0, TEXT_BRIGHT),
            Readout::Cylinder,
        ),
    );

    child(
        commands,
        desk,
        (
            Node {
                width: Val::Px(1.0),
                height: Val::Px(SPEEDO * 0.78),
                ..default()
            },
            BackgroundColor(EDGE),
        ),
    );

    // --- The levers, and the rest of the train.
    let levers = child(
        commands,
        desk,
        Node {
            width: Val::Px(196.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(6.0),
            ..default()
        },
    );
    heading(commands, fonts, levers, "hud-levers", None);
    row(commands, fonts, levers, "hud-power", Readout::Power, 13.0);
    bar(commands, levers, Meter::Power, 4.0);
    row(
        commands,
        fonts,
        levers,
        "hud-brake",
        Readout::BrakeValve,
        13.0,
    );
    row(commands, fonts, levers, "hud-effort", Readout::Effort, 13.0);
    row(
        commands,
        fonts,
        levers,
        "hud-reverser",
        Readout::Reverser,
        13.0,
    );
    let afb = row(commands, fonts, levers, "hud-afb", Readout::Afb, 13.0);
    commands.entity(afb).insert(Block::Afb);
    row(
        commands,
        fonts,
        levers,
        "hud-odometer",
        Readout::Odometer,
        13.0,
    );
}

/// The column an instrument stands in: the dial, and the figures under it.
fn column(commands: &mut Commands, parent: Entity) -> Entity {
    child(
        commands,
        parent,
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: Val::Px(5.0),
            ..default()
        },
    )
}

/// The line of figures under an instrument, as wide as the instrument is.
fn under(commands: &mut Commands, parent: Entity, width: f32) -> Entity {
    child(
        commands,
        parent,
        Node {
            width: Val::Px(width),
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceEvenly,
            column_gap: Val::Px(8.0),
            ..default()
        },
    )
}

/// What the line asks for next, signed the way the line signs it.
fn build_ahead(commands: &mut Commands, fonts: &Fonts, drawings: &Drawings, parent: Entity) {
    let ahead = hardware(
        commands,
        parent,
        Node {
            width: Val::Px(148.0),
            align_items: AlignItems::Center,
            padding: UiRect::new(Val::Px(12.0), Val::Px(12.0), Val::Px(10.0), Val::Px(12.0)),
            row_gap: Val::Px(8.0),
            ..panel_node()
        },
    );
    commands.entity(ahead).insert(Block::Ahead);
    let head = child(
        commands,
        ahead,
        Node {
            width: Val::Percent(100.0),
            ..default()
        },
    );
    heading(commands, fonts, head, "hud-ahead", None);

    let sign = child(
        commands,
        ahead,
        Node {
            width: Val::Px(72.0),
            height: Val::Px(72.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
    );
    // The Lf 7 board carries the figure; Hp 0 is a disc and carries nothing.
    let board = child(
        commands,
        sign,
        (cover(), picture(&drawings.board, WARN), Block::AheadBoard),
    );
    let _ = board;
    child(
        commands,
        sign,
        (
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(50.0),
                top: Val::Percent(50.0),
                width: Val::Px(52.0),
                height: Val::Px(52.0),
                margin: UiRect::new(Val::Px(-26.0), Val::ZERO, Val::Px(-26.0), Val::ZERO),
                ..default()
            },
            picture(&drawings.disc, BRAND),
            Block::AheadStop,
        ),
    );
    let figure = child(
        commands,
        sign,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Percent(38.0),
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
    );
    child(
        commands,
        figure,
        (
            label(fonts, String::new(), Face::Mono, 22.0, TEXT_BRIGHT),
            Readout::AheadSpeed,
        ),
    );
    bar(commands, ahead, Meter::Ahead, 4.0);
    child(
        commands,
        ahead,
        (
            label(fonts, String::new(), Face::Mono, 12.0, TEXT_MID),
            Readout::AheadDistance,
        ),
    );
}

/// F5: the keyboard, in the groups a driver would look for it in, and under it what the
/// ten annunciators mean — the one thing a pictogram cannot say for itself.
fn build_help(commands: &mut Commands, fonts: &Fonts, drawings: &Drawings, root: Entity) {
    let scrim = child(
        commands,
        root,
        (
            Node {
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..cover()
            },
            BackgroundColor(Color::srgba(0.020, 0.020, 0.024, 0.90)),
            Block::Help,
        ),
    );
    let sheet = hardware(
        commands,
        scrim,
        Node {
            padding: UiRect::all(Val::Px(30.0)),
            row_gap: Val::Px(18.0),
            ..panel_node()
        },
    );
    heading(commands, fonts, sheet, "hud-help", None);
    let columns = child(
        commands,
        sheet,
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Start,
            column_gap: Val::Px(30.0),
            ..default()
        },
    );
    for (group, keys) in HELP {
        let column = child(
            commands,
            columns,
            Node {
                width: Val::Px(214.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(5.0),
                ..default()
            },
        );
        heading(commands, fonts, column, group, None);
        for (cap, action) in keys {
            let line = child(
                commands,
                column,
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(10.0),
                    ..default()
                },
            );
            let key = child(
                commands,
                line,
                (
                    Node {
                        min_width: Val::Px(72.0),
                        padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                        justify_content: JustifyContent::Center,
                        border_radius: BorderRadius::all(Val::Px(3.0)),
                        ..default()
                    },
                    BackgroundColor(CHIP),
                ),
            );
            child(
                commands,
                key,
                label(fonts, cap.to_string(), Face::Mono, 11.0, TEXT),
            );
            child(
                commands,
                line,
                label(fonts, t!(action), Face::Sans, 12.0, TEXT_MID),
            );
        }
    }

    heading(commands, fonts, sheet, "hud-help-annunciators", None);
    let legend = child(
        commands,
        sheet,
        Node {
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::Wrap,
            column_gap: Val::Px(22.0),
            row_gap: Val::Px(8.0),
            max_width: Val::Px(1190.0),
            ..default()
        },
    );
    for (_, icon, key) in Chip::ALL {
        let entry = child(
            commands,
            legend,
            Node {
                width: Val::Px(210.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(9.0),
                ..default()
            },
        );
        child(
            commands,
            entry,
            (
                Node {
                    width: Val::Px(19.0),
                    height: Val::Px(19.0),
                    flex_shrink: 0.0,
                    ..default()
                },
                picture(&drawings.icons[icon as usize], TEXT_MID),
            ),
        );
        child(
            commands,
            entry,
            label(fonts, t!(key), Face::Sans, 12.0, TEXT_MID),
        );
    }
    child(
        commands,
        sheet,
        label(fonts, t!("hud-help-close"), Face::Sans, 11.0, TEXT_DIM),
    );
}

/// F6: everything the simulation knows that a driver has no use for — one mono block,
/// because it is a diagnostic and dressing it up would make it look like an instrument.
fn build_diagnostics(commands: &mut Commands, fonts: &Fonts, root: Entity) {
    let block = hardware(
        commands,
        root,
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(MARGIN),
            right: Val::Px(MARGIN + 310.0),
            width: Val::Px(400.0),
            ..panel_node()
        },
    );
    commands.entity(block).insert(Block::Diagnostics);
    heading(commands, fonts, block, "hud-diagnostics", None);
    child(
        commands,
        block,
        (
            label(fonts, String::new(), Face::Mono, 11.0, TEXT_MID),
            Readout::Diagnostics,
        ),
    );
}

// ---------------------------------------------------------------------------------
// Filling it in
// ---------------------------------------------------------------------------------

/// F5 and F6, and whether the display is on at all. Runs in every state: the pause
/// overlay draws its own scrim, and a HUD showing through it would read as still live.
pub fn hud_visibility(
    keys: Res<ButtonInput<KeyCode>>,
    mut gameplay: ResMut<Gameplay>,
    over: Option<Res<HudOverride>>,
    game: Res<State<crate::GameState>>,
    mut overlays: ResMut<Overlays>,
    mut root: Query<&mut Visibility, With<Hud>>,
) {
    let driving = *game.get() == crate::GameState::Driving;
    if driving && keys.just_pressed(KeyCode::F5) {
        overlays.help = !overlays.help;
    }
    if driving && keys.just_pressed(KeyCode::F6) {
        overlays.diagnostics = !overlays.diagnostics;
    }
    // F7 walks full → reduced → off → full. The resource is only written on the press:
    // touching it every frame would have the settings file rewritten every frame.
    if driving && keys.just_pressed(KeyCode::F7) {
        gameplay.hud = mode(&gameplay, over.as_deref()).cycle(1);
    }
    let mode = mode(&gameplay, over.as_deref());
    for mut visibility in root.iter_mut() {
        *visibility = if mode.drawn() && driving {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

/// Every node the display writes into, in one parameter. A Bevy system takes sixteen
/// parameters at most and the HUD has more than that between its resources and its eight
/// kinds of node; bundling the queries is what keeps them in one system, and one system
/// is what lets them share a single [`Frame`] — the look-ahead down the line is scanned
/// once a frame, not once per query.
#[derive(SystemParam)]
pub struct Nodes<'w, 's> {
    readouts: Query<'w, 's, (&'static Readout, &'static mut Text, &'static mut TextColor)>,
    gauges: Query<
        'w,
        's,
        (
            &'static Gauge,
            &'static mut Node,
            &'static mut BackgroundColor,
        ),
        Without<Block>,
    >,
    needles: Query<
        'w,
        's,
        (
            &'static Needle,
            &'static mut UiTransform,
            &'static mut ImageNode,
        ),
        Without<Chip>,
    >,
    chips: Query<'w, 's, (&'static Chip, &'static mut ImageNode)>,
    lamps: Query<'w, 's, (&'static Lamp, &'static Children)>,
    glasses: Query<'w, 's, &'static mut ImageNode, (Without<Chip>, Without<Needle>)>,
    legends: Query<'w, 's, &'static mut TextColor, Without<Readout>>,
    blocks: Query<'w, 's, (&'static Block, &'static mut Node), Without<Gauge>>,
}

/// Everything the display shows, once per frame.
// A Bevy system takes its resources as parameters — the argument count says nothing here.
#[allow(clippy::too_many_arguments)]
pub fn update_hud(
    sim: Res<SimResource>,
    player: Res<PlayerTrain>,
    terrain: Res<TerrainInfo>,
    streamer: Res<TerrainStreamer>,
    view: Res<ViewDistance>,
    mouse: Res<CabMouse>,
    gameplay: Res<Gameplay>,
    over: Option<Res<HudOverride>>,
    overlays: Res<Overlays>,
    drawings: Res<Drawings>,
    // Only present in a multiplayer run (`net.rs`); single player never sees the line.
    session: Option<Res<crate::net::Session>>,
    mut nodes: Nodes,
) {
    let mode = mode(&gameplay, over.as_deref());
    if !mode.drawn() {
        return;
    }
    let frame = Frame::read(
        &sim.0,
        player.0,
        mode,
        &overlays,
        &drawings,
        &mouse,
        &terrain,
        &streamer,
        &view,
        session.as_deref(),
    );

    for (readout, mut content, mut color) in nodes.readouts.iter_mut() {
        let (value, tone) = frame.readout(*readout);
        if **content != value {
            **content = value;
        }
        if color.0 != tone {
            color.0 = tone;
        }
    }
    for (gauge, mut node, mut background) in nodes.gauges.iter_mut() {
        let (fraction, tone) = frame.gauge(gauge.0);
        node.width = Val::Percent(fraction.clamp(0.0, 1.0) * 100.0);
        if background.0 != tone {
            background.0 = tone;
        }
    }
    for (needle, mut transform, mut image) in nodes.needles.iter_mut() {
        let (fraction, tone) = frame.gauge(needle.0);
        transform.rotation = Rot2::radians(glyphs::dial_angle(fraction));
        if image.color != tone {
            image.color = tone;
        }
    }
    for (chip, mut image) in nodes.chips.iter_mut() {
        image.color = frame.chip(*chip);
    }
    for (lamp, children) in nodes.lamps.iter() {
        let lit = frame.lamp(lamp.name);
        for part in children.iter() {
            if let Ok(mut glass) = nodes.glasses.get_mut(part) {
                glass.color = if lit { lamp.tone } else { GLASS_DARK };
            }
            if let Ok(mut color) = nodes.legends.get_mut(part) {
                color.0 = if lit { lamp.tone } else { TEXT_FAINT };
            }
        }
    }
    for (block, mut node) in nodes.blocks.iter_mut() {
        let display = if frame.shows(*block) {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
}

/// Which stop of the timetable a row of the ribbon shows. Row 1 is always the next stop,
/// row 0 the one behind it, and the rows after it follow. Before the first stop of a run
/// there is nothing behind, and row 0 stays empty.
fn ribbon_index(next: usize, row: usize) -> Option<usize> {
    if row == 0 && next == 0 {
        return None;
    }
    (next + row).checked_sub(1)
}

// ---------------------------------------------------------------------------------
// One frame's worth of the simulation
// ---------------------------------------------------------------------------------

/// Everything the readouts, gauges, lamps and blocks are derived from, read once per
/// frame. The scan down the line and the indicator list cost something to produce and are
/// wanted in several places; the rest is here so that what a figure *means* is decided in
/// one spot rather than in every arm that prints it.
struct Frame<'a> {
    sim: &'a Sim,
    train: &'a Train,
    /// The vehicle at the head of the train: the one whose air, drive and protection the
    /// driver reads. ponytail: the first vehicle, not the occupied cab — a HUD for a train
    /// driven from the rear needs `train.cab` here and in the pressure readouts alike.
    loco: &'a Vehicle,
    runtime: &'a TrainRuntime,
    cab: &'a sim_core::cab::CabInputs,
    /// Simulation time [s] and time of day [s since midnight].
    time: f64,
    clock: f64,
    /// Permitted speed here [km/h], off the track.
    limit: f64,
    /// Full scale of the speedometer [km/h] — the face it was drawn with.
    scale: f64,
    /// The next restriction worth braking for, if the look-ahead found one.
    ahead: Option<sim_core::lookahead::Restriction>,
    /// Indicator lamps of the leading vehicle's protection, and the phase of the blink.
    indicators: Vec<Indicator>,
    blink: bool,
    /// The next stop of the timetable, and how far away it is [m].
    stop: Option<(&'a ScheduledStop, Option<f64>)>,
    /// The scenario messages still young enough to stand, newest first.
    messages: Vec<&'a Message>,
    /// The cab control under the cursor, already worded.
    hover: Option<String>,
    /// The two overlays: whether they are open, and what F6 has to say.
    help: bool,
    diagnostics: Option<String>,
    /// How much of the display this step draws.
    mode: HudMode,
}

impl<'a> Frame<'a> {
    // A frame is the whole HUD's input; it reads what the HUD shows, not one thing.
    #[allow(clippy::too_many_arguments)]
    fn read(
        sim: &'a Sim,
        player: usize,
        mode: HudMode,
        overlays: &Overlays,
        drawings: &Drawings,
        mouse: &CabMouse,
        terrain: &TerrainInfo,
        streamer: &TerrainStreamer,
        view: &ViewDistance,
        session: Option<&crate::net::Session>,
    ) -> Self {
        let train = &sim.trains[player];
        let loco = &train.vehicles[0];
        let runtime = &sim.runtime[player];
        let scan = sim_core::lookahead::scan(&sim.net, &sim.interlock, loco.pos, LOOKAHEAD);
        let limit = loco.pos.speed_limit(&sim.net);
        // Only a restriction that actually restricts is news — the line's own steps back
        // up to line speed are nothing to brake for.
        let ahead = scan
            .restrictions
            .iter()
            .find(|r| r.speed < limit - 1.0)
            .copied();
        let stop = sim
            .score
            .timetable
            .stops
            .get(sim.score.next_stop)
            .map(|stop| (stop, stop.distance_from(&sim.net, loco.pos, 20_000.0)));
        let messages = sim
            .scenario
            .recent_messages(MESSAGES)
            .iter()
            .rev()
            .filter(|m| sim.time - m.time < MESSAGE_LIFE)
            .collect();
        let hover = mouse.hover_info.map(|(key, value)| {
            t!(
                "hud-control",
                name = t!(key),
                value = format!("{:.0}", value * 100.0)
            )
        });
        let mut frame = Self {
            sim,
            train,
            loco,
            runtime,
            cab: &sim.controls[player],
            time: sim.time,
            clock: sim.clock().rem_euclid(DAY),
            limit,
            scale: drawings.speed_scale,
            ahead,
            indicators: loco.safety.indicators(),
            blink: (sim.time * BLINK_HZ).fract() < 0.5,
            stop,
            messages,
            hover,
            help: overlays.help,
            diagnostics: None,
            mode,
        };
        // Only worked out while the panel is open — it walks every signal of the line.
        if overlays.diagnostics {
            frame.diagnostics = Some(frame.diagnose(terrain, streamer, view, session));
        }
        frame
    }

    // -----------------------------------------------------------------------------
    // Wording
    // -----------------------------------------------------------------------------

    /// `12:34:56` from a time of day in seconds.
    fn hms(seconds: f64) -> String {
        let seconds = seconds.rem_euclid(DAY);
        format!(
            "{:02}:{:02}:{:02}",
            (seconds / 3600.0) as u32,
            (seconds / 60.0) as u32 % 60,
            seconds as u32 % 60
        )
    }

    /// A distance the way a driver says it: metres up close, kilometres beyond one.
    fn distance(metres: f64) -> String {
        if metres < 1000.0 {
            format!("{:.0} m", (metres / 10.0).round() * 10.0)
        } else {
            format!("{} km", decimal(metres / 1000.0, 1))
        }
    }

    /// The wall clock a scheduled second falls on. A daily timetable counts from midnight
    /// and a scenario's from the start of the run; `next_occurrence` is what tells the two
    /// apart, so the HUD never has to.
    fn scheduled(&self, seconds: f64) -> f64 {
        let start = self.sim.start.seconds();
        start
            + self
                .sim
                .score
                .timetable
                .next_occurrence(self.time, start, seconds)
    }

    /// How the train stands against its timetable [min], positive = late.
    ///
    /// Two things make it up. The delay it left the last stop with is carried — that does
    /// not go away by itself. On top of that, once the scheduled arrival at the next stop
    /// has passed and the train is not there, it is late by however long it has been. A
    /// train that has not reached a stop yet is *not* early, which is why the projected
    /// figure is only ever allowed to make the delay worse: without that, every run would
    /// open by announcing itself seven minutes ahead of a stop it has not moved towards.
    fn delay(&self) -> Option<f64> {
        let (stop, _) = self.stop?;
        let carried = self
            .sim
            .score
            .stops
            .last()
            .map(|report| report.delay)
            .unwrap_or(0.0);
        let start = self.sim.start.seconds();
        let due = self
            .sim
            .score
            .timetable
            .delay(self.time, start, stop.arrival);
        Some(carried.max(due) / 60.0)
    }

    /// `12:34` — the ribbon reads down a column, and seconds in it would only be noise.
    fn clock_time(seconds: f64) -> String {
        let seconds = seconds.rem_euclid(DAY);
        format!(
            "{:02}:{:02}",
            (seconds / 3600.0) as u32,
            (seconds / 60.0) as u32 % 60
        )
    }

    /// The stop on row `index` of the ribbon: 0 is the one behind, 1 the next, 2 and 3
    /// the ones after it. `None` where the timetable does not reach that far — at the
    /// start of a run there is nothing behind, at the end nothing ahead.
    fn ribbon(&self, index: usize) -> Option<&'a ScheduledStop> {
        let at = ribbon_index(self.sim.score.next_stop, index)?;
        self.sim.score.timetable.stops.get(at)
    }

    /// How bright a row of the ribbon is. The next stop is the one thing on the block a
    /// driver has to find without looking for it; what is behind steps out of the way.
    fn ribbon_tone(&self, index: usize) -> Color {
        match index {
            0 => TEXT_FAINT,
            1 => TEXT_BRIGHT,
            _ => TEXT_MID,
        }
    }

    /// The speed the train protection supervises, where it supervises one at all.
    fn supervision(&self) -> Option<f64> {
        self.runtime.protection.speed_limit
    }

    /// The speed that must not be exceeded here: the lower of what the line permits and
    /// what the protection supervises.
    fn ceiling(&self) -> f64 {
        self.supervision().unwrap_or(self.limit).min(self.limit)
    }

    /// How the speed is coloured: bone white while it is legal, amber in the last few
    /// km/h below the ceiling, red above it. The needle takes the same tone, so the
    /// figure and the pointer never disagree.
    fn speed_tone(&self) -> Color {
        let speed = self.train.speed_kmh().abs();
        let ceiling = self.ceiling();
        if speed > ceiling + 1.0 {
            BRAND
        } else if speed > ceiling - 4.0 {
            WARN
        } else {
            TEXT_BRIGHT
        }
    }

    /// The PZB's train category lamp — the one indicator whose name is its own legend.
    fn category_lamp(&self) -> Option<&Indicator> {
        self.indicators
            .iter()
            .find(|i| !i.name.is_empty() && i.name.chars().all(|c| c.is_ascii_digit()))
    }

    /// A numeric indicator of the MFA, if the system reports one.
    fn mfa(&self, name: &str) -> Option<f64> {
        self.indicators
            .iter()
            .find(|i| i.name == name)
            .and_then(|i| i.value)
    }

    fn mfa_text(&self, name: &str, unit: &str) -> String {
        self.mfa(name)
            .map(|v| format!("{v:.0} {unit}"))
            .unwrap_or_default()
    }

    /// Is this lamp lit? A flashing one counts as lit for half of every blink.
    fn lamp(&self, name: &str) -> bool {
        let indicator = if name.is_empty() {
            self.category_lamp()
        } else {
            self.indicators.iter().find(|i| i.name == name)
        };
        match indicator.map(|i| i.lamp) {
            Some(LampState::On) => true,
            Some(LampState::Blinking) => self.blink,
            _ => false,
        }
    }

    /// The banner: what has taken the train out of the driver's hands, if anything.
    fn alert(&self) -> Option<String> {
        match self.runtime.protection.action {
            ProtectionAction::EmergencyBrake => Some(t!("hud-alert-emergency")),
            ProtectionAction::ForcedServiceBrake => Some(t!("hud-alert-forced")),
            ProtectionAction::TractionCutOff => Some(t!("hud-alert-cut-off")),
            ProtectionAction::None => self.runtime.blocked.then(|| t!("hud-alert-blocked")),
        }
    }

    /// The cylinder pressure the driver watches: whichever of the automatic brake and the
    /// direct one is actually pressing the blocks against the wheel.
    fn cylinder(&self) -> f64 {
        self.loco
            .brake
            .cylinder
            .max(self.loco.brake.direct_cylinder)
    }

    /// The drive's own instrument, three rows of it: what is measured, and what it reads.
    /// A steam locomotive is read at the boiler, a diesel at the engine, an electric at
    /// the wire — the panel does not impose a shape the vehicle does not have.
    fn drive_row(&self, index: usize) -> Option<(String, String)> {
        let drive = self.loco.traction.drives[0];
        let catenary = || {
            (
                t!("hud-catenary"),
                format!(
                    "{} kV",
                    decimal(self.loco.traction.line_voltage / 1000.0, 1)
                ),
            )
        };
        let current = || {
            (
                t!("hud-motor-current"),
                format!("{:.0} A", drive.motor_current),
            )
        };
        let rows: [Option<(String, String)>; 3] = match self.loco.spec.traction()? {
            TractionSpec::Diesel { electric, .. } => [
                Some((t!("hud-engine"), format!("{:.0} 1/min", drive.engine_rpm))),
                Some((
                    t!("hud-fill"),
                    format!("{:.0} %", drive.engine_fill * 100.0),
                )),
                Some(if electric.is_some() {
                    // A diesel-electric is read at the generator, not at the transmission.
                    (
                        t!("hud-generator"),
                        format!(
                            "{:.0} V   {:.0} A",
                            drive.generator_voltage, drive.motor_current
                        ),
                    )
                } else {
                    (
                        t!("hud-converter"),
                        format!("{}   ν {}", drive.circuit + 1, decimal(drive.circuit_nu, 2)),
                    )
                }),
            ],
            TractionSpec::Steam { loco: steam, .. } => match drive.steam {
                Some(boiler) => [
                    Some((
                        t!("hud-boiler"),
                        format!("{} bar", decimal(boiler.pressure, 1)),
                    )),
                    Some((
                        t!("hud-water-glass"),
                        format!("{:.0} %", boiler.glass(steam).clamp(0.0, 1.0) * 100.0),
                    )),
                    Some((
                        t!("hud-fire"),
                        format!(
                            "{:.0} %   {:.0} kg",
                            boiler.fire_intensity * 100.0,
                            boiler.fire_mass
                        ),
                    )),
                ],
                None => [None, None, None],
            },
            TractionSpec::TapChanger { steps, .. } => [
                Some((
                    t!("hud-notch"),
                    format!("{} / {steps}", decimal(drive.step, 1)),
                )),
                Some(catenary()),
                Some(current()),
            ],
            _ => [
                Some(catenary()),
                Some(current()),
                (drive.dynamic_force.abs() > 1000.0).then(|| {
                    (
                        t!("hud-dynamic-brake"),
                        format!("{:.0} kN", drive.dynamic_force.abs() / 1000.0),
                    )
                }),
            ],
        };
        rows.into_iter().nth(index).flatten()
    }

    // -----------------------------------------------------------------------------
    // What every node asks for
    // -----------------------------------------------------------------------------

    fn readout(&self, readout: Readout) -> (String, Color) {
        match readout {
            // --- The run.
            Readout::Clock => (Self::hms(self.clock), TEXT_BRIGHT),
            Readout::Delay => match self.delay() {
                None => (String::new(), TEXT_MID),
                Some(minutes) if minutes >= 1.0 => {
                    (t!("hud-late", minutes = format!("{minutes:.0}")), WARN)
                }
                Some(minutes) if minutes <= -1.0 => (
                    t!("hud-early", minutes = format!("{:.0}", -minutes)),
                    TEXT_MID,
                ),
                Some(_) => (t!("hud-on-time"), TEXT_BRIGHT),
            },
            Readout::Service => {
                let table = &self.sim.score.timetable;
                // Most timetables write the category into the number already ("RE 4711");
                // only prefix it where the number stands on its own.
                let service = if table.category.is_empty()
                    || table.number.starts_with(table.category.as_str())
                {
                    table.number.clone()
                } else {
                    format!("{} {}", table.category, table.number)
                };
                let service = service.trim();
                let name = &self.sim.scenario.scenario.name;
                (
                    match (service.is_empty(), name.is_empty()) {
                        (true, true) => t!("hud-free-run"),
                        (true, false) => name.clone(),
                        (false, true) => service.to_string(),
                        (false, false) => format!("{service}  ·  {name}"),
                    },
                    TEXT_MID,
                )
            }
            // One line of the ribbon. An empty row prints nothing at all — the block
            // that holds it has already collapsed, and a half-filled line under a
            // collapsed one would be worse than no line.
            Readout::StopName(index) => (
                self.ribbon(index)
                    .map(|s| s.name.clone())
                    .unwrap_or_default(),
                self.ribbon_tone(index),
            ),
            Readout::StopTime(index) => (
                self.ribbon(index)
                    .map(|s| Self::clock_time(self.scheduled(s.departure)))
                    .unwrap_or_default(),
                self.ribbon_tone(index),
            ),
            Readout::StopPlatform(index) => (
                self.ribbon(index)
                    .map(|s| s.platform.clone())
                    .filter(|p| !p.is_empty())
                    .map(|p| t!("hud-platform", platform = p))
                    .unwrap_or_default(),
                TEXT_MID,
            ),
            Readout::LegDistance => (
                self.stop
                    .and_then(|(_, d)| d)
                    .map(Self::distance)
                    .unwrap_or_default(),
                ACCENT,
            ),
            Readout::Score => {
                let report = self.sim.score.report(self.sim.scenario.bonus);
                match &self.sim.scenario.outcome {
                    Some(outcome) => (
                        format!(
                            "{}  {}",
                            report.total,
                            if outcome.success {
                                t!("hud-scenario-passed")
                            } else {
                                t!("hud-scenario-failed")
                            }
                        ),
                        if outcome.success { TEXT_BRIGHT } else { WARN },
                    ),
                    None => (report.total.to_string(), TEXT_BRIGHT),
                }
            }

            // --- Systems.
            Readout::DriveLabel(index) => (
                self.drive_row(index).map(|(l, _)| l).unwrap_or_default(),
                TEXT_MID,
            ),
            Readout::Drive(index) => (
                self.drive_row(index).map(|(_, v)| v).unwrap_or_default(),
                TEXT_BRIGHT,
            ),

            // --- The desk.
            Readout::Speed => (
                format!("{:.0}", self.train.speed_kmh().abs()),
                self.speed_tone(),
            ),
            Readout::SpeedLimit => (
                t!("hud-permitted", speed = format!("{:.0}", self.limit)),
                WARN,
            ),
            Readout::Supervision => match self.supervision() {
                Some(v) => (t!("hud-supervised", speed = format!("{v:.0}")), BRAND),
                None => (String::new(), TEXT_MID),
            },
            Readout::Power => (
                format!("{:+.0} %", self.cab.throttle * 100.0),
                if self.cab.throttle < -0.01 {
                    WARN
                } else {
                    TEXT_BRIGHT
                },
            ),
            Readout::BrakeValve => match self.cab.brake_valve {
                DriverBrakeValve::Release => (t!("hud-valve-release"), TEXT_BRIGHT),
                DriverBrakeValve::Lap => (t!("hud-valve-lap"), TEXT_BRIGHT),
                DriverBrakeValve::Fill => (t!("hud-valve-fill"), TEXT_BRIGHT),
                DriverBrakeValve::Service(drop) => (
                    t!("hud-valve-service", drop = decimal(drop, 2)),
                    TEXT_BRIGHT,
                ),
                DriverBrakeValve::Emergency => (t!("hud-valve-emergency"), BRAND),
            },
            Readout::Effort => {
                let braking: f64 = self.train.vehicles.iter().map(|v| v.brake_effort).sum();
                let tractive = self.loco.tractive_effort;
                if braking > tractive.abs() {
                    (format!("−{:.0} kN", braking / 1000.0), WARN)
                } else {
                    (format!("{:.0} kN", tractive / 1000.0), TEXT_BRIGHT)
                }
            }
            // The two figures under the Doppelmanometer keep the needles' colours, so
            // which reading belongs to which pointer needs no legend.
            Readout::Pipe => (
                t!("hud-air-pipe", value = decimal(self.loco.brake.pipe, 1)),
                TEXT_BRIGHT,
            ),
            Readout::Reservoir => (
                t!(
                    "hud-air-reservoir",
                    value = decimal(self.loco.brake.main_reservoir, 1)
                ),
                if self.loco.brake.main_reservoir < LOW_RESERVOIR {
                    WARN
                } else {
                    BRAND
                },
            ),
            Readout::Cylinder => (
                t!("hud-air-cylinder", value = decimal(self.cylinder(), 1)),
                TEXT_BRIGHT,
            ),
            Readout::Reverser => match self.cab.reverser {
                1 => (t!("hud-forward"), TEXT_BRIGHT),
                -1 => (t!("hud-reverse"), TEXT_BRIGHT),
                _ => (t!("hud-neutral"), TEXT_MID),
            },
            Readout::Afb => {
                if self.cab.afb {
                    (format!("{:.0} km/h", self.cab.afb_target), TEXT_BRIGHT)
                } else {
                    (t!("common-off"), TEXT_MID)
                }
            }
            Readout::Odometer => (Self::distance(self.runtime.odometer), TEXT_BRIGHT),

            // --- Train protection.
            Readout::Protection => (
                match &self.loco.safety {
                    SafetySystems::De(de) => de
                        .pzb
                        .map(|pzb| format!("{}  ·  {:?}", pzb.variant.name(), pzb.train_type))
                        .unwrap_or_default(),
                    SafetySystems::None => String::new(),
                },
                TEXT_MID,
            ),
            Readout::PzbNote => (
                match &self.loco.safety {
                    SafetySystems::De(de) => match de.pzb {
                        Some(pzb) => match pzb.self_test().phase() {
                            SelfTestPhase::Passed if pzb.is_restrictive() => {
                                t!("hud-pzb-restrictive")
                            }
                            SelfTestPhase::Passed => String::new(),
                            phase => t!("hud-self-test", phase = format!("{phase:?}")),
                        },
                        None => String::new(),
                    },
                    SafetySystems::None => String::new(),
                },
                WARN,
            ),
            // The category lamp's legend is the category: 85, 70, 55 — and it lights with
            // its glass like every other legend on the housing.
            Readout::CategoryLamp => (
                self.category_lamp()
                    .map(|i| i.name.to_string())
                    .unwrap_or_default(),
                if self.lamp("") { ACCENT } else { TEXT_FAINT },
            ),
            Readout::LzbPermitted => (self.mfa_text("mfa_v_soll", "km/h"), TEXT_BRIGHT),
            Readout::LzbTarget => (self.mfa_text("mfa_v_ziel", "km/h"), TEXT_BRIGHT),
            Readout::LzbDistance => (self.mfa_text("mfa_zielentfernung", "m"), TEXT_BRIGHT),

            // --- Look-ahead. The board carries the figure; the disc of Hp 0 carries none.
            Readout::AheadSpeed => match self.ahead {
                Some(r) if r.speed <= 0.1 => (String::new(), TEXT_BRIGHT),
                Some(r) => (format!("{:.0}", r.speed), WARN),
                None => (String::new(), TEXT_BRIGHT),
            },
            Readout::AheadDistance => match self.ahead {
                Some(r) if r.speed <= 0.1 => (
                    t!("hud-stop-in", distance = Self::distance(r.distance)),
                    BRAND,
                ),
                Some(r) => (
                    t!("hud-in", distance = Self::distance(r.distance)),
                    TEXT_MID,
                ),
                None => (String::new(), TEXT_MID),
            },

            // --- The interruptions.
            Readout::Alert => (self.alert().unwrap_or_default(), TEXT_BRIGHT),
            // Newest at the top and brightest; the ones behind it step back a tier each,
            // so the column reads in the order the messages arrived without being a list.
            Readout::Message(index) => (
                self.messages
                    .get(index)
                    .map(|m| format!("{} {}", if m.announcement { "»" } else { "•" }, m.text))
                    .unwrap_or_default(),
                match index {
                    0 => TEXT_BRIGHT,
                    1 => TEXT,
                    _ => TEXT_MID,
                },
            ),
            Readout::Hover => (self.hover.clone().unwrap_or_default(), TEXT),

            // --- F6.
            Readout::Diagnostics => (self.diagnostics.clone().unwrap_or_default(), TEXT_MID),
        }
    }

    /// Where a pointer stands, or how far a bar has filled, and in what colour.
    fn gauge(&self, meter: Meter) -> (f32, Color) {
        let scale = self.scale.max(1.0);
        match meter {
            Meter::Speed => (
                (self.train.speed_kmh().abs() / scale) as f32,
                self.speed_tone(),
            ),
            Meter::Limit => ((self.limit.min(scale) / scale) as f32, WARN),
            Meter::Supervision => (
                (self.supervision().unwrap_or(0.0).min(scale) / scale) as f32,
                BRAND,
            ),
            Meter::Power => (self.cab.throttle.max(0.0) as f32, ACCENT),
            Meter::Pipe => ((self.loco.brake.pipe / MANOMETER_MAX) as f32, TEXT_BRIGHT),
            Meter::Reservoir => (
                (self.loco.brake.main_reservoir / MANOMETER_MAX) as f32,
                if self.loco.brake.main_reservoir < LOW_RESERVOIR {
                    WARN
                } else {
                    BRAND
                },
            ),
            Meter::Cylinder => ((self.cylinder() / CYLINDER_MAX) as f32, WARN),
            // Closeness rather than distance: the bar fills as the restriction comes up,
            // which is the direction the urgency runs in.
            Meter::Ahead => match self.ahead {
                Some(r) => (
                    1.0 - (r.distance / LOOKAHEAD).clamp(0.0, 1.0) as f32,
                    if r.speed <= 0.1 { BRAND } else { WARN },
                ),
                None => (0.0, TRACK),
            },
        }
    }

    /// An annunciator's colour: bone white when lit, amber where being lit is itself the
    /// news, and all but invisible when the thing is simply off.
    fn chip(&self, chip: Chip) -> Color {
        let traction = &self.loco.traction;
        let (on, warns) = match chip {
            Chip::Battery => (traction.battery, false),
            Chip::Pantograph => (traction.pantograph > 0.5, false),
            Chip::MainSwitch => (traction.main_switch, false),
            Chip::Compressor => (traction.compressor, false),
            Chip::Parking => (self.loco.brake.parking_applied, true),
            Chip::Sanding => (self.cab.sanding, false),
            Chip::Doors => (!self.runtime.doors.closed_and_locked, true),
            Chip::Lights => (self.cab.headlights, false),
            Chip::Slip => (self.loco.slip.abs() > 0.05, true),
            Chip::Heat => (traction.drives[0].peak_temp() > HOT_MOTOR, true),
        };
        match (on, warns) {
            (true, true) => WARN,
            (true, false) => TEXT_BRIGHT,
            (false, _) => TEXT_FAINT,
        }
    }

    /// Whether a block has anything to say on this vehicle, in this moment.
    fn shows(&self, block: Block) -> bool {
        // The reduced step keeps what the train is driven by and drops what it is
        // planned by. An interruption — the banner, a scenario message — is neither, and
        // stays in every step that draws anything at all.
        if !self.mode.informs()
            && matches!(
                block,
                Block::Journey | Block::Systems | Block::TopWash | Block::Ahead | Block::Hover
            )
        {
            return false;
        }
        match block {
            Block::Journey | Block::Systems | Block::TopWash => true,
            Block::Ribbon => self.stop.is_some(),
            Block::StopRow(index) => self.ribbon(index).is_some(),
            Block::Wedge => self.stop.is_some(),
            Block::Score => !self.sim.score.timetable.stops.is_empty(),
            Block::DriveRow(index) => self.drive_row(index).is_some(),
            Block::Afb => self
                .train
                .vehicles
                .get(self.train.cab)
                .is_some_and(|v| v.spec.afb),
            Block::Safety => !self.indicators.is_empty(),
            Block::Lzb => self.indicators.iter().any(|i| i.name.starts_with("lzb_")),
            Block::LzbValues => self.mfa("mfa_v_soll").is_some(),
            Block::Supervision => self.supervision().is_some(),
            Block::Ahead => self.ahead.is_some(),
            Block::AheadBoard => self.ahead.is_some_and(|r| r.speed > 0.1),
            Block::AheadStop => self.ahead.is_some_and(|r| r.speed <= 0.1),
            Block::Alert => self.alert().is_some(),
            Block::Message(index) => index < self.messages.len(),
            Block::Hover => self.hover.is_some(),
            Block::Help => self.help,
            Block::Diagnostics => self.diagnostics.is_some(),
        }
    }

    /// The F6 block: what the old text HUD printed that a driver has no use for.
    fn diagnose(
        &self,
        terrain: &TerrainInfo,
        streamer: &TerrainStreamer,
        view: &ViewDistance,
        session: Option<&crate::net::Session>,
    ) -> String {
        let drive = self.loco.traction.drives[0];
        let aspects: Vec<String> = self
            .sim
            .interlock
            .signals
            .iter()
            .map(|s| {
                format!(
                    "{}{}",
                    s.aspect.main.map(|m| format!("{m:?}")).unwrap_or_default(),
                    s.aspect
                        .distant
                        .map(|d| format!("/{d:?}"))
                        .unwrap_or_default()
                )
            })
            .collect();
        let mut lines = vec![
            t!(
                "hud-diag-terrain",
                tiles = terrain.0.tiles,
                pending = streamer.pending_tiles(),
                triangles = terrain.0.triangles,
                megabytes = decimal(terrain.0.memory() as f64 / 1e6, 1),
                view = format!("{:.0}", view.0),
            ),
            t!(
                "hud-diag-air",
                auxiliary = decimal(self.loco.brake.aux_reservoir, 2),
                direct = decimal(self.loco.brake.direct_cylinder, 2),
                air = format!("{:.0}", self.loco.brake.air_consumed),
            ),
            t!(
                "hud-diag-axles",
                slipping = self
                    .loco
                    .axles
                    .iter()
                    .filter(|a| a.slip.abs() > 0.02)
                    .count(),
                axles = self.loco.axles.len(),
                worst = decimal(self.loco.slip, 2),
            ),
            t!(
                "hud-diag-temperature",
                motor = format!("{:.0}", drive.motor_temp),
                resistor = format!("{:.0}", drive.resistor_temp.max(drive.brake_resistor_temp)),
            ),
            t!("hud-diag-signals", aspects = aspects.join("  ")),
        ];
        if let Some(session) = session {
            lines.push(t!(
                "hud-diag-network",
                state = t!(if session.joined {
                    "hud-network-joined"
                } else {
                    "hud-network-connecting"
                }),
                latency = format!("{:.0}", session.rtt * 1000.0),
                correction = decimal(session.correction(self.sim.score.train) * 100.0, 1),
            ));
        }
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ribbon is an off-by-one waiting to happen: row 1 has to be the next stop
    /// whatever else is on screen, and the row above it has to be empty at the start of a
    /// run rather than wrapping round to the end of the timetable.
    #[test]
    fn the_ribbon_puts_the_next_stop_on_row_one() {
        assert_eq!(
            ribbon_index(0, 0),
            None,
            "nothing lies behind the first stop"
        );
        assert_eq!(
            ribbon_index(0, 1),
            Some(0),
            "row 1 is the stop being driven to"
        );
        assert_eq!(ribbon_index(0, 2), Some(1));
        assert_eq!(ribbon_index(3, 0), Some(2), "row 0 is the stop just left");
        assert_eq!(ribbon_index(3, 1), Some(3));
        assert_eq!(ribbon_index(3, 3), Some(5));
    }

    /// F7 walks the three steps and comes back round; the settings page walks them the
    /// other way with the same call.
    #[test]
    fn the_display_cycles_through_its_three_steps() {
        let steps = [HudMode::Full, HudMode::Reduced, HudMode::Off];
        let mut mode = HudMode::Full;
        for expected in [HudMode::Reduced, HudMode::Off, HudMode::Full] {
            mode = mode.cycle(1);
            assert_eq!(mode, expected);
        }
        for step in steps {
            assert_eq!(step.cycle(1).cycle(-1), step, "back is the way it came");
        }
        assert!(HudMode::Full.informs());
        assert!(!HudMode::Reduced.informs(), "reduced drops what informs");
        assert!(
            HudMode::Reduced.drawn(),
            "reduced still draws the instruments"
        );
        assert!(!HudMode::Off.drawn());
    }
}
