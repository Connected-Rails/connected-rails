//! TrainSim-DE signal editor — modular signal models: glTF parts on mount points,
//! lamp-image bindings, live preview (plan ch. 15.3, the Zusi assembly pattern).
//!
//! ```text
//! trainsim-signal-editor [signal_model.ron] [--frames N] [--screenshot file.png]
//! ```
//!
//! A desktop application like the vehicle editor: menu bar, docked panel, native
//! file dialogs. The viewport shows the assembled signal; the lamp test lights any
//! lamp image without starting the simulator.
//!
//! Parts live in a mod and are addressed relative to the `mods/` directory
//! (`<mod>/assets/<file>.gltf`) — exactly as the simulator loads them later.

mod ui;

use bevy::asset::io::AssetSourceBuilder;
use bevy::asset::io::file::FileAssetReader;
use bevy::gltf::Gltf;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy_egui::{EguiPlugin, EguiPrimaryContextPass, PrimaryEguiContext};
use i18n::t;
use serde::{Deserialize, Serialize};
use sim_core::interlock::{LampBinding, MotionBinding, SignalModel};
use sim_core::train::{Motion, lod_level};
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

/// Asset source of the mods: `mods://<mod>/assets/…` — the same one the simulator uses.
pub const MOD_SOURCE: &str = "mods";

/// The `mods/` directory next to the game.
pub fn mods_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_default().join("mods")
}

fn mod_asset_source() -> AssetSourceBuilder {
    let root = mods_dir();
    AssetSourceBuilder::new(move || Box::new(FileAssetReader::new(root.clone())))
}

/// What the status bar says, and whether it is bad news.
pub enum Status {
    Info(String),
    Error(String),
}

impl Status {
    pub fn text(&self) -> &str {
        match self {
            Status::Info(text) | Status::Error(text) => text,
        }
    }

    pub fn is_error(&self) -> bool {
        matches!(self, Status::Error(_))
    }
}

/// Loading state of one part's glTF in the preview.
#[derive(Default)]
pub struct PartState {
    /// File the handle belongs to — reloaded when the model's file changes.
    pub file: String,
    pub gltf: Option<Handle<Gltf>>,
    /// Node names of the loaded file, for the mount-point and lamp combos.
    pub nodes: Vec<String>,
    /// The preview instance of this part exists.
    pub spawned: bool,
}

/// Everything the editor is working on.
///
/// ponytail: no undo stack yet — confirm-on-discard guards the file, and a
/// signal model is a dozen lines; the vehicle editor's history moves in when
/// models grow past that.
#[derive(Resource)]
pub struct Editor {
    pub model: SignalModel,
    /// File the model came from, `None` for a new one.
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub status: Status,
    /// Per part: glTF handle, node names, preview state.
    pub parts: Vec<PartState>,
    /// Lamp-image strings lit in the preview.
    pub lit: BTreeSet<String>,
    /// Which level of detail the viewport shows (with a non-empty LOD table).
    pub preview_lod: u8,
    /// Bumped on any structural change — the preview is rebuilt from scratch.
    pub revision: u64,
    /// Handle of the editor window, the owner of every native dialog.
    pub window: Option<bevy::window::RawHandleWrapper>,
    pub settings: Settings,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            model: SignalModel::default(),
            path: None,
            dirty: false,
            status: Status::Info(t!("status-new-signal-model")),
            parts: Vec::new(),
            lit: BTreeSet::new(),
            preview_lod: 0,
            revision: 0,
            window: None,
            settings: Settings::load(),
        }
    }
}

