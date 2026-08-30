//! Connected Rails — Bevy app: rendering, camera, input, HUD (plan ch. 12).
//!
//! The app ticks `sim-core` with a fixed time step and mirrors the state into ECS components.
//! Simulation logic does **not** belong here.

mod audio;
mod bindings;
mod cab;
mod console;
mod crew;
mod displays;
mod glyphs;
mod hud;
mod menu;
mod models;
mod mods_ui;
mod net;
mod render;
mod services;
mod settings;
mod signals;
mod streaming;
mod theme;
mod ui;
mod walk;
mod world;

use ai_driver::AiDriver;
use bevy::ecs::resource::IsResource;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::picking::mesh_picking::{MeshPickingCamera, MeshPickingPlugin, MeshPickingSettings};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use content::import::dgm::TerrainSource;
use content::terrain::{
    Scenery, TerrainBuilder, TerrainEdits, TerrainOptions, TerrainStats, Vegetation,
};
use content::vehicles::passenger_coach;
use mod_runtime::ModRuntime;
use render::{Origin, TerrainChunk, VehicleView, WorldAnchored};
use sim_core::Sim;
use sim_core::train::{Train, Vehicle, VehicleSpec};
use track_model::TrackPosition;
use world_coords::RenderOrigin;
// Daylight factor of this frame, 0 (night) … 1 (full day) — written by
// `update_daylight`, read by everything that switches with darkness: the
// headlights here, the mods' `_NIGHT` nodes in `world-render`.
use world_render::{Daylight, PeopleClock, sky};

/// Menu first, the world only on starting the run — that is what lets a mod toggled on
/// the menu apply without restarting the process.
///
/// `Paused` is the run standing still under the Esc overlay: every driving system is
/// gated on `Driving`, so entering it freezes the simulation, the clock and the camera
/// while the menu keeps running.
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GameState {
    #[default]
    Menu,
    Driving,
    Paused,
}

/// The running simulation.
#[derive(Resource)]
pub struct SimResource(pub Sim);

/// Headlight cone at one end of a train (M6 night lighting). `reverse` marks the
/// cone on the rear end; `update_headlights` lights the one facing the direction
/// of travel while the cab's light switch is on.
#[derive(Component)]
struct Headlight {
    train: usize,
    reverse: bool,
}

/// Red tail lamp (Zg 101) at one end of a train. `update_headlights` shows the
/// pair on the end the train runs *away* from while the light switch is on —
/// emissive lenses, so bloom does the glowing after dark.
#[derive(Component)]
struct TailLamp {
    train: usize,
    reverse: bool,
}

/// Cab light of the player's leading vehicle (`CabInputs::cab_light`).
#[derive(Component)]
struct CabLamp;

/// Precipitation field around the camera: one static mesh of crossed quads,
/// moved downwards and wrapped every [`PRECIP_PERIOD`] metres (`update_precipitation`).
#[derive(Component)]
pub(crate) struct Precipitation {
    snow: bool,
    /// Fall speed [m/s].
    speed: f32,
    /// The layer of a few big out-of-focus drops right in front of the lens.
    near: bool,
}

/// Height of one repetition of the precipitation mesh [m]. The mesh repeats its
/// particles three times in y, so wrapping the fall offset keeps the camera
/// covered by at least ±one period.
const PRECIP_PERIOD: f32 = 24.0;

/// Beyond this the near-field fog is off: the atmosphere's aerial perspective is
/// the haze of a clear day, and a second one on top of it would be a grey veil.
const CLEAR_VISIBILITY: f32 = 8_000.0;

/// Which train is driven by the player.
#[derive(Resource)]
pub struct PlayerTrain(pub usize);

/// AI drivers of the remaining trains.
#[derive(Resource)]
pub struct AiDrivers(pub Vec<(usize, AiDriver)>);

/// Loaded mods with their Lua state (plan ch. 19).
#[derive(Resource)]
pub struct Mods(pub ModRuntime);

/// Terrain view distance [m] — tiles beyond it are hidden. Also the streaming load
/// radius: nothing further away is built, so nothing further away can be drawn either.
/// Comes from the settings (`settings::Graphics::view_distance`) when the run starts.
#[derive(Resource)]
pub struct ViewDistance(pub f32);

/// Key figures of the generated terrain (for the HUD).
#[derive(Resource, Default)]
pub struct TerrainInfo(pub TerrainStats);

/// Number of frames from `--frames N` (CI rendering smoke test, plan ch. 18).
#[derive(Resource)]
struct FrameLimit(u32);

/// Target file from `--screenshot <file.png>`.
#[derive(Resource)]
struct ShotPath(String);

/// The font everything numeric draws with — the full Fira Mono, not Bevy's ASCII subset
/// of it. The HUD, the cab displays and the mod panel are laid out in columns, so they
/// need the fixed advance; the menu keeps it for figures, ids and key caps.
/// Compiled in rather than loaded, so the binary stays self-contained.
const UI_FONT: &[u8] = include_bytes!("../fonts/FiraMono-Regular.ttf");

/// Fira Sans for the menu's prose. Same family as the mono above and the same licence
/// file (SIL OFL 1.1, `fonts/LICENSE-Fira.txt`) — prose and figures therefore read as
/// one typeface rather than as two fonts that happen to sit next to each other.
const MENU_FONT: &[u8] = include_bytes!("../fonts/FiraSans-Regular.ttf");
const MENU_FONT_SEMIBOLD: &[u8] = include_bytes!("../fonts/FiraSans-SemiBold.ttf");

