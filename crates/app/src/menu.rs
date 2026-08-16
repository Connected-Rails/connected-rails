//! Main menu: a navigation column on the left, the page's rows in the middle, what the
//! highlighted row actually is on the right, the game's name above and the key hints below.
//!
//! Four sections hang in the navigation — drive, mods, settings, quit — and the drive
//! section walks line → vehicle → scenario. Keyboard (↑/↓, ←/→, Enter, Esc, Tab) and
//! mouse (hover selects, click confirms) drive the same selection index, so neither input
//! is a special case. The world is built only on leaving the menu, so a mod toggled here
//! takes effect on start — no restart. Any run flag on the command line (`--line`,
//! `--frames`, …) skips the menu entirely, which keeps the documented CLI and CI
//! invocations non-interactive.
//!
//! Two rules hold the look together. **Prose is Fira Sans, machine output is Fira Mono** —
//! names and sentences in the proportional face, ids, versions, metres, per cent and key
//! caps in the fixed one, so figures stay in their columns and the two faces still read as
//! one family. And **a state is a surface, not a decoration**: the selected row is an
//! opaque tier plus an accent bar on its leading edge, never a gradient washing across the
//! row, which leaves its own right-hand end unpainted and drops the second line to an
//! unreadable contrast.
//!
//! The rows are torn down and rebuilt whenever their fingerprint changes rather than
//! patched in place: a row is four nodes deep and carries a different shape per page, and
//! a rebuild costs twenty entities on a menu that is idle the rest of the time. The detail
//! pane is filled in the same pass, which is why looking up a line's length may build it.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use bevy::prelude::*;
use bevy::text::FontSource;
use bevy::ui::ScrollPosition;
use content::musterbahn;
use content::route::LineSource;
use i18n::t;
use sim_core::brakes::BrakeKind;
use sim_core::drive::TractionSpec;
use sim_core::train::VehicleSpec;

use crate::mods_ui::{self, ModManager};
use crate::settings::{self, Audio, Gameplay, Graphics};
use crate::{GameState, Mods};

// Surfaces, opaque and in tiers. Translucent panels over a background gradient measured
// 1.04:1 against each other — three panels nobody can see are not three panels.
/// The page behind everything.
const BASE: Color = Color::srgb(0.055, 0.067, 0.086);
/// Navigation column, footer bar, detail pane.
const PANE: Color = Color::srgb(0.071, 0.090, 0.118);
/// A row at rest …
const ROW: Color = Color::srgb(0.086, 0.110, 0.141);
/// … under the cursor …
const ROW_HOVER: Color = Color::srgb(0.110, 0.141, 0.188);
/// … and the one the selection sits on.
const ROW_ACTIVE: Color = Color::srgb(0.137, 0.173, 0.220);
/// The leading slot of a row, which will hold artwork once there is any.
const SLOT: Color = Color::srgb(0.125, 0.157, 0.196);
/// Slider track and the off state of a toggle.
const TRACK: Color = Color::srgb(0.165, 0.200, 0.247);
/// Key cap in the footer.
const CHIP: Color = Color::srgb(0.118, 0.153, 0.200);
/// Rules and panel edges — opaque, so they do not change with what is behind them.
const HAIRLINE: Color = Color::srgb(0.137, 0.169, 0.208);

/// The one accent: signal amber. Selection, focus and the primary action, nothing else.
const ACCENT: Color = Color::srgb(0.941, 0.663, 0.231);
/// The same amber at the alpha a badge sits on.
const ACCENT_SOFT: Color = Color::srgba(0.941, 0.663, 0.231, 0.14);
/// A mod that is missing a dependency.
const DANGER: Color = Color::srgb(0.878, 0.341, 0.294);

const TEXT_BRIGHT: Color = Color::WHITE;
const TEXT: Color = Color::srgb(0.910, 0.930, 0.949);
const TEXT_MID: Color = Color::srgb(0.659, 0.702, 0.753);
const TEXT_DIM: Color = Color::srgb(0.486, 0.529, 0.580);
const TEXT_FAINT: Color = Color::srgb(0.431, 0.478, 0.533);

/// Height of a row, of a section heading on the settings page, and the gap below either.
/// The list scrolls by the running sum of these, so a heading may be shorter than a row
/// without the keyboard losing track of where a row sits.
const ROW_HEIGHT: f32 = 56.0;
const HEADING_HEIGHT: f32 = 46.0;
const ROW_GAP: f32 = 6.0;

/// Width the list stops growing at — narrow beside the detail pane, wider on the two
/// pages that have none.
const LIST_WIDTH: f32 = 520.0;
const LIST_WIDTH_WIDE: f32 = 760.0;
const DETAIL_WIDTH: f32 = 380.0;

/// The three faces the menu uses. Mono is the app's default font handle, so it needs no
/// handle of its own.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Face {
    Sans,
    Semibold,
    Mono,
}

/// Handles of the two proportional faces, put into `Assets<Font>` while the app is built.
#[derive(Resource, Default, Clone)]
pub struct Fonts {
    pub sans: Handle<Font>,
    pub semibold: Handle<Font>,
}

impl Fonts {
    fn source(&self, face: Face) -> FontSource {
        match face {
            Face::Sans => FontSource::Handle(self.sans.clone()),
            Face::Semibold => FontSource::Handle(self.semibold.clone()),
            Face::Mono => FontSource::default(),
        }
    }
}

/// The player's choices. `None` means the built-in default — `setup` falls back to the
/// example line, the BR 101 and no scenario for exactly that case.
#[derive(Resource, Default, Clone)]
pub struct Selection {
    pub line_ref: Option<String>,
    pub loco_id: Option<String>,
    pub scenario_id: Option<String>,
}

/// An entry of the navigation column.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Drive,
    Mods,
    Settings,
    Quit,
}

/// The navigation column, in order: section and the key of its label.
const NAV: [(Section, &str); 4] = [
    (Section::Drive, "menu-drive"),
    (Section::Mods, "menu-mods"),
    (Section::Settings, "menu-settings"),
    (Section::Quit, "menu-quit"),
];

/// Which list the menu is showing. The drive section walks Line → Loco → Scenario.
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
enum Page {
    #[default]
    Line,
    Loco,
    Scenario,
    Mods,
    Settings,
}

impl Page {
    /// Which navigation entry is lit while this page is open.
    fn section(self) -> Section {
        match self {
            Page::Line | Page::Loco | Page::Scenario => Section::Drive,
            Page::Mods => Section::Mods,
            Page::Settings => Section::Settings,
        }
    }

    /// Where Esc goes. The first page of the drive section is the menu's home and has
    /// nowhere left to go.
    fn back(self) -> Option<Page> {
        match self {
            Page::Line => None,
            Page::Loco => Some(Page::Line),
            Page::Scenario => Some(Page::Loco),
            Page::Mods | Page::Settings => Some(Page::Line),
        }
    }

    /// Position of this page in the drive section's three steps.
    fn step(self) -> Option<usize> {
        match self {
            Page::Line => Some(1),
            Page::Loco => Some(2),
            Page::Scenario => Some(3),
            _ => None,
        }
    }
}

/// Which page the menu shows and which row is selected.
#[derive(Resource, Default)]
pub struct MenuState {
    page: Page,
    selected: usize,
    /// Row under the cursor, which is shown apart from the selection so a mouse and a
    /// keyboard user can both see where they are.
    hovered: Option<usize>,
    /// Set by a click observer, consumed like an Enter press.
    clicked: bool,
    /// Navigation entry a click landed on, consumed like a Tab press.
    nav_click: Option<usize>,
    /// Labels of the line and the vehicle already picked — the drive section shows them
    /// as its breadcrumb, and only the menu ever needs them as text.
    chosen: [String; 2],
    /// The nodes the navigation entries, the rows, the detail pane and the key hints
    /// hang off.
    nav: Option<Entity>,
    list: Option<Entity>,
    detail: Option<Entity>,
    hints: Option<Entity>,
    /// Fingerprint of what is on screen; everything is rebuilt when it changes.
    drawn: Option<u64>,
}

/// Which page `--menu <page>` opens on. A screenshot cannot press keys, so without this
/// only the first page could ever be photographed.
#[derive(Resource)]
pub struct StartPage(pub String);

impl Page {
    fn named(name: &str) -> Option<Page> {
        match name {
            "line" => Some(Page::Line),
            "loco" => Some(Page::Loco),
            "scenario" => Some(Page::Scenario),
            "mods" => Some(Page::Mods),
            "settings" => Some(Page::Settings),
            _ => None,
        }
    }
}

