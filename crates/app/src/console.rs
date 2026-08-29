//! The console (plan 16.3): a line to type commands into, opened and closed with F8.
//!
//! A developer tool, but part of the game's own interface: it is built from the same
//! plain UI nodes the HUD and the mod panel use — no egui — and everything it prints
//! goes through the i18n crate like any other user-visible text. The commands are one
//! static table — name, usage, a line of help, a function — and `Tab` completes over
//! that table and over each command's own arguments while the line is being typed.
//!
//! **Multiplayer.** The world belongs to the server, so a command that moves it is
//! asked for, not taken (`CLAUDE.md` ch. 20). `weather` on a client therefore posts a
//! [`net::WeatherRequest`]; the server applies it to its own timeline and answers
//! *every* client with a [`net::WeatherSet`] anchored to the moment it applied — the
//! asking client included, so all peers run the same five-minute transition and stay in
//! the same rain. `time` moves the run's clock, which the operating day's dispatcher,
//! the scenario and the sun all hang off; a client is refused, because a clock jump
//! cannot be replicated as a setpoint and would put the peers' worlds out of step. It
//! is gated rather than shipped half-way. The `fly` command, by contrast, moves nothing
//! but the local view — a camera of one's own is client-owned (`CLAUDE.md` ch. 20) — so
//! it needs no wire at all.

use crate::bindings;
use crate::theme::{Face, Fonts, TEXT_BRIGHT, TEXT_FAINT, TEXT_MID, text};
use crate::ui::{CameraMode, CameraState};
use crate::{GameState, SimResource, net};
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use i18n::{lookup, t};
use sim_core::Sim;
use sim_core::timetable::DAY;
use sim_core::weather::Preset;

/// The panel sits on the bottom edge, full width, and is read over the world.
const BACKGROUND: Color = Color::srgba(0.047, 0.047, 0.055, 0.90);

/// Lines of the log shown at once — the log itself keeps more ([`MAX_LOG`]) and the
/// panel just shows the last of it.
const LOG_LINES: usize = 14;

/// How many suggestions `Tab` cycles through. Presets beyond the window are reached by
/// typing another letter — thirteen lines would cover half the screen.
const SUGGESTIONS: usize = 6;

/// The log forgets everything past this — a console that grows without end is a leak.
const MAX_LOG: usize = 200;

/// Root node of the panel.
#[derive(Component)]
struct ConsolePanel;

/// The log above the completion list.
#[derive(Component)]
pub(crate) struct LogMarker;

/// The completion list above the input line.
#[derive(Component)]
pub(crate) struct SuggestMarker;

/// The input line itself, prompt included.
#[derive(Component)]
pub(crate) struct InputMarker;

/// The one line of key help at the bottom, set once at spawn.
#[derive(Component)]
struct HintMarker;

/// The whole console: what is typed, what was answered, and where in the history and
/// the completion list the player stands.
#[derive(Resource, Default)]
pub struct Console {
    pub open: bool,
    input: String,
    log: Vec<String>,
    history: Vec<String>,
    /// Where in the history the player is browsing, counted back from the newest —
    /// `None` while a fresh line is being typed.
    history_pos: Option<usize>,
    /// What was typed before the history browsing took the line over.
    draft: String,
    /// What `Tab` is cycling through: the candidates, where it stands in them, and
    /// which word of the line it is filling. Reset by every keystroke that is not Tab.
    completing: Option<Cycle>,
}

/// One run of `Tab` completions over one word of the line.
struct Cycle {
    list: Vec<String>,
    index: usize,
    /// Index of the word being filled — counted in whole words, so a word that is not
    /// started yet (the line ends in a space) is `words.len()`.
    word: usize,
}

impl Console {
    /// One line into the log, forgetting the oldest beyond [`MAX_LOG`].
    fn print(&mut self, line: impl Into<String>) {
        self.log.push(line.into());
        if self.log.len() > MAX_LOG {
            let excess = self.log.len() - MAX_LOG;
            self.log.drain(..excess);
        }
    }
}

/// Register the resource, build the panel once for the whole process — it outlives the
/// runs and is only ever shown while one is being driven — and keep its visibility.
///
/// `--console` starts with the panel open: a screenshot cannot press F8, and it is the
/// same courtesy the F5/F6 overlays get (`--overlays`).
pub fn plugin(app: &mut App) {
    let open = std::env::args().any(|flag| flag == "--console");
    app.init_resource::<Console>()
        .add_systems(Startup, spawn)
        .add_systems(Update, visibility);
    if open {
        app.world_mut().resource_mut::<Console>().open = true;
    }
}

