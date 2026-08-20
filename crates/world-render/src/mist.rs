//! Ground mist (plan 14.1): the fog that lies *in* a valley rather than
//! everywhere, and the shafts the sun cuts through it.
//!
//! The haze of the weather is a scattering term of the atmosphere itself
//! (`sky::haze`) — that is what closes the view down to 300 m and takes the
//! colour of the hour. What it cannot do is end at a height: a planetary medium
//! has no top. So a layer of mist is one of Bevy's own [`FogVolume`]s, a box
//! riding with the camera, raymarched against the sun's shadow map so light
//! shafts fall through it.
//!
//! It is the one part of the weather that costs enough to be a setting
//! ([`Quality::volumetric`]), because a raymarch per pixel is half a millisecond
//! whatever else is on the screen.

use crate::sky::{Sky, Sun};
use bevy::light::{FogVolume, VolumetricFog, VolumetricLight};
use bevy::prelude::*;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Quality>()
        .add_systems(Startup, spawn)
        .add_systems(Update, update);
}

/// What the renderer is allowed to spend. The simulator writes it from its
/// graphics settings; the editor leaves it as it is.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quality {
    /// Draw the ground mist as a volume, with light shafts through it.
    pub volumetric: bool,
}

impl Default for Quality {
    fn default() -> Self {
        Self { volumetric: true }
    }
}

/// The one mist volume, kept on the camera.
#[derive(Component)]
struct Mist;

/// Side length of the volume \[m\]. Big enough that its edge is beyond anything
/// the mist itself lets you see, small enough that the march stays cheap.
const EXTENT: f32 = 900.0;

fn spawn(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    commands.spawn((
        Mist,
        FogVolume {
            // The shape of the mist: the same noise the clouds are made of, at a
            // scale where one billow is a field wide.
            density_texture: Some(images.add(crate::clouds::noise_volume(32, 3.0, 41))),
            density_factor: 0.0,
            scattering: 1.0,
            absorption: 0.02,
            // Mist scatters forwards hard — which is why a sunrise through it is
            // blinding and the same mist behind you is barely there.
            scattering_asymmetry: 0.7,
            ..default()
        },
        Transform::from_scale(Vec3::splat(EXTENT)),
        Visibility::Hidden,
    ));
}

fn update(
    sky: Res<Sky>,
    quality: Res<Quality>,
    mut commands: Commands,
    camera: Query<(Entity, &GlobalTransform, Has<VolumetricFog>), With<Camera3d>>,
    sun: Query<(Entity, Has<VolumetricLight>), With<Sun>>,
    mut mist: Query<(&mut FogVolume, &mut Transform, &mut Visibility), With<Mist>>,
) {
    let depth = sky.weather.fog_depth;
    let wanted = quality.volumetric && depth > 0.0;
    for (camera, transform, has_fog) in &camera {
        match (wanted, has_fog) {
            (true, false) => {
                commands.entity(camera).insert(VolumetricFog {
                    // Half the default: the layer is thin and the steps are
                    // spent inside it, not on the kilometres of clear air above.
                    step_count: 32,
                    ..default()
                });
            }
            (false, true) => {
                commands.entity(camera).remove::<VolumetricFog>();
            }
            _ => {}
        }
        for (mut volume, mut volume_transform, mut visibility) in &mut mist {
            *visibility = if wanted {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            if !wanted {
                continue;
            }
            // The layer sits on the ground, which is y = 0 in render space, and
            // follows the camera in the plane.
            let eye = transform.translation();
            *volume_transform = Transform::from_translation(Vec3::new(eye.x, depth * 0.5, eye.z))
                .with_scale(Vec3::new(EXTENT, depth, EXTENT));
            // Koschmieder again: the sight the weather asks for, as an extinction
            // per metre. The atmosphere already carries part of it, so this is
            // the half that has a top.
            volume.density_factor = (3.912 / sky.weather.visibility * 0.5).clamp(0.0, 0.05);
        }
    }
    // Light shafts need a light that casts them.
    for (sun, has_volumetric) in &sun {
        match (wanted, has_volumetric) {
            (true, false) => {
                commands.entity(sun).insert(VolumetricLight);
            }
            (false, true) => {
                commands.entity(sun).remove::<VolumetricLight>();
            }
            _ => {}
        }
    }
}