/// A text node of the frame that is refilled every frame.
#[derive(Component)]
pub enum MenuLabel {
    Title,
    Caption,
}

/// One entry of the navigation column, by index into [`NAV`].
#[derive(Component)]
struct NavRow(usize);

/// One row of the content list, by index into the page's entries.
#[derive(Component)]
struct MenuRow(usize);

/// One row of the content list: what it says and what it does.
#[derive(Default)]
struct Entry {
    label: String,
    /// Short second line — key figures of a line, id and version of a mod. Always
    /// machine output, therefore always mono.
    meta: String,
    /// Two letters in the leading slot. A stand-in for the artwork this list will carry
    /// once routes and vehicles ship with images, and the same size as one.
    monogram: String,
    /// Where the row comes from: the simulator itself, or the mod that brought it. What
    /// keeps the built-in example line apart from the mod of the same name.
    chip: String,
    /// A sentence explaining the row, shown on the selected row only — nine hint lines
    /// at once are a wall of prose, one is help.
    hint: String,
    /// Right-hand column of the row.
    value: String,
    /// What this row selects (`None` = the built-in default).
    id: Option<String>,
    /// Something is wrong with this row — a mod missing a dependency. It keeps a red
    /// edge and shows its `hint` whether it is selected or not.
    warning: bool,
    /// A section heading on the settings page: drawn, but never selected.
    heading: bool,
    /// Which setting the row changes, if any …
    setting: Option<Setting>,
    /// … and how it is operated, read off the settings when the row is built.
    control: Option<Control>,
}

impl Entry {
    fn height(&self) -> f32 {
        if self.heading {
            HEADING_HEIGHT
        } else {
            ROW_HEIGHT
        }
    }
}

/// A single adjustable value on the settings page.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Setting {
    ViewDistance,
    Shadows,
    Bloom,
    Fullscreen,
    VSync,
    Volume,
    Language,
    Hud,
    LookSpeed,
    Reset,
}

/// How a setting is operated — which is also how it is drawn, so nothing on the page
/// looks like plain text when it can actually be changed.
enum Control {
    /// Binary, drawn as a pill.
    Toggle(bool),
    /// A range, drawn as a filled track with its value beside it.
    Slider(f32),
    /// One of a handful of named options, drawn between two chevrons.
    Choice,
    /// Does something rather than holding a value.
    Action,
}

/// The settings page: headings and what stands under them.
const SETTINGS: [(&str, &[Setting]); 3] = [
    (
        "set-graphics",
        &[
            Setting::ViewDistance,
            Setting::Shadows,
            Setting::Bloom,
            Setting::Fullscreen,
            Setting::VSync,
        ],
    ),
    ("set-audio", &[Setting::Volume]),
    (
        "set-gameplay",
        &[Setting::Language, Setting::Hud, Setting::LookSpeed],
    ),
];

impl Setting {
    /// The message key of the label; the help line is this plus `-hint`.
    fn key(self) -> &'static str {
        match self {
            Setting::ViewDistance => "set-view-distance",
            Setting::Shadows => "set-shadows",
            Setting::Bloom => "set-bloom",
            Setting::Fullscreen => "set-fullscreen",
            Setting::VSync => "set-vsync",
            Setting::Volume => "set-volume",
            Setting::Language => "set-language",
            Setting::Hud => "set-hud",
            Setting::LookSpeed => "set-look-speed",
            Setting::Reset => "set-reset",
        }
    }

    /// Whether the setting is baked into the scene when the run starts. Those rows carry
    /// a badge instead of repeating the same sentence in three help lines.
    fn needs_restart(self) -> bool {
        matches!(
            self,
            Setting::ViewDistance | Setting::Shadows | Setting::Bloom
        )
    }

    fn control(self, graphics: &Graphics, audio: &Audio, gameplay: &Gameplay) -> Control {
        use settings::{LOOK_SPEED, VIEW_DISTANCE, VOLUME};
        match self {
            Setting::ViewDistance => {
                Control::Slider(fraction(graphics.view_distance, VIEW_DISTANCE))
            }
            Setting::Volume => Control::Slider(fraction(audio.master, VOLUME)),
            Setting::LookSpeed => Control::Slider(fraction(gameplay.look_speed, LOOK_SPEED)),
            Setting::Shadows => Control::Toggle(graphics.shadows),
            Setting::Bloom => Control::Toggle(graphics.bloom),
            Setting::Fullscreen => Control::Toggle(graphics.fullscreen),
            Setting::VSync => Control::Toggle(graphics.vsync),
            Setting::Hud => Control::Toggle(gameplay.hud),
            Setting::Language => Control::Choice,
            Setting::Reset => Control::Action,
        }
    }

    /// What the row shows beside its control. A toggle says it with the pill alone.
    fn value(self, graphics: &Graphics, audio: &Audio, gameplay: &Gameplay) -> String {
        match self {
            Setting::ViewDistance => {
                t!(
                    "set-metres",
                    value = i18n::decimal(f64::from(graphics.view_distance), 0)
                )
            }
            Setting::Volume => t!(
                "set-percent",
                value = i18n::decimal(f64::from(audio.master) * 100.0, 0)
            ),
            Setting::LookSpeed => t!(
                "set-factor",
                value = i18n::decimal(f64::from(gameplay.look_speed), 1)
            ),
            Setting::Language => language_name(&gameplay.language),
            _ => String::new(),
        }
    }
}

/// Where `value` sits in `(min, max, step)`, 0 … 1 — the filled part of a slider.
fn fraction(value: f32, range: (f32, f32, f32)) -> f32 {
    let (min, max, _) = range;
    ((value - min) / (max - min)).clamp(0.0, 1.0)
}

/// Applies one step of ← (`dir` −1) or → / Enter (`dir` +1) to a setting.
fn change(
    setting: Setting,
    dir: i32,
    graphics: &mut Graphics,
    audio: &mut Audio,
    gameplay: &mut Gameplay,
) {
    use settings::{LOOK_SPEED, VIEW_DISTANCE, VOLUME};
    match setting {
        Setting::ViewDistance => {
            graphics.view_distance = step(graphics.view_distance, dir, VIEW_DISTANCE);
        }
        Setting::Shadows => graphics.shadows = !graphics.shadows,
        Setting::Bloom => graphics.bloom = !graphics.bloom,
        Setting::Fullscreen => graphics.fullscreen = !graphics.fullscreen,
        Setting::VSync => graphics.vsync = !graphics.vsync,
        Setting::Volume => audio.master = step(audio.master, dir, VOLUME),
        Setting::Language => {
            gameplay.language = next_language(&gameplay.language, dir);
            settings::apply_language(&gameplay.language);
        }
        Setting::Hud => gameplay.hud = !gameplay.hud,
        Setting::LookSpeed => gameplay.look_speed = step(gameplay.look_speed, dir, LOOK_SPEED),
        Setting::Reset => {
            *graphics = Graphics::default();
            *audio = Audio::default();
            *gameplay = Gameplay::default();
            settings::apply_language(&gameplay.language);
        }
    }
}

/// One step through `(min, max, step)`, clamped to the ends.
fn step(value: f32, dir: i32, range: (f32, f32, f32)) -> f32 {
    let (min, max, by) = range;
    (value + dir as f32 * by).clamp(min, max)
}

/// The language codes the menu cycles through: the system's (empty) plus everything
/// `i18n` ships.
fn next_language(current: &str, dir: i32) -> String {
    let mut codes = vec![""];
    codes.extend(i18n::LANGUAGES.iter().map(|(code, _)| *code));
    let at = codes.iter().position(|c| *c == current).unwrap_or(0) as i32;
    let next = (at + dir).rem_euclid(codes.len() as i32) as usize;
    codes[next].to_string()
}

/// The name a language code is shown under; the empty code is the system's.
fn language_name(code: &str) -> String {
    i18n::LANGUAGES
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, name)| (*name).to_string())
        .unwrap_or_else(|| t!("set-language-system"))
}

fn onoff(on: bool) -> String {
    if on {
        t!("common-on")
    } else {
        t!("common-off")
    }
}

// ---------------------------------------------------------------------------------
// Frame
// ---------------------------------------------------------------------------------

