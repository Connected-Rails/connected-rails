//! Persisted user settings — Bevy's own `bevy::settings`, one TOML file in the
//! platform's preferences directory (`%APPDATA%`, `~/.config`, …).
//!
//! Three resources, three sections in `settings.toml`: `[graphics]`, `[audio]`,
//! `[gameplay]`. The settings page of the menu (`menu.rs`) writes the resources,
//! the `apply_*` systems mirror them onto the window, the mixer and the language,
//! and every change writes the file on the i/o pool. A synchronous save on `AppExit`
//! on top of that, so a change made in the very last frame still lands.
//!
//! **Every setting applies the moment it is dialled.** Language, volume, HUD and look
//! sensitivity are read where they are used; window mode and vertical sync go onto the
//! window; view distance, shadows, bloom, anti-aliasing and the shadow and mist quality
//! reach into a running scene through `apply_scene`; the texture quality generates the
//! ground textures again into the handles the terrain already holds. A setting that needs
//! a restart is an excuse, and it would go stale the moment there is a pause menu.

use std::sync::OnceLock;

use bevy::anti_alias::fxaa::{Fxaa, Sensitivity};
use bevy::anti_alias::smaa::{Smaa, SmaaPreset};
use bevy::ecs::system::EntityCommands;
use bevy::light::DirectionalLightShadowMap;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::view::Msaa;
use bevy::settings::{
    ReflectSettingsGroup, SaveSettings, SaveSettingsSync, SettingsGroup, SettingsPlugin,
};
use bevy::window::{MonitorSelection, PresentMode, PrimaryWindow, VideoModeSelection, WindowMode};
use std::time::{Duration, Instant};

use crate::ViewDistance;
use crate::streaming::TerrainStreamer;
use crate::ui::CabCamera;

/// Names the directory the settings file lives in — reverse domain name, so it
/// cannot collide with another app's.
const APP_ID: &str = "dev.vanlueck.connected-rails";

/// Terrain load and draw radius: smallest, largest, one step of the menu [m].
pub const VIEW_DISTANCE: (f32, f32, f32) = (1_000.0, 12_000.0, 500.0);
/// Master volume: smallest, largest, one step (0 … 1).
pub const VOLUME: (f32, f32, f32) = (0.0, 1.0, 0.05);
/// Mouse look sensitivity as a factor on the built-in speed.
pub const LOOK_SPEED: (f32, f32, f32) = (0.2, 3.0, 0.1);
/// Frame cap: smallest, largest, one step \[1/s\]. The top step is not a rate at all
/// but *no cap* — a slider that runs into "unlimited" says what it does with one row,
/// which two rows could not.
pub const MAX_FPS: (f32, f32, f32) = (30.0, 250.0, 10.0);

/// Everything that costs frames.
#[derive(Resource, SettingsGroup, Reflect, Clone, Debug, PartialEq)]
#[reflect(Resource, SettingsGroup, Default)]
#[settings_group(group = "graphics")]
pub struct Graphics {
    /// Terrain load and draw radius [m] — read by `setup` when the run starts.
    pub view_distance: f32,
    /// Windowed, borderless over the monitor, or exclusive fullscreen.
    ///
    /// A settings file from before this was three steps carries `fullscreen = true`; the
    /// loader keeps the default for a field it cannot read, so such a file comes up
    /// windowed and the choice has to be made once more.
    pub window: WindowStyle,
    pub vsync: bool,
    /// Frames a second the simulator holds itself to; [`MAX_FPS`]'s top step means it
    /// does not. Vertical sync only ever offers the monitor's rate, which is not the
    /// same question.
    pub max_fps: f32,
    /// Bloom on the camera: what makes lamp lenses glow after dark.
    pub bloom: bool,
    /// Shadow maps of the sun. Off is a large win on weak hardware.
    pub shadows: bool,
    /// Edge length of the sun's shadow map — where a shadow's steps come from.
    pub shadow_quality: Quality,
    /// Ground mist as a volume, with the sun's shafts through it — half a
    /// millisecond a frame in foggy weather, and nothing at all in clear.
    pub mist: bool,
    /// Steps of the raymarch through that volume — the whole cost of the mist.
    pub mist_quality: Quality,
    /// Size and filtering of the ground textures the simulator generates.
    pub texture_quality: Quality,
    /// Which anti-aliasing runs on the cab camera.
    pub anti_aliasing: AntiAliasing,
    /// How hard it works: the sample count for MSAA, the preset for SMAA, the edge
    /// threshold for FXAA. One knob for all three, because the question the player
    /// is answering is the same one.
    pub aa_quality: Quality,
}

