//! Connected Rails signal editor — modular signal models: glTF parts on mount points,
//! lamp-image bindings, live preview (plan ch. 15.3, the Zusi assembly pattern).
//!
//! ```text
//! trainsim-signal-editor [signal_model.ron] [--frames N] [--screenshot file.png]
//!   [--render-only --view front|rear|left|right|front-left|front-right|rear-left|rear-right]
//!   [--focus full|head|detail|base --aspect hp0|hp1|hp2|vr0|vr1|vr2|sh0|sh1 --lod N]
//!   [--target-node NAME_PREFIX] [--isolate-target]
//!   [--from-aspect ASPECT --motion-time SECONDS]
//!   [--background neutral|light|dark --bounds-json file.json]
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
use bevy::camera::{ScalingMode, primitives::Aabb, visibility::RenderLayers};
use bevy::gltf::Gltf;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy_egui::{EguiPlugin, EguiPrimaryContextPass, PrimaryEguiContext};
use i18n::t;
use serde::{Deserialize, Serialize};
use sim_core::interlock::{LampBinding, MotionBinding, MotionProfile, SignalModel, advance_motion};
use sim_core::train::{Motion, lod_level};
use std::collections::{BTreeSet, HashMap, HashSet};
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

/// `%APPDATA%\Connected Rails\` on Windows, `$XDG_CONFIG_HOME` or `~/.config` elsewhere.
fn settings_path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("Connected Rails").join("signal-editor.ron"))
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

/// A reproducible camera direction for an asset-review screenshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CaptureView {
    Front,
    Rear,
    Left,
    Right,
    FrontLeft,
    FrontRight,
    RearLeft,
    RearRight,
}

impl CaptureView {
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "front" => Self::Front,
            "rear" => Self::Rear,
            "left" => Self::Left,
            "right" => Self::Right,
            "front-left" => Self::FrontLeft,
            "front-right" => Self::FrontRight,
            "rear-left" => Self::RearLeft,
            "rear-right" => Self::RearRight,
            _ => return None,
        })
    }

    /// From the signal towards the camera. Signal fronts face +Z.
    fn direction(self) -> Vec3 {
        match self {
            Self::Front => Vec3::Z,
            Self::Rear => Vec3::NEG_Z,
            Self::Left => Vec3::NEG_X,
            Self::Right => Vec3::X,
            Self::FrontLeft => Vec3::new(-1.0, 0.12, 1.0).normalize(),
            Self::FrontRight => Vec3::new(1.0, 0.12, 1.0).normalize(),
            Self::RearLeft => Vec3::new(-1.0, 0.12, -1.0).normalize(),
            Self::RearRight => Vec3::new(1.0, 0.12, -1.0).normalize(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CaptureFocus {
    Full,
    Head,
    Detail,
    Base,
}

/// Flat studio backdrops for repeatable silhouette and material review.
///
/// Neutral is deliberately lighter than the interactive editor: black-painted
/// rear mechanisms must remain legible in an unattended review matrix. Light
/// is the explicit high-contrast check for those backs; dark preserves the old
/// viewport look and exposes white edge halos.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CaptureBackground {
    Neutral,
    Light,
    Dark,
}

impl CaptureBackground {
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "neutral" => Self::Neutral,
            "light" => Self::Light,
            "dark" => Self::Dark,
            _ => return None,
        })
    }

    fn color(self) -> Color {
        match self {
            Self::Neutral => Color::srgb(0.42, 0.45, 0.49),
            Self::Light => Color::srgb(0.74, 0.77, 0.81),
            Self::Dark => Color::srgb(0.16, 0.17, 0.19),
        }
    }
}

impl CaptureFocus {
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "full" => Self::Full,
            "head" => Self::Head,
            "detail" => Self::Detail,
            "base" => Self::Base,
            _ => return None,
        })
    }
}

/// UI-free, orthographic output used by `tools/signals/preview.py`.
#[derive(Resource)]
pub struct CaptureSettings {
    path: String,
    /// Machine-readable full and framed bounds beside the screenshot.  This
    /// lets the review wrapper catch centimetre-scale drift without trying to
    /// infer dimensions from pixels or camera perspective.
    bounds_path: Option<String>,
    view: CaptureView,
    focus: CaptureFocus,
    /// Optional glTF node-name prefix used only for camera framing.  The rest
    /// of the assembled signal stays visible, so attachment errors remain
    /// obvious in a close component review.
    target_node: Option<String>,
    /// Render only the target node and its descendants while retaining the
    /// untouched full assembly for bounds and transform evaluation.
    isolate_target: bool,
    width: u32,
    height: u32,
    settle_frames: u32,
    /// Optional deterministic sample of a real aspect transition. The target
    /// is `Editor::lit`; these are the channels active at t=0.
    motion: Option<MotionCapture>,
}

