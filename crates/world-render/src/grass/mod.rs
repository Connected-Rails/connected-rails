//! Meadow grass around the camera, generated and drawn by the GPU (plan ch. 14).
//!
//! The first version grew the meadow on the CPU: every 32 m cell within reach
//! of the camera became a mesh of a million vertices, built on the main thread
//! the frame the camera came near, uploaded whole, and drawn through the full
//! PBR fragment path three LOD levels deep. A train at line speed entered a new
//! row of cells every second, and every one of them was a visible hitch.
//!
//! Nothing of the meadow is built on the CPU any more. It works the way the
//! grass of a current console title does:
//!
//! 1. **A ground cache.** The terrain tiles around the camera — and the
//!    fields, roads and waters draped on them — are drawn once, top down,
//!    into a texture of heights and grass weights ([`ground`]). It is drawn
//!    again only when the camera has moved a good way or a tile has streamed
//!    in or out, so in a normal frame it costs nothing.
//! 2. **A scatter pass.** Each frame a compute shader walks a grid of 4 m
//!    patches around the camera. A patch outside the view frustum costs one
//!    box test; a patch inside lays out blades on a low-discrepancy sequence,
//!    thins them with distance, reads their feet off the ground cache, culls
//!    each blade against the frustum, and appends what survives to one of
//!    three instance lists ([`render`]).
//! 3. **Three indirect draws.** One draw per level of detail; the vertex
//!    shader bends each instance along a quadratic Bézier into the wind,
//!    tapers it, keeps it at least a pixel wide, and the fragment shader
//!    lights it with the same PBR, shadow, fog and weather path as the rest
//!    of the world.
//!
//! The thinning is what makes it seamless: every blade has a rank, and a
//! blade stands wherever its rank is below the density the distance asks
//! for. Coming closer adds blades and never moves one, the survivors widen
//! as their neighbours go so the sward keeps its cover, and a blade at the
//! threshold grows in rather than popping.
//!
//! **Multiplayer.** Nothing here is state. Every blade is a function of the
//! ground and a hash of its own position.

mod ground;
mod render;

use bevy::asset::embedded_asset;
use bevy::camera::RenderTarget;
use bevy::ecs::query::QueryItem;
use bevy::prelude::*;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::sync_component::SyncComponent;
use fields::CropClass;
use fields::phenology;

use crate::{Season, WorldView, sky::Sky, weather::WeatherParams};

/// What the graphics settings decide about the meadow.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct GrassRenderSettings {
    pub enabled: bool,
    /// How far from the camera the last blade stands \[m\].
    pub range: f32,
    /// Scale on the stand's density, 1 being the authored meadow.
    pub density: f32,
}

impl Default for GrassRenderSettings {
    fn default() -> Self {
        Self::new(true, 220.0, 1.0)
    }
}

impl GrassRenderSettings {
    pub fn new(enabled: bool, range: f32, density: f32) -> Self {
        Self {
            enabled,
            range: range.clamp(40.0, 400.0),
            density: density.clamp(0.1, 1.5),
        }
    }
}

/// A surface the ground cache is drawn from.
///
/// The terrain carries the grass weight of its splat in the red vertex
/// colour; everything draped on it cuts a hole, so no blade stands in a
/// wheat field, on a carriageway or in a lake.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroundSurface {
    /// Grass grows where the splat says.
    Terrain,
    /// Nothing grows here.
    Excluded,
}

/// A [`GroundSurface`] as the render world sees it: the mesh, where it
/// stands this frame, and whether it grows or excludes.
#[derive(Component, Clone, Debug)]
pub struct ExtractedGroundSurface {
    pub mesh: AssetId<Mesh>,
    pub world_from_local: Mat4,
    pub excluded: bool,
}

impl SyncComponent for GroundSurface {
    type Target = ExtractedGroundSurface;
}

impl ExtractComponent for GroundSurface {
    type QueryData = (
        &'static GroundSurface,
        &'static Mesh3d,
        &'static GlobalTransform,
    );
    type QueryFilter = ();
    type Out = ExtractedGroundSurface;

