//! The material of the rain and snow field (plan 14.1) — see
//! `precipitation.wgsl` for what it draws. The field itself, its fall and its
//! slant into the relative wind, belong to whoever has a camera and a train:
//! `app::update_precipitation`.

use bevy::asset::embedded_asset;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;
use sim_core::weather::{Precip, Weather};

pub(crate) fn plugin(app: &mut App) {
    embedded_asset!(app, "precipitation.wgsl");
    app.add_plugins(MaterialPlugin::<PrecipitationMaterial>::default());
}

/// What one field of drops is told.
#[derive(ShaderType, Debug, Clone, Copy, Default)]
pub struct PrecipitationParams {
    /// x = intensity 0…1, y = 1 for snow, z = opacity, w = streak length.
    pub state: Vec4,
    /// rgb = the light the drops carry, w = distance the near fade ends at \[m\].
    pub light: Vec4,
    /// xyz = direction towards the sun, w = daylight 0…1.
    pub sun: Vec4,
}

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone, Default)]
pub struct PrecipitationMaterial {
    #[uniform(0)]
    pub params: PrecipitationParams,
}

impl Material for PrecipitationMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://world_render/precipitation.wgsl".into()
    }

    /// Blended towards the colour of the air, not added. A drop is a lens: it
    /// carries the sky's own light, so against a bright sky it all but vanishes
    /// and against a dark cutting it stands out — additive streaks are always
    /// *brighter* than what is behind them, which is the flat white overlay that
    /// dates a rain effect by a decade.
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    fn enable_shadows() -> bool {
        false
    }

    fn enable_prepass() -> bool {
        false
    }

    /// A drop is two quads crossed at right angles, and half of those faces away
    /// from any given camera. Culled, the field draws as a dashed line.
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// Luminance of the night sky a drop carries when nothing else lights it
/// \[cd/m²\] — the same overcast night as the clouds' floor.
const NIGHT_SKY: f32 = 0.01;

/// What the weather makes of a field of drops: how many of them are alive, how
/// long they are drawn and how brightly. `artificial` is the vehicle's own
/// light on the ground \[lx\] (`Sky::artificial`).
///
/// `near` is the layer of a few big out-of-focus drops close to the lens — the
/// one that actually sells rain, which is why it is thinned to a tenth and drawn
/// dimmer and shorter than the field behind it.
pub fn params(
    weather: Weather,
    daylight: f32,
    artificial: f32,
    snow_field: bool,
    near: bool,
) -> PrecipitationParams {
    let falling = weather.precip == Precip::Snow;
    // A field only draws what matches it: the snow mesh stays empty in the rain.
    let intensity = if (weather.precip == Precip::None) || (falling != snow_field) {
        0.0
    } else {
        // 4 mm/h is a normal rain and fills the field; a downpour saturates it.
        (weather.rate / 4.0).clamp(0.0, 1.0)
    };
    // How solidly a drop covers what is behind it. Rain is nearly clear; a
    // flake is not.
    // A drizzle is a shimmer; a downpour is a curtain. The per-streak coverage
    // follows the rate, on top of the count doing the same.
    let opacity = if falling {
        0.8
    } else {
        (0.11 + weather.rate * 0.016).clamp(0.10, 0.32)
    };
    PrecipitationParams {
        state: Vec4::new(
            intensity * if near { 0.12 } else { 1.0 },
            f32::from(u8::from(falling)),
            opacity * if near { 0.8 } else { 1.0 },
            // Heavier rain falls faster and draws longer; a flake fills its quad.
            if falling {
                1.0
            } else {
                (0.35 + weather.rate / 20.0).min(0.95)
            },
        ),
        // Drops carry the light of the sky they fall through — calibrated near
        // the horizon sky's own luminance, so a streak against the sky is a
        // shimmer and the same streak against a dark cutting is a line.
        // A streak is a lens that averages the whole sky behind it — its mean
        // luminance sits *below* the bright horizon, which is why real rain is a
        // grey shimmer and never a white line. At night the sky is a hundredth
        // of a candela and the streaks are what the headlights catch
        // (`Sky::artificial`, lux on the track, over π as a white thing in it).
        light: (Vec3::new(0.66, 0.69, 0.75)
            * (crate::sky::SUN_ILLUMINANCE * 0.16 * daylight
                + NIGHT_SKY
                + artificial / std::f32::consts::PI))
            .extend(1.6),
        sun: Vec4::new(0.0, 1.0, 0.0, daylight),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::weather::Preset;

    #[test]
    fn each_field_draws_only_what_matches_it() {
        let rain = Preset::Rain.weather();
        assert!(
            params(rain, 1.0, 0.0, false, false).state.x > 0.0,
            "rain falls"
        );
        assert_eq!(
            params(rain, 1.0, 0.0, true, false).state.x,
            0.0,
            "not as snow"
        );

        let snow = Preset::Snow.weather();
        assert!(
            params(snow, 1.0, 0.0, true, false).state.x > 0.0,
            "snow falls"
        );
        assert_eq!(
            params(snow, 1.0, 0.0, false, false).state.x,
            0.0,
            "not as rain"
        );

        let clear = Preset::Clear.weather();
        assert_eq!(params(clear, 1.0, 0.0, false, false).state.x, 0.0);
        assert_eq!(params(clear, 1.0, 0.0, true, false).state.x, 0.0);
    }

    #[test]
    fn a_drizzle_is_thinner_than_a_downpour() {
        let drizzle = params(Preset::Drizzle.weather(), 1.0, 0.0, false, false)
            .state
            .x;
        let rain = params(Preset::Rain.weather(), 1.0, 0.0, false, false)
            .state
            .x;
        let storm = params(Preset::Storm.weather(), 1.0, 0.0, false, false)
            .state
            .x;
        assert!(drizzle < rain && rain <= storm, "{drizzle} {rain} {storm}");
        assert_eq!(storm, 1.0, "a downpour fills the field");
    }

    #[test]
    fn the_near_layer_is_a_few_big_drops() {
        let far = params(Preset::Rain.weather(), 1.0, 0.0, false, false);
        let near = params(Preset::Rain.weather(), 1.0, 0.0, false, true);
        assert!(near.state.x < far.state.x * 0.2, "far fewer of them");
        assert!(near.state.z < far.state.z, "and dimmer");
    }
}
