//! Clouds over the world (plan 14.1) — two ways of drawing the same sky, and a
//! switch between them ([`Quality::volumetric`]).
//!
//! Both raymarch, and neither does it per screen pixel. The pass writes a
//! 2048 × 1024 equirectangular panorama and a dome samples it: a camera that
//! stands on the ground never enters a cloud, so a direction is all a cloud has
//! to be a function of. What makes that panorama affordable at this size is
//! **amortisation** — one texel in sixteen is rewritten each frame, on a 4 × 4
//! Bayer slot, and the pass does not clear, so the other fifteen keep what
//! earlier frames left. 131 k texels a frame, which is fewer than the 768 × 384
//! panorama this replaces cost at full rate, for 2.7 × the angular resolution.
//!
//! What is marched into it depends on the setting:
//!
//! * **Volumetric** — Guerrilla's Nubis: a shape noise eroded by a detail noise,
//!   Beer-Powder attenuation, a forward-scattering phase for the silver lining,
//!   96 steps along the ray and 6 towards the sun.
//! * **Layered** — the same shape field and the same scattering, read once where
//!   the ray crosses the middle of the deck, with the self-shadow walked across
//!   that height field. Roughly a twentieth of the cost, at the same resolution,
//!   so the cheap sky is soft and sharp rather than soft and blurred.
//!
//! The dome hangs in the transparent phase, which Bevy runs *after* its
//! atmosphere (`render_sky`) — so the clouds composite over the finished sky the
//! same way the stars and the moon already do, and no render-graph node is needed.
//! It filters the panorama cubically, and it is where the lightning is added,
//! because a strike lasts less time than one turn of the amortisation.
//!
//! *Ceiling:* no translation parallax, and no flying into a cloud. At a 1.5 km
//! base and 90 km/h the first is metres against kilometres; the upgrade path for
//! both is to march the same shader at half resolution in screen space.
//!
//! **Multiplayer.** The drift is a function of the scenario clock, so two clients
//! see the same cloud over the same field without a byte crossing the wire.

use crate::sky::{SKY_RADIUS, Sky, Sun};

use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ClearColorConfig, RenderTarget};
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Mesh3d, MeshBuilder, Meshable, SphereKind};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat, TextureUsages,
};
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dPlugin, MeshMaterial2d};

/// Panorama size. 0.18° of sky a texel, against the 0.47° of the 768 × 384 this
/// replaced — which is about where a cumulus edge stops being a staircase,
/// because at ten kilometres a real one is that soft anyway.
const PANORAMA: UVec2 = UVec2::new(2048, 1024);

/// Frames one turn of the amortisation takes — the 4 × 4 Bayer pattern of
/// `clouds.wgsl`. A quarter of a second at 60 Hz, in which a cloud five
/// kilometres out drifts a fifth of a texel.
const AMORTISE: u32 = 16;

/// The layer the panorama camera and its quad live on, so the pass sees nothing
/// else and nothing else sees them.
const CLOUD_LAYER: usize = 7;

/// Radius of the dome. Inside the star sphere, so the transparent phase — which
/// sorts back to front — draws the stars first and the clouds over them.
const DOME_RADIUS: f32 = SKY_RADIUS * 0.9;

/// What the wind at the cloud base is against the wind the weather reports.
///
/// [`Weather::wind`](sim_core::weather::Weather::wind) is the ten-metre wind — what a
/// station measures, and what the rain in front of the camera has to lean into. A
/// kilometre and a half up, out of the friction of the ground, it blows two to three
/// times that: the Ekman spiral unwinds into the geostrophic wind, which is why a
/// still afternoon can have a deck crossing the sky. Drifting the clouds at the
/// ground's own speed is what made a fair-weather sky sit almost still — 2 m/s is
/// 7 km/h, and a cumulus does not crawl.
const WIND_ALOFT: f32 = 2.5;

/// The colour the sky puts back into a cloud, and the colour a strike lights one
/// with. The same tint for both: a flash is the sky lighting the deck from the
/// inside, only far brighter and for two frames.
const SKY_TINT: Vec3 = Vec3::new(0.50, 0.62, 0.85);

pub(crate) fn plugin(app: &mut App) {
    embedded_asset!(app, "clouds.wgsl");
    embedded_asset!(app, "cloud_dome.wgsl");
    app.init_resource::<Quality>()
        .add_plugins((
            Material2dPlugin::<CloudMaterial>::default(),
            MaterialPlugin::<DomeMaterial>::default(),
        ))
        .add_systems(Startup, spawn)
        .add_systems(Update, update);
}