    fn extract_component(
        (surface, mesh, transform): QueryItem<'_, '_, Self::QueryData>,
    ) -> Option<Self::Out> {
        Some(ExtractedGroundSurface {
            mesh: mesh.id(),
            world_from_local: transform.to_matrix(),
            excluded: *surface == GroundSurface::Excluded,
        })
    }
}

/// The camera the meadow is scattered around — the one that draws the world,
/// see [`crate::draws_the_world`]. Set every frame by [`mark_view`].
#[derive(Component, Clone, Copy, Default, ExtractComponent)]
pub struct GrassView;

/// The one entity the indirect draws hang on. Bevy's render phases want an
/// entity per item; the meadow is one item.
#[derive(Component, Clone, Copy, Default, ExtractComponent)]
pub struct GrassRenderer;

/// Everything the render world needs from the main world besides the
/// surfaces and the camera: the settings, the weather, and the day.
#[derive(Resource, Clone, Copy, Debug, PartialEq, ExtractResource)]
pub struct GrassEnvironment {
    pub settings: GrassRenderSettings,
    pub weather: WeatherParams,
    pub season: Season,
    /// The stand's height today \[m\] — the grassland phenology, so a meadow
    /// is short after the cut and long before it.
    pub height: f32,
}

impl Default for GrassEnvironment {
    fn default() -> Self {
        Self {
            settings: GrassRenderSettings::default(),
            weather: WeatherParams::default(),
            season: Season::default(),
            height: 0.22,
        }
    }
}

pub(crate) fn plugin(app: &mut App) {
    embedded_asset!(app, "ground.wgsl");
    embedded_asset!(app, "scatter.wgsl");
    embedded_asset!(app, "blades.wgsl");
    app.init_resource::<GrassRenderSettings>()
        .init_resource::<GrassEnvironment>()
        .add_plugins((
            ExtractComponentPlugin::<GroundSurface>::default(),
            ExtractComponentPlugin::<GrassView>::default(),
            ExtractComponentPlugin::<GrassRenderer>::default(),
            ExtractResourcePlugin::<GrassEnvironment>::default(),
        ))
        .add_systems(Startup, spawn_renderer)
        .add_systems(PostUpdate, (mark_view, feed_environment));
    render::plugin(app);
}

fn spawn_renderer(mut commands: Commands) {
    commands.spawn((crate::Persistent, GrassRenderer, Name::new("meadow grass")));
}

/// Puts [`GrassView`] on the camera that draws the world, and on no other.
#[allow(clippy::type_complexity)]
fn mark_view(
    mut commands: Commands,
    cameras: Query<
        (
            Entity,
            &Camera,
            &RenderTarget,
            Has<WorldView>,
            Has<GrassView>,
        ),
        With<Camera3d>,
    >,
) {
    let world = cameras
        .iter()
        .find(|(_, camera, target, world_view, _)| {
            crate::draws_the_world(camera, target, *world_view)
        })
        .map(|(entity, ..)| entity);
    for (entity, _, _, _, marked) in &cameras {
        let wanted = Some(entity) == world;
        if wanted && !marked {
            commands.entity(entity).insert(GrassView);
        } else if !wanted && marked {
            commands.entity(entity).remove::<GrassView>();
        }
    }
}

/// Writes the day's meadow: the settings, the weather the blades stand in,
/// the season and the height the grassland calendar gives.
fn feed_environment(
    sky: Res<Sky>,
    settings: Res<GrassRenderSettings>,
    mut environment: ResMut<GrassEnvironment>,
) {
    let growth = phenology::growth(CropClass::Grassland, sky.month, sky.day, 0);
    let next = GrassEnvironment {
        settings: *settings,
        weather: WeatherParams::of(&sky),
        season: Season::on(sky.month, sky.day),
        // A verge is a lawn layer, not knee-high tussocks: the calendar's
        // height, held between a fresh cut and a June meadow.
        height: growth.height.clamp(0.12, 0.28),
    };
    if *environment != next {
        *environment = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_are_kept_within_what_the_buffers_hold() {
        let wild = GrassRenderSettings::new(true, 5_000.0, 9.0);
        assert!(wild.range <= 400.0);
        assert!(wild.density <= 1.5);
        let off = GrassRenderSettings::new(false, 100.0, 0.5);
        assert!(!off.enabled);
    }
}
