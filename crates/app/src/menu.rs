//! Main menu: a title screen, and a full-width flow behind it.
//!
//! **The title screen** is the game's front door — wordmark, and four verbs set large:
//! begin a run, mods, settings, quit. No panels, no persistent navigation rail. A rail
//! down the left with a content pane beside it is the shape of a web dashboard, and it
//! reads as one no matter how it is coloured.
//!
//! **The flow** takes the whole screen. It begins with the run — a scenario, a service
//! out of an operating day, or free rein — and everything else follows from it: a
//! scenario names the line it plays on, and a service names the line of its operating
//! day, so the route is derived rather than asked for. Only what the run leaves open is
//! still a step, so picking a prepared run can be the whole flow, while the free run
//! walks route and vehicle as before. Which step that is stands in a numbered rail across
//! the top, with what has been picked under each — that is the breadcrumb, the back
//! button and the progress bar in one. The list sits left, and beside it a pane reads the
//! highlighted entry out of the loaded content. Esc walks the steps back and leaves at
//! the title screen.
//!
//! Keyboard (↑/↓, ←/→, Enter, Esc) and mouse (wheel scrolls, hover selects, click
//! confirms) drive the
//! same selection index, so neither input is a special case. The world is built only on
//! leaving the menu, so a mod toggled here takes effect on start — no restart. Any run
//! flag on the command line (`--line`, `--frames`, …) skips the menu entirely, which keeps
//! the documented CLI and CI invocations non-interactive.
//!
//! Three rules hold the look together. **Prose is Fira Sans, machine output is Fira
//! Mono** — names and sentences in the proportional face, ids, versions, metres, per cent
//! and key caps in the fixed one, so figures stay in their columns and the two faces still
//! read as one family. **A state is a surface, not a decoration**: the selected row is an
//! opaque tier plus a bar on its leading edge, never a gradient washing across the row.
//! And **the interface is monochrome**; the one saturated colour is traffic red, which
//! appears exactly twice — as the mark above the wordmark, and on the button that starts
//! something. A second accent hue would only compete with it. Amber is left for the one
//! warning there is: a mod missing a dependency.
//!
//! Every setting applies the moment it is dialled — see `settings::apply_scene`. Nothing
//! here says "takes effect on the next run", because nothing does.
//!
//! The rows are torn down and rebuilt whenever their fingerprint changes rather than
//! patched in place: a row is four nodes deep and carries a different shape per page, and
//! a rebuild costs twenty entities on a menu that is idle the rest of the time. The detail
//! pane is filled in the same pass, which is why looking up a line's length may build it.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::widget::NodeImageMode;
use bevy::ui::{BackgroundGradient, ColorStop, LinearGradient, ScrollPosition};
use content::musterbahn;
use content::route::LineSource;
use i18n::t;
use sim_core::brakes::BrakeKind;
use sim_core::day::{Date, OperatingDay, RunSetup};
use sim_core::drive::TractionSpec;
use sim_core::train::{VehicleSpec, VehicleVariant};
use sim_core::weather::{Preset, WeatherChoice};

use crate::bindings::{self, Action, Bind, Bindable, Bindings, Binds, Rebind};
use crate::mods_ui::{self, ModManager};
use crate::settings::{self, Audio, Gameplay, Graphics};
use crate::theme::{
    ACCENT, BASE, BRAND, CHIP, Face, Fonts, HAIRLINE, PANE, ROW, ROW_ACTIVE, ROW_HOVER, SLOT, TEXT,
    TEXT_BRIGHT, TEXT_DIM, TEXT_FAINT, TEXT_MID, TRACK, WARN, Wallpaper, rule, text,
};
use crate::world::{BUILTIN_DAY, ServiceRef, resolve_day};
use crate::{GameState, Mods};

/// Height of a row, of a section heading on the settings page, and the gap below either.
/// The list scrolls by the running sum of these, so a heading may be shorter than a row
/// without the keyboard losing track of where a row sits.
const ROW_HEIGHT: f32 = 56.0;
const HEADING_HEIGHT: f32 = 46.0;
const VERB_HEIGHT: f32 = 52.0;
const ROW_GAP: f32 = 6.0;

/// Width the list stops growing at — narrow beside the detail pane, wider on the two
/// pages that have none.
const LIST_WIDTH: f32 = 520.0;
const LIST_WIDTH_WIDE: f32 = 760.0;
const DETAIL_WIDTH: f32 = 380.0;

/// The player's choices. `None` means the built-in default — `setup` falls back to the
/// example line, the BR 101 and no scenario for exactly that case.
#[derive(Resource, Default, Clone)]
pub struct Selection {
    pub line_ref: Option<String>,
    pub loco_id: Option<String>,
    pub scenario_id: Option<String>,
    /// Which of the vehicle's variants it runs in — the index `Vehicle::variant` takes.
    /// `None` = the vehicle itself, which is what a vehicle without variants is.
    pub variant: Option<usize>,
    /// The service out of an operating day the player took instead of a scenario
    /// (plan ch. 11).
    pub service: Option<ServiceRef>,
    /// The date and the weather they set for it on [`Page::Setup`]. `None` = whatever the
    /// plan itself says, which is what a run started from the command line gets.
    pub setup: Option<RunSetup>,
}

/// The verbs on the title screen, in order: the key of the label, and the page it opens.
/// Quit carries no page — it leaves.
const VERBS: [(&str, Option<Page>); 4] = [
    ("menu-drive", Some(Page::Run)),
    ("menu-mods", Some(Page::Mods)),
    ("menu-settings", Some(Page::Settings)),
    ("menu-quit", None),
];

/// The verbs of the pause overlay. Shorter on purpose: mods cannot be toggled into a
/// world that is already built. Leaving for the title screen tears the built world down
/// (`main::tear_down_run`), which is why it is its own verb and not simply Esc.
const PAUSE_VERBS: [&str; 4] = ["menu-resume", "menu-settings", "menu-title", "menu-quit"];

/// The settings the overlay leaves out of its groups. The language belongs to the front
/// end — switching it in the middle of a run is not a driving decision. (Reset is left out
/// too, but it hangs below the groups rather than inside one.)
const NOT_WHILE_DRIVING: [Setting; 1] = [Setting::Language];

/// Which screen the menu is showing. `Root` is the title screen; everything else is the
/// full-width flow behind it, and [`Page::Run`] opens the steps of picking a run.
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
enum Page {
    #[default]
    Root,
    /// The root of the overlay over a standing run.
    Pause,
    /// The first step: which run is being driven — a scenario, a service out of an
    /// operating day, or free rein. Everything after it depends on what it left open.
    Run,
    /// Which route the run takes. Only walked where the run names none.
    Line,
    /// What is at the head of the train. Only walked where the run does not say.
    Loco,
    /// Date and weather for a timetable run — a service lies at the same hour of the same
    /// line every time it is taken, so those two are the player's.
    Setup,
    Mods,
    Settings,
    /// The keyboard and the controllers, one row per action. A page of its own rather
    /// than a group on the settings page: sixty rows under a heading would bury the
    /// dozen settings above them.
    Controls,
}

impl Page {
    /// The page of verbs this one belongs under — the title screen, or the pause overlay.
    fn home(overlay: bool) -> Page {
        if overlay { Page::Pause } else { Page::Root }
    }

    /// Whether this page is a list of verbs rather than a list of content.
    fn is_home(self) -> bool {
        matches!(self, Page::Root | Page::Pause)
    }

    /// Where Esc goes. Inside the drive flow that is the step before this one, which is
    /// not a fixed page any more — a scenario brings its own route and its own train, and
    /// the steps it answered are not walked. Everything else leads back to the page of
    /// verbs, from where the front end has nowhere to go and the overlay resumes the run.
    fn back(self, overlay: bool, flow: &[Page]) -> Option<Page> {
        match self {
            Page::Root | Page::Pause => None,
            Page::Mods | Page::Settings => Some(Page::home(overlay)),
            Page::Controls => Some(Page::Settings),
            _ => match flow.iter().position(|page| *page == self) {
                Some(0) | None => Some(Page::home(overlay)),
                Some(at) => Some(flow[at - 1]),
            },
        }
    }

    /// The title the step rail shows over this step of the drive flow.
    fn step_title(self) -> &'static str {
        match self {
            Page::Run => "menu-select-run",
            Page::Line => "menu-select-line",
            Page::Loco => "menu-select-loco",
            Page::Setup => "menu-run-setup",
            _ => "",
        }
    }

    /// Where the answer given on this step is remembered — [`MenuState::chosen`] is
    /// indexed by it, so a step keeps its answer even while another run hides it.
    fn slot(self) -> Option<usize> {
        match self {
            Page::Run => Some(0),
            Page::Line => Some(1),
            Page::Loco => Some(2),
            Page::Setup => Some(3),
            _ => None,
        }
    }
}

/// Which of the steps behind the run a run leaves for the player to answer.
///
/// A scenario that names its line and brings its own consists answers both by itself —
/// picking it is the whole flow. One that says neither asks for both, which is what the
/// free run is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Open {
    /// The run names no route, so the player picks one ([`Page::Line`]).
    line: bool,
    /// The run does not say what is at the head, so the player picks ([`Page::Loco`]).
    loco: bool,
    /// It is a service, so date and weather are set before it starts ([`Page::Setup`]).
    setup: bool,
}

impl Open {
    /// The pages of the flow, in the order they are walked. The run picker itself is
    /// always the first of them.
    fn flow(self) -> Vec<Page> {
        let mut pages = vec![Page::Run];
        pages.extend(self.line.then_some(Page::Line));
        pages.extend(self.loco.then_some(Page::Loco));
        pages.extend(self.setup.then_some(Page::Setup));
        pages
    }

    /// The page after `page`, or `None` where `page` is the last step and Enter starts
    /// the run.
    fn after(self, page: Page) -> Option<Page> {
        let flow = self.flow();
        let at = flow.iter().position(|p| *p == page)?;
        flow.get(at + 1).copied()
    }
}

/// Which page the menu shows and which row is selected.
#[derive(Resource, Default)]
pub struct MenuState {
    page: Page,
    /// The menu is the Esc overlay over a run rather than the game's front end: no
    /// wallpaper, no camera of its own, a scrim over the standing world, and a shorter
    /// list of settings.
    overlay: bool,
    selected: usize,
    /// Row under the cursor, which is shown apart from the selection so a mouse and a
    /// keyboard user can both see where they are.
    hovered: Option<usize>,
    /// Set by a click observer, consumed like an Enter press.
    clicked: bool,
    /// The row of the controls page that is waiting for its new key, button or axis.
    /// While it is set, the whole keyboard belongs to that row rather than to the menu.
    rebinding: Option<Bindable>,
    /// Which variant of the highlighted vehicle the pane is showing; ← / → dial it. A
    /// counter rather than an index — [`variant_of`] wraps it into what the vehicle has,
    /// so moving to a vehicle with fewer variants can never point past the end.
    variant: usize,
    /// Labels of what has been picked, indexed by [`Page::slot`] — the step rail shows
    /// them under their step, and only the menu ever needs them as text.
    chosen: [String; 4],
    /// The two screens, shown one at a time.
    title_screen: Option<Entity>,
    flow_screen: Option<Entity>,
    /// The nodes the verbs, the step rail, the rows, the detail pane and the key hints
    /// hang off. `verbs` and `list` are both row lists — only one of them is filled.
    verbs: Option<Entity>,
    steps: Option<Entity>,
    list: Option<Entity>,
    detail: Option<Entity>,
    hints: Option<Entity>,
    /// The dark wash over the wallpaper, which is heavier where there is more text.
    scrim: Option<Entity>,
    /// Fingerprint of what is on screen; everything is rebuilt when it changes.
    drawn: Option<u64>,
    /// The row the list was last scrolled to. Without it the list would be dragged back
    /// to the selection every frame and the wheel could not move it at all.
    scrolled_to: usize,
}

/// Which page `--menu <page>` opens on. A screenshot cannot press keys, so without this
/// only the first page could ever be photographed.
#[derive(Resource)]
pub struct StartPage(pub String);

impl Page {
    fn named(name: &str) -> Option<Page> {
        match name {
            "root" => Some(Page::Root),
            "line" => Some(Page::Line),
            "loco" => Some(Page::Loco),
            // `scenario` is what this page was called while it came last; the screenshots
            // and the README still name it, and it is the same list.
            "run" | "scenario" => Some(Page::Run),
            "setup" => Some(Page::Setup),
            "mods" => Some(Page::Mods),
            "settings" => Some(Page::Settings),
            "controls" => Some(Page::Controls),
            _ => None,
        }
    }
}

/// A text node of the frame that is refilled every frame. `Hint` belongs to the title
/// screen and says what the highlighted verb does; `Title` and `Caption` head the flow.
#[derive(Component)]
pub enum MenuLabel {
    Title,
    Caption,
    Hint,
}

/// One row of a list, by index into the page's entries. Verbs on the title screen and
/// rows in the flow both carry it — they are the same selection.
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
    /// A verb on the title screen — set large, on nothing, with a red marker when the
    /// cursor is on it. Not a card in a list.
    verb: bool,
    /// Which setting the row changes, if any …
    setting: Option<Setting>,
    /// … or what it binds, on the controls page.
    binding: Option<Bindable>,
    /// … or which of the run picker's own values it dials.
    run: Option<RunOption>,
    /// … and how it is operated, read off the settings when the row is built.
    control: Option<Control>,
    /// The service this row takes, where the row is one out of an operating day rather
    /// than a scenario.
    service: Option<ServiceRef>,
}