/// What the renderer is allowed to spend on the sky. The simulator writes it from
/// its graphics settings; the editor leaves it as it is.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quality {
    /// March the cloud deck as a volume — billows, a lit interior and parallax
    /// through it. Off draws the same clouds as one lit sheet, at the same
    /// resolution and about a twentieth of the cost.
    pub volumetric: bool,
}

impl Default for Quality {
    fn default() -> Self {
        Self { volumetric: true }
    }
}

/// What the march is told about the sky it is marching in.
#[derive(ShaderType, Debug, Clone, Copy, Default)]
pub struct CloudParams {
    /// xyz = direction towards the sun, w = 1 by day, 0 at night.
    pub sun: Vec4,
    /// rgb = the light that reaches the cloud layer, a = drift time \[s\].
    pub light: Vec4,
    /// rgb = the sky's own light on the clouds from every other direction,
    /// a = cover 0 … 1.
    pub ambient: Vec4,
    /// x = base \[m\], y = thickness \[m\], z/w = wind \[m/s\] in render space.
    pub layer: Vec4,
    /// x = the Bayer slot this frame writes, or −1 for every texel at once;
    /// y = 1 volumetric, 0 layered.
    pub frame: Vec4,
}

/// The march itself, drawn into the panorama by a camera of its own.
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct CloudMaterial {
    #[uniform(0)]
    params: CloudParams,
    /// Low-frequency shape: r = Perlin-Worley, gba = Worley at three octaves.
    #[texture(1, dimension = "3d")]
    #[sampler(2)]
    shape: Handle<Image>,
    /// High-frequency erosion of the cloud's edge.
    #[texture(3, dimension = "3d")]
    #[sampler(4)]
    detail: Handle<Image>,
}

impl Material2d for CloudMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://world_render/clouds.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Opaque
    }
}

/// What the dome is told about the air the clouds hang in.
#[derive(ShaderType, Debug, Clone, Copy, Default)]
pub struct DomeParams {
    /// x = extinction of the weather's haze \[1/m\], y = its scale height \[m\].
    /// Without this a fog would close the view to 300 m and still show a sky full
    /// of crisp cumulus.
    pub haze: Vec4,
    /// rgb = what a lightning strike puts into the deck this frame.
    ///
    /// Here rather than in the march because the panorama takes sixteen frames to
    /// turn over and a strike lasts two — through it, a flash would light a
    /// sixteenth of the sky at a time.
    pub flash: Vec4,
}

/// The dome that shows the panorama in the world.
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct DomeMaterial {
    #[texture(0)]
    #[sampler(1)]
    panorama: Handle<Image>,
    #[uniform(2)]
    params: DomeParams,
}

impl Material for DomeMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://world_render/cloud_dome.wgsl".into()
    }

    /// Over the sky, under everything solid — and never a shadow caster. What the
    /// march returns is light along a ray, so it is already multiplied by its own
    /// coverage.
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Premultiplied
    }

    fn enable_shadows() -> bool {
        false
    }

    fn enable_prepass() -> bool {
        false
    }
}

/// Marks the dome so [`update`] can keep it on the camera.
#[derive(Component)]
pub struct Dome;

/// Spawns the panorama, the camera that draws it, and the dome that shows it.
/// Startup rather than a call in `sky::spawn`, so neither program has to know
/// that the sky grew a cloud layer.
fn spawn(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut clouds: ResMut<Assets<CloudMaterial>>,
    mut domes: ResMut<Assets<DomeMaterial>>,
) {
    let panorama = images.add(panorama_target());
    let material = clouds.add(CloudMaterial {
        params: CloudParams::default(),
        shape: images.add(noise_volume(64, 4.0, 7)),
        detail: images.add(noise_volume(32, 8.0, 23)),
    });

    // A camera of its own, on its own layer, rendering one quad — the cheapest
    // offscreen pass Bevy offers without a render-graph node. `order: -1` puts it
    // in front of the world camera that samples what it wrote.
    let layer = RenderLayers::layer(CLOUD_LAYER);
    commands.spawn((
        crate::Persistent,
        Camera2d,
        Camera {
            order: -1,
            // **Not cleared.** Each frame writes one texel in sixteen and the
            // rest have to survive — that is the whole amortisation. It also
            // means the panorama has to be filled before it is first shown,
            // which `update` does by marching every texel for the first turn.
            clear_color: ClearColorConfig::None,
            ..default()
        },
        RenderTarget::Image(panorama.clone().into()),
        layer.clone(),
    ));
    commands.spawn((
        crate::Persistent,
        Mesh2d(meshes.add(Rectangle::new(PANORAMA.x as f32, PANORAMA.y as f32))),
        MeshMaterial2d(material),
        layer,
    ));

    commands.spawn((
        crate::Persistent,
        Dome,
        Mesh3d(meshes.add(dome_mesh())),
        MeshMaterial3d(domes.add(DomeMaterial {
            panorama,
            params: DomeParams::default(),
        })),
        Transform::from_scale(Vec3::splat(DOME_RADIUS)),
        Visibility::default(),
    ));
}