/// The picture behind the main menu, compiled in like the fonts so the binary needs no
/// asset directory beside it.
///
/// ponytail: **placeholder, not ours to ship** — replace it with our own render before
/// any release. A single file swap; nothing reads it but the menu.
const MENU_BACKGROUND: &[u8] = include_bytes!("../images/menu-background.jpg");

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    // A dedicated server never gets as far as a window: it builds the same world and
    // serves it, and that is the whole process (`net.rs`).
    if let Some(address) = flag("--dedicated").or_else(|| {
        args.iter()
            .any(|a| a == "--dedicated")
            .then(|| net::DEFAULT_PORT.to_string())
    }) {
        net::run_dedicated(&address);
        return;
    }

    let shot = flag("--screenshot");
    if let Some(dir) = shot.as_ref().and_then(|p| std::path::Path::new(p).parent()) {
        let _ = std::fs::create_dir_all(dir);
    }
    // Without `--frames`, about a second of run-up is enough for an image.
    let frame_limit = flag("--frames")
        .and_then(|n| n.parse::<u32>().ok())
        .or_else(|| shot.as_ref().map(|_| 60));
    // Any run flag skips the menu — the documented CLI and CI invocations stay
    // non-interactive.
    let run_flags = [
        "--line",
        "--loco",
        "--scenario",
        "--camera",
        "--dgm",
        "--frames",
        "--screenshot",
        "--connect",
        "--time",
        "--date",
        "--weather",
        "--wipers",
        "--console",
    ];
    // `--menu` overrules them again — the only way to put the menu itself in front of
    // `--screenshot`, which would otherwise photograph the world behind it. It takes an
    // optional page (`--menu settings`), because a screenshot cannot press keys.
    let menu_page = flag("--menu").filter(|page| !page.starts_with("--"));
    let autostart =
        !args.iter().any(|a| a == "--menu") && args.iter().any(|a| run_flags.contains(&a.as_str()));

    let mut app = App::new();
    // Models, textures and sounds of a mod come from its own directory: `mods://<mod>/…`.
    // Has to be registered before the asset plugin.
    app.register_asset_source(world_render::MOD_SOURCE, world_render::mod_asset_source());
    // Settings before the window: loading happens while the plugin is built, so the
    // stored language translates the title and the stored window mode creates the
    // window — no flip on the first frame.
    app.add_plugins((settings::plugin, bindings::plugin));
    let graphics = app.world().resource::<settings::Graphics>().clone();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: i18n::t!("window-simulator"),
                    mode: settings::window_mode(&graphics),
                    present_mode: settings::present_mode(&graphics),
                    ..default()
                }),
                ..default()
            })
            // The mixer is kira's (`audio.rs`); Bevy's own audio would open a second output
            // device and hold it for nothing.
            .disable::<bevy::audio::AudioPlugin>(),
    )
    .add_plugins(app_icon::plugin)
    // Terrain splatting (plan ch. 14): shader and material, shared with the
    // route editor, which draws the same ground.
    .add_plugins(world_render::WorldRenderPlugin)
    // Frame time and entity count for the F6 panel — the two numbers that say
    // whether the streaming keeps up.
    .add_plugins((
        bevy::diagnostic::FrameTimeDiagnosticsPlugin::default(),
        bevy::diagnostic::EntityCountDiagnosticsPlugin::default(),
    ))
    // The mixer (`audio.rs`) — opened here rather than in `Startup`, because the initial
    // state transition into `Driving` runs before any startup schedule.
    .add_plugins(audio::plugin)
    // The atmosphere covers every pixel the world does not; this is only what
    // shows before the first sky pass.
    .insert_resource(ClearColor(Color::BLACK))
    // Mouse picking for the 3D cab: only marked control meshes catch the ray —
    // without the marker requirement every terrain tile would compete for it.
    .add_plugins(MeshPickingPlugin)
    .insert_resource(MeshPickingSettings {
        require_markers: true,
        ..default()
    })
    // Multiplayer, if the command line asked for it — otherwise this adds nothing.
    .add_plugins(net::plugin)
    // The command console (F8): its panel exists for the whole process, its typing runs
    // in the driving chain, and its weather wishes travel as a message whether a socket
    // does or not.
    .add_plugins(console::plugin)
    .init_resource::<ui::CameraState>()
    .init_resource::<walk::Walker>()
    .init_resource::<cab::CabMouse>()
    .init_resource::<mods_ui::ModManager>()
    .init_resource::<hud::Overlays>()
    .init_resource::<menu::MenuState>()
    .init_resource::<menu::Selection>()
    // HTML cab screens hold a boa script context, which is `!Send` — a non-send
    // resource keeps them on the main thread, where the display chain runs anyway.
    .init_non_send::<displays::HtmlGauges>()
    // Mods before menu and world — both read the resource. Inserted while the app is
    // built: the initial state transition runs before every startup schedule, so a
    // loading system would come too late for `setup`.
    .insert_resource(Mods(ModRuntime::load("mods")))
    .insert_state(if autostart {
        GameState::Driving
    } else {
        GameState::Menu
    })
    // The wish to take a train over. It is written in single player too and read only by
    // the network layer, so the type has to exist whether a socket does or not.
    .add_message::<net::TakeOverRequest>()
    // The console's weather wish, the same shape of thing: written in single player,
    // posted only where a socket exists (`net::client_send`).
    .add_message::<net::WeatherRequest>()
    .add_systems(Startup, log_mods)
    // The world the last run built goes first — otherwise the next `setup` would put a
    // second one on top of it.
    .add_systems(
        OnEnter(GameState::Menu),
        (tear_down_run, menu::spawn_menu).chain(),
    )
    // The same menu, as an overlay over the standing world.
    .add_systems(OnEnter(GameState::Paused), menu::spawn_pause)
    .add_systems(
        Update,
        (menu::menu, menu::scroll_menu)
            .chain()
            .run_if(in_state(GameState::Menu).or_else(in_state(GameState::Paused))),
    )
    .add_systems(
        Update,
        pause_on_escape
            .after(console::console)
            .run_if(in_state(GameState::Driving)),
    )
    // Both run in every state: the pause menu needs its cursor back, and the HUD has to
    // go away behind the overlay rather than shine through it. The console's panel goes
    // with them — it hides behind the pause overlay like the HUD does.
    .add_systems(
        Update,
        (ui::grab_cursor, hud::hud_visibility, hud::refresh_help_caps),
    )
    // The sound table and the display cameras need the trains, which `setup` only
    // creates when its commands are applied — the chain inserts that sync point. It runs
    // when there is no run yet: coming back from the pause overlay enters `Driving` too,
    // and building the world a second time is not what resuming means (`RunBuilt`).
    .add_systems(
        OnEnter(GameState::Driving),
        (
            remember_before_run,
            setup,
            audio::setup_audio,
            displays::setup_displays,
            mark_run_built,
        )
            .chain()
            .run_if(not(resource_exists::<RunBuilt>)),
    )
    .add_systems(
        Update,
        (
            // The simulation, in the order one step of it happens: the console first —
            // it holds the keyboard while it is open, and its answer to Enter lands
            // before the step reads the world — then the levers, who is on them, what
            // the plan puts on the line, the AI, and the step itself. A group of its
            // own because Bevy's tuples end at twenty.
            (
                console::console,
                ui::player_input,
                cab::apply_mouse,
                crew::crew_change,
                dispatch_services,
                drive_ai,
                step_simulation,
            )
                .chain(),
            feed_people_clock,
            run_mod_scripts,
            displays::update_displays,
            rebase_origin,
            sync_vehicles,
            feed_sky,
            update_headlights,
            walk::walk_player,
            ui::camera_control,
            walk::place_character,
            walk::animate_walker,
            update_precipitation,
            streaming::stream_terrain,
            terrain_visibility,
            hud::update_hud,
            audio::update_audio,
            mods_ui::mod_manager,
        )
            .chain()
            .run_if(in_state(GameState::Driving)),
    )
    // Vehicle models from mods: bind glTF nodes, switch LODs, move parts (plan ch. 15.3).
    .add_systems(
        Update,
        (
            models::bind_nodes,
            models::update_lod,
            models::animate_parts,
            models::animate_backlight,
            models::animate_controls,
            models::animate_digits,
            models::update_windscreens,
            displays::bind_display_nodes,
            cab::update_highlight,
            world_render::mount_parts,
            world_render::bind_lamps,
            signals::update_lamps,
            signals::animate_motions,
            signals::update_signal_lods,
            signals::update_placeholders,
        )
            .after(sync_vehicles)
            .run_if(in_state(GameState::Driving)),
    );
    // Bevy ships an ASCII subset of Fira Mono as the default font, which leaves every
    // umlaut and every arrow in the German UI as a box. Overwriting the asset the empty
    // `TextFont` handle points at swaps it for the full face everywhere at once — HUD,
    // mod panel and cab displays — and it is the same typeface, so nothing moves.
    // The menu asks for the two Fira Sans faces by handle on top of that.
    let fonts = {
        let mut assets = app.world_mut().resource_mut::<Assets<Font>>();
        assets
            .insert(AssetId::default(), Font::from_bytes(UI_FONT.to_vec()))
            .expect("the default font slot takes the full Fira Mono");
        theme::Fonts {
            sans: assets.add(Font::from_bytes(MENU_FONT.to_vec())),
            semibold: assets.add(Font::from_bytes(MENU_FONT_SEMIBOLD.to_vec())),
        }
    };
    app.insert_resource(fonts);
    let wallpaper = bevy::image::Image::from_buffer(
        MENU_BACKGROUND,
        bevy::image::ImageType::Extension("jpg"),
        bevy::image::CompressedImageFormats::NONE,
        true,
        bevy::image::ImageSampler::linear(),
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    )
    .expect("the compiled-in menu background decodes");
    let wallpaper = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(wallpaper);
    app.insert_resource(theme::Wallpaper(wallpaper));
    if let Some(frames) = frame_limit {
        app.insert_resource(FrameLimit(frames))
            .add_systems(Update, exit_after_frames);
    }
    if let Some(path) = shot {
        app.insert_resource(ShotPath(path));
    }
    if let Some(page) = menu_page {
        app.insert_resource(menu::StartPage(page));
    }
    app.run();
}

/// Exits the app after the given number of frames — with `--screenshot` the window is
/// captured beforehand.
///
/// The last frame's diagnostics go into the log on the way out. A `--frames`
/// run is how the rendering is measured (the forest test of
/// `tools/trees/bench_forest.mjs` is one), and reading them off a screenshot of
/// the F6 overlay is neither scriptable nor precise.
fn exit_after_frames(
    limit: Res<FrameLimit>,
    shot: Option<Res<ShotPath>>,
    diagnostics: Res<bevy::diagnostic::DiagnosticsStore>,
    terrain: Res<TerrainInfo>,
    mut commands: Commands,
    mut count: Local<u32>,
    mut exit: MessageWriter<AppExit>,
) {
    *count += 1;
    if *count == limit.0 {
        let perf = hud::Perf::read(&diagnostics);
        info!(
            "after {} frames: {:.0} fps, {:.1} ms, {} entities; \
             terrain {} tiles, {} triangles, {:.1} MB",
            limit.0,
            perf.fps,
            perf.frame_ms,
            perf.entities,
            terrain.0.tiles,
            terrain.0.triangles,
            terrain.0.memory() as f64 / (1024.0 * 1024.0),
        );
    }
    let Some(shot) = shot else {
        if *count >= limit.0 {
            exit.write(AppExit::Success);
        }
        return;
    };
    if *count == limit.0 {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(shot.0.clone()));
    }
    // The capture goes through the render thread: it only lands on disk a few frames later.
    if *count >= limit.0 + 10 {
        exit.write(AppExit::Success);
    }
}

/// Everything that was alive before the run was built: the window, the picking pointers,
/// the menu, the cloud dome. What is not in here is the run.
#[derive(Resource)]
struct BeforeRun(std::collections::HashSet<Entity>);

/// What a run may have built, and nothing a despawn has any business touching:
///
/// * a **resource** is an entity of its own in Bevy 0.19, and despawning one would
///   *remove* the resource — the run's are replaced by the next `setup`, and the rest
///   are not the run's to take;
/// * an **observer** belongs to no world at all — Bevy drops the ones whose watched
///   entity has gone and keeps the rest, which is exactly right here;
/// * [`world_render::Persistent`] is what a plugin puts up once at startup, and a run it
///   was never part of must not be able to take it down.
type RunRoots<'w, 's> = Query<
    'w,
    's,
    Entity,
    (
        Without<ChildOf>,
        Without<world_render::Persistent>,
        Without<IsResource>,
        Without<Observer>,
    ),
>;

/// Takes that snapshot — chained in front of `setup`, so it sees the world without one.
fn remember_before_run(mut commands: Commands, entities: Query<Entity>) {
    commands.insert_resource(BeforeRun(entities.iter().collect()));
}

/// A run's world stands built. `OnEnter(GameState::Driving)` fires on the way back out of
/// the pause overlay exactly as it does on the way in from the menu, and the chain behind
/// it *builds* a run: without this guard, resuming would put a second world, a second
/// camera and a second simulation on top of the first, and drive the one the player is
/// no longer looking through.
///
/// Set last in the chain and dropped by [`tear_down_run`], so the state is "a run exists"
/// rather than "the player was once driving": leaving for the title screen tears the
/// world down and the next `Drive` builds one again.
#[derive(Resource)]
struct RunBuilt;

/// Closes the chain that built the run.
fn mark_run_built(mut commands: Commands) {
    commands.insert_resource(RunBuilt);
}

