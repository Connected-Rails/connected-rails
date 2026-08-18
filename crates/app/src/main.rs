//! Connected Rails — Bevy app: rendering, camera, input, HUD (plan ch. 12).
//!
//! The app ticks `sim-core` with a fixed time step and mirrors the state into ECS components.
//! Simulation logic does **not** belong here.

mod audio;
mod cab;
mod displays;
mod menu;
mod models;
mod mods_ui;
mod render;
mod settings;
mod signals;
mod streaming;
mod ui;
mod walk;

use ai_driver::{AiDriver, ScheduledStop, Timetable, TimetableKind};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::picking::mesh_picking::{MeshPickingCamera, MeshPickingPlugin, MeshPickingSettings};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use content::import::dgm::TerrainSource;
use content::terrain::{TerrainBuilder, TerrainEdits, TerrainOptions, TerrainStats, Vegetation};
use content::vehicles::{br101, passenger_coach};
use content::{musterbahn, re_4711, to_musterstadt};
use mod_runtime::ModRuntime;
use render::{Origin, TerrainChunk, VehicleView, WorldAnchored};
use sim_core::Sim;
use sim_core::train::{Train, Vehicle, VehicleSpec, Weather};
use track_model::{EdgeId, TrackPosition};
use world_coords::{RenderOrigin, sun};
// Daylight factor of this frame, 0 (night) … 1 (full day) — written by
// `update_daylight`, read by everything that switches with darkness: the
// headlights here, the mods' `_NIGHT` nodes in `world-render`.
use bevy::gltf::GltfAssetLabel;
use world_render::Daylight;

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
}

/// Height of one repetition of the precipitation mesh [m]. The mesh repeats its
/// particles three times in y, so wrapping the fall offset keeps the camera
/// covered by at least ±one period.
const PRECIP_PERIOD: f32 = 24.0;

/// Sight distance that stands in for "clear" [m] — far beyond the camera's far
/// plane, so the fog is invisible without a weather that pulls it in.
const CLEAR_VISIBILITY: f32 = 100_000.0;

/// The one directional light that is the sun (`update_daylight`).
#[derive(Component)]
struct Sun;

