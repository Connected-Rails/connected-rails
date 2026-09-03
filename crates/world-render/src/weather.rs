//! What the weather does to the things in the world (plan 14.1).
//!
//! The look itself lives in `weather.wgsl`, which both materials share. This is
//! the plumbing around it:
//!
//! * [`WeatherMaterial`] — `StandardMaterial` plus that shader. Every material of
//!   every model in the world is swapped for one of these on the way in
//!   ([`dress`]), so a mod's building gets wet and snowy without shipping a line
//!   of its own. Vehicles are left alone: a cab has an inside, and until there is
//!   a precipitation occlusion map the snow would settle on the driver's seat.
//! * [`WeatherParams`] — the one uniform behind it, written only when the weather
//!   has moved far enough to see. The animation reads the view's own clock, so a
//!   ripple does not cost an upload per frame.
//!
//! **Multiplayer.** Nothing here is state. The uniform is a function of
//! [`Sky`](crate::sky::Sky), which is a function of the scenario clock.

use crate::{TerrainMaterial, WorldAnchored, sky::Sky};
use bevy::asset::{AssetId, embedded_asset};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::{ShaderRef, load_shader_library};

/// Everything in the world that is not a vehicle draws with this.
pub type WeatherMaterial = ExtendedMaterial<StandardMaterial, WeatherExt>;

pub(crate) fn plugin(app: &mut App) {
    load_shader_library!(app, "weather.wgsl");
    embedded_asset!(app, "weather_object.wgsl");
    app.add_plugins(MaterialPlugin::<WeatherMaterial>::default())
        .add_systems(Update, (dress, update));
}

/// The whole world's weather, as the shader wants it.
#[derive(ShaderType, Debug, Clone, Copy, Default, PartialEq)]
pub struct WeatherParams {
    /// x = surface water 0…1, y = lying snow 0…1, z = rain rate \[mm/h\],
    /// w = how much of the sun the clouds take, 0…1.
    pub state: Vec4,
    /// xy = wind in render space \[m/s\], zw = reserved.
    pub wind: Vec4,
}

impl WeatherParams {
    /// What the sky says, in the shader's layout.
    pub fn of(sky: &Sky) -> Self {
        let weather = sky.weather;
        // Meteorological bearing: the direction the wind comes *from*, clockwise
        // from north. Render space is +x east, −z north.
        let (sin, cos) = weather.bearing.sin_cos();
        Self {
            state: Vec4::new(
                sky.wetness,
                sky.snow,
                if weather.precip.is_liquid() {
                    weather.rate
                } else {
                    0.0
                },
                sky.cloud_shadow,
            ),
            wind: Vec4::new(-sin * weather.wind, cos * weather.wind, 0.0, 0.0),
        }
    }

    /// Whether the difference to `other` is worth an upload. Wetness and snow move
    /// over minutes, so most frames answer no.
    fn differs(self, other: Self) -> bool {
        (self.state - other.state).abs().max_element() > 0.005
            || (self.wind - other.wind).abs().max_element() > 0.1
    }
}

/// The extension itself — one uniform, one shader.
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone, Default)]
pub struct WeatherExt {
    #[uniform(100)]
    pub weather: WeatherParams,
}

impl MaterialExtension for WeatherExt {
    fn fragment_shader() -> ShaderRef {
        "embedded://world_render/weather_object.wgsl".into()
    }
}

/// Swaps the plain material of everything anchored in the world for one that
/// knows about the weather.
///
/// A glTF scene spawns its meshes as children, so the marker is looked for up the
/// tree. One extended material per base material, cached — two hundred trees that
/// shared a material before still share one, and still batch into one draw.
// A Bevy system takes its world access as parameters; the count says nothing here.
#[allow(clippy::too_many_arguments)]
fn dress(
    mut commands: Commands,
    new: Query<
        (Entity, &MeshMaterial3d<StandardMaterial>),
        Added<MeshMaterial3d<StandardMaterial>>,
    >,
    parents: Query<&ChildOf>,
    anchored: Query<(), With<WorldAnchored>>,
    base: Res<Assets<StandardMaterial>>,
    mut extended: ResMut<Assets<WeatherMaterial>>,
    sky: Res<Sky>,
    mut cache: Local<HashMap<AssetId<StandardMaterial>, Handle<WeatherMaterial>>>,
    // Entities whose base material had not finished loading yet.
    mut pending: Local<Vec<(Entity, Handle<StandardMaterial>)>>,
) {
    let outdoors = |entity: Entity| {
        anchored.contains(entity)
            || parents
                .iter_ancestors(entity)
                .any(|parent| anchored.contains(parent))
    };
    let mut todo = std::mem::take(&mut *pending);
    todo.extend(
        new.iter()
            .filter(|(entity, _)| outdoors(*entity))
            .map(|(entity, material)| (entity, material.0.clone())),
    );

    let params = WeatherParams::of(&sky);
    for (entity, handle) in todo {
        // The entity may be gone by now: the editor's terrain streaming despawns
        // a tile with the trees on it, and a rebuild throws away everything the
        // document spawned. Waiting in `pending` for a material makes that a
        // question of frames rather than of one — and a dressing queued for an
        // entity that no longer exists takes the app down when the command
        // buffer is applied, not where it was queued.
        if commands.get_entity(entity).is_err() {
            continue;
        }
        let Some(material) = base.get(&handle) else {
            // The scene is spawned, the material is still loading — next frame.
            pending.push((entity, handle));
            continue;
        };
        let dressed = cache
            .entry(handle.id())
            .or_insert_with(|| {
                extended.add(WeatherMaterial {
                    base: material.clone(),
                    extension: WeatherExt { weather: params },
                })
            })
            .clone();
        // `try_`: the despawn may also come from a system that runs between
        // this one and the sync point that applies its commands.
        commands
            .entity(entity)
            .try_remove::<MeshMaterial3d<StandardMaterial>>()
            .try_insert(MeshMaterial3d(dressed));
    }
}