/// The image the march writes into: half float, because a sunlit cloud edge is
/// well above 1, and wrapped in longitude so the seam behind the observer closes.
fn panorama_target() -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width: PANORAMA.x,
            height: PANORAMA.y,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0; 8],
        TextureFormat::Rgba16Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..default()
    });
    image
}

/// Half a sphere, seen from the inside.
fn dome_mesh() -> Mesh {
    Sphere::new(1.0)
        .mesh()
        .kind(SphereKind::Uv {
            sectors: 32,
            stacks: 16,
        })
        .build()
        .with_inverted_winding()
        .expect("uv sphere is indexed")
}

/// A tileable 3D noise volume: `r` is a Perlin-Worley mix, `gba` are Worley
/// octaves — the four channels a cloud shape needs, in one RGBA8 lookup.
///
/// Generated rather than shipped, like the ground textures: the repository
/// carries no binary assets. 64³ costs about a quarter of a second at startup.
pub(crate) fn noise_volume(size: u32, frequency: f32, seed: u64) -> Image {
    let mut data = vec![0u8; (size * size * size * 4) as usize];
    let n = size as f32;
    for z in 0..size {
        for y in 0..size {
            for x in 0..size {
                let p = Vec3::new(x as f32, y as f32, z as f32) / n;
                let perlin = fbm(p, frequency, seed);
                // Worley inverted: a cloud is the space *between* the cells.
                let worley: [f32; 3] = std::array::from_fn(|i| {
                    1.0 - worley(p, frequency * (2.0f32).powi(i as i32 + 1), seed + i as u64)
                });
                // Perlin remapped by the coarsest Worley — Schneider's mix, which
                // is what gives a cumulus its billows instead of a fog bank.
                let shape = remap(perlin, 1.0 - worley[0], 1.0, 0.0, 1.0).clamp(0.0, 1.0);
                let index = (((z * size + y) * size + x) * 4) as usize;
                data[index] = (shape * 255.0) as u8;
                for (i, w) in worley.iter().enumerate() {
                    data[index + 1 + i] = (w.clamp(0.0, 1.0) * 255.0) as u8;
                }
            }
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: size,
        },
        TextureDimension::D3,
        data,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..default()
    });
    image
}

/// Value noise over a wrapping lattice — three octaves is what a cloud shape
/// needs before the Worley erosion takes over.
fn fbm(p: Vec3, frequency: f32, seed: u64) -> f32 {
    let mut sum = 0.0;
    let mut amplitude = 0.5;
    let mut f = frequency;
    for octave in 0..3 {
        sum += amplitude * value_noise(p, f, seed + octave);
        amplitude *= 0.5;
        f *= 2.0;
    }
    sum
}

fn value_noise(p: Vec3, frequency: f32, seed: u64) -> f32 {
    let period = frequency.max(1.0) as i64;
    let scaled = p * frequency;
    let cell = scaled.floor();
    let f = scaled - cell;
    // Smoothstep, so the lattice does not show as a grid of creases.
    let u = f * f * (Vec3::splat(3.0) - 2.0 * f);
    let corner = |dx: i64, dy: i64, dz: i64| {
        hash01(
            (cell.x as i64 + dx).rem_euclid(period),
            (cell.y as i64 + dy).rem_euclid(period),
            (cell.z as i64 + dz).rem_euclid(period),
            seed,
        )
    };
    let mix = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let x00 = mix(corner(0, 0, 0), corner(1, 0, 0), u.x);
    let x10 = mix(corner(0, 1, 0), corner(1, 1, 0), u.x);
    let x01 = mix(corner(0, 0, 1), corner(1, 0, 1), u.x);
    let x11 = mix(corner(0, 1, 1), corner(1, 1, 1), u.x);
    mix(mix(x00, x10, u.y), mix(x01, x11, u.y), u.z)
}

