//! TrainSim-DE — Bevy app: rendering, camera, input, HUD (plan ch. 12).
//!
//! The app ticks `sim-core` with a fixed time step and mirrors the state into ECS components.
//! Simulation logic does **not** belong here.

mod audio;
mod models;
mod mods_ui;
mod render;
mod streaming;
mod ui;

use ai_driver::{AiDriver, ScheduledStop, Timetable};
use bevy::asset::io::AssetSourceBuilder;
use bevy::asset::io::file::FileAssetReader;
use bevy::audio::{DefaultSpatialScale, SpatialScale};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use content::import::dgm::TerrainSource;
use content::terrain::{TerrainBuilder, TerrainOptions, TerrainStats};
use content::vehicles::{br101, passenger_coach};
use content::{musterbahn, re_4711, to_musterstadt};
use mod_runtime::ModRuntime;
use render::{Origin, TerrainChunk, VehicleView, WorldAnchored};
use sim_core::Sim;
use sim_core::train::{Train, Vehicle, VehicleSpec};
use track_model::{EdgeId, TrackPosition};
use world_coords::RenderOrigin;

/// The running simulation.
#[derive(Resource)]
pub struct SimResource(pub Sim);

/// Which train is driven by the player.
#[derive(Resource)]
pub struct PlayerTrain(pub usize);

/// AI drivers of the remaining trains.
#[derive(Resource)]
pub struct AiDrivers(pub Vec<(usize, AiDriver)>);

/// Loaded mods with their Lua state (plan ch. 19).
#[derive(Resource)]
pub struct Mods(pub ModRuntime);

/// Terrain view distance [m] — tiles beyond it are hidden.
#[derive(Resource)]
pub struct ViewDistance(pub f32);

/// Streaming load radius [m] — nothing further away is built, so nothing further away
/// can be drawn either.
const LOAD_RADIUS: f64 = 4_000.0;

/// Key figures of the generated terrain (for the HUD).
#[derive(Resource, Default)]
pub struct TerrainInfo(pub TerrainStats);

/// Number of frames from `--frames N` (CI rendering smoke test, plan ch. 18).
#[derive(Resource)]
struct FrameLimit(u32);

