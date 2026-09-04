//! Clouds over the world (plan 14.1) — two ways of drawing the same sky, and a
//! switch between them ([`Quality::volumetric`]).
//!
//! Both raymarch, and neither does it per screen pixel. The pass writes a
//! 2048 × 1024 equirectangular panorama and a dome samples it: a camera that
//! stands on the ground never enters a cloud, so a direction is all a cloud has
//! to be a function of. What makes that panorama affordable at this size is
//! **amortisation** — one texel in sixteen is marched each frame, on a 4 × 4
//! Bayer slot, and the other fifteen are carried over from the frame before.
//! 131 k texels a frame, which is fewer than the 768 × 384 panorama this
//! replaces cost at full rate, for 2.7 × the angular resolution.
//!
//! A march is not written over its texel but **blended into it**. The panorama
//! is two buffers that swap roles every frame — the pass reads the one written
//! last frame and writes the other — and a fresh march is mixed into what the
//! texel held at a weight that gives the panorama a memory of about a second
//! ([`HISTORY_SECONDS`]), whatever the frame rate. Each march sends its ray
//! through a new point of the texel, starts it a new way into the first step
//! and aims its light cone somewhere new every turn, so what the blend
//! converges on is the integral over the texel, the step and the cone: a cloud
//! edge filtered over the texel's footprint of sky, and a body without noise.
//! Both are what a 2K screen, which stretches a texel over five pixels, showed
//! as a raster — a stepped edge, and one sample's noise frozen into every
//! texel, because the jitter used to *be* the Bayer slot, a 4 × 4 pattern that
//! repeated exactly and never averaged out. The deck drifts while the panorama
//! remembers, so a march reads its history where the cloud *was* a turn ago
//! ([`TurnClock`]) and the blend follows the cloud instead of smearing it along
//! its path. A change that makes the history worthless — the quality switch, a
//! jump of the clock — starts the panorama over.
//!
//! What is marched into it depends on the setting:
//!
//! * **Volumetric** — Guerrilla's Nubis: a shape noise eroded by a detail noise
//!   that is wispy at the base and billowy above, a height profile that narrows
//!   a cumulus towards its top, Beer attenuation, a forward-scattering phase for
//!   the silver lining, 96 steps along the ray and 6 towards the sun.
//! * **Layered** — the same shape field and the same scattering, read once where
//!   the ray crosses the middle of the deck, with the self-shadow walked across
//!   that height field. Roughly a twentieth of the cost, at the same resolution,
//!   so the cheap sky is soft and sharp rather than soft and blurred.
//!
//! Neither guesses what colour the sky is. Bevy's atmosphere writes its sky-view
//! table into a cubemap every frame for its own image-based lighting
//! ([`AtmosphereEnvironmentMapLight`](bevy::light::AtmosphereEnvironmentMapLight)),
//! and [`update`] hands that cubemap to the march. It is what lights the shaded
//! side of a cloud — blue at noon, dim and warm at dusk, whatever the weather's
//! haze has made of it — and it is what a far cloud fades into through the air in
//! front of it. The one thing the sky cannot say is what the *ground* puts back
//! up into a cloud base, so that is estimated here from the sun and the surface.
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
use bevy::light::GeneratedEnvironmentMapLight;
use bevy::mesh::{Mesh3d, MeshBuilder, Meshable, SphereKind};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat, TextureUsages,
};
use bevy::render::view::Msaa;
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dPlugin, MeshMaterial2d};
use std::f32::consts::PI;

/// Panorama size. 0.18° of sky a texel, against the 0.47° of the 768 × 384 this
/// replaced — which is about where a cumulus edge stops being a staircase,
/// because at ten kilometres a real one is that soft anyway.
const PANORAMA: UVec2 = UVec2::new(2048, 1024);

/// Frames one turn of the amortisation takes — the 4 × 4 Bayer pattern of
/// `clouds.wgsl`. A quarter of a second at 60 Hz, in which a cloud five
/// kilometres out drifts a fifth of a texel.
const AMORTISE: u32 = 16;

/// How long the panorama remembers \[s\]. A remarched texel is blended into
/// what it held at the weight that makes the running average forget over about
/// this long, whatever the frame rate — so the noise one march leaves is
/// averaged over as many marches as fit into a second. Long enough to take the
/// jitter and the cone wobble out of the volumetric path — at the horizon a
/// step is 300 m against wisps of 90, and one sample of that is what a 2K
/// screen showed as a raster — and short enough that a deck drifting at 25 m/s
/// five kilometres out smears by a texel or two and no more.
const HISTORY_SECONDS: f32 = 1.0;

/// The least weight a fresh march ever gets. Past a few hundred frames a second
/// the second's worth of turns would make each one nearly weightless, and the
/// panorama would lag a change of weather by more than the eye forgives.
const HISTORY_BLEND_MIN: f32 = 0.08;