fn spawn(mut commands: Commands, fonts: Res<Fonts>) {
    let panel = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(14.0)),
                row_gap: Val::Px(6.0),
                ..default()
            },
            BackgroundColor(BACKGROUND),
            // Its own hierarchy against the HUD's, and over it: the desk sits exactly
            // where the panel opens, and the console is the topmost thing on screen
            // while it is up.
            GlobalZIndex(100),
            Visibility::Hidden,
            Pickable::IGNORE,
            ConsolePanel,
        ))
        .id();
    // In reading order: the log, the completion list, the input line, the hint.
    line(
        &mut commands,
        &fonts,
        panel,
        String::new(),
        13.0,
        TEXT_MID,
        LogMarker,
    );
    line(
        &mut commands,
        &fonts,
        panel,
        String::new(),
        13.0,
        TEXT_MID,
        SuggestMarker,
    );
    line(
        &mut commands,
        &fonts,
        panel,
        String::new(),
        15.0,
        TEXT_BRIGHT,
        InputMarker,
    );
    line(
        &mut commands,
        &fonts,
        panel,
        t!("console-hint"),
        11.0,
        TEXT_FAINT,
        HintMarker,
    );
}

/// One line of the panel: a child of it, mono type over the world.
#[allow(clippy::too_many_arguments)]
fn line<M: Component>(
    commands: &mut Commands,
    fonts: &Fonts,
    parent: Entity,
    content: String,
    size: f32,
    color: Color,
    marker: M,
) {
    commands.spawn((
        ChildOf(parent),
        text(fonts, content, Face::Mono, size, color),
        Pickable::IGNORE,
        marker,
    ));
}

/// The panel is shown while the console is open during a run — hidden on the menu,
/// behind the pause overlay, and whenever the run is not on. Runs in every state: the
/// console may stand open when the pause overlay comes up over it, and it must not
/// shine through.
fn visibility(
    game: Res<State<GameState>>,
    console: Res<Console>,
    mut panel: Query<&mut Visibility, With<ConsolePanel>>,
) {
    let wanted = if console.open && *game.get() == GameState::Driving {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for mut current in &mut panel {
        if *current != wanted {
            *current = wanted;
        }
    }
}

/// The frame's work: the F8 key, the typing, the completion and what Enter runs — then
/// the panel's text, which is one block each for log, suggestions and input.
///
/// First in the driving chain: the systems after it read `Console::open` and stay off
/// their keys while the console holds the keyboard — `W` is a letter there, not
/// throttle up.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn console(
    mut state: ResMut<Console>,
    input: bindings::Input,
    keys: Res<ButtonInput<KeyCode>>,
    mut typed: MessageReader<KeyboardInput>,
    mut sim: ResMut<SimResource>,
    mut camera: ResMut<CameraState>,
    role: Option<Res<net::Role>>,
    mut wishes: MessageWriter<net::WeatherRequest>,
    mut log: Query<&mut Text, With<LogMarker>>,
    mut suggest: Query<&mut Text, (With<SuggestMarker>, Without<LogMarker>)>,
    mut line: Query<
        &mut Text,
        (
            With<InputMarker>,
            Without<LogMarker>,
            Without<SuggestMarker>,
        ),
    >,
) {
    if input.just_pressed(bindings::Action::Console) {
        state.open = !state.open;
        state.completing = None;
        state.history_pos = None;
    }
    if !state.open {
        return;
    }
    // Characters with a modifier held are shortcuts, not text — except AltRight, which
    // is AltGr on the German layout and the only way to type @, \ and friends.
    let shortcut = keys.pressed(KeyCode::ControlLeft)
        || keys.pressed(KeyCode::SuperLeft)
        || keys.pressed(KeyCode::SuperRight);
    for press in typed.read() {
        if press.state != ButtonState::Pressed {
            continue;
        }
        match &press.logical_key {
            Key::Escape if !press.repeat => {
                state.open = false;
                state.completing = None;
            }
            Key::Enter if !press.repeat => {
                if let Some(preset) = run_line(
                    &mut state,
                    &mut sim.0,
                    &mut camera,
                    is_client(role.as_deref()),
                ) {
                    wishes.write(net::WeatherRequest(preset));
                }
            }
            Key::Tab if !press.repeat => complete(&mut state),
            Key::Backspace if !shortcut => {
                // `pop` takes a whole grapheme, so an umlaut goes in one press.
                state.input.pop();
                state.completing = None;
            }
            Key::ArrowUp if !press.repeat => history_back(&mut state),
            Key::ArrowDown if !press.repeat => history_forward(&mut state),
            Key::Character(text) if !shortcut => {
                state.input.push_str(text);
                state.completing = None;
            }
            _ => {}
        }
    }

    let Ok(mut log) = log.single_mut() else {
        return;
    };
    let start = state.log.len().saturating_sub(LOG_LINES);
    **log = state.log[start..].join("\n");
    let Ok(mut suggest) = suggest.single_mut() else {
        return;
    };
    let (list, selected) = match &state.completing {
        Some(cycle) => (cycle.list.clone(), Some(cycle.index)),
        None => (suggestions(&state.input), None),
    };
    **suggest = list
        .iter()
        .enumerate()
        .map(|(i, entry)| format!("{}{entry}", if selected == Some(i) { "> " } else { "  " }))
        .collect::<Vec<_>>()
        .join("\n");
    let Ok(mut line) = line.single_mut() else {
        return;
    };
    **line = format!("> {}", state.input);
}

