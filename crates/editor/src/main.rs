//! TrainSim-DE editor — top-down view of a line with aerial imagery overlay.
//!
//! ```text
//! train-sim-editor [line.ron] [--imagery <config.ron>] [--frames N]
//! ```
//!
//! Without a line file the example line is loaded. The overlay configuration is created
//! on first start and can be reloaded at runtime (F5).

mod overlay;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use content::LineSource;
use glam::DVec3;
use imagery::{ImageryConfig, ZoomMode};
use overlay::{Overlay, OverlayTile};
use track_model::TrackNetwork;
use world_coords::{EcefPos, EnuFrame, RenderOrigin, geo};

/// Geographic position of a world point in **degrees** — `geo::from_ecef` returns radians,
/// while both the tile grid and the display work in degrees.
pub fn focus_degrees(position: EcefPos) -> (f64, f64) {
    let (lat, lon, _) = geo::from_ecef(position);
    (lat.to_degrees(), lon.to_degrees())
}

/// Render origin (floating origin).
#[derive(Resource)]
pub struct Origin(pub RenderOrigin);

/// View point of the editor in world coordinates.
#[derive(Resource)]
pub struct Focus {
    pub position: EcefPos,
    /// Camera height above the view point [m].
    pub height: f64,
}

/// The loaded track network.
#[derive(Resource)]
pub struct Line {
    pub net: TrackNetwork,
    pub name: String,
    pub path: Option<String>,
}

/// World-anchored objects (track) — to be followed up on a rebase.
#[derive(Component)]
struct WorldAnchored {
    anchor: EcefPos,
}

/// Path of the overlay configuration.
#[derive(Resource)]
struct ConfigPath(String);

#[derive(Resource)]
struct FrameLimit(u32);

/// Target file from `--screenshot <file.png>`.
#[derive(Resource)]
struct ShotPath(String);

#[derive(Component)]
struct HudText;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let line_path = args.first().filter(|a| !a.starts_with("--")).cloned();
    let config_path = flag("--imagery").unwrap_or_else(|| "imagery.ron".into());
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
            title: "TrainSim-DE Editor".into(),
            ..default()
        }),
        ..default()
    }))
    .insert_resource(ClearColor(Color::srgb(0.08, 0.09, 0.11)))
    .insert_resource(ConfigPath(config_path))
    .insert_resource(LinePath(line_path))
    .add_systems(Startup, setup)
    .add_systems(
        Update,
        (
            camera_control,
            overlay_control,
            overlay::update,
            rebase_origin,
            update_hud,
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

#[derive(Resource)]
struct LinePath(Option<String>);

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    config_path: Res<ConfigPath>,
    line_path: Res<LinePath>,
) {
    // Load the line.
    let (source, name, path) = match &line_path.0 {
        Some(path) => match std::fs::read_to_string(path).map(|t| LineSource::from_ron(&t)) {
            Ok(Ok(source)) => {
                let name = source.name.clone();
                (source, name, Some(path.clone()))
            }
            _ => {
                warn!("{path} not readable — example line loaded");
                (content::musterbahn(), "Musterbahn".into(), None)
            }
        },
        None => (content::musterbahn(), "Musterbahn".into(), None),
    };
    let compiled = source.compile().expect("line compiles");
    let net = compiled.net;

    // View point at the middle of the line.
    let first = net.edges()[0].eval(0.0).pos;
    let middle = &net.edges()[net.edges().len() / 2];
    let focus_position = middle.eval(middle.length() / 2.0).pos;
    let base_height = geo::from_ecef(focus_position).2;
    let origin = RenderOrigin::new(focus_position);
    let _ = first;

    spawn_track(&mut commands, &mut meshes, &mut materials, &net, &origin);

    // Load (or create) the overlay configuration.
    let (config, message) = ImageryConfig::load_or_create(&config_path.0);
    if let Some(message) = &message {
        info!("{message}");
    }
    info!(
        "Aerial imagery: {} ({} providers)",
        config
            .provider()
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "—".into()),
        config.providers.len()
    );

    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            far: 60_000.0,
            ..default()
        }),
        Transform::default(),
        AmbientLight {
            color: Color::WHITE,
            brightness: 800.0,
            ..default()
        },
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 8_000.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(100.0, 400.0, 100.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    commands.spawn((
        Text::new(""),
        TextFont {
            font_size: bevy::text::FontSize::Px(15.0),
            ..default()
        },
        TextColor(Color::WHITE),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(10.0),
            left: Val::Px(10.0),
            ..default()
        },
        HudText,
    ));

    commands.insert_resource(Overlay::new(
        config,
        base_height,
        message.unwrap_or_default(),
    ));
    commands.insert_resource(Focus {
        position: focus_position,
        height: 900.0,
    });
    commands.insert_resource(Origin(origin));
    commands.insert_resource(Line { net, name, path });
}