/// A clock that moves by more than this between two frames has jumped — the
/// console's `time`, a new run — and taken the drift of the deck kilometres
/// with it, so the history describes a sky that is gone. A frame is a tenth of a
/// second at the very worst; a jump is minutes.
const TIME_JUMP: f64 = 10.0;

/// The fractional part of the golden ratio, and Roberts' R2 pair: the steps of
/// the best-spread sequences there are in one and in two dimensions. The first
/// walks the ray jitter from turn to turn, the second the point inside the texel
/// the ray is sent through.
const GOLDEN: f64 = 0.618_033_988_749_895;
const R2: [f64; 2] = [0.754_877_666_246_692_7, 0.569_840_290_998_053_2];

/// The layer the panorama camera and its quad live on, so the pass sees nothing
/// else and nothing else sees them.
const CLOUD_LAYER: usize = 7;

/// Radius of the dome. Inside the star sphere, so the transparent phase — which
/// sorts back to front — draws the stars first and the clouds over them.
const DOME_RADIUS: f32 = SKY_RADIUS * 0.9;

/// Side of the shape volume, and of the detail volume.
///
/// The shape's Worley octaves run up to 32 cells across it; at 64³ that finest
/// octave had two texels to a cell and was sampled as speckle rather than as
/// shape, and the detail's ran up to 64 cells over 32 texels, which is noise in
/// the strict sense — the fine grain the old clouds had in an evening sky. Four
/// texels to a cell is where a Worley cell still reads as a cell.
const SHAPE_VOLUME: u32 = 128;
const DETAIL_VOLUME: u32 = 64;

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

/// The colour of a clear sky's light, for the two places the sky itself cannot
/// be asked: the share of the ground's light that is skylight, and the floor a
/// night cloud is lit to. It is also the colour a strike lights the deck with —
/// a flash is the sky lighting the deck from the inside, only far brighter and
/// for two frames.
const SKY_TINT: Vec3 = Vec3::new(0.50, 0.62, 0.85);

/// What the sky's own light on the ground is against the sun's illuminance, as a
/// fraction of [`SUN_ILLUMINANCE`](crate::sky::SUN_ILLUMINANCE).
pub(crate) const SKY_SHARE: f32 = 0.10;
/// The least a cloud is ever lit to \[cd/m²\]: an overcast night sky over open
/// country, a hundredth of a candela — what keeps a night cloud a shape against
/// the stars instead of a hole in them once the exposure has opened for the dark.
const NIGHT_FLOOR: f32 = 0.01;

/// Albedo of what lies under the deck: fields and woods, the same when wet but
/// darker, and snow. A cloud base over a snowfield is lit nearly as brightly
/// from below as from above, which is why a winter overcast is so pale.
const GROUND_ALBEDO: Vec3 = Vec3::new(0.17, 0.19, 0.11);
const WET_DARKENING: f32 = 0.6;
const SNOW_ALBEDO: Vec3 = Vec3::new(0.80, 0.82, 0.88);

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
    /// rgb = the light that reaches the cloud layer \[lx\], a = drift time \[s\].
    pub light: Vec4,
    /// rgb = what the ground puts back up into the base of the deck \[cd/m²\],
    /// a = cover 0 … 1.
    pub ground: Vec4,
    /// rgb = the least light a cloud is ever lit by; a reserved.
    pub floor: Vec4,
    /// x = base \[m\], y = thickness \[m\], z/w = wind \[m/s\] in render space.
    pub layer: Vec4,
    /// x = the Bayer slot this frame writes, or −1 for every texel at once;
    /// y = 1 volumetric, 0 layered; z = extinction of the weather's haze
    /// \[1/m\], w = its scale height \[m\] — the march fades a far cloud into
    /// the fog the way it fades one into the blue.
    pub frame: Vec4,
    /// x = the weight of this frame's march against what the texel held (1
    /// replaces it); y = where in its golden-ratio sequence this turn's ray
    /// jitter stands, 0 … 1, and scaled up the seed of the cone wobble; z = the
    /// point in the texel this turn's ray is sent through, 0 … 1, on the
    /// panorama's x axis; w = the scenario seconds since the texels this frame
    /// marches were last marched, by which the deck has drifted since their
    /// history was written.
    pub history: Vec4,
    /// x = the point in the texel on the panorama's y axis; yzw reserved.
    pub history2: Vec4,
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
    /// The atmosphere's own view of the sky, one texel per direction — Bevy's
    /// cubemap, once a camera has one. Until then Bevy binds its white 1 × 1
    /// fallback, which at the sun's scale of illuminance is as good as black
    /// and leaves the clouds to the sun and the floor.
    #[texture(5, dimension = "cube")]
    #[sampler(6)]
    sky: Option<Handle<Image>>,
    /// Last frame's panorama — the other of the two buffers — which this
    /// frame's pass carries over texel by texel and blends its marches into,
    /// sampled where the cloud stood a turn ago.
    #[texture(7)]
    #[sampler(8)]
    history: Handle<Image>,
}