fn is_client(role: Option<&net::Role>) -> bool {
    role == Some(&net::Role::Client)
}

// ------------------------------------------------------------------------------ typing

/// Takes the line over from the history: `↑` walks back, `↓` forward, and what was
/// being typed when the browsing started comes back at the end of the list.
fn history_back(state: &mut Console) {
    if state.history.is_empty() {
        return;
    }
    state.history_pos = Some(match state.history_pos {
        None => {
            state.draft = state.input.clone();
            0
        }
        Some(pos) => (pos + 1).min(state.history.len() - 1),
    });
    let pos = state.history_pos.expect("just set");
    state.input = state.history[state.history.len() - 1 - pos].clone();
}

fn history_forward(state: &mut Console) {
    let Some(pos) = state.history_pos else {
        return;
    };
    if pos == 0 {
        state.history_pos = None;
        state.input = std::mem::take(&mut state.draft);
    } else {
        state.history_pos = Some(pos - 1);
        state.input = state.history[state.history.len() - 1 - (pos - 1)].clone();
    }
}

/// Completes the word the cursor stands behind: command names on the first word, the
/// command's own arguments afterwards. One candidate fills it in; further presses of
/// `Tab` cycle the candidates over the same word — any other keystroke ends the cycle.
fn complete(state: &mut Console) {
    // A cycle under way: take the next candidate, over the same word, without asking
    // the line again — the line meanwhile holds the last candidate, which is not what
    // is being typed.
    if let Some(cycle) = state.completing.as_mut() {
        cycle.index = (cycle.index + 1) % cycle.list.len();
        let picked = cycle.list[cycle.index].clone();
        let word = cycle.word;
        set_word(state, word, picked);
        return;
    }
    let ends_with_space = state.input.is_empty() || state.input.ends_with(char::is_whitespace);
    let words: Vec<&str> = state.input.split_whitespace().collect();
    let (word, prefix) = if ends_with_space {
        (words.len(), "")
    } else {
        (words.len() - 1, words.last().copied().unwrap_or(""))
    };
    let list = completions(&words, word, prefix);
    if list.is_empty() {
        return;
    }
    let picked = list[0].clone();
    set_word(state, word, picked);
    state.completing = Some(Cycle {
        list,
        index: 0,
        word,
    });
}

/// Puts `picked` in place as word number `word` of the line, words before it kept,
/// the cursor left behind it.
fn set_word(state: &mut Console, word: usize, picked: String) {
    let mut rebuilt: Vec<String> = state
        .input
        .split_whitespace()
        .take(word)
        .map(Into::into)
        .collect();
    rebuilt.push(picked);
    state.input = rebuilt.join(" ") + " ";
}

/// What the completion would offer for the line as it stands right now — the same list
/// the panel shows unselected, and what `Tab` then cycles through.
fn suggestions(line: &str) -> Vec<String> {
    let words: Vec<&str> = line.split_whitespace().collect();
    let ends_with_space = line.is_empty() || line.ends_with(char::is_whitespace);
    let (word, prefix) = if ends_with_space {
        (words.len(), "")
    } else {
        (words.len() - 1, words.last().copied().unwrap_or(""))
    };
    completions(&words, word, prefix)
}

