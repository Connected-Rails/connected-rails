//! TrainSim-DE route editor — top-down view of a line with aerial imagery overlay,
//! track drawing and device placement (plan ch. 15, editor v1).
//!
//! ```text
//! trainsim-route-editor [line.ron] [--imagery <config.ron>] [--frames N]
//! ```
//!
//! Without a line file the example line is loaded. The overlay configuration is created
//! on first start and can be reloaded at runtime (F5).

mod overlay;
mod tools;
mod ui;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy_egui::{EguiPlugin, EguiPrimaryContextPass, PrimaryEguiContext};
use content::LineSource;
use glam::DVec3;
use i18n::t;
use imagery::{ImageryConfig, ZoomMode};
use overlay::{Overlay, OverlayTile};
use tools::EditorState;
use track_model::{DeviceKind, TrackNetwork};
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

/// The document: the line in source form, its compilation, and the save state.
#[derive(Resource)]
pub struct Line {
    pub source: LineSource,
    pub net: TrackNetwork,
    pub path: Option<String>,
    pub dirty: bool,
    /// Recompile the source and respawn track and markers next frame.
    pub needs_rebuild: bool,
    /// Move the view to the line's middle on the next rebuild (after Open).
    pub recenter: bool,
}

const UNDO_DEPTH: usize = 64;

/// Undo history of the document — one snapshot of the source per interaction.
#[derive(Resource)]
pub struct History {
    pub undo: Vec<LineSource>,
    pub redo: Vec<LineSource>,
    /// The source as it looked after the last frame — the diff detector.
    last: LineSource,
    /// Whether the source changed in the previous frame, too: a drag mutates
    /// it every frame and must still cost one step.
    changing: bool,
}

impl History {
    pub fn new(source: LineSource) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            last: source,
            changing: false,
        }
    }

    /// A new document (Open, New) starts its own history.
    pub fn reset(&mut self, source: &LineSource) {
        self.undo.clear();
        self.redo.clear();
        self.last = source.clone();
        self.changing = false;
    }

    pub fn undo(&mut self, line: &mut Line) {
        if let Some(state) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut line.source, state));
            self.last = line.source.clone();
            self.changing = false;
            line.dirty = true;
            line.needs_rebuild = true;
        }
    }

    pub fn redo(&mut self, line: &mut Line) {
        if let Some(state) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut line.source, state));
            self.last = line.source.clone();
            self.changing = false;
            line.dirty = true;
            line.needs_rebuild = true;
        }
    }
}

/// World-anchored objects (track, device markers) — re-aligned on a rebase.
#[derive(Component)]
struct WorldAnchored {
    anchor: EcefPos,
}

/// Device marker quad — scaled with the view height so it stays clickable.
#[derive(Component)]
struct DeviceMarker;

/// Path of the overlay configuration.
#[derive(Resource)]
struct ConfigPath(String);

#[derive(Resource)]
struct FrameLimit(u32);

/// Target file from `--screenshot <file.png>`.
#[derive(Resource)]
struct ShotPath(String);