impl Editor {
    /// Reads a signal model file.
    pub fn open(&mut self, path: PathBuf) {
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| {
                ron::from_str::<SignalModel>(&text)
                    .map_err(|e: ron::error::SpannedError| e.to_string())
            }) {
            Ok(model) => {
                self.status = Status::Info(t!("status-loaded", file = path.display()));
                self.model = model;
                self.path = Some(path);
                self.dirty = false;
                self.parts.clear();
                self.lit.clear();
                self.preview_lod = 0;
                self.revision += 1;
            }
            Err(e) => {
                self.status = Status::Error(t!("status-error", file = path.display(), error = e))
            }
        }
    }

    /// Writes the model back as RON.
    pub fn save(&mut self, path: PathBuf) {
        let text = ron::ser::to_string_pretty(&self.model, ron::ser::PrettyConfig::default())
            .expect("signal model is serializable");
        match std::fs::write(&path, text) {
            Ok(()) => {
                self.status = Status::Info(t!("status-written", file = path.display()));
                self.path = Some(path);
                self.dirty = false;
            }
            Err(e) => {
                self.status = Status::Error(t!("status-error", file = path.display(), error = e))
            }
        }
    }

    /// Name for the window title: the file stem, or the placeholder for a new model.
    pub fn display_name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| t!("heading-signal-model"))
    }

    /// Removes part `index` and keeps every reference consistent: mounts and lamp
    /// bindings on it are dropped, indices behind it move down.
    pub fn remove_part(&mut self, index: usize) {
        self.model.parts.remove(index);
        let idx = index as u32;
        for part in &mut self.model.parts {
            part.mount = match part.mount.take() {
                Some((p, _)) if p == idx => None,
                Some((p, node)) if p > idx => Some((p - 1, node)),
                keep => keep,
            };
        }
        self.model.lamps.retain(|l| l.part != idx);
        for lamp in &mut self.model.lamps {
            if lamp.part > idx {
                lamp.part -= 1;
            }
        }
        self.parts.clear();
        self.revision += 1;
        self.dirty = true;
    }

    /// Would mounting `child` on `parent` close a loop? Follows the mount chain
    /// upwards from `parent`; hitting `child` means the child would hang under
    /// itself.
    pub fn would_cycle(&self, child: usize, parent: usize) -> bool {
        let mut current = parent;
        for _ in 0..=self.model.parts.len() {
            if current == child {
                return true;
            }
            match self.model.parts.get(current).and_then(|p| p.mount.as_ref()) {
                Some((next, _)) => current = *next as usize,
                None => return false,
            }
        }
        true
    }
}

/// Persistent choices, next to the other editors' settings files.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Settings {
    /// Language picked under View; `None` follows the operating system.
    #[serde(default)]
    pub language: Option<String>,
    /// Size of the window as the user left it.
    #[serde(default)]
    pub window: Option<(f32, f32)>,
}

impl Settings {
    pub fn load() -> Self {
        settings_path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| ron::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// `TRAINSIM_LANG` wins: it is an explicit instruction for this one run.
    pub fn apply_language(&self) {
        if std::env::var_os("TRAINSIM_LANG").is_some() {
            return;
        }
        if let Some(language) = &self.language {
            i18n::set_language(language);
        }
    }

    pub fn set_language(&mut self, code: &str) {
        self.language = Some(code.to_owned());
        self.save();
    }

    /// Best effort — a settings file that cannot be written is no reason to
    /// interrupt the user.
    pub fn save(&self) {
        let Some(path) = settings_path() else {
            return;
        };
        let Ok(text) = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()) else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, text);
    }
}

/// `%APPDATA%\TrainSim-DE\` on Windows, `$XDG_CONFIG_HOME` or `~/.config` elsewhere.
fn settings_path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("TrainSim-DE").join("signal-editor.ron"))
}

/// Orbit camera around the signal.
#[derive(Resource)]
pub struct View {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub target: Vec3,
    /// Whether the user has moved the camera yet — the hint goes once they have.
    pub used: bool,
    /// The 3D viewport in logical pixels; the camera only listens inside it.
    pub viewport: Rect,
}

impl Default for View {
    fn default() -> Self {
        Self {
            yaw: 0.6,
            pitch: 0.15,
            distance: 12.0,
            target: Vec3::new(0.0, 2.5, 0.0),
            used: false,
            viewport: Rect::default(),
        }
    }
}

/// Root of one part's preview instance.
#[derive(Component)]
struct PreviewPart {
    part: usize,
}

/// The part still waits to be hung onto its mount node.
#[derive(Component)]
struct Unmounted;

/// Node whose visibility the lamp test has taken over — released (shown again)
/// when its binding goes, so editing a binding never leaves a node stuck hidden.
#[derive(Component)]
struct PreviewLamp;