pub fn spawn_menu(
    mut commands: Commands,
    fonts: Res<Fonts>,
    start: Option<Res<StartPage>>,
    mut menu: ResMut<MenuState>,
) {
    // The world with its 3D camera does not exist yet — the menu brings its own.
    commands.spawn((Camera2d, DespawnOnExit(GameState::Menu)));
    let root = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(BASE),
            DespawnOnExit(GameState::Menu),
        ))
        .id();

    // Header: the game's name and what it is. The version moved to the footer — alone in
    // the top right corner it had nine hundred pixels of nothing to its left.
    let header = commands
        .spawn((
            Node {
                height: Val::Px(88.0),
                // Header, rule and footer are fixed furniture. Without this taffy
                // squeezes them the moment the list is longer than the screen, and the
                // whole page creeps upwards as rows are added.
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(5.0),
                padding: UiRect::horizontal(Val::Px(40.0)),
                ..default()
            },
            ChildOf(root),
        ))
        .id();
    commands.spawn((
        text(&fonts, t!("window-simulator"), Face::Semibold, 28.0, TEXT),
        ChildOf(header),
    ));
    commands.spawn((
        text(&fonts, t!("menu-tagline"), Face::Sans, 13.0, TEXT_DIM),
        ChildOf(header),
    ));
    commands.spawn((rule(Val::Percent(100.0)), ChildOf(root)));

    // `min_height: 0` on every flex column below: without it a flex item refuses to
    // shrink under its content, and the scrolling list would push the footer off screen.
    let body = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                ..default()
            },
            ChildOf(root),
        ))
        .id();
    let sidebar = commands
        .spawn((
            Node {
                width: Val::Px(260.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(10.0),
                padding: UiRect::axes(Val::Px(16.0), Val::Px(28.0)),
                border: UiRect::right(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(PANE),
            BorderColor::all(HAIRLINE),
            ChildOf(body),
        ))
        .id();
    commands.spawn((
        text(
            &fonts,
            t!("menu-nav-title").to_uppercase(),
            Face::Semibold,
            11.0,
            TEXT_DIM,
        ),
        Node {
            margin: UiRect::left(Val::Px(16.0)),
            ..default()
        },
        ChildOf(sidebar),
    ));
    let nav = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.0),
                ..default()
            },
            ChildOf(sidebar),
        ))
        .id();

    let content = commands
        .spawn((
            Node {
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                min_height: Val::Px(0.0),
                padding: UiRect::axes(Val::Px(40.0), Val::Px(32.0)),
                row_gap: Val::Px(6.0),
                ..default()
            },
            ChildOf(body),
        ))
        .id();
    commands.spawn((
        text(&fonts, String::new(), Face::Semibold, 20.0, TEXT),
        MenuLabel::Title,
        ChildOf(content),
    ));
    commands.spawn((
        text(&fonts, String::new(), Face::Sans, 13.0, TEXT_DIM),
        MenuLabel::Caption,
        ChildOf(content),
    ));
    let split = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                column_gap: Val::Px(24.0),
                margin: UiRect::top(Val::Px(20.0)),
                ..default()
            },
            ChildOf(content),
        ))
        .id();
    let list = commands
        .spawn((
            list_node(LIST_WIDTH),
            ScrollPosition::default(),
            ChildOf(split),
        ))
        .id();
    let detail = commands.spawn((detail_node(false), ChildOf(split))).id();

    let footer = commands
        .spawn((
            Node {
                height: Val::Px(48.0),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(24.0),
                padding: UiRect::horizontal(Val::Px(40.0)),
                border: UiRect::top(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(PANE),
            BorderColor::all(HAIRLINE),
            ChildOf(root),
        ))
        .id();
    commands.spawn((
        text(
            &fonts,
            format!("v{}", env!("CARGO_PKG_VERSION")),
            Face::Mono,
            11.0,
            TEXT_FAINT,
        ),
        ChildOf(footer),
    ));
    let hints = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(18.0),
                ..default()
            },
            ChildOf(footer),
        ))
        .id();

    let page = start
        .as_ref()
        .and_then(|start| Page::named(&start.0))
        .unwrap_or_default();
    *menu = MenuState {
        nav: Some(nav),
        list: Some(list),
        detail: Some(detail),
        hints: Some(hints),
        page,
        // The settings page opens on a heading; `menu` moves the cursor off it on the
        // first frame, before anything is drawn.
        ..default()
    };
}

/// The list. It stops growing at `width` — a row given the full pane prints "Modul Ost"
/// across nine hundred pixels, which is what left the right-hand third looking empty.
/// The drive pages hand that space to the detail pane; the other two simply run wider.
fn list_node(width: f32) -> Node {
    Node {
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(ROW_GAP),
        flex_grow: 1.0,
        max_width: Val::Px(width),
        min_height: Val::Px(0.0),
        overflow: Overflow::scroll_y(),
        ..default()
    }
}

/// The detail pane, shown on the three drive pages and collapsed on the other two —
/// there is nothing to say about a settings row that the row does not already say.
fn detail_node(shown: bool) -> Node {
    Node {
        display: if shown { Display::Flex } else { Display::None },
        width: Val::Px(DETAIL_WIDTH),
        flex_shrink: 0.0,
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(12.0),
        padding: UiRect::all(Val::Px(20.0)),
        border_radius: BorderRadius::all(Val::Px(8.0)),
        overflow: Overflow::clip(),
        ..default()
    }
}

/// A one pixel hairline. Opaque, so it does not change with what lies behind it.
fn rule(width: Val) -> impl Bundle {
    (
        Node {
            width,
            height: Val::Px(1.0),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(HAIRLINE),
    )
}

fn text(fonts: &Fonts, content: String, face: Face, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(content),
        TextFont {
            font: fonts.source(face),
            font_size: bevy::text::FontSize::Px(size),
            ..default()
        },
        TextColor(color),
    )
}

// ---------------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------------

/// ↑/↓ or hover selects, Enter or left click confirms, ←/→ dials a setting, Esc goes one
/// page back, Tab steps to the next section.
// A Bevy system takes its resources as parameters — the argument count says nothing here.
#[allow(clippy::too_many_arguments)]
pub fn menu(
    keys: Res<ButtonInput<KeyCode>>,
    fonts: Res<Fonts>,
    mut commands: Commands,
    mut menu: ResMut<MenuState>,
    mut selection: ResMut<Selection>,
    mut manager: ResMut<ModManager>,
    mut mods: ResMut<Mods>,
    mut graphics: ResMut<Graphics>,
    mut audio: ResMut<Audio>,
    mut gameplay: ResMut<Gameplay>,
    mut next: ResMut<NextState<GameState>>,
    mut exit: MessageWriter<AppExit>,
    mut labels: Query<(&MenuLabel, &mut Text)>,
    mut lists: Query<(&ComputedNode, &mut ScrollPosition)>,
) {
    let (Some(nav), Some(list), Some(detail), Some(hints)) =
        (menu.nav, menu.list, menu.detail, menu.hints)
    else {
        return;
    };

    // The observers only record what was hit; every state change happens here.
    if let Some(index) = menu.nav_click.take() {
        open(&mut menu, &mut exit, index);
    }
    if keys.just_pressed(KeyCode::Tab) {
        let at = NAV
            .iter()
            .position(|(s, _)| *s == menu.page.section())
            .unwrap_or(0);
        open(&mut menu, &mut exit, (at + 1) % NAV.len());
    }

    let items = entries(menu.page, &mods.0, &graphics, &audio, &gameplay);
    if items.is_empty() {
        menu.selected = 0;
    } else {
        let last = items.len() - 1;
        if keys.just_pressed(KeyCode::ArrowDown) {
            menu.selected = selectable(&items, (menu.selected + 1) % items.len(), 1);
        } else if keys.just_pressed(KeyCode::ArrowUp) {
            menu.selected = selectable(&items, (menu.selected + last) % items.len(), -1);
        } else {
            // A shrinking list (a mod switched off) must not leave the cursor past the
            // end, and a page that opens on a heading must not leave it on one.
            menu.selected = selectable(&items, menu.selected.min(last), 1);
        }
    }

    let confirmed = keys.just_pressed(KeyCode::Enter) || std::mem::take(&mut menu.clicked);
    let dial = i32::from(keys.just_pressed(KeyCode::ArrowRight))
        - i32::from(keys.just_pressed(KeyCode::ArrowLeft));
    let entry = items.get(menu.selected);

    if menu.page == Page::Settings {
        // Enter reads as one step forward, so a row can be worked with the keyboard
        // alone and a click does the obvious thing.
        let dir = if confirmed { 1 } else { dial };
        if dir != 0
            && let Some(setting) = entry.and_then(|e| e.setting)
        {
            change(setting, dir, &mut graphics, &mut audio, &mut gameplay);
        }
    } else if confirmed && let Some(entry) = entry {
        let id = entry.id.clone();
        let label = entry.label.clone();
        match menu.page {
            Page::Line => {
                selection.line_ref = id;
                menu.chosen[0] = label;
                go(&mut menu, Page::Loco);
            }
            Page::Loco => {
                selection.loco_id = id;
                menu.chosen[1] = label;
                go(&mut menu, Page::Scenario);
            }
            Page::Scenario => {
                selection.scenario_id = id;
                next.set(GameState::Driving);
            }
            Page::Mods => {
                mods_ui::toggle(&mut mods.0, menu.selected, &mut manager);
                // Reload right away, so the selection lists show what is enabled now.
                // Every mod stays in `manifests` either way, so the row keeps its index.
                mods.0 = mod_runtime::ModRuntime::load("mods");
            }
            Page::Settings => {}
        }
    }
    if keys.just_pressed(KeyCode::Escape)
        && let Some(back) = menu.page.back()
    {
        go(&mut menu, back);
    }

    // The page and the values may have changed above — re-read before drawing.
    let page = menu.page;
    let items = entries(page, &mods.0, &graphics, &audio, &gameplay);
    let print = fingerprint(page, &items, menu.selected, menu.hovered);
    if menu.drawn != Some(print) {
        build_nav(&mut commands, &fonts, nav, page, &menu.chosen);
        build_rows(&mut commands, &fonts, list, &items, &menu);
        build_detail(
            &mut commands,
            &fonts,
            detail,
            list,
            page,
            items.get(menu.selected),
            &mods.0,
        );
        build_hints(&mut commands, &fonts, hints, page);
        menu.drawn = Some(print);
    }

    for (label, mut text) in &mut labels {
        let content = match label {
            MenuLabel::Title => title(page),
            MenuLabel::Caption => caption(page, &mods.0, &manager),
        };
        if **text != content {
            **text = content;
        }
    }

    scroll_into_view(&mut lists, list, menu.selected, &items);
}