#[derive(Clone, Debug)]
struct MotionCapture {
    from: BTreeSet<String>,
    target: BTreeSet<String>,
    elapsed: f32,
}

#[derive(Resource, Default)]
struct CaptureProgress {
    settled: u32,
    after_shot: u32,
}

/// Exact-size image rendered by a windowless capture run.
#[derive(Resource)]
struct CaptureTarget {
    image: Handle<Image>,
}

fn aspect_lamps(aspect: &str) -> Option<&'static [&'static str]> {
    Some(match aspect {
        "hp0" => &["lamp_red"],
        "hp1" => &["fluegel1", "lamp_green"],
        // Both command schemes are selected deliberately: ordinary two-arm
        // models bind fluegel1/fluegel2, coupled Hp0/Hp2 models bind only the
        // shared channel. Unbound preview channels are harmless.
        "hp2" => &[
            "fluegel1",
            "fluegel2",
            "fluegel_gekuppelt",
            "lamp_green",
            "lamp_yellow",
        ],
        "vr0" => &["vr0_licht"],
        "vr1" => &[
            "scheibe_weg",
            "vr1_licht",
            "vr_blende_links_gruen",
            "vr_blende_rechts_gruen",
        ],
        "vr2" => &["vr2_fluegel", "vr2_licht", "vr_blende_rechts_gruen"],
        // The mechanical Sh face is internally lit/reflective in both
        // positions. Only its black bar and coupled rear shutter rotate.
        "sh0" => &["sh_white"],
        "none" => &[],
        "sh1" => &["sperrscheibe_frei", "sh_white"],
        _ => return None,
    })
}

fn aspect_lamp_set(aspect: &str) -> Option<BTreeSet<String>> {
    aspect_lamps(aspect).map(|lamps| lamps.iter().map(|lamp| (*lamp).to_owned()).collect())
}