fn update(
    sky: Res<Sky>,
    mut objects: ResMut<Assets<WeatherMaterial>>,
    mut terrain: ResMut<Assets<TerrainMaterial>>,
    mut fields: ResMut<Assets<crate::farmland::FieldMaterial>>,
    mut water: ResMut<Assets<crate::water::WaterMaterial>>,
    mut roads: ResMut<Assets<crate::roads::RoadMaterial>>,
    mut grass: ResMut<Assets<crate::plants::GrassMaterial>>,
    mut last: Local<Option<WeatherParams>>,
) {
    let params = WeatherParams::of(&sky);
    if last.is_some_and(|last| !params.differs(last)) {
        return;
    }
    *last = Some(params);
    for (_, material) in objects.iter_mut() {
        material.extension.weather = params;
    }
    for (_, material) in terrain.iter_mut() {
        material.extension.weather = params;
    }
    // The farmland takes the same rain and the same cloud shadow as the ground
    // it lies on — anything else and a field would stay dry in a downpour.
    for (_, material) in fields.iter_mut() {
        material.extension.weather = params;
    }
    // The water most of all: its waves are the wind, its rings are the rain.
    for (_, material) in water.iter_mut() {
        material.extension.weather = params;
    }
    // The roads take it like the fields: wet asphalt polishes, snow covers.
    for (_, material) in roads.iter_mut() {
        material.extension.weather = params;
    }
    // Close meadow blades use their own vertex stage so the same wind bends
    // them; the fragment stage still receives the world's wetness and snow.
    for (_, material) in grass.iter_mut() {
        material.extension.weather = params;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::weather::Preset;

    #[test]
    fn the_uniform_carries_what_the_shader_reads() {
        let mut sky = Sky {
            weather: Preset::Rain.weather(),
            ..default()
        };
        sky.wetness = 0.5;
        sky.snow = 0.25;
        let params = WeatherParams::of(&sky);
        assert_eq!(params.state.x, 0.5);
        assert_eq!(params.state.y, 0.25);
        assert_eq!(params.state.z, Preset::Rain.weather().rate, "rain falls");
        // Snow is not rain: no ripples on the ground while it falls.
        sky.weather = Preset::Snow.weather();
        assert_eq!(WeatherParams::of(&sky).state.z, 0.0);
    }

    #[test]
    fn a_westerly_blows_towards_the_east() {
        let sky = Sky {
            weather: sim_core::weather::Weather {
                wind: 10.0,
                // 270° — the wind comes from the west.
                bearing: std::f32::consts::FRAC_PI_2 * 3.0,
                ..Preset::Clear.weather()
            },
            ..default()
        };
        let wind = WeatherParams::of(&sky).wind;
        assert!(wind.x > 9.9, "blows east: {wind:?}");
        assert!(wind.y.abs() < 0.1, "and not north or south: {wind:?}");
    }

    #[test]
    fn small_changes_do_not_re_upload() {
        let a = WeatherParams {
            state: Vec4::new(0.5, 0.0, 4.0, 0.0),
            wind: Vec4::ZERO,
        };
        let b = WeatherParams {
            state: Vec4::new(0.502, 0.0, 4.0, 0.0),
            ..a
        };
        assert!(!a.differs(b));
        assert!(a.differs(WeatherParams {
            state: Vec4::new(0.6, 0.0, 4.0, 0.0),
            ..a
        }));
    }
}