impl Material2d for CloudMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://world_render/clouds.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Opaque
    }
}

/// What the dome is told that the panorama cannot carry.
#[derive(ShaderType, Debug, Clone, Copy, Default)]
pub struct DomeParams {
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

    /// The sphere follows the camera, so its mesh centre has zero view distance.
    /// Without a bias Bevy sorts it after every other transparent mesh, putting
    /// clouds in front of alpha-blended foliage, windows and precipitation.
    /// Treat its centre as one dome radius farther away; the actual sphere depth
    /// test still keeps every opaque and alpha-masked surface in front.
    fn depth_bias(&self) -> f32 {
        -DOME_RADIUS
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

/// The two panorama buffers. Each frame one is written and the other read, and
/// [`update`] swaps them — a pass cannot sample the texture it renders into,
/// and the blend needs what the texel held.
#[derive(Resource)]
struct Panorama {
    buffers: [Handle<Image>; 2],
}

/// Marks the camera that draws the panorama, so [`update`] can point it at the
/// buffer whose turn it is.
#[derive(Component)]
struct PanoramaCamera;

/// The scenario clock at each of the last [`AMORTISE`] frames, so a march can
/// be told how long ago its texel was last marched — one turn, whatever the
/// frame rate did in between — and read its history where the deck has drifted
/// from.
#[derive(Default)]
struct TurnClock {
    seconds: [f64; AMORTISE as usize],
}

impl TurnClock {
    /// Records `now` for `frame` and returns the seconds since the texels this
    /// frame marches were last marched. Zero through the warm-up: every texel
    /// is marched every frame there, and a frame's drift is not worth a
    /// resample.
    fn advance(&mut self, frame: u32, now: f64) -> f32 {
        let slot = (frame % AMORTISE) as usize;
        let delta = if frame >= AMORTISE {
            (now - self.seconds[slot]).max(0.0)
        } else {
            0.0
        };
        self.seconds[slot] = now;
        delta as f32
    }
}

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
    let buffers = [images.add(panorama_target()), images.add(panorama_target())];
    let material = clouds.add(CloudMaterial {
        params: CloudParams::default(),
        shape: images.add(noise_volume(SHAPE_VOLUME, 4.0, 7, Shape::Dense)),
        // Worley at 4, 8 and 16 cells over the 1.4 km the shader tiles it on: a
        // 350 m billow down to an 87 m wisp. Only its Worley channels are read.
        detail: images.add(noise_volume(DETAIL_VOLUME, 2.0, 23, Shape::Dense)),
        sky: None,
        history: buffers[1].clone(),
    });

    // A camera of its own, on its own layer, rendering one quad — the cheapest
    // offscreen pass Bevy offers without a render-graph node. `order: -1` puts it
    // in front of the world camera that samples what it wrote.
    let layer = RenderLayers::layer(CLOUD_LAYER);
    commands.spawn((
        crate::Persistent,
        PanoramaCamera,
        Camera2d,
        Camera {
            order: -1,
            // Not cleared: every texel is written each frame, marched or carried
            // over from the other buffer, so a clear would only be paid for.
            clear_color: ClearColorConfig::None,
            ..default()
        },
        // One quad over the whole target has no edge to multisample; the
        // default four samples would cost a resolve and a panorama four times
        // the size, every frame.
        Msaa::Off,
        RenderTarget::Image(buffers[0].clone().into()),
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
            panorama: buffers[0].clone(),
            params: DomeParams::default(),
        })),
        Transform::from_scale(Vec3::splat(DOME_RADIUS)),
        Visibility::default(),
    ));
    commands.insert_resource(Panorama { buffers });
}

/// One of the two images the march writes into: half float, because a sunlit
/// cloud edge is well above 1, and wrapped in longitude so the seam behind the
/// observer closes.
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

/// How the red channel of a [`noise_volume`] combines its Perlin with the
/// coarsest of its Worley octaves.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Shape {
    /// The Perlin *masked* to the Worley's cells, zero between them: a field of
    /// separate patches — a ground mist that lies in some hollows and not in
    /// others.
    Masked,
    /// The Perlin *and* the Worley, centred on a half: a field that is somewhere
    /// on every point, so a threshold on it runs from a few fair-weather cumulus
    /// all the way to a closed deck. A masked field cannot close — where the
    /// mask is zero no threshold finds a cloud — which is what left an overcast
    /// sky full of holes.
    Dense,
}