/// Number of frames from `--frames N` (CI smoke test).
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
    let frame_limit = flag("--frames")
        .and_then(|n| n.parse::<u32>().ok())
        .or_else(|| shot.as_ref().map(|_| 60));

    // Language before the first status message goes through `t!`.
    Settings::load().apply_language();
    let mut editor = Editor::default();
    if let Some(file) = args.first().filter(|a| !a.starts_with("--")) {
        editor.open(PathBuf::from(file));
    }

    let mut app = App::new();
    // Parts come out of the mods, exactly as the simulator reads them later.
    // Has to be registered before the asset plugin.
    app.register_asset_source(MOD_SOURCE, mod_asset_source());
    let mut window = Window {
        title: t!("window-signal-editor"),
        resolution: bevy::window::WindowResolution::new(1280, 860),
        ..default()
    };
    if let Some((w, h)) = editor.settings.window {
        window.resolution = bevy::window::WindowResolution::new(w as u32, h as u32);
    }
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(window),
        // The close button has to ask about unsaved work; `confirm_close` owns it.
        close_when_requested: false,
        ..default()
    }))
    .add_plugins(EguiPlugin::default())
    // The UI belongs on our own camera (see the vehicle editor for the why).
    .insert_resource(bevy_egui::EguiGlobalSettings {
        auto_create_primary_context: false,
        ..default()
    })
    .insert_resource(ClearColor(Color::srgb(0.16, 0.17, 0.19)))
    .insert_resource(editor)
    .init_resource::<View>()
    .add_systems(Startup, setup)
    .add_systems(EguiPrimaryContextPass, ui::draw)
    .add_systems(
        Update,
        (
            sync_parts,
            mount_preview,
            preview_lamps,
            preview_motions,
            apply_preview_lod,
            orbit_camera,
            ground_grid,
            confirm_close,
            update_title,
            track_window_size,
        ),
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

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Camera3d::default(),
        // HDR + bloom, like the simulator: the lamp test glows the way the
        // night run will.
        bevy::camera::Hdr,
        bevy::post_process::bloom::Bloom::NATURAL,
        Projection::Perspective(PerspectiveProjection {
            far: 5_000.0,
            ..default()
        }),
        Transform::default(),
        PrimaryEguiContext,
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 12_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(30.0, 60.0, 30.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        AmbientLight {
            color: Color::srgb(0.8, 0.85, 1.0),
            brightness: 400.0,
            ..default()
        },
        Transform::default(),
    ));

    // Track stub as orientation: the signal stands beside it, facing +Z like a
    // driver approaching along -Z would see it.
    let rail = materials.add(StandardMaterial {
        base_color: Color::srgb(0.35, 0.36, 0.38),
        perceptual_roughness: 0.9,
        ..default()
    });
    let rail_mesh = meshes.add(Cuboid::new(0.07, 0.15, 40.0));
    for side in [-1.0, 1.0] {
        commands.spawn((
            Mesh3d(rail_mesh.clone()),
            MeshMaterial3d(rail.clone()),
            Transform::from_xyz(
                side * (sim_core::train::STANDARD_GAUGE as f32) / 2.0 - 3.0,
                -0.075,
                0.0,
            ),
        ));
    }
}