/// Opens the n-th navigation entry.
fn open(menu: &mut MenuState, exit: &mut MessageWriter<AppExit>, index: usize) {
    match NAV.get(index).map(|(section, _)| *section) {
        Some(Section::Drive) => go(menu, Page::Line),
        Some(Section::Mods) => go(menu, Page::Mods),
        Some(Section::Settings) => go(menu, Page::Settings),
        Some(Section::Quit) => {
            exit.write(AppExit::Success);
        }
        None => {}
    }
}

fn go(menu: &mut MenuState, page: Page) {
    menu.page = page;
    menu.selected = 0;
    menu.hovered = None;
}

/// The first selectable row from `from` in direction `dir`, wrapping. The settings page
/// is the only one with headings, which are drawn but never land on the cursor.
fn selectable(items: &[Entry], from: usize, dir: i32) -> usize {
    let count = items.len();
    let mut at = from;
    for _ in 0..count {
        if !items[at].heading {
            return at;
        }
        at = (at as i32 + dir).rem_euclid(count as i32) as usize;
    }
    from
}

/// Hovering a row selects it — the cursor and ↑/↓ share one index — and additionally
/// marks it as hovered, so the two are told apart on screen.
fn on_row_over(over: On<Pointer<Over>>, rows: Query<&MenuRow>, mut menu: ResMut<MenuState>) {
    if let Ok(MenuRow(i)) = rows.get(over.event().entity) {
        menu.selected = *i;
        menu.hovered = Some(*i);
    }
}

fn on_row_out(out: On<Pointer<Out>>, rows: Query<&MenuRow>, mut menu: ResMut<MenuState>) {
    if let Ok(MenuRow(i)) = rows.get(out.event().entity)
        && menu.hovered == Some(*i)
    {
        menu.hovered = None;
    }
}

fn on_row_click(click: On<Pointer<Click>>, rows: Query<&MenuRow>, mut menu: ResMut<MenuState>) {
    if click.event().event.button != PointerButton::Primary {
        return;
    }
    if let Ok(MenuRow(i)) = rows.get(click.event().entity) {
        menu.selected = *i;
        menu.clicked = true;
    }
}

/// The button in the detail pane does exactly what Enter on the selected row does.
fn on_action_click(click: On<Pointer<Click>>, mut menu: ResMut<MenuState>) {
    if click.event().event.button == PointerButton::Primary {
        menu.clicked = true;
    }
}

fn on_nav_click(click: On<Pointer<Click>>, rows: Query<&NavRow>, mut menu: ResMut<MenuState>) {
    if click.event().event.button != PointerButton::Primary {
        return;
    }
    if let Ok(NavRow(i)) = rows.get(click.event().entity) {
        menu.nav_click = Some(*i);
    }
}

/// Everything that is drawn, in one number — the rows are rebuilt when it changes.
fn fingerprint(page: Page, items: &[Entry], selected: usize, hovered: Option<usize>) -> u64 {
    let mut hasher = DefaultHasher::new();
    (page as u8, selected, hovered, items.len()).hash(&mut hasher);
    for entry in items {
        (
            &entry.label,
            &entry.meta,
            &entry.chip,
            &entry.value,
            entry.heading,
        )
            .hash(&mut hasher);
    }
    hasher.finish()
}

/// Keeps the selected row inside the list's viewport. Rows and headings differ in height,
/// so where the n-th one sits is the running sum of the ones above it.
fn scroll_into_view(
    lists: &mut Query<(&ComputedNode, &mut ScrollPosition)>,
    list: Entity,
    selected: usize,
    items: &[Entry],
) {
    let Ok((node, mut scroll)) = lists.get_mut(list) else {
        return;
    };
    let view = node.size().y * node.inverse_scale_factor();
    // Before the first layout the node has no size yet. Scrolling against that would park
    // the list somewhere below the top, and the rule below only ever scrolls as far as it
    // has to — it would never find its way back.
    if view <= 1.0 {
        return;
    }
    let offset = |upto: usize| -> f32 {
        items
            .iter()
            .take(upto)
            .map(|e| e.height() + ROW_GAP)
            .sum::<f32>()
    };
    let top = offset(selected);
    let bottom = top + items.get(selected).map_or(ROW_HEIGHT, Entry::height);
    let limit = (offset(items.len()) - ROW_GAP - view).max(0.0);
    let wanted = scroll.0.y.min(top).max(bottom - view).clamp(0.0, limit);
    if (scroll.0.y - wanted).abs() > 0.5 {
        scroll.0.y = wanted;
    }
}

// ---------------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------------

/// The three steps of the drive section, in order: their titles, and which of them the
/// player has already answered.
const STEPS: [&str; 3] = [
    "menu-select-line",
    "menu-select-loco",
    "menu-select-scenario",
];

fn build_nav(
    commands: &mut Commands,
    fonts: &Fonts,
    nav: Entity,
    page: Page,
    chosen: &[String; 2],
) {
    commands.entity(nav).despawn_related::<Children>();
    for (i, (section, key)) in NAV.iter().enumerate() {
        let on = *section == page.section();
        let row = commands
            .spawn((
                Node {
                    height: Val::Px(44.0),
                    flex_shrink: 0.0,
                    justify_content: JustifyContent::Center,
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::left(Val::Px(13.0)),
                    border: UiRect::left(Val::Px(3.0)),
                    border_radius: BorderRadius::all(Val::Px(6.0)),
                    ..default()
                },
                BorderColor::all(if on { ACCENT } else { Color::NONE }),
                BackgroundColor(if on { ROW_ACTIVE } else { Color::NONE }),
                NavRow(i),
                ChildOf(nav),
            ))
            .observe(on_nav_click)
            .id();
        commands.spawn((
            text(
                fonts,
                t!(key),
                if on { Face::Semibold } else { Face::Sans },
                15.0,
                if on { TEXT_BRIGHT } else { TEXT_MID },
            ),
            ChildOf(row),
        ));
        // The drive section unfolds into its three steps, each showing what was picked
        // for it. That is the breadcrumb the "step 1 of 3" line used to stand in for —
        // and it says at a glance which answers are already in.
        if on && *section == Section::Drive {
            for (step, title) in STEPS.iter().enumerate() {
                let here = page.step() == Some(step + 1);
                let answer = chosen.get(step).filter(|c| !c.is_empty());
                let node = commands
                    .spawn((
                        Node {
                            height: Val::Px(26.0),
                            flex_shrink: 0.0,
                            justify_content: JustifyContent::Center,
                            padding: UiRect::left(Val::Px(29.0)),
                            ..default()
                        },
                        ChildOf(nav),
                    ))
                    .id();
                commands.spawn((
                    text(
                        fonts,
                        answer.cloned().unwrap_or_else(|| t!(title)),
                        Face::Sans,
                        12.0,
                        match (here, answer.is_some()) {
                            (true, _) => ACCENT,
                            (false, true) => TEXT_MID,
                            (false, false) => TEXT_FAINT,
                        },
                    ),
                    ChildOf(node),
                ));
            }
        }
    }
}