/// Target file from `--screenshot <file.png>`.
#[derive(Resource)]
struct ShotPath(String);

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

    let mut app = App::new();
    // Models, textures and sounds of a mod come from its own directory: `mods://<mod>/…`.
    // Has to be registered before the asset plugin.
    app.register_asset_source(models::SOURCE, mod_asset_source());
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: i18n::t!("window-simulator"),
            ..default()
        }),
        ..default()
    }))
    .insert_resource(ClearColor(Color::srgb(0.55, 0.68, 0.82)))
    // Spatial audio is measured in metres here, and a train is heard hundreds of them
    // away — at the default scale of 1 everything but the cab would be inaudible.
    .insert_resource(DefaultSpatialScale(SpatialScale::new(0.02)))
    .init_resource::<ui::CameraState>()
    .init_resource::<mods_ui::ModManager>()
    .add_systems(Startup, setup)
    // The sound table needs the trains and their view entities, which `setup` only
    // creates when its commands are applied — that is after `Startup`.
    .add_systems(PostStartup, audio::setup_audio)
    .add_systems(
        Update,
        (
            ui::player_input,
            drive_ai,
            step_simulation,
            run_mod_scripts,
            rebase_origin,
            sync_vehicles,
            ui::camera_control,
            streaming::stream_terrain,
            terrain_visibility,
            ui::update_hud,
            audio::update_audio,
            mods_ui::mod_manager,
        )
            .chain(),
    )
    // Vehicle models from mods: bind glTF nodes, switch LODs, move parts (plan ch. 15.3).
    .add_systems(
        Update,
        (
            models::bind_nodes,
            models::update_lod,
            models::animate_parts,
        )
            .after(sync_vehicles),
    );
    if let Some(frames) = frame_limit {
        app.insert_resource(FrameLimit(frames))
            .add_systems(Update, exit_after_frames);
    }
    if let Some(path) = shot {
        app.insert_resource(ShotPath(path));
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

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    assets: Res<AssetServer>,
) {
    // Mods first — line, vehicles and signal types may come from them (plan ch. 19).
    let mut mods = ModRuntime::load("mods");
    for warning in mods.log() {
        warn!("mod: {warning}");
    }
    info!(
        "Mods: {} of {} enabled ({} vehicles, {} lines, {} scenarios, {} signal types, {} scripts)",
        mods.mods.manifests.iter().filter(|m| m.enabled).count(),
        mods.mods.manifests.len(),
        mods.mods.vehicles.len(),
        mods.mods.lines.len(),
        mods.mods.scenarios.len(),
        mods.mods.signal_types.len(),
        mods.mods.scripts.len()
    );

    // Build line and simulation. `--line <mod>:<name>` takes a line from a mod.
    let modded = arg("--line").and_then(|id| match mods.mods.lines.get(&id) {
        Some(line) => Some(line.clone()),
        None => {
            warn!("line {id} not found — using the example line");
            None
        }
    });
    let line_source = modded.clone().unwrap_or_else(musterbahn);
    let mut line = line_source.compile().expect("line compiles");
    for warning in mods
        .mods
        .apply_signal_types(&line_source, &mut line.interlock)
    {
        warn!("{}: {warning}", line_source.name);
    }
    let mut sim = Sim::new(line.net, line.interlock, 2024);

    // `--loco <mod>:<name>` puts a vehicle from a mod at the head of the train.
    let loco = arg("--loco")
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
    if modded.is_none() {
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
                stops: vec![ScheduledStop {
                    name: "Musterstadt".into(),
                    edge: EdgeId(2),
                    s: 2600.0,
                    arrival: 300.0,
                    departure: 360.0,
                    platform: "1".into(),
                }],
            }),
        ));

        // Load the scenario with timetable and scoring (plan 11.4).
        let mut scenario = to_musterstadt();
        scenario.player_train = player;
        sim.set_scenario(scenario, re_4711());
    }

    // `--scenario <mod>:<name>` runs a scenario out of a mod. It brings no timetable of its
    // own — ponytail: scoring then counts the scenario points only, add timetables to mods
    // when a mod actually wants stop scoring.
    if let Some(id) = arg("--scenario") {
        match mods.mods.scenarios.get(&id) {
            Some(scenario) => {
                let mut scenario = scenario.clone();
                scenario.player_train = player;
                let number = scenario.name.clone();
                sim.set_scenario(
                    scenario,
                    sim_core::timetable::Timetable {
                        number,
                        ..default()
                    },
                );
            }
            None => warn!("scenario {id} not found"),
        }
    }

    // Line and scenario hooks: `on_load` now, `on_frame` every frame (plan 19.7).
    mods.begin(&mut sim, &line_source);

    // Render origin at the head of the train.
    let start = sim.trains[player].vehicles[0].pos.pose(&sim.net).pos;
    let origin = RenderOrigin::new(start);

    render::spawn_track(
        &mut commands,
        &mut meshes,
        &mut materials,
        &sim.net,
        &origin,
    );

    // Terrain: from real elevation data with `--dgm <directory>`, otherwise flat.
    // Tiles are not built here but while driving (plan 4.3) — a 100 km line has more
    // terrain than fits in memory at once.
    let source = std::env::args()
        .skip_while(|a| a != "--dgm")
        .nth(1)
        .and_then(|dir| match TerrainSource::from_dir(&dir, dgm_zone()) {
            Ok(s) => {
                info!("DGM: {} tiles from {dir}", s.tile_count());
                Some(s)
            }
            Err(e) => {
                warn!("DGM {dir} not readable: {e}");
                None
            }
        });
    let terrain_options = TerrainOptions {
        zone: dgm_zone(),
        fallback_height: 100.0,
        ..default()
    };
    let streamer = streaming::TerrainStreamer::new(
        TerrainBuilder::new(&sim.net, source, terrain_options),
        render::terrain_materials(&mut materials),
        LOAD_RADIUS,
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
    for train in std::iter::once(player).chain(drivers.iter().map(|(t, _)| *t)) {
        for (i, v) in sim.trains[train].vehicles.iter().enumerate() {
            let view = VehicleView { train, vehicle: i };
            // A vehicle with a model gets its glTF; everything else stays a body
            // (plan ch. 15.3).
            if let Some(model) = v.spec.model.as_ref().filter(|m| !m.file.is_empty()) {
                let entity = commands
                    .spawn((Transform::default(), Visibility::default(), view))
                    .id();
                models::spawn(&mut commands, &assets, entity, &view, &model.file);
                continue;
            }
            let mesh = meshes.add(Cuboid::new(3.0, 3.8, v.spec.length as f32));
            commands.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(if v.is_powered() {
                    body.clone()
                } else {
                    coach.clone()
                }),
                Transform::default(),
                view,
            ));
        }
    }

    // Sun and sky light.
    commands.spawn((
        DirectionalLight {
            illuminance: 20_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(200.0, 400.0, 200.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Camera3d::default(),
        AmbientLight {
            color: Color::srgb(0.7, 0.8, 1.0),
            brightness: 250.0,
            ..default()
        },
        Projection::Perspective(PerspectiveProjection {
            far: 20_000.0,
            ..default()
        }),
        Transform::default(),
        ui::CabCamera,
    ));

    ui::spawn_hud(&mut commands);
    mods_ui::spawn_panel(&mut commands);

    // `--camera outside` starts on the external camera — handy for screenshots of a
    // vehicle model.
    if arg("--camera").as_deref() == Some("outside") {
        commands.insert_resource(ui::CameraState {
            mode: ui::CameraMode::Outside,
            distance: 40.0,
            pitch: -0.15,
            ..default()
        });
    }

    commands.insert_resource(TerrainInfo::default());
    commands.insert_resource(streamer);
    commands.insert_resource(ViewDistance(LOAD_RADIUS as f32));
    commands.insert_resource(Origin(origin));
    commands.insert_resource(PlayerTrain(player));
    commands.insert_resource(AiDrivers(drivers));
    commands.insert_resource(Mods(mods));
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

/// Asset source for `mods://` — the `mods/` directory next to the game, the same one the
/// mod runtime reads.
fn mod_asset_source() -> AssetSourceBuilder {
    let root = std::env::current_dir().unwrap_or_default().join("mods");
    AssetSourceBuilder::new(move || Box::new(FileAssetReader::new(root.clone())))
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