/// Keeps the per-part loading state in step with the model and (re)spawns the
/// preview: on a structural change everything is torn down and built afresh —
/// a signal is a handful of parts, rebuilding is cheaper than diffing.
fn sync_parts(
    mut commands: Commands,
    mut editor: ResMut<Editor>,
    assets: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    instances: Query<Entity, With<PreviewPart>>,
    mut last_revision: Local<Option<u64>>,
) {
    let editor = &mut *editor;
    if *last_revision != Some(editor.revision) {
        *last_revision = Some(editor.revision);
        for entity in instances.iter() {
            // A mounted part sits inside its parent's subtree and may already be
            // gone with it.
            commands.entity(entity).try_despawn();
        }
        for part in &mut editor.parts {
            part.spawned = false;
        }
    }

    editor
        .parts
        .resize_with(editor.model.parts.len(), PartState::default);
    for (i, part) in editor.model.parts.iter().enumerate() {
        let state = &mut editor.parts[i];
        if state.file != part.file {
            state.file = part.file.clone();
            state.gltf = (!part.file.is_empty())
                .then(|| assets.load(format!("{MOD_SOURCE}://{}", part.file)));
            state.nodes.clear();
            state.spawned = false;
        }
        let Some(gltf) = state.gltf.as_ref().and_then(|h| gltfs.get(h)) else {
            continue;
        };
        if state.nodes.is_empty() {
            state.nodes = gltf.named_nodes.keys().map(|n| n.to_string()).collect();
            state.nodes.sort();
        }
        if !state.spawned {
            state.spawned = true;
            let Some(scene) = gltf
                .default_scene
                .clone()
                .or_else(|| gltf.scenes.first().cloned())
            else {
                continue;
            };
            let mut entity = commands.spawn((
                WorldAssetRoot(scene),
                Transform::default(),
                PreviewPart { part: i },
            ));
            // Hidden until it hangs on its mount — same rule as the simulator.
            if part.mount.is_some() {
                entity.insert((Visibility::Hidden, Unmounted));
            } else {
                entity.insert(Visibility::default());
            }
        }
    }
}

/// Hangs waiting parts onto their mount nodes — the editor's copy of the
/// simulator's mounting, against the live model instead of a spawned line.
fn mount_preview(
    mut commands: Commands,
    editor: Res<Editor>,
    unmounted: Query<(Entity, &PreviewPart), With<Unmounted>>,
    parts: Query<(Entity, &PreviewPart)>,
    children: Query<&Children>,
    named: Query<&Name>,
) {
    for (entity, part) in unmounted.iter() {
        let Some((parent_index, node)) = editor
            .model
            .parts
            .get(part.part)
            .and_then(|p| p.mount.clone())
        else {
            // Mount removed while waiting: the part is a root now.
            commands
                .entity(entity)
                .insert(Visibility::Inherited)
                .remove::<Unmounted>();
            continue;
        };
        let Some((parent_root, _)) = parts.iter().find(|(_, p)| p.part == parent_index as usize)
        else {
            continue;
        };
        // Walk the parent's own nodes; a part already mounted inside it belongs
        // to another file and must not be searched.
        let mut stack = vec![parent_root];
        while let Some(e) = stack.pop() {
            if e != parent_root && parts.contains(e) {
                continue;
            }
            if let Ok(kids) = children.get(e) {
                stack.extend(kids.iter());
            }
            if named.get(e).is_ok_and(|n| n.as_str() == node) {
                commands
                    .entity(entity)
                    .insert((ChildOf(e), Visibility::Inherited))
                    .remove::<Unmounted>();
                break;
            }
        }
    }
}

/// Applies the lamp test: a bound node is visible while its lamp image is lit.
fn preview_lamps(
    mut commands: Commands,
    editor: Res<Editor>,
    roots: Query<(Entity, &PreviewPart)>,
    children: Query<&Children>,
    mut named: Query<(&Name, Option<&mut Visibility>, Has<PreviewLamp>)>,
) {
    for (root, part) in roots.iter() {
        let bindings: Vec<&LampBinding> = editor
            .model
            .lamps
            .iter()
            .filter(|l| l.part as usize == part.part)
            .collect();
        let mut stack = vec![root];
        while let Some(entity) = stack.pop() {
            // Do not cross into parts mounted inside this one.
            if entity != root && roots.contains(entity) {
                continue;
            }
            if let Ok(kids) = children.get(entity) {
                stack.extend(kids.iter());
            }
            let Ok((name, visibility, taken)) = named.get_mut(entity) else {
                continue;
            };
            let wanted = match bindings.iter().find(|l| l.node == name.as_str()) {
                Some(binding) => {
                    commands.entity(entity).insert(PreviewLamp);
                    if editor.lit.contains(&binding.lamp) {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    }
                }
                // Not bound (any more): give the node back to the file.
                None if taken => {
                    commands.entity(entity).remove::<PreviewLamp>();
                    Visibility::Inherited
                }
                None => continue,
            };
            match visibility {
                Some(mut current) => *current = wanted,
                // A glTF node does not have to carry `Visibility`.
                None => {
                    commands.entity(entity).insert(wanted);
                }
            }
        }
    }
}