/// A tileable 3D noise volume: `r` is a Perlin-Worley mix, `gba` are Worley
/// octaves at twice, four and eight times `frequency` — the four channels a
/// cloud shape needs, in one RGBA8 lookup.
///
/// Generated rather than shipped, like the ground textures: the repository
/// carries no binary assets. Every core takes a slab of the volume, and each
/// Worley octave lays its feature points out once instead of hashing them again
/// for every voxel and each of its 27 neighbours — 128³ is a few seconds on one
/// core and a fraction of one on all of them.
pub(crate) fn noise_volume(size: u32, frequency: f32, seed: u64, shape: Shape) -> Image {
    let n = size as f32;
    let octaves: [WorleyCells; 3] = std::array::from_fn(|i| {
        WorleyCells::new(frequency * 2.0f32.powi(i as i32 + 1), seed + i as u64)
    });
    let slab = (size * size * 4) as usize;
    let mut data = vec![0u8; slab * size as usize];
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let slabs_per_thread = (size as usize).div_ceil(threads);
    std::thread::scope(|scope| {
        for (chunk_index, chunk) in data.chunks_mut(slab * slabs_per_thread).enumerate() {
            let octaves = &octaves;
            scope.spawn(move || {
                for (dz, slice) in chunk.chunks_mut(slab).enumerate() {
                    let z = (chunk_index * slabs_per_thread + dz) as f32;
                    for y in 0..size {
                        for x in 0..size {
                            let p = Vec3::new(x as f32, y as f32, z) / n;
                            let perlin = fbm(p, frequency, seed);
                            // Worley inverted: a cloud is a billow *around* a
                            // feature point, not the space between them.
                            let worley: [f32; 3] =
                                std::array::from_fn(|i| 1.0 - octaves[i].distance(p));
                            // Either way the Worley's cells are what give a
                            // cumulus its billows instead of a fog bank.
                            let mixed = match shape {
                                Shape::Masked => remap(perlin, 1.0 - worley[0], 1.0, 0.0, 1.0),
                                Shape::Dense => perlin + worley[0] - 0.5,
                            }
                            .clamp(0.0, 1.0);
                            let index = ((y * size + x) * 4) as usize;
                            slice[index] = (mixed * 255.0) as u8;
                            for (i, w) in worley.iter().enumerate() {
                                slice[index + 1 + i] = (w.clamp(0.0, 1.0) * 255.0) as u8;
                            }
                        }
                    }
                }
            });
        }
    });
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

/// Gradient noise over a wrapping lattice, three octaves — the soft masses the
/// Worley then carves into billows, in 0 … 1.
///
/// Gradient rather than value noise on purpose: value noise has its extremes
/// *on* the lattice points and its slopes between them, and clouds grown from it
/// come out as evenly spaced lumps with the grid showing through. Perlin's
/// gradients put the extremes between the points, where nothing lines up.
fn fbm(p: Vec3, frequency: f32, seed: u64) -> f32 {
    let mut sum = 0.0;
    let mut amplitude = 0.5;
    let mut f = frequency;
    for octave in 0..3 {
        sum += amplitude * perlin(p, f, seed + octave);
        amplitude *= 0.5;
        f *= 2.0;
    }
    // Three octaves of ±1 noise rarely leave ±0.3 in practice; this is what
    // spreads them over the byte without clipping much of either tail.
    (0.5 + sum * 1.5).clamp(0.0, 1.0)
}

/// Perlin's gradient noise on a lattice of `frequency` cells that wraps, in
/// about −1 … 1.
fn perlin(p: Vec3, frequency: f32, seed: u64) -> f32 {
    let period = frequency.max(1.0) as i64;
    let scaled = p * frequency;
    let cell = scaled.floor();
    let f = scaled - cell;
    // Perlin's quintic fade: no first or second derivative at the lattice, so
    // nothing creases there.
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let corner = |dx: i64, dy: i64, dz: i64| {
        let g = gradient(
            (cell.x as i64 + dx).rem_euclid(period),
            (cell.y as i64 + dy).rem_euclid(period),
            (cell.z as i64 + dz).rem_euclid(period),
            seed,
        );
        g.dot(f - Vec3::new(dx as f32, dy as f32, dz as f32))
    };
    let mix = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let x00 = mix(corner(0, 0, 0), corner(1, 0, 0), u.x);
    let x10 = mix(corner(0, 1, 0), corner(1, 1, 0), u.x);
    let x01 = mix(corner(0, 0, 1), corner(1, 0, 1), u.x);
    let x11 = mix(corner(0, 1, 1), corner(1, 1, 1), u.x);
    mix(mix(x00, x10, u.y), mix(x01, x11, u.y), u.z)
}

/// One of Perlin's twelve edge gradients, picked by the lattice point.
fn gradient(x: i64, y: i64, z: i64, seed: u64) -> Vec3 {
    const GRADIENTS: [Vec3; 12] = [
        Vec3::new(1.0, 1.0, 0.0),
        Vec3::new(-1.0, 1.0, 0.0),
        Vec3::new(1.0, -1.0, 0.0),
        Vec3::new(-1.0, -1.0, 0.0),
        Vec3::new(1.0, 0.0, 1.0),
        Vec3::new(-1.0, 0.0, 1.0),
        Vec3::new(1.0, 0.0, -1.0),
        Vec3::new(-1.0, 0.0, -1.0),
        Vec3::new(0.0, 1.0, 1.0),
        Vec3::new(0.0, -1.0, 1.0),
        Vec3::new(0.0, 1.0, -1.0),
        Vec3::new(0.0, -1.0, -1.0),
    ];
    GRADIENTS[(hash01(x, y, z, seed) * 12.0) as usize % 12]
}

/// One feature point per cell of a wrapping lattice, laid out once — the
/// cellular noise whose *inverse* looks like a cloud's billows.
struct WorleyCells {
    period: usize,
    /// Where in its cell each point sits, 0 … 1 on every axis, `z`-major.
    points: Vec<Vec3>,
}

impl WorleyCells {
    fn new(frequency: f32, seed: u64) -> Self {
        let period = frequency.max(1.0) as usize;
        let points = (0..period * period * period)
            .map(|i| {
                let x = (i % period) as i64;
                let y = ((i / period) % period) as i64;
                let z = (i / (period * period)) as i64;
                Vec3::new(
                    hash01(x, y, z, seed),
                    hash01(x, y, z, seed + 101),
                    hash01(x, y, z, seed + 211),
                )
            })
            .collect();
        Self { period, points }
    }

    /// Distance from `p` (0 … 1 on every axis) to the nearest point, in cells
    /// and capped at one.
    fn distance(&self, p: Vec3) -> f32 {
        let period = self.period as i64;
        let scaled = p * self.period as f32;
        let cell = scaled.floor();
        let wrap = |v: f32| (v as i64).rem_euclid(period) as usize;
        let mut nearest = f32::MAX;
        for dz in -1..=1 {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let neighbour = cell + Vec3::new(dx as f32, dy as f32, dz as f32);
                    let index = (wrap(neighbour.z) * self.period + wrap(neighbour.y)) * self.period
                        + wrap(neighbour.x);
                    let offset = self.points[index];
                    nearest = nearest.min((neighbour + offset - scaled).length_squared());
                }
            }
        }
        nearest.sqrt().min(1.0)
    }
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