/// Track ribbon as a dark quad — reference for the position in the aerial imagery.
fn spawn_track(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    net: &TrackNetwork,
    origin: &RenderOrigin,
) {
    let material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.95, 0.35, 0.15),
        unlit: true,
        ..default()
    });

    for edge in net.edges() {
        let frame = EnuFrame::at(edge.anchor);
        let steps = ((edge.length() / 5.0).ceil() as usize).max(2);
        let mut positions = Vec::with_capacity((steps + 1) * 2);
        for i in 0..=steps {
            let s = edge.length() * i as f64 / steps as f64;
            let pose = edge.eval(s);
            let center = frame.to_local(pose.pos);
            let tangent = frame.dir_to_local(pose.tangent);
            let up = frame.dir_to_local(pose.up);
            let right = tangent.cross(up).normalize_or_zero() * 1.5;
            for side in [-1.0, 1.0] {
                let p = center + right * side + DVec3::new(0.0, 0.0, 0.4);
                positions.push([p.x as f32, p.z as f32, -p.y as f32]);
            }
        }
        let mut indices = Vec::new();
        for row in 0..steps {
            let a = (row * 2) as u32;
            indices.extend_from_slice(&[a, a + 2, a + 1, a + 1, a + 2, a + 3]);
        }

        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        let count = positions.len();
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; count]);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32, 0.0]; count]);
        mesh.insert_indices(Indices::U32(indices));

        let (translation, rotation) = origin.frame_transform(&frame);
        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(translation).with_rotation(rotation),
            WorldAnchored {
                anchor: edge.anchor,
            },
        ));
    }
}

/// Move the view point and change the height.
fn camera_control(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    origin: Res<Origin>,
    mut focus: ResMut<Focus>,
    mut camera: Query<&mut Transform, With<Camera3d>>,
) {
    let dt = time.delta_secs_f64();
    // Movement scales with the height: far up, panning is generous.
    let speed = focus.height * 0.8 * dt;
    let frame = EnuFrame::at(focus.position);
    let mut shift = DVec3::ZERO;
    if keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp) {
        shift += frame.north * speed;
    }
    if keys.pressed(KeyCode::KeyS) || keys.pressed(KeyCode::ArrowDown) {
        shift -= frame.north * speed;
    }
    if keys.pressed(KeyCode::KeyA) || keys.pressed(KeyCode::ArrowLeft) {
        shift -= frame.east * speed;
    }
    if keys.pressed(KeyCode::KeyD) || keys.pressed(KeyCode::ArrowRight) {
        shift += frame.east * speed;
    }
    if shift != DVec3::ZERO {
        focus.position = EcefPos(focus.position.0 + shift);
    }
    if keys.pressed(KeyCode::PageUp) || keys.pressed(KeyCode::NumpadSubtract) {
        focus.height = (focus.height * (1.0 + dt)).min(20_000.0);
    }
    if keys.pressed(KeyCode::PageDown) || keys.pressed(KeyCode::NumpadAdd) {
        focus.height = (focus.height * (1.0 - dt)).max(60.0);
    }

    let Ok(mut transform) = camera.single_mut() else {
        return;
    };
    let frame = EnuFrame::at(focus.position);
    let center = origin.0.to_render(focus.position);
    let up = origin.0.dir_to_render(frame.up);
    let north = origin.0.dir_to_render(frame.north);
    transform.translation = center + up * focus.height as f32;
    transform.look_at(center, north);
}