/// Moves motion-bound nodes towards their lamp-test targets — the editor's copy
/// of the simulator's swing, driven by the toggles instead of the interlocking.
/// Base transforms are remembered on first touch.
fn preview_motions(
    time: Res<Time>,
    editor: Res<Editor>,
    roots: Query<(Entity, &PreviewPart)>,
    children: Query<&Children>,
    mut named: Query<(&Name, &mut Transform)>,
    mut bases: Local<HashMap<Entity, Transform>>,
    mut values: Local<HashMap<Entity, f32>>,
) {
    let dt = time.delta_secs();
    for (root, part) in roots.iter() {
        let bindings: Vec<&MotionBinding> = editor
            .model
            .motions
            .iter()
            .filter(|m| m.part as usize == part.part)
            .collect();
        if bindings.is_empty() {
            continue;
        }
        let mut stack = vec![root];
        while let Some(entity) = stack.pop() {
            // Do not cross into parts mounted inside this one.
            if entity != root && roots.contains(entity) {
                continue;
            }
            if let Ok(kids) = children.get(entity) {
                stack.extend(kids.iter());
            }
            let Ok((name, mut transform)) = named.get_mut(entity) else {
                continue;
            };
            let Some(binding) = bindings.iter().find(|m| m.node == name.as_str()) else {
                continue;
            };
            let target = if editor.lit.contains(&binding.lamp) {
                1.0
            } else {
                0.0
            };
            let value = values.entry(entity).or_insert(0.0);
            *value = slew(*value, target, dt, binding.seconds as f32);
            // Visibility motions are the lamp mechanism; the preview leaves
            // them to the lamp test.
            if !matches!(binding.motion, Motion::Visibility) {
                let base = *bases.entry(entity).or_insert(*transform);
                *transform = base * motion_transform(&binding.motion, *value);
            }
        }
    }
}

/// One step of the travel towards `target` — the editor's copy of the
/// simulator's linear swing.
fn slew(value: f32, target: f32, dt: f32, seconds: f32) -> f32 {
    if seconds <= 0.0 {
        return target;
    }
    let step = dt / seconds;
    (value + (target - value).clamp(-step, step)).clamp(0.0, 1.0)
}

/// Transform a [`Motion`] produces at `value` — the editor's copy of the
/// mapping the app applies at runtime.
fn motion_transform(motion: &Motion, value: f32) -> Transform {
    match *motion {
        Motion::Visibility => Transform::IDENTITY,
        Motion::Rotate { axis, degrees } => Transform::from_rotation(Quat::from_axis_angle(
            Vec3::from(axis).normalize_or_zero(),
            (degrees * value).to_radians(),
        )),
        Motion::Translate { axis, metres } => {
            Transform::from_translation(Vec3::from(axis) * metres * value)
        }
    }
}