/// Distance to the nearest of one point per cell, wrapped — the cellular noise
/// whose *inverse* looks like a cloud's billows.
fn worley(p: Vec3, frequency: f32, seed: u64) -> f32 {
    let period = frequency.max(1.0) as i64;
    let scaled = p * frequency;
    let cell = scaled.floor();
    let mut nearest = f32::MAX;
    for dz in -1..=1 {
        for dy in -1..=1 {
            for dx in -1..=1 {
                let neighbour = cell + Vec3::new(dx as f32, dy as f32, dz as f32);
                let wrapped = (
                    (neighbour.x as i64).rem_euclid(period),
                    (neighbour.y as i64).rem_euclid(period),
                    (neighbour.z as i64).rem_euclid(period),
                );
                let offset = Vec3::new(
                    hash01(wrapped.0, wrapped.1, wrapped.2, seed),
                    hash01(wrapped.0, wrapped.1, wrapped.2, seed + 101),
                    hash01(wrapped.0, wrapped.1, wrapped.2, seed + 211),
                );
                nearest = nearest.min((neighbour + offset - scaled).length());
            }
        }
    }
    nearest.min(1.0)
}

fn hash01(x: i64, y: i64, z: i64, seed: u64) -> f32 {
    let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ (z as u64).wrapping_mul(0x1656_67B1_9E37_79F9)
        ^ seed.wrapping_mul(0xD6E8_FEB8_6659_FD93);
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    (h >> 40) as f32 / (1 << 24) as f32
}

fn remap(value: f32, from_min: f32, from_max: f32, to_min: f32, to_max: f32) -> f32 {
    to_min + (value - from_min) / (from_max - from_min).max(1e-5) * (to_max - to_min)
}

/// Feeds the march from the sky, turns the amortisation over and keeps the dome
/// on the camera.
#[allow(clippy::too_many_arguments)]
fn update(
    sky: Res<Sky>,
    quality: Res<Quality>,
    mut materials: ResMut<Assets<CloudMaterial>>,
    mut domes: ResMut<Assets<DomeMaterial>>,
    handles: Query<&MeshMaterial2d<CloudMaterial>>,
    dome_handles: Query<&MeshMaterial3d<DomeMaterial>>,
    sun: Query<&Transform, (With<Sun>, Without<Dome>)>,
    camera: Query<&GlobalTransform, (With<Camera3d>, Without<Dome>)>,
    mut dome: Query<&mut Transform, (With<Dome>, Without<Sun>)>,
    mut frame: Local<u32>,
) {
    // The sun's own transform looks *along* the light, so the direction towards
    // it is the other way.
    let sun_dir = sun
        .iter()
        .next()
        .map_or(Vec3::Y, |t| -t.forward().as_vec3());
    let weather = sky.weather;
    let daylight = ((sun_dir.y * 90.0f32.to_radians().sin() + 0.1) * 4.0).clamp(0.0, 1.0);

    // A panorama that holds nothing — the first frames, or the frames after the
    // setting was dialled and every texel means something else now — has to be
    // marched whole, because there is no older frame worth keeping.
    *frame = if quality.is_changed() {
        0
    } else {
        frame.saturating_add(1)
    };
    let writing = writing_slot(*frame).map_or(-1.0, |slot| slot as f32);

    let params = CloudParams {
        sun: sun_dir.extend(daylight),
        // Illuminance, not radiance: the march multiplies it by a phase function
        // that is normalised over the sphere, so the 1/4π lives there.
        light: (sunlight(sun_dir.y) * crate::sky::SUN_ILLUMINANCE).extend(sky.seconds as f32),
        // What the rest of the sky puts back into a cloud. Blue by day, and never
        // quite black, or a night cloud would be a hole in the stars. The
        // lightning is *not* in here — it is on the dome, where every frame runs.
        ambient: (SKY_TINT * crate::sky::SUN_ILLUMINANCE * 0.10 * (0.05 + 0.95 * daylight))
            .extend(weather.cover),
        layer: Vec4::new(
            weather.base,
            // A closed deck is a thick one; a fair-weather cumulus is not.
            600.0 + 1_400.0 * weather.cover,
            -weather.bearing.sin() * weather.wind * WIND_ALOFT,
            weather.bearing.cos() * weather.wind * WIND_ALOFT,
        ),
        frame: Vec4::new(writing, f32::from(u8::from(quality.volumetric)), 0.0, 0.0),
    };
    for handle in &handles {
        if let Some(mut material) = materials.get_mut(&handle.0) {
            material.params = params;
        }
    }

    let dome_params = DomeParams {
        haze: Vec4::new(
            crate::sky::haze_extinction(weather.visibility),
            crate::sky::haze_height(weather.fog_depth),
            0.0,
            0.0,
        ),
        // A strike lights the whole deck from the inside, which is what a
        // thunderstorm looks like from under it — the channel itself is behind
        // the cloud far more often than not.
        flash: (SKY_TINT * crate::sky::SUN_ILLUMINANCE * 0.9 * sky.flash).extend(0.0),
    };
    for handle in &dome_handles {
        if let Some(mut dome) = domes.get_mut(&handle.0) {
            dome.params = dome_params;
        }
    }

    let eye = camera.iter().next().map_or(Vec3::ZERO, |t| t.translation());
    for mut transform in &mut dome {
        *transform = Transform::from_translation(eye).with_scale(Vec3::splat(DOME_RADIUS));
    }
}