/// Drops the built world when the player leaves a run for the title screen, so the next
/// `setup` builds into an empty world rather than beside the old one.
///
/// Roots only — `despawn` takes the children with it — and only the ones [`RunRoots`]
/// lets through. An entity id carries a generation, so an id the run reused for something
/// of its own is a *different* [`Entity`] than the one remembered and is dropped as it
/// should be.
fn tear_down_run(
    mut commands: Commands,
    before: Option<Res<BeforeRun>>,
    roots: RunRoots,
    mixer: Option<ResMut<audio::Audio>>,
    mut walker: ResMut<walk::Walker>,
    mut camera: ResMut<ui::CameraState>,
) {
    // No run has been built yet — this is the title screen the program starts on.
    let Some(before) = before else {
        return;
    };
    for entity in &roots {
        if !before.0.contains(&entity) {
            commands.entity(entity).despawn();
        }
    }
    // A mixer track outlives the entity it followed: dropping the tracks is what stops
    // the loops of a run the player has just left.
    if let Some(mut mixer) = mixer {
        mixer.silence();
    }
    // Both of these point into the world that has just gone: the walker at a vehicle or
    // at a place on the earth, the camera at a wayside spot beside it.
    *walker = default();
    *camera = default();
    commands.remove_resource::<BeforeRun>();
    commands.remove_resource::<RunBuilt>();
}

/// The pause key during a run raises the overlay, which also holds the settings. Leaving
/// it again is the overlay's own job — this system only runs while `Driving`, so the Esc
/// that resumes cannot bounce straight back into the pause. The menu's own Esc stays a
/// key rather than a binding: whatever the pause is bound to, there is always a way out.
///
/// While the console is open, Esc belongs to it: it closes the console, and the pause
/// waits for the next press. `console::console` runs first in the driving chain, so the
/// closing has already happened when this reads the flag.
fn pause_on_escape(
    input: bindings::Input,
    console: Res<console::Console>,
    mut next: ResMut<NextState<GameState>>,
) {
    if console.open {
        return;
    }
    if input.just_pressed(bindings::Action::Pause) {
        next.set(GameState::Paused);
    }
}

/// Logs what the mod loading found — the loading itself happens while the app is built.
fn log_mods(mods: Res<Mods>) {
    let mods = &mods.0;
    for warning in mods.log() {
        warn!("mod: {warning}");
    }
    info!(
        "Mods: {} of {} enabled ({} vehicles, {} lines, {} compositions, {} scenarios, \
         {} timetables, {} operating days, {} signal types, {} scripts)",
        mods.mods.manifests.iter().filter(|m| m.enabled).count(),
        mods.mods.manifests.len(),
        mods.mods.vehicles.len(),
        mods.mods.lines.len(),
        mods.mods.compositions.len(),
        mods.mods.scenarios.len(),
        mods.mods.timetables.len(),
        mods.mods.days.len(),
        mods.mods.signal_types.len(),
        mods.mods.scripts.len()
    );
}