/// The candidates for word number `word` of the line, of which `prefix` is typed.
fn completions(words: &[&str], word: usize, prefix: &str) -> Vec<String> {
    let prefix = prefix.trim_start_matches('/').to_ascii_lowercase();
    let base: Vec<String> = if word == 0 {
        COMMANDS.iter().map(|c| c.name.to_string()).collect()
    } else {
        let Some(first) = words.first().map(|word| word.trim_start_matches('/')) else {
            return Vec::new();
        };
        let Some(command) = COMMANDS.iter().find(|c| c.name.eq_ignore_ascii_case(first)) else {
            return Vec::new();
        };
        (command.args)(word - 1)
    };
    base.into_iter()
        .filter(|entry| entry.to_ascii_lowercase().starts_with(&prefix))
        .take(SUGGESTIONS)
        .collect()
}

// ----------------------------------------------------------------------------- commands

/// What a command finds when it runs: the simulation, the console to print into, the
/// view state, and whether this side has to ask the server for what it wants.
struct Ctx<'a> {
    sim: &'a mut Sim,
    console: &'a mut Console,
    /// The view state, which `fly` flips — the camera is nothing the simulation holds.
    camera: &'a mut CameraState,
    /// True on a multiplayer client — the world is the server's, so commands wish.
    client: bool,
    /// The weather a client's `weather` command asked the server for.
    wish: Option<Preset>,
}

struct Command {
    name: &'static str,
    /// i18n key of the usage line the completion and `help` show.
    usage: &'static str,
    /// i18n key of the one-line description.
    help: &'static str,
    /// The candidates for the n-th argument, 0-based.
    args: fn(usize) -> Vec<String>,
    run: fn(&mut Ctx, &[&str]),
}

const COMMANDS: [Command; 5] = [
    Command {
        name: "weather",
        usage: "console-usage-weather",
        help: "console-help-weather",
        args: weather_args,
        run: cmd_weather,
    },
    Command {
        name: "time",
        usage: "console-usage-time",
        help: "console-help-time",
        args: no_args,
        run: cmd_time,
    },
    Command {
        name: "fly",
        usage: "console-usage-fly",
        help: "console-help-fly",
        args: no_args,
        run: cmd_fly,
    },
    Command {
        name: "help",
        usage: "console-usage-help",
        help: "console-help-help",
        args: command_args,
        run: cmd_help,
    },
    Command {
        name: "clear",
        usage: "console-usage-clear",
        help: "console-help-clear",
        args: no_args,
        run: cmd_clear,
    },
];

fn no_args(_: usize) -> Vec<String> {
    Vec::new()
}

fn command_args(word: usize) -> Vec<String> {
    if word == 0 {
        COMMANDS.iter().map(|c| c.name.to_string()).collect()
    } else {
        Vec::new()
    }
}

fn weather_args(word: usize) -> Vec<String> {
    if word == 0 {
        Preset::ALL.iter().map(|p| preset_name(*p)).collect()
    } else {
        Vec::new()
    }
}

/// The name a preset is typed and printed under — its own English name, in whatever
/// case the code spells it, lower. Commands and arguments do not follow the interface
/// language; the sentences around them do.
fn preset_name(preset: Preset) -> String {
    format!("{preset:?}").to_ascii_lowercase()
}

/// Matches a preset by its English name, case-insensitively.
fn find_preset(wanted: &str) -> Option<Preset> {
    let wanted = wanted.to_ascii_lowercase();
    Preset::ALL
        .into_iter()
        .find(|preset| preset_name(*preset) == wanted)
}

fn cmd_weather(ctx: &mut Ctx, args: &[&str]) {
    let Some(wanted) = args.first() else {
        let now = ctx.sim.weather.now;
        let name = Preset::of(now)
            .map(preset_name)
            .unwrap_or_else(|| "custom".to_string());
        ctx.console.print(t!(
            "console-weather-now",
            weather = name,
            rate = i18n::decimal(f64::from(now.rate), 1),
            wind = i18n::decimal(f64::from(now.wind), 1),
            temp = i18n::decimal(f64::from(now.temperature), 1),
        ));
        return;
    };
    let Some(preset) = find_preset(wanted) else {
        ctx.console
            .print(t!("console-unknown-weather", name = *wanted));
        let list = Preset::ALL
            .iter()
            .map(|p| preset_name(*p))
            .collect::<Vec<_>>()
            .join(", ");
        ctx.console.print(t!("console-weather-list", list = list));
        return;
    };
    let name = preset_name(preset);
    if ctx.client {
        ctx.wish = Some(preset);
        ctx.console
            .print(t!("console-weather-asked", weather = name));
    } else {
        // The front moves in over `weather::TRANSITION` — rain builds from a first
        // drizzle and the rail goes greasy before it goes wet, exactly as a change of
        // air mass does.
        ctx.sim.weather.set(preset.weather(), ctx.sim.time);
        ctx.console.print(t!("console-weather-set", weather = name));
    }
}