fn parse_window(value: Option<String>) -> Option<(u32, u32)> {
    let value = value?;
    let (width, height) = value.split_once('x')?;
    Some((width.parse().ok()?, height.parse().ok()?))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let shot = flag("--screenshot");
    let render_only = args.iter().any(|arg| arg == "--render-only");
    if let Some(dir) = shot.as_ref().and_then(|p| std::path::Path::new(p).parent()) {
        let _ = std::fs::create_dir_all(dir);
    }
    let requested_frames = flag("--frames").and_then(|n| n.parse::<u32>().ok());
    let frame_limit = (!render_only)
        .then(|| requested_frames.or_else(|| shot.as_ref().map(|_| 60)))
        .flatten();

    // Language before the first status message goes through `t!`.
    Settings::load().apply_language();
    let mut editor = Editor::default();
    if let Some(file) = args.first().filter(|a| !a.starts_with("--")) {
        editor.open(PathBuf::from(file));
    }
    if let Some(level) = flag("--lod").and_then(|value| value.parse::<u8>().ok()) {
        editor.preview_lod = level;
    }
    if let Some(aspect) = flag("--aspect") {
        editor.lit =
            aspect_lamp_set(&aspect).unwrap_or_else(|| panic!("unknown --aspect {aspect}"));
    }
    if let Some(lamps) = flag("--lamps") {
        editor.lit.extend(
            lamps
                .split(',')
                .filter(|lamp| !lamp.is_empty())
                .map(str::to_owned),
        );
    }

    let requested_size = parse_window(flag("--window")).unwrap_or((900, 1200));
    let motion = flag("--from-aspect").map(|aspect| MotionCapture {
        from: aspect_lamp_set(&aspect).unwrap_or_else(|| panic!("unknown --from-aspect {aspect}")),
        target: editor.lit.clone(),
        elapsed: flag("--motion-time")
            .and_then(|value| value.parse::<f32>().ok())
            .unwrap_or(0.0)
            .clamp(0.0, 60.0),
    });
    // The first film-strip frame is the complete source aspect. Once motion
    // starts, the target lamps are selected while the mechanical filters and
    // arms follow their sampled trajectories.
    if let Some(sample) = motion.as_ref().filter(|sample| sample.elapsed == 0.0) {
        editor.lit = sample.from.clone();
    }
    let capture = if render_only {
        shot.clone().map(|path| CaptureSettings {
            path,
            bounds_path: flag("--bounds-json"),
            view: flag("--view")
                .as_deref()
                .and_then(CaptureView::parse)
                .unwrap_or(CaptureView::Front),
            focus: flag("--focus")
                .as_deref()
                .and_then(CaptureFocus::parse)
                .unwrap_or(CaptureFocus::Full),
            target_node: flag("--target-node"),
            isolate_target: args.iter().any(|arg| arg == "--isolate-target"),
            width: requested_size.0,
            height: requested_size.1,
            settle_frames: requested_frames.unwrap_or(35),
            motion,
        })
    } else {
        None
    };
    if capture
        .as_ref()
        .is_some_and(|settings| settings.isolate_target && settings.target_node.is_none())
    {
        panic!("--isolate-target requires --target-node NAME_PREFIX");
    }
    let background = if render_only {
        flag("--background")
            .as_deref()
            .map(|value| {
                CaptureBackground::parse(value)
                    .unwrap_or_else(|| panic!("unknown --background {value}"))
            })
            .unwrap_or(CaptureBackground::Neutral)
    } else {
        CaptureBackground::Dark
    };

    let capture_run = capture.is_some();
    let mut app = App::new();
    // Parts come out of the mods, exactly as the simulator reads them later.
    // Has to be registered before the asset plugin.
    app.register_asset_source(MOD_SOURCE, mod_asset_source());
    let mut window = Window {
        title: t!("window-signal-editor"),
        resolution: bevy::window::WindowResolution::new(requested_size.0, requested_size.1),
        ..default()
    };
    if capture.is_none()
        && flag("--window").is_none()
        && let Some((w, h)) = editor.settings.window
    {
        window.resolution = bevy::window::WindowResolution::new(w as u32, h as u32);
    }
    let mut default_plugins = DefaultPlugins.set(WindowPlugin {
        primary_window: (!capture_run).then_some(window),
        // The close button has to ask about unsaved work; `confirm_close` owns it.
        close_when_requested: false,
        exit_condition: if capture_run {
            bevy::window::ExitCondition::DontExit
        } else {
            bevy::window::ExitCondition::OnAllClosed
        },
        ..default()
    });
    if capture_run {
        // An offscreen image, not a hidden window: compositors are then unable
        // to resize a nominal 1200x900 review into the current desktop tile.
        default_plugins =
            default_plugins
                .disable::<bevy::winit::WinitPlugin>()
                .set(bevy::render::RenderPlugin {
                    synchronous_pipeline_compilation: true,
                    ..default()
                });
    }
    app.add_plugins(default_plugins)
        .add_plugins(app_icon::plugin)
        .insert_resource(ClearColor(background.color()))
        .insert_resource(editor)
        .init_resource::<View>()
        .add_systems(Startup, setup)
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

    if capture_run {
        // Without winit there is no event loop to advance frames.
        app.add_plugins(bevy::app::ScheduleRunnerPlugin::run_loop(
            std::time::Duration::ZERO,
        ));
    }

    if capture.is_none() {
        app.add_plugins(EguiPlugin::default())
            // The UI belongs on our own camera (see the vehicle editor for the why).
            .insert_resource(bevy_egui::EguiGlobalSettings {
                auto_create_primary_context: false,
                ..default()
            })
            .add_systems(EguiPrimaryContextPass, ui::draw);
    }

    if let Some(capture) = capture {
        app.insert_resource(capture)
            .init_resource::<CaptureProgress>()
            .add_systems(
                Update,
                isolate_capture_target
                    .after(mount_preview)
                    .after(preview_lamps)
                    .after(apply_preview_lod),
            )
            .add_systems(
                Update,
                capture_preview
                    .after(mount_preview)
                    .after(preview_motions)
                    .after(apply_preview_lod)
                    .after(isolate_capture_target),
            );
    }

    if let Some(frames) = frame_limit {
        app.insert_resource(FrameLimit(frames))
            .add_systems(Update, exit_after_frames);
    }
    if !render_only && let Some(path) = shot {
        app.insert_resource(ShotPath(path));
    }
    app.run();
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    capture: Option<Res<CaptureSettings>>,
) {
    let projection = if capture.is_some() {
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: 12.0,
            },
            near: 0.01,
            far: 500.0,
            ..OrthographicProjection::default_3d()
        })
    } else {
        Projection::Perspective(PerspectiveProjection {
            far: 5_000.0,
            ..default()
        })
    };
    let capture_target = capture.as_ref().map(|capture| {
        let mut image = Image::new_fill(
            bevy::render::render_resource::Extent3d {
                width: capture.width,
                height: capture.height,
                depth_or_array_layers: 1,
            },
            bevy::render::render_resource::TextureDimension::D2,
            &[0, 0, 0, 255],
            bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
            bevy::asset::RenderAssetUsages::default(),
        );
        image.texture_descriptor.usage =
            bevy::render::render_resource::TextureUsages::RENDER_ATTACHMENT
                | bevy::render::render_resource::TextureUsages::COPY_SRC
                | bevy::render::render_resource::TextureUsages::TEXTURE_BINDING;
        images.add(image)
    });
    if let Some(image) = capture_target.as_ref() {
        commands.insert_resource(CaptureTarget {
            image: image.clone(),
        });
    }

    let mut camera = commands.spawn((
        Camera3d::default(),
        // HDR + bloom, like the simulator: the lamp test glows the way the
        // night run will.
        bevy::camera::Hdr,
        bevy::post_process::bloom::Bloom::NATURAL,
        projection,
        Transform::default(),
    ));
    if let Some(image) = capture_target {
        camera.insert(bevy::camera::RenderTarget::Image(image.into()));
    }
    if capture
        .as_ref()
        .is_some_and(|settings| settings.isolate_target)
    {
        // Layer 0 remains on the target so the ordinary studio lights still
        // illuminate it. Layer 1 is camera-exclusive and therefore removes
        // occluding sibling nodes without changing visibility or bounds.
        camera.insert(RenderLayers::layer(1));
    }
    if capture.is_none() {
        camera.insert(PrimaryEguiContext);
    }
    let isolated_capture = capture
        .as_ref()
        .is_some_and(|settings| settings.isolate_target);
    let mut key_light = commands.spawn((
        DirectionalLight {
            illuminance: 12_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(30.0, 60.0, 30.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    if isolated_capture {
        key_light.insert(RenderLayers::from_layers(&[0, 1]));
    }
    let mut ambient = commands.spawn((
        AmbientLight {
            color: Color::srgb(0.8, 0.85, 1.0),
            brightness: 700.0,
            ..default()
        },
        Transform::default(),
    ));
    if isolated_capture {
        ambient.insert(RenderLayers::from_layers(&[0, 1]));
    }
    // Rear fill keeps dark-painted mechanisms readable in every review view.
    let mut fill_light = commands.spawn((
        DirectionalLight {
            illuminance: 6_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(-24.0, 36.0, -30.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    if isolated_capture {
        fill_light.insert(RenderLayers::from_layers(&[0, 1]));
    }

    if capture.is_some() {
        return;
    }

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

/// Moves motion-bound nodes towards their lamp-test targets with the simulator's
/// own kinematics, driven by the toggles instead of the interlocking. Base
/// transforms and dynamic state are remembered on first touch.
fn preview_motions(
    time: Res<Time>,
    editor: Res<Editor>,
    roots: Query<(Entity, &PreviewPart)>,
    children: Query<&Children>,
    mut named: Query<(&Name, &mut Transform)>,
    mut bases: Local<HashMap<Entity, Transform>>,
    mut states: Local<HashMap<Entity, (f32, f32)>>,
    capture: Option<Res<CaptureSettings>>,
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
            let state = states.entry(entity).or_insert((0.0, 0.0));
            *state = if let Some(sample) = capture
                .as_ref()
                .and_then(|settings| settings.motion.as_ref())
            {
                let target = sample.target.contains(&binding.lamp) as u8 as f32;
                sample_motion(
                    binding.profile,
                    binding.seconds as f32,
                    sample.from.contains(&binding.lamp) as u8 as f32,
                    target,
                    sample.elapsed,
                )
            } else if capture.is_some() {
                // A normal review plate represents the settled signal aspect,
                // while an explicit motion capture above samples its real path.
                let target = editor.lit.contains(&binding.lamp) as u8 as f32;
                (target, 0.0)
            } else {
                let target = editor.lit.contains(&binding.lamp) as u8 as f32;
                advance_motion(
                    binding.profile,
                    state.0,
                    state.1,
                    target,
                    dt,
                    binding.seconds as f32,
                )
            };
            // Visibility motions are the lamp mechanism; the preview leaves
            // them to the lamp test.
            if !matches!(binding.motion, Motion::Visibility) {
                let base = *bases.entry(entity).or_insert(*transform);
                *transform = base * motion_transform(&binding.motion, state.0);
            }
        }
    }
}

/// Samples a transition at an exact elapsed time with the simulator's own
/// motion integrator. A fixed outer step keeps screenshots independent of the
/// renderer frame rate; `advance_motion` retains its 240 Hz impact substeps.
fn sample_motion(
    profile: MotionProfile,
    seconds: f32,
    start: f32,
    target: f32,
    elapsed: f32,
) -> (f32, f32) {
    let mut state = (start.clamp(0.0, 1.0), 0.0);
    let mut remaining = elapsed.max(0.0).min(60.0);
    while remaining > 0.0 {
        let dt = remaining.min(1.0 / 60.0);
        state = advance_motion(profile, state.0, state.1, target, dt, seconds);
        remaining -= dt;
    }
    state
}

/// Transform a [`Motion`] produces at `value` — the editor's copy of the
/// mapping the app applies at runtime.
fn motion_transform(motion: &Motion, value: f32) -> Transform {
    match *motion {
        Motion::Visibility | Motion::Emissive => Transform::IDENTITY,
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
    mut nodes: Query<(Entity, &Name, Option<&mut Visibility>, Has<PreviewLamp>)>,
) {
    for (entity, name, visibility, lamp) in nodes.iter_mut() {
        let Some(level) = lod_level(name.as_str()) else {
            continue;
        };
        let selected = editor.model.lods.is_empty() || level == editor.preview_lod;
        // A selected lamp remains under `preview_lamps` control. Replacing its
        // Hidden state with Inherited here made every unlit lens glow in older
        // editor screenshots, even though the simulator itself was correct.
        if selected && lamp {
            continue;
        }
        let wanted = selected
            .then_some(Visibility::Inherited)
            .unwrap_or(Visibility::Hidden);
        match visibility {
            Some(mut current) => *current = wanted,
            None => {
                commands.entity(entity).insert(wanted);
            }
        }
    }
}

/// Put a requested component and all its mesh descendants on the capture-only
/// render layer.  Visibility is deliberately left untouched: target framing,
/// active lamp state, LOD selection and the machine-readable assembly bounds
/// continue to be evaluated against the exact loaded signal.
fn isolate_capture_target(
    capture: Res<CaptureSettings>,
    named: Query<(Entity, &Name)>,
    children: Query<&Children>,
    mut tagged: Local<HashSet<Entity>>,
    mut commands: Commands,
) {
    if !capture.isolate_target {
        return;
    }
    let Some(prefix) = capture.target_node.as_deref() else {
        return;
    };
    for (entity, name) in named.iter() {
        if !name.as_str().starts_with(prefix) {
            continue;
        }
        for member in std::iter::once(entity).chain(children.iter_descendants(entity)) {
            if tagged.insert(member) {
                commands
                    .entity(member)
                    .insert(RenderLayers::from_layers(&[0, 1]));
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
    capture: Option<Res<CaptureSettings>>,
) {
    if capture.is_some() {
        return;
    }
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

/// Frames only the assembled signal, then takes a deterministic orthographic
/// screenshot after its meshes and textures have spent a few frames on the GPU.
fn capture_preview(
    capture: Res<CaptureSettings>,
    target: Res<CaptureTarget>,
    mut progress: ResMut<CaptureProgress>,
    assets: Res<AssetServer>,
    editor: Res<Editor>,
    roots: Query<Entity, With<PreviewPart>>,
    children: Query<&Children>,
    names: Query<&Name>,
    bounds: Query<(&GlobalTransform, &Aabb, Option<&Visibility>)>,
    mut camera: Query<(&mut Transform, &mut Projection), With<Camera3d>>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    if progress.after_shot > 0 {
        progress.after_shot += 1;
        if progress.after_shot >= 10 {
            exit.write(AppExit::Success);
        }
        return;
    }

    let ready = editor.parts.iter().all(|part| {
        part.gltf
            .as_ref()
            .is_some_and(|handle| assets.is_loaded_with_dependencies(handle))
    });
    let Some((assembly_min, assembly_max)) = ready
        .then(|| preview_bounds(&roots, &children, &names, &bounds, None))
        .flatten()
    else {
        return;
    };
    let (mut min, mut max) = if let Some(prefix) = capture.target_node.as_deref() {
        preview_bounds(&roots, &children, &names, &bounds, Some(prefix))
            .unwrap_or_else(|| panic!("--target-node {prefix:?} did not match a visible node"))
    } else {
        (assembly_min, assembly_max)
    };

    // A named component already supplies exact bounds and must never be cut in
    // half by a generic top/base window. Without a target, fixed metre crops
    // keep the same feature at the same scale across revisions.
    if capture.target_node.is_none() {
        match capture.focus {
            CaptureFocus::Full => {}
            CaptureFocus::Head => min.y = min.y.max(max.y - 3.2),
            // A fixed 1.25-m top crop resolves enamel grain, glass Fresnel rings,
            // fasteners and edge wear without changing camera scale from one
            // revision to the next. It is the material microscope paired with
            // the broader three-direction head review.
            CaptureFocus::Detail => min.y = min.y.max(max.y - 1.25),
            CaptureFocus::Base => max.y = max.y.min(min.y + 2.5),
        }
    }
    let center = (min + max) * 0.5;
    let direction = capture.view.direction();
    let right = Vec3::Y.cross(direction).normalize_or_zero();
    let up = direction.cross(right).normalize_or_zero();

    // Project the axis-aligned world box into the requested camera plane. This
    // fits side and quarter views without the huge empty margin a bounding
    // sphere would add to a twelve-metre mast.
    let mut screen_min = Vec2::splat(f32::INFINITY);
    let mut screen_max = Vec2::splat(f32::NEG_INFINITY);
    for corner in box_corners(min, max) {
        let delta = corner - center;
        let projected = Vec2::new(delta.dot(right), delta.dot(up));
        screen_min = screen_min.min(projected);
        screen_max = screen_max.max(projected);
    }
    let size = screen_max - screen_min;
    let aspect = capture.width as f32 / capture.height as f32;
    let viewport_height = (size.y.max(size.x / aspect) * 1.12).max(0.25);

    let Ok((mut transform, mut projection)) = camera.single_mut() else {
        return;
    };
    *transform = Transform::from_translation(center + direction * 40.0).looking_at(center, Vec3::Y);
    *projection = Projection::Orthographic(OrthographicProjection {
        scaling_mode: ScalingMode::FixedVertical { viewport_height },
        near: 0.01,
        far: 100.0,
        ..OrthographicProjection::default_3d()
    });
    progress.settled += 1;

    if progress.settled >= capture.settle_frames {
        if let Some(path) = &capture.bounds_path {
            let motion = capture_motion_report(&editor, &capture);
            write_capture_bounds(path, assembly_min, assembly_max, min, max, motion)
                .unwrap_or_else(|error| panic!("cannot write capture bounds {path}: {error}"));
        }
        commands
            .spawn(Screenshot::image(target.image.clone()))
            .observe(save_to_disk(capture.path.clone()));
        progress.after_shot = 1;
    }
}

/// Numerical state of one bound mechanism in the exact screenshot frame.
///
/// Keeping this beside the image makes a review independent of eyeballing a
/// five-degree rebound or deciding from perspective alone which way a blade
/// moved. It is derived with the same sampler that drives the rendered node.
#[derive(Serialize)]
struct CaptureMotionReport {
    node: String,
    lamp: String,
    profile: String,
    seconds: f32,
    travel: f32,
    velocity: f32,
    kind: &'static str,
    axis: [f32; 3],
    configured_amount: f32,
    effective_amount: f32,
    unit: &'static str,
}

fn capture_motion_report(editor: &Editor, capture: &CaptureSettings) -> Vec<CaptureMotionReport> {
    editor
        .model
        .motions
        .iter()
        .map(|binding| {
            let (travel, velocity) = if let Some(sample) = &capture.motion {
                sample_motion(
                    binding.profile,
                    binding.seconds as f32,
                    sample.from.contains(&binding.lamp) as u8 as f32,
                    sample.target.contains(&binding.lamp) as u8 as f32,
                    sample.elapsed,
                )
            } else {
                (editor.lit.contains(&binding.lamp) as u8 as f32, 0.0)
            };
            let (kind, axis, configured_amount, unit) = match binding.motion {
                Motion::Rotate { axis, degrees } => ("rotate", axis, degrees, "degree"),
                Motion::Translate { axis, metres } => ("translate", axis, metres, "metre"),
                Motion::Visibility => ("visibility", [0.0; 3], 1.0, "ratio"),
                Motion::Emissive => ("emissive", [0.0; 3], 1.0, "ratio"),
            };
            CaptureMotionReport {
                node: binding.node.clone(),
                lamp: binding.lamp.clone(),
                profile: format!("{:?}", binding.profile),
                seconds: binding.seconds as f32,
                travel,
                velocity,
                kind,
                axis,
                configured_amount,
                effective_amount: configured_amount * travel,
                unit,
            }
        })
        .collect()
}

fn preview_bounds(
    roots: &Query<Entity, With<PreviewPart>>,
    children: &Query<&Children>,
    names: &Query<&Name>,
    bounds: &Query<(&GlobalTransform, &Aabb, Option<&Visibility>)>,
    target_prefix: Option<&str>,
) -> Option<(Vec3, Vec3)> {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for root in roots.iter() {
        for entity in std::iter::once(root).chain(children.iter_descendants(root)) {
            if target_prefix.is_some_and(|prefix| {
                !names
                    .get(entity)
                    .is_ok_and(|name| name.as_str().starts_with(prefix))
            }) {
                continue;
            }
            let Ok((transform, aabb, visibility)) = bounds.get(entity) else {
                continue;
            };
            // Hidden LODs and inactive lamp/filter meshes must not enlarge the
            // measured assembly.  Their transforms can differ from the chosen
            // state even though Bevy correctly omits them from the screenshot.
            if matches!(visibility, Some(Visibility::Hidden)) {
                continue;
            }
            let local_min = Vec3::from(aabb.center) - Vec3::from(aabb.half_extents);
            let local_max = Vec3::from(aabb.center) + Vec3::from(aabb.half_extents);
            for corner in box_corners(local_min, local_max) {
                let world = transform.affine().transform_point3(corner);
                min = min.min(world);
                max = max.max(world);
            }
        }
    }
    (min.x <= max.x).then_some((min, max))
}

fn write_capture_bounds(
    path: &str,
    assembly_min: Vec3,
    assembly_max: Vec3,
    framed_min: Vec3,
    framed_max: Vec3,
    motions: Vec<CaptureMotionReport>,
) -> std::io::Result<()> {
    let path = std::path::Path::new(path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    #[derive(Serialize)]
    struct Report {
        unit: &'static str,
        assembly_min: [f32; 3],
        assembly_max: [f32; 3],
        assembly_size: [f32; 3],
        framed_min: [f32; 3],
        framed_max: [f32; 3],
        framed_size: [f32; 3],
        motions: Vec<CaptureMotionReport>,
    }
    let array = |value: Vec3| value.to_array();
    let report = Report {
        unit: "metre",
        assembly_min: array(assembly_min),
        assembly_max: array(assembly_max),
        assembly_size: array(assembly_max - assembly_min),
        framed_min: array(framed_min),
        framed_max: array(framed_max),
        framed_size: array(framed_max - framed_min),
        motions,
    };
    let text = serde_json::to_string_pretty(&report).map_err(std::io::Error::other)? + "\n";
    std::fs::write(path, text)
}

fn box_corners(min: Vec3, max: Vec3) -> [Vec3; 8] {
    [
        Vec3::new(min.x, min.y, min.z),
        Vec3::new(max.x, min.y, min.z),
        Vec3::new(min.x, max.y, min.z),
        Vec3::new(max.x, max.y, min.z),
        Vec3::new(min.x, min.y, max.z),
        Vec3::new(max.x, min.y, max.z),
        Vec3::new(min.x, max.y, max.z),
        Vec3::new(max.x, max.y, max.z),
    ]
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

    #[test]
    fn capture_views_have_unambiguous_cardinal_directions() {
        assert_eq!(CaptureView::parse("front"), Some(CaptureView::Front));
        assert_eq!(CaptureView::Front.direction(), Vec3::Z);
        assert_eq!(CaptureView::Rear.direction(), Vec3::NEG_Z);
        assert_eq!(CaptureView::Left.direction(), Vec3::NEG_X);
        assert_eq!(CaptureView::Right.direction(), Vec3::X);
        assert!(CaptureView::parse("somewhere").is_none());
    }

    #[test]
    fn form_signal_aspects_include_visual_and_motion_channels() {
        assert_eq!(aspect_lamps("hp0"), Some(&["lamp_red"] as &[_]));
        assert_eq!(
            aspect_lamps("hp2"),
            Some(&[
                "fluegel1",
                "fluegel2",
                "fluegel_gekuppelt",
                "lamp_green",
                "lamp_yellow",
            ] as &[_],)
        );
        assert!(aspect_lamps("vr1").unwrap().contains(&"scheibe_weg"));
        assert!(
            aspect_lamps("vr1")
                .unwrap()
                .contains(&"vr_blende_links_gruen")
        );
        assert_eq!(aspect_lamps("sh0"), Some(&["sh_white"] as &[_]));
        assert!(aspect_lamps("sh1").unwrap().contains(&"sperrscheibe_frei"));
        assert!(aspect_lamps("invalid").is_none());
    }

    #[test]
    fn capture_window_parser_rejects_incomplete_sizes() {
        assert_eq!(parse_window(Some("1600x1200".into())), Some((1600, 1200)));
        assert_eq!(parse_window(Some("1600".into())), None);
        assert_eq!(parse_window(Some("wide×high".into())), None);
    }

    #[test]
    fn capture_backgrounds_are_explicit_and_neutral_is_not_the_old_dark_view() {
        assert_eq!(
            CaptureBackground::parse("neutral"),
            Some(CaptureBackground::Neutral)
        );
        assert_eq!(
            CaptureBackground::parse("light"),
            Some(CaptureBackground::Light)
        );
        assert_eq!(
            CaptureBackground::parse("dark"),
            Some(CaptureBackground::Dark)
        );
        assert!(CaptureBackground::parse("sky").is_none());
        assert_ne!(
            CaptureBackground::Neutral.color(),
            CaptureBackground::Dark.color()
        );
    }

    #[test]
    fn deterministic_motion_sample_uses_the_runtime_kinematics() {
        let profile = MotionProfile::Semaphore {
            fall_seconds: 0.75,
            rebound: 0.36,
        };
        let falling = sample_motion(profile, 1.8, 1.0, 0.0, 0.50);
        let rebound = sample_motion(profile, 1.8, 1.0, 0.0, 0.90);
        let settled = sample_motion(profile, 1.8, 1.0, 0.0, 2.50);

        assert!(falling.0 > 0.40 && falling.1 < 0.0);
        assert!(rebound.0 > 0.05 && rebound.1 > 0.0);
        assert_eq!(settled, (0.0, 0.0));

        // The canonical 45-degree Hauptsignal arm reaches a clearly readable
        // first rebound peak of about 5.8 degrees. Lock the requested stronger
        // bounce numerically so a later material or geometry pass cannot
        // quietly damp it back to an almost invisible twitch.
        let first_peak_degrees = sample_motion(profile, 1.8, 1.0, 0.0, 1.02).0 * 45.0;
        assert!(
            (5.7..=6.0).contains(&first_peak_degrees),
            "unexpected first Hp rebound peak: {first_peak_degrees} degrees"
        );
    }

    #[test]
    fn deterministic_motion_sample_starts_in_the_source_aspect() {
        assert_eq!(
            sample_motion(MotionProfile::Linear, 1.0, 1.0, 0.0, 0.0),
            (1.0, 0.0)
        );
        let halfway = sample_motion(MotionProfile::Linear, 1.0, 0.0, 1.0, 0.5);
        assert!((halfway.0 - 0.5).abs() < 1e-5);
    }

    #[test]
    fn box_corner_helper_covers_both_extrema() {
        let corners = box_corners(Vec3::new(-1.0, -2.0, -3.0), Vec3::new(4.0, 5.0, 6.0));
        assert!(corners.contains(&Vec3::new(-1.0, -2.0, -3.0)));
        assert!(corners.contains(&Vec3::new(4.0, 5.0, 6.0)));
        assert_eq!(corners.len(), 8);
    }
}