impl Default for Graphics {
    fn default() -> Self {
        Self {
            view_distance: 4_000.0,
            window: WindowStyle::Windowed,
            vsync: true,
            max_fps: MAX_FPS.1,
            bloom: true,
            shadows: true,
            shadow_quality: Quality::Medium,
            mist: true,
            mist_quality: Quality::Medium,
            texture_quality: Quality::Medium,
            // What Bevy does without being asked, so a settings file from before this
            // page had the row comes up looking the way it did.
            anti_aliasing: AntiAliasing::Msaa,
            aa_quality: Quality::Medium,
        }
    }
}

/// How the edges are smoothed. Three techniques and off, in the order they cost:
/// FXAA is one cheap pass over the finished picture, SMAA a sharper one, MSAA
/// resolves the geometry itself and is the only one that costs memory bandwidth.
///
/// Temporal anti-aliasing is deliberately absent: it needs motion vectors out of the
/// prepass, and the rain and snow fields draw without one (`precipitation.rs`), so
/// every drop would smear a trail across the windscreen.
#[derive(Reflect, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[reflect(Default, PartialEq)]
pub enum AntiAliasing {
    Off,
    Fxaa,
    Smaa,
    #[default]
    Msaa,
}

impl AntiAliasing {
    const ORDER: [AntiAliasing; 4] = [
        AntiAliasing::Off,
        AntiAliasing::Fxaa,
        AntiAliasing::Smaa,
        AntiAliasing::Msaa,
    ];

    /// One step of the settings page's chevrons.
    pub fn cycle(self, dir: i32) -> Self {
        cycle(&Self::ORDER, self, dir)
    }

    /// What this technique is called on the settings page.
    pub fn key(self) -> &'static str {
        match self {
            AntiAliasing::Off => "set-aa-off",
            AntiAliasing::Fxaa => "set-aa-fxaa",
            AntiAliasing::Smaa => "set-aa-smaa",
            AntiAliasing::Msaa => "set-aa-msaa",
        }
    }

    /// What the quality step under it is called: MSAA counts samples and says so, the
    /// other two only work harder, and off has nothing to be dialled.
    pub fn level_key(self, quality: Quality) -> &'static str {
        match (self, quality) {
            (AntiAliasing::Off, _) => "set-quality-none",
            (AntiAliasing::Msaa, Quality::Low) => "set-aa-2x",
            (AntiAliasing::Msaa, Quality::Medium) => "set-aa-4x",
            (AntiAliasing::Msaa, Quality::High) => "set-aa-8x",
            _ => quality.key(),
        }
    }
}

/// Three steps of "how hard does this work", shared by everything on the page that has
/// a quality rather than a value: the shadows, the mist, the ground textures and the
/// anti-aliasing. One scale, so a player who has learnt what Medium costs on one row
/// knows what it means on the next.
#[derive(Reflect, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[reflect(Default, PartialEq)]
pub enum Quality {
    Low,
    #[default]
    Medium,
    High,
}

impl Quality {
    const ORDER: [Quality; 3] = [Quality::Low, Quality::Medium, Quality::High];

    /// One step of the settings page's chevrons.
    pub fn cycle(self, dir: i32) -> Self {
        cycle(&Self::ORDER, self, dir)
    }