/// All overlay settings via keys.
fn overlay_control(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut overlay: ResMut<Overlay>,
    config_path: Res<ConfigPath>,
    focus: Res<Focus>,
) {
    let mut config = overlay.config().clone();
    let mut changed = false;
    let mut rebuild = false;

    if keys.just_pressed(KeyCode::KeyO) {
        config.enabled = !config.enabled;
        changed = true;
    }
    if keys.just_pressed(KeyCode::KeyP) {
        config.cycle_provider();
        changed = true;
        rebuild = true;
    }
    if keys.just_pressed(KeyCode::BracketLeft) {
        config.opacity = (config.opacity - 0.1).max(0.0);
        changed = true;
        rebuild = true;
    }
    if keys.just_pressed(KeyCode::BracketRight) {
        config.opacity = (config.opacity + 0.1).min(1.0);
        changed = true;
        rebuild = true;
    }
    // Zoom level: turn the current level into a fixed one and shift it.
    let (lat, _) = focus_degrees(focus.position);
    if keys.just_pressed(KeyCode::Comma) {
        config.zoom = ZoomMode::Fixed(config.zoom_for(lat).saturating_sub(1));
        changed = true;
    }
    if keys.just_pressed(KeyCode::Period) {
        config.zoom = ZoomMode::Fixed(config.zoom_for(lat).saturating_add(1));
        changed = true;
    }
    if keys.just_pressed(KeyCode::KeyZ) {
        config.zoom = ZoomMode::Resolution(0.5);
        changed = true;
    }
    // Image offset against the map (aerial imagery is often off by metres).
    let step = if keys.pressed(KeyCode::ShiftLeft) {
        5.0
    } else {
        0.5
    };
    for (key, delta) in [
        (KeyCode::Numpad8, (0.0, step)),
        (KeyCode::Numpad2, (0.0, -step)),
        (KeyCode::Numpad6, (step, 0.0)),
        (KeyCode::Numpad4, (-step, 0.0)),
    ] {
        if keys.just_pressed(key) {
            config.offset.0 += delta.0;
            config.offset.1 += delta.1;
            changed = true;
            rebuild = true;
        }
    }
    if keys.just_pressed(KeyCode::Numpad5) {
        config.offset = (0.0, 0.0);
        changed = true;
        rebuild = true;
    }
    if keys.just_pressed(KeyCode::KeyL) {
        config.cache.offline = !config.cache.offline;
        changed = true;
    }

    if keys.just_pressed(KeyCode::KeyC) {
        overlay.source.clear_cache();
        overlay.clear(&mut commands);
        overlay.status = "Cache cleared".into();
    }
    if keys.just_pressed(KeyCode::KeyR) {
        overlay.source.retry_failed();
        overlay.status = "Failed attempts reset".into();
    }
    if keys.just_pressed(KeyCode::F5) {
        let (loaded, message) = ImageryConfig::load_or_create(&config_path.0);
        overlay.status = message.unwrap_or_else(|| format!("{} loaded", config_path.0));
        overlay.apply(&mut commands, loaded);
        return;
    }
    if keys.just_pressed(KeyCode::F2) {
        overlay.status = match config.save(&config_path.0) {
            Ok(()) => format!("{} saved", config_path.0),
            Err(e) => format!("Saving failed: {e}"),
        };
    }

    if changed {
        if rebuild {
            overlay.apply(&mut commands, config);
        } else {
            overlay.source.set_config(config);
        }
    }
}

/// Follow up the origin and re-align world-anchored objects.
fn rebase_origin(
    focus: Res<Focus>,
    mut origin: ResMut<Origin>,
    mut anchored: Query<(&WorldAnchored, &mut Transform), Without<OverlayTile>>,
    mut tiles: Query<(&OverlayTile, &mut Transform), Without<WorldAnchored>>,
) {
    if !origin.0.rebase_if_needed(focus.position) {
        return;
    }
    for (item, mut transform) in anchored.iter_mut() {
        let frame = EnuFrame::at(item.anchor);
        let (translation, rotation) = origin.0.frame_transform(&frame);
        transform.translation = translation;
        transform.rotation = rotation;
    }
    overlay::resync(&origin.0, &mut tiles);
}