fn build_rows(
    commands: &mut Commands,
    fonts: &Fonts,
    list: Entity,
    items: &[Entry],
    menu: &MenuState,
) {
    commands.entity(list).despawn_related::<Children>();
    for (i, entry) in items.iter().enumerate() {
        if entry.heading {
            build_heading(commands, fonts, list, i, entry);
            continue;
        }
        let on = i == menu.selected;
        let row = commands
            .spawn((
                Node {
                    height: Val::Px(ROW_HEIGHT),
                    // Without this taffy squeezes fourteen settings rows into the height
                    // of ten instead of letting the list scroll, and every row loses a
                    // third of its height.
                    flex_shrink: 0.0,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(12.0),
                    padding: UiRect::horizontal(Val::Px(12.0)),
                    border: UiRect::left(Val::Px(3.0)),
                    border_radius: BorderRadius::all(Val::Px(6.0)),
                    ..default()
                },
                BorderColor::all(match (on, entry.warning) {
                    (true, _) => ACCENT,
                    (false, true) => DANGER,
                    (false, false) => Color::NONE,
                }),
                BackgroundColor(match (on, menu.hovered == Some(i)) {
                    (true, _) => ROW_ACTIVE,
                    (false, true) => ROW_HOVER,
                    (false, false) => ROW,
                }),
                MenuRow(i),
                ChildOf(list),
            ))
            .observe(on_row_over)
            .observe(on_row_out)
            .observe(on_row_click)
            .id();

        if !entry.monogram.is_empty() {
            let slot = commands
                .spawn((
                    Node {
                        width: Val::Px(40.0),
                        height: Val::Px(40.0),
                        flex_shrink: 0.0,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border_radius: BorderRadius::all(Val::Px(6.0)),
                        ..default()
                    },
                    BackgroundColor(SLOT),
                    ChildOf(row),
                ))
                .id();
            commands.spawn((
                text(fonts, entry.monogram.clone(), Face::Mono, 13.0, TEXT_MID),
                ChildOf(slot),
            ));
        }

        let column = commands
            .spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    row_gap: Val::Px(3.0),
                    ..default()
                },
                ChildOf(row),
            ))
            .id();
        let line = commands
            .spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(8.0),
                    ..default()
                },
                ChildOf(column),
            ))
            .id();
        commands.spawn((
            text(
                fonts,
                entry.label.clone(),
                if on { Face::Semibold } else { Face::Sans },
                15.0,
                if on { TEXT_BRIGHT } else { TEXT },
            ),
            ChildOf(line),
        ));
        if !entry.chip.is_empty() {
            build_chip(commands, fonts, line, &entry.chip, TEXT_DIM, Color::NONE);
        }
        if entry.setting.is_some_and(Setting::needs_restart) {
            build_chip(
                commands,
                fonts,
                line,
                &t!("set-restart-badge"),
                ACCENT,
                ACCENT_SOFT,
            );
        }
        // The help line belongs to the row the cursor is on. All nine at once is a wall
        // of prose that nobody reads.
        let second = if entry.warning {
            Some((entry.hint.clone(), Face::Sans, DANGER))
        } else if on && !entry.hint.is_empty() {
            Some((entry.hint.clone(), Face::Sans, TEXT_MID))
        } else if !entry.meta.is_empty() {
            Some((entry.meta.clone(), Face::Mono, TEXT_DIM))
        } else {
            None
        };
        if let Some((content, face, color)) = second {
            commands.spawn((text(fonts, content, face, 12.0, color), ChildOf(column)));
        }

        if let Some(control) = &entry.control {
            build_control(commands, fonts, row, control, entry, on);
        } else if !entry.value.is_empty() {
            commands.spawn((
                text(
                    fonts,
                    entry.value.clone(),
                    Face::Mono,
                    13.0,
                    if on { ACCENT } else { TEXT_DIM },
                ),
                ChildOf(row),
            ));
        }
    }
}

/// A section heading on the settings page: a small caps label and a rule filling the
/// rest of the line, so the groups read as groups without drawing boxes around them.
fn build_heading(
    commands: &mut Commands,
    fonts: &Fonts,
    list: Entity,
    index: usize,
    entry: &Entry,
) {
    let heading = commands
        .spawn((
            Node {
                height: Val::Px(HEADING_HEIGHT),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(12.0),
                padding: UiRect::new(Val::Px(3.0), Val::Px(0.0), Val::Px(18.0), Val::Px(0.0)),
                ..default()
            },
            MenuRow(index),
            ChildOf(list),
        ))
        .id();
    commands.spawn((
        text(
            fonts,
            entry.label.to_uppercase(),
            Face::Semibold,
            11.0,
            TEXT_DIM,
        ),
        ChildOf(heading),
    ));
    commands.spawn((
        Node {
            flex_grow: 1.0,
            height: Val::Px(1.0),
            ..default()
        },
        BackgroundColor(HAIRLINE),
        ChildOf(heading),
    ));
}

/// A small pill: the mod a row came from, or the note that a setting waits for the next
/// run. Uppercase at 10 px needs the extra padding to stop looking like a grey brick.
fn build_chip(
    commands: &mut Commands,
    fonts: &Fonts,
    parent: Entity,
    label: &str,
    color: Color,
    background: Color,
) {
    let chip = commands
        .spawn((
            Node {
                padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(3.0)),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(background),
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        text(fonts, label.to_uppercase(), Face::Semibold, 10.0, color),
        ChildOf(chip),
    ));
}

/// The right-hand zone of a settings row. Every setting gets something that looks
/// operable — a pill, a track, a pair of chevrons — instead of naked text at the margin.
fn build_control(
    commands: &mut Commands,
    fonts: &Fonts,
    row: Entity,
    control: &Control,
    entry: &Entry,
    on: bool,
) {
    let zone = commands
        .spawn((
            Node {
                width: Val::Px(230.0),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::End,
                column_gap: Val::Px(10.0),
                ..default()
            },
            ChildOf(row),
        ))
        .id();
    match control {
        Control::Toggle(state) => {
            let pill = commands
                .spawn((
                    Node {
                        width: Val::Px(40.0),
                        height: Val::Px(22.0),
                        flex_shrink: 0.0,
                        align_items: AlignItems::Center,
                        justify_content: if *state {
                            JustifyContent::End
                        } else {
                            JustifyContent::Start
                        },
                        padding: UiRect::all(Val::Px(3.0)),
                        border_radius: BorderRadius::all(Val::Px(11.0)),
                        ..default()
                    },
                    BackgroundColor(if *state { ACCENT } else { TRACK }),
                    ChildOf(zone),
                ))
                .id();
            commands.spawn((
                Node {
                    width: Val::Px(16.0),
                    height: Val::Px(16.0),
                    border_radius: BorderRadius::all(Val::Px(8.0)),
                    ..default()
                },
                BackgroundColor(if *state { BASE } else { TEXT_DIM }),
                ChildOf(pill),
            ));
        }
        Control::Slider(filled) => {
            let track = commands
                .spawn((
                    Node {
                        width: Val::Px(140.0),
                        height: Val::Px(4.0),
                        flex_shrink: 0.0,
                        border_radius: BorderRadius::all(Val::Px(2.0)),
                        ..default()
                    },
                    BackgroundColor(TRACK),
                    ChildOf(zone),
                ))
                .id();
            commands.spawn((
                Node {
                    width: Val::Percent(filled * 100.0),
                    height: Val::Percent(100.0),
                    border_radius: BorderRadius::all(Val::Px(2.0)),
                    ..default()
                },
                BackgroundColor(if on { ACCENT } else { TEXT_DIM }),
                ChildOf(track),
            ));
            build_value(commands, fonts, zone, &entry.value, on, 66.0);
        }
        Control::Choice => {
            commands.spawn((
                text(fonts, "‹".into(), Face::Mono, 14.0, TEXT_DIM),
                ChildOf(zone),
            ));
            build_value(commands, fonts, zone, &entry.value, on, 96.0);
            commands.spawn((
                text(fonts, "›".into(), Face::Mono, 14.0, TEXT_DIM),
                ChildOf(zone),
            ));
        }
        Control::Action => {
            commands.spawn((
                text(
                    fonts,
                    "▸".into(),
                    Face::Mono,
                    14.0,
                    if on { ACCENT } else { TEXT_DIM },
                ),
                ChildOf(zone),
            ));
        }
    }
}