    /// What this step is called on the settings page.
    pub fn key(self) -> &'static str {
        match self {
            Quality::Low => "set-quality-low",
            Quality::Medium => "set-quality-medium",
            Quality::High => "set-quality-high",
        }
    }

    /// Edge length of the sun's shadow map \[texels\]. Four times the texels per step,
    /// which is what a doubled edge costs.
    fn shadow_map(self) -> usize {
        match self {
            Quality::Low => 1_024,
            Quality::Medium => 2_048,
            Quality::High => 4_096,
        }
    }

    /// Steps of the raymarch through the mist volume. Bevy's own default is 64; half of
    /// it is enough for a layer that is only a few hundred metres deep.
    fn mist_steps(self) -> u32 {
        match self {
            Quality::Low => 16,
            Quality::Medium => 32,
            Quality::High => 64,
        }
    }

    /// Edge length of a generated ground texture \[texels\], and how far the sampler
    /// follows it into the distance.
    fn ground_texture(self) -> (u32, u16) {
        match self {
            Quality::Low => (128, 1),
            Quality::Medium => (256, 4),
            Quality::High => (512, 16),
        }
    }

    fn msaa(self) -> Msaa {
        match self {
            Quality::Low => Msaa::Sample2,
            Quality::Medium => Msaa::Sample4,
            Quality::High => Msaa::Sample8,
        }
    }

    fn sensitivity(self) -> Sensitivity {
        match self {
            Quality::Low => Sensitivity::Low,
            Quality::Medium => Sensitivity::Medium,
            Quality::High => Sensitivity::High,
        }
    }

    fn preset(self) -> SmaaPreset {
        match self {
            Quality::Low => SmaaPreset::Low,
            Quality::Medium => SmaaPreset::Medium,
            Quality::High => SmaaPreset::High,
        }
    }
}

/// How the window sits on the screen.
#[derive(Reflect, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[reflect(Default, PartialEq)]
pub enum WindowStyle {
    #[default]
    Windowed,
    /// Borderless over the whole monitor, at the monitor's own resolution — what most
    /// people mean by fullscreen, and the one that alt-tabs without a mode change.
    Borderless,
    /// Exclusive fullscreen: the monitor is put into the video mode it is already in and
    /// handed to the program alone.
    Fullscreen,
}

impl WindowStyle {
    const ORDER: [WindowStyle; 3] = [
        WindowStyle::Windowed,
        WindowStyle::Borderless,
        WindowStyle::Fullscreen,
    ];

    pub fn cycle(self, dir: i32) -> Self {
        cycle(&Self::ORDER, self, dir)
    }

    pub fn key(self) -> &'static str {
        match self {
            WindowStyle::Windowed => "set-window-windowed",
            WindowStyle::Borderless => "set-window-borderless",
            WindowStyle::Fullscreen => "set-window-fullscreen",
        }
    }
}

/// One step through a fixed list of named options, wrapping.
fn cycle<T: Copy + PartialEq>(order: &[T], value: T, dir: i32) -> T {
    let at = order.iter().position(|v| *v == value).unwrap_or(0) as i32;
    order[(at + dir).rem_euclid(order.len() as i32) as usize]
}

/// Puts the chosen anti-aliasing on a camera. Called where the camera is spawned
/// (`setup`) and again from `apply_scene` whenever the setting is dialled — the three
/// techniques are three different components, so switching means removing the other two.
pub fn apply_anti_aliasing(camera: &mut EntityCommands, graphics: &Graphics) {
    // MSAA is a required component of every camera, so it is set rather than added:
    // the other two techniques want it off, or they smooth an already smoothed edge.
    camera.insert(match graphics.anti_aliasing {
        AntiAliasing::Msaa => graphics.aa_quality.msaa(),
        _ => Msaa::Off,
    });
    match graphics.anti_aliasing {
        AntiAliasing::Fxaa => {
            camera.remove::<Smaa>().insert(Fxaa {
                enabled: true,
                edge_threshold: graphics.aa_quality.sensitivity(),
                edge_threshold_min: graphics.aa_quality.sensitivity(),
            });
        }
        AntiAliasing::Smaa => {
            camera.remove::<Fxaa>().insert(Smaa {
                preset: graphics.aa_quality.preset(),
            });
        }
        _ => {
            camera.remove::<Fxaa>().remove::<Smaa>();
        }
    }
}