fn update_hud(
    overlay: Res<Overlay>,
    focus: Res<Focus>,
    line: Res<Line>,
    mut query: Query<&mut Text, With<HudText>>,
) {
    let Ok(mut text) = query.single_mut() else {
        return;
    };
    let config = overlay.config();
    let provider = config.provider();
    let (lat, lon) = focus_degrees(focus.position);
    let stats = overlay.source.cache_stats();

    let mut lines = vec![
        format!(
            "Line: {} ({} edges{})",
            line.name,
            line.net.edges().len(),
            line.path
                .as_ref()
                .map(|p| format!(", {p}"))
                .unwrap_or_default()
        ),
        format!(
            "View point {lat:.5}°, {lon:.5}°   Height {:.0} m",
            focus.height
        ),
        format!(
            "Aerial imagery: {}   {}   Opacity {:.0} %   Zoom {} ({})",
            provider.map(|p| p.name.as_str()).unwrap_or("—"),
            if config.enabled { "on" } else { "off" },
            config.opacity * 100.0,
            overlay.zoom,
            match config.zoom {
                ZoomMode::Fixed(_) => "fixed".to_string(),
                ZoomMode::Resolution(m) => format!("{m:.2} m/px"),
            }
        ),
        format!(
            "Tiles: {} shown, {} in flight   Offset {:+.1}/{:+.1} m{}",
            overlay.tiles_shown(),
            overlay.source.pending(),
            config.offset.0,
            config.offset.1,
            if config.cache.offline {
                "   OFFLINE"
            } else {
                ""
            }
        ),
        format!(
            "Cache: {} hits ({} disk), {} stored, {} evicted, {:.1} MB in {}",
            stats.hits_memory + stats.hits_disk,
            stats.hits_disk,
            stats.stored,
            stats.evicted,
            overlay.source.disk_usage() as f64 / 1e6,
            config.cache.directory.display()
        ),
    ];
    if let Some(provider) = provider
        && !provider.attribution.is_empty()
    {
        lines.push(format!("© {}", provider.attribution));
    }
    if !overlay.status.is_empty() {
        lines.push(overlay.status.clone());
    }
    for error in overlay.source.errors.iter().rev().take(2) {
        lines.push(format!("Error: {error}"));
    }
    lines.push(String::new());
    lines.push("WASD/arrows pan   PgUp/PgDn height   O overlay   P provider   [ ] opacity".into());
    lines.push(", . zoom level   Z automatic   Numpad 4/6/8/2 offset, 5 reset".into());
    lines.push("L offline mode   C clear cache   R reset errors   F5 load   F2 save".into());

    **text = lines.join("\n");
}

/// Exits the editor after the given number of frames — with `--screenshot` the window is
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The unit at this seam was already wrong once: `geo::from_ecef` returns radians,
    /// the tile grid expects degrees — the tiles thus ended up thousands of kilometres
    /// off, in the middle of the Atlantic.
    #[test]
    fn view_point_comes_in_degrees() {
        let position = geo::to_ecef_deg(52.0006, 10.0509, 146.0);
        let (lat, lon) = focus_degrees(position);
        assert!((lat - 52.0006).abs() < 1e-6, "{lat}");
        assert!((lon - 10.0509).abs() < 1e-6, "{lon}");

        let tile = imagery::TileId::from_lat_lon(lat, lon, 18);
        let (west, south, east, north) = tile.bounds();
        assert!(west < 10.0509 && 10.0509 < east, "{west}…{east}");
        assert!(south < 52.0006 && 52.0006 < north, "{south}…{north}");
    }

    #[test]
    fn example_line_is_in_view() {
        let compiled = content::musterbahn().compile().expect("compiles");
        assert!(!compiled.net.edges().is_empty());
        let middle = &compiled.net.edges()[compiled.net.edges().len() / 2];
        let (lat, lon) = focus_degrees(middle.eval(middle.length() / 2.0).pos);
        assert!((51.0..53.0).contains(&lat) && (9.0..11.0).contains(&lon));
    }
}