impl Entry {
    fn height(&self) -> f32 {
        match (self.heading, self.verb) {
            (true, _) => HEADING_HEIGHT,
            (_, true) => VERB_HEIGHT,
            _ => ROW_HEIGHT,
        }
    }
}

/// A single adjustable value on the settings page.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Setting {
    ViewDistance,
    TextureQuality,
    Grass,
    GrassQuality,
    Shadows,
    ShadowQuality,
    Bloom,
    VolumetricClouds,
    Mist,
    MistQuality,
    AntiAliasing,
    AaQuality,
    Upscaling,
    UpscalingQuality,
    Window,
    VSync,
    MaxFps,
    Volume,
    Language,
    Hud,
    LookSpeed,
    /// Opens [`Page::Controls`]; it holds no value of its own.
    Controls,
    Reset,
}

/// A value the run picker sets before a timetable run starts (plan ch. 11).
///
/// A scenario brings its own date and its own sky and is started straight from the list.
/// A service does not: it is the same hour of the same line every time it is taken, so
/// the day it plays on and the weather over it are the player's to choose.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RunOption {
    /// The day the service runs on — the plan's own to begin with.
    Date,
    /// Generated for that day, or one weather named and held.
    Weather,
    /// Which weather, where one is named. Absent while the day makes its own.
    Preset,
}

/// The rows of [`Page::Setup`], in order — the second of them decides whether the third is
/// there at all.
const RUN_OPTIONS: [RunOption; 3] = [RunOption::Date, RunOption::Weather, RunOption::Preset];

impl RunOption {
    /// The message key of the label; the help line is this plus `-hint`.
    fn key(self) -> &'static str {
        match self {
            RunOption::Date => "run-date",
            RunOption::Weather => "run-weather",
            RunOption::Preset => "run-preset",
        }
    }

    /// What the row shows on its right.
    fn value(self, setup: &RunSetup) -> String {
        match self {
            RunOption::Date => date_label(setup.date),
            RunOption::Weather => t!(match setup.weather {
                WeatherChoice::Dynamic => "run-weather-dynamic",
                WeatherChoice::Fixed(_) => "run-weather-fixed",
            }),
            RunOption::Preset => match setup.weather {
                WeatherChoice::Fixed(preset) => t!(preset_key(preset)),
                WeatherChoice::Dynamic => t!("common-none"),
            },
        }
    }
}

/// One step of ← (`dir` −1) or → (`dir` +1) on a run option.
fn change_run(option: RunOption, dir: i32, setup: &mut RunSetup) {
    match option {
        RunOption::Date => setup.date = setup.date.shifted(i64::from(dir)),
        // Switching to a named weather starts at the one the generated day would be
        // closest to in spirit: a clear day, which is also what the eye reads as "none".
        RunOption::Weather => {
            setup.weather = match setup.weather {
                WeatherChoice::Dynamic => WeatherChoice::Fixed(Preset::Clear),
                WeatherChoice::Fixed(_) => WeatherChoice::Dynamic,
            }
        }
        RunOption::Preset => {
            if let WeatherChoice::Fixed(preset) = setup.weather {
                let all = Preset::ALL;
                let at = all.iter().position(|p| *p == preset).unwrap_or(0);
                let next = (at as i32 + dir).rem_euclid(all.len() as i32) as usize;
                setup.weather = WeatherChoice::Fixed(all[next]);
            }
        }
    }
}

/// The message key a named weather is shown under — the same names the vehicle editor and
/// the scenario format use.
fn preset_key(preset: Preset) -> &'static str {
    match preset {
        Preset::Clear => "weather-clear",
        Preset::Cloudy => "weather-cloudy",
        Preset::Overcast => "weather-overcast",
        Preset::Fog => "weather-fog",
        Preset::Drizzle => "weather-drizzle",
        Preset::Rain => "weather-rain",
        Preset::Storm => "weather-storm",
        Preset::Thunderstorm => "weather-thunderstorm",
        Preset::Sleet => "weather-sleet",
        Preset::Snow => "weather-snow",
        Preset::Blizzard => "weather-blizzard",
        Preset::Hail => "weather-hail",
        Preset::Frost => "weather-frost",
    }
}

/// A date as the run picker prints it: 15.08.2026, machine output down to the dots.
fn date_label(date: Date) -> String {
    format!("{:02}.{:02}.{}", date.day, date.month, date.year)
}

/// A time of day out of seconds since midnight, for a timetable row: 08:12.
fn clock_label(seconds: f64) -> String {
    let minutes = (seconds / 60.0).round() as i64;
    let minutes = minutes.rem_euclid(24 * 60);
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
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
const SETTINGS: [(&str, &[Setting]); 4] = [
    // First, and a section of its own: it is the one row that opens a page rather than
    // dialling a value, and at the foot of the last group it sat below the fold on a list
    // that has to be scrolled — a whole page nobody would find.
    ("set-input", &[Setting::Controls]),
    (
        "set-graphics",
        &[
            Setting::ViewDistance,
            Setting::TextureQuality,
            Setting::Grass,
            Setting::GrassQuality,
            Setting::Shadows,
            Setting::ShadowQuality,
            Setting::Bloom,
            Setting::VolumetricClouds,
            Setting::Mist,
            Setting::MistQuality,
            Setting::AntiAliasing,
            Setting::AaQuality,
            Setting::Upscaling,
            Setting::UpscalingQuality,
            Setting::Window,
            Setting::VSync,
            Setting::MaxFps,
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
            Setting::TextureQuality => "set-texture-quality",
            Setting::Grass => "set-grass",
            Setting::GrassQuality => "set-grass-quality",
            Setting::Shadows => "set-shadows",
            Setting::ShadowQuality => "set-shadow-quality",
            Setting::Bloom => "set-bloom",
            Setting::VolumetricClouds => "set-volumetric-clouds",
            Setting::Mist => "set-mist",
            Setting::MistQuality => "set-mist-quality",
            Setting::AntiAliasing => "set-aa",
            Setting::AaQuality => "set-aa-quality",
            Setting::Upscaling => "set-upscaling",
            Setting::UpscalingQuality => "set-upscaling-quality",
            Setting::Window => "set-window",
            Setting::MaxFps => "set-max-fps",
            Setting::VSync => "set-vsync",
            Setting::Volume => "set-volume",
            Setting::Language => "set-language",
            Setting::Hud => "set-hud",
            Setting::LookSpeed => "set-look-speed",
            Setting::Controls => "set-controls",
            Setting::Reset => "set-reset",
        }
    }

    fn control(self, graphics: &Graphics, audio: &Audio, gameplay: &Gameplay) -> Control {
        use settings::{LOOK_SPEED, MAX_FPS, VIEW_DISTANCE, VOLUME};
        match self {
            Setting::ViewDistance => {
                Control::Slider(fraction(graphics.view_distance, VIEW_DISTANCE))
            }
            Setting::Volume => Control::Slider(fraction(audio.master, VOLUME)),
            Setting::MaxFps => Control::Slider(fraction(graphics.max_fps, MAX_FPS)),
            Setting::LookSpeed => Control::Slider(fraction(gameplay.look_speed, LOOK_SPEED)),
            Setting::Shadows => Control::Toggle(graphics.shadows),
            Setting::Bloom => Control::Toggle(graphics.bloom),
            Setting::VolumetricClouds => Control::Toggle(graphics.volumetric_clouds),
            Setting::Mist => Control::Toggle(graphics.mist),
            Setting::Grass => Control::Toggle(graphics.grass),
            Setting::VSync => Control::Toggle(graphics.vsync),
            Setting::AntiAliasing
            | Setting::AaQuality
            | Setting::Upscaling
            | Setting::UpscalingQuality
            | Setting::ShadowQuality
            | Setting::MistQuality
            | Setting::GrassQuality
            | Setting::TextureQuality
            | Setting::Window => Control::Choice,
            // Three steps, so it is dialled like the language rather than switched.
            Setting::Hud => Control::Choice,
            Setting::Language => Control::Choice,
            Setting::Controls | Setting::Reset => Control::Action,
        }
    }

    /// What the row shows beside its control. A toggle says it with the pill alone.
    fn value(self, graphics: &Graphics, audio: &Audio, gameplay: &Gameplay) -> String {
        use settings::MAX_FPS;
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
            Setting::Hud => t!(gameplay.hud.key()),
            Setting::AntiAliasing => t!(graphics.anti_aliasing.key()),
            // MSAA counts its samples, off has nothing to be dialled at all, and the
            // other two are simply Low … High (`AntiAliasing::level_key`).
            Setting::AaQuality => t!(graphics.anti_aliasing.level_key(graphics.aa_quality)),
            Setting::Upscaling => t!(graphics.upscaling.key()),
            // As with the quality under the anti-aliasing: a dash where the thing it
            // belongs to is off, because a step that changes nothing should not read
            // like one that does.
            Setting::UpscalingQuality => t!(dimmed(
                graphics.upscaling != settings::Upscaling::Off,
                graphics.upscaling_quality
            )),
            Setting::ShadowQuality => t!(dimmed(graphics.shadows, graphics.shadow_quality)),
            Setting::MistQuality => t!(dimmed(graphics.mist, graphics.mist_quality)),
            Setting::GrassQuality => t!(dimmed(graphics.grass, graphics.grass_quality)),
            Setting::TextureQuality => t!(graphics.texture_quality.key()),
            Setting::Window => t!(graphics.window.key()),
            // The top step of the slider is not a rate but the absence of one.
            Setting::MaxFps => {
                if graphics.max_fps >= MAX_FPS.1 {
                    t!("set-fps-unlimited")
                } else {
                    t!(
                        "set-fps",
                        value = i18n::decimal(f64::from(graphics.max_fps), 0)
                    )
                }
            }
            _ => String::new(),
        }
    }
}

/// The name of a quality step, or a dash where the thing it belongs to is switched off:
/// a step that changes nothing should not read like one that does.
fn dimmed(on: bool, quality: settings::Quality) -> &'static str {
    if on {
        quality.key()
    } else {
        "set-quality-none"
    }
}

/// Where `value` sits in `(min, max, step)`, 0 … 1 — the filled part of a slider.
fn fraction(value: f32, range: (f32, f32, f32)) -> f32 {
    let (min, max, _) = range;
    ((value - min) / (max - min)).clamp(0.0, 1.0)
}

/// A row of the controls page is waiting for what works it. Returns whether it has it.
///
/// Esc leaves the binding as it was and Backspace takes it away entirely, on either kind
/// of row. Beyond that a button row takes the next key or controller button pressed — the
/// two halves are set apart, so one control can answer to both — and a lever row takes the
/// next stick or trigger moved, because a lever bound to a button would be back to nudging.
///
/// Whatever else was on that key, button or axis lets go of it. Two levers moving on one
/// press, with nothing on screen saying why, is not a binding but a bug report.
fn capture(
    row: Bindable,
    keys: &ButtonInput<KeyCode>,
    pads: &Query<&Gamepad>,
    binds: &mut Binds,
    bindings: &mut Bindings,
) -> bool {
    if keys.just_pressed(KeyCode::Escape) {
        return true;
    }
    let cleared = keys.just_pressed(KeyCode::Backspace);
    match row {
        Bindable::Button(action) => {
            let mut bind = binds.get(action);
            if cleared {
                bind = Bind::default();
            } else if let Some(key) = keys.get_just_pressed().next() {
                bind.key = Some(*key);
            } else if let Some(button) = pads.iter().flat_map(Gamepad::get_just_pressed).next() {
                bind.pad = Some(*button);
            } else {
                return false;
            }
            free_button(binds, action, bind);
            binds.bind(action, bind);
        }
        Bindable::Lever(lever) => {
            let input = if cleared {
                None
            } else if let Some(moved) = bindings::moved(pads) {
                Some(moved)
            } else {
                return false;
            };
            for other in bindings::LEVERS.iter().map(|row| row.0) {
                if other != lever && input.is_some() && binds.lever(other) == input {
                    binds.bind_lever(other, None);
                }
            }
            binds.bind_lever(lever, input);
        }
    }
    *bindings = Bindings::of(binds);
    true
}

/// Takes the key and the controller button of `bind` off every other action.
fn free_button(binds: &mut Binds, action: Action, bind: Bind) {
    for other in bindings::rows().map(|row| row.0).filter(|a| *a != action) {
        let mut taken = binds.get(other);
        if taken.key.is_some() && taken.key == bind.key {
            taken.key = None;
        }
        if taken.pad.is_some() && taken.pad == bind.pad {
            taken.pad = None;
        }
        binds.bind(other, taken);
    }
}