/// The mixer. One knob for now — the sim has a single output bus.
#[derive(Resource, SettingsGroup, Reflect, Clone, Debug, PartialEq)]
#[reflect(Resource, SettingsGroup, Default)]
#[settings_group(group = "audio")]
pub struct Audio {
    /// Linear master volume, 0 … 1.
    pub master: f32,
}

impl Default for Audio {
    fn default() -> Self {
        Self { master: 0.8 }
    }
}

/// How much of the head-up display is drawn (`hud.rs`). Three steps rather than a
/// switch, because the two reasons to turn a HUD off are different ones: driving by the
/// cab's own instruments still wants the train protection in view, and a photograph wants
/// nothing at all.
///
/// The step between them keeps what a driver *drives* by — the desk and the protection
/// lamps, plus anything that interrupts — and drops what is information rather than
/// driving: the run, the systems, the look-ahead. Cycled with F7 in the run and dialled
/// on the settings page.
#[derive(Reflect, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[reflect(Default, PartialEq)]
pub enum HudMode {
    #[default]
    Full,
    Reduced,
    Off,
}

impl HudMode {
    /// One step of F7 (`dir` +1) or of the settings page's chevrons.
    pub fn cycle(self, dir: i32) -> Self {
        const ORDER: [HudMode; 3] = [HudMode::Full, HudMode::Reduced, HudMode::Off];
        let at = ORDER.iter().position(|m| *m == self).unwrap_or(0) as i32;
        ORDER[(at + dir).rem_euclid(ORDER.len() as i32) as usize]
    }

    /// Is anything drawn at all?
    pub fn drawn(self) -> bool {
        self != HudMode::Off
    }

    /// Are the zones that inform rather than instrument drawn — the run, the systems and
    /// the look-ahead?
    pub fn informs(self) -> bool {
        self == HudMode::Full
    }

    /// What this step is called on the settings page.
    pub fn key(self) -> &'static str {
        match self {
            HudMode::Full => "set-hud-full",
            HudMode::Reduced => "set-hud-reduced",
            HudMode::Off => "set-hud-off",
        }
    }
}

/// Everything that is neither picture nor sound.
#[derive(Resource, SettingsGroup, Reflect, Clone, Debug, PartialEq)]
#[reflect(Resource, SettingsGroup, Default)]
#[settings_group(group = "gameplay")]
pub struct Gameplay {
    /// A code out of `i18n::LANGUAGES`; empty means whatever the system asks for.
    pub language: String,
    /// How much of the HUD is drawn while driving.
    ///
    /// A settings file from before this was three steps carries `hud = true`; the loader
    /// keeps the default for a field it cannot read, so such a file comes up on `Full`.
    pub hud: HudMode,
    /// Factor on the built-in mouse look speed.
    pub look_speed: f32,
}

impl Default for Gameplay {
    fn default() -> Self {
        Self {
            language: String::new(),
            hud: HudMode::Full,
            look_speed: 1.0,
        }
    }
}