/// What the menu and the panel have asked for — applied by `overlay_control`
/// through the same code paths as the keyboard shortcuts.
#[derive(Resource, Default)]
pub struct Request {
    pub toggle_overlay: bool,
    pub cycle_provider: bool,
    pub toggle_offline: bool,
    pub clear_cache: bool,
    pub retry_failed: bool,
    pub load_config: bool,
    pub save_config: bool,
    /// Configuration edited in the panel; the flag says whether the tiles
    /// must be rebuilt (provider, opacity or offset changed).
    pub config: Option<(ImageryConfig, bool)>,
}

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
            title: t!("window-route-editor"),
            ..default()
        }),
        // The close button must pass through `confirm_close`, or it is the one
        // route that still throws unsaved work away.
        close_when_requested: false,
        ..default()
    }))
    .add_plugins(EguiPlugin::default())
    // The UI belongs on our own camera. Left to itself, `bevy_egui` creates a context on a
    // camera without a render graph — depending on which startup system runs first, and the
    // panels then stay invisible.
    .insert_resource(bevy_egui::EguiGlobalSettings {
        auto_create_primary_context: false,
        ..default()
    })
    .insert_resource(ClearColor(Color::srgb(0.08, 0.09, 0.11)))
    .insert_resource(ConfigPath(config_path))
    .insert_resource(LinePath(line_path))
    .init_resource::<Request>()
    .init_resource::<EditorState>()
    .add_systems(Startup, setup)
    .add_systems(EguiPrimaryContextPass, ui::draw)
    .add_systems(
        Update,
        (
            camera_control,
            tools::tool_input,
            track_changes,
            rebuild,
            overlay_control,
            overlay::update,
            rebase_origin,
            tools::draw_gizmos,
            scale_markers,
            update_title,
            confirm_close,
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

fn setup(mut commands: Commands, config_path: Res<ConfigPath>, line_path: Res<LinePath>) {
    // Load the line.
    let (source, path) = match &line_path.0 {
        Some(path) => match std::fs::read_to_string(path).map(|t| LineSource::from_ron(&t)) {
            Ok(Ok(source)) => (source, Some(path.clone())),
            _ => {
                warn!("{path} not readable — example line loaded");
                (content::musterbahn(), None)
            }
        },
        None => (content::musterbahn(), None),
    };
    let net = match source.compile() {
        Ok(compiled) => compiled.net,
        Err(e) => {
            warn!("line does not compile ({e:?}) — example line loaded");
            content::musterbahn().compile().expect("compiles").net
        }
    };

    // View point at the middle of the line; an empty line starts over the example area.
    let focus_position = match net.edges().get(net.edges().len() / 2) {
        Some(middle) => middle.eval(middle.length() / 2.0).pos,
        None => geo::to_ecef_deg(52.0, 10.0, 146.0),
    };
    let base_height = geo::from_ecef(focus_position).2;
    let origin = RenderOrigin::new(focus_position);

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
        PrimaryEguiContext,
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 8_000.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(100.0, 400.0, 100.0).looking_at(Vec3::ZERO, Vec3::Y),
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
    commands.insert_resource(History::new(source.clone()));
    // Track and markers are spawned by `rebuild` on the first frame.
    commands.insert_resource(Line {
        source,
        net,
        path,
        dirty: false,
        needs_rebuild: true,
        recenter: false,
    });
}

/// One undo step per interaction: whoever mutated `Line::source` this frame —
/// a tool click or a panel drag — is picked up here by comparison, so the
/// mutation sites stay plain writes.
fn track_changes(mut line: ResMut<Line>, mut history: ResMut<History>) {
    record_change(&mut line, &mut history);
}

fn record_change(line: &mut Line, history: &mut History) {
    if line.source == history.last {
        history.changing = false;
        return;
    }
    if !history.changing {
        if history.undo.len() == UNDO_DEPTH {
            history.undo.remove(0);
        }
        let last = history.last.clone();
        history.undo.push(last);
        history.redo.clear();
    }
    history.changing = true;
    history.last = line.source.clone();
    line.dirty = true;
    line.needs_rebuild = true;
}

/// Recompiles the source and respawns track and markers after every edit.
#[allow(clippy::too_many_arguments)]
fn rebuild(
    mut line: ResMut<Line>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut origin: ResMut<Origin>,
    mut focus: ResMut<Focus>,
    mut overlay: ResMut<Overlay>,
    old: Query<Entity, (With<WorldAnchored>, Without<OverlayTile>)>,
) {
    if !line.needs_rebuild {
        return;
    }
    line.needs_rebuild = false;
    match line.source.compile() {
        Ok(compiled) => line.net = compiled.net,
        Err(e) => {
            overlay.status = t!("status-compile-error", error = format!("{e:?}"));
            return;
        }
    }
    for entity in old.iter() {
        commands.entity(entity).despawn();
    }
    if line.recenter {
        line.recenter = false;
        let edges = line.net.edges();
        if let Some(middle) = edges.get(edges.len() / 2) {
            focus.position = middle.eval(middle.length() / 2.0).pos;
            origin.0 = RenderOrigin::new(focus.position);
            overlay.clear(&mut commands);
        }
    }
    spawn_track(
        &mut commands,
        &mut meshes,
        &mut materials,
        &line.net,
        &origin.0,
    );
    spawn_markers(
        &mut commands,
        &mut meshes,
        &mut materials,
        &line.source,
        &line.net,
        &origin.0,
    );
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
            // Counter-clockwise seen from above — a clockwise strip is a
            // backface to the top-down camera and is culled.
            indices.extend_from_slice(&[a, a + 1, a + 2, a + 2, a + 1, a + 3]);
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

/// One colored quad per trackside device, lifted above the ribbon.
fn spawn_markers(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    source: &LineSource,
    net: &TrackNetwork,
    origin: &RenderOrigin,
) {
    let mesh = meshes
        .add(Mesh::from(Plane3d::default().mesh().size(2.0, 2.0)).translated_by(Vec3::Y * 0.8));
    for device in &source.devices {
        let Some(pos) = tools::device_pos(net, device) else {
            continue;
        };
        let frame = EnuFrame::at(pos);
        let (translation, rotation) = origin.frame_transform(&frame);
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: marker_color(&device.kind),
                unlit: true,
                ..default()
            })),
            Transform::from_translation(translation).with_rotation(rotation),
            WorldAnchored { anchor: pos },
            DeviceMarker,
        ));
    }
}