/// The numeric readout beside a control — mono in a fixed-width box, so the digits sit
/// still while the value is dialled.
fn build_value(
    commands: &mut Commands,
    fonts: &Fonts,
    zone: Entity,
    value: &str,
    on: bool,
    width: f32,
) {
    let box_ = commands
        .spawn((
            Node {
                width: Val::Px(width),
                flex_shrink: 0.0,
                justify_content: JustifyContent::End,
                ..default()
            },
            ChildOf(zone),
        ))
        .id();
    commands.spawn((
        text(
            fonts,
            value.to_string(),
            Face::Mono,
            13.0,
            if on { ACCENT } else { TEXT_MID },
        ),
        ChildOf(box_),
    ));
}

/// The key hints, as separate chips rather than one string padded with double spaces —
/// those collapse the moment the face is proportional.
fn build_hints(commands: &mut Commands, fonts: &Fonts, hints: Entity, page: Page) {
    commands.entity(hints).despawn_related::<Children>();
    let mut keys: Vec<(&str, &str)> = vec![("↑/↓", "menu-hint-select")];
    if page == Page::Settings {
        keys.push(("←/→", "menu-hint-change"));
        keys.push(("Enter", "menu-hint-next"));
    } else {
        keys.push((
            "Enter",
            match page {
                Page::Scenario => "menu-hint-start",
                Page::Mods => "menu-hint-toggle",
                _ => "menu-hint-confirm",
            },
        ));
    }
    keys.push(("Esc", "menu-hint-back"));
    keys.push(("Tab", "menu-hint-section"));
    for (cap, label) in keys {
        let chip = commands
            .spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(7.0),
                    ..default()
                },
                ChildOf(hints),
            ))
            .id();
        let cap_box = commands
            .spawn((
                Node {
                    padding: UiRect::axes(Val::Px(7.0), Val::Px(3.0)),
                    border_radius: BorderRadius::all(Val::Px(4.0)),
                    ..default()
                },
                BackgroundColor(CHIP),
                ChildOf(chip),
            ))
            .id();
        commands.spawn((
            text(fonts, cap.into(), Face::Mono, 11.0, TEXT_MID),
            ChildOf(cap_box),
        ));
        commands.spawn((
            text(fonts, t!(label), Face::Sans, 12.0, TEXT_DIM),
            ChildOf(chip),
        ));
    }
}

/// What the highlighted row actually is: a reserved image box, the name, and the figures
/// out of the content itself. Nothing here is invented for the menu — length, mass,
/// braked kind and start time are read off the same data the simulation runs on.
#[allow(clippy::too_many_arguments)]
fn build_detail(
    commands: &mut Commands,
    fonts: &Fonts,
    detail: Entity,
    list: Entity,
    page: Page,
    entry: Option<&Entry>,
    runtime: &mod_runtime::ModRuntime,
) {
    commands.entity(detail).despawn_related::<Children>();
    let Some(facts) = entry.and_then(|entry| facts(page, entry, runtime)) else {
        commands.entity(detail).insert(detail_node(false));
        commands.entity(list).insert(list_node(LIST_WIDTH_WIDE));
        return;
    };
    commands
        .entity(detail)
        .insert((detail_node(true), BackgroundColor(PANE)));
    commands.entity(list).insert(list_node(LIST_WIDTH));

    // The box the route or vehicle image goes in once there is one. Empty it still gives
    // the pane a top edge and keeps the layout honest about what is missing.
    let plate = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(170.0),
                // The one thing in the pane that may give up space: a long scenario
                // description must not push the button off the bottom edge.
                min_height: Val::Px(70.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(Val::Px(6.0)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(ROW),
            ChildOf(detail),
        ))
        .id();
    commands.spawn((
        text(fonts, facts.monogram, Face::Mono, 42.0, SLOT),
        ChildOf(plate),
    ));

    commands.spawn((
        text(fonts, facts.title, Face::Semibold, 18.0, TEXT),
        ChildOf(detail),
    ));
    if !facts.body.is_empty() {
        commands.spawn((
            text(fonts, facts.body, Face::Sans, 13.0, TEXT_MID),
            ChildOf(detail),
        ));
    }
    let table = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                flex_shrink: 0.0,
                ..default()
            },
            ChildOf(detail),
        ))
        .id();
    for (i, (label, value)) in facts.rows.iter().enumerate() {
        if i > 0 {
            commands.spawn((rule(Val::Percent(100.0)), ChildOf(table)));
        }
        let line = commands
            .spawn((
                Node {
                    height: Val::Px(30.0),
                    flex_shrink: 0.0,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                    ..default()
                },
                ChildOf(table),
            ))
            .id();
        commands.spawn((
            text(fonts, label.clone(), Face::Sans, 12.0, TEXT_DIM),
            ChildOf(line),
        ));
        commands.spawn((
            text(fonts, value.clone(), Face::Mono, 13.0, TEXT),
            ChildOf(line),
        ));
    }

    // The primary action, pinned to the bottom of the pane: what Enter does, said in
    // words and clickable. The one saturated fill on the screen besides the selection.
    let action = commands
        .spawn((
            Node {
                height: Val::Px(40.0),
                flex_shrink: 0.0,
                margin: UiRect::top(Val::Auto),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(Val::Px(5.0)),
                ..default()
            },
            BackgroundColor(ACCENT),
            ChildOf(detail),
        ))
        .observe(on_action_click)
        .id();
    commands.spawn((
        text(
            fonts,
            t!(if page == Page::Scenario {
                "menu-action-start"
            } else {
                "menu-action-next"
            }),
            Face::Semibold,
            14.0,
            BASE,
        ),
        ChildOf(action),
    ));
}

// ---------------------------------------------------------------------------------
// Content
// ---------------------------------------------------------------------------------

fn title(page: Page) -> String {
    match page {
        Page::Line => t!("menu-select-line"),
        Page::Loco => t!("menu-select-loco"),
        Page::Scenario => t!("menu-select-scenario"),
        Page::Mods => t!("mods-title"),
        Page::Settings => t!("menu-settings"),
    }
}

/// The line under the title: how far through the drive section we are, what the mods
/// contribute, or where the settings file lives.
fn caption(page: Page, runtime: &mod_runtime::ModRuntime, manager: &ModManager) -> String {
    match page {
        Page::Settings => t!("set-stored"),
        Page::Mods => mods_ui::details(runtime, manager, true),
        _ => t!("menu-step", step = page.step().unwrap_or(1), total = 3),
    }
}

/// Two letters standing in for artwork: the initials of the name.
fn monogram(name: &str) -> String {
    let letters: String = name
        .split_whitespace()
        .filter_map(|word| word.chars().next())
        .take(2)
        .collect();
    if letters.is_empty() {
        name.chars().take(2).collect::<String>().to_uppercase()
    } else {
        letters.to_uppercase()
    }
}

/// The mod an id belongs to (`example:modul_ost` → `example`).
fn origin(id: &str) -> String {
    id.split_once(':').map_or(id, |(m, _)| m).to_string()
}