/// Loads the settings and keeps window, mixer, language and the file in step.
///
/// Add this **before** `DefaultPlugins`: loading happens while the plugin is built,
/// so the stored language is in place by the time the window title is translated and
/// the stored window mode by the time the window is created.
pub fn plugin(app: &mut App) {
    // Ask i18n what the system wants before a stored choice can override it — that
    // answer is what "System" means on the settings page.
    let _ = system_language();
    app.register_type::<Graphics>()
        .register_type::<Audio>()
        .register_type::<Gameplay>()
        .register_type::<HudMode>()
        .register_type::<AntiAliasing>()
        .register_type::<Quality>()
        .register_type::<WindowStyle>()
        .add_plugins(SettingsPlugin::new(APP_ID));
    // `TRAINSIM_LANG` stays the outermost override: a scripted or CI run sets it and
    // must not be steered by whatever the user last picked in the menu. Without it the
    // stored choice applies, and the menu switches the language itself from there on.
    if std::env::var_os("TRAINSIM_LANG").is_none() {
        apply_language(&app.world().resource::<Gameplay>().language);
    }
    app.add_systems(
        Update,
        (
            apply_window.run_if(resource_changed::<Graphics>),
            apply_scene.run_if(resource_changed::<Graphics>),
            // Regenerating three textures is not something to do on a frame that only
            // moved a slider, so this one waits for its own setting.
            apply_ground_textures.run_if(texture_quality_changed),
            apply_audio.run_if(resource_changed::<Audio>),
            save_changed.run_if(
                resource_changed::<Graphics>
                    .or_else(resource_changed::<Audio>)
                    .or_else(resource_changed::<Gameplay>),
            ),
        ),
    )
    // Last in the frame, so what it sleeps through is a finished one.
    .add_systems(Last, (save_on_exit, limit_frame_rate));
}

/// True on the frame the texture quality is dialled, and on no other.
fn texture_quality_changed(graphics: Res<Graphics>, mut last: Local<Option<Quality>>) -> bool {
    let changed = *last != Some(graphics.texture_quality) && last.is_some();
    *last = Some(graphics.texture_quality);
    changed
}

/// The language the operating system asks for, frozen at startup.
pub fn system_language() -> &'static str {
    static LANGUAGE: OnceLock<String> = OnceLock::new();
    LANGUAGE.get_or_init(i18n::language)
}

/// Switches the interface language; an empty code means the system's.
pub fn apply_language(code: &str) {
    i18n::set_language(if code.is_empty() {
        system_language()
    } else {
        code
    });
}

/// Size and filtering of the generated ground textures for the current setting — read
/// by `setup` when the run starts and by `apply_ground_textures` whenever it changes.
pub fn ground_quality(graphics: &Graphics) -> world_render::GroundQuality {
    let (size, anisotropy) = graphics.texture_quality.ground_texture();
    world_render::GroundQuality { size, anisotropy }
}

/// Generates the ground textures again into the handles the terrain material already
/// holds, so a dialled texture quality reaches the terrain standing on screen.
///
/// The season is the one the run was built with (`setup`), so the ground keeps the month
/// it started in. Without a running simulation there is no season and no terrain either.
fn apply_ground_textures(
    graphics: Res<Graphics>,
    sim: Option<Res<crate::SimResource>>,
    mut images: ResMut<Assets<Image>>,
    materials: Res<Assets<world_render::TerrainMaterial>>,
) {
    let Some(sim) = sim else {
        return;
    };
    let season = world_render::Season::on(sim.0.start.month, sim.0.start.day);
    world_render::retexture_ground(&mut images, &materials, season, ground_quality(&graphics));
}

/// Window mode for the current setting — also used to create the window.
pub fn window_mode(graphics: &Graphics) -> WindowMode {
    match graphics.window {
        WindowStyle::Windowed => WindowMode::Windowed,
        WindowStyle::Borderless => WindowMode::BorderlessFullscreen(MonitorSelection::Current),
        // The mode the monitor is already in: a simulator has nothing to gain from
        // changing the desktop resolution, and everything to lose when it crashes in it.
        //
        // `Primary` rather than `Current` because this also creates the window, and a
        // window that does not exist yet is on no monitor — where borderless shrugs and
        // takes the one it lands on, exclusive fullscreen has to name a monitor and Bevy
        // panics when it cannot.
        WindowStyle::Fullscreen => {
            WindowMode::Fullscreen(MonitorSelection::Primary, VideoModeSelection::Current)
        }
    }
}

