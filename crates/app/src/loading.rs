//! Loading screen: the wait between picking a run and driving it.
//!
//! Building a run used to happen in one synchronous `setup` on entering `Driving`,
//! freezing the last menu frame until the world stood. Now the menu hands over to
//! [`GameState::Loading`](crate::GameState), which shows this screen and builds the
//! run in stages — one [`Stage`] per frame, so the progress bar, the status line and
//! the spinner all get a frame to breathe between two heavy steps. When the last
//! stage is done the bar eases to full and the run starts, which takes this screen
//! down with it (`DespawnOnExit`).
//!
//! The stages are the old `setup` cut along its own seams: the simulation first
//! (line, trains, scenario), then the terrain data, the track and the signals, the
//! vehicles, and finally sky, camera and HUD. The command line flavours (`--line`,
//! `--dgm`, …) read the same flags `setup` read, so a non-interactive run builds
//! exactly what it built before — only with a screen in front of it.
//!
//! Multiplayer (CLAUDE.md): nothing here travels. The loader only builds the local
//! world out of the local selection; the setpoints still replicate through
//! `CabInputs` once the run is driving.

use bevy::core_pipeline::prepass::DepthPrepass;
use bevy::pbr::FogFalloff;
use bevy::picking::mesh_picking::MeshPickingCamera;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::ui::widget::NodeImageMode;
use content::import::dgm::TerrainSource;
use content::terrain::{
    Buildings, Scenery, TerrainBuilder, TerrainEdits, TerrainOptions, Vegetation,
};
use i18n::t;
use mod_runtime::ModRuntime;
use world_coords::RenderOrigin;

use crate::menu::Selection;
use crate::render::{self, Origin};
use crate::streaming::TerrainStreamer;
use crate::theme::{
    BASE, BRAND, Face, Fonts, TEXT_BRIGHT, TEXT_DIM, TEXT_FAINT, TEXT_MID, TRACK, Wallpaper, text,
};
use crate::{GameState, Mods, PlayerTrain, SimResource, TerrainInfo, VehicleKit, ViewDistance};

/// One step of the build. Exactly one runs per frame while loading, in this order —
/// each leaves its products in [`LoadingStash`] for the later ones.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Stage {
    /// Line, trains and scenario out of the menu selection (`world::build`).
    Sim,
    /// Elevation data, crowd, fields, waters and roads into a [`TerrainBuilder`].
    Terrain,
    /// Track meshes and signal models into the world.
    Track,
    /// Vehicle catalogue, terrain streamer and the trains' views.
    Vehicles,
    /// Sky, camera, precipitation, HUD and the walker's body.
    Sky,
    /// Everything into resources, then the sound table and the cab displays.
    Finish,
    /// Built. The bar still eases to full before the run starts.
    Done,
}

/// How far the build is: the stage on the bench, the fraction the bar eases towards,
/// the fraction it shows, and what the status line says (an i18n key).
#[derive(Resource)]
pub(crate) struct LoadingProgress {
    stage: Stage,
    fraction: f32,
    shown: f32,
    status: &'static str,
    done: bool,
    finalized: bool,
}

/// What the finished stages leave for the later ones. Everything a run is made of
/// passes through here on its way into the world.
#[derive(Resource, Default)]
pub(crate) struct LoadingStash {
    world: Option<crate::world::World>,
    origin: Option<RenderOrigin>,
    season: Option<render::Season>,
    fingerprint: u64,
    builder: Option<TerrainBuilder>,
    passenger_names: Vec<String>,
    kit: Option<VehicleKit>,
    streamer: Option<TerrainStreamer>,
}

/// The two lines under the bar that change while loading: what step this is, and how
/// far along it is.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoadingLine {
    Status,
    Percent,
}

/// The run's name over the bar — what the wait is for. Only read by the test.
#[derive(Component)]
pub(crate) struct LoadingTitle;

/// The fill of the progress bar, 0 … 100 per cent wide.
#[derive(Component)]
pub(crate) struct LoadingFill;

/// The spinning baton over the bar. `animate_loading` turns it a little every frame.
#[derive(Component)]
pub(crate) struct LoadingSpinner;

