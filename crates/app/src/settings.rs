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
//! sensitivity are read where they are used; fullscreen and vertical sync go onto the
//! window; view distance, shadows and bloom reach into a running scene through
//! `apply_scene`. A setting that needs a restart is an excuse, and it would go stale the
//! moment there is a pause menu.

use std::sync::OnceLock;

use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::settings::{
    ReflectSettingsGroup, SaveSettings, SaveSettingsSync, SettingsGroup, SettingsPlugin,
};
use bevy::window::{MonitorSelection, PresentMode, PrimaryWindow, WindowMode};

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

/// Everything that costs frames.
#[derive(Resource, SettingsGroup, Reflect, Clone, Debug, PartialEq)]
#[reflect(Resource, SettingsGroup, Default)]
#[settings_group(group = "graphics")]
pub struct Graphics {
    /// Terrain load and draw radius [m] — read by `setup` when the run starts.
    pub view_distance: f32,
    pub fullscreen: bool,
    pub vsync: bool,
    /// Bloom on the camera: what makes lamp lenses glow after dark.
    pub bloom: bool,
    /// Shadow maps of the sun. Off is a large win on weak hardware.
    pub shadows: bool,
    /// Ground mist as a volume, with the sun's shafts through it — half a
    /// millisecond a frame in foggy weather, and nothing at all in clear.
    pub mist: bool,
}

impl Default for Graphics {
    fn default() -> Self {
        Self {
            view_distance: 4_000.0,
            fullscreen: false,
            vsync: true,
            bloom: true,
            shadows: true,
            mist: true,
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
            apply_audio.run_if(resource_changed::<Audio>),
            save_changed.run_if(
                resource_changed::<Graphics>
                    .or_else(resource_changed::<Audio>)
                    .or_else(resource_changed::<Gameplay>),
            ),
        ),
    )
    .add_systems(Last, save_on_exit);
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

/// Window mode for the current setting — also used to create the window.
pub fn window_mode(graphics: &Graphics) -> WindowMode {
    if graphics.fullscreen {
        WindowMode::BorderlessFullscreen(MonitorSelection::Current)
    } else {
        WindowMode::Windowed
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

/// View distance and bloom into a scene that is already running. Every parameter is
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
    // Like the two above: none of this exists while the menu is up.
    if let Some(mut quality) = quality {
        quality.volumetric = graphics.mist;
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