/// Which of the sixteen Bayer slots this frame writes, or `None` while the
/// panorama still has to be marched whole.
///
/// `frame` counts from the last time the sky's meaning changed. The first turn
/// is marched entire — the pass does not clear, so before that there is nothing
/// worth keeping — and from there each slot comes up once a turn.
fn writing_slot(frame: u32) -> Option<u32> {
    (frame >= AMORTISE).then_some(frame % AMORTISE)
}

/// The colour of the sunlight that reaches the cloud layer, from the sun's
/// elevation alone — Kasten & Young's air mass through the same zenith optical
/// depth the star shader uses, so the two agree on what a low sun looks like.
fn sunlight(sin_elevation: f32) -> Vec3 {
    const ZENITH_OPTICAL_DEPTH: Vec3 = Vec3::new(0.081, 0.150, 0.315);
    let degrees = sin_elevation.clamp(-1.0, 1.0).asin().to_degrees();
    let air_mass =
        1.0 / (sin_elevation.max(0.0) + 0.50572 * (degrees + 6.07995).max(0.5).powf(-1.6364));
    let transmittance = (-ZENITH_OPTICAL_DEPTH * air_mass).exp();
    // Below the horizon the earth is in the way; the fade is the last light on a
    // cloud base after sunset.
    transmittance * smoothstep(-0.08, 0.02, sin_elevation)
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The amortisation has to warm up and then visit every slot. Skip the warm-up
    /// and the first frames show whatever the texture memory held; skip a slot and
    /// a sixteenth of the sky stands still for ever.
    #[test]
    fn the_amortisation_warms_up_and_then_visits_every_slot() {
        for frame in 0..AMORTISE {
            assert_eq!(
                writing_slot(frame),
                None,
                "frame {frame} is still warming up"
            );
        }
        let visited: std::collections::BTreeSet<u32> =
            (AMORTISE..AMORTISE * 3).filter_map(writing_slot).collect();
        assert_eq!(
            visited,
            (0..AMORTISE).collect(),
            "every slot comes up, and none twice over"
        );
    }

    #[test]
    fn the_noise_wraps() {
        // A volume that does not tile shows its seams as straight lines in the sky.
        for (a, b) in [
            (Vec3::new(0.0, 0.3, 0.7), Vec3::new(1.0, 0.3, 0.7)),
            (Vec3::new(0.2, 0.0, 0.5), Vec3::new(0.2, 1.0, 0.5)),
            (Vec3::new(0.4, 0.6, 0.0), Vec3::new(0.4, 0.6, 1.0)),
        ] {
            assert!(
                (value_noise(a, 4.0, 1) - value_noise(b, 4.0, 1)).abs() < 1e-5,
                "value noise seam at {a} / {b}"
            );
            assert!(
                (worley(a, 4.0, 1) - worley(b, 4.0, 1)).abs() < 1e-5,
                "worley seam at {a} / {b}"
            );
        }
    }

    #[test]
    fn sunlight_reddens_and_goes_out() {
        let noon = sunlight(1.0);
        let low = sunlight(0.05);
        assert!(noon.x > 0.9 && noon.z > 0.7, "white at the zenith: {noon}");
        assert!(low.x > low.z * 2.0, "red near the horizon: {low}");
        assert_eq!(sunlight(-0.2), Vec3::ZERO, "gone once it has set");
    }

    #[test]
    fn the_volume_has_every_channel() {
        let image = noise_volume(8, 2.0, 5);
        let data = image.data.as_ref().expect("generated in memory");
        assert_eq!(data.len(), 8 * 8 * 8 * 4);
        for channel in 0..4 {
            let spread = data
                .iter()
                .skip(channel)
                .step_by(4)
                .fold((255u8, 0u8), |(lo, hi), &v| (lo.min(v), hi.max(v)));
            assert!(
                spread.1 - spread.0 > 20,
                "channel {channel} is flat: {spread:?}"
            );
        }
    }
}
