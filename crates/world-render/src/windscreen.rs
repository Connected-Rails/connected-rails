//! Rain on the cab glass (plan 14.1) — the material behind `windscreen.wgsl`.
//!
//! A vehicle names its panes in its own file
//! ([`CabSpec::windscreen`](sim_core::cab::CabSpec::windscreen)); the app binds
//! those nodes and swaps their material for one of these (`app::models`). What
//! the pane looks like dry is still the model's business: this is an extension
//! over whatever `StandardMaterial` the glTF brought.
//!
//! **Multiplayer.** The wiper is a `CabInputs` lever like any other, so it
//! travels as a setpoint; everything here is drawn from it.

use bevy::asset::embedded_asset;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use sim_core::cab::WiperSpec;
use sim_core::weather::Weather;

/// The pane's own material: what the model made it, plus the water on it.
pub type WindscreenMaterial = ExtendedMaterial<StandardMaterial, WindscreenExt>;

pub(crate) fn plugin(app: &mut App) {
    embedded_asset!(app, "windscreen.wgsl");
    app.add_plugins(MaterialPlugin::<WindscreenMaterial>::default());
}

/// What one pane is told.
#[derive(ShaderType, Debug, Clone, Copy, Default)]
pub struct WindscreenParams {
    /// x = water on the pane 0-1, y = rain rate [mm/h], z = speed [m/s],
    /// w = simulation time [s] - the clock the 3D blade is posed by.
    pub state: Vec4,
    /// x = wiper period [s], y = duty (the share of it the blade moves),
    /// z = 1 while the wiper is engaged on this pane, w = film regrow time [s].
    pub wiper: Vec4,
    /// xy = the blade's pivot in pane UV, zw = pane size [m].
    pub geom: Vec4,
    /// x = blade rest angle [rad], y = sweep [rad], z/w = inner and outer
    /// radius of the swept annulus [m].
    pub blade: Vec4,
}

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone, Default)]
pub struct WindscreenExt {
    #[uniform(100)]
    pub params: WindscreenParams,
}

impl MaterialExtension for WindscreenExt {
    fn fragment_shader() -> ShaderRef {
        "embedded://world_render/windscreen.wgsl".into()
    }
}

/// What the weather, the train and the wiper put on one pane.
///
/// The shader reconstructs the blade's whole sweep from `time` and the mode's
/// period - the same triangle the 3D blade is posed by - so the cleared arc and
/// the drawn blade cannot drift apart. `wiper` is `None` on a pane with no
/// blade of its own.
pub fn params(
    weather: Weather,
    wetness: f32,
    speed: f32,
    time: f64,
    mode: u8,
    wiper: Option<&WiperSpec>,
) -> WindscreenParams {
    let rate = if weather.precip.is_liquid() {
        weather.rate
    } else {
        // Snow melts on a heated pane rather than lying on it, and sleet is
        // water by the time it runs.
        weather.rate * 0.35
    };
    let rate = if weather.precip == sim_core::weather::Precip::None {
        0.0
    } else {
        rate
    };
    // The sweep the modes drive (`app::models::wiper_position`): interval does
    // one sweep in the first third of five seconds, slow and fast run through.
    let (period, duty) = match mode {
        1 => (5.0, 1.0 / 3.0),
        2 => (1.0 / 0.45, 1.0),
        _ => (1.25, 1.0),
    };
    let engaged = wiper.is_some() && mode > 0;
    let spec = wiper.cloned().unwrap_or(WiperSpec {
        pivot: [0.5, 0.0],
        length: 0.6,
        rest_degrees: 0.0,
        sweep_degrees: 75.0,
        pane: [1.8, 1.1],
    });
    WindscreenParams {
        state: Vec4::new(wetness.clamp(0.0, 1.0), rate, speed.abs(), time as f32),
        wiper: Vec4::new(
            period,
            duty,
            f32::from(u8::from(engaged)),
            // The wiped arc fills back in as fast as the rain can wet it.
            (10.0 / (rate + 0.5)).clamp(1.5, 10.0),
        ),
        geom: Vec4::new(spec.pivot[0], spec.pivot[1], spec.pane[0], spec.pane[1]),
        blade: Vec4::new(
            spec.rest_degrees.to_radians(),
            spec.sweep_degrees.to_radians(),
            0.05,
            spec.length,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::weather::Preset;

    fn spec() -> WiperSpec {
        WiperSpec {
            pivot: [0.26, 0.05],
            length: 0.6,
            rest_degrees: 0.0,
            sweep_degrees: 75.0,
            pane: [1.84, 1.1],
        }
    }

    #[test]
    fn only_what_falls_lands_on_the_pane() {
        let dry = params(Preset::Clear.weather(), 0.0, 30.0, 0.0, 0, None);
        assert_eq!(dry.state.y, 0.0, "nothing falling, nothing running");

        let rain = params(Preset::Rain.weather(), 1.0, 30.0, 0.0, 0, None);
        assert_eq!(rain.state.y, Preset::Rain.weather().rate);
        assert_eq!(rain.state.x, 1.0);

        // Snow does not stand on the glass the way rain does.
        let snow = params(Preset::Snow.weather(), 1.0, 0.0, 0.0, 0, None);
        assert!(snow.state.y < Preset::Snow.weather().rate);
    }

    #[test]
    fn the_wiper_engages_only_with_a_blade_and_a_mode() {
        let w = Preset::Rain.weather();
        let s = spec();
        assert_eq!(params(w, 1.0, 0.0, 0.0, 0, Some(&s)).wiper.z, 0.0, "off");
        assert_eq!(params(w, 1.0, 0.0, 0.0, 2, None).wiper.z, 0.0, "no blade");
        let on = params(w, 1.0, 0.0, 0.0, 2, Some(&s));
        assert_eq!(on.wiper.z, 1.0);
        // Slow mode: the 0.45 Hz triangle of `wiper_position`, running through.
        assert!((on.wiper.x - 1.0 / 0.45).abs() < 1e-6);
        assert_eq!(on.wiper.y, 1.0);
        // The geometry reaches the shader in radians and metres.
        assert!((on.blade.y - 75.0f32.to_radians()).abs() < 1e-6);
        assert_eq!(on.geom.zw(), Vec2::new(1.84, 1.1));
    }

    #[test]
    fn heavier_rain_closes_the_wiped_arc_sooner() {
        let s = spec();
        let drizzle = params(Preset::Drizzle.weather(), 1.0, 0.0, 0.0, 3, Some(&s));
        let storm = params(Preset::Storm.weather(), 1.0, 0.0, 0.0, 3, Some(&s));
        assert!(storm.wiper.w < drizzle.wiper.w);
    }
}
