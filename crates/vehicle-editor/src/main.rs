//! TrainSim-DE vehicle editor — vehicle data, glTF model, levels of detail, moving parts.
//!
//! ```text
//! trainsim-vehicle-editor [vehicle.ron] [--frames N] [--screenshot file.png]
//! ```
//!
//! A desktop application, not a game screen: menu bar, docked panels, the operating
//! system's own file dialogs. The 3D viewport in the middle shows the imported model
//! against a reference body of the length over buffers.
//!
//! Models live in a mod and are addressed relative to the `mods/` directory
//! (`<mod>/assets/<file>.gltf`) — exactly as the simulator loads them later.

mod model;
mod powertrain;
mod settings;
mod ui;

use bevy::asset::io::AssetSourceBuilder;
use bevy::asset::io::file::FileAssetReader;
use bevy::gltf::{Gltf, GltfNode};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy_egui::{EguiPlugin, EguiPrimaryContextPass, PrimaryEguiContext};
use i18n::t;
use sim_core::train::{VehicleModel, VehicleSpec};
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
///
/// A failure that reads exactly like a success is worse than no message: the
/// user walks away believing the file was written.
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

/// Everything the editor is working on.
#[derive(Resource)]
pub struct Editor {
    pub spec: VehicleSpec,
    /// File the vehicle came from, `None` for a new one.
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub status: Status,
    /// The model file currently loading/loaded.
    pub gltf: Option<Handle<Gltf>>,
    /// Which file that is — so a newly opened vehicle brings its model along.
    pub loaded_file: String,
    /// Nodes of the loaded file.
    pub nodes: Vec<model::Node>,
    /// Show the reference body of the length over buffers.
    pub show_reference: bool,
    /// Show the one-metre ground grid.
    pub show_grid: bool,
    /// Which level of detail the viewport shows.
    pub preview_lod: u8,
    /// Substring the node list is narrowed to; empty shows all of them.
    pub node_filter: String,
    /// States to go back to, oldest first.
    pub undo: Vec<VehicleSpec>,
    /// States undone, to go forward to again.
    pub redo: Vec<VehicleSpec>,
    /// Whether the spec changed in the previous frame — one continuous drag
    /// changes it in every frame of the drag and must still cost one step.
    pub changing: bool,
    /// The comment warning has been answered once; do not ask again this
    /// session.
    pub warned_about_comments: bool,
    /// What survives between runs.
    pub settings: settings::Settings,
}

/// How many steps back the editor remembers. A spec is a few hundred bytes;
/// the limit is about not growing without bound, not about memory pressure.
const UNDO_DEPTH: usize = 128;

impl Default for Editor {
    fn default() -> Self {
        let settings = settings::Settings::load();
        Self {
            spec: VehicleSpec::default(),
            path: None,
            dirty: false,
            status: Status::Info(t!("status-new-vehicle")),
            gltf: None,
            loaded_file: String::new(),
            nodes: Vec::new(),
            show_reference: settings.show_reference,
            show_grid: settings.show_grid,
            preview_lod: 0,
            node_filter: String::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            changing: false,
            warned_about_comments: false,
            settings,
        }
    }
}

impl Editor {
    /// Reads a vehicle file.
    pub fn open(&mut self, path: PathBuf) {
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| {
                ron::from_str::<VehicleSpec>(&text)
                    .map_err(|e: ron::error::SpannedError| e.to_string())
            }) {
            Ok(spec) => {
                self.status = Status::Info(t!("status-loaded", file = path.display()));
                self.spec = spec;
                self.path = Some(path);
                self.dirty = false;
                self.nodes.clear();
                self.gltf = None;
                self.loaded_file.clear();
                self.forget_history();
                if let Some(path) = &self.path {
                    self.settings.remember(&path.clone());
                }
            }
            Err(e) => {
                self.status = Status::Error(t!("status-error", file = path.display(), error = e))
            }
        }
    }

    /// Writes the vehicle back as RON.
    pub fn save(&mut self, path: PathBuf) {
        let text = ron::ser::to_string_pretty(&self.spec, ron::ser::PrettyConfig::default())
            .expect("vehicle is serializable");
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

    /// The vehicle's model, created on first use.
    pub fn model_mut(&mut self) -> &mut VehicleModel {
        self.spec.model.get_or_insert_with(VehicleModel::default)
    }

    /// Records the state the user has just left, and drops the redo branch —
    /// once they edit again, what was undone is no longer reachable.
    pub fn remember(&mut self, state: VehicleSpec) {
        // The same state twice in a row is not a step — it would only cost the
        // user an undo press that visibly does nothing.
        if self.undo.last() == Some(&state) {
            return;
        }
        if self.undo.len() == UNDO_DEPTH {
            self.undo.remove(0);
        }
        self.undo.push(state);
        self.redo.clear();
    }

    pub fn undo(&mut self) {
        if let Some(state) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.spec, state));
            self.dirty = true;
            // Stepping through the history ends whatever interaction was
            // running. Without this the next edit counts as a continuation of
            // it and records no step of its own.
            self.changing = false;
        }
    }

    pub fn redo(&mut self) {
        if let Some(state) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.spec, state));
            self.dirty = true;
            self.changing = false;
        }
    }

    /// Drops a level of detail, keeping the preview on one that still exists.
    ///
    /// Previewing a level the model no longer lists hides every node it has —
    /// an empty viewport with nothing on screen to say why.
    pub fn remove_lod(&mut self, index: usize) {
        let gone = self.model_mut().lods.remove(index);
        if self.preview_lod == gone.level {
            self.preview_lod = self
                .spec
                .model
                .as_ref()
                .and_then(|m| m.lods.iter().map(|lod| lod.level).min())
                .unwrap_or(0);
        }
    }

    /// A file just loaded is a fresh start — there is nothing before it that
    /// undo could sensibly reach.
    fn forget_history(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.changing = false;
    }
}