/// Presentation mode for the current setting — also used to create the window.
pub fn present_mode(graphics: &Graphics) -> PresentMode {
    if graphics.vsync {
        PresentMode::AutoVsync
    } else {
        PresentMode::AutoNoVsync
    }
}

/// Fullscreen and vertical sync onto the live window. Both are compared first: an
/// assignment marks `Window` changed, and winit answers a changed window mode by
/// recreating the surface even when nothing about it actually moved.
fn apply_window(graphics: Res<Graphics>, mut window: Query<&mut Window, With<PrimaryWindow>>) {
    let Ok(mut window) = window.single_mut() else {
        return;
    };
    let mode = window_mode(&graphics);
    if window.mode != mode {
        window.mode = mode;
    }
    let present = present_mode(&graphics);
    if window.present_mode != present {
        window.present_mode = present;
    }
}

/// View distance, bloom and anti-aliasing into a scene that is already running. Every parameter is
/// optional because none of it exists while the menu is up — the world is built on
/// leaving it.
///
/// Shadows need nothing here: `update_daylight` re-reads the setting every frame anyway,
/// because it also has to switch them off at night and under an overcast sky.
fn apply_scene(
    graphics: Res<Graphics>,
    mut commands: Commands,
    view: Option<ResMut<ViewDistance>>,
    streamer: Option<ResMut<TerrainStreamer>>,
    quality: Option<ResMut<world_render::mist::Quality>>,
    cameras: Query<(Entity, Has<Bloom>), With<CabCamera>>,
) {
    // The sun's shadow map is a resource of Bevy's own, rebuilt from it every frame.
    commands.insert_resource(DirectionalLightShadowMap {
        size: graphics.shadow_quality.shadow_map(),
    });
    // Like the two above: none of this exists while the menu is up.
    if let Some(mut quality) = quality {
        quality.volumetric = graphics.mist;
        quality.steps = graphics.mist_quality.mist_steps();
    }
    if let Some(mut view) = view {
        view.0 = graphics.view_distance;
    }
    if let Some(mut streamer) = streamer {
        streamer.set_load_radius(f64::from(graphics.view_distance));
    }
    for (camera, has_bloom) in &cameras {
        match (graphics.bloom, has_bloom) {
            (true, false) => {
                commands.entity(camera).insert(Bloom::NATURAL);
            }
            (false, true) => {
                commands.entity(camera).remove::<Bloom>();
            }
            _ => {}
        }
        apply_anti_aliasing(&mut commands.entity(camera), &graphics);
    }
}

/// Holds the program to [`Graphics::max_fps`]. Bevy paces nothing by itself and vertical
/// sync only ever offers the monitor's own rate — this is what lets a laptop sit at 60
/// while the panel runs at 144, and what keeps a menu from spinning a GPU at 900 frames a
/// second.
///
/// Runs last in the frame and sleeps to the next slot rather than for a fixed span, so a
/// frame that overran does not push the following one out with it. Falling behind resets
/// the schedule instead of catching up: a burst of unpaced frames after a hitch is worse
/// than the hitch.
///
/// ponytail: `thread::sleep` — a millisecond of jitter on a 16 ms frame. Spin the last
/// millisecond the day someone can see it.
fn limit_frame_rate(graphics: Res<Graphics>, mut slot: Local<Option<Instant>>) {
    if graphics.max_fps >= MAX_FPS.1 {
        *slot = None;
        return;
    }
    let budget = Duration::from_secs_f32(1.0 / graphics.max_fps.max(1.0));
    let now = Instant::now();
    if let Some(wait) = slot.and_then(|slot| slot.checked_duration_since(now)) {
        std::thread::sleep(wait);
    }
    *slot = Some(next_slot(now, *slot, budget));
}

/// When the frame after this one may start. Early means the slot simply moves on by the
/// budget, so the schedule keeps its rhythm; late means it starts again from now, because
/// firing off the frames that were missed makes a hitch worse rather than better.
fn next_slot(now: Instant, slot: Option<Instant>, budget: Duration) -> Instant {
    match slot {
        Some(slot) if slot > now => slot + budget,
        _ => now + budget,
    }
}