/// Shows only the previewed level of detail once a LOD table exists — otherwise
/// every level sits inside the others. Without a table all levels show.
fn apply_preview_lod(
    mut commands: Commands,
    editor: Res<Editor>,
    mut nodes: Query<(Entity, &Name, Option<&mut Visibility>)>,
) {
    for (entity, name, visibility) in nodes.iter_mut() {
        let Some(level) = lod_level(name.as_str()) else {
            continue;
        };
        let wanted = if editor.model.lods.is_empty() || level == editor.preview_lod {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        match visibility {
            Some(mut current) => *current = wanted,
            None => {
                commands.entity(entity).insert(wanted);
            }
        }
    }
}

/// Orbit with the right mouse button, zoom with the wheel — same as the other
/// editors; the hand-built background `Ui` is hit-test blind, so the cursor is
/// tested against the stored viewport rect.
fn orbit_camera(
    mut view: ResMut<View>,
    mut camera: Query<&mut Transform, With<Camera3d>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut motion: MessageReader<bevy::input::mouse::MouseMotion>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
) {
    let over_ui = window
        .single()
        .ok()
        .and_then(|w| w.cursor_position())
        .is_none_or(|p| !view.viewport.contains(p));

    let drag: Vec2 = motion.read().map(|m| m.delta).sum();
    if !over_ui && buttons.pressed(MouseButton::Right) && drag != Vec2::ZERO {
        view.yaw -= drag.x * 0.005;
        view.pitch = (view.pitch + drag.y * 0.005).clamp(-1.5, 1.5);
        view.used = true;
    }
    let scroll: f32 = wheel.read().map(|w| w.y).sum();
    if !over_ui && scroll != 0.0 {
        view.distance = (view.distance * (1.0 - scroll * 0.1)).clamp(1.0, 200.0);
        view.used = true;
    }

    let offset = Vec3::new(
        view.distance * view.pitch.cos() * view.yaw.sin(),
        view.distance * view.pitch.sin(),
        view.distance * view.pitch.cos() * view.yaw.cos(),
    );
    for mut transform in camera.iter_mut() {
        *transform =
            Transform::from_translation(view.target + offset).looking_at(view.target, Vec3::Y);
    }
}

/// A one-metre grid on the ground — the ruler the mast heights are read against.
fn ground_grid(mut gizmos: Gizmos) {
    gizmos
        .grid(
            Isometry3d::new(
                Vec3::ZERO,
                Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
            ),
            UVec2::new(16, 16),
            Vec2::splat(1.0),
            Color::srgb(0.26, 0.28, 0.31),
        )
        .outer_edges();
}

/// Names the model and its unsaved state in the window title.
fn update_title(
    editor: Res<Editor>,
    mut windows: Query<&mut Window, With<bevy::window::PrimaryWindow>>,
) {
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    let key = if editor.dirty {
        "window-signal-editor-unsaved"
    } else {
        "window-signal-editor-named"
    };
    let title = t!(key, name = editor.display_name());
    if window.title != title {
        window.title = title;
    }
}

/// Keeps the window size in memory; the file is written when the user leaves.
fn track_window_size(
    mut editor: ResMut<Editor>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let size = (window.width(), window.height());
    if editor.settings.window != Some(size) {
        editor.settings.window = Some(size);
    }
}

/// Answers the close button: leave only once unsaved work is dealt with.
fn confirm_close(
    mut requests: MessageReader<bevy::window::WindowCloseRequested>,
    mut editor: ResMut<Editor>,
    mut exit: MessageWriter<AppExit>,
) {
    if requests.read().next().is_none() {
        return;
    }
    requests.clear();
    if ui::confirm_discard(&mut editor) {
        editor.settings.save();
        exit.write(AppExit::Success);
    }
}

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
    if *count >= limit.0 + 10 {
        exit.write(AppExit::Success);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::interlock::SignalPart;

    fn model_with_chain() -> SignalModel {
        SignalModel {
            parts: vec![
                SignalPart {
                    file: "m/a.gltf".into(),
                    mount: None,
                },
                SignalPart {
                    file: "m/b.gltf".into(),
                    mount: Some((0, "mp_1".into())),
                },
                SignalPart {
                    file: "m/c.gltf".into(),
                    mount: Some((1, "mp_2".into())),
                },
            ],
            lamps: vec![
                LampBinding {
                    lamp: "red".into(),
                    part: 1,
                    node: "lamp_red".into(),
                },
                LampBinding {
                    lamp: "zs3_4".into(),
                    part: 2,
                    node: "zs3_4".into(),
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn removing_a_part_keeps_references_consistent() {
        let mut editor = Editor {
            model: model_with_chain(),
            ..Default::default()
        };
        editor.remove_part(1);
        // The screen is gone: the Zs3 lost its mount and moved down one index …
        assert_eq!(editor.model.parts.len(), 2);
        assert_eq!(editor.model.parts[1].mount, None);
        // … its binding follows, the screen's own binding is dropped.
        assert_eq!(editor.model.lamps.len(), 1);
        assert_eq!(editor.model.lamps[0].part, 1);
    }

    #[test]
    fn mounting_below_itself_is_a_cycle() {
        let editor = Editor {
            model: model_with_chain(),
            ..Default::default()
        };
        // The Zs3 hangs on the screen: the screen cannot hang on the Zs3 …
        assert!(editor.would_cycle(1, 2));
        assert!(editor.would_cycle(0, 0));
        // … the other way round stays a chain.
        assert!(!editor.would_cycle(2, 0));
    }
}