/// Second, dim directional light for moonlit nights.
#[derive(Component)]
struct Moon;

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
    app.add_plugins(settings::plugin);
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
    // Terrain splatting (plan ch. 14): shader and material, shared with the
    // route editor, which draws the same ground.
    .add_plugins(world_render::WorldRenderPlugin)
    // The mixer (`audio.rs`) — opened here rather than in `Startup`, because the initial
    // state transition into `Driving` runs before any startup schedule.
    .add_plugins(audio::plugin)
    .insert_resource(ClearColor(Color::srgb(0.55, 0.68, 0.82)))
    // Mouse picking for the 3D cab: only marked control meshes catch the ray —
    // without the marker requirement every terrain tile would compete for it.
    .add_plugins(MeshPickingPlugin)
    .insert_resource(MeshPickingSettings {
        require_markers: true,
        ..default()
    })
    .init_resource::<ui::CameraState>()
    .init_resource::<walk::Walker>()
    .init_resource::<cab::CabMouse>()
    .init_resource::<mods_ui::ModManager>()
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
    .add_systems(Startup, log_mods)
    .add_systems(OnEnter(GameState::Menu), menu::spawn_menu)
    // The same menu, as an overlay over the standing world.
    .add_systems(OnEnter(GameState::Paused), menu::spawn_pause)
    .add_systems(
        Update,
        menu::menu.run_if(in_state(GameState::Menu).or_else(in_state(GameState::Paused))),
    )
    .add_systems(Update, pause_on_escape.run_if(in_state(GameState::Driving)))
    // Runs in every state: the pause menu needs its cursor back.
    .add_systems(Update, ui::grab_cursor)
    // The sound table and the display cameras need the trains, which `setup` only
    // creates when its commands are applied — the chain inserts that sync point.
    .add_systems(
        OnEnter(GameState::Driving),
        (setup, audio::setup_audio, displays::setup_displays).chain(),
    )
    .add_systems(
        Update,
        (
            ui::player_input,
            cab::apply_mouse,
            drive_ai,
            step_simulation,
            run_mod_scripts,
            displays::update_displays,
            rebase_origin,
            sync_vehicles,
            update_daylight,
            update_headlights,
            walk::walk_player,
            ui::camera_control,
            walk::place_character,
            update_precipitation,
            streaming::stream_terrain,
            terrain_visibility,
            ui::update_hud,
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
        menu::Fonts {
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
    app.insert_resource(menu::Wallpaper(wallpaper));
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
fn exit_after_frames(
    limit: Res<FrameLimit>,
    shot: Option<Res<ShotPath>>,
    mut commands: Commands,
    mut count: Local<u32>,
    mut exit: MessageWriter<AppExit>,
) {
    *count += 1;
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

/// Esc during a run raises the pause overlay, which also holds the settings. Leaving it
/// again is the overlay's own job — this system only runs while `Driving`, so the Esc that
/// resumes cannot bounce straight back into the pause.
fn pause_on_escape(keys: Res<ButtonInput<KeyCode>>, mut next: ResMut<NextState<GameState>>) {
    if keys.just_pressed(KeyCode::Escape) {
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
        "Mods: {} of {} enabled ({} vehicles, {} lines, {} compositions, {} scenarios, {} timetables, {} signal types, {} scripts)",
        mods.mods.manifests.iter().filter(|m| m.enabled).count(),
        mods.mods.manifests.len(),
        mods.mods.vehicles.len(),
        mods.mods.lines.len(),
        mods.mods.compositions.len(),
        mods.mods.scenarios.len(),
        mods.mods.timetables.len(),
        mods.mods.signal_types.len(),
        mods.mods.scripts.len()
    );
}

#[allow(clippy::too_many_arguments)]
fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut terrain_materials: ResMut<Assets<render::TerrainMaterial>>,
    assets: Res<AssetServer>,
    mut mods: ResMut<Mods>,
    mut manager: ResMut<mods_ui::ModManager>,
    selection: Res<menu::Selection>,
    graphics: Res<settings::Graphics>,
) {
    // A mod was toggled on the menu: reload, so the world is built from the set on disk.
    if manager.restart_needed {
        mods.0 = ModRuntime::load("mods");
        for warning in mods.0.log() {
            warn!("mod: {warning}");
        }
        manager.restart_needed = false;
    }
    let mods = &mut mods.0;

    // Build line and simulation. Selection comes from the menu or from CLI flags.
    // CLI flags take precedence (command line usage stays non-interactive).
    let scenario_id = arg("--scenario").or_else(|| selection.scenario_id.clone());
    let line_ref = arg("--line")
        .or_else(|| selection.line_ref.clone())
        .or_else(|| {
            scenario_id
                .as_ref()
                .and_then(|id| mods.mods.scenarios.get(id))
                .and_then(|s| s.line.clone())
        });
    let resolved = line_ref.and_then(|id| match mods.mods.resolve_line(&id) {
        Ok(composed) => {
            for note in &composed.notes {
                info!("{id}: {note}");
            }
            Some(composed)
        }
        Err(e) => {
            warn!("line {id}: {e} — using the example line");
            None
        }
    });
    let modded = resolved.is_some();
    let module_offsets = resolved
        .as_ref()
        .map(|c| c.offsets.clone())
        .unwrap_or_default();
    let line_source = resolved.map(|c| c.line).unwrap_or_else(musterbahn);
    let mut line = line_source.compile().expect("line compiles");
    for warning in mods
        .mods
        .apply_signal_types(&line_source, &mut line.interlock)
    {
        warn!("{}: {warning}", line_source.name);
    }
    // Track types: specs behind the names, and the superstructure speed cap
    // merged into the one profile AI, LZB, HUD and scoring read.
    for warning in mods.mods.apply_track_types(&mut line.net) {
        warn!("{}: {warning}", line_source.name);
    }
    let mut sim = Sim::new(line.net, line.interlock, 2024);

    // Vehicle from menu selection or CLI flag.
    let loco = arg("--loco")
        .or_else(|| selection.loco_id.clone())
        .and_then(|id| match mods.mods.vehicles.get(&id) {
            Some(spec) => Some(spec.clone()),
            None => {
                warn!("vehicle {id} not found — using the BR 101");
                None
            }
        })
        .unwrap_or_else(br101);

    let player = spawn_train(&mut sim, TrackPosition::new(EdgeId(0), 200.0, 1), 5, loco);

    // Second train, timetable and scenario belong to the example line — a modded line
    // brings its own scenario or none at all.
    let mut drivers = Vec::new();
    if !modded {
        let ai_train = spawn_train(
            &mut sim,
            TrackPosition::new(EdgeId(1), 400.0, 1),
            3,
            br101(),
        );
        drivers.push((
            ai_train,
            AiDriver::new(Timetable {
                number: "RB 20".into(),
                category: "RB".into(),
                kind: TimetableKind::Scenario,
                module: None,
                stops: vec![ScheduledStop {
                    name: "Musterstadt".into(),
                    edge: EdgeId(2),
                    s: 2600.0,
                    arrival: 300.0,
                    departure: 360.0,
                    platform: "1".into(),
                    module: None,
                }],
            }),
        ));

        // Load the scenario with timetable and scoring (plan 11.4).
        let mut scenario = to_musterstadt();
        scenario.player_train = player;
        sim.set_scenario(scenario, re_4711());
    }

    // `--scenario <mod>:<name>` runs a scenario out of a mod. A `timetable/*.ron` the
    // scenario references adds stop scoring; without one only the scenario points count.
    if let Some(id) = scenario_id {
        match mods.mods.scenarios.get(&id) {
            Some(scenario) => {
                let mut scenario = scenario.clone();
                scenario.player_train = player;
                for warning in mod_runtime::qualify_scenario(&mut scenario, &module_offsets) {
                    warn!("scenario {id}: {warning}");
                }
                let timetable = scenario
                    .timetable
                    .as_deref()
                    .and_then(|name| {
                        let timetable = mods.mods.timetables.get(name).cloned();
                        if timetable.is_none() {
                            warn!("scenario {id}: timetable {name:?} not found");
                        }
                        timetable
                    })
                    .map(|mut timetable| {
                        for warning in
                            mod_runtime::qualify_timetable(&mut timetable, &module_offsets)
                        {
                            warn!("scenario {id}: {warning}");
                        }
                        timetable
                    })
                    .unwrap_or_else(|| sim_core::timetable::Timetable {
                        number: scenario.name.clone(),
                        ..default()
                    });
                sim.set_scenario(scenario, timetable);
            }
            None => warn!("scenario {id} not found"),
        }
    }

    // Line and scenario hooks: `on_load` now, `on_frame` every frame (plan 19.7).
    mods.begin(&mut sim, &line_source);

    // Render origin at the head of the train.
    let start = sim.trains[player].vehicles[0].pos.pose(&sim.net).pos;
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
    let mut terrain_builder = TerrainBuilder::new(&sim.net, sources, terrain_options)
        .with_vegetation(Vegetation::from_line(&line_source, terrain_options.zone))
        .with_edits(TerrainEdits::from_line(&line_source, terrain_options.zone));

    render::spawn_track(
        &mut commands,
        &mut meshes,
        &mut materials,
        &assets,
        &sim.net,
        &origin,
    );
    // Scenery objects: the line's furniture, placed relative to the track.
    world_render::spawn_objects(
        &mut commands,
        &mut meshes,
        &mut materials,
        &assets,
        &line_source,
        &sim.net,
        &origin,
        &mods.mods.objects,
        Some(&mut terrain_builder),
        season,
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

    // Vegetation: the line's tree objects resolved against the installed mods.
    let tree_catalog = render::tree_catalog(
        terrain_builder.tree_objects(),
        &mods.mods.objects,
        &assets,
        &mut meshes,
        &mut materials,
        season,
    );
    let streamer = streaming::TerrainStreamer::new(
        terrain_builder,
        render::terrain_material(&mut images, &mut terrain_materials, season),
        tree_catalog,
        f64::from(graphics.view_distance),
    );

    // Vehicles as simple bodies — the 3D cab comes in M6.
    let body = materials.add(StandardMaterial {
        base_color: Color::srgb(0.70, 0.12, 0.14),
        perceptual_roughness: 0.6,
        ..default()
    });
    let coach = materials.add(StandardMaterial {
        base_color: Color::srgb(0.80, 0.80, 0.84),
        perceptual_roughness: 0.6,
        ..default()
    });
    // Zg 101: two red lamps on the current rear end (`update_headlights`).
    // ponytail: emissive spheres at the placeholder body's face — modelled
    // vehicles get real lenses once their glTF carries them as content.
    let tail_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.25, 0.02, 0.02),
        emissive: LinearRgba::rgb(6.0, 0.08, 0.08),
        ..default()
    });
    let tail_mesh = meshes.add(Sphere::new(0.09));
    for train in std::iter::once(player).chain(drivers.iter().map(|(t, _)| *t)) {
        let last = sim.trains[train].vehicles.len() - 1;
        for (i, v) in sim.trains[train].vehicles.iter().enumerate() {
            let view = VehicleView { train, vehicle: i };
            // A vehicle with a model gets its glTF; everything else stays a body
            // (plan ch. 15.3).
            let entity = if let Some(model) = v.spec.model.as_ref().filter(|m| !m.file.is_empty()) {
                let entity = commands
                    .spawn((Transform::default(), Visibility::default(), view))
                    .id();
                models::spawn(&mut commands, &assets, entity, &view, &model.file);
                entity
            } else {
                let mesh = meshes.add(Cuboid::new(3.0, 3.8, v.spec.length as f32));
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
                        // The vehicle origin sits 2.2 m above the rail; the lamp
                        // below it at the end, aimed a touch onto the track.
                        Transform::from_xyz(0.0, -0.6, end)
                            .looking_to(Vec3::new(0.0, -0.06, dir).normalize(), Vec3::Y),
                    ));
                    for x in [-1.0, 1.0] {
                        parent.spawn((
                            TailLamp { train, reverse },
                            Mesh3d(tail_mesh.clone()),
                            MeshMaterial3d(tail_material.clone()),
                            Transform::from_xyz(x, -0.6, end),
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
                        Transform::from_xyz(0.0, 0.4, -(v.spec.length as f32) / 2.0 + 1.8),
                    ));
                });
            }
        }
    }

    // Sun, moon and sky follow the scenario clock (`update_daylight`).
    commands.spawn((
        Sun,
        DirectionalLight {
            illuminance: 20_000.0,
            shadow_maps_enabled: graphics.shadows,
            ..default()
        },
        Transform::from_xyz(200.0, 400.0, 200.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Moon,
        DirectionalLight {
            illuminance: 0.0,
            color: Color::srgb(0.75, 0.82, 1.0),
            ..default()
        },
        Transform::default(),
    ));
    let camera = commands
        .spawn((
            Camera3d::default(),
            // HDR: emissive lamp lenses glow at night (M6 night lighting) — the glow
            // itself is bloom, which the settings can switch off.
            bevy::camera::Hdr,
            AmbientLight {
                color: Color::srgb(0.7, 0.8, 1.0),
                brightness: 250.0,
                ..default()
            },
            Projection::Perspective(PerspectiveProjection {
                far: 20_000.0,
                ..default()
            }),
            // Weather visibility (M6): `update_daylight` pulls the falloff in and
            // keeps the fog colour on the sky colour.
            DistanceFog {
                falloff: FogFalloff::from_visibility(CLEAR_VISIBILITY),
                ..default()
            },
            Transform::default(),
            MeshPickingCamera,
            ui::CabCamera,
        ))
        .id();
    if graphics.bloom {
        commands.entity(camera).insert(Bloom::NATURAL);
    }

    // Rain and snow: a particle column of crossed quads that follows the camera
    // and scrolls downwards (`update_precipitation`). Both fields exist from the
    // start; the scenario's weather decides which one is visible.
    let precip_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.75, 0.78, 0.82, 0.5),
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        double_sided: true,
        perceptual_roughness: 1.0,
        ..default()
    });
    let rain = meshes.add(precipitation_mesh(2000, 0.025, 0.4, 11));
    let snow = meshes.add(precipitation_mesh(1500, 0.06, 0.06, 12));
    for (mesh, snow, speed) in [(rain, false, 9.0), (snow, true, 1.4)] {
        commands.spawn((
            Precipitation { snow, speed },
            Mesh3d(mesh),
            MeshMaterial3d(precip_material.clone()),
            Transform::default(),
            Visibility::Hidden,
        ));
    }

    ui::spawn_hud(&mut commands);
    mods_ui::spawn_panel(&mut commands);

    // A character model for the walker (plan ch. 12.4): `--character <file>` takes the
    // same `mods://` paths as the vehicle models. Without one the walker stays a body
    // without a picture, which in the first person is all he ever is.
    if let Some(file) = arg("--character") {
        let scene = assets.load(GltfAssetLabel::Scene(0).from_asset(models::asset_path(&file)));
        commands.spawn((
            WorldAssetRoot(scene),
            Transform::default(),
            Visibility::Hidden,
            walk::CharacterModel,
        ));
    }

    // `--camera outside` starts on the external camera — handy for screenshots of a
    // vehicle model — and `--camera walk` on foot, which a screenshot cannot reach
    // otherwise (F4 needs a key press).
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
        _ => {}
    }

    commands.insert_resource(TerrainInfo::default());
    commands.insert_resource(streamer);
    commands.insert_resource(ViewDistance(graphics.view_distance));
    commands.insert_resource(Origin(origin));
    commands.insert_resource(PlayerTrain(player));
    commands.insert_resource(AiDrivers(drivers));
    commands.insert_resource(SimResource(sim));
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
fn arg(name: &str) -> Option<String> {
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
fn spawn_train(sim: &mut Sim, head: TrackPosition, coaches: usize, loco: VehicleSpec) -> usize {
    let mut vehicles = vec![Vehicle::new(loco, head)];
    for _ in 0..coaches {
        vehicles.push(Vehicle::new(passenger_coach(), head));
    }
    let index = sim.add_train(Train::assemble(vehicles, head, &sim.net));
    // Vehicles start prepared — the "cold locomotive" is a scenario of its own (M6).
    for v in &mut sim.trains[index].vehicles {
        if v.is_powered() {
            v.traction.battery = true;
            v.traction.pantograph_command = true;
            v.traction.main_switch_command = true;
            v.traction.pantograph = 1.0;
            v.traction.compressor = true;
        }
    }
    index
}

fn drive_ai(mut sim: ResMut<SimResource>, mut drivers: ResMut<AiDrivers>, time: Res<Time>) {
    let dt = time.delta_secs_f64().min(0.25);
    for (train, ai) in drivers.0.iter_mut() {
        ai.drive(&mut sim.0, *train, dt);
    }
}

fn step_simulation(mut sim: ResMut<SimResource>, time: Res<Time>) {
    sim.0.advance(time.delta_secs_f64());
}

/// Behaviour scripts of the mods — signal aspects and cab automation (plan ch. 19).
fn run_mod_scripts(mut sim: ResMut<SimResource>, mut mods: ResMut<Mods>, time: Res<Time>) {
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
    let head = sim.0.trains[player.0].vehicles[0].pos.pose(&sim.0.net).pos;
    if origin.0.rebase_if_needed(head) {
        render::resync_anchored(&origin.0, &mut anchored);
    }
}

/// Mirror vehicle poses from the simulation into transforms.
fn sync_vehicles(
    sim: Res<SimResource>,
    origin: Res<Origin>,
    mut query: Query<(&VehicleView, &mut Transform)>,
) {
    for (view, mut transform) in query.iter_mut() {
        let Some(train) = sim.0.trains.get(view.train) else {
            continue;
        };
        let Some(vehicle) = train.vehicles.get(view.vehicle) else {
            continue;
        };
        let pose = vehicle.pos.pose(&sim.0.net);
        let up = origin.0.dir_to_render(pose.up);
        transform.translation = origin.0.to_render(pose.pos) + up * 2.2;
        transform.rotation = origin.0.look_rotation(pose.tangent, pose.up);
    }
}

/// Transform and light of one celestial body, disjoint from the other one.
type BodyLight<'w, 's, B, Other> = Query<
    'w,
    's,
    (&'static mut Transform, &'static mut DirectionalLight),
    (With<B>, Without<Other>),
>;

/// Sun and moon follow the wall clock (plan ch. 14): position from date, time and the
/// georeferenced location, light and sky colour from the sun's elevation; the weather
/// dims the sun, greys the sky and pulls the distance fog in (M6).
// A Bevy system takes its resources as parameters — the argument count says nothing here.
#[allow(clippy::too_many_arguments)]
fn update_daylight(
    sim: Res<SimResource>,
    origin: Res<Origin>,
    graphics: Res<settings::Graphics>,
    mut clear: ResMut<ClearColor>,
    mut daylight: ResMut<Daylight>,
    mut sun: BodyLight<Sun, Moon>,
    mut moon: BodyLight<Moon, Sun>,
    mut ambient: Query<&mut AmbientLight, With<ui::CabCamera>>,
    mut fog: Query<&mut DistanceFog, With<ui::CabCamera>>,
) {
    let (lat, lon, _) = world_coords::geo::from_ecef(origin.0.position());
    let start = sim.0.start;
    let jd = sun::julian_date(
        start.year,
        start.month,
        start.day,
        start.seconds_ut() + sim.0.time,
    );

    let (az, el) = sun::sun_position(jd, lat, lon);
    let e = el.to_degrees() as f32;
    // Daylight factor: ramps up through civil twilight, 1 in full daylight.
    let day = ((e + 6.0) / 12.0).clamp(0.0, 1.0);
    daylight.0 = day;

    // Weather (M6): an overcast sky dims the sun and greys the sky; the
    // visibility pulls the camera's distance fog in.
    let weather = sim.0.weather;
    let overcast: f32 = match weather {
        Weather::Clear => 0.0,
        Weather::Rain => 0.8,
        Weather::Snow => 0.6,
        Weather::Fog => 0.7,
    };

    if let Ok((mut tf, mut light)) = sun.single_mut() {
        *tf = Transform::from_rotation(look_at_body(az, el));
        light.illuminance = 20_000.0 * (1.0 - 0.85 * overcast) * el.sin().max(0.0) as f32;
        light.shadow_maps_enabled = graphics.shadows && e > 0.0 && overcast < 0.5;
        let c = lerp3(
            (1.0, 0.60, 0.30),
            (1.0, 1.0, 1.0),
            (e / 15.0).clamp(0.0, 1.0),
        );
        light.color = Color::srgb(c.0, c.1, c.2);
    }

    let (maz, mel, phase) = sun::moon_position(jd, lat, lon);
    let moonlight = if mel > 0.0 {
        phase as f32 * (1.0 - day)
    } else {
        0.0
    };
    if let Ok((mut tf, mut light)) = moon.single_mut() {
        *tf = Transform::from_rotation(look_at_body(maz, mel));
        // ponytail: a real full moon is ~0.25 lx and invisible without auto-exposure —
        // the night is lit artistically bright instead.
        light.illuminance = 40.0 * moonlight;
    }

    for mut a in &mut ambient {
        a.brightness = 6.0 + 244.0 * day + 10.0 * moonlight;
        let c = lerp3((0.45, 0.55, 0.85), (0.7, 0.8, 1.0), day);
        a.color = Color::srgb(c.0, c.1, c.2);
    }

    // Sky: night ↔ day, with a warm band while the sun crosses the horizon;
    // overcast weather greys it all out (at night it stays dark).
    let sky = lerp3((0.01, 0.02, 0.05), (0.55, 0.68, 0.82), day);
    let dawn = (1.0 - (e / 10.0).abs()).clamp(0.0, 0.6);
    let sky = lerp3(sky, (0.83, 0.52, 0.32), dawn);
    let sky = lerp3(sky, (0.56, 0.58, 0.61), overcast * day);
    clear.0 = Color::srgb(sky.0, sky.1, sky.2);

    for mut fog in &mut fog {
        fog.color = clear.0;
        let visibility = weather.visibility().map_or(CLEAR_VISIBILITY, |v| v as f32);
        fog.falloff = FogFalloff::from_visibility(visibility);
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

/// Rotation of a directional light shining *from* azimuth/elevation onto the scene.
/// The render space is ENU-aligned: +X east, +Y up, −Z north.
fn look_at_body(azimuth: f64, elevation: f64) -> Quat {
    let (sa, ca) = azimuth.sin_cos();
    let (se, ce) = elevation.sin_cos();
    let to_body = Vec3::new((ce * sa) as f32, se as f32, (-ce * ca) as f32);
    Transform::default().looking_to(-to_body, Vec3::Y).rotation
}

fn lerp3(a: (f32, f32, f32), b: (f32, f32, f32), t: f32) -> (f32, f32, f32) {
    (
        a.0 + (b.0 - a.0) * t,
        a.1 + (b.1 - a.1) * t,
        a.2 + (b.2 - a.2) * t,
    )
}

/// Particle field for rain or snow: `count` crossed quad pairs of `w` × `h` metres
/// in a 36 × 36 m column, repeated three times in y with period [`PRECIP_PERIOD`]
/// so the fall offset can wrap seamlessly (`update_precipitation`).
fn precipitation_mesh(count: usize, w: f32, h: f32, seed: u64) -> Mesh {
    let mut rng = sim_core::rng::Rng::new(seed);
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut indices = Vec::new();
    for _ in 0..count {
        let x = rng.range(-18.0, 18.0) as f32;
        let z = rng.range(-18.0, 18.0) as f32;
        let y = rng.range(0.0, f64::from(PRECIP_PERIOD)) as f32;
        for k in 0..3 {
            let y = y + k as f32 * PRECIP_PERIOD;
            // Two quads crossed at right angles, so the particle is visible
            // from every side without billboarding.
            for (dx, dz, normal) in [
                (w / 2.0, 0.0, [0.0, 0.0, 1.0]),
                (0.0, w / 2.0, [1.0, 0.0, 0.0]),
            ] {
                let base = positions.len() as u32;
                positions.extend([
                    [x - dx, y, z - dz],
                    [x + dx, y, z + dz],
                    [x + dx, y + h, z + dz],
                    [x - dx, y + h, z - dz],
                ]);
                normals.extend([normal; 4]);
                indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
            }
        }
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, default());
    mesh.try_insert_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .expect("positions fit");
    mesh.try_insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .expect("normals fit");
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

/// Tilts the fall axis into the relative wind: streaks lean against the
/// direction of travel by `atan(v / fall speed)`, capped at [`MAX_SLANT`].
fn fall_rotation(vel: Vec3, fall_speed: f32) -> Quat {
    let wind = (-vel).clamp_length_max(fall_speed * MAX_SLANT);
    Quat::from_rotation_arc(Vec3::NEG_Y, (wind + Vec3::NEG_Y * fall_speed).normalize())
}

/// Keeps the precipitation column on the camera, lets it fall along an axis
/// slanted by the relative wind, and shows the field the current weather asks
/// for. The wind is the player train's speed — the outside cameras ride along,
/// so the same slant is right for them.
fn update_precipitation(
    sim: Res<SimResource>,
    player: Res<PlayerTrain>,
    origin: Res<Origin>,
    camera: Query<&Transform, With<ui::CabCamera>>,
    mut fields: Query<(&Precipitation, &mut Transform, &mut Visibility), Without<ui::CabCamera>>,
) {
    let Ok(cam) = camera.single() else {
        return;
    };
    let vehicle = &sim.0.trains[player.0].vehicles[0];
    let vel = origin.0.dir_to_render(vehicle.pos.pose(&sim.0.net).tangent) * vehicle.v as f32;
    for (field, mut tf, mut visibility) in &mut fields {
        let wanted = match sim.0.weather {
            Weather::Rain => !field.snow,
            Weather::Snow => field.snow,
            Weather::Clear | Weather::Fog => false,
        };
        *visibility = if wanted {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        let rot = fall_rotation(vel, field.speed);
        tf.rotation = rot;
        tf.translation = cam.translation
            + rot
                * Vec3::new(
                    0.0,
                    -PRECIP_PERIOD - fall_offset(sim.0.time, field.speed),
                    0.0,
                );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fall_offset_wraps_within_one_period() {
        for t in [0.0, 1.7, 100.0, 86_400.0] {
            let o = fall_offset(t, 9.0);
            assert!((0.0..PRECIP_PERIOD).contains(&o), "offset {o} at t={t}");
        }
    }

    #[test]
    fn fall_rotation_leans_against_travel_and_caps() {
        // At rest the streaks stay vertical.
        let dir = fall_rotation(Vec3::ZERO, 9.0) * Vec3::NEG_Y;
        assert!(dir.angle_between(Vec3::NEG_Y) < 1e-4);
        // 20 m/s forward (−z): streaks lean backwards by atan(20/9).
        let dir = fall_rotation(Vec3::new(0.0, 0.0, -20.0), 9.0) * Vec3::NEG_Y;
        assert!((dir.z / -dir.y - 20.0 / 9.0).abs() < 1e-3, "slant {dir}");
        // Far above the cap the slant ratio stays at MAX_SLANT.
        let dir = fall_rotation(Vec3::new(0.0, 0.0, -100.0), 9.0) * Vec3::NEG_Y;
        assert!(
            (dir.z / -dir.y - MAX_SLANT).abs() < 1e-3,
            "capped slant {dir}"
        );
    }
}