/// Toggles the free camera: on, it detaches from the train and flies where it looks;
/// off, the driver is back in his seat. Purely local — the view is the one thing a
/// client owns outright, so there is nothing to ask the server about.
fn cmd_fly(ctx: &mut Ctx, _: &[&str]) {
    if ctx.camera.mode == CameraMode::Fly {
        ctx.camera.mode = CameraMode::Cab;
        ctx.console.print(t!("console-fly-off"));
    } else {
        ctx.camera.mode = CameraMode::Fly;
        ctx.console.print(t!("console-fly-on"));
    }
}

fn cmd_time(ctx: &mut Ctx, args: &[&str]) {
    let Some(wanted) = args.first() else {
        ctx.console
            .print(t!("console-time-now", time = clock_text(ctx.sim.clock())));
        return;
    };
    if ctx.client {
        ctx.console.print(t!("console-time-mp"));
        return;
    }
    let Some(target) = parse_clock(wanted) else {
        ctx.console.print(t!("console-usage-time"));
        return;
    };
    // Always forward: the run has no past to rewind — the trains keep their state while
    // the clock jumps, and the plan catches up on the next dispatch.
    let now = ctx.sim.clock().rem_euclid(DAY);
    let delta = (target - now).rem_euclid(DAY);
    if delta > 0.0 {
        ctx.sim.time += delta;
    }
    ctx.console
        .print(t!("console-time-set", time = clock_text(ctx.sim.clock())));
}

fn cmd_help(ctx: &mut Ctx, args: &[&str]) {
    let shown: Vec<&Command> = match args.first() {
        Some(wanted) => COMMANDS
            .iter()
            .filter(|c| c.name.eq_ignore_ascii_case(wanted))
            .collect(),
        None => COMMANDS.iter().collect(),
    };
    if shown.is_empty() {
        ctx.console.print(t!("console-unknown", name = args[0]));
        return;
    }
    for command in shown {
        ctx.console.print(format!(
            "{} {} · {}",
            command.name,
            lookup(command.usage),
            lookup(command.help)
        ));
    }
}

fn cmd_clear(ctx: &mut Ctx, _: &[&str]) {
    ctx.console.log.clear();
}

/// The wall clock of the run as `HH:MM:SS`.
fn clock_text(clock: f64) -> String {
    let seconds = clock.rem_euclid(DAY).floor() as u64;
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3_600,
        seconds / 60 % 60,
        seconds % 60
    )
}

/// Reads `HH:MM` or `HH:MM[:SS]` into seconds since midnight.
fn parse_clock(text: &str) -> Option<f64> {
    let mut parts = text.split(':');
    let hour: u32 = parts.next()?.parse().ok()?;
    let minute: u32 = parts.next()?.parse().ok()?;
    let second: u32 = parts.next().map_or(Ok(0), |s| s.parse()).ok()?;
    if parts.next().is_some() || hour >= 24 || minute >= 60 || second >= 60 {
        return None;
    }
    Some(f64::from(hour * 3_600 + minute * 60 + second))
}

