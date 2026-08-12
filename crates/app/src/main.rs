//! TrainSim-DE — Bevy app: rendering, camera, input, HUD (plan ch. 12).
//!
//! The app ticks `sim-core` with a fixed time step and mirrors the state into ECS components.
//! Simulation logic does **not** belong here.

mod render;
mod ui;

use ai_driver::{AiDriver, ScheduledStop, Timetable};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use content::import::dgm::TerrainSource;
use content::terrain::{TerrainOptions, TerrainStats};
use content::vehicles::{br101, de_pzb, de_pzb_lzb, passenger_coach, vehicle};
use content::{musterbahn, re_4711, to_musterstadt};
use render::{Origin, TerrainChunk, VehicleView, WorldAnchored};
use sim_core::Sim;
use sim_core::safety::SafetySystems;
use sim_core::safety::de::TrainType;
use sim_core::train::Train;
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

/// Terrain view distance [m] — tiles beyond it are hidden.
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
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "TrainSim-DE".into(),
            ..default()
        }),
        ..default()
    }))
    .insert_resource(ClearColor(Color::srgb(0.55, 0.68, 0.82)))
    .add_systems(Startup, setup)
    .add_systems(
        Update,
        (
            ui::player_input,
            drive_ai,
            step_simulation,
            rebase_origin,
            sync_vehicles,
            ui::camera_control,
            terrain_visibility,
            ui::update_hud,
        )
            .chain(),
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
) {
    // Build line and simulation.
    let line = musterbahn().compile().expect("example line compiles");
    let mut sim = Sim::new(line.net, line.interlock, 2024);

    let player = spawn_train(&mut sim, TrackPosition::new(EdgeId(0), 200.0, 1), 5, true);
    let ai_train = spawn_train(&mut sim, TrackPosition::new(EdgeId(1), 400.0, 1), 3, false);

    let ai = AiDriver::new(Timetable {
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
    });

    // Load the scenario with timetable and scoring (plan 11.4).
    let mut scenario = to_musterstadt();
    scenario.player_train = player;
    sim.set_scenario(scenario, re_4711());

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
    let mut source = std::env::args()
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
    let (tiles, stats) = content::terrain::build(&sim.net, source.as_mut(), &terrain_options);
    info!(
        "Terrain: {} tiles, {} triangles, {:.1} MB",
        stats.tiles,
        stats.triangles,
        stats.memory() as f64 / 1e6
    );
    render::spawn_terrain(&mut commands, &mut meshes, &mut materials, &tiles, &origin);

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
    for train in [player, ai_train] {
        for (i, v) in sim.trains[train].vehicles.iter().enumerate() {
            let mesh = meshes.add(Cuboid::new(3.0, 3.8, v.spec.length as f32));
            commands.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(if v.is_powered() {
                    body.clone()
                } else {
                    coach.clone()
                }),
                Transform::default(),
                VehicleView { train, vehicle: i },
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

    commands.insert_resource(TerrainInfo(stats));
    commands.insert_resource(ViewDistance(6_000.0));
    commands.insert_resource(Origin(origin));
    commands.insert_resource(PlayerTrain(player));
    commands.insert_resource(AiDrivers(vec![(ai_train, ai)]));
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

/// Places a train of BR 101 + coaches at the given position.
fn spawn_train(sim: &mut Sim, head: TrackPosition, coaches: usize, with_lzb: bool) -> usize {
    let safety = if with_lzb {
        de_pzb_lzb(TrainType::O)
    } else {
        de_pzb(TrainType::O)
    };
    let mut vehicles = vec![vehicle(br101(), head, safety)];
    for _ in 0..coaches {
        vehicles.push(vehicle(passenger_coach(), head, SafetySystems::None));
    }
    let train = Train::assemble(vehicles, head, &sim.net);
    let index = sim.add_train(train);
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