#[allow(clippy::too_many_arguments)]
fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    world_materials: render::WorldMaterials,
    mut images: ResMut<Assets<Image>>,
    mut terrain_materials: ResMut<Assets<render::TerrainMaterial>>,
    mut media: ResMut<Assets<bevy::light::atmosphere::ScatteringMedium>>,
    mut star_materials: ResMut<Assets<sky::StarMaterial>>,
    mut moon_materials: ResMut<Assets<sky::MoonMaterial>>,
    mut precip_materials: ResMut<Assets<world_render::precipitation::PrecipitationMaterial>>,
    assets: Res<AssetServer>,
    mut mods: ResMut<Mods>,
    mut manager: ResMut<mods_ui::ModManager>,
    selection: Res<menu::Selection>,
    graphics: Res<settings::Graphics>,
    binds: Res<bindings::Binds>,
    fonts: Res<theme::Fonts>,
) {
    // Back to the names the body has always used.
    let render::WorldMaterials {
        standard: mut materials,
        rail: mut rail_materials,
    } = world_materials;

    // `--hud full|reduced|off` puts the display in one of its three steps for a
    // screenshot, which cannot press a key. It goes into a resource of its own rather than
    // into the setting: the settings file is written on exit whether anything changed or
    // not, and a photograph must not leave its step behind in the player's preferences.
    if let Some(step) = arg("--hud") {
        match step.as_str() {
            "off" => commands.insert_resource(hud::HudOverride(settings::HudMode::Off)),
            "reduced" => commands.insert_resource(hud::HudOverride(settings::HudMode::Reduced)),
            "full" => commands.insert_resource(hud::HudOverride(settings::HudMode::Full)),
            other => warn!("unknown --hud step {other}"),
        }
    }
    // A mod was toggled on the menu: reload, so the world is built from the set on disk.
    if manager.restart_needed {
        mods.0 = ModRuntime::load("mods");
        for warning in mods.0.log() {
            warn!("mod: {warning}");
        }
        manager.restart_needed = false;
    }
    let mods = &mut mods.0;

    let world::World {
        mut sim,
        player,
        drivers,
        line: line_source,
        day,
        dispatch,
    } = world::build(mods, &selection);
    // `--time 21:40` and `--date 2026-10-03` move the run's wall clock, the way
    // `--hud` moves the display: a screenshot cannot open the scenario file, and
    // the night sky is exactly what a rendering smoke test wants to see.
    if let Some(clock) = arg("--time")
        && let Some((hour, minute)) = parse_pair(&clock, ':')
    {
        sim.start.hour = hour;
        sim.start.minute = minute;
    }
    if let Some(date) = world::date_arg() {
        sim.start.year = date.year;
        sim.start.month = date.month;
        sim.start.day = date.day;
    }
    // `--wipers 2` starts with the wipers running: they are a cab control, and a
    // screenshot has no hands.
    if let Some(mode) = arg("--wipers").and_then(|m| m.parse::<u8>().ok()) {
        for cab in &mut sim.controls {
            cab.wipers = mode.min(3);
        }
    }
    // `--weather snow` starts one of `sim_core::weather`'s presets. In a normal
    // run the front *moves in* over `weather::TRANSITION` — rain builds from a
    // first drizzle, the pane wets slowly, the rail goes greasy before wet. Only
    // a screenshot gets it placed at once, ground and all: it cannot wait five
    // minutes, and it wants the end state, not the approach.
    if let Some(name) = arg("--weather") {
        let wanted = name.to_ascii_lowercase();
        match sim_core::weather::Preset::ALL
            .into_iter()
            .find(|p| format!("{p:?}").to_ascii_lowercase() == wanted)
        {
            Some(preset) if arg("--screenshot").is_some() => {
                sim.weather.place(preset.weather(), 0.0)
            }
            Some(preset) => sim.weather.set(preset.weather(), 0.0),
            None => warn!("unknown weather: {name}"),
        }
    }
    // Both sides of a multiplayer run have to have built the same world; the fingerprint
    // is what says so on joining (`net.rs`).
    let fingerprint = world::fingerprint(&line_source.name, &sim);

    // Render origin at the head of the train. A consist with no vehicles stands nowhere,
    // so the origin starts at the line's own anchor instead (`sim_core::shunt`).
    let start = sim.trains[player]
        .vehicles
        .first()
        .map(|v| v.pos.pose(&sim.net).pos)
        .unwrap_or_else(|| {
            sim.net
                .edges()
                .first()
                .map_or_else(world_coords::EcefPos::default, |e| e.eval(0.0).pos)
        });
    let origin = RenderOrigin::new(start);

    // Ground, scenery and foliage wear the season of the scenario's start date
    // — the same date the sun and moon are computed from (plan ch. 14).
    let season = render::Season::on(sim.start.month, sim.start.day);

    // Terrain: from real elevation data with `--dgm <directory>`, otherwise flat.
    // Tiles are not built here but while driving (plan 4.3) — a 100 km line has more
    // terrain than fits in memory at once. The builder exists before the scenery,
    // because objects that snap to the terrain ask it for the ground height.
    // `--dgm` may be repeated for a line across a UTM zone boundary; the n-th
    // `--epsg` belongs to the n-th `--dgm` (the last one carries on when there
    // are fewer).
    let zones = args_all("--epsg");
    let mut sources = Vec::new();
    for (i, dir) in args_all("--dgm").iter().enumerate() {
        let zone = zones
            .get(i)
            .or_else(|| zones.last())
            .and_then(|v| v.parse().ok())
            .and_then(world_coords::geo::utm_zone_from_epsg)
            .unwrap_or(32);
        match TerrainSource::from_dir(dir, zone) {
            Ok(s) => {
                info!("DGM: {} tiles from {dir} (zone {zone})", s.tile_count());
                sources.push(s);
            }
            Err(e) => warn!("DGM {dir} not readable: {e}"),
        }
    }
    // Height data the module ships with itself — behind the `--dgm` sources, so
    // whoever passes the original delivery keeps its finer grid.
    for h in &line_source.heights {
        let Some(dir) = mods.mods.resolve_path(&h.path) else {
            warn!("height data {}: mod not installed", h.path);
            continue;
        };
        match TerrainSource::from_dir(&dir, h.zone) {
            Ok(s) => {
                info!(
                    "module heights: {} tiles from {} (zone {})",
                    s.tile_count(),
                    dir.display(),
                    h.zone
                );
                sources.push(s);
            }
            Err(e) => warn!("height data {} not readable: {e}", dir.display()),
        }
    }
    let terrain_options = TerrainOptions {
        zone: dgm_zone(),
        fallback_height: 100.0,
        ..default()
    };
    // The people (plan ch. 12): the passengers the crowd and the seats are made
    // of, in registry order, and the walker's own body. Nothing about them is
    // replicated — the crowd is a function of the line's name and the seats of
    // the train's indices, so every client shows the same faces.
    let passenger_names: Vec<String> = mods
        .mods
        .characters
        .iter()
        .filter(|(_, c)| c.has_role(content::Role::Passenger))
        .map(|(key, _)| key.clone())
        .collect();
    let crowd = content::Crowd::from_line(
        &line_source,
        &sim.net,
        terrain_options.zone,
        &passenger_names,
        content::people::line_seed(&line_source.name),
    );
    info!(
        "people: {} on the platforms and ways ({} of them walking), {} passenger characters installed",
        crowd.len(),
        crowd.walking(),
        passenger_names.len()
    );
    let passengers =
        world_render::Passengers::resolve(&passenger_names, &mods.mods.characters, &assets);
    // The line's farmland, cut to the tiles it covers (see `content::farmland`).
    let farmland = content::farmland::Fields::from_line(
        &line_source,
        terrain_options.zone,
        terrain_options.tile_size,
    );
    info!("fields: {} on the line", farmland.len());
    let waters = content::water::Waters::from_line(
        &line_source,
        terrain_options.zone,
        terrain_options.tile_size,
    );
    info!("water: {} on the line", waters.len());
    // The line's roads, their carriageways draped on the terrain at build
    // time (see `content::roads`).
    let roads = content::roads::Roads::from_line(
        &line_source,
        terrain_options.zone,
        terrain_options.tile_size,
    );
    info!("roads: {} on the line", roads.len());
    // Trees, scenery objects and people come with the tiles: each stands on
    // the ground of the tile it lands on, and streams in and out with it.
    let terrain_builder = TerrainBuilder::new(&sim.net, sources, terrain_options)
        .with_vegetation(Vegetation::from_line(&line_source, terrain_options.zone))
        .with_scenery(Scenery::from_line(
            &line_source,
            &sim.net,
            terrain_options.zone,
        ))
        .with_crowd(crowd)
        .with_fields(farmland)
        .with_waters(waters)
        .with_roads(roads)
        .with_edits(TerrainEdits::from_line(&line_source, terrain_options.zone));

    render::spawn_track(
        &mut commands,
        &mut meshes,
        &mut materials,
        &mut rail_materials,
        &assets,
        &sim.net,
        &origin,
    );

    // Signal models (plan ch. 15.3): the placement's override, otherwise the signal
    // type's default; a signal without either gets the placeholder mast.
    let signal_models: Vec<Option<sim_core::interlock::SignalModel>> = line_source
        .signals
        .iter()
        .enumerate()
        .map(|(i, _)| {
            let name = mods.mods.signal_model_name(&line_source, i)?;
            let model = mods.mods.signal_models.get(name).cloned();
            if model.is_none() {
                warn!("signal {i}: unknown signal model {name:?}");
            }
            model
        })
        .collect();
    let views: Vec<world_render::SignalView> = sim
        .interlock
        .signals
        .iter()
        .enumerate()
        .map(|(i, s)| world_render::SignalView {
            device: s.device,
            kind: s.kind,
            aspect: s.aspect,
            model: signal_models.get(i).and_then(|m| m.as_ref()),
        })
        .collect();
    let aspect_materials = world_render::spawn_signals(
        &mut commands,
        &mut meshes,
        &mut materials,
        &assets,
        &sim.net,
        &views,
        &origin,
    );
    drop(views);
    commands.insert_resource(aspect_materials);
    commands.insert_resource(world_render::SignalModels(signal_models));

    // Vegetation, scenery and the crowd: the line's object names resolved
    // against the installed mods.
    let catalog = world_render::WorldCatalog::new(
        terrain_builder.tree_objects(),
        terrain_builder.scenery_objects(),
        &mods.mods.objects,
        passengers.clone(),
        &assets,
        &mut meshes,
        &mut materials,
        season,
    );
    let streamer = streaming::TerrainStreamer::new(
        terrain_builder,
        render::terrain_material(
            &mut images,
            &mut terrain_materials,
            season,
            settings::ground_quality(&graphics),
        ),
        catalog,
        f64::from(graphics.view_distance),
    );

    // Vehicles as simple bodies — the 3D cab comes in M6.
    let kit = VehicleKit {
        body: materials.add(StandardMaterial {
            base_color: Color::srgb(0.70, 0.12, 0.14),
            perceptual_roughness: 0.6,
            ..default()
        }),
        coach: materials.add(StandardMaterial {
            base_color: Color::srgb(0.80, 0.80, 0.84),
            perceptual_roughness: 0.6,
            ..default()
        }),
        // Zg 101: two red lamps on the current rear end (`update_headlights`).
        // ponytail: emissive spheres at the placeholder body's face — modelled
        // vehicles get real lenses once their glTF carries them as content.
        tail_material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.25, 0.02, 0.02),
            emissive: LinearRgba::rgb(6.0, 0.08, 0.08),
            ..default()
        }),
        tail_mesh: meshes.add(Sphere::new(0.09)),
        passengers,
    };
    for train in std::iter::once(player).chain(drivers.iter().map(|(t, _)| *t)) {
        spawn_vehicle_views(
            &mut commands,
            &assets,
            &mut meshes,
            &kit,
            &sim,
            train,
            player,
        );
    }
    commands.insert_resource(kit);
    // Atmosphere, sun, moon and stars — all of them off the scenario clock and
    // the georeferenced place (`feed_sky`).
    sky::spawn(
        &mut commands,
        &mut meshes,
        &mut media,
        &mut star_materials,
        &mut moon_materials,
        graphics.shadows,
    );
    let camera = commands
        .spawn((
            Camera3d::default(),
            // HDR: emissive lamp lenses glow at night (M6 night lighting) — the glow
            // itself is bloom, which the settings can switch off.
            bevy::camera::Hdr,
            // The sky lights the scene itself; what stays here is the floor a
            // moonless night needs to keep the ground off pure black (`feed_sky`).
            AmbientLight {
                color: Color::srgb(0.7, 0.8, 1.0),
                brightness: 20.0,
                ..default()
            },
            sky::camera_settings(),
            // Near-field extinction (`feed_sky`): the atmosphere's own haze term
            // carries the colour and the distance, but a planetary medium's LUTs
            // do not resolve 300 m of fog. This is what closes it.
            DistanceFog {
                falloff: FogFalloff::from_visibility(CLEAR_VISIBILITY),
                ..default()
            },
            Projection::Perspective(PerspectiveProjection {
                far: 20_000.0,
                ..default()
            }),
            Transform::default(),
            MeshPickingCamera,
            ui::CabCamera,
        ))
        .id();
    if graphics.bloom {
        commands.entity(camera).insert(Bloom::NATURAL);
    }
    // `apply_scene` only fires on a changed setting, and starting a run does not change
    // one — so the camera is dressed here as well as there.
    settings::apply_anti_aliasing(&mut commands.entity(camera), &graphics);

    // Rain and snow: a particle column of crossed quads that follows the camera
    // and scrolls downwards (`update_precipitation`). Both fields exist from the
    // start; the scenario's weather decides which one is visible.
    // Four fields: rain and snow, each with a far one that fills the view and a
    // near one of a few big out-of-focus drops. The near layer is what makes rain
    // read as rain rather than as a grey curtain.
    for (count, w, h, spread, seed, snow, near, speed) in [
        // A raindrop is millimetres wide; what the eye sees is its smear, thin
        // and faint. Many of them, not fat ones — the fat ones are the near
        // layer's job, and even those are centimetres, not fists.
        (22000, 0.009, 0.42, 20.0, 11, false, false, 9.0),
        (170, 0.045, 0.90, 3.5, 13, false, true, 9.0),
        (8000, 0.03, 0.03, 18.0, 12, true, false, 1.4),
        (110, 0.09, 0.09, 3.0, 14, true, true, 1.4),
    ] {
        commands.spawn((
            Precipitation { snow, speed, near },
            Mesh3d(meshes.add(precipitation_mesh(count, w, h, spread, seed))),
            MeshMaterial3d(
                precip_materials.add(world_render::precipitation::PrecipitationMaterial::default()),
            ),
            Transform::default(),
            Visibility::Hidden,
        ));
    }

    // `--overlays` opens the key sheet and the diagnostics from the start. A screenshot
    // cannot press F5, in the same way it cannot press a key on the menu — `--menu <page>`
    // is there for that reason and this is the same one.
    if std::env::args().any(|a| a == "--overlays") {
        commands.insert_resource(hud::Overlays {
            help: true,
            diagnostics: true,
        });
    }
    ui::spawn_crosshair(&mut commands);
    // The speedometer is drawn for one scale, so the face has to be made after the
    // vehicle is known — a dial whose figures changed with the line would be a bar chart
    // pretending to be an instrument.
    let v_max = {
        let train = &sim.trains[player];
        train
            .vehicles
            .get(train.cab)
            .map(|v| v.spec.v_max)
            .filter(|v| *v > 0.0)
            .unwrap_or(160.0)
    };
    let drawings = hud::Drawings::draw(&mut images, v_max);
    hud::spawn_hud(&mut commands, &fonts, &drawings, &binds);
    commands.insert_resource(drawings);
    mods_ui::spawn_panel(&mut commands);

    // A character model for the walker (plan ch. 12.4): `--character` names one of
    // the mods' people (`people:f01_lena`) or takes a file on the same `mods://` paths
    // as the vehicle models; without the flag the first character with the `Player`
    // role, in registry order. Without any the walker stays a body without a picture,
    // which in the first person is all he ever is. The model is a person like the
    // passengers (`world_render::people`); `walk::animate_walker` moves it.
    let character = match arg("--character") {
        Some(key) => Some(
            mods.mods
                .characters
                .get(&key)
                .map_or(key, |c| c.model.clone()),
        ),
        None => mods
            .mods
            .characters
            .values()
            .find(|c| c.has_role(content::Role::Player))
            .map(|c| c.model.clone()),
    };
    if let Some(file) = character {
        info!("walker: character {file}");
        let character = world_render::CharacterAssets::load(&assets, &file);
        commands.spawn((
            world_render::person_bundle(
                &character,
                content::Pose::Idle(0),
                0.0,
                world_render::PERSON_CULL,
            ),
            Transform::default(),
            Visibility::Hidden,
            walk::CharacterModel,
        ));
    }

    // `--camera outside` starts on the external camera — handy for screenshots of a
    // vehicle model — `--camera walk` on foot, which a screenshot cannot reach
    // otherwise (F4 needs a key press), and `--camera fly` in the free camera of the
    // console's `fly` command.
    match arg("--camera").as_deref() {
        Some("outside") => {
            commands.insert_resource(ui::CameraState {
                mode: ui::CameraMode::Outside,
                distance: 40.0,
                pitch: -0.15,
                ..default()
            });
        }
        Some("walk") => {
            commands.insert_resource(ui::CameraState {
                mode: ui::CameraMode::Walk,
                ..default()
            });
        }
        Some("fly") => {
            // Seeded where the wayside camera would put itself — beside the track,
            // looking at the train. Without it the free camera would wake at the raw
            // origin: rail-head height, inside the lead vehicle.
            let mut state = ui::CameraState {
                mode: ui::CameraMode::Fly,
                ..default()
            };
            if let Some(front) = sim.trains[player].vehicles.first() {
                let pose = front.pos.pose(&sim.net);
                let pos = origin.to_render(pose.pos);
                let up = origin.dir_to_render(pose.up);
                let forward = origin.dir_to_render(pose.tangent);
                let right = forward.cross(up).normalize_or_zero();
                state.fly = Some(pos + right * 25.0 + up * 6.0);
                let at = (pos + up * 2.0 - state.fly.unwrap()).normalize();
                state.pitch = at.y.asin();
                // The angles of the walk's view convention: forward is
                // (−sin yaw · cos pitch, sin pitch, −cos yaw · cos pitch).
                state.yaw = (-at.x).atan2(-at.z);
            }
            commands.insert_resource(state);
        }
        _ => {}
    }

    commands.insert_resource(TerrainInfo::default());
    commands.insert_resource(streamer);
    commands.insert_resource(ViewDistance(graphics.view_distance));
    commands.insert_resource(Origin(origin));
    commands.insert_resource(net::WorldId(fingerprint));
    commands.insert_resource(PlayerTrain(player));
    // The run begins with the player at the desk of the train it put them in.
    commands.insert_resource(crew::Duty(Some(player)));
    commands.insert_resource(AiDrivers(drivers));
    commands.insert_resource(SimResource(sim));
    // A timetable run keeps dispatching after the world is built (`dispatch_services`);
    // a scenario and a free run have nothing left to put on the line.
    commands.insert_resource(dispatch);
    match day {
        Some(run) => commands.insert_resource(run),
        None => commands.remove_resource::<services::DayRun>(),
    }
}