/// Orbit camera around the vehicle.
#[derive(Resource)]
pub struct View {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub target: Vec3,
    /// Whether the user has moved the camera yet. Until they have, the
    /// viewport says how.
    pub used: bool,
}

impl Default for View {
    fn default() -> Self {
        Self {
            yaw: 0.7,
            pitch: 0.35,
            distance: 40.0,
            target: Vec3::new(0.0, 2.0, 0.0),
            used: false,
        }
    }
}

/// Marker of the spawned model instance — replaced on every import.
#[derive(Component)]
struct ModelInstance;

/// Marker of the reference body (length over buffers).
#[derive(Component)]
struct ReferenceBody;

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
    // `--window 1280x2000` — fixed size for reproducible CI screenshots.
    let window_size = flag("--window").and_then(|s| {
        let (w, h) = s.split_once('x')?;
        Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?))
    });
    if let Some(dir) = shot.as_ref().and_then(|p| std::path::Path::new(p).parent()) {
        let _ = std::fs::create_dir_all(dir);
    }
    let frame_limit = flag("--frames")
        .and_then(|n| n.parse::<u32>().ok())
        .or_else(|| shot.as_ref().map(|_| 60));

    // Language before anything else is built: the editor's own first status
    // message goes through `t!`. Loading the settings twice at startup costs a
    // few microseconds and keeps `Editor::default()` self-contained for New.
    settings::Settings::load().apply_language();
    let mut editor = Editor::default();
    if let Some(file) = args.first().filter(|a| !a.starts_with("--")) {
        editor.open(PathBuf::from(file));
    }

    let mut app = App::new();
    // Models come out of the mods, exactly as the simulator reads them later:
    // `mods://<mod>/assets/<file>.gltf`. Has to be registered before the asset plugin.
    app.register_asset_source(MOD_SOURCE, mod_asset_source());
    let mut window = Window {
        title: t!("window-vehicle-editor"),
        // Two data panels plus the viewport — the Bevy default of 1280 px
        // leaves the 3D view a sliver.
        resolution: bevy::window::WindowResolution::new(1440, 900),
        ..default()
    };
    if let Some((w, h)) = window_size {
        window.resolution = bevy::window::WindowResolution::new(w as u32, h as u32);
    } else if let Some((w, h)) = editor.settings.window {
        window.resolution = bevy::window::WindowResolution::new(w as u32, h as u32);
    }
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(window),
        // The close button is the way most people leave, so it has to ask
        // about unsaved work too. Nothing closes the window but `confirm_close`
        // from here on.
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
    .insert_resource(ClearColor(Color::srgb(0.16, 0.17, 0.19)))
    .insert_resource(editor)
    .init_resource::<View>()
    .add_systems(Startup, setup)
    .add_systems(EguiPrimaryContextPass, ui::draw)
    .add_systems(
        Update,
        (
            poll_model,
            orbit_camera,
            update_reference,
            apply_preview_lod,
            confirm_close,
            update_title,
            ground_grid,
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

    // Track: two rails at standard gauge as an orientation for the model.
    let rail = materials.add(StandardMaterial {
        base_color: Color::srgb(0.35, 0.36, 0.38),
        perceptual_roughness: 0.9,
        ..default()
    });
    let rail_mesh = meshes.add(Cuboid::new(0.07, 0.15, 400.0));
    for side in [-1.0, 1.0] {
        commands.spawn((
            Mesh3d(rail_mesh.clone()),
            MeshMaterial3d(rail.clone()),
            Transform::from_xyz(
                side * (sim_core::train::STANDARD_GAUGE as f32) / 2.0,
                -0.075,
                0.0,
            ),
        ));
    }

    // Reference body of the length over buffers — updated in `update_reference`. A little
    // larger than the loading gauge, so its faces never coincide with the model's.
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(3.2, 4.2, 20.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgba(0.3, 0.7, 1.0, 0.12),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        })),
        Transform::default(),
        ReferenceBody,
    ));
}