/// The mixer only exists once the output device opened — without one the simulator runs
/// silent and the slider on the settings page has nothing to move.
fn apply_audio(mixer: Option<ResMut<crate::audio::Audio>>, audio: Res<Audio>) {
    if let Some(mut mixer) = mixer {
        mixer.set_master(audio.master);
    }
}

/// A changed setting is written out right away, on the i/o pool.
///
/// `Always` rather than `IfChanged`, here as on exit: what changed is already decided
/// by the run condition above and by the fact that we are exiting. `IfChanged` asks
/// `bevy-settings` to compare the resources' change ticks itself, and on 0.19.0 that
/// comparison never comes out true for a settings resource, so nothing is ever written.
///
/// ponytail: that also rules out `SaveSettingsDeferred`, whose timer ends in an
/// `IfChanged`. No loss — every setting here moves on a key press, not on a dragged
/// slider, so there is no burst of changes for a debounce to swallow.
fn save_changed(mut commands: Commands) {
    commands.queue(SaveSettings::Always);
}

fn save_on_exit(mut exits: MessageReader<AppExit>, mut commands: Commands) {
    if exits.read().next().is_some() {
        commands.queue(SaveSettingsSync::Always);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ranges the menu steps through have to contain their own default, or the
    /// first press of ← or → would jump the value somewhere else entirely.
    #[test]
    fn defaults_lie_inside_the_ranges() {
        let graphics = Graphics::default();
        assert!((VIEW_DISTANCE.0..=VIEW_DISTANCE.1).contains(&graphics.view_distance));
        assert!((VOLUME.0..=VOLUME.1).contains(&Audio::default().master));
        assert!((LOOK_SPEED.0..=LOOK_SPEED.1).contains(&Gameplay::default().look_speed));
    }

    /// The view distance reaches a scene that is already running. Without this the
    /// setting would only be read while the world is built, and the menu would be back to
    /// promising an effect "on the next run".
    #[test]
    fn the_view_distance_reaches_a_running_scene() {
        let mut app = App::new();
        app.insert_resource(Graphics::default())
            .insert_resource(ViewDistance(0.0))
            .add_systems(Update, apply_scene.run_if(resource_changed::<Graphics>));
        app.update();
        assert_eq!(
            app.world().resource::<ViewDistance>().0,
            Graphics::default().view_distance,
            "the first frame hands the stored value over"
        );

        app.world_mut().resource_mut::<Graphics>().view_distance = 9_000.0;
        app.update();
        assert_eq!(app.world().resource::<ViewDistance>().0, 9_000.0);
    }

    /// The frame cap keeps its rhythm while it is met and starts afresh when it is
    /// missed — a hitch must not be followed by a burst of frames making up for it.
    #[test]
    fn a_missed_frame_slot_is_not_made_up_for() {
        let budget = Duration::from_millis(10);
        let now = Instant::now();

        // On time, with 4 ms of the slot left: the next one is 10 ms after *the slot*,
        // not after now, so the schedule does not drift with every early frame.
        let slot = now + Duration::from_millis(4);
        assert_eq!(next_slot(now, Some(slot), budget), slot + budget);

        // Late by 30 ms: the three slots that went by are gone, not owed.
        let slot = now - Duration::from_millis(30);
        assert_eq!(next_slot(now, Some(slot), budget), now + budget);

        // The first frame has no slot behind it.
        assert_eq!(next_slot(now, None, budget), now + budget);
    }

    /// An empty language code is the system's, not English — the menu shows it as
    /// "System" and must not silently turn a German desktop into an English one.
    #[test]
    fn the_empty_language_code_is_the_system_language() {
        apply_language("de");
        assert_eq!(i18n::language(), "de");
        apply_language("");
        assert_eq!(i18n::language(), system_language());
    }
}