/// UTM zone of the DGM data from `--epsg`, default 32 (western Germany).
fn dgm_zone() -> u8 {
    std::env::args()
        .skip_while(|a| a != "--epsg")
        .nth(1)
        .and_then(|v| v.parse().ok())
        .and_then(world_coords::geo::utm_zone_from_epsg)
        .unwrap_or(32)
}

/// Hides terrain tiles outside the view distance.
///
/// Bevy already culls against the view frustum; this additionally limits the depth
/// so that distant tiles never enter the draw list in the first place.
fn terrain_visibility(
    view: Res<ViewDistance>,
    camera: Query<&GlobalTransform, With<ui::CabCamera>>,
    mut tiles: Query<(&TerrainChunk, &Transform, &mut Visibility)>,
) {
    let Ok(camera) = camera.single() else {
        return;
    };
    let eye = camera.translation();
    for (chunk, transform, mut visibility) in tiles.iter_mut() {
        let distance = eye.distance(transform.translation) - chunk.radius;
        let limit = view.0 * lod_range(chunk.lod);
        *visibility = if distance <= limit {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

/// Fine tiles are hidden earlier than coarse ones — they add nothing at a distance.
fn lod_range(lod: u8) -> f32 {
    match lod {
        0 => 0.25,
        1 => 0.5,
        2 => 0.75,
        _ => 1.0,
    }
}

/// Value of a command line option (`--name <value>`).
/// Splits `"21:40"` into its two numbers — the one shape both `--time` and the
/// date parsing need.
fn parse_pair(text: &str, separator: char) -> Option<(u32, u32)> {
    let (left, right) = text.split_once(separator)?;
    Some((left.trim().parse().ok()?, right.trim().parse().ok()?))
}

pub(crate) fn arg(name: &str) -> Option<String> {
    std::env::args().skip_while(|a| a != name).nth(1)
}

/// Every value of a repeatable command line option, in order.
fn args_all(name: &str) -> Vec<String> {
    let args: Vec<String> = std::env::args().collect();
    args.windows(2)
        .filter(|w| w[0] == name)
        .map(|w| w[1].clone())
        .collect()
}

/// Places a train of one loco + coaches at the given position.
///
/// Train protection and door control come from the vehicles themselves (`VehicleSpec`),
/// not from command line options.
pub(crate) fn spawn_train(
    sim: &mut Sim,
    head: TrackPosition,
    coaches: usize,
    loco: VehicleSpec,
) -> usize {
    let mut vehicles = vec![loco];
    vehicles.extend(std::iter::repeat_n(passenger_coach(), coaches));
    spawn_consist(sim, head, vehicles, true)
}

/// A train of exactly these vehicles, head first, standing at `head`.
///
/// What a scenario's or an operating day's `consists:` list comes to
/// (`sim_core::consist::ConsistSource`): the vehicles are named there one by one instead
/// of "a locomotive and n coaches", because a rake of vans behind a shunter is a train
/// too. `prepared` is battery on, pantograph up and main switch in — a cold engine the
/// driver has to wake up is a scenario of its own (M6).
pub(crate) fn spawn_consist(
    sim: &mut Sim,
    head: TrackPosition,
    vehicles: Vec<VehicleSpec>,
    prepared: bool,
) -> usize {
    let vehicles = vehicles
        .into_iter()
        .map(|spec| Vehicle::new(spec, head))
        .collect();
    let index = sim.add_train(Train::assemble(vehicles, head, &sim.net));
    if prepared {
        for v in &mut sim.trains[index].vehicles {
            if v.is_powered() {
                v.traction.battery = true;
                v.traction.pantograph_command = true;
                v.traction.main_switch_command = true;
                v.traction.pantograph = 1.0;
                v.traction.compressor = true;
            }
        }
    }
    index
}

/// The materials and the mesh every placeholder vehicle is drawn with.
///
/// A resource rather than four locals in `setup`, because a train is no longer only put
/// on the line before the first frame: an operating day dispatches its services as their
/// hour comes (`dispatch_services`), and what they are drawn with has to outlive the
/// frame the world was built in.
#[derive(Resource, Clone)]
pub(crate) struct VehicleKit {
    body: Handle<StandardMaterial>,
    coach: Handle<StandardMaterial>,
    tail_material: Handle<StandardMaterial>,
    tail_mesh: Handle<Mesh>,
    /// The people a vehicle's seats are filled from (plan ch. 12).
    passengers: world_render::Passengers,
}

/// Everything one train is drawn with: a body or its glTF per vehicle, the passengers
/// in the seats its model lists, the headlight cones and tail lamps at both ends, and
/// the cab lamp in the player's leading vehicle.
pub(crate) fn spawn_vehicle_views(
    commands: &mut Commands,
    assets: &AssetServer,
    meshes: &mut Assets<Mesh>,
    kit: &VehicleKit,
    sim: &Sim,
    train: usize,
    player: usize,
) {
    let (body, coach) = (&kit.body, &kit.coach);
    let (tail_material, tail_mesh) = (&kit.tail_material, &kit.tail_mesh);
    let last = sim.trains[train].vehicles.len().saturating_sub(1);
    for (i, v) in sim.trains[train].vehicles.iter().enumerate() {
        let view = VehicleView { train, vehicle: i };
        // A vehicle with a model gets its glTF; everything else stays a body
        // (plan ch. 15.3).
        // Through `model_file`, so a variant's own livery is the one that loads.
        let entity = if let Some(file) = v.spec.model_file(v.variant).filter(|f| !f.is_empty()) {
            let file = file.to_string();
            let entity = commands
                .spawn((Transform::default(), Visibility::default(), view))
                .id();
            models::spawn(commands, assets, entity, &view, &file);
            // The seats: about two thirds taken, decided by the indices alone, so
            // every client seats the same people (`world_render::people`).
            if let Some(seats) = v
                .spec
                .model
                .as_ref()
                .map(|m| m.seats.as_slice())
                .filter(|s| !s.is_empty())
            {
                commands.entity(entity).with_children(|parent| {
                    let taken =
                        world_render::spawn_seated(parent, &kit.passengers, seats, train, i);
                    info!(
                        "train {train}, vehicle {i}: {taken} of {} seats taken",
                        seats.len()
                    );
                });
            }
            entity
        } else {
            let mesh = meshes.add(
                Mesh::from(Cuboid::new(3.0, 3.8, v.spec.length as f32))
                    .translated_by(Vec3::Y * 2.2),
            );
            commands
                .spawn((
                    Mesh3d(mesh),
                    MeshMaterial3d(if v.is_powered() {
                        body.clone()
                    } else {
                        coach.clone()
                    }),
                    Transform::default(),
                    view,
                ))
                .id()
        };
        // Headlight cones and red tail lamps (Zg 101) at both ends of the
        // train; `update_headlights` lights the cones on the end facing the
        // direction of travel and the tail lamps on the other one.
        let mut cones = Vec::new();
        if i == 0 {
            cones.push((-(v.spec.length as f32) / 2.0, false));
        }
        if i == last {
            cones.push(((v.spec.length as f32) / 2.0, true));
        }
        for (end, reverse) in cones {
            let dir = if reverse { 1.0 } else { -1.0 };
            commands.entity(entity).with_children(|parent| {
                parent.spawn((
                    Headlight { train, reverse },
                    SpotLight {
                        color: Color::srgb(1.0, 0.95, 0.85),
                        intensity: 0.0,
                        range: 300.0,
                        inner_angle: 0.18,
                        outer_angle: 0.32,
                        ..default()
                    },
                    // Buffer height above the rail, at the end of the vehicle,
                    // aimed a touch onto the track.
                    Transform::from_xyz(0.0, 1.6, end)
                        .looking_to(Vec3::new(0.0, -0.06, dir).normalize(), Vec3::Y),
                ));
                for x in [-1.0, 1.0] {
                    parent.spawn((
                        TailLamp { train, reverse },
                        Mesh3d(tail_mesh.clone()),
                        MeshMaterial3d(tail_material.clone()),
                        Transform::from_xyz(x, 1.6, end),
                        Visibility::Hidden,
                    ));
                }
            });
        }
        // Cab light behind the front window of the player's leading vehicle.
        if train == player && i == 0 {
            commands.entity(entity).with_children(|parent| {
                parent.spawn((
                    CabLamp,
                    PointLight {
                        color: Color::srgb(1.0, 0.9, 0.75),
                        intensity: 0.0,
                        range: 4.0,
                        ..default()
                    },
                    Transform::from_xyz(0.0, 2.6, -(v.spec.length as f32) / 2.0 + 1.8),
                ));
            });
        }
    }
}

/// The operating day's dispatcher: puts its services on the line as their hour comes and
/// stables them again when it is over (plan ch. 11).
///
/// Every peer runs it. Which services are out is a pure function of the clock, so the
/// train list stays the same on all of them without a message about it; what stays the
/// server's is the *driving*, which is why a client is given the trains but not the
/// drivers (`services`, CLAUDE.md ch. 20).
// A Bevy system takes its resources as parameters — the argument count says nothing here.
#[allow(clippy::too_many_arguments)]
pub(crate) fn dispatch_services(
    mut commands: Commands,
    mut sim: ResMut<SimResource>,
    mut dispatch: ResMut<services::Dispatch>,
    mut drivers: ResMut<AiDrivers>,
    // The three that draw a train are optional: the dedicated server runs this system on
    // `MinimalPlugins`, where there is no asset plugin and nothing to draw with.
    mut meshes: Option<ResMut<Assets<Mesh>>>,
    mut fallback: Local<Option<sim_core::train::VehicleSpec>>,
    run: Option<Res<services::DayRun>>,
    kit: Option<Res<VehicleKit>>,
    assets: Option<Res<AssetServer>>,
    player: Res<PlayerTrain>,
    duty: Res<crew::Duty>,
    host: Option<Res<net::Host>>,
    mods: Res<Mods>,
    role: Option<Res<net::Role>>,
) {
    let Some(run) = run else { return };
    // A service that names no vehicle of its own gets the built-in one rather than
    // whatever the player picked in the menu: two clients would otherwise put different
    // trains on the line for the same working.
    let fallback = fallback.get_or_insert_with(content::vehicles::br101);
    // Nobody's train is stabled out from under them: the player's own, and on a server
    // every train a client has taken over.
    let mut driven: Vec<usize> = duty.0.into_iter().collect();
    if let Some(host) = host.as_deref() {
        driven.extend(host.driven());
    }
    let changes = services::dispatch(
        &mut sim.0,
        &run,
        &mut dispatch,
        &driven,
        &mods.0.mods.vehicles,
        fallback,
    );
    let client = role.is_some_and(|role| *role == net::Role::Client);
    // A working that is over has nobody in the cab any more. Its driver has to go with it,
    // or the AI would keep driving a unit that has just been put in a siding.
    for train in &changes.released {
        drivers.0.retain(|(driven, _)| driven != train);
    }
    let clock = sim.0.clock();
    for service in changes.started {
        // A unit that has been out before is already drawn; one that has just been built
        // is not. The dedicated server draws nothing at all.
        if service.fresh
            && let (Some(kit), Some(assets), Some(meshes)) =
                (kit.as_deref(), assets.as_deref(), meshes.as_deref_mut())
        {
            spawn_vehicle_views(
                &mut commands,
                assets,
                meshes,
                kit,
                &sim.0,
                service.train,
                player.0,
            );
        }
        info!(
            "{} on the line as train {}",
            run.day.services[service.service].number, service.train
        );
        if client {
            continue;
        }
        let ai = services::driver_for(&run.day.services[service.service], clock);
        match drivers
            .0
            .iter_mut()
            .find(|(train, _)| *train == service.train)
        {
            Some(slot) => slot.1 = ai,
            None => drivers.0.push((service.train, ai)),
        }
    }
}

pub(crate) fn drive_ai(
    mut sim: ResMut<SimResource>,
    mut drivers: ResMut<AiDrivers>,
    duty: Res<crew::Duty>,
    time: Res<Time>,
    host: Option<Res<net::Host>>,
    role: Option<Res<net::Role>>,
) {
    // On a client every train but the player's is driven from the server's setpoints — a
    // second AI running here would fight them.
    if role.is_some_and(|role| *role == net::Role::Client) {
        return;
    }
    let dt = time.delta_secs_f64().min(0.25);
    for (train, ai) in drivers.0.iter_mut() {
        // The train the player is in charge of drives itself, as does one a client has
        // taken over; one that is stabled between two workings has nobody in the cab at
        // all. This is the whole of the arbitration (`crate::crew`).
        if sim.0.trains.get(*train).is_none_or(|t| t.stabled)
            || duty.0 == Some(*train)
            || host
                .as_ref()
                .is_some_and(|host| host.is_player_driven(*train))
        {
            continue;
        }
        ai.drive(&mut sim.0, *train, dt);
    }
}

pub(crate) fn step_simulation(mut sim: ResMut<SimResource>, time: Res<Time>) {
    sim.0.advance(time.delta_secs_f64());
}

/// Hands the walkers their clock (`world_render::PeopleClock`): the
/// simulation's, not the frame's, so they stand still while the run is paused
/// and every client — the clock is what the server keeps in step — walks them
/// to the same spots. Nothing about them travels (CLAUDE.md, *Multiplayer*).
pub(crate) fn feed_people_clock(sim: Res<SimResource>, mut clock: ResMut<PeopleClock>) {
    let now = sim.0.clock();
    if clock.0 != now {
        clock.0 = now;
    }
}

/// Behaviour scripts of the mods — signal aspects and cab automation (plan ch. 19).
pub(crate) fn run_mod_scripts(
    mut sim: ResMut<SimResource>,
    mut mods: ResMut<Mods>,
    time: Res<Time>,
) {
    let dt = time.delta_secs_f64().min(0.25);
    mods.0.post_step(&mut sim.0, dt);
}

/// Follow up the origin and re-place all world-anchored objects.
fn rebase_origin(
    sim: Res<SimResource>,
    player: Res<PlayerTrain>,
    mut origin: ResMut<Origin>,
    mut anchored: Query<(&WorldAnchored, &mut Transform)>,
) {
    // An empty consist has no head to follow; the origin stays where it is.
    let Some(head) = sim.0.trains[player.0]
        .vehicles
        .first()
        .map(|v| v.pos.pose(&sim.0.net).pos)
    else {
        return;
    };
    if origin.0.rebase_if_needed(head) {
        render::resync_anchored(&origin.0, &mut anchored);
    }
}

/// Mirror vehicle poses from the simulation into transforms.
///
/// The view sits on the rail head, which is where a vehicle model's own origin is:
/// the BR 101's wheels touch y = 0 and its cab eye is 2.55 m above it. Everything
/// hung on a vehicle — lamps, the eye point (`ui::follow`), the walker's frame
/// (`walk::frame`) — measures from there.
fn sync_vehicles(
    sim: Res<SimResource>,
    origin: Res<Origin>,
    mut query: Query<(&VehicleView, &mut Transform, &mut Visibility)>,
) {
    for (view, mut transform, mut visibility) in query.iter_mut() {
        let Some(train) = sim.0.trains.get(view.train) else {
            continue;
        };
        // A stabled train is out of service and off the line — nothing of it is drawn,
        // its lamps and its cab light included, which the hierarchy takes care of.
        let wanted = if train.stabled {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
        let Some(vehicle) = train.vehicles.get(view.vehicle) else {
            continue;
        };
        let pose = vehicle.pos.pose(&sim.0.net);
        transform.translation = origin.0.to_render(pose.pos);
        transform.rotation = origin.0.look_rotation(pose.tangent, pose.up);
    }
}

/// Feeds the sky (`world_render::sky`) from the run: the scenario's clock, the
/// place the render origin sits at, and the weather. Sun, moon, stars and the
/// scattering that makes the sky blue all follow from those three — the whole
/// day/night cycle of plan ch. 14 is this system plus that module.
///
/// What stays here is the two things the sky does not own: the ambient floor a
/// moonless night needs, and the distance fog the weather pulls in (M6).
fn feed_sky(
    sim: Res<SimResource>,
    origin: Res<Origin>,
    daylight: Res<Daylight>,
    mut sky: ResMut<sky::Sky>,
    mut ambient: Query<&mut AmbientLight, With<ui::CabCamera>>,
    mut fog: Query<&mut DistanceFog, With<ui::CabCamera>>,
) {
    let (latitude, longitude, _) = world_coords::geo::from_ecef(origin.0.position());
    let start = sim.0.start;
    *sky = sky::Sky {
        year: start.year,
        month: start.month,
        day: start.day,
        seconds: start.seconds() + sim.0.time,
        utc_offset: start.utc_offset,
        latitude,
        longitude,
        weather: sim.0.weather.now,
        wetness: sim.0.weather.wetness,
        snow: sim.0.weather.snow,
        cloud_shadow: sim.0.weather.now.cover,
        // A strike lights the sky for a third of a second (plan 14.1). The
        // thunder that follows it is the sound table's business (`audio.rs`).
        flash: sim
            .0
            .weather
            .lightning(sim.0.time)
            .map_or(0.0, |strike| strike.brightness(sim.0.time)),
    };

    // Fog and heavy snow: the scattering medium reddens and brightens them
    // correctly at every hour, but its look-up tables are cut for a planet, not
    // for the first three hundred metres. Below `CLEAR_VISIBILITY` an analytic
    // falloff closes the near field the way Koschmieder says it should; above it
    // the atmosphere has the whole job.
    // ponytail: two models for one haze, matched by eye at the seam. The honest
    // fix is a fog volume around the camera, and that is what `mist` is for once
    // it can carry the whole sight rather than only the layer.
    let visibility = sim.0.weather.now.visibility;
    for mut fog in &mut fog {
        fog.falloff = FogFalloff::from_visibility(visibility.min(CLEAR_VISIBILITY));
        // The colour of the air itself: what the sky is at the horizon.
        let lit = 0.15 + 0.85 * daylight.0;
        fog.color = Color::srgb(0.66 * lit, 0.69 * lit, 0.74 * lit);
    }

    let night = 1.0 - daylight.0;
    for mut ambient in &mut ambient {
        // The sky's own image-based light carries the day; this is the floor
        // underneath it, so a night without a moon is dark and not blind. A
        // lightning flash comes on top of it, and at night it is the only light
        // there is.
        ambient.brightness = 8.0 + 24.0 * night + 4_000.0 * sky.flash;
    }
}

/// Headlights follow the light switch, the direction of travel and the darkness:
/// full beam at night on the end the train runs towards, off in daylight (M6).
/// The red tail lamps (Zg 101) mark the opposite end, by day as by night.
/// The cab lamp follows its own switch alone.
fn update_headlights(
    daylight: Res<Daylight>,
    sim: Res<SimResource>,
    mut heads: Query<(&Headlight, &mut SpotLight)>,
    mut tails: Query<(&TailLamp, &mut Visibility)>,
    mut cab_lamp: Query<&mut PointLight, With<CabLamp>>,
    player: Res<PlayerTrain>,
) {
    let night = 1.0 - daylight.0;
    for (head, mut light) in heads.iter_mut() {
        let cab = &sim.0.controls[head.train];
        let backwards = cab.reverser < 0;
        let on = cab.headlights && head.reverse == backwards;
        // ponytail: like the moon above, lit artistically bright — the night
        // scene has no auto-exposure to lift a physical beam out of the black.
        light.intensity = if on { 2_000_000_000.0 * night } else { 0.0 };
    }
    for (tail, mut vis) in &mut tails {
        let cab = &sim.0.controls[tail.train];
        let backwards = cab.reverser < 0;
        *vis = if cab.headlights && tail.reverse != backwards {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    let on = sim.0.controls[player.0].cab_light;
    for mut lamp in &mut cab_lamp {
        lamp.intensity = if on { 60_000.0 } else { 0.0 };
    }
}

/// Particle field for rain or snow: `count` crossed quad pairs of `w` × `h` metres
/// in a disc of radius `spread` around the camera, repeated three times in y
/// with period
/// [`PRECIP_PERIOD`] so the fall offset can wrap seamlessly
/// (`update_precipitation`). Each particle carries the random number the shader
/// thins the field by.
fn precipitation_mesh(count: usize, w: f32, h: f32, spread: f32, seed: u64) -> Mesh {
    let mut rng = sim_core::rng::Rng::new(seed);
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();
    for _ in 0..count {
        // A full disc — no hole. The old hole was a tube along the fall axis, and
        // the moment the wind slanted the column that empty tube pointed straight
        // down the view: the gap in the rain at speed. The nearest arm's length
        // is faded in the shader instead, where "near" can be a sphere.
        let angle = rng.range(0.0, std::f64::consts::TAU) as f32;
        let radius = spread * (rng.range(0.0, 1.0) as f32).sqrt();
        let x = radius * angle.cos();
        let z = radius * angle.sin();
        let y = rng.range(0.0, f64::from(PRECIP_PERIOD)) as f32;
        // Three numbers of the drop's own: whether it falls at this intensity
        // (the shader discards above it, so one mesh serves drizzle and
        // downpour), how brightly it catches the light, and how long it draws —
        // a field of identical streaks reads as a pattern, not as rain.
        let alive = rng.range(0.0, 1.0) as f32;
        let glint = rng.range(0.0, 1.0) as f32;
        let length = rng.range(0.0, 1.0) as f32;
        for k in 0..3 {
            let y = y + k as f32 * PRECIP_PERIOD;
            // One quad per particle, turned by its own angle round the column's
            // axis — the drops are scattered over every heading anyway, so a
            // second crossed quad would only draw the same streak twice.
            let (sx, sz) = (angle.sin(), angle.cos());
            let (dx, dz) = (sx * w / 2.0, sz * w / 2.0);
            let base = positions.len() as u32;
            positions.extend([
                [x - dx, y, z - dz],
                [x + dx, y, z + dz],
                [x + dx, y + h, z + dz],
                [x - dx, y + h, z - dz],
            ]);
            normals.extend([[sz, 0.0, -sx]; 4]);
            uvs.extend([[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]);
            colors.extend([[alive, glint, length, 1.0]; 4]);
            indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, default());
    mesh.try_insert_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .expect("positions fit");
    mesh.try_insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .expect("normals fit");
    mesh.try_insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .expect("uvs fit");
    mesh.try_insert_attribute(Mesh::ATTRIBUTE_COLOR, colors)
        .expect("colors fit");
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Downward scroll of the precipitation field, wrapped to one mesh period.
fn fall_offset(time: f64, speed: f32) -> f32 {
    ((time * f64::from(speed)) % f64::from(PRECIP_PERIOD)) as f32
}

/// Streak slant cap: relative wind beyond this multiple of the fall speed tilts
/// the column no further (~68°) — laid flat, it would stop covering the camera.
const MAX_SLANT: f32 = 2.5;

/// Tilts the fall axis into the wind the drops meet — the weather's own plus the
/// train's rush of air — by `atan(wind / fall speed)`, capped at [`MAX_SLANT`].
fn fall_rotation(wind: Vec3, fall_speed: f32) -> Quat {
    let wind = wind.clamp_length_max(fall_speed * MAX_SLANT);
    Quat::from_rotation_arc(Vec3::NEG_Y, (wind + Vec3::NEG_Y * fall_speed).normalize())
}

/// Keeps the precipitation column on the camera, lets it fall along an axis
/// slanted by the relative wind, and shows the field the current weather asks
/// for. The wind is the player train's speed — the outside cameras ride along,
/// so the same slant is right for them.
#[allow(clippy::too_many_arguments)]
fn update_precipitation(
    sim: Res<SimResource>,
    player: Res<PlayerTrain>,
    origin: Res<Origin>,
    daylight: Res<Daylight>,
    view: Res<ui::CameraState>,
    camera: Query<&Transform, With<ui::CabCamera>>,
    sun: Query<&Transform, (With<sky::Sun>, Without<Precipitation>)>,
    mut materials: ResMut<Assets<world_render::precipitation::PrecipitationMaterial>>,
    mut fields: Query<
        (
            &Precipitation,
            &MeshMaterial3d<world_render::precipitation::PrecipitationMaterial>,
            &mut Transform,
            &mut Visibility,
        ),
        Without<ui::CabCamera>,
    >,
) {
    let Ok(cam) = camera.single() else {
        return;
    };
    let Some(vehicle) = sim.0.trains[player.0].vehicles.first() else {
        return;
    };
    let vel = origin.0.dir_to_render(vehicle.pos.pose(&sim.0.net).tangent) * vehicle.v as f32;
    let weather = sim.0.weather.now;
    // Nothing falls on a train in a tunnel: the track type already says where one
    // is, because the sound (`audio.rs`) needed the same answer.
    let sheltered = sim
        .0
        .net
        .track_type_at(vehicle.pos.edge, vehicle.pos.s)
        .reverb
        .clamp(0.0, 1.0) as f32;
    // The wind the drops actually meet: the weather's own — swaying in strength
    // and direction with the gusts, because a curtain of rain never stands at one
    // frozen angle — plus the train's own rush of air the other way.
    let t = sim.0.time;
    let gusting = 1.0 + weather.gust * (0.45 * (t * 0.9).sin() + 0.25 * (t * 2.3).sin()) as f32;
    let veer = weather.gust * 0.35 * ((t * 0.31).sin() as f32);
    let (sin, cos) = (weather.bearing + veer).sin_cos();
    let wind = Vec3::new(-sin, 0.0, cos) * (weather.wind * gusting) - vel;
    // Towards the sun: the drops forward-scatter its light, so the curtain glows
    // looking into it and nearly vanishes looking away.
    let sun_dir = sun
        .iter()
        .next()
        .map_or(Vec3::Y, |t| -t.forward().as_vec3());
    // Seen from inside, the near layer stands in the cab rather than in front of
    // it: its drops are closer than the windscreen. The far field keeps its hole
    // around the camera and stays where it belongs, outside the glass.
    let inside = view.mode.inside();
    for (field, material, mut tf, mut visibility) in &mut fields {
        let mut params =
            world_render::precipitation::params(weather, daylight.0, field.snow, field.near);
        params.state.x *= 1.0 - sheltered;
        if inside && field.near {
            params.state.x = 0.0;
        }
        // Terminal velocity does not grow — what grows is the smear across the
        // eye. The apparent speed is fall plus wind, and the mesh is stretched
        // along its fall axis by that ratio: the scroll speeds up with it, the
        // streaks lengthen with it, and the same light over a longer line is a
        // dimmer line.
        let capped = wind.clamp_length_max(field.speed * MAX_SLANT);
        let stretch = (capped + Vec3::NEG_Y * field.speed).length() / field.speed;
        params.state.z /= stretch.sqrt();
        // Where the shader starts fading the nearest drops out: an arm's length,
        // or from the seat far enough that nothing draws inside the cab.
        params.light.w = if inside { 2.6 } else { 1.6 };
        params.sun = sun_dir.extend(daylight.0);
        if let Some(mut material) = materials.get_mut(&material.0) {
            material.params = params;
        }
        *visibility = if params.state.x > 0.0 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        let rot = fall_rotation(wind, field.speed);
        tf.rotation = rot;
        tf.scale = Vec3::new(1.0, stretch, 1.0);
        tf.translation = cam.translation
            + rot
                * Vec3::new(
                    0.0,
                    (-PRECIP_PERIOD - fall_offset(sim.0.time, field.speed)) * stretch,
                    0.0,
                );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The dedicated server has no asset plugin and nothing to draw with, and still has to
    /// keep putting the day's services on the line — `dispatch_services` is the one system
    /// that runs on both sides, so its rendering half has to be optional.
    #[test]
    fn services_are_dispatched_without_anything_to_draw_with() {
        let mut mods = ModRuntime::load("../../mods");
        let day = content::musterbahn_day();
        // The 05:12 out of Musterbach; the next working leaves at 05:42.
        let index = day
            .services
            .iter()
            .position(|service| service.departure() == 5.0 * 3_600.0 + 720.0)
            .expect("the plan has an 05:12");
        let built = world::build(
            &mut mods,
            &menu::Selection {
                service: Some(world::ServiceRef {
                    day: world::BUILTIN_DAY.into(),
                    index,
                }),
                ..default()
            },
        );
        let run = built.day.expect("a timetable run carries its plan");
        let player = built.player;

        let mut app = App::new();
        // `MinimalPlugins` is exactly what `net::run_dedicated` builds on: no assets, no
        // renderer, no window.
        app.add_plugins(MinimalPlugins)
            .insert_resource(PlayerTrain(player))
            .insert_resource(crew::Duty(Some(player)))
            .insert_resource(AiDrivers(built.drivers))
            .insert_resource(built.dispatch)
            .insert_resource(Mods(mods))
            .insert_resource(net::Role::Server)
            .insert_resource(run)
            .insert_resource(SimResource(built.sim))
            .add_systems(Update, dispatch_services);

        let before = app.world().resource::<SimResource>().0.trains.len();
        assert_eq!(before, 1, "the plan runs one train at a time");
        // Half an hour on, the next working is due.
        app.world_mut().resource_mut::<SimResource>().0.time = 30.0 * 60.0;
        app.update();

        let trains = app.world().resource::<SimResource>().0.trains.len();
        assert_eq!(trains, before + 1, "no service was put on the line");
        // … and the server drives it, because the server owns every AI.
        let drivers = &app.world().resource::<AiDrivers>().0;
        assert_eq!(drivers.len(), 1);
        assert_ne!(drivers[0].0, player, "not the player's train");
    }

    /// Leaving a run for the title screen has to take the world with it and nothing else
    /// — the window, the picking pointers and the cloud dome are all older than the run
    /// and all still needed by the next one.
    #[test]
    fn going_back_to_the_menu_drops_the_run_and_only_the_run() {
        let mut world = World::new();
        world.init_resource::<walk::Walker>();
        world.init_resource::<ui::CameraState>();
        let mut snapshot = Schedule::default();
        snapshot.add_systems(remember_before_run);
        let mut leave = Schedule::default();
        leave.add_systems(tear_down_run);

        // Before the run: something the program put up at startup, with a child of its
        // own, and the cloud dome.
        let older = world.spawn_empty().id();
        world.spawn(ChildOf(older));
        let dome = world.spawn(world_render::Persistent).id();
        snapshot.run(&mut world);

        // The run: a root with a child, and one more persistent entity made after the
        // snapshot — the marker is what saves it, not the moment it was made.
        let train = world.spawn_empty().id();
        world.spawn(ChildOf(train));
        let late_dome = world.spawn(world_render::Persistent).id();
        // A resource inserted by the run is an entity like any other in Bevy 0.19, and
        // despawning it would take the resource with it.
        world.insert_resource(ViewDistance(4_000.0));

        leave.run(&mut world);
        assert!(
            world.get_resource::<ViewDistance>().is_some(),
            "lost a resource"
        );
        assert!(world.get_entity(older).is_ok(), "dropped the older entity");
        assert!(world.get_entity(dome).is_ok(), "dropped the cloud dome");
        assert!(
            world.get_entity(late_dome).is_ok(),
            "dropped a persistent one"
        );
        assert!(
            world.get_entity(train).is_err(),
            "the run is still standing"
        );
        let left = world.iter_entities().count();

        // A second visit to the title screen, with no run behind it, does nothing.
        leave.run(&mut world);
        assert_eq!(
            world.iter_entities().count(),
            left,
            "a second visit took more"
        );
    }

    /// Resuming out of the pause overlay enters `Driving` a second time. The chain that
    /// builds the run must not come with it — a second `setup` would put a second world,
    /// a second camera and a second simulation on top of the one being driven. Leaving
    /// for the title screen tears the world down, so the next drive builds again.
    #[test]
    fn the_run_is_built_once_and_not_again_on_resuming() {
        #[derive(Resource, Default)]
        struct Builds(usize);

        /// Everything a run put into the world, in this test one entity.
        #[derive(Component)]
        struct OfTheRun;

        // Stands in for `setup`, which needs a GPU.
        fn build(mut commands: Commands, mut builds: ResMut<Builds>) {
            builds.0 += 1;
            commands.spawn(OfTheRun);
        }

        // How much of a run stands in the world right now.
        fn standing(app: &mut App) -> usize {
            let mut of_the_run = app.world_mut().query::<&OfTheRun>();
            of_the_run.iter(app.world()).count()
        }

        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
            .init_resource::<Builds>()
            .init_resource::<walk::Walker>()
            .init_resource::<ui::CameraState>()
            .insert_state(GameState::Menu)
            .add_systems(OnEnter(GameState::Menu), tear_down_run)
            .add_systems(
                OnEnter(GameState::Driving),
                (remember_before_run, build, mark_run_built)
                    .chain()
                    .run_if(not(resource_exists::<RunBuilt>)),
            );
        app.update();

        let go = |app: &mut App, state| {
            app.world_mut().insert_resource(NextState::Pending(state));
            app.update();
        };
        go(&mut app, GameState::Driving);
        assert_eq!(app.world().resource::<Builds>().0, 1);
        assert_eq!(standing(&mut app), 1);

        // Esc and back again: the same run, not a new one.
        go(&mut app, GameState::Paused);
        go(&mut app, GameState::Driving);
        assert_eq!(
            app.world().resource::<Builds>().0,
            1,
            "resuming built the world a second time"
        );

        // The title screen still takes the run with it — the pause must not have moved
        // the snapshot the teardown works from onto the built world.
        go(&mut app, GameState::Menu);
        assert_eq!(standing(&mut app), 0, "the run outlived the title screen");
        assert!(
            app.world().get_resource::<RunBuilt>().is_none(),
            "the torn-down run still counts as built"
        );

        // And the drive after it builds one again.
        go(&mut app, GameState::Driving);
        assert_eq!(
            app.world().resource::<Builds>().0,
            2,
            "the next drive stayed on the title screen's empty world"
        );
    }

    #[test]
    fn fall_offset_wraps_within_one_period() {
        for t in [0.0, 1.7, 100.0, 86_400.0] {
            let o = fall_offset(t, 9.0);
            assert!((0.0..PRECIP_PERIOD).contains(&o), "offset {o} at t={t}");
        }
    }

    #[test]
    fn fall_rotation_leans_into_the_wind_and_caps() {
        // In still air the streaks stay vertical.
        let dir = fall_rotation(Vec3::ZERO, 9.0) * Vec3::NEG_Y;
        assert!(dir.angle_between(Vec3::NEG_Y) < 1e-4);
        // 20 m/s of air moving towards +z (a train running towards −z sees this):
        // the streaks lean that way by atan(20/9).
        let dir = fall_rotation(Vec3::new(0.0, 0.0, 20.0), 9.0) * Vec3::NEG_Y;
        assert!((dir.z / -dir.y - 20.0 / 9.0).abs() < 1e-3, "slant {dir}");
        // Far above the cap the slant ratio stays at MAX_SLANT.
        let dir = fall_rotation(Vec3::new(0.0, 0.0, 100.0), 9.0) * Vec3::NEG_Y;
        assert!(
            (dir.z / -dir.y - MAX_SLANT).abs() < 1e-3,
            "capped slant {dir}"
        );
    }
}