/// Marker colors per device kind — legend colors, not simulation state.
fn marker_color(kind: &DeviceKind) -> Color {
    match kind {
        DeviceKind::Signal => Color::srgb(0.90, 0.22, 0.22),
        DeviceKind::Magnet => Color::srgb(0.95, 0.60, 0.12),
        DeviceKind::LineConductor => Color::srgb(0.65, 0.40, 0.95),
        DeviceKind::Balise => Color::srgb(0.20, 0.80, 0.85),
        DeviceKind::SpeedBoard => Color::srgb(0.95, 0.85, 0.25),
        DeviceKind::Platform => Color::srgb(0.30, 0.55, 0.95),
        DeviceKind::StopBoard => Color::srgb(0.92, 0.92, 0.92),
        DeviceKind::BlockMarker => Color::srgb(0.25, 0.80, 0.55),
        DeviceKind::NeutralSection => Color::srgb(0.85, 0.30, 0.75),
        DeviceKind::Other(_) => Color::srgb(0.60, 0.62, 0.68),
    }
}

/// Keeps device markers a few pixels big at any height.
fn scale_markers(focus: Res<Focus>, mut markers: Query<&mut Transform, With<DeviceMarker>>) {
    let scale = ((focus.height / 250.0) as f32).clamp(1.0, 40.0);
    for mut transform in markers.iter_mut() {
        transform.scale = Vec3::splat(scale);
    }
}