/// Feeds the march from the sky, turns the amortisation over, swaps the two
/// panorama buffers and keeps the dome on the camera.
#[allow(clippy::too_many_arguments)]
fn update(
    sky: Res<Sky>,
    quality: Res<Quality>,
    time: Res<Time>,
    panorama: Res<Panorama>,
    mut materials: ResMut<Assets<CloudMaterial>>,
    mut domes: ResMut<Assets<DomeMaterial>>,
    handles: Query<&MeshMaterial2d<CloudMaterial>>,
    dome_handles: Query<&MeshMaterial3d<DomeMaterial>>,
    sun: Query<&Transform, (With<Sun>, Without<Dome>)>,
    camera: Query<&GlobalTransform, (With<Camera3d>, Without<Dome>)>,
    // The atmosphere's cubemap: Bevy puts this on every camera that asked for an
    // `AtmosphereEnvironmentMapLight`, and the handle in it is the sky itself.
    sky_map: Query<&GeneratedEnvironmentMapLight>,
    mut dome: Query<&mut Transform, (With<Dome>, Without<Sun>)>,
    mut target: Query<&mut RenderTarget, With<PanoramaCamera>>,
    mut frame: Local<u32>,
    mut last_seconds: Local<Option<f64>>,
    mut clock: Local<TurnClock>,
) {
    // The sun's own transform looks *along* the light, so the direction towards
    // it is the other way.
    let sun_dir = sun
        .iter()
        .next()
        .map_or(Vec3::Y, |t| -t.forward().as_vec3());
    let weather = sky.weather;
    let daylight = ((sun_dir.y * 90.0f32.to_radians().sin() + 0.1) * 4.0).clamp(0.0, 1.0);

    // A panorama that holds nothing — the first frames, the frames after the
    // setting was dialled and every texel means something else now, or after
    // the clock jumped and took the deck kilometres along — has to be marched
    // whole, because there is no older frame worth keeping.
    let jumped = last_seconds.is_some_and(|last| (sky.seconds - last).abs() > TIME_JUMP);
    *last_seconds = Some(sky.seconds);
    *frame = if quality.is_changed() || jumped {
        0
    } else {
        // Wrapping on purpose: the wrap is one more warm-up, two years in.
        frame.wrapping_add(1)
    };
    let writing = writing_slot(*frame).map_or(-1.0, |slot| slot as f32);
    let (blend, turn) = history(*frame, time.delta_secs());
    let sequence = sequence(turn);
    let turn_seconds = clock.advance(*frame, sky.seconds);

    let params = CloudParams {
        sun: sun_dir.extend(daylight),
        // Illuminance, not radiance: the march multiplies it by a phase function
        // that is normalised over the sphere, so the 1/4π lives there.
        light: (sunlight(sun_dir.y) * crate::sky::SUN_ILLUMINANCE).extend(sky.seconds as f32),
        ground: ground_light(sun_dir.y, daylight, weather.cover, sky.wetness, sky.snow)
            .extend(weather.cover),
        // Never quite black, or a night cloud would be a hole in the stars. The
        // lightning is *not* in here — it is on the dome, where every frame runs.
        floor: (SKY_TINT * NIGHT_FLOOR).extend(0.0),
        layer: Vec4::new(
            weather.base,
            // A closed deck is a thick one; a fair-weather cumulus is not.
            600.0 + 1_400.0 * weather.cover,
            -weather.bearing.sin() * weather.wind * WIND_ALOFT,
            weather.bearing.cos() * weather.wind * WIND_ALOFT,
        ),
        frame: Vec4::new(
            writing,
            f32::from(u8::from(quality.volumetric)),
            crate::sky::haze_extinction(weather.visibility),
            crate::sky::haze_height(weather.fog_depth),
        ),
        history: Vec4::new(blend, sequence.x, sequence.y, turn_seconds),
        history2: Vec4::new(sequence.z, 0.0, 0.0, 0.0),
    };
    // The cubemap arrives with the first camera and is a new image with every
    // run's camera; the march follows whichever one there is.
    let sky_map = sky_map
        .iter()
        .next()
        .map(|light| light.environment_map.clone());
    // The buffers swap roles every frame: this frame's pass reads what last
    // frame's wrote and writes the other, and the dome shows the one just
    // written. Which is which does not matter on the frame after a reset — the
    // blend is 1 there and the history is not looked at.
    let (write, read) = if frame.is_multiple_of(2) {
        (0, 1)
    } else {
        (1, 0)
    };
    for handle in &handles {
        if let Some(mut material) = materials.get_mut(&handle.0) {
            material.params = params;
            material.history = panorama.buffers[read].clone();
            if material.sky != sky_map {
                material.sky = sky_map.clone();
            }
        }
    }
    for mut target in &mut target {
        *target = RenderTarget::Image(panorama.buffers[write].clone().into());
    }

    let dome_params = DomeParams {
        // A strike lights the whole deck from the inside, which is what a
        // thunderstorm looks like from under it — the channel itself is behind
        // the cloud far more often than not.
        flash: (SKY_TINT * crate::sky::SUN_ILLUMINANCE * 0.9 * sky.flash).extend(0.0),
    };
    for handle in &dome_handles {
        if let Some(mut dome) = domes.get_mut(&handle.0) {
            dome.params = dome_params;
            dome.panorama = panorama.buffers[write].clone();
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

/// What this frame's march weighs against the texel's history, and which turn
/// of the jitter sequence it takes.
///
/// The first frame after a reset replaces the history outright. The frames of
/// the warm-up turn — every texel marched, every frame — average exactly,
/// `1/n`, until that drops under the running weight, which is one turn's share
/// of [`HISTORY_SECONDS`] at the current frame rate: more frames a second are
/// more marches to average, and each of them counts for less. The turn moves on
/// with every march of a texel — every frame while warming up, once a turn
/// after — so no two marches blended together share a jitter.
fn history(frame: u32, delta_secs: f32) -> (f32, u32) {
    let running = (AMORTISE as f32 * delta_secs / HISTORY_SECONDS).clamp(HISTORY_BLEND_MIN, 1.0);
    let blend = (1.0 / (frame as f32 + 1.0)).max(running);
    let turn = if frame < AMORTISE {
        frame
    } else {
        AMORTISE + frame / AMORTISE
    };
    (blend, turn)
}

/// Where a turn stands in its sequences: x = the ray jitter's offset, yz = the
/// point in the texel, all 0 … 1. Taken in f64 and only the fractions sent — a
/// turn count times an irrational outgrows an f32's fraction within an hour.
fn sequence(turn: u32) -> Vec3 {
    let turn = f64::from(turn);
    Vec3::new(
        (turn * GOLDEN).fract() as f32,
        (0.5 + turn * R2[0]).fract() as f32,
        (0.5 + turn * R2[1]).fract() as f32,
    )
}

/// The colour of the sunlight that reaches the cloud layer, from the sun's
/// elevation alone — Kasten & Young's air mass through the same zenith optical
/// depth the star shader uses, so the two agree on what a low sun looks like.
pub(crate) fn sunlight(sin_elevation: f32) -> Vec3 {
    const ZENITH_OPTICAL_DEPTH: Vec3 = Vec3::new(0.081, 0.150, 0.315);
    let degrees = sin_elevation.clamp(-1.0, 1.0).asin().to_degrees();
    let air_mass =
        1.0 / (sin_elevation.max(0.0) + 0.50572 * (degrees + 6.07995).max(0.5).powf(-1.6364));
    let transmittance = (-ZENITH_OPTICAL_DEPTH * air_mass).exp();
    // Below the horizon the earth is in the way; the fade is the last light on a
    // cloud base after sunset.
    transmittance * smoothstep(-0.08, 0.02, sin_elevation)
}

/// What the ground puts back up into the base of the deck \[cd/m²\]: its albedo
/// times what the sun and the sky put down on it, over π.
///
/// The sun's share is the direct light on level ground less what the deck
/// itself takes — the same share `sky::update` takes off the light — and the
/// sky's share is the estimate the night floor is built on, since the cubemap
/// cannot be read from here. A snowfield sends most of both back up, which is
/// why a winter overcast is so much paler than a summer one.
fn ground_light(sin_elevation: f32, daylight: f32, cover: f32, wetness: f32, snow: f32) -> Vec3 {
    let albedo = GROUND_ALBEDO
        .lerp(GROUND_ALBEDO * WET_DARKENING, wetness)
        .lerp(SNOW_ALBEDO, snow);
    let under_deck = 1.0 - 0.85 * cover;
    let sun = sunlight(sin_elevation) * crate::sky::SUN_ILLUMINANCE * sin_elevation.max(0.0) / PI;
    let sky = SKY_TINT * crate::sky::SUN_ILLUMINANCE * SKY_SHARE * daylight;
    albedo * (sun + sky) * under_deck
}

pub(crate) fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dome_sorts_behind_transparent_world_geometry() {
        let dome = DomeMaterial {
            panorama: Handle::default(),
            params: DomeParams::default(),
        };
        assert_eq!(dome.depth_bias(), -DOME_RADIUS);
    }

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

    /// The blend replaces the history on the first frame after a reset, averages
    /// the warm-up exactly, and settles on one turn's share of a second — never
    /// under the floor, never over one.
    #[test]
    fn the_history_forgets_over_about_a_second() {
        let dt = 1.0 / 60.0;
        assert_eq!(
            history(0, dt).0,
            1.0,
            "nothing worth keeping on the first frame"
        );
        assert!(
            (history(1, dt).0 - 0.5).abs() < 1e-6,
            "the second frame is averaged with the first"
        );
        let settled = history(AMORTISE * 4, dt).0;
        assert!(
            (settled - AMORTISE as f32 * dt / HISTORY_SECONDS).abs() < 1e-6,
            "a turn's share of a second: {settled}"
        );
        assert_eq!(
            history(AMORTISE * 4, 1.0 / 240.0).0,
            HISTORY_BLEND_MIN,
            "floored at high frame rates"
        );
        assert_eq!(
            history(AMORTISE * 4, 0.5).0,
            1.0,
            "a slideshow has no history worth keeping"
        );
    }

    /// Every march of one texel has to stand somewhere new in the jitter
    /// sequence — the warm-up frames one after the other, and after that once a
    /// turn — or the blend averages the same sample with itself.
    #[test]
    fn the_jitter_sequence_moves_on_with_every_march() {
        let dt = 1.0 / 60.0;
        let mut turns: Vec<u32> = (0..AMORTISE).map(|frame| history(frame, dt).1).collect();
        // Slot 3's marches after the warm-up.
        turns.extend((1..5).map(|turn| history(AMORTISE * turn + 3, dt).1));
        let distinct: std::collections::BTreeSet<u32> = turns.iter().copied().collect();
        assert_eq!(
            distinct.len(),
            turns.len(),
            "a turn came up twice: {turns:?}"
        );
        // And the two frames of one turn, marching different slots, share it.
        assert_eq!(
            history(AMORTISE * 2 + 3, dt).1,
            history(AMORTISE * 2 + 9, dt).1
        );
    }

    /// The sequences stay inside the step and the texel, and over as many turns
    /// as the history spans no two of them come close — a repeat would be the
    /// same sample averaged with itself.
    #[test]
    fn the_sequences_spread_out() {
        let points: Vec<Vec3> = (0..64).map(sequence).collect();
        for p in &points {
            assert!(
                p.min_element() >= 0.0 && p.max_element() < 1.0,
                "{p} out of range"
            );
        }
        for (i, a) in points.iter().enumerate() {
            for b in &points[i + 1..] {
                assert!((a.x - b.x).abs() > 1e-3, "jitter repeats: {a} / {b}");
                let d = ((a.y - b.y).powi(2) + (a.z - b.z).powi(2)).sqrt();
                assert!(d > 1e-2, "texel point repeats: {a} / {b}");
            }
        }
    }

    /// A march is told how long ago its texel was last marched: nothing while
    /// warming up, a turn's worth of clock after — sixteen frames, however long
    /// they took.
    #[test]
    fn the_turn_clock_measures_a_turn() {
        let mut clock = TurnClock::default();
        let mut now = 100.0;
        for frame in 0..AMORTISE {
            assert_eq!(clock.advance(frame, now), 0.0, "warm-up frame {frame}");
            now += 1.0 / 60.0;
        }
        for frame in AMORTISE..AMORTISE * 3 {
            let turn = clock.advance(frame, now);
            assert!(
                (turn - AMORTISE as f32 / 60.0).abs() < 1e-4,
                "frame {frame}: {turn}"
            );
            now += 1.0 / 60.0;
        }
        // A stutter is measured, not assumed.
        now += 0.5;
        let turn = clock.advance(AMORTISE * 3, now);
        assert!(
            (turn - (AMORTISE as f32 / 60.0 + 0.5)).abs() < 1e-4,
            "{turn}"
        );
    }

    #[test]
    fn the_noise_wraps() {
        // A volume that does not tile shows its seams as straight lines in the sky.
        let cells = WorleyCells::new(4.0, 1);
        for (a, b) in [
            (Vec3::new(0.0, 0.3, 0.7), Vec3::new(1.0, 0.3, 0.7)),
            (Vec3::new(0.2, 0.0, 0.5), Vec3::new(0.2, 1.0, 0.5)),
            (Vec3::new(0.4, 0.6, 0.0), Vec3::new(0.4, 0.6, 1.0)),
        ] {
            assert!(
                (perlin(a, 4.0, 1) - perlin(b, 4.0, 1)).abs() < 1e-5,
                "perlin seam at {a} / {b}"
            );
            assert!(
                (cells.distance(a) - cells.distance(b)).abs() < 1e-5,
                "worley seam at {a} / {b}"
            );
        }
    }

    /// Gradient noise is zero *on* its lattice and swings between; a wrong
    /// normalisation shows as a shape channel that is all floor or all ceiling.
    #[test]
    fn the_noise_fills_its_range_without_living_at_the_ends() {
        let mut lo = f32::MAX;
        let mut hi = f32::MIN;
        let mut sum = 0.0;
        let mut clipped = 0;
        let n = 24;
        for z in 0..n {
            for y in 0..n {
                for x in 0..n {
                    let p = Vec3::new(x as f32, y as f32, z as f32) / n as f32;
                    let v = fbm(p, 4.0, 3);
                    lo = lo.min(v);
                    hi = hi.max(v);
                    sum += v;
                    if v <= 0.0 || v >= 1.0 {
                        clipped += 1;
                    }
                }
            }
        }
        let mean = sum / (n * n * n) as f32;
        assert!(lo < 0.25 && hi > 0.75, "too narrow: {lo} … {hi}");
        assert!((mean - 0.5).abs() < 0.08, "off centre: mean {mean}");
        assert!(
            clipped < n * n * n / 20,
            "{clipped} of {} samples pinned to an end",
            n * n * n
        );
    }

    #[test]
    fn sunlight_reddens_and_goes_out() {
        let noon = sunlight(1.0);
        let low = sunlight(0.05);
        assert!(noon.x > 0.9 && noon.z > 0.7, "white at the zenith: {noon}");
        assert!(low.x > low.z * 2.0, "red near the horizon: {low}");
        assert_eq!(sunlight(-0.2), Vec3::ZERO, "gone once it has set");
    }

    /// Snow turns a cloud base pale, and night turns the ground off.
    #[test]
    fn the_ground_lights_a_cloud_base_by_what_lies_on_it() {
        let summer = ground_light(0.8, 1.0, 0.4, 0.0, 0.0);
        let winter = ground_light(0.3, 1.0, 0.9, 0.0, 1.0);
        assert!(
            winter.z > summer.z,
            "snow under a low sun outshines fields under a high one: {winter} vs {summer}"
        );
        assert!(summer.y > summer.z, "fields are green: {summer}");
        assert_eq!(
            ground_light(-0.3, 0.0, 0.4, 0.0, 0.0),
            Vec3::ZERO,
            "dark at night"
        );
    }

    #[test]
    fn the_volume_has_every_channel() {
        let image = noise_volume(8, 2.0, 5, Shape::Dense);
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

    /// The slabs the threads take must tile the volume exactly: a size that does
    /// not divide by the core count is the common case, not the corner one.
    #[test]
    fn the_volume_is_the_same_whoever_builds_it() {
        let a = noise_volume(13, 2.0, 9, Shape::Masked);
        let b = noise_volume(13, 2.0, 9, Shape::Masked);
        assert_eq!(a.data, b.data);
        let data = a.data.as_ref().expect("generated in memory");
        // The last slab is the one a miscount would leave blank.
        let last = &data[12 * 13 * 13 * 4..];
        assert!(last.iter().any(|&v| v != 0), "last slab never written");
    }
}