/// Runs the line as it is typed: echoes it, remembers it, and hands it to its command.
///
/// Returns the weather a client asked the server for — the caller puts it on the wire,
/// because a command has no business knowing that a socket exists.
fn run_line(
    state: &mut Console,
    sim: &mut Sim,
    camera: &mut CameraState,
    client: bool,
) -> Option<Preset> {
    let line = state.input.trim().to_string();
    state.print(format!("> {line}"));
    state.input.clear();
    state.completing = None;
    state.history_pos = None;
    if line.is_empty() {
        return None;
    }
    if state.history.last().is_none_or(|last| *last != line) {
        state.history.push(line.clone());
    }
    let mut words = line.split_whitespace();
    // A leading slash is tolerated: other games' consoles take `/fly`, and the habit
    // comes with the player. What is looked up is the word without it.
    let name = words.next()?.trim_start_matches('/');
    let args: Vec<&str> = words.collect();
    let Some(command) = COMMANDS.iter().find(|c| c.name.eq_ignore_ascii_case(name)) else {
        state.print(t!("console-unknown", name = name));
        return None;
    };
    let mut ctx = Ctx {
        sim,
        console: state,
        camera,
        client,
        wish: None,
    };
    (command.run)(&mut ctx, &args);
    ctx.wish
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::weather::Precip;
    use track_model::{EdgeId, NodeKind, Segment, TrackEdge, TrackNetwork};
    use world_coords::geo::to_ecef_deg;

    /// A one-edge network to build a `Sim` on — the commands only ever touch the clock
    /// and the weather, so one straight piece of track is all they ever see.
    fn sim() -> Sim {
        let mut net = TrackNetwork::new();
        let a = net.add_node(NodeKind::Buffer);
        let b = net.add_node(NodeKind::Buffer);
        net.add_edge(TrackEdge::new(
            EdgeId(0),
            a,
            b,
            to_ecef_deg(52.0, 10.0, 100.0),
            0.0,
            vec![Segment::straight(2_000.0)],
        ));
        net.finish();
        Sim::new(net, sim_core::interlock::Interlock::default(), 1)
    }

    #[test]
    fn clocks_read_and_print_round_the_day() {
        assert_eq!(parse_clock("8:05"), Some(8.0 * 3_600.0 + 300.0));
        assert_eq!(
            parse_clock("21:40:15"),
            Some(21.0 * 3_600.0 + 40.0 * 60.0 + 15.0)
        );
        assert_eq!(parse_clock("24:00"), None, "a day has no 24th hour");
        assert_eq!(parse_clock("12:60"), None);
        assert_eq!(parse_clock("12:30:99"), None);
        assert_eq!(parse_clock("12:30:00:00"), None);
        assert_eq!(parse_clock("half past"), None);
        assert_eq!(clock_text(0.0), "00:00:00");
        assert_eq!(clock_text(8.0 * 3_600.0 + 5.0 * 60.0), "08:05:00");
        // The clock keeps growing past a day on multi-day runs; the wall clock wraps.
        assert_eq!(clock_text(DAY + 1_800.0), "00:30:00");
    }

    /// The time command only ever moves the clock forward — the run has no past to
    /// rewind, and the next occurrence of an earlier time of day is tomorrow's.
    #[test]
    fn the_time_command_moves_forward_only() {
        let mut console = Console::default();
        let mut sim = sim();
        // Midnight start, so the wall clock reads the run time and nothing else.
        sim.start.hour = 0;
        sim.start.minute = 0;
        sim.time = 20.0 * 3_600.0;
        let mut camera = CameraState::default();
        let mut ctx = Ctx {
            sim: &mut sim,
            console: &mut console,
            camera: &mut camera,
            client: false,
            wish: None,
        };
        cmd_time(&mut ctx, &["06:00"]);
        // Half a day forward to tomorrow's six, not ten hours back.
        assert_eq!(clock_text(ctx.sim.clock()), "06:00:00");
        assert!((ctx.sim.time - 30.0 * 3_600.0).abs() < 1e-9);
    }

    #[test]
    fn presets_are_found_by_their_english_name() {
        assert_eq!(find_preset("rain"), Some(Preset::Rain));
        assert_eq!(
            find_preset("RAIN"),
            Some(Preset::Rain),
            "case does not matter"
        );
        assert_eq!(find_preset("no-such-weather"), None);
    }

    #[test]
    fn completions_follow_the_word_being_typed() {
        // An empty line offers every command.
        assert_eq!(
            suggestions(""),
            COMMANDS
                .iter()
                .map(|c| c.name.to_string())
                .collect::<Vec<_>>()
        );
        // A prefix narrows them.
        assert_eq!(suggestions("cl"), vec!["clear".to_string()]);
        // A slash before the command is tolerated, as in the consoles the habit of
        // typing one comes from.
        assert_eq!(suggestions("/fl"), vec!["fly".to_string()]);
        assert_eq!(suggestions("/weather rai"), vec!["rain".to_string()]);
        // A finished word followed by a space offers that command's arguments.
        let list = suggestions("weather ");
        assert!(list.contains(&"rain".to_string()));
        assert!(list.len() <= SUGGESTIONS);
        // … and the arguments complete from a prefix.
        assert_eq!(suggestions("weather rai"), vec!["rain".to_string()]);
        // A word that is no command offers nothing.
        assert!(suggestions("frobnicate ").is_empty());
        assert!(suggestions("frobnicate").is_empty());
    }

    #[test]
    fn a_line_is_echoed_and_then_runs() {
        let mut console = Console::default();
        let mut sim = sim();
        let mut camera = CameraState::default();
        console.input = "frobnicate now".into();
        assert_eq!(run_line(&mut console, &mut sim, &mut camera, false), None);
        assert!(
            console.log.iter().any(|l| l.contains("> frobnicate now")),
            "the line is echoed before it is answered"
        );
        assert!(
            console
                .log
                .iter()
                .any(|l| l.contains("frobnicate") && !l.starts_with('>'))
        );
        // A known command runs, and a client's weather becomes a wish for the server.
        console.input = "weather rain".into();
        assert_eq!(run_line(&mut console, &mut sim, &mut camera, false), None);
        assert_eq!(
            sim.weather.now.precip,
            Precip::None,
            "the transition has only just started — the rain is not here yet"
        );
        console.input = "weather snow".into();
        assert_eq!(
            run_line(&mut console, &mut sim, &mut camera, true),
            Some(Preset::Snow)
        );
        assert_eq!(
            sim.weather.now.precip,
            Precip::None,
            "a client does not touch the world it does not own"
        );
        // The line went into the history, newest last.
        assert_eq!(
            console.history,
            ["frobnicate now", "weather rain", "weather snow"]
        );
    }

    /// `fly` hands the view to the free camera and takes it back — and the slash of the
    /// `/fly` spelling changes nothing, only the way the command is written.
    #[test]
    fn fly_toggles_the_free_camera() {
        let mut console = Console::default();
        let mut sim = sim();
        let mut camera = CameraState::default();
        console.input = "fly".into();
        assert_eq!(run_line(&mut console, &mut sim, &mut camera, false), None);
        assert_eq!(camera.mode, CameraMode::Fly);
        console.input = "/fly".into();
        assert_eq!(run_line(&mut console, &mut sim, &mut camera, false), None);
        assert_eq!(camera.mode, CameraMode::Cab);
        assert!(
            console.log.iter().any(|l| l.contains("> /fly")),
            "the echo shows the line as it was typed"
        );
        // A client flies its own camera: no wish for the server comes of it.
        console.input = "/fly".into();
        assert_eq!(run_line(&mut console, &mut sim, &mut camera, true), None);
        assert_eq!(camera.mode, CameraMode::Fly);
    }

    #[test]
    fn the_history_walks_back_and_forward_over_the_draft() {
        let mut console = Console {
            input: "hel".into(),
            history: vec!["weather rain".into(), "time 8:00".into()],
            ..Default::default()
        };
        history_back(&mut console);
        assert_eq!(console.input, "time 8:00");
        history_back(&mut console);
        assert_eq!(console.input, "weather rain");
        history_back(&mut console);
        assert_eq!(console.input, "weather rain", "the oldest line holds");
        history_forward(&mut console);
        assert_eq!(console.input, "time 8:00");
        history_forward(&mut console);
        assert_eq!(console.input, "hel", "and the draft comes back");
        history_forward(&mut console);
        assert_eq!(console.input, "hel", "nothing past the draft");
    }

    #[test]
    fn tab_completes_and_cycles() {
        let mut console = Console {
            input: "we".into(),
            ..Default::default()
        };
        complete(&mut console);
        assert_eq!(console.input, "weather ");
        // The player moves on: a space ends the cycle over the command names, and the
        // arguments complete from there — cycling walks the presets, one per press.
        console.input = "weather ".into();
        console.completing = None;
        complete(&mut console);
        let first = console.input.clone();
        assert!(
            first.starts_with("weather clear "),
            "the presets come in their own order, got {first:?}"
        );
        complete(&mut console);
        assert_ne!(
            console.input, first,
            "the second press takes the next preset"
        );
        assert!(console.completing.is_some());
        // A keystroke resets the cycle, and a prefix picks one preset out.
        console.input = "weather r".into();
        console.completing = None;
        complete(&mut console);
        assert_eq!(console.input, "weather rain ");
    }
}