/// Waits for the imported glTF and reads its nodes.
fn poll_model(
    mut commands: Commands,
    mut editor: ResMut<Editor>,
    gltfs: Res<Assets<Gltf>>,
    nodes: Res<Assets<GltfNode>>,
    instances: Query<Entity, With<ModelInstance>>,
) {
    if !editor.nodes.is_empty() {
        return;
    }
    let Some(handle) = editor.gltf.clone() else {
        return;
    };
    let Some(gltf) = gltfs.get(&handle) else {
        return;
    };
    editor.nodes = model::inspect(gltf, &nodes);
    editor.status = Status::Info(t!("status-nodes-read", count = editor.nodes.len()));

    for entity in instances.iter() {
        commands.entity(entity).despawn();
    }
    if let Some(scene) = gltf
        .default_scene
        .clone()
        .or_else(|| gltf.scenes.first().cloned())
    {
        commands.spawn((WorldAssetRoot(scene), Transform::default(), ModelInstance));
    }
}

/// Shows only the level of detail selected in the model panel — otherwise all levels sit
/// inside one another and fight over the depth buffer.
fn apply_preview_lod(
    mut commands: Commands,
    editor: Res<Editor>,
    // A glTF node does not have to carry `Visibility` — it is inserted where it is missing.
    mut nodes: Query<(Entity, &Name, Option<&mut Visibility>)>,
) {
    for (entity, name, visibility) in nodes.iter_mut() {
        let Some(level) = model::lod_level(name.as_str()) else {
            continue;
        };
        let wanted = if level == editor.preview_lod {
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

/// Reference body follows length, and is shown or hidden.
fn update_reference(
    editor: Res<Editor>,
    mut body: Query<(&mut Transform, &mut Visibility), With<ReferenceBody>>,
) {
    for (mut transform, mut visibility) in body.iter_mut() {
        transform.scale = Vec3::new(1.0, 1.0, editor.spec.length as f32 / 20.0 * 1.002);
        transform.translation = Vec3::new(0.0, 2.1, 0.0);
        *visibility = if editor.show_reference {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

/// A one-metre grid on the ground under the vehicle.
///
/// Length over buffers, axle base and bogie spacing are what this editor is
/// about, and until now the viewport gave the eye nothing to measure them
/// against — the vehicle floated in an even grey. One metre is the unit the
/// numbers in the form are written in, so the grid is readable as a ruler and
/// not just as decoration. It follows the vehicle's length so it frames
/// whatever is loaded, from a shunter to a multiple unit.
fn ground_grid(editor: Res<Editor>, mut gizmos: Gizmos) {
    if !editor.show_grid {
        return;
    }
    let half_length = (editor.spec.length as f32 * 0.5).ceil() + 4.0;
    let cells = UVec2::new(16, (half_length * 2.0) as u32);
    gizmos
        .grid(
            Isometry3d::new(Vec3::ZERO, Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
            cells,
            Vec2::splat(1.0),
            Color::srgb(0.26, 0.28, 0.31),
        )
        .outer_edges();
}

/// Orbit with the right mouse button, zoom with the wheel — usual DCC controls.
fn orbit_camera(
    mut view: ResMut<View>,
    mut camera: Query<&mut Transform, With<Camera3d>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut motion: MessageReader<bevy::input::mouse::MouseMotion>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    // Do not steal the mouse while it is over a panel. `bevy_egui` updates the
    // resource after the pass, when the panel layout of the frame is known.
    over_ui: Res<bevy_egui::input::EguiWantsInput>,
) {
    let over_ui = over_ui.wants_any_pointer_input();

    let drag: Vec2 = motion.read().map(|m| m.delta).sum();
    if !over_ui && buttons.pressed(MouseButton::Right) && drag != Vec2::ZERO {
        view.yaw -= drag.x * 0.005;
        view.pitch = (view.pitch + drag.y * 0.005).clamp(-1.5, 1.5);
        view.used = true;
    }
    let scroll: f32 = wheel.read().map(|w| w.y).sum();
    if !over_ui && scroll != 0.0 {
        view.distance = (view.distance * (1.0 - scroll * 0.1)).clamp(2.0, 400.0);
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

/// Names the vehicle and its unsaved state in the window title.
///
/// The title bar is the only part of the editor still readable from the task
/// bar or the window switcher — with several vehicles open, a fixed
/// "TrainSim-DE — Vehicle editor" on each of them tells you nothing.
fn update_title(editor: Res<Editor>, mut windows: Query<&mut Window, With<bevy::window::PrimaryWindow>>) {
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    let key = if editor.dirty {
        "window-vehicle-editor-unsaved"
    } else {
        "window-vehicle-editor-named"
    };
    let title = t!(key, name = editor.spec.name);
    if window.title != title {
        window.title = title;
    }
}

/// Keeps the window size in the settings struct — in memory only. Writing on
/// every frame of a resize drag would hammer the disk; the file is written
/// when the user leaves.
fn track_window_size(mut editor: ResMut<Editor>, windows: Query<&Window, With<bevy::window::PrimaryWindow>>) {
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