/// The rows of a page. Every selection page opens with the built-in default, so the list
/// is never empty and the run starts even with no mod installed.
fn entries(
    page: Page,
    runtime: &mod_runtime::ModRuntime,
    graphics: &Graphics,
    audio: &Audio,
    gameplay: &Gameplay,
) -> Vec<Entry> {
    let mods = &runtime.mods;
    // The row every selection page opens with. It carries a chip rather than "(built-in)"
    // in its name, so it no longer reads as a second mod of the same name.
    let builtin = |key: &str, meta: String| Entry {
        label: t!(key),
        meta,
        monogram: monogram(&t!(key)),
        chip: t!("menu-chip-builtin"),
        ..default()
    };
    match page {
        // Lines and compositions share one list: `resolve_line` takes either name, and
        // the player is picking a route, not a file format.
        Page::Line => std::iter::once(builtin("menu-line-builtin", line_meta(&musterbahn())))
            .chain(
                mods.lines
                    .iter()
                    .map(|(id, line)| named(id, &line.name, line_meta(line))),
            )
            .chain(mods.compositions.iter().map(|(id, composition)| Entry {
                chip: t!("menu-chip-composition"),
                ..named(id, &composition.name, String::new())
            }))
            .collect(),
        Page::Loco => std::iter::once(builtin(
            "menu-loco-builtin",
            vehicle_meta(&content::vehicles::br101()),
        ))
        .chain(
            mods.vehicles
                .iter()
                .map(|(id, spec)| named(id, &spec.name, vehicle_meta(spec))),
        )
        .collect(),
        Page::Scenario => std::iter::once(builtin("menu-scenario-none", String::new()))
            .chain(mods.scenarios.iter().map(|(id, scenario)| {
                named(
                    id,
                    &scenario.name,
                    format!("{:02}:{:02}", scenario.start.hour, scenario.start.minute),
                )
            }))
            .collect(),
        // A row for the hint keeps the page from being blank; toggling it is a no-op.
        Page::Mods if mods.manifests.is_empty() => vec![Entry {
            label: t!("mods-none"),
            ..default()
        }],
        Page::Mods => mods
            .manifests
            .iter()
            .map(|manifest| {
                let missing = mods.missing_depends(&manifest.id);
                Entry {
                    label: manifest.name.clone(),
                    meta: format!("{}  {}", manifest.id, manifest.version),
                    monogram: monogram(&manifest.name),
                    hint: if missing.is_empty() {
                        String::new()
                    } else {
                        t!("mods-missing-depends", depends = missing.join(", "))
                    },
                    warning: !missing.is_empty(),
                    // The same pill the settings page uses — a mod is on or off, and it
                    // should not look like a different kind of switch here.
                    // `value` is what the fingerprint notices when it flips.
                    value: onoff(manifest.enabled),
                    control: Some(Control::Toggle(manifest.enabled)),
                    ..default()
                }
            })
            .collect(),
        Page::Settings => SETTINGS
            .iter()
            .flat_map(|(heading, group)| {
                std::iter::once(Entry {
                    label: t!(heading),
                    heading: true,
                    ..default()
                })
                .chain(group.iter().map(|setting| Entry {
                    label: t!(setting.key()),
                    hint: t!(&format!("{}-hint", setting.key())),
                    value: setting.value(graphics, audio, gameplay),
                    setting: Some(*setting),
                    control: Some(setting.control(graphics, audio, gameplay)),
                    ..default()
                }))
            })
            .chain(std::iter::once(Entry {
                label: t!(Setting::Reset.key()),
                hint: t!("set-reset-hint"),
                setting: Some(Setting::Reset),
                control: Some(Control::Action),
                ..default()
            }))
            .collect(),
    }
}

fn named(id: &str, name: &str, meta: String) -> Entry {
    Entry {
        label: name.to_string(),
        meta,
        monogram: monogram(name),
        chip: origin(id),
        id: Some(id.to_string()),
        ..default()
    }
}

/// `12,4 km · 3 Signale` — the second line of a line's row.
fn line_meta(line: &LineSource) -> String {
    t!(
        "menu-meta-line",
        length = i18n::decimal(line_length(line) / 1000.0, 1),
        signals = line.signals.len()
    )
}

/// `84 t · 220 km/h` — the second line of a vehicle's row.
fn vehicle_meta(spec: &VehicleSpec) -> String {
    t!(
        "menu-meta-vehicle",
        mass = i18n::decimal(spec.mass_empty / 1000.0, 0),
        speed = i18n::decimal(spec.v_max, 0)
    )
}

/// Length of a line [m] — the arc length of every segment of every edge.
fn line_length(line: &LineSource) -> f64 {
    line.edges
        .iter()
        .flat_map(|edge| edge.segments.iter())
        .map(|segment| segment.len)
        .sum()
}

/// Highest permitted speed anywhere on the line [km/h], or `None` where the line states
/// none and the default applies.
fn line_speed(line: &LineSource) -> Option<f64> {
    line.edges
        .iter()
        .flat_map(|edge| edge.speed.iter())
        .map(|(_, v)| *v)
        .max_by(f64::total_cmp)
}

/// Everything the detail pane shows about one row.
struct Facts {
    title: String,
    monogram: String,
    body: String,
    rows: Vec<(String, String)>,
}

/// Looks the highlighted row up in the loaded content. `None` on the pages that have no
/// detail pane, and for the "no scenario" row, which is the absence of a choice.
fn facts(page: Page, entry: &Entry, runtime: &mod_runtime::ModRuntime) -> Option<Facts> {
    let mods = &runtime.mods;
    let base = |rows| Facts {
        title: entry.label.clone(),
        monogram: entry.monogram.clone(),
        body: String::new(),
        rows,
    };
    match page {
        Page::Line => {
            let owned;
            let line = match &entry.id {
                Some(id) => match mods.lines.get(id) {
                    Some(line) => line,
                    // A composition, whose figures only exist once its modules are put
                    // together. The pane keeps its place rather than folding away.
                    None => return Some(base(Vec::new())),
                },
                None => {
                    owned = musterbahn();
                    &owned
                }
            };
            let mut rows = vec![
                (
                    t!("menu-fact-length"),
                    t!(
                        "menu-fact-km",
                        value = i18n::decimal(line_length(line) / 1000.0, 1)
                    ),
                ),
                (t!("menu-fact-signals"), line.signals.len().to_string()),
                (t!("menu-fact-scenery"), line.objects.len().to_string()),
            ];
            if let Some(speed) = line_speed(line) {
                rows.insert(
                    1,
                    (
                        t!("veh-vmax"),
                        t!("menu-fact-kmh", value = i18n::decimal(speed, 0)),
                    ),
                );
            }
            Some(base(rows))
        }
        Page::Loco => {
            let owned;
            let spec = match &entry.id {
                Some(id) => match mods.vehicles.get(id) {
                    Some(spec) => spec,
                    None => return Some(base(Vec::new())),
                },
                None => {
                    owned = content::vehicles::br101();
                    &owned
                }
            };
            Some(base(vec![
                (
                    t!("veh-length"),
                    t!("menu-fact-m", value = i18n::decimal(spec.length, 1)),
                ),
                (
                    t!("veh-mass"),
                    t!(
                        "menu-fact-t",
                        value = i18n::decimal(spec.mass_empty / 1000.0, 0)
                    ),
                ),
                (
                    t!("veh-vmax"),
                    t!("menu-fact-kmh", value = i18n::decimal(spec.v_max, 0)),
                ),
                (t!("menu-fact-drive"), t!(traction_key(&spec.traction))),
                (t!("menu-fact-brake"), t!(friction_key(&spec.brake.kind))),
            ]))
        }
        Page::Scenario => {
            // The free run is the absence of a scenario, not a missing one — it gets the
            // pane and the start button like every other row.
            let Some(scenario) = entry.id.as_ref().and_then(|id| mods.scenarios.get(id)) else {
                return Some(Facts {
                    body: t!("menu-free-run"),
                    ..base(Vec::new())
                });
            };
            let start = &scenario.start;
            Some(Facts {
                body: scenario.description.clone(),
                rows: vec![
                    (
                        t!("menu-fact-start"),
                        format!(
                            "{:02}:{:02}  {:02}.{:02}.{}",
                            start.hour, start.minute, start.day, start.month, start.year
                        ),
                    ),
                    (
                        t!("menu-fact-timetable"),
                        scenario
                            .timetable
                            .clone()
                            .unwrap_or_else(|| t!("common-none")),
                    ),
                    (
                        t!("menu-fact-line"),
                        scenario.line.clone().unwrap_or_else(|| t!("common-none")),
                    ),
                    (t!("menu-fact-events"), scenario.events.len().to_string()),
                ],
                ..base(Vec::new())
            })
        }
        Page::Mods | Page::Settings => None,
    }
}

/// The same mapping the vehicle editor uses, so a drive is named identically in both.
fn traction_key(traction: &Option<TractionSpec>) -> &'static str {
    match traction {
        None => "traction-none",
        Some(TractionSpec::Curve { .. }) => "traction-curve",
        Some(TractionSpec::TapChanger { .. }) => "traction-tap",
        Some(TractionSpec::Converter { .. }) => "traction-converter",
        Some(TractionSpec::Diesel { .. }) => "traction-diesel",
    }
}