/// Move the view point and change the height — keyboard as before, plus the
/// map conventions every modder expects: wheel zooms, middle button drags.
#[allow(clippy::too_many_arguments)]
fn camera_control(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    mut motion: MessageReader<bevy::input::mouse::MouseMotion>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    time: Res<Time>,
    origin: Res<Origin>,
    mut state: ResMut<EditorState>,
    mut focus: ResMut<Focus>,
    mut camera: Query<&mut Transform, With<Camera3d>>,
) {
    let dt = time.delta_secs_f64();
    let frame = EnuFrame::at(focus.position);
    // While a text field has focus, WASD is typing, not panning.
    if !state.typing {
        // Movement scales with the height: far up, panning is generous.
        let speed = focus.height * 0.8 * dt;
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
            state.map_used = true;
        }
        if keys.pressed(KeyCode::PageUp) || keys.pressed(KeyCode::NumpadSubtract) {
            focus.height = (focus.height * (1.0 + dt)).min(20_000.0);
        }
        if keys.pressed(KeyCode::PageDown) || keys.pressed(KeyCode::NumpadAdd) {
            focus.height = (focus.height * (1.0 - dt)).max(60.0);
        }
    }

    // Mouse input only inside the viewport rect the panels leave free — the
    // hand-built panel layout is invisible to egui's own hit test, so the
    // check is ours (see `EditorState::viewport`).
    let over_map = windows
        .single()
        .ok()
        .and_then(|w| w.cursor_position())
        .is_some_and(|p| state.viewport.contains(p));
    let scroll: f32 = wheel.read().map(|w| w.y).sum();
    if over_map && scroll != 0.0 {
        focus.height = (focus.height * (1.0 - scroll as f64 * 0.15)).clamp(60.0, 20_000.0);
        state.map_used = true;
    }
    let drag: Vec2 = motion.read().map(|m| m.delta).sum();
    if over_map && buttons.pressed(MouseButton::Middle) && drag != Vec2::ZERO {
        // Metres per pixel on the focus plane (45° vertical fov), so the map
        // sticks to the cursor while it is dragged.
        let metres_per_px = focus.height * 2.0 * (std::f64::consts::FRAC_PI_8).tan()
            / (state.viewport.height().max(1.0) as f64);
        let shift = frame.east * (drag.x as f64 * metres_per_px)
            - frame.north * (drag.y as f64 * metres_per_px);
        focus.position = EcefPos(focus.position.0 - shift);
        state.map_used = true;
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
    mut request: ResMut<Request>,
    config_path: Res<ConfigPath>,
    state: Res<EditorState>,
    focus: Res<Focus>,
) {
    let mut config = overlay.config().clone();
    let mut changed = false;
    let mut rebuild = false;

    // Menu entries and panel widgets take the same paths as the keys.
    let menu = std::mem::take(&mut *request);
    if let Some((edited, rebuild_tiles)) = menu.config {
        config = edited;
        changed = true;
        rebuild |= rebuild_tiles;
    }
    if menu.toggle_overlay {
        config.enabled = !config.enabled;
        changed = true;
    }
    if menu.cycle_provider {
        config.cycle_provider();
        changed = true;
        rebuild = true;
    }
    if menu.toggle_offline {
        config.cache.offline = !config.cache.offline;
        changed = true;
    }
    if menu.clear_cache {
        overlay.source.clear_cache();
        overlay.clear(&mut commands);
        overlay.status = t!("status-cache-cleared");
    }
    if menu.retry_failed {
        overlay.source.retry_failed();
        overlay.status = t!("status-retry-reset");
    }
    if menu.save_config {
        overlay.status = match config.save(&config_path.0) {
            Ok(()) => t!("status-saved", file = config_path.0),
            Err(e) => t!("status-save-failed", error = e),
        };
    }
    if menu.load_config {
        let (loaded, message) = ImageryConfig::load_or_create(&config_path.0);
        overlay.status = message.unwrap_or_else(|| t!("status-loaded", file = config_path.0));
        overlay.apply(&mut commands, loaded);
        return;
    }

    // The letter keys are overlay shortcuts only while no text field is typed in.
    if state.typing {
        if changed {
            if rebuild {
                overlay.apply(&mut commands, config);
            } else {
                overlay.source.set_config(config);
            }
        }
        return;
    }

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
        overlay.status = t!("status-cache-cleared");
    }
    if keys.just_pressed(KeyCode::KeyR) {
        overlay.source.retry_failed();
        overlay.status = t!("status-retry-reset");
    }
    if keys.just_pressed(KeyCode::F5) {
        let (loaded, message) = ImageryConfig::load_or_create(&config_path.0);
        overlay.status = message.unwrap_or_else(|| t!("status-loaded", file = config_path.0));
        overlay.apply(&mut commands, loaded);
        return;
    }
    if keys.just_pressed(KeyCode::F2) {
        overlay.status = match config.save(&config_path.0) {
            Ok(()) => t!("status-saved", file = config_path.0),
            Err(e) => t!("status-save-failed", error = e),
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

/// The window title names the document, plus the unsaved marker.
fn update_title(
    line: Res<Line>,
    mut windows: Query<&mut Window, With<bevy::window::PrimaryWindow>>,
) {
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    let key = if line.dirty {
        "window-route-editor-unsaved"
    } else {
        "window-route-editor-named"
    };
    let title = t!(key, name = line.source.name);
    if window.title != title {
        window.title = title;
    }
}

/// The window's close button goes through the discard guard like Quit does.
fn confirm_close(
    mut requests: MessageReader<bevy::window::WindowCloseRequested>,
    mut line: ResMut<Line>,
    mut state: ResMut<EditorState>,
    mut overlay: ResMut<Overlay>,
    mut exit: MessageWriter<AppExit>,
) {
    if requests.read().next().is_none() {
        return;
    }
    requests.clear();
    if ui::confirm_discard(&mut line, &mut state, &mut overlay) {
        exit.write(AppExit::Success);
    }
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

    /// The undo history records one step per mutation and survives a round trip.
    #[test]
    fn undo_round_trips_an_edit() {
        let source = content::musterbahn();
        let mut line = Line {
            source: source.clone(),
            net: source.compile().unwrap().net,
            path: None,
            dirty: false,
            needs_rebuild: false,
            recenter: false,
        };
        let mut history = History::new(source.clone());

        // An edit, picked up by the diff detector.
        line.source.devices.pop();
        record_change(&mut line, &mut history);
        assert!(line.dirty && history.undo.len() == 1);

        history.undo(&mut line);
        assert_eq!(line.source, source);
        history.redo(&mut line);
        assert_eq!(line.source.devices.len(), source.devices.len() - 1);
    }
}