/// Applies one step of ← (`dir` −1) or → / Enter (`dir` +1) to a setting.
fn change(
    setting: Setting,
    dir: i32,
    graphics: &mut Graphics,
    audio: &mut Audio,
    gameplay: &mut Gameplay,
    support: &settings::UpscalingSupport,
) {
    use settings::{LOOK_SPEED, MAX_FPS, VIEW_DISTANCE, VOLUME};
    match setting {
        Setting::ViewDistance => {
            graphics.view_distance = step(graphics.view_distance, dir, VIEW_DISTANCE);
        }
        Setting::Shadows => graphics.shadows = !graphics.shadows,
        Setting::Bloom => graphics.bloom = !graphics.bloom,
        Setting::VolumetricClouds => graphics.volumetric_clouds = !graphics.volumetric_clouds,
        Setting::Mist => graphics.mist = !graphics.mist,
        Setting::Grass => graphics.grass = !graphics.grass,
        Setting::AntiAliasing => graphics.anti_aliasing = graphics.anti_aliasing.cycle(dir),
        Setting::AaQuality => graphics.aa_quality = graphics.aa_quality.cycle(dir),
        // The upscaling row only walks through what this machine can run.
        Setting::Upscaling => {
            graphics.upscaling = graphics
                .upscaling
                .cycle_in(settings::Upscaling::options(*support), dir);
        }
        Setting::UpscalingQuality => {
            graphics.upscaling_quality = graphics.upscaling_quality.cycle(dir);
        }
        Setting::ShadowQuality => graphics.shadow_quality = graphics.shadow_quality.cycle(dir),
        Setting::MistQuality => graphics.mist_quality = graphics.mist_quality.cycle(dir),
        Setting::GrassQuality => graphics.grass_quality = graphics.grass_quality.cycle(dir),
        Setting::TextureQuality => graphics.texture_quality = graphics.texture_quality.cycle(dir),
        Setting::Window => graphics.window = graphics.window.cycle(dir),
        Setting::VSync => graphics.vsync = !graphics.vsync,
        Setting::MaxFps => graphics.max_fps = step(graphics.max_fps, dir, MAX_FPS),
        Setting::Volume => audio.master = step(audio.master, dir, VOLUME),
        Setting::Language => {
            gameplay.language = next_language(&gameplay.language, dir);
            settings::apply_language(&gameplay.language);
        }
        Setting::Hud => gameplay.hud = gameplay.hud.cycle(dir),
        Setting::LookSpeed => gameplay.look_speed = step(gameplay.look_speed, dir, LOOK_SPEED),
        // Opening a page is not changing a value — `menu` does it, where the page is.
        Setting::Controls => {}
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

/// The game's front end: its own camera, the wallpaper, the title screen.
pub fn spawn_menu(
    commands: Commands,
    fonts: Res<Fonts>,
    wallpaper: Res<Wallpaper>,
    start: Option<Res<StartPage>>,
    mut selection: ResMut<Selection>,
    menu: ResMut<MenuState>,
) {
    // `--menu setup` is a screenshot of a page that only exists once a service has been
    // picked. A screenshot cannot pick one, so it is given the first of the built-in day
    // — the same reason `StartPage` exists at all.
    if start.as_deref().map(|start| start.0.as_str()) == Some("setup")
        && selection.service.is_none()
    {
        let day = content::musterbahn_day();
        if let Some((index, _)) = day.playable().next() {
            selection.service = Some(ServiceRef {
                day: BUILTIN_DAY.into(),
                index,
            });
            selection.setup = Some(day.setup());
        }
    }
    spawn(commands, &fonts, &wallpaper, start.as_deref(), menu, false);
}

/// The same menu as an overlay over a run that is standing still: no camera (the cab's
/// draws the UI), no wallpaper (the world is the picture), a scrim to lift the type off it.
pub fn spawn_pause(
    commands: Commands,
    fonts: Res<Fonts>,
    wallpaper: Res<Wallpaper>,
    menu: ResMut<MenuState>,
) {
    spawn(commands, &fonts, &wallpaper, None, menu, true);
}

fn spawn(
    mut commands: Commands,
    fonts: &Fonts,
    wallpaper: &Wallpaper,
    start: Option<&StartPage>,
    mut menu: ResMut<MenuState>,
    overlay: bool,
) {
    let leaves = if overlay {
        GameState::Paused
    } else {
        GameState::Menu
    };
    // In front of a run the world with its 3D camera does not exist yet, so the menu
    // brings its own; over one, the cab camera already draws the UI and a second camera
    // would only compose over it.
    if !overlay {
        commands.spawn((Camera2d, DespawnOnExit(leaves)));
    }
    let root = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(if overlay { Color::NONE } else { BASE }),
            DespawnOnExit(leaves),
        ))
        .id();

    // The wallpaper, and the wash that makes text sit on it. Both absolute and first, so
    // everything spawned after them draws on top. Photography behind type without a scrim
    // is the one thing that looks cheap no matter how good the photograph is — and over a
    // run the photograph is the run, which needs the same treatment.
    if !overlay {
        commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            ImageNode {
                image: wallpaper.0.clone(),
                image_mode: NodeImageMode::Stretch,
                ..default()
            },
            ChildOf(root),
        ));
    }
    let scrim = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            scrim_gradient(Page::home(overlay), overlay),
            ChildOf(root),
        ))
        .id();

    // --- Title screen: wordmark, the verbs, and one line about the highlighted one.
    let title_screen = commands
        .spawn((
            Node {
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                padding: UiRect::horizontal(Val::Px(96.0)),
                ..default()
            },
            ChildOf(root),
        ))
        .id();
    // The mark: traffic red, the colour the trains in the picture are painted.
    commands.spawn((
        Node {
            width: Val::Px(52.0),
            height: Val::Px(4.0),
            margin: UiRect::bottom(Val::Px(20.0)),
            ..default()
        },
        BackgroundColor(BRAND),
        ChildOf(title_screen),
    ));
    // Over a run the wordmark steps aside for what this actually is.
    commands.spawn((
        text(
            fonts,
            if overlay {
                t!("menu-paused")
            } else {
                t!("window-simulator")
            },
            Face::Semibold,
            if overlay { 38.0 } else { 54.0 },
            TEXT_BRIGHT,
        ),
        ChildOf(title_screen),
    ));
    if !overlay {
        commands.spawn((
            text(fonts, t!("menu-tagline"), Face::Sans, 15.0, TEXT_MID),
            Node {
                margin: UiRect::top(Val::Px(6.0)),
                ..default()
            },
            ChildOf(title_screen),
        ));
    }
    let verbs = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                margin: UiRect::top(Val::Px(48.0)),
                ..default()
            },
            ChildOf(title_screen),
        ))
        .id();
    commands.spawn((
        text(fonts, String::new(), Face::Sans, 13.0, TEXT_MID),
        Node {
            height: Val::Px(20.0),
            margin: UiRect::top(Val::Px(20.0)),
            ..default()
        },
        MenuLabel::Hint,
        ChildOf(title_screen),
    ));

    // --- Flow: step rail, page title, list and detail pane, all full width.
    // `min_height: 0` on every flex column: without it a flex item refuses to shrink
    // under its content, and the scrolling list would push the footer off screen.
    let flow_screen = commands
        .spawn((
            Node {
                display: Display::None,
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            ChildOf(root),
        ))
        .id();
    let head = commands
        .spawn((
            Node {
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                padding: UiRect::new(Val::Px(64.0), Val::Px(64.0), Val::Px(36.0), Val::Px(24.0)),
                ..default()
            },
            ChildOf(flow_screen),
        ))
        .id();
    let steps = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                margin: UiRect::bottom(Val::Px(22.0)),
                ..default()
            },
            ChildOf(head),
        ))
        .id();
    commands.spawn((
        text(fonts, String::new(), Face::Semibold, 26.0, TEXT_BRIGHT),
        MenuLabel::Title,
        ChildOf(head),
    ));
    commands.spawn((
        text(fonts, String::new(), Face::Sans, 13.0, TEXT_MID),
        MenuLabel::Caption,
        ChildOf(head),
    ));
    let split = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                column_gap: Val::Px(28.0),
                padding: UiRect::new(Val::Px(64.0), Val::Px(64.0), Val::Px(0.0), Val::Px(28.0)),
                ..default()
            },
            ChildOf(flow_screen),
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

    // --- Footer: version and the key hints, on both screens.
    let footer = commands
        .spawn((
            Node {
                height: Val::Px(52.0),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(24.0),
                padding: UiRect::horizontal(Val::Px(64.0)),
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
            fonts,
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
        .and_then(|start| Page::named(&start.0))
        .unwrap_or(Page::home(overlay));
    *menu = MenuState {
        overlay,
        title_screen: Some(title_screen),
        flow_screen: Some(flow_screen),
        verbs: Some(verbs),
        steps: Some(steps),
        list: Some(list),
        detail: Some(detail),
        hints: Some(hints),
        scrim: Some(scrim),
        page,
        // The settings page opens on a heading; `menu` moves the cursor off it on the
        // first frame, before anything is drawn.
        ..default()
    };
}

/// The wash over whatever is behind the menu — the wallpaper in front of a run, the
/// standing world during one. The page of verbs keeps its type on the left, so the
/// gradient runs left to right and lets the picture breathe where nothing is written; a
/// flow page covers the screen in text and needs it almost gone. The overlay stays a
/// little thinner throughout: the world under it should still be recognisable as the place
/// the run was paused in.
fn scrim_gradient(page: Page, overlay: bool) -> BackgroundGradient {
    let wash = |alpha: f32| {
        Color::srgba(
            0.020,
            0.020,
            0.024,
            if overlay { alpha * 0.82 } else { alpha },
        )
    };
    let stops = if page.is_home() {
        vec![
            ColorStop::percent(wash(0.97), 0.0),
            ColorStop::percent(wash(0.90), 34.0),
            ColorStop::percent(wash(0.42), 100.0),
        ]
    } else {
        vec![
            ColorStop::percent(wash(0.955), 0.0),
            ColorStop::percent(wash(0.90), 100.0),
        ]
    };
    BackgroundGradient::from(LinearGradient::to_right(stops))
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

/// The detail pane, shown on the pages of the drive flow and collapsed on the rest —
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

// ---------------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------------

/// ↑/↓ or hover selects, Enter or left click confirms, ←/→ dials a setting, Esc goes one
/// step back and leaves at the title screen.
// A Bevy system takes its resources as parameters — the argument count says nothing here.
#[allow(clippy::too_many_arguments)]
pub fn menu(
    keys: Res<ButtonInput<KeyCode>>,
    fonts: Res<Fonts>,
    // Optional so the menu can be driven without an asset plugin — the tests run it
    // headless, and the only asset it loads is a vehicle's preview image.
    assets: Option<Res<AssetServer>>,
    mut commands: Commands,
    mut menu: ResMut<MenuState>,
    mut selection: ResMut<Selection>,
    mut manager: ResMut<ModManager>,
    mut mods: ResMut<Mods>,
    mut settings: settings::GraphicsWrite,
    mut audio: ResMut<Audio>,
    mut gameplay: ResMut<Gameplay>,
    mut rebind: Rebind,
    mut next: ResMut<NextState<GameState>>,
    mut exit: MessageWriter<AppExit>,
    mut labels: Query<(&MenuLabel, &mut Text)>,
    mut lists: Query<(&ComputedNode, &mut ScrollPosition)>,
) {
    let (Some(list), Some(verbs), Some(detail), Some(hints)) =
        (menu.list, menu.verbs, menu.detail, menu.hints)
    else {
        return;
    };
    let overlay = menu.overlay;
    // A row waiting for its new key owns the whole keyboard: ↑/↓, Enter and Esc are what
    // the player is about to bind, not what works the menu. The page is still drawn below,
    // so the row can say that it is waiting.
    if let Some(action) = menu.rebinding {
        if capture(
            action,
            &keys,
            &rebind.pads,
            &mut rebind.binds,
            &mut rebind.bindings,
        ) {
            menu.rebinding = None;
        }
    } else {
        let items = entries(
            menu.page,
            overlay,
            &mods.0,
            &selection,
            &settings.graphics,
            &audio,
            &gameplay,
            &rebind.binds,
            menu.rebinding,
        );
        if items.is_empty() {
            menu.selected = 0;
        } else {
            let last = items.len() - 1;
            if keys.just_pressed(KeyCode::ArrowDown) {
                menu.selected = selectable(&items, (menu.selected + 1) % items.len(), 1);
                menu.variant = 0;
            } else if keys.just_pressed(KeyCode::ArrowUp) {
                menu.selected = selectable(&items, (menu.selected + last) % items.len(), -1);
                menu.variant = 0;
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

        // ← / → on the vehicle page dial the livery of the highlighted vehicle. The same
        // pair of keys as a setting's choice, because it is the same kind of question — one
        // of a handful of named options.
        if menu.page == Page::Loco && dial != 0 {
            let variants = entry.map_or(0, |entry| variant_count(entry, &mods.0));
            if variants > 0 {
                menu.variant = (menu.variant as i32 + dial).rem_euclid(variants as i32) as usize;
            }
        }

        if menu.page == Page::Settings {
            // Enter reads as one step forward, so a row can be worked with the keyboard
            // alone and a click does the obvious thing.
            let dir = if confirmed { 1 } else { dial };
            if dir != 0
                && let Some(setting) = entry.and_then(|e| e.setting)
            {
                // The controls row holds no value, so ← / → have nothing to dial on it —
                // only Enter walks into the page it names.
                if setting == Setting::Controls {
                    if confirmed {
                        go(&mut menu, Page::Controls);
                    }
                } else {
                    change(
                        setting,
                        dir,
                        &mut settings.graphics,
                        &mut audio,
                        &mut gameplay,
                        &settings.upscaling,
                    );
                }
            }
        } else if menu.page == Page::Setup && selection.service.is_none() {
            // Nothing to set up: the page belongs to a service, and no service was taken.
            go(&mut menu, Page::Run);
        } else if menu.page == Page::Setup {
            // ← / → dial the value, Enter starts — this is a step of the drive flow, and
            // in that flow Enter has meant "on you go" since the first page. Starting
            // means the loading screen: it builds the run behind its progress bar.
            if dial != 0
                && let Some(option) = entry.and_then(|e| e.run)
            {
                let mut setup = selection.setup.unwrap_or_default();
                change_run(option, dial, &mut setup);
                selection.setup = Some(setup);
            }
            if confirmed {
                next.set(GameState::Loading);
            }
        } else if confirmed && let Some(entry) = entry {
            let id = entry.id.clone();
            let label = entry.label.clone();
            let service = entry.service.clone();
            match menu.page {
                // The title screen: a verb opens its page, or leaves.
                Page::Root => match VERBS.get(menu.selected).and_then(|(_, page)| *page) {
                    Some(page) => go(&mut menu, page),
                    None => {
                        exit.write(AppExit::Success);
                    }
                },
                // The pause overlay: resume, settings, back to the title screen, or leave.
                Page::Pause => match menu.selected {
                    0 => next.set(GameState::Driving),
                    1 => go(&mut menu, Page::Settings),
                    2 => next.set(GameState::Menu),
                    _ => {
                        exit.write(AppExit::Success);
                    }
                },
                // The first step, and the one the others follow from: the run says which
                // route it takes and, where it brings consists or names a vehicle, what
                // runs on it. Only what it leaves open is still walked — a scenario that
                // answered both starts from here.
                Page::Run => {
                    let (route, open) = run_of(&mods.0, id.as_deref(), service.as_ref());
                    // A service is set up before it is driven: the date and the weather
                    // are the player's, and the plan's own are what they start from.
                    selection.setup = service
                        .as_ref()
                        .and_then(|reference| resolve_day(&mods.0, &reference.day))
                        .map(|day| day.setup());
                    selection.scenario_id = id;
                    selection.service = service;
                    // The route the run named. An open one is cleared rather than kept:
                    // what the last run stood on is not an answer to this one.
                    selection.line_ref = route.line_ref();
                    choose(&mut menu, Page::Run, label);
                    if !open.line {
                        choose(&mut menu, Page::Line, route_name(&mods.0.mods, &route));
                    }
                    advance(&mut menu, &mut next, open.after(Page::Run));
                }
                Page::Line => {
                    selection.line_ref = id;
                    choose(&mut menu, Page::Line, label);
                    let to = next_step(&mods.0, &selection, Page::Line);
                    advance(&mut menu, &mut next, to);
                }
                Page::Loco => {
                    // The dress belongs on the `Vehicle`, where it is deterministic state
                    // like the vehicle itself — `world::build` is what has to take it
                    // over from here.
                    let variants = variant_count(entry, &mods.0);
                    selection.variant = (variants > 0).then(|| menu.variant % variants);
                    selection.loco_id = id;
                    choose(&mut menu, Page::Loco, label);
                    let to = next_step(&mods.0, &selection, Page::Loco);
                    advance(&mut menu, &mut next, to);
                }
                // Handled above, before the confirm: the page has no rows that lead
                // anywhere, only values that are dialled.
                Page::Setup => {}
                Page::Mods => {
                    mods_ui::toggle(&mut mods.0, menu.selected, &mut manager);
                    // Reload right away, so the selection lists show what is enabled now.
                    // Every mod stays in `manifests` either way, so the row keeps its index.
                    mods.0 = mod_runtime::ModRuntime::load("mods");
                }
                Page::Settings => {}
                // Enter on a binding row hands the keyboard over to it; the one row
                // that binds nothing is the one that puts every key back.
                Page::Controls => match entry.binding {
                    Some(action) => menu.rebinding = Some(action),
                    None => {
                        *rebind.binds = Binds::default();
                        *rebind.bindings = Bindings::default();
                    }
                },
            }
        }
        if keys.just_pressed(KeyCode::Escape) {
            // Which steps lie behind this one is the run's to say, so Esc asks the run
            // that was taken rather than a fixed chain of pages.
            let walked = flow(&mods.0, menu.page, &selection, None);
            match menu.page.back(overlay, &walked) {
                Some(back) => go(&mut menu, back),
                // Esc on the overlay's own root is the way out of the pause: the run goes on.
                None if overlay => next.set(GameState::Driving),
                None => {}
            }
        }
    }

    // The page and the values may have changed above — re-read before drawing.
    let page = menu.page;
    let rows = if page.is_home() { verbs } else { list };
    let items = entries(
        page,
        overlay,
        &mods.0,
        &selection,
        &settings.graphics,
        &audio,
        &gameplay,
        &rebind.binds,
        menu.rebinding,
    );
    let print = fingerprint(page, &items, menu.selected, menu.hovered, menu.variant);
    if menu.drawn != Some(print) {
        let variants = page == Page::Loco
            && items
                .get(menu.selected)
                .is_some_and(|entry| variant_count(entry, &mods.0) > 0);
        // The rail is the flow of the run under the cursor, not a fixed three steps: a
        // scenario that brings its route and its train asks one question, and the rail
        // has to say so before the answer is given rather than after.
        let walked = flow(&mods.0, page, &selection, items.get(menu.selected));
        let last = walked.last() == Some(&page);
        show_screen(&mut commands, &menu, page);
        build_steps(
            &mut commands,
            &fonts,
            menu.steps,
            page,
            &menu.chosen,
            &walked,
        );
        build_rows(&mut commands, &fonts, rows, &items, &menu);
        build_detail(
            &mut commands,
            &fonts,
            assets.as_deref(),
            detail,
            list,
            page,
            items.get(menu.selected),
            &mods.0,
            &selection,
            menu.variant,
            last,
        );
        build_hints(&mut commands, &fonts, hints, page, variants, last);
        menu.drawn = Some(print);
    }

    let hint = items
        .get(menu.selected)
        .map(|entry| entry.hint.clone())
        .unwrap_or_default();
    for (label, mut text) in &mut labels {
        let content = match label {
            MenuLabel::Title => title(page),
            MenuLabel::Caption => caption(page, &mods.0, &manager),
            MenuLabel::Hint => hint.clone(),
        };
        if **text != content {
            **text = content;
        }
    }

    let (selected, mut scrolled_to) = (menu.selected, menu.scrolled_to);
    scroll_into_view(&mut lists, list, selected, &mut scrolled_to, &items);
    menu.scrolled_to = scrolled_to;
}

/// Swaps the two screens and re-weights the wash over what lies behind them.
fn show_screen(commands: &mut Commands, menu: &MenuState, page: Page) {
    let home = page.is_home();
    for (entity, shown) in [(menu.title_screen, home), (menu.flow_screen, !home)] {
        let Some(entity) = entity else { continue };
        commands
            .entity(entity)
            .entry::<Node>()
            .and_modify(move |mut node| {
                node.display = if shown { Display::Flex } else { Display::None };
            });
    }
    if let Some(scrim) = menu.scrim {
        commands
            .entity(scrim)
            .insert(scrim_gradient(page, menu.overlay));
    }
}

fn go(menu: &mut MenuState, page: Page) {
    menu.page = page;
    menu.selected = 0;
    // A page opens at the top even when the row number does not change — nothing has been
    // scrolled to on a list that did not exist a frame ago.
    menu.scrolled_to = usize::MAX;
    menu.hovered = None;
    menu.variant = 0;
    // Leaving the page while a row waits for its key: the wait goes with it, or the next
    // page would swallow the first thing pressed on it.
    menu.rebinding = None;
}

/// Walks to the next step of the drive flow, or hands over to the loading screen
/// where there is none left — it builds the run behind its progress bar.
fn advance(menu: &mut MenuState, next: &mut NextState<GameState>, page: Option<Page>) {
    match page {
        Some(page) => go(menu, page),
        None => next.set(GameState::Loading),
    }
}

/// Remembers what a step was answered with, and forgets the answers behind it — a second
/// walk through the flow must not show what the first one left standing.
fn choose(menu: &mut MenuState, page: Page, label: String) {
    let Some(slot) = page.slot() else { return };
    menu.chosen[slot] = label;
    for later in &mut menu.chosen[slot + 1..] {
        later.clear();
    }
}

/// The step after `page` for the run the selection holds, or `None` where `page` is the
/// last one and the run starts.
fn next_step(runtime: &mod_runtime::ModRuntime, selection: &Selection, page: Page) -> Option<Page> {
    run_of(
        runtime,
        selection.scenario_id.as_deref(),
        selection.service.as_ref(),
    )
    .1
    .after(page)
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

/// Everything that is drawn, in one number — the rows are rebuilt when it changes.
fn fingerprint(
    page: Page,
    items: &[Entry],
    selected: usize,
    hovered: Option<usize>,
    variant: usize,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    (page as u8, selected, hovered, items.len(), variant).hash(&mut hasher);
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

/// The wheel over a list. Bevy's UI keeps a scroll offset on the node and moves it for
/// nobody: [`scroll_into_view`] is the keyboard's half of the job, and this is the mouse's.
///
/// A system of its own rather than a few lines inside [`menu`], which is already at the
/// sixteen parameters a Bevy system may have.
pub fn scroll_menu(
    mut wheel: MessageReader<MouseWheel>,
    menu: Res<MenuState>,
    mut lists: Query<(&ComputedNode, &mut ScrollPosition)>,
) {
    // A line is worth a row, so one notch of the wheel steps the list by one — the same
    // distance ↓ moves the cursor, which is what makes the two feel like one list.
    let by: f32 = wheel
        .read()
        .map(|event| match event.unit {
            MouseScrollUnit::Line => event.y * ROW_HEIGHT,
            MouseScrollUnit::Pixel => event.y,
        })
        .sum();
    let Some(list) = menu.list.filter(|_| by != 0.0) else {
        return;
    };
    let Ok((node, mut scroll)) = lists.get_mut(list) else {
        return;
    };
    // Taffy measures in physical pixels and the offset is in logical ones.
    let scale = node.inverse_scale_factor;
    let limit = ((node.content_size.y - node.size.y) * scale).max(0.0);
    // Up the wheel, up the content: the offset counts down from the top.
    scroll.0.y = (scroll.0.y - by).clamp(0.0, limit);
}

/// Keeps the selected row inside the list's viewport. Rows and headings differ in height,
/// so where the n-th one sits is the running sum of the ones above it.
///
/// Only when the selection has actually moved. Holding the list to the cursor every frame
/// would be an invariant rather than a courtesy, and it would drag the wheel straight back
/// wherever it scrolled to.
fn scroll_into_view(
    lists: &mut Query<(&ComputedNode, &mut ScrollPosition)>,
    list: Entity,
    selected: usize,
    scrolled_to: &mut usize,
    items: &[Entry],
) {
    if selected == *scrolled_to {
        return;
    }
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
    *scrolled_to = selected;
}

// ---------------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------------

/// The numbered rail across the top of the flow: which step this is, and what has been
/// answered for the ones behind it. Breadcrumb, progress bar and the reason there is no
/// navigation rail down the side, in one row.
///
/// `walked` is the flow of the run being picked, which is not the same length every time:
/// a scenario that names its line and brings its own consists is one step, the free run
/// is three. A step that is never walked is never promised.
fn build_steps(
    commands: &mut Commands,
    fonts: &Fonts,
    steps: Option<Entity>,
    page: Page,
    chosen: &[String; 4],
    walked: &[Page],
) {
    let Some(steps) = steps else { return };
    commands.entity(steps).despawn_related::<Children>();
    let Some(current) = walked.iter().position(|step| *step == page) else {
        return;
    };
    for (index, step_page) in walked.iter().enumerate() {
        let number = index + 1;
        let title = step_page.step_title();
        let here = index == current;
        let done = index < current;
        if index > 0 {
            commands.spawn((
                Node {
                    width: Val::Px(40.0),
                    height: Val::Px(1.0),
                    margin: UiRect::horizontal(Val::Px(16.0)),
                    ..default()
                },
                BackgroundColor(if done || here { TEXT_FAINT } else { HAIRLINE }),
                ChildOf(steps),
            ));
        }
        let step = commands
            .spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(10.0),
                    ..default()
                },
                ChildOf(steps),
            ))
            .id();
        let disc = commands
            .spawn((
                Node {
                    width: Val::Px(24.0),
                    height: Val::Px(24.0),
                    flex_shrink: 0.0,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border_radius: BorderRadius::all(Val::Px(12.0)),
                    ..default()
                },
                BackgroundColor(if here {
                    ACCENT
                } else if done {
                    ROW_ACTIVE
                } else {
                    ROW
                }),
                ChildOf(step),
            ))
            .id();
        commands.spawn((
            text(
                fonts,
                number.to_string(),
                Face::Mono,
                11.0,
                if here { BASE } else { TEXT_DIM },
            ),
            ChildOf(disc),
        ));
        let column = commands
            .spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(2.0),
                    ..default()
                },
                ChildOf(step),
            ))
            .id();
        commands.spawn((
            text(
                fonts,
                t!(title).to_uppercase(),
                Face::Semibold,
                10.0,
                if here { TEXT_MID } else { TEXT_FAINT },
            ),
            ChildOf(column),
        ));
        // Under the label: what was picked for that step, so the answers stay on screen
        // while the next question is asked.
        let answer = step_page
            .slot()
            .and_then(|slot| chosen.get(slot))
            .filter(|c| !c.is_empty());
        commands.spawn((
            text(
                fonts,
                answer.cloned().unwrap_or_else(|| t!("common-none")),
                Face::Sans,
                13.0,
                match (here, answer.is_some()) {
                    (true, _) => TEXT_BRIGHT,
                    (false, true) => TEXT_MID,
                    (false, false) => TEXT_FAINT,
                },
            ),
            ChildOf(column),
        ));
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
        if entry.verb {
            build_verb(commands, fonts, list, i, entry, i == menu.selected);
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
                    (false, true) => WARN,
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
            build_chip(commands, fonts, line, &entry.chip);
        }
        // The help line belongs to the row the cursor is on. All nine at once is a wall
        // of prose that nobody reads.
        let second = if entry.warning {
            Some((entry.hint.clone(), Face::Sans, WARN))
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

/// A verb on the title screen: large type on nothing at all, with a red marker and a step
/// to the right when the cursor is on it. No card, no fill — a front door is not a list.
fn build_verb(
    commands: &mut Commands,
    fonts: &Fonts,
    list: Entity,
    index: usize,
    entry: &Entry,
    on: bool,
) {
    let row = commands
        .spawn((
            Node {
                height: Val::Px(VERB_HEIGHT),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(18.0),
                padding: UiRect::left(Val::Px(if on { 0.0 } else { 16.0 })),
                ..default()
            },
            MenuRow(index),
            ChildOf(list),
        ))
        .observe(on_row_over)
        .observe(on_row_out)
        .observe(on_row_click)
        .id();
    if on {
        commands.spawn((
            Node {
                width: Val::Px(4.0),
                height: Val::Px(26.0),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(BRAND),
            ChildOf(row),
        ));
    }
    commands.spawn((
        text(
            fonts,
            entry.label.clone(),
            if on { Face::Semibold } else { Face::Sans },
            26.0,
            if on { TEXT_BRIGHT } else { TEXT_MID },
        ),
        ChildOf(row),
    ));
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

/// Where a row came from, beside its name: the simulator itself, the mod that brought it,
/// or that it is a composition. Uppercase at 10 px needs the padding to stop looking like
/// a grey brick.
fn build_chip(commands: &mut Commands, fonts: &Fonts, parent: Entity, label: &str) {
    let chip = commands
        .spawn((
            Node {
                padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                flex_shrink: 0.0,
                ..default()
            },
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        text(fonts, label.to_uppercase(), Face::Semibold, 10.0, TEXT_DIM),
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
            // The same column a choice's value sits in — a slider whose top step is a
            // word ("unlimited") needs the room, and the two kinds of row then line up.
            build_value(commands, fonts, zone, &entry.value, on, 96.0);
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
                    // An arrow, not a solid triangle: Fira Mono has no ▸ and draws a
                    // notdef box where the row should say that Enter does something.
                    "→".into(),
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
fn build_hints(
    commands: &mut Commands,
    fonts: &Fonts,
    hints: Entity,
    page: Page,
    variants: bool,
    last: bool,
) {
    commands.entity(hints).despawn_related::<Children>();
    let mut keys: Vec<(&str, &str)> = vec![("↑/↓", "menu-hint-select")];
    if page == Page::Controls {
        keys.push(("Enter", "ctl-hint-rebind"));
        keys.push(("Backspace", "ctl-hint-clear"));
        keys.push(("Esc", "menu-hint-back"));
        build_hint_chips(commands, fonts, hints, &keys);
        return;
    }
    if page == Page::Settings {
        keys.push(("←/→", "menu-hint-change"));
        keys.push(("Enter", "menu-hint-next"));
    } else if page == Page::Setup {
        keys.push(("←/→", "menu-hint-change"));
        keys.push(("Enter", "menu-hint-start"));
    } else {
        // Only where there is something to dial: a vehicle that comes in one dress
        // would be offering a key that does nothing.
        if variants {
            keys.push(("←/→", "menu-hint-change"));
        }
        keys.push((
            "Enter",
            match page {
                Page::Mods => "menu-hint-toggle",
                Page::Root | Page::Pause => "menu-hint-open",
                // On the last step of the flow Enter is the start — which step that is
                // depends on what the run left open.
                _ if last => "menu-hint-start",
                _ => "menu-hint-confirm",
            },
        ));
    }
    match page {
        Page::Root => {}
        Page::Pause => keys.push(("Esc", "menu-hint-resume")),
        _ => keys.push(("Esc", "menu-hint-back")),
    }
    build_hint_chips(commands, fonts, hints, &keys);
}

/// The chips themselves, once the page has decided which keys it offers.
fn build_hint_chips(commands: &mut Commands, fonts: &Fonts, hints: Entity, keys: &[(&str, &str)]) {
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
            text(fonts, (*cap).into(), Face::Mono, 11.0, TEXT_MID),
            ChildOf(cap_box),
        ));
        commands.spawn((
            text(fonts, t!(*label), Face::Sans, 12.0, TEXT_DIM),
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
    assets: Option<&AssetServer>,
    detail: Entity,
    list: Entity,
    page: Page,
    entry: Option<&Entry>,
    runtime: &mod_runtime::ModRuntime,
    selection: &Selection,
    variant: usize,
    // Whether this is the last step of the flow — what the button at the foot says.
    last: bool,
) {
    commands.entity(detail).despawn_related::<Children>();
    let Some(facts) = entry.and_then(|entry| facts(page, entry, runtime, selection, variant))
    else {
        commands.entity(detail).insert(detail_node(false));
        commands.entity(list).insert(list_node(LIST_WIDTH_WIDE));
        return;
    };
    commands
        .entity(detail)
        .insert((detail_node(true), BackgroundColor(PANE)));
    commands.entity(list).insert(list_node(LIST_WIDTH));

    // The box the route or vehicle image goes in. Without one it stays the monogram,
    // which still gives the pane a top edge and keeps the layout honest about what is
    // missing.
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
    // The preview picture the mod ships, over the monogram rather than instead of it: a
    // file that is not there draws nothing at all — Bevy skips an image node whose
    // texture never arrived — and the two letters underneath stay the picture.
    if let (Some(assets), false) = (assets, facts.thumbnail.is_empty()) {
        commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            ImageNode {
                image: assets.load(crate::models::asset_path(&facts.thumbnail)),
                image_mode: NodeImageMode::Stretch,
                ..default()
            },
            ChildOf(plate),
        ));
    }

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
            t!(if last {
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
        Page::Root | Page::Pause => String::new(),
        Page::Line => t!("menu-select-line"),
        Page::Loco => t!("menu-select-loco"),
        Page::Run => t!("menu-select-run"),
        Page::Setup => t!("menu-run-setup"),
        Page::Mods => t!("mods-title"),
        Page::Settings => t!("menu-settings"),
        Page::Controls => t!("ctl-title"),
    }
}

/// The line under the title: what to do on this page, what the mods contribute, or where
/// the settings file lives. Which step we are on is the rail's job now.
fn caption(page: Page, runtime: &mod_runtime::ModRuntime, manager: &ModManager) -> String {
    match page {
        Page::Root | Page::Pause => String::new(),
        Page::Settings => t!("set-stored"),
        Page::Controls => t!("ctl-caption"),
        Page::Mods => mods_ui::details(runtime, manager, true),
        Page::Line => t!("menu-select-line-hint"),
        Page::Loco => t!("menu-select-loco-hint"),
        Page::Run => t!("menu-select-run-hint"),
        Page::Setup => t!("menu-run-setup-hint"),
    }
}

/// What stands in the slot beside a service: its category, which is already the two
/// letters a monogram wants — "RB", not the "R" the initials of one word come to.
fn category_mark(service: &sim_core::day::Service) -> String {
    let category: String = service.category.chars().take(2).collect();
    if category.is_empty() {
        monogram(&service.number)
    } else {
        category.to_uppercase()
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

/// Every operating day the run picker knows, the built-in one first.
fn days(runtime: &mod_runtime::ModRuntime) -> Vec<(String, OperatingDay)> {
    std::iter::once((BUILTIN_DAY.to_string(), content::musterbahn_day()))
        .chain(
            runtime
                .mods
                .days
                .iter()
                .map(|(id, day)| (id.clone(), day.clone())),
        )
        .collect()
}

/// The route a run takes — what the run picker derives from the run instead of asking
/// for it. A scenario's stops, origins and event triggers are indices into one line's
/// track graph; put on another they address whatever happens to lie at that number, so
/// the route is the run's to name, not the player's to guess.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Route {
    /// The example line the simulator brings itself. The built-in operating day runs on
    /// it and on nothing else — it names no line only because that line has no mod id.
    Builtin,
    /// A line or a composition out of a mod, by id.
    Mod(String),
    /// The run names none, so the player is asked and the run is put on the answer. What
    /// the free run is, and what a mod's day or scenario without a `line:` stays.
    Open,
}

impl Route {
    /// The route a `line:` field names.
    fn named(line: Option<&str>) -> Route {
        line.map_or(Route::Open, |id| Route::Mod(id.to_string()))
    }

    /// What [`Selection::line_ref`] becomes for it — `None` is the built-in line, and an
    /// open route starts there too until the player says otherwise.
    fn line_ref(&self) -> Option<String> {
        match self {
            Route::Mod(id) => Some(id.clone()),
            Route::Builtin | Route::Open => None,
        }
    }
}

/// The name a route is shown under. An id no installed mod brought stays the id, so a
/// run pointing at content that is not there still says what it is missing.
fn route_name(mods: &mod_runtime::Mods, route: &Route) -> String {
    match route {
        Route::Builtin => t!("menu-line-builtin"),
        Route::Open => t!("menu-route-open"),
        Route::Mod(id) => mods
            .lines
            .get(id)
            .map(|line| line.name.clone())
            .or_else(|| mods.compositions.get(id).map(|c| c.name.clone()))
            .unwrap_or_else(|| id.clone()),
    }
}

/// The route a run takes and what it leaves for the player to answer, read back out of
/// the loaded content. The free run — neither a scenario nor a service — leaves all of it.
fn run_of(
    runtime: &mod_runtime::ModRuntime,
    scenario_id: Option<&str>,
    service: Option<&ServiceRef>,
) -> (Route, Open) {
    let (route, loco, setup) = if let Some(reference) = service {
        let day = resolve_day(runtime, &reference.day);
        let route = if reference.day == BUILTIN_DAY {
            Route::Builtin
        } else {
            Route::named(day.as_ref().and_then(|day| day.line.as_deref()))
        };
        // A working that names its vehicle has answered the question — the timetable says
        // what runs, and `world::build` takes it over the menu's pick.
        let loco = day
            .as_ref()
            .and_then(|day| day.services.get(reference.index))
            .is_none_or(|service| service.vehicle.is_none());
        (route, loco, true)
    } else if let Some(id) = scenario_id {
        let scenario = runtime.mods.scenarios.get(id);
        // A scenario with consists of its own puts the player's train on the line itself;
        // the vehicle picked in the menu is never asked for then.
        (
            Route::named(scenario.and_then(|s| s.line.as_deref())),
            scenario.is_none_or(|s| s.consists.is_empty()),
            false,
        )
    } else {
        (Route::Open, true, false)
    };
    let open = Open {
        line: route == Route::Open,
        loco,
        setup,
    };
    (route, open)
}

/// The steps of the flow as they stand: derived from the run under the cursor while the
/// run picker is open, and from the run already taken on every step behind it.
fn flow(
    runtime: &mod_runtime::ModRuntime,
    page: Page,
    selection: &Selection,
    entry: Option<&Entry>,
) -> Vec<Page> {
    let (scenario, service) = if page == Page::Run {
        match entry {
            Some(entry) => (entry.id.as_deref(), entry.service.as_ref()),
            None => (None, None),
        }
    } else {
        (selection.scenario_id.as_deref(), selection.service.as_ref())
    };
    run_of(runtime, scenario, service).1.flow()
}

/// The mod an id belongs to (`example:modul_ost` → `example`).
fn origin(id: &str) -> String {
    id.split_once(':').map_or(id, |(m, _)| m).to_string()
}

/// The rows of a page. Every selection page opens with the built-in default, so the list
/// is never empty and the run starts even with no mod installed.
// The rows of a page depend on everything a page can show; the argument count says
// nothing here either.
#[allow(clippy::too_many_arguments)]
fn entries(
    page: Page,
    overlay: bool,
    runtime: &mod_runtime::ModRuntime,
    selection: &Selection,
    graphics: &Graphics,
    audio: &Audio,
    gameplay: &Gameplay,
    binds: &Binds,
    rebinding: Option<Bindable>,
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
        // The two pages of verbs, each with one line about what it does.
        Page::Root | Page::Pause => {
            let keys: Vec<&str> = if page == Page::Pause {
                PAUSE_VERBS.to_vec()
            } else {
                VERBS.iter().map(|(key, _)| *key).collect()
            };
            keys.into_iter()
                .map(|key| Entry {
                    label: t!(key),
                    hint: t!(&format!("{key}-hint")),
                    verb: true,
                    ..default()
                })
                .collect()
        }
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
        // The first question, and the one the rest follow from. Two kinds of run under
        // it: a scenario, which brings its own hour and its own task, and a service out
        // of an operating day, which is one working of a timetable that runs all day and
        // starts over at midnight (plan ch. 11). Both name the route they take, so they
        // stand under it — picking one is picking the line as well.
        Page::Run => {
            // The free run first, as on every selection page: the built-in default, which
            // names neither a route nor a train and therefore walks the whole flow.
            let mut rows = vec![builtin("menu-scenario-none", String::new())];
            let days = days(runtime);
            // The routes in the order the content is loaded in: the mods' lines and
            // compositions by id, then the example line the simulator brings itself, and
            // last the runs that name no route at all. The built-in day is nineteen hours
            // of services — put first it would bury every mod below it, and unlike the
            // free run at the top it is not the default anything falls back to.
            //
            // A run whose route is not installed falls under no heading and is not
            // offered: it could only be started on the wrong line.
            let routes = mods
                .lines
                .keys()
                .chain(mods.compositions.keys())
                .map(|id| Route::Mod(id.clone()))
                .chain([Route::Builtin, Route::Open]);
            for route in routes {
                let scenarios: Vec<(&String, &sim_core::scenario::Scenario)> = mods
                    .scenarios
                    .iter()
                    .filter(|(_, scenario)| Route::named(scenario.line.as_deref()) == route)
                    .collect();
                if !scenarios.is_empty() {
                    rows.push(heading(t!(
                        "menu-scenario-heading",
                        route = route_name(mods, &route)
                    )));
                    rows.extend(scenarios.into_iter().map(|(id, scenario)| {
                        named(
                            id,
                            &scenario.name,
                            format!("{:02}:{:02}", scenario.start.hour, scenario.start.minute),
                        )
                    }));
                }
                for (id, day) in &days {
                    let day_route = if id == BUILTIN_DAY {
                        Route::Builtin
                    } else {
                        Route::named(day.line.as_deref())
                    };
                    if day_route != route || day.playable().next().is_none() {
                        continue;
                    }
                    rows.push(heading(t!(
                        "menu-day-heading",
                        name = day.name.clone(),
                        route = route_name(mods, &route)
                    )));
                    for (index, service) in day.playable() {
                        let (from, to) = service.route();
                        rows.push(Entry {
                            label: t!("menu-service", from = from, to = to),
                            meta: format!(
                                "{}  {} – {}",
                                service.number,
                                clock_label(service.departure()),
                                clock_label(service.arrival())
                            ),
                            monogram: category_mark(service),
                            chip: if id == BUILTIN_DAY {
                                t!("menu-chip-builtin")
                            } else {
                                origin(id)
                            },
                            hint: service.description.clone(),
                            service: Some(ServiceRef {
                                day: id.clone(),
                                index,
                            }),
                            ..default()
                        });
                    }
                }
            }
            rows
        }
        // The one step a scenario never walks: what day the service runs on and what the
        // weather does over it. The preset row is only there while a weather is named —
        // a row that says "none" and cannot be dialled is a dead control.
        Page::Setup => {
            let setup = selection.setup.unwrap_or_default();
            RUN_OPTIONS
                .into_iter()
                .filter(|option| {
                    *option != RunOption::Preset || matches!(setup.weather, WeatherChoice::Fixed(_))
                })
                .map(|option| Entry {
                    label: t!(option.key()),
                    hint: t!(&format!("{}-hint", option.key())),
                    value: option.value(&setup),
                    run: Some(option),
                    control: Some(Control::Choice),
                    ..default()
                })
                .collect()
        }
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
        // Over a run the page is shorter (see `NOT_WHILE_DRIVING`) — and a group that
        // loses all of its rows loses its heading with them.
        Page::Settings => SETTINGS
            .iter()
            .flat_map(|(heading, group)| {
                let group: Vec<&Setting> = group
                    .iter()
                    .filter(|setting| !overlay || !NOT_WHILE_DRIVING.contains(setting))
                    .collect();
                let heading = (!group.is_empty()).then(|| Entry {
                    label: t!(heading),
                    heading: true,
                    ..default()
                });
                heading
                    .into_iter()
                    .chain(group.into_iter().map(|setting| Entry {
                        label: t!(setting.key()),
                        hint: t!(&format!("{}-hint", setting.key())),
                        value: setting.value(graphics, audio, gameplay),
                        setting: Some(*setting),
                        control: Some(setting.control(graphics, audio, gameplay)),
                        ..default()
                    }))
            })
            // Resetting everything at once is too blunt a thing to have under the cursor
            // while a train is standing on a gradient.
            .chain((!overlay).then(|| Entry {
                label: t!(Setting::Reset.key()),
                hint: t!("set-reset-hint"),
                setting: Some(Setting::Reset),
                control: Some(Control::Action),
                ..default()
            }))
            .collect(),
        // One row per action, under the heading of the group it belongs to. The value
        // column is machine output in two halves — the key, and the controller button —
        // which is why it is one mono string rather than a control.
        Page::Controls => bindings::ACTIONS
            .iter()
            .flat_map(|(heading, group)| {
                std::iter::once(Entry {
                    label: t!(heading),
                    heading: true,
                    ..default()
                })
                .chain(group.iter().map(|(action, name, _, _)| Entry {
                    label: t!(&format!("ctl-{name}")),
                    value: if rebinding == Some(Bindable::Button(*action)) {
                        t!("ctl-press")
                    } else {
                        bound(binds.get(*action))
                    },
                    binding: Some(Bindable::Button(*action)),
                    ..default()
                }))
            })
            // The three that have a position rather than a direction. Their key column is
            // empty by construction: a key cannot hold a lever, which is the whole reason
            // they are a group of their own.
            .chain(std::iter::once(Entry {
                label: t!("ctl-group-levers"),
                heading: true,
                ..default()
            }))
            .chain(bindings::LEVERS.iter().map(|(lever, name)| Entry {
                label: t!(&format!("ctl-{name}")),
                hint: t!("ctl-lever-hint"),
                value: if rebinding == Some(Bindable::Lever(*lever)) {
                    t!("ctl-move")
                } else {
                    lever_bound(binds.lever(*lever))
                },
                binding: Some(Bindable::Lever(*lever)),
                ..default()
            }))
            // The only row that binds nothing: it puts every key back.
            .chain(std::iter::once(Entry {
                label: t!("ctl-reset"),
                hint: t!("ctl-reset-hint"),
                control: Some(Control::Action),
                ..default()
            }))
            .collect(),
    }
}

/// The right-hand column of a controls row: the key, then the controller button, each a
/// dash where there is none.
///
/// Two columns in one mono string, padded to the widest label either of them has. The
/// zone is right-aligned, so without the padding the keys would step left and right down
/// the page with every controller button that is longer than the one above it.
fn bound(bind: Bind) -> String {
    let key = bind
        .key
        .map_or_else(|| t!("ctl-unbound"), bindings::key_label);
    let pad = bind
        .pad
        .map_or_else(|| t!("ctl-unbound"), bindings::pad_label);
    format!("{key:>7}  {pad:<13}")
}

/// The same two columns for a lever row. The key half is a dash and stays one: a key has
/// no position to give, which is what the group is about.
fn lever_bound(input: Option<bevy::input::gamepad::GamepadInput>) -> String {
    let axis = input.map_or_else(|| t!("ctl-unbound"), bindings::input_label);
    let none = t!("ctl-unbound");
    format!("{none:>7}  {axis:<13}")
}

/// A section heading: drawn over the rows below it, never landed on by the cursor.
fn heading(label: String) -> Entry {
    Entry {
        label,
        heading: true,
        ..default()
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

/// How many variants the vehicle of a row comes in — 0 for a row that is not a vehicle,
/// and for a vehicle that comes in one dress and therefore has nothing to choose.
fn variant_count(entry: &Entry, runtime: &mod_runtime::ModRuntime) -> usize {
    match &entry.id {
        Some(id) => runtime
            .mods
            .vehicles
            .get(id)
            .map_or(0, |spec| spec.variants.len()),
        None => content::vehicles::br101().variants.len(),
    }
}

/// The variant a counter picks out of what the vehicle has: `None` where it has none,
/// otherwise wrapped into range, so the first one is the default and the dial never
/// points past the end.
fn variant_of(spec: &VehicleSpec, at: usize) -> Option<usize> {
    (!spec.variants.is_empty()).then(|| at % spec.variants.len())
}

/// The data-sheet rows of a vehicle: what [`sim_core::train::VehicleMeta`] states, and
/// nothing it leaves empty — a row with nothing behind it says less than no row at all,
/// and a build year of 0 means "not stated" rather than the year zero. A variant
/// overrides the era it is shown under.
fn meta_rows(spec: &VehicleSpec, dress: Option<&VehicleVariant>) -> Vec<(String, String)> {
    let meta = &spec.meta;
    let mut rows = Vec::new();
    if let Some(dress) = dress {
        // Between chevrons, exactly like a setting's choice — that is what says it can
        // be dialled, here as there.
        rows.push((t!("menu-fact-variant"), format!("‹ {} ›", dress.name)));
    }
    let epoch = dress
        .map(|d| d.epoch.as_str())
        .filter(|epoch| !epoch.is_empty())
        .unwrap_or(&meta.epoch);
    let year = if meta.build_year > 0 {
        meta.build_year.to_string()
    } else {
        String::new()
    };
    for (key, value) in [
        ("menu-fact-class", meta.class.as_str()),
        ("menu-fact-manufacturer", meta.manufacturer.as_str()),
        ("menu-fact-build-year", year.as_str()),
        ("menu-fact-epoch", epoch),
        ("menu-fact-operator", meta.operator.as_str()),
        ("menu-fact-country", meta.country.as_str()),
        ("menu-fact-author", meta.author.as_str()),
    ] {
        if !value.is_empty() {
            rows.push((t!(key), value.to_string()));
        }
    }
    rows
}

/// Everything the detail pane shows about one row.
struct Facts {
    title: String,
    monogram: String,
    body: String,
    rows: Vec<(String, String)>,
    /// Preview image below `mods/`; empty leaves the monogram standing.
    thumbnail: String,
}

/// Looks the highlighted row up in the loaded content. `None` on the pages that have no
/// detail pane, and for the "no scenario" row, which is the absence of a choice.
///
/// `variant` is the counter ← / → dial; it only means anything to a vehicle.
fn facts(
    page: Page,
    entry: &Entry,
    runtime: &mod_runtime::ModRuntime,
    selection: &Selection,
    variant: usize,
) -> Option<Facts> {
    let mods = &runtime.mods;
    let base = |rows| Facts {
        title: entry.label.clone(),
        monogram: entry.monogram.clone(),
        body: String::new(),
        rows,
        thumbnail: String::new(),
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
            // The dress first: what the vehicle is called and looked like, then what it
            // weighs and pulls. A variant states its own era and its own sentence where
            // it differs from the vehicle's.
            let dress = variant_of(spec, variant).and_then(|i| spec.variants.get(i));
            let mut rows = meta_rows(spec, dress);
            rows.extend([
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
                (t!("menu-fact-drive"), t!(traction_key(spec.traction()))),
                (t!("menu-fact-brake"), t!(friction_key(&spec.brake.kind))),
            ]);
            Some(Facts {
                body: dress
                    .map(|d| d.description.clone())
                    .filter(|text| !text.is_empty())
                    .unwrap_or_else(|| spec.meta.description.clone()),
                thumbnail: spec.meta.thumbnail.clone(),
                ..base(rows)
            })
        }
        Page::Run => {
            // A working out of an operating day: what the timetable says about it. Its
            // date and its weather are not here — they are the next step's question.
            if let Some(reference) = &entry.service {
                let day = resolve_day(runtime, &reference.day)?;
                let service = day.services.get(reference.index)?;
                let route = if reference.day == BUILTIN_DAY {
                    Route::Builtin
                } else {
                    Route::named(day.line.as_deref())
                };
                return Some(Facts {
                    body: service.description.clone(),
                    rows: vec![
                        (t!("menu-fact-train"), service.number.clone()),
                        (t!("menu-fact-departure"), clock_label(service.departure())),
                        (t!("menu-fact-arrival"), clock_label(service.arrival())),
                        (t!("menu-fact-stops"), service.stops.len().to_string()),
                        (t!("menu-fact-line"), route_name(mods, &route)),
                    ],
                    ..base(Vec::new())
                });
            }
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
                        route_name(mods, &Route::named(scenario.line.as_deref())),
                    ),
                    (t!("menu-fact-events"), scenario.events.len().to_string()),
                ],
                ..base(Vec::new())
            })
        }
        // The pane on the setup page describes the service being set up, not the row —
        // the rows are its date and its weather, and they say what they are themselves.
        Page::Setup => {
            let reference = selection.service.as_ref()?;
            let day = resolve_day(runtime, &reference.day)?;
            let service = day.services.get(reference.index)?;
            let setup = selection.setup.unwrap_or_else(|| day.setup());
            let (from, to) = service.route();
            Some(Facts {
                title: t!("menu-service", from = from, to = to),
                monogram: category_mark(service),
                body: service.description.clone(),
                rows: vec![
                    (t!("menu-fact-train"), service.number.clone()),
                    (t!("menu-fact-departure"), clock_label(service.departure())),
                    (t!("menu-fact-arrival"), clock_label(service.arrival())),
                    (t!("menu-fact-stops"), service.stops.len().to_string()),
                    (t!("run-date"), date_label(setup.date)),
                    (
                        t!("run-weather"),
                        match setup.weather {
                            WeatherChoice::Dynamic => t!("run-weather-dynamic"),
                            WeatherChoice::Fixed(preset) => t!(preset_key(preset)),
                        },
                    ),
                ],
                thumbnail: String::new(),
            })
        }
        Page::Root | Page::Pause | Page::Mods | Page::Settings | Page::Controls => None,
    }
}

/// The same mapping the vehicle editor uses, so a drive is named identically in both.
fn traction_key(traction: Option<&TractionSpec>) -> &'static str {
    match traction {
        None => "traction-none",
        Some(TractionSpec::Curve { .. }) => "traction-curve",
        Some(TractionSpec::TapChanger { .. }) => "traction-tap",
        Some(TractionSpec::Converter { .. }) => "traction-converter",
        Some(TractionSpec::Diesel { .. }) => "traction-diesel",
        Some(TractionSpec::Steam { .. }) => "traction-steam",
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
    // Only the lever tests name the type — the page itself reads it out of `LEVERS`.
    use crate::bindings::Lever;

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
            .init_resource::<settings::UpscalingSupport>()
            .init_resource::<Binds>()
            .init_resource::<Bindings>()
            .init_resource::<Fonts>()
            .init_resource::<Wallpaper>()
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
        for page in [Page::Line, Page::Loco, Page::Run, Page::Settings] {
            let items = entries(
                page,
                false,
                &runtime,
                &Selection::default(),
                &graphics,
                &audio,
                &gameplay,
                &Binds::default(),
                None,
            );
            assert!(!items.is_empty(), "{page:?} is empty");
        }
        // The defaults carry no id — `setup` reads that as "use the built-in".
        for page in [Page::Line, Page::Loco, Page::Run] {
            let items = entries(
                page,
                false,
                &runtime,
                &Selection::default(),
                &graphics,
                &audio,
                &gameplay,
                &Binds::default(),
                None,
            );
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
            for entry in entries(
                page,
                false,
                &runtime,
                &Selection::default(),
                &graphics,
                &audio,
                &gameplay,
                &Binds::default(),
                None,
            ) {
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
        let lines = entries(
            Page::Line,
            false,
            &runtime,
            &Selection::default(),
            &graphics,
            &audio,
            &gameplay,
            &Binds::default(),
            None,
        );
        let line = facts(Page::Line, &lines[0], &runtime, &Selection::default(), 0)
            .expect("the built-in line has facts");
        assert!(!line.rows.is_empty());
        assert!(
            line_length(&musterbahn()) > 1000.0,
            "the example line is km long"
        );

        let locos = entries(
            Page::Loco,
            false,
            &runtime,
            &Selection::default(),
            &graphics,
            &audio,
            &gameplay,
            &Binds::default(),
            None,
        );
        let loco = facts(Page::Loco, &locos[0], &runtime, &Selection::default(), 0)
            .expect("the BR 101 has facts");
        assert_eq!(loco.rows.len(), 5);
        // Mods and settings have nothing to show beside the list.
        let settings = entries(
            Page::Settings,
            false,
            &runtime,
            &Selection::default(),
            &graphics,
            &audio,
            &gameplay,
            &Binds::default(),
            None,
        );
        assert!(
            facts(
                Page::Settings,
                &settings[1],
                &runtime,
                &Selection::default(),
                0
            )
            .is_none()
        );
    }

    /// The whole flow without a window: the title screen opens the first step, three
    /// confirmations pick line, vehicle and scenario and hand over to the loading
    /// screen; Esc walks back the same way and ends at the title screen.
    #[test]
    fn the_start_flow_reaches_loading_and_esc_walks_back() {
        let mut app = app();
        assert_eq!(page(&app), Page::Root, "the menu opens on the title screen");
        key(&mut app, KeyCode::Enter);
        assert_eq!(page(&app), Page::Run, "the run comes first");

        // The free run names neither a route nor a train, so it is the one run that still
        // asks both questions.
        key(&mut app, KeyCode::Enter);
        assert_eq!(page(&app), Page::Line);
        key(&mut app, KeyCode::Escape);
        assert_eq!(page(&app), Page::Run);

        // What the row observers set: a click confirms exactly like Enter, and once.
        app.world_mut().resource_mut::<MenuState>().clicked = true;
        app.update();
        assert_eq!(page(&app), Page::Line);
        app.update();
        assert_eq!(page(&app), Page::Line, "the click was consumed");

        key(&mut app, KeyCode::Enter);
        assert_eq!(page(&app), Page::Loco);
        key(&mut app, KeyCode::Escape);
        assert_eq!(page(&app), Page::Line);
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Enter);
        assert_eq!(
            *app.world().resource::<State<GameState>>().get(),
            GameState::Loading
        );
        let selection = app.world().resource::<Selection>();
        assert!(selection.line_ref.is_none(), "the built-in line was picked");
        assert!(selection.scenario_id.is_none(), "no scenario was picked");
    }

    /// The route is the run's to name, not the player's to guess: a scenario stands under
    /// the line it plays on, taking it puts the selection on that line, and the step that
    /// would have asked for one is not walked at all. Before, every scenario was offered
    /// whatever line had been picked, and the run was then built on the wrong graph.
    #[test]
    fn picking_a_scenario_settles_the_route_it_plays_on() {
        let mut app = app();
        key(&mut app, KeyCode::Enter);
        assert_eq!(page(&app), Page::Run);

        // The Bördefahrt: its own line, no consists of its own — route settled, vehicle
        // still open, so exactly one step is left.
        let runtime = mod_runtime::ModRuntime::load("../../mods");
        let items = list_of(Page::Run, &runtime, &Selection::default());
        let at = items
            .iter()
            .position(|entry| entry.id.as_deref() == Some("example:boerdefahrt"))
            .expect("the example mod ships the Bördefahrt");
        // It stands under a heading naming its line, not under any other.
        let heading = items[..at]
            .iter()
            .rfind(|entry| entry.heading)
            .expect("a heading over it");
        assert!(
            heading.label.contains(&route_name(
                &runtime.mods,
                &Route::Mod("example:boerde".into())
            )),
            "the Bördefahrt stands under {:?}",
            heading.label
        );

        walk_to(&mut app, &items, at);
        key(&mut app, KeyCode::Enter);
        let selection = app.world().resource::<Selection>();
        assert_eq!(
            selection.line_ref.as_deref(),
            Some("example:boerde"),
            "the line came with the scenario"
        );
        assert_eq!(
            selection.scenario_id.as_deref(),
            Some("example:boerdefahrt")
        );
        assert_eq!(page(&app), Page::Loco, "and the route was never asked for");
        // Esc goes back past the step that was skipped, not into it.
        key(&mut app, KeyCode::Escape);
        assert_eq!(page(&app), Page::Run);
    }

    /// Every run in the list stands under the route it takes, and a run whose route no
    /// installed mod brought is not offered at all — it could only start on another line.
    #[test]
    fn every_run_stands_under_its_route() {
        let mut runtime = mod_runtime::ModRuntime::load("../../mods");
        let items = list_of(Page::Run, &runtime, &Selection::default());
        let mut heading = String::new();
        let mut checked = 0;
        for entry in &items {
            if entry.heading {
                heading.clone_from(&entry.label);
                continue;
            }
            // The free run is the one row without a run behind it.
            if entry.id.is_none() && entry.service.is_none() {
                continue;
            }
            let (route, open) = run_of(&runtime, entry.id.as_deref(), entry.service.as_ref());
            assert!(
                heading.contains(&route_name(&runtime.mods, &route)),
                "{} stands under {heading:?}",
                entry.label
            );
            assert_eq!(
                open.line,
                route == Route::Open,
                "{}: a named route is not asked for again",
                entry.label
            );
            checked += 1;
        }
        assert!(checked > 0, "the example mod ships runs");
        // A scenario whose line is not installed is left out rather than started on
        // whatever else is there.
        runtime.mods.scenarios.insert(
            "test:orphan".into(),
            sim_core::scenario::Scenario {
                name: "Waise".into(),
                line: Some("nowhere:at-all".into()),
                ..default()
            },
        );
        let items = list_of(Page::Run, &runtime, &Selection::default());
        assert!(
            !items
                .iter()
                .any(|entry| entry.id.as_deref() == Some("test:orphan")),
            "a run without its route is not offered"
        );
    }

    /// Puts the cursor on row `at` of a freshly opened page. ↓ moves by selectable rows,
    /// so the headings in between are not presses of their own.
    fn walk_to(app: &mut App, items: &[Entry], at: usize) {
        for _ in 0..items[..at].iter().filter(|entry| !entry.heading).count() {
            key(app, KeyCode::ArrowDown);
        }
    }

    /// The rows of a page, with the defaults for everything the run picker does not read.
    fn list_of(page: Page, runtime: &mod_runtime::ModRuntime, selection: &Selection) -> Vec<Entry> {
        entries(
            page,
            false,
            runtime,
            selection,
            &default(),
            &default(),
            &default(),
            &Binds::default(),
            None,
        )
    }

    /// Picking a service out of an operating day does not start it: the plan says where
    /// it runs, so the route is settled, but what is at the head and which day it plays
    /// on are still the player's.
    #[test]
    fn a_timetable_run_is_set_up_before_it_starts() {
        let mut app = app();
        key(&mut app, KeyCode::Enter);
        assert_eq!(page(&app), Page::Run);

        // Walk down to the first service — past the free run and the heading over the
        // day's services, which the cursor never lands on.
        let runtime = mod_runtime::ModRuntime::load("../../mods");
        let items = list_of(Page::Run, &runtime, &Selection::default());
        let first = items
            .iter()
            .position(|entry| {
                entry
                    .service
                    .as_ref()
                    .is_some_and(|reference| reference.day == BUILTIN_DAY)
            })
            .expect("the built-in operating day offers services");
        assert!(items[first - 1].heading, "under a heading of its own");
        walk_to(&mut app, &items, first);
        key(&mut app, KeyCode::Enter);
        // The built-in day is the built-in line's timetable, so the route step is skipped
        // and the vehicle comes next.
        assert_eq!(page(&app), Page::Loco);
        assert!(
            app.world().resource::<Selection>().line_ref.is_none(),
            "on the line the plan belongs to"
        );
        key(&mut app, KeyCode::Enter);
        assert_eq!(page(&app), Page::Setup, "a service is set up, not started");
        assert_eq!(
            *app.world().resource::<State<GameState>>().get(),
            GameState::Menu
        );

        // The defaults are the plan's own, and ← / → move them.
        let setup = app.world().resource::<Selection>().setup.expect("a setup");
        assert_eq!(setup, content::musterbahn_day().setup());
        key(&mut app, KeyCode::ArrowRight);
        let moved = app.world().resource::<Selection>().setup.expect("a setup");
        assert_eq!(
            moved.date,
            setup.date.shifted(1),
            "the date dialled a day on"
        );

        // The weather row: dynamic by default, and overriding it opens the row that says
        // which weather — a row that is not there while the day makes its own.
        key(&mut app, KeyCode::ArrowDown);
        assert_eq!(rows(&app).len(), 2, "no preset row under a dynamic sky");
        key(&mut app, KeyCode::ArrowRight);
        let fixed = app.world().resource::<Selection>().setup.expect("a setup");
        assert!(matches!(fixed.weather, WeatherChoice::Fixed(Preset::Clear)));
        assert_eq!(rows(&app).len(), 3, "and now it can be named");
        key(&mut app, KeyCode::ArrowDown);
        key(&mut app, KeyCode::ArrowRight);
        let named = app.world().resource::<Selection>().setup.expect("a setup");
        assert!(matches!(
            named.weather,
            WeatherChoice::Fixed(Preset::Cloudy)
        ));

        // Esc walks the flow back, one step at a time, to the run list — which opens at
        // the top again …
        key(&mut app, KeyCode::Escape);
        assert_eq!(page(&app), Page::Loco);
        key(&mut app, KeyCode::Escape);
        assert_eq!(page(&app), Page::Run);
        // … so the service has to be walked to a second time. Enter on the setup page
        // starts the run — that is what the button in the pane says as well.
        walk_to(&mut app, &items, first);
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Enter);
        assert_eq!(page(&app), Page::Setup);
        key(&mut app, KeyCode::Enter);
        assert_eq!(
            *app.world().resource::<State<GameState>>().get(),
            GameState::Loading
        );
        let selection = app.world().resource::<Selection>();
        let service = selection.service.as_ref().expect("a service was taken");
        assert_eq!(service.day, BUILTIN_DAY);
        assert!(selection.scenario_id.is_none(), "and no scenario with it");
    }

    /// The rows the run picker's setup page is showing right now.
    fn rows(app: &App) -> Vec<Entry> {
        entries(
            Page::Setup,
            false,
            &app.world().resource::<Mods>().0,
            app.world().resource::<Selection>(),
            &default(),
            &default(),
            &default(),
            &Binds::default(),
            None,
        )
    }

    /// Every verb of the title screen leads somewhere, and Esc from anywhere leads back
    /// to it — without a navigation rail that is the only way home.
    #[test]
    fn every_verb_opens_its_page_and_esc_leads_home() {
        for (index, expected) in [(1, Page::Mods), (2, Page::Settings)] {
            let mut app = app();
            for _ in 0..index {
                key(&mut app, KeyCode::ArrowDown);
            }
            key(&mut app, KeyCode::Enter);
            assert_eq!(page(&app), expected);
            key(&mut app, KeyCode::Escape);
            assert_eq!(page(&app), Page::Root);
        }
        // The last verb is the one that has no page: it leaves.
        let mut app = app();
        for _ in 0..3 {
            key(&mut app, KeyCode::ArrowDown);
        }
        key(&mut app, KeyCode::Enter);
        assert_eq!(page(&app), Page::Root, "quitting does not open a page");
    }

    /// The pause overlay: Esc resumes, the settings are reachable from it and come back
    /// to it, and the page it shows is shorter than the front end's.
    #[test]
    fn the_pause_overlay_resumes_and_holds_the_settings() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<MenuState>()
            .init_resource::<Selection>()
            .init_resource::<ModManager>()
            .init_resource::<Graphics>()
            .init_resource::<Audio>()
            .init_resource::<Gameplay>()
            .init_resource::<settings::UpscalingSupport>()
            .init_resource::<Binds>()
            .init_resource::<Bindings>()
            .init_resource::<Fonts>()
            .init_resource::<Wallpaper>()
            .insert_resource(Mods(mod_runtime::ModRuntime::load("../../mods")))
            .insert_state(GameState::Paused)
            .add_systems(OnEnter(GameState::Paused), spawn_pause)
            .add_systems(Update, menu.run_if(in_state(GameState::Paused)));
        app.update();
        assert_eq!(page(&app), Page::Pause);

        // Esc on the overlay's root is the way out — back into the run, not to a title
        // screen that would have to tear the world down.
        key(&mut app, KeyCode::Escape);
        assert_eq!(
            *app.world().resource::<State<GameState>>().get(),
            GameState::Driving
        );

        // Row 1 is the settings, and Esc from there comes back to the overlay.
        app.world_mut()
            .insert_resource(NextState::Pending(GameState::Paused));
        app.update();
        key(&mut app, KeyCode::ArrowDown);
        key(&mut app, KeyCode::Enter);
        assert_eq!(page(&app), Page::Settings);
        key(&mut app, KeyCode::Escape);
        assert_eq!(page(&app), Page::Pause);

        // Row 2 leaves the run for the title screen — the one verb here that Esc does
        // *not* also do, which is why it has to be its own row.
        key(&mut app, KeyCode::ArrowDown);
        key(&mut app, KeyCode::ArrowDown);
        key(&mut app, KeyCode::Enter);
        assert_eq!(
            *app.world().resource::<State<GameState>>().get(),
            GameState::Menu
        );
    }

    /// What "abgespeckt" means: the overlay's settings page leaves the language and the
    /// reset out, and nothing else.
    #[test]
    fn the_overlay_settings_are_shorter_by_exactly_two_rows() {
        let (runtime, graphics, audio, gameplay) = loaded();
        let front = entries(
            Page::Settings,
            false,
            &runtime,
            &Selection::default(),
            &graphics,
            &audio,
            &gameplay,
            &Binds::default(),
            None,
        );
        let paused = entries(
            Page::Settings,
            true,
            &runtime,
            &Selection::default(),
            &graphics,
            &audio,
            &gameplay,
            &Binds::default(),
            None,
        );
        let settings = |items: &[Entry]| -> Vec<Setting> {
            items.iter().filter_map(|entry| entry.setting).collect()
        };
        let missing: Vec<Setting> = settings(&front)
            .into_iter()
            .filter(|setting| !settings(&paused).contains(setting))
            .collect();
        assert_eq!(missing, vec![Setting::Language, Setting::Reset]);
        // Every group that still has rows keeps its heading, and no group is left empty.
        assert!(paused.iter().any(|entry| entry.heading));
        for (index, entry) in paused.iter().enumerate() {
            if entry.heading {
                assert!(
                    paused.get(index + 1).is_some_and(|next| !next.heading),
                    "an empty group kept its heading"
                );
            }
        }
    }

    /// The controls page: Enter hands the keyboard over, the next key becomes the
    /// binding, and whatever else answered to that key lets go of it. The whole point of
    /// the page is this one exchange, so it is the one thing under test.
    #[test]
    fn a_row_takes_the_next_key_and_takes_it_off_whoever_had_it() {
        let mut app = app();
        app.world_mut().resource_mut::<MenuState>().page = Page::Controls;
        app.update();
        assert_eq!(
            app.world().resource::<MenuState>().selected,
            1,
            "row 0 is the driving heading"
        );

        // O works the pre-controlled brake until this row takes it.
        assert_eq!(
            app.world().resource::<Binds>().get(Action::EpBrake).key,
            Some(KeyCode::KeyO)
        );
        key(&mut app, KeyCode::Enter);
        assert_eq!(
            app.world().resource::<MenuState>().rebinding,
            Some(Bindable::Button(Action::ThrottleUp)),
            "Enter puts the row into waiting"
        );

        key(&mut app, KeyCode::KeyO);
        let world = app.world();
        assert_eq!(world.resource::<MenuState>().rebinding, None);
        assert_eq!(
            world.resource::<Binds>().get(Action::ThrottleUp).key,
            Some(KeyCode::KeyO)
        );
        assert_eq!(
            world.resource::<Binds>().get(Action::EpBrake).key,
            None,
            "one key, one lever"
        );
        // Both changes reach the settings file, and nothing else does.
        assert_eq!(
            world.resource::<Bindings>().binds,
            ["throttle-up KeyO DPadUp", "ep-brake - -"]
        );
    }

    /// The three levers stand under the buttons, with an empty key column and the bound
    /// axis in the controller one — a lever is bound to an axis or to nothing.
    #[test]
    fn the_lever_rows_show_the_axis_and_no_key() {
        let (runtime, graphics, audio, gameplay) = loaded();
        let mut binds = Binds::default();
        binds.bind_lever(
            Lever::BrakeValve,
            Some(bevy::input::gamepad::GamepadInput::Button(
                GamepadButton::RightTrigger2,
            )),
        );
        let items = entries(
            Page::Controls,
            false,
            &runtime,
            &Selection::default(),
            &graphics,
            &audio,
            &gameplay,
            &binds,
            None,
        );
        let row = |lever: Lever| {
            items
                .iter()
                .find(|entry| entry.binding == Some(Bindable::Lever(lever)))
                .unwrap_or_else(|| panic!("{} has no row", lever.name()))
        };
        assert!(
            row(Lever::BrakeValve).value.contains("RightTrigger2"),
            "the bound axis stands in the controller column"
        );
        // Unbound, and its key column is a dash whether it is bound or not.
        let throttle = &row(Lever::Throttle).value;
        assert_eq!(throttle.split_whitespace().count(), 2, "two dashes");
        // Every lever is on the page, under a heading of its own.
        assert_eq!(
            items
                .iter()
                .filter(|entry| matches!(entry.binding, Some(Bindable::Lever(_))))
                .count(),
            bindings::LEVERS.len()
        );
    }

    /// The settings page opens on a value, not on the heading above it, and ← / → dial
    /// that value inside its range.
    #[test]
    fn the_settings_page_skips_headings_and_dials_values() {
        let mut app = app();
        app.world_mut().resource_mut::<MenuState>().page = Page::Settings;
        app.update();
        assert_eq!(page(&app), Page::Settings);
        assert_eq!(
            app.world().resource::<MenuState>().selected,
            1,
            "row 0 is the input heading"
        );

        // The first row opens a page rather than holding a value, so ← / → do nothing to
        // it and Enter is what walks in.
        key(&mut app, KeyCode::ArrowRight);
        assert_eq!(page(&app), Page::Settings, "a dial must not open a page");

        // Down onto the view distance: one step, because the graphics heading in between
        // is drawn but never lands on the cursor.
        key(&mut app, KeyCode::ArrowDown);
        let before = app.world().resource::<Graphics>().view_distance;
        key(&mut app, KeyCode::ArrowRight);
        let after = app.world().resource::<Graphics>().view_distance;
        assert_eq!(after, before + settings::VIEW_DISTANCE.2);

        // ↑ walks back over the graphics heading onto a value, never onto the heading.
        key(&mut app, KeyCode::ArrowUp);
        let selected = app.world().resource::<MenuState>().selected;
        let items = entries(
            Page::Settings,
            false,
            &app.world().resource::<Mods>().0,
            app.world().resource::<Selection>(),
            app.world().resource::<Graphics>(),
            app.world().resource::<Audio>(),
            app.world().resource::<Gameplay>(),
            &Binds::default(),
            None,
        );
        assert!(!items[selected].heading);
    }

    /// Every setting the page offers can be dialled in both directions and stays inside
    /// its range — a knob that leaves it would be written to disk and read back wrong.
    #[test]
    fn every_setting_stays_inside_its_range() {
        let (mut graphics, mut audio, mut gameplay) = default();
        let support = settings::UpscalingSupport::default();
        for (_, group) in SETTINGS {
            for setting in group {
                for dir in [-1, 1] {
                    for _ in 0..40 {
                        change(
                            *setting,
                            dir,
                            &mut graphics,
                            &mut audio,
                            &mut gameplay,
                            &support,
                        );
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

    /// A vehicle that states nothing about itself gets no rows about itself — an empty
    /// line in the pane says less than no line at all. A build year of 0 is "not
    /// stated", not the year zero.
    #[test]
    fn only_stated_metadata_becomes_a_row() {
        use sim_core::train::VehicleMeta;

        let mut spec = VehicleSpec::default();
        assert!(meta_rows(&spec, None).is_empty());

        spec.meta = VehicleMeta {
            class: "BR 101".into(),
            build_year: 0,
            ..VehicleMeta::default()
        };
        assert_eq!(meta_rows(&spec, None).len(), 1, "the year 0 is not a row");
        assert_eq!(meta_rows(&spec, None)[0].1, "BR 101");
        spec.meta.build_year = 1996;
        let rows = meta_rows(&spec, None);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|(_, value)| value == "1996"));
        assert!(
            rows.iter()
                .all(|(key, value)| !key.is_empty() && !value.is_empty())
        );
    }

    /// The variant dial: a vehicle with variants opens on the first one, the counter
    /// wraps into what the vehicle has, and a vehicle without variants runs as itself.
    #[test]
    fn the_variant_dial_wraps_and_opens_on_the_first() {
        let mut spec = VehicleSpec::default();
        assert_eq!(variant_of(&spec, 0), None);
        assert_eq!(variant_of(&spec, 7), None, "nothing to point at");

        spec.variants = vec![
            VehicleVariant {
                name: "verkehrsrot".into(),
                ..VehicleVariant::default()
            },
            VehicleVariant {
                name: "orientrot".into(),
                epoch: "IV".into(),
                ..VehicleVariant::default()
            },
        ];
        assert_eq!(variant_of(&spec, 0), Some(0));
        assert_eq!(variant_of(&spec, 3), Some(1));
        // The chosen dress heads the rows and overrides the era it is shown under.
        let rows = meta_rows(&spec, spec.variants.get(1));
        assert_eq!(rows[0].1, "‹ orientrot ›");
        assert!(rows.iter().any(|(_, value)| value == "IV"));
    }

    /// The plate never ends up empty: the monogram is drawn whether there is a preview
    /// image or not, so a missing file leaves the two letters standing.
    #[test]
    fn a_vehicle_without_a_preview_keeps_its_monogram() {
        let (runtime, graphics, audio, gameplay) = loaded();
        let locos = entries(
            Page::Loco,
            false,
            &runtime,
            &Selection::default(),
            &graphics,
            &audio,
            &gameplay,
            &Binds::default(),
            None,
        );
        for entry in &locos {
            let facts = facts(Page::Loco, entry, &runtime, &Selection::default(), 0)
                .expect("a vehicle has facts");
            assert!(!facts.monogram.is_empty(), "{}", entry.label);
        }
        // A vehicle that ships no image says so, rather than an empty path into `mods/`.
        let builtin = facts(Page::Loco, &locos[0], &runtime, &Selection::default(), 0)
            .expect("the BR 101 has facts");
        assert!(builtin.thumbnail.is_empty());
    }

    /// Every message the vehicle pane prints has to exist — `i18n` checks that the
    /// languages agree, this checks that the key is there at all.
    #[test]
    fn the_vehicle_facts_have_their_messages() {
        for key in [
            "menu-fact-variant",
            "menu-fact-class",
            "menu-fact-manufacturer",
            "menu-fact-build-year",
            "menu-fact-epoch",
            "menu-fact-operator",
            "menu-fact-country",
            "menu-fact-author",
            "menu-hint-change",
        ] {
            assert!(i18n::maybe(key).is_some(), "{key}");
        }
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