fn friction_key(kind: &BrakeKind) -> &'static str {
    match kind {
        BrakeKind::Block => "friction-block",
        BrakeKind::Disc => "friction-disc",
        BrakeKind::CompositeK => "friction-k",
        BrakeKind::CompositeLl => "friction-ll",
        BrakeKind::Magnetic => "friction-magnetic",
        BrakeKind::Custom(_) => "friction-custom",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<MenuState>()
            .init_resource::<Selection>()
            .init_resource::<ModManager>()
            .init_resource::<Graphics>()
            .init_resource::<Audio>()
            .init_resource::<Gameplay>()
            .init_resource::<Fonts>()
            // The example mod is the run's content; a missing directory is not an error,
            // so this also covers the "no mods installed" case on CI.
            .insert_resource(Mods(mod_runtime::ModRuntime::load("../../mods")))
            .init_state::<GameState>()
            .add_systems(Startup, spawn_menu)
            .add_systems(Update, menu);
        app.update();
        app
    }

    /// `press` only counts as `just_pressed` while the key was up — the release has to be
    /// simulated too, or the second Enter is silently swallowed.
    fn key(app: &mut App, code: KeyCode) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(code);
        app.update();
        let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        input.reset(code);
        input.clear();
        app.update();
    }

    fn page(app: &App) -> Page {
        app.world().resource::<MenuState>().page
    }

    fn loaded() -> (mod_runtime::ModRuntime, Graphics, Audio, Gameplay) {
        (
            mod_runtime::ModRuntime::load("../../mods"),
            default(),
            default(),
            default(),
        )
    }

    /// Every selection page offers the built-in default, so an empty mods directory still
    /// yields a startable run — the `% len()` navigation would panic otherwise.
    #[test]
    fn selection_pages_are_never_empty() {
        let runtime = mod_runtime::ModRuntime::load("does-not-exist");
        let (graphics, audio, gameplay) = default();
        for page in [Page::Line, Page::Loco, Page::Scenario, Page::Settings] {
            let items = entries(page, &runtime, &graphics, &audio, &gameplay);
            assert!(!items.is_empty(), "{page:?} is empty");
        }
        // The defaults carry no id — `setup` reads that as "use the built-in".
        for page in [Page::Line, Page::Loco, Page::Scenario] {
            let items = entries(page, &runtime, &graphics, &audio, &gameplay);
            assert!(items[0].id.is_none(), "{page:?}");
        }
    }

    /// The built-in rows must not be mistakable for a mod's row of the same name: the
    /// example mod ships a line called exactly like the built-in one, and before the
    /// provenance chip the two were distinguishable only by a grey sub-line.
    #[test]
    fn every_row_says_where_it_comes_from() {
        let (runtime, graphics, audio, gameplay) = loaded();
        for page in [Page::Line, Page::Loco] {
            for entry in entries(page, &runtime, &graphics, &audio, &gameplay) {
                assert!(
                    !entry.chip.is_empty(),
                    "{page:?}: {} has no chip",
                    entry.label
                );
                assert!(!entry.monogram.is_empty(), "{page:?}: {}", entry.label);
            }
        }
    }

    /// The detail pane reads real figures out of the content — a line that reports zero
    /// length means the pane is showing a placeholder instead of the route.
    #[test]
    fn the_detail_pane_reads_the_content() {
        let (runtime, graphics, audio, gameplay) = loaded();
        let lines = entries(Page::Line, &runtime, &graphics, &audio, &gameplay);
        let line = facts(Page::Line, &lines[0], &runtime).expect("the built-in line has facts");
        assert!(!line.rows.is_empty());
        assert!(
            line_length(&musterbahn()) > 1000.0,
            "the example line is km long"
        );

        let locos = entries(Page::Loco, &runtime, &graphics, &audio, &gameplay);
        let loco = facts(Page::Loco, &locos[0], &runtime).expect("the BR 101 has facts");
        assert_eq!(loco.rows.len(), 5);
        // Mods and settings have nothing to show beside the list.
        let settings = entries(Page::Settings, &runtime, &graphics, &audio, &gameplay);
        assert!(facts(Page::Settings, &settings[1], &runtime).is_none());
    }

    /// The whole flow without a window: three confirmations pick line, vehicle and
    /// scenario and hand over to `Driving`; Esc walks back the same way.
    #[test]
    fn the_start_flow_reaches_driving_and_esc_walks_back() {
        let mut app = app();
        assert_eq!(page(&app), Page::Line);

        key(&mut app, KeyCode::Enter);
        assert_eq!(page(&app), Page::Loco);
        key(&mut app, KeyCode::Escape);
        assert_eq!(page(&app), Page::Line);

        // What the row observers set: a click confirms exactly like Enter, and once.
        app.world_mut().resource_mut::<MenuState>().clicked = true;
        app.update();
        assert_eq!(page(&app), Page::Loco);
        app.update();
        assert_eq!(page(&app), Page::Loco, "the click was consumed");

        key(&mut app, KeyCode::Enter);
        assert_eq!(page(&app), Page::Scenario);
        key(&mut app, KeyCode::Escape);
        assert_eq!(page(&app), Page::Loco);
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Enter);
        assert_eq!(
            *app.world().resource::<State<GameState>>().get(),
            GameState::Driving
        );
        let selection = app.world().resource::<Selection>();
        assert!(selection.line_ref.is_none(), "the built-in line was picked");
        assert!(selection.scenario_id.is_none(), "no scenario was picked");
    }

    /// Tab walks the navigation column; the mods page is reachable and Esc leads home.
    #[test]
    fn tab_walks_the_navigation_column() {
        let mut app = app();
        key(&mut app, KeyCode::Tab);
        assert_eq!(page(&app), Page::Mods);
        key(&mut app, KeyCode::Tab);
        assert_eq!(page(&app), Page::Settings);
        key(&mut app, KeyCode::Escape);
        assert_eq!(page(&app), Page::Line);
    }

    /// The settings page opens on a value, not on the heading above it, and ← / → dial
    /// that value inside its range.
    #[test]
    fn the_settings_page_skips_headings_and_dials_values() {
        let mut app = app();
        app.world_mut().resource_mut::<MenuState>().nav_click = Some(2);
        app.update();
        assert_eq!(page(&app), Page::Settings);
        assert_eq!(
            app.world().resource::<MenuState>().selected,
            1,
            "row 0 is the graphics heading"
        );

        let before = app.world().resource::<Graphics>().view_distance;
        key(&mut app, KeyCode::ArrowRight);
        let after = app.world().resource::<Graphics>().view_distance;
        assert_eq!(after, before + settings::VIEW_DISTANCE.2);

        // ↑ from the first value wraps past the last row to the last value, never onto a
        // heading.
        key(&mut app, KeyCode::ArrowUp);
        let selected = app.world().resource::<MenuState>().selected;
        let items = entries(
            Page::Settings,
            &app.world().resource::<Mods>().0,
            app.world().resource::<Graphics>(),
            app.world().resource::<Audio>(),
            app.world().resource::<Gameplay>(),
        );
        assert!(!items[selected].heading);
    }

    /// Every setting the page offers can be dialled in both directions and stays inside
    /// its range — a knob that leaves it would be written to disk and read back wrong.
    #[test]
    fn every_setting_stays_inside_its_range() {
        let (mut graphics, mut audio, mut gameplay) = default();
        for (_, group) in SETTINGS {
            for setting in group {
                for dir in [-1, 1] {
                    for _ in 0..40 {
                        change(*setting, dir, &mut graphics, &mut audio, &mut gameplay);
                    }
                }
            }
        }
        assert!(
            (settings::VIEW_DISTANCE.0..=settings::VIEW_DISTANCE.1)
                .contains(&graphics.view_distance)
        );
        assert!((settings::VOLUME.0..=settings::VOLUME.1).contains(&audio.master));
        assert!((settings::LOOK_SPEED.0..=settings::LOOK_SPEED.1).contains(&gameplay.look_speed));
        // The language cycles through system + every shipped language and back.
        assert!(
            gameplay.language.is_empty()
                || i18n::LANGUAGES.iter().any(|(c, _)| *c == gameplay.language)
        );
    }

    /// A slider's fill has to sit at the same place the number says. Both ends included,
    /// or the track would look full at the minimum.
    #[test]
    fn a_sliders_fill_follows_its_value() {
        let range = settings::VOLUME;
        assert_eq!(fraction(range.0, range), 0.0);
        assert_eq!(fraction(range.1, range), 1.0);
        assert!((fraction((range.0 + range.1) / 2.0, range) - 0.5).abs() < 1e-6);
    }
}