/// Puts the screen up and the build on the bench. Chained behind
/// `remember_before_run`, so the snapshot predates every entity made here.
pub(crate) fn begin_loading(
    mut commands: Commands,
    fonts: Res<Fonts>,
    wallpaper: Res<Wallpaper>,
    selection: Res<Selection>,
    mods: Res<Mods>,
) {
    commands.insert_resource(LoadingStash::default());
    commands.insert_resource(LoadingProgress {
        stage: Stage::Sim,
        fraction: 0.05,
        shown: 0.0,
        status: "load-step-sim",
        done: false,
        finalized: false,
    });
    let (title, subtitle) = run_title(&selection, &mods.0);
    spawn_screen(&mut commands, &fonts, &wallpaper, title, subtitle);
}

/// What the screen says is being built: the scenario's name, the service's number
/// and route, or — on a free run — the line and the vehicle.
fn run_title(selection: &Selection, mods: &ModRuntime) -> (String, String) {
    let line_label = match &selection.line_ref {
        Some(id) => mods
            .mods
            .lines
            .get(id)
            .map(|line| line.name.clone())
            .or_else(|| {
                mods.mods
                    .compositions
                    .get(id)
                    .map(|composition| composition.name.clone())
            })
            .unwrap_or_else(|| id.clone()),
        None => t!("menu-line-builtin"),
    };
    if let Some(id) = &selection.scenario_id {
        let name = mods
            .mods
            .scenarios
            .get(id)
            .map(|scenario| scenario.name.clone())
            .unwrap_or_else(|| id.clone());
        return (name, line_label);
    }
    if let Some(reference) = &selection.service {
        let name = crate::world::resolve_day(mods, &reference.day)
            .and_then(|day| day.services.get(reference.index).cloned())
            .map(|service| {
                let (from, to) = service.route();
                format!("{} · {from} – {to}", service.number)
            })
            .unwrap_or_else(|| t!("menu-select-run"));
        return (name, line_label);
    }
    let vehicle_label = match &selection.loco_id {
        Some(id) => mods
            .mods
            .vehicles
            .get(id)
            .map(|vehicle| vehicle.name.clone())
            .unwrap_or_else(|| id.clone()),
        None => t!("menu-loco-builtin"),
    };
    (line_label, vehicle_label)
}

