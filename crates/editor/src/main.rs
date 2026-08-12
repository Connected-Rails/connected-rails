//! TrainSim-DE Editor — Draufsicht auf eine Strecke mit Luftbild-Overlay.
//!
//! ```text
//! train-sim-editor [strecke.ron] [--imagery <konfig.ron>] [--frames N]
//! ```
//!
//! Ohne Streckendatei wird die Beispielstrecke geladen. Die Overlay-Konfiguration wird
//! beim ersten Start angelegt und lässt sich im Betrieb neu laden (F5).

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

/// Geografische Lage eines Weltpunkts in **Grad** — `geo::from_ecef` liefert Bogenmaß,
/// und Kachelraster wie Anzeige rechnen in Grad.
pub fn focus_degrees(position: EcefPos) -> (f64, f64) {
    let (lat, lon, _) = geo::from_ecef(position);
    (lat.to_degrees(), lon.to_degrees())
}

/// Renderursprung (Floating Origin).
#[derive(Resource)]
pub struct Origin(pub RenderOrigin);

/// Blickpunkt des Editors in Weltkoordinaten.
#[derive(Resource)]
pub struct Focus {
    pub position: EcefPos,
    /// Kamerahöhe über dem Blickpunkt [m].
    pub height: f64,
}

/// Das geladene Gleisnetz.
#[derive(Resource)]
pub struct Line {
    pub net: TrackNetwork,
    pub name: String,
    pub path: Option<String>,
}

/// Weltverankerte Objekte (Gleis) — beim Rebase nachzuführen.
#[derive(Component)]
struct WorldAnchored {
    anchor: EcefPos,
}

/// Pfad der Overlay-Konfiguration.
#[derive(Resource)]
struct ConfigPath(String);

#[derive(Resource)]
struct FrameLimit(u32);

/// Zieldatei aus `--screenshot <datei.png>`.
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
    // Ohne `--frames` reicht für ein Bild etwa eine Sekunde Anlauf.
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
    // Strecke laden.
    let (source, name, path) = match &line_path.0 {
        Some(path) => match std::fs::read_to_string(path).map(|t| LineSource::from_ron(&t)) {
            Ok(Ok(source)) => {
                let name = source.name.clone();
                (source, name, Some(path.clone()))
            }
            _ => {
                warn!("{path} nicht lesbar — Beispielstrecke geladen");
                (content::musterbahn(), "Musterbahn".into(), None)
            }
        },
        None => (content::musterbahn(), "Musterbahn".into(), None),
    };
    let compiled = source.compile().expect("Strecke übersetzbar");
    let net = compiled.net;

    // Blickpunkt in die Streckenmitte.
    let first = net.edges()[0].eval(0.0).pos;
    let middle = &net.edges()[net.edges().len() / 2];
    let focus_position = middle.eval(middle.length() / 2.0).pos;
    let base_height = geo::from_ecef(focus_position).2;
    let origin = RenderOrigin::new(focus_position);
    let _ = first;

    spawn_track(&mut commands, &mut meshes, &mut materials, &net, &origin);

    // Overlay-Konfiguration laden (oder anlegen).
    let (config, message) = ImageryConfig::load_or_create(&config_path.0);
    if let Some(message) = &message {
        info!("{message}");
    }
    info!(
        "Luftbild: {} ({} Anbieter)",
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

/// Gleisband als dunkle Fläche — Bezug für die Lage im Luftbild.
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

/// Blickpunkt verschieben und Höhe ändern.
fn camera_control(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    origin: Res<Origin>,
    mut focus: ResMut<Focus>,
    mut camera: Query<&mut Transform, With<Camera3d>>,
) {
    let dt = time.delta_secs_f64();
    // Bewegung skaliert mit der Höhe: weit oben wird großzügig geschoben.
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

/// Alle Overlay-Einstellungen über Tasten.
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
    // Zoomstufe: aus der aktuellen Stufe eine feste machen und verschieben.
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
    // Bildversatz gegen die Karte (Luftbilder liegen oft um Meter daneben).
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
        overlay.status = "Cache geleert".into();
    }
    if keys.just_pressed(KeyCode::KeyR) {
        overlay.source.retry_failed();
        overlay.status = "Fehlversuche zurückgesetzt".into();
    }
    if keys.just_pressed(KeyCode::F5) {
        let (loaded, message) = ImageryConfig::load_or_create(&config_path.0);
        overlay.status = message.unwrap_or_else(|| format!("{} geladen", config_path.0));
        overlay.apply(&mut commands, loaded);
        return;
    }
    if keys.just_pressed(KeyCode::F2) {
        overlay.status = match config.save(&config_path.0) {
            Ok(()) => format!("{} gespeichert", config_path.0),
            Err(e) => format!("Speichern fehlgeschlagen: {e}"),
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

/// Origin nachführen und weltverankerte Objekte neu ausrichten.
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
            "Strecke: {} ({} Kanten{})",
            line.name,
            line.net.edges().len(),
            line.path
                .as_ref()
                .map(|p| format!(", {p}"))
                .unwrap_or_default()
        ),
        format!(
            "Blickpunkt {lat:.5}°, {lon:.5}°   Höhe {:.0} m",
            focus.height
        ),
        format!(
            "Luftbild: {}   {}   Deckkraft {:.0} %   Zoom {} ({})",
            provider.map(|p| p.name.as_str()).unwrap_or("—"),
            if config.enabled { "ein" } else { "aus" },
            config.opacity * 100.0,
            overlay.zoom,
            match config.zoom {
                ZoomMode::Fixed(_) => "fest".to_string(),
                ZoomMode::Resolution(m) => format!("{m:.2} m/Px"),
            }
        ),
        format!(
            "Kacheln: {} sichtbar, {} unterwegs   Versatz {:+.1}/{:+.1} m{}",
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
            "Cache: {} Treffer ({} Platte), {} geladen, {} verworfen, {:.1} MB in {}",
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
        lines.push(format!("Fehler: {error}"));
    }
    lines.push(String::new());
    lines.push(
        "WASD/Pfeile verschieben   Bild↑/↓ Höhe   O Overlay   P Anbieter   [ ] Deckkraft".into(),
    );
    lines.push(
        ", . Zoomstufe   Z automatisch   Ziffernblock 4/6/8/2 Versatz, 5 zurücksetzen".into(),
    );
    lines.push(
        "L Offlinebetrieb   C Cache leeren   R Fehler zurücksetzen   F5 laden   F2 sichern".into(),
    );

    **text = lines.join("\n");
}

/// Beendet den Editor nach der vorgegebenen Framezahl — mit `--screenshot` wird davor
/// das Fenster aufgenommen.
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
    // Die Aufnahme läuft über den Render-Thread: erst ein paar Frames später liegt sie auf der Platte.
    if *count >= limit.0 + 10 {
        exit.write(AppExit::Success);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Die Einheit an dieser Nahtstelle war schon einmal falsch: `geo::from_ecef` liefert
    /// Bogenmaß, das Kachelraster erwartet Grad — die Kacheln landeten damit tausende
    /// Kilometer daneben, mitten im Atlantik.
    #[test]
    fn blickpunkt_kommt_in_grad() {
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
    fn beispielstrecke_liegt_im_blickfeld() {
        let compiled = content::musterbahn().compile().expect("übersetzbar");
        assert!(!compiled.net.edges().is_empty());
        let middle = &compiled.net.edges()[compiled.net.edges().len() / 2];
        let (lat, lon) = focus_degrees(middle.eval(middle.length() / 2.0).pos);
        assert!((51.0..53.0).contains(&lat) && (9.0..11.0).contains(&lon));
    }
}