fn spawn_screen(
    commands: &mut Commands,
    fonts: &Fonts,
    wallpaper: &Wallpaper,
    title: String,
    subtitle: String,
) {
    commands.spawn((Camera2d, DespawnOnExit(GameState::Loading)));
    let root = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(14.0),
                ..default()
            },
            BackgroundColor(BASE),
            DespawnOnExit(GameState::Loading),
        ))
        .id();
    // The menu's wallpaper, under a wash heavy enough for type.
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
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(Color::srgba(0.020, 0.020, 0.024, 0.90)),
        ChildOf(root),
    ));
    // The mark: traffic red, the same colour the start button wears.
    commands.spawn((
        Node {
            width: Val::Px(52.0),
            height: Val::Px(4.0),
            ..default()
        },
        BackgroundColor(BRAND),
        ChildOf(root),
    ));
    commands.spawn((
        text(
            fonts,
            t!("load-title").to_uppercase(),
            Face::Semibold,
            12.0,
            TEXT_DIM,
        ),
        ChildOf(root),
    ));
    commands.spawn((
        text(fonts, title, Face::Semibold, 30.0, TEXT_BRIGHT),
        LoadingTitle,
        ChildOf(root),
    ));
    commands.spawn((
        text(fonts, subtitle, Face::Sans, 14.0, TEXT_MID),
        ChildOf(root),
    ));
    // The spinner: a baton turning around its own centre.
    let slot = commands
        .spawn((
            Node {
                width: Val::Px(120.0),
                height: Val::Px(44.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            ChildOf(root),
        ))
        .id();
    commands.spawn((
        Node {
            width: Val::Px(38.0),
            height: Val::Px(5.0),
            border_radius: BorderRadius::all(Val::Px(2.5)),
            ..default()
        },
        BackgroundColor(BRAND),
        LoadingSpinner,
        ChildOf(slot),
    ));
    // The bar: traffic red filling a dark track, per cent and step below it.
    let track = commands
        .spawn((
            Node {
                width: Val::Px(480.0),
                height: Val::Px(6.0),
                border_radius: BorderRadius::all(Val::Px(3.0)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(TRACK),
            ChildOf(root),
        ))
        .id();
    commands.spawn((
        Node {
            width: Val::Percent(0.0),
            height: Val::Percent(100.0),
            border_radius: BorderRadius::all(Val::Px(3.0)),
            ..default()
        },
        BackgroundColor(BRAND),
        LoadingFill,
        ChildOf(track),
    ));
    commands.spawn((
        text(fonts, "0 %".to_string(), Face::Mono, 12.0, TEXT_DIM),
        LoadingLine::Percent,
        ChildOf(root),
    ));
    commands.spawn((
        text(fonts, t!("load-step-sim"), Face::Sans, 14.0, TEXT_MID),
        LoadingLine::Status,
        ChildOf(root),
    ));
    commands.spawn((
        text(fonts, t!("load-hint"), Face::Sans, 12.0, TEXT_FAINT),
        ChildOf(root),
    ));
}

/// Hands the build to the next stage: what fraction the bar eases towards now, and
/// what the status line says while it gets there.
fn advance(progress: &mut LoadingProgress, stage: Stage, fraction: f32, status: &'static str) {
    progress.stage = stage;
    progress.fraction = fraction;
    progress.status = status;
}

// ---------------------------------------------------------------------------------
// Stages
// ---------------------------------------------------------------------------------

/// The simulation: line, trains and either a scenario or a service out of an
/// operating day. Everything after this reads it back out of the stash.
pub(crate) fn load_sim(
    mut commands: Commands,
    mut mods: ResMut<Mods>,
    mut manager: ResMut<crate::mods_ui::ModManager>,
    selection: Res<Selection>,
    mut stash: ResMut<LoadingStash>,
    mut progress: ResMut<LoadingProgress>,
) {
    if progress.stage != Stage::Sim {
        return;
    }
    // `--hud full|reduced|off` puts the display in one of its three steps for a
    // screenshot, which cannot press a key. It goes into a resource of its own rather than
    // into the setting: the settings file is written on exit whether anything changed or
    // not, and a photograph must not leave its step behind in the player's preferences.
    if let Some(step) = crate::arg("--hud") {
        match step.as_str() {
            "off" => {
                commands.insert_resource(crate::hud::HudOverride(crate::settings::HudMode::Off))
            }
            "reduced" => {
                commands.insert_resource(crate::hud::HudOverride(crate::settings::HudMode::Reduced))
            }
            "full" => {
                commands.insert_resource(crate::hud::HudOverride(crate::settings::HudMode::Full))
            }
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

    let mut built = crate::world::build(&mut mods.0, &selection);
    // `--time 21:40` and `--date 2026-10-03` move the run's wall clock, the way
    // `--hud` moves the display: a screenshot cannot open the scenario file, and
    // the night sky is exactly what a rendering smoke test wants to see.
    if let Some(clock) = crate::arg("--time")
        && let Some((hour, minute)) = crate::parse_pair(&clock, ':')
    {
        built.sim.start.hour = hour;
        built.sim.start.minute = minute;
    }
    if let Some(date) = crate::world::date_arg() {
        built.sim.start.year = date.year;
        built.sim.start.month = date.month;
        built.sim.start.day = date.day;
    }
    // `--wipers 2` starts with the wipers running: they are a cab control, and a
    // screenshot has no hands.
    if let Some(mode) = crate::arg("--wipers").and_then(|m| m.parse::<u8>().ok()) {
        for cab in &mut built.sim.controls {
            cab.wipers = mode.min(3);
        }
    }
    // `--weather snow` starts one of `sim_core::weather`'s presets. In a normal
    // run the front *moves in* over `weather::TRANSITION` — rain builds from a
    // first drizzle, the pane wets slowly, the rail goes greasy before wet. Only
    // a screenshot gets it placed at once, ground and all: it cannot wait five
    // minutes, and it wants the end state, not the approach.
    if let Some(name) = crate::arg("--weather") {
        let wanted = name.to_ascii_lowercase();
        match sim_core::weather::Preset::ALL
            .into_iter()
            .find(|p| format!("{p:?}").to_ascii_lowercase() == wanted)
        {
            Some(preset) if crate::arg("--screenshot").is_some() => {
                built.sim.weather.place(preset.weather(), 0.0)
            }
            Some(preset) => built.sim.weather.set(preset.weather(), 0.0),
            None => warn!("unknown weather: {name}"),
        }
    }
    // Both sides of a multiplayer run have to have built the same world; the fingerprint
    // is what says so on joining (`net.rs`).
    let fingerprint = crate::world::fingerprint(&built.line.name, &built.sim);

    // Render origin at the head of the train. A consist with no vehicles stands nowhere,
    // so the origin starts at the line's own anchor instead (`sim_core::shunt`).
    let start = built.sim.trains[built.player]
        .vehicles
        .first()
        .map(|v| v.pos.pose(&built.sim.net).pos)
        .unwrap_or_else(|| {
            built
                .sim
                .net
                .edges()
                .first()
                .map_or_else(world_coords::EcefPos::default, |e| e.eval(0.0).pos)
        });
    let origin = RenderOrigin::new(start);

    // Ground, scenery and foliage wear the season of the scenario's start date
    // — the same date the sun and moon are computed from (plan ch. 14).
    let season = render::Season::on(built.sim.start.month, built.sim.start.day);

    stash.fingerprint = fingerprint;
    stash.origin = Some(origin);
    stash.season = Some(season);
    stash.world = Some(built);
    advance(&mut progress, Stage::Terrain, 0.22, "load-step-terrain");
}

/// The terrain data: elevation sources, crowd, fields, waters and roads, all packed
/// into the builder the streamer later shares read-only.
pub(crate) fn load_terrain(
    mods: Res<Mods>,
    mut stash: ResMut<LoadingStash>,
    mut progress: ResMut<LoadingProgress>,
) {
    if progress.stage != Stage::Terrain {
        return;
    }
    let world = stash.world.as_ref().expect("the sim is built first");
    // Terrain: from real elevation data with `--dgm <directory>`, otherwise flat.
    // Tiles are not built here but while driving (plan 4.3) — a 100 km line has more
    // terrain than fits in memory at once. The builder exists before the scenery,
    // because objects that snap to the terrain ask it for the ground height.
    // `--dgm` may be repeated for a line across a UTM zone boundary; the n-th
    // `--epsg` belongs to the n-th `--dgm` (the last one carries on when there
    // are fewer).
    let zones = crate::args_all("--epsg");
    let mut sources = Vec::new();
    for (i, dir) in crate::args_all("--dgm").iter().enumerate() {
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
    for h in &world.line.heights {
        let Some(dir) = mods.0.mods.resolve_path(&h.path) else {
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
        zone: crate::dgm_zone(),
        fallback_height: 100.0,
        ..default()
    };
    // The people (plan ch. 12): the passengers the crowd and the seats are made
    // of, in registry order. Nothing about them is replicated — the crowd is a
    // function of the line's name and the seats of the train's indices, so every
    // client shows the same faces.
    let passenger_names: Vec<String> = mods
        .0
        .mods
        .characters
        .iter()
        .filter(|(_, c)| c.has_role(content::Role::Passenger))
        .map(|(key, _)| key.clone())
        .collect();
    let crowd = content::Crowd::from_line(
        &world.line,
        &world.sim.net,
        terrain_options.zone,
        &passenger_names,
        content::people::line_seed(&world.line.name),
    );
    info!(
        "people: {} on the platforms and ways ({} of them walking), {} passenger characters installed",
        crowd.len(),
        crowd.walking(),
        passenger_names.len()
    );
    // The line's farmland, cut to the tiles it covers (see `content::farmland`).
    let farmland = content::farmland::Fields::from_line(
        &world.line,
        terrain_options.zone,
        terrain_options.tile_size,
    );
    match farmland.overlaps() {
        (0, _) => info!("fields: {} on the line", farmland.len()),
        // Parcels that stand on each other are a fault in the register, not
        // in the renderer: the later one gives the ground up so nothing is
        // drawn twice, and the count says how much of that had to happen.
        (n, area) => info!(
            "fields: {} on the line ({n} overlapped one another, {:.2} ha given up)",
            farmland.len(),
            area / 10_000.0,
        ),
    }
    let waters = content::water::Waters::from_line(
        &world.line,
        terrain_options.zone,
        terrain_options.tile_size,
    );
    info!("water: {} on the line", waters.len());
    // The line's roads, their carriageways draped on the terrain at build
    // time (see `content::roads`).
    let roads = content::roads::Roads::from_line(
        &world.line,
        terrain_options.zone,
        terrain_options.tile_size,
    );
    info!("roads: {} on the line", roads.len());
    // Trees, scenery objects and people come with the tiles: each stands on
    // the ground of the tile it lands on, and streams in and out with it.
    let terrain_builder = TerrainBuilder::new(&world.sim.net, sources, terrain_options)
        .with_vegetation(Vegetation::from_line(&world.line, terrain_options.zone))
        .with_scenery(Scenery::from_line(
            &world.line,
            &world.sim.net,
            terrain_options.zone,
        ))
        .with_buildings(Buildings::from_line(
            &world.line,
            &world.sim.net,
            terrain_options.zone,
        ))
        .with_crowd(crowd)
        .with_fields(farmland)
        .with_waters(waters)
        .with_roads(roads)
        .with_power_lines(content::power::PowerLines::from_line(
            &world.line,
            terrain_options.zone,
            terrain_options.tile_size,
        ))
        .with_edits(TerrainEdits::from_line(&world.line, terrain_options.zone));
    stash.builder = Some(terrain_builder);
    stash.passenger_names = passenger_names;
    advance(&mut progress, Stage::Track, 0.40, "load-step-track");
}

/// The track meshes and every signal of the line: the placement's override,
/// otherwise the signal type's default, otherwise the placeholder mast.
pub(crate) fn load_track(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    world_materials: render::WorldMaterials,
    assets: Res<AssetServer>,
    mods: Res<Mods>,
    stash: Res<LoadingStash>,
    mut progress: ResMut<LoadingProgress>,
) {
    if progress.stage != Stage::Track {
        return;
    }
    // Back to the names the body has always used.
    let render::WorldMaterials {
        standard: mut materials,
        rail: mut rail_materials,
    } = world_materials;
    let world = stash.world.as_ref().expect("the sim is built first");
    let origin = stash.origin.as_ref().expect("the origin is placed first");

    render::spawn_track(
        &mut commands,
        &mut meshes,
        &mut materials,
        &mut rail_materials,
        &assets,
        &world.sim.net,
        origin,
    );

    // Signal models (plan ch. 15.3): the placement's override, otherwise the signal
    // type's default; a signal without either gets the placeholder mast.
    let line_source = &world.line;
    let signal_models: Vec<Option<sim_core::interlock::SignalModel>> = line_source
        .signals
        .iter()
        .enumerate()
        .map(|(i, _)| {
            let name = mods.0.mods.signal_model_name(line_source, i)?;
            let model = mods.0.mods.signal_models.get(name).cloned();
            if model.is_none() {
                warn!("signal {i}: unknown signal model {name:?}");
            }
            model
        })
        .collect();
    let views: Vec<world_render::SignalView> = world
        .sim
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
        &world.sim.net,
        &views,
        origin,
    );
    drop(views);
    commands.insert_resource(aspect_materials);
    commands.insert_resource(world_render::SignalModels(signal_models));
    advance(&mut progress, Stage::Vehicles, 0.58, "load-step-vehicles");
}

/// The trains' views and everything the streaming needs: the object catalogue, the
/// streamer itself and the placeholder kit a dispatched service is drawn with.
#[allow(clippy::too_many_arguments)]
pub(crate) fn load_vehicles(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    world_materials: render::WorldMaterials,
    assets: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    mut terrain_materials: ResMut<Assets<render::TerrainMaterial>>,
    settings: crate::settings::GraphicsRead,
    mods: Res<Mods>,
    mut stash: ResMut<LoadingStash>,
    mut progress: ResMut<LoadingProgress>,
) {
    if progress.stage != Stage::Vehicles {
        return;
    }
    let render::WorldMaterials {
        standard: mut materials,
        rail: _,
    } = world_materials;
    let season = stash
        .season
        .expect("the season is read off the clock first");
    let builder = stash
        .builder
        .take()
        .expect("the terrain data is packed first");
    let world = stash.world.as_ref().expect("the sim is built first");

    // Vegetation, scenery and the crowd: the line's object names resolved
    // against the installed mods.
    let passengers =
        world_render::Passengers::resolve(&stash.passenger_names, &mods.0.mods.characters, &assets);
    let catalog = world_render::WorldCatalog::new(
        builder.tree_objects(),
        builder.scenery_objects(),
        &mods.0.mods.objects,
        passengers.clone(),
        &assets,
        &mut meshes,
        &mut materials,
        season,
    );
    let streamer = TerrainStreamer::new(
        builder,
        render::terrain_material(
            &mut images,
            &mut terrain_materials,
            season,
            crate::settings::ground_quality(&settings.graphics),
        ),
        catalog,
        f64::from(settings.graphics.view_distance),
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
    for train in std::iter::once(world.player).chain(world.drivers.iter().map(|(t, _)| *t)) {
        crate::spawn_vehicle_views(
            &mut commands,
            &assets,
            &mut meshes,
            &kit,
            &world.sim,
            train,
            world.player,
        );
    }
    stash.kit = Some(kit);
    stash.streamer = Some(streamer);
    advance(&mut progress, Stage::Sky, 0.74, "load-step-sky");
}

/// Sky, camera, precipitation, HUD and the walker's body. Nothing in here moves —
/// it only puts up what the first frame draws.
#[allow(clippy::too_many_arguments)]
pub(crate) fn load_sky(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut media: ResMut<Assets<bevy::light::atmosphere::ScatteringMedium>>,
    mut star_materials: ResMut<Assets<world_render::sky::StarMaterial>>,
    mut moon_materials: ResMut<Assets<world_render::sky::MoonMaterial>>,
    mut precip_materials: ResMut<Assets<world_render::precipitation::PrecipitationMaterial>>,
    assets: Res<AssetServer>,
    settings: crate::settings::GraphicsRead,
    mut images: ResMut<Assets<Image>>,
    fonts: Res<Fonts>,
    binds: Res<crate::bindings::Binds>,
    mods: Res<Mods>,
    stash: Res<LoadingStash>,
    mut progress: ResMut<LoadingProgress>,
) {
    if progress.stage != Stage::Sky {
        return;
    }
    let world = stash.world.as_ref().expect("the sim is built first");
    let origin = stash.origin.as_ref().expect("the origin is placed first");

    // Atmosphere, sun, moon and stars — all of them off the scenario clock and
    // the georeferenced place (`feed_sky`).
    world_render::sky::spawn(
        &mut commands,
        &mut meshes,
        &mut media,
        &mut star_materials,
        &mut moon_materials,
        settings.graphics.shadows,
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
            world_render::sky::camera_settings(),
            // The water reads its reflections out of the depth the world was
            // drawn with (`world_render::water`); without the prepass it falls
            // back to mirroring the sky alone. The upscalers want it too.
            DepthPrepass,
            // Near-field extinction (`feed_sky`): the atmosphere's own haze term
            // carries the colour and the distance, but a planetary medium's LUTs
            // do not resolve 300 m of fog. This is what closes it.
            DistanceFog {
                falloff: FogFalloff::from_visibility(crate::CLEAR_VISIBILITY),
                ..default()
            },
            Projection::Perspective(PerspectiveProjection {
                far: 20_000.0,
                ..default()
            }),
            Transform::default(),
            MeshPickingCamera,
            crate::ui::CabCamera,
        ))
        .id();
    if settings.graphics.bloom {
        commands.entity(camera).insert(Bloom::NATURAL);
    }
    // `apply_scene` only fires on a changed setting, and starting a run does not change
    // one — so the camera is dressed here as well as there. Upscaling before the
    // anti-aliasing: the latter reads it, because an upscaler wants MSAA off.
    crate::settings::apply_upscaling(
        &mut commands.entity(camera),
        &settings.graphics,
        *settings.upscaling,
    );
    crate::settings::apply_anti_aliasing(&mut commands.entity(camera), &settings.graphics);

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
            crate::Precipitation { snow, speed, near },
            Mesh3d(meshes.add(crate::precipitation_mesh(count, w, h, spread, seed))),
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
        commands.insert_resource(crate::hud::Overlays {
            help: true,
            diagnostics: true,
        });
    }
    crate::ui::spawn_crosshair(&mut commands);
    // The speedometer is drawn for one scale, so the face has to be made after the
    // vehicle is known — a dial whose figures changed with the line would be a bar chart
    // pretending to be an instrument.
    let v_max = {
        let train = &world.sim.trains[world.player];
        train
            .vehicles
            .get(train.cab)
            .map(|v| v.spec.v_max)
            .filter(|v| *v > 0.0)
            .unwrap_or(160.0)
    };
    let drawings = crate::hud::Drawings::draw(&mut images, v_max);
    crate::hud::spawn_hud(&mut commands, &fonts, &drawings, &binds);
    commands.insert_resource(drawings);
    crate::mods_ui::spawn_panel(&mut commands);

    // A character model for the walker (plan ch. 12.4): `--character` names one of
    // the mods' people (`people:f01_lena`) or takes a file on the same `mods://` paths
    // as the vehicle models; without the flag the first character with the `Player`
    // role, in registry order. Without any the walker stays a body without a picture,
    // which in the first person is all he ever is. The model is a person like the
    // passengers (`world_render::people`); `walk::animate_walker` moves it.
    let character = match crate::arg("--character") {
        Some(key) => Some(
            mods.0
                .mods
                .characters
                .get(&key)
                .map_or(key, |c| c.model.clone()),
        ),
        None => mods
            .0
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
            crate::walk::CharacterModel,
        ));
    }

    // `--camera outside` starts on the external camera — handy for screenshots of a
    // vehicle model — `--camera walk` on foot, which a screenshot cannot reach
    // otherwise (F4 needs a key press), and `--camera fly` in the free camera of the
    // console's `fly` command.
    match crate::arg("--camera").as_deref() {
        Some("outside") => {
            commands.insert_resource(crate::ui::CameraState {
                mode: crate::ui::CameraMode::Outside,
                distance: 40.0,
                pitch: -0.15,
                ..default()
            });
        }
        Some("walk") => {
            commands.insert_resource(crate::ui::CameraState {
                mode: crate::ui::CameraMode::Walk,
                ..default()
            });
        }
        Some("fly") => {
            // Seeded where the wayside camera would put itself — beside the track,
            // looking at the train. Without it the free camera would wake at the raw
            // origin: rail-head height, inside the lead vehicle.
            //
            // `--fly` and `--look` move both ends of that: a screenshot has no hands
            // on the mouse, so without them the free camera can only ever photograph
            // the train, and anything taller than about ten metres runs out of the
            // top of the frame however far back the camera stands.
            let mut state = crate::ui::CameraState {
                mode: crate::ui::CameraMode::Fly,
                ..default()
            };
            if let Some(front) = world.sim.trains[world.player].vehicles.first() {
                let pose = front.pos.pose(&world.sim.net);
                let pos = origin.to_render(pose.pos);
                let up = origin.dir_to_render(pose.up);
                let forward = origin.dir_to_render(pose.tangent);
                let right = forward.cross(up).normalize_or_zero();
                let at = |offset: Vec3| pos + right * offset.x + up * offset.y + forward * offset.z;
                let eye = at(crate::view_offset("--fly").unwrap_or(Vec3::new(25.0, 6.0, 0.0)));
                let target = at(crate::view_offset("--look").unwrap_or(Vec3::new(0.0, 2.0, 0.0)));
                state.fly = Some(eye);
                let dir = (target - eye).normalize_or_zero();
                state.pitch = dir.y.asin();
                // The angles of the walk's view convention: forward is
                // (−sin yaw · cos pitch, sin pitch, −cos yaw · cos pitch).
                state.yaw = (-dir.x).atan2(-dir.z);
            }
            commands.insert_resource(state);
        }
        _ => {}
    }
    advance(&mut progress, Stage::Finish, 0.88, "load-step-ready");
}

/// Everything into resources: the streamer, the kit, the simulation and who drives
/// what. The sound table and the cab displays follow in [`finalize_loading`], once
/// these commands are applied.
pub(crate) fn finish_loading(
    mut commands: Commands,
    settings: crate::settings::GraphicsRead,
    mut stash: ResMut<LoadingStash>,
    mut progress: ResMut<LoadingProgress>,
) {
    if progress.stage != Stage::Finish {
        return;
    }
    let stash = std::mem::take(&mut *stash);
    let world = stash.world.expect("the sim is built first");
    commands.insert_resource(TerrainInfo::default());
    commands.insert_resource(stash.streamer.expect("the streamer is built first"));
    commands.insert_resource(stash.kit.expect("the kit is built first"));
    commands.insert_resource(ViewDistance(settings.graphics.view_distance));
    commands.insert_resource(Origin(stash.origin.expect("the origin is placed first")));
    commands.insert_resource(crate::net::WorldId(stash.fingerprint));
    commands.insert_resource(PlayerTrain(world.player));
    // The run begins with the player at the desk of the train it put them in.
    commands.insert_resource(crate::crew::Duty(Some(world.player)));
    commands.insert_resource(crate::AiDrivers(world.drivers));
    commands.insert_resource(SimResource(world.sim));
    // A timetable run keeps dispatching after the world is built (`dispatch_services`);
    // a scenario and a free run have nothing left to put on the line.
    commands.insert_resource(world.dispatch);
    match world.day {
        Some(run) => commands.insert_resource(run),
        None => commands.remove_resource::<crate::services::DayRun>(),
    }
    advance(&mut progress, Stage::Done, 1.0, "load-step-ready");
    progress.done = true;
}

/// The sound table and the display cameras need the trains, which `finish_loading`
/// only creates when its commands are applied — chained behind them, this runs once
/// they stand. Afterwards the run counts as built, so coming back out of the pause
/// overlay never builds it twice.
pub(crate) fn finalize_loading(mut commands: Commands, mut progress: ResMut<LoadingProgress>) {
    if !progress.done || progress.finalized {
        return;
    }
    progress.finalized = true;
    commands.insert_resource(crate::RunBuilt);
}

/// While the run is built but not yet counted as such: the sound table and the cab
/// displays still have to be put up.
pub(crate) fn finalizing(progress: Option<Res<LoadingProgress>>) -> bool {
    progress.is_some_and(|progress| progress.done && !progress.finalized)
}

/// The bar has filled and the run stands built: drive.
pub(crate) fn enter_driving(
    progress: Res<LoadingProgress>,
    mut next: ResMut<NextState<GameState>>,
) {
    if progress.finalized && progress.shown >= 0.996 {
        next.set(GameState::Driving);
    }
}

// ---------------------------------------------------------------------------------
// Screen animation
// ---------------------------------------------------------------------------------

/// Eases the bar towards its target, and refills the per cent and status lines.
/// Runs every frame while loading — including the frames a heavy stage blocks
/// around, which is what keeps the wait legible.
pub(crate) fn update_loading_ui(
    time: Res<Time>,
    mut progress: ResMut<LoadingProgress>,
    mut fills: Query<&mut Node, With<LoadingFill>>,
    mut lines: Query<(&LoadingLine, &mut Text)>,
    mut colors: Query<(&LoadingLine, &mut TextColor)>,
) {
    progress.shown += (progress.fraction - progress.shown) * (time.delta_secs() * 6.0).min(1.0);
    if (progress.fraction - progress.shown).abs() < 0.004 {
        progress.shown = progress.fraction;
    }
    for mut node in &mut fills {
        node.width = Val::Percent(progress.shown * 100.0);
    }
    let status = t!(progress.status);
    let percent = format!("{:.0} %", progress.shown * 100.0);
    for (line, mut text) in &mut lines {
        let content = match line {
            LoadingLine::Status => status.clone(),
            LoadingLine::Percent => percent.clone(),
        };
        if **text != content {
            **text = content;
        }
    }
    // The status line breathes while the bar is still moving.
    let pulse = 0.55 + 0.45 * (0.5 + 0.5 * (time.elapsed_secs() * 3.2).sin());
    for (line, mut color) in &mut colors {
        if *line == LoadingLine::Status {
            color.0 = TEXT_MID.with_alpha(pulse);
        }
    }
}

/// Turns the spinner a little every frame.
pub(crate) fn animate_loading(
    time: Res<Time>,
    mut spinners: Query<&mut Transform, With<LoadingSpinner>>,
) {
    for mut spinner in &mut spinners {
        spinner.rotate_z(time.delta_secs() * 3.6);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The screen comes up naming the run, and the bar starts moving towards it.
    #[test]
    fn the_loading_screen_names_the_run_and_fills_the_bar() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
            .init_resource::<Fonts>()
            .init_resource::<Wallpaper>()
            .init_resource::<Selection>()
            .insert_resource(Mods(ModRuntime::load("../../mods")))
            .insert_state(GameState::Loading)
            .add_systems(OnEnter(GameState::Loading), begin_loading)
            .add_systems(
                Update,
                (update_loading_ui, animate_loading).run_if(in_state(GameState::Loading)),
            );
        app.update();

        // A free run with nothing picked is the built-in line and the built-in vehicle.
        let mut titles = app.world_mut().query::<(&LoadingTitle, &Text)>();
        let (_, title) = titles.single(app.world()).expect("a title");
        assert_eq!(**title, t!("menu-line-builtin"));

        // The bar starts empty and moves a little every frame, and the status line
        // names the first step.
        let mut fills = app.world_mut().query_filtered::<&Node, With<LoadingFill>>();
        let before = match fills.single(app.world()).expect("a bar").width {
            Val::Percent(width) => width,
            width => panic!("the fill is a percentage, not {width:?}"),
        };
        for _ in 0..5 {
            app.update();
        }
        let after = match fills.single(app.world()).expect("a bar").width {
            Val::Percent(width) => width,
            width => panic!("the fill is a percentage, not {width:?}"),
        };
        assert!(after > before, "the bar did not move");
        let mut lines = app.world_mut().query::<(&LoadingLine, &Text)>();
        let status = lines
            .iter(app.world())
            .find(|(line, _)| **line == LoadingLine::Status)
            .expect("a status line");
        assert_eq!(**status.1, t!("load-step-sim"));
    }
}
