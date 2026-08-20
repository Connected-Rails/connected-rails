//! The sky (plan ch. 14): a physically based atmosphere, the sun and the moon
//! where the date and the place put them, and the real stars behind both.
//!
//! Four pieces, each the cheapest one that is still the real thing:
//!
//! * **Atmosphere.** Bevy's own implementation of [Hillaire 2020], the technique
//!   Unreal's Sky Atmosphere is built on: small look-up tables (transmittance,
//!   multiple scattering) that are rebuilt every frame from the sun's direction.
//!   Rayleigh and Mie scattering come out of it, and with them the blue noon, the
//!   red sunset, the blue hour after it and the haze that lies over a valley ten
//!   kilometres away. The view itself is ray marched against those tables
//!   ([`AtmosphereMode::Raymarched`]) rather than read out of a sky-view table,
//!   because the table path cannot carry the weather's haze — see `update`.
//! * **Sun.** One directional light, aimed from the date, the clock and the place
//!   ([`world_coords::sun`]). Bevy draws its disk into the atmosphere itself
//!   ([`SunDisk`]) and applies the atmospheric extinction to it, so the sun reddens
//!   and swells at the horizon without anything here saying so.
//! * **Moon.** A second, dim directional light for the ground, plus a disk half a
//!   degree wide in the sky. The disk is shaded from the *real* sun direction, so
//!   its phase, and the way the lit edge points at the sun below the horizon, are
//!   the ones the almanac gives.
//! * **Stars.** The 8 900 naked-eye stars of the HYG catalogue (`stars.bin`,
//!   baked by `tools/gen_stars.py`), one point sprite each, held in J2000
//!   equatorial coordinates and turned into the local sky by the observer's
//!   latitude and the sidereal time. Orion stands where it stands, the pole star
//!   sits at the latitude's altitude, and everything turns four minutes a day
//!   against the clock. The Milky Way is added as a band of procedural faint ones.
//!
//! Everything hangs off [`Sky`] — date, time and the observer's latitude and
//! longitude. The simulator fills it from the scenario clock and the render
//! origin, the route editor from its own time panel and the module's anchor.
//!
//! **Multiplayer.** Nothing here is state: the sky is a pure function of the
//! scenario clock, which is already the same on every machine, and of the place,
//! which is the line. Two clients standing next to each other see the same sky
//! without a single byte crossing the wire.
//!
//! [Hillaire 2020]: https://sebh.github.io/publications/egsr2020.pdf

use crate::Daylight;
use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::camera::Camera3d;
use bevy::light::atmosphere::{Falloff, PhaseFunction, ScatteringMedium, ScatteringTerm};
use bevy::light::{Atmosphere, AtmosphereEnvironmentMapLight, SunDisk, light_consts::lux};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{AtmosphereMode, AtmosphereSettings};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use sim_core::weather::Weather;
use std::f32::consts::TAU;
use world_coords::sun;

/// Registers the sky's materials and its update. Part of
/// [`WorldRenderPlugin`](crate::WorldRenderPlugin) — both programs draw the same sky.
pub(crate) fn plugin(app: &mut App) {
    embedded_asset!(app, "stars.wgsl");
    embedded_asset!(app, "moon.wgsl");
    app.add_plugins((
        MaterialPlugin::<StarMaterial>::default(),
        MaterialPlugin::<MoonMaterial>::default(),
    ))
    .init_resource::<Sky>()
    .add_systems(Update, update);
}

/// When and where the sky is seen from — the whole input of the system.
///
/// Both programs overwrite it every frame: the simulator from the scenario's start
/// date plus the running clock and from the render origin, the route editor from
/// its time panel and from the module's anchor.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct Sky {
    pub year: i32,
    /// 1–12.
    pub month: u32,
    /// 1–31.
    pub day: u32,
    /// Seconds since local midnight. A run that passes midnight simply counts on;
    /// the Julian date takes any number of seconds.
    pub seconds: f64,
    /// Local clock ahead of UT \[h\] — Germany: 1 in winter, 2 in summer.
    pub utc_offset: f64,
    /// Observer's geodetic latitude \[rad\]. This is what decides how high the pole
    /// star stands and how steeply the sun comes up.
    pub latitude: f64,
    /// Observer's geodetic longitude \[rad\].
    pub longitude: f64,
    /// Surface water the rain has left, 0 … 1 — `sim_core::weather::Timeline`
    /// integrates it, the material shaders read it (`crate::weather`).
    pub wetness: f32,
    /// Lying snow, 0 = bare … 1 = closed cover. Same journey.
    pub snow: f32,
    /// How much of the sun the clouds are taking away right here, 0 … 1.
    /// Written by the cloud pass; 0 while there is none.
    pub cloud_shadow: f32,
    /// A lightning channel lighting the sky, 1 … 0
    /// (`sim_core::weather::Strike::brightness`).
    pub flash: f32,
    /// The weather (plan 14.1). The sky reads three things out of it: the cover,
    /// which dims the sun and puts out the stars; the visibility, which becomes a
    /// scattering term of the atmosphere itself; and the depth of a ground fog
    /// layer, which decides how high that term reaches.
    pub weather: Weather,
}

impl Default for Sky {
    /// Midsummer noon over central Germany — the same date and place as
    /// `sim_core::scenario::StartTime::default`, so an editor that says nothing
    /// shows the module in the light the simulator would.
    fn default() -> Self {
        Self {
            year: 2026,
            month: 6,
            day: 21,
            seconds: 12.0 * 3600.0,
            utc_offset: 2.0,
            latitude: 52.0_f64.to_radians(),
            longitude: 10.0_f64.to_radians(),
            wetness: 0.0,
            snow: 0.0,
            cloud_shadow: 0.0,
            flash: 0.0,
            weather: Weather::default(),
        }
    }
}

impl Sky {
    /// Hour of the local clock (0–23) — what the editor's time panel edits.
    pub fn hour(&self) -> u32 {
        (self.seconds.div_euclid(3600.0).rem_euclid(24.0)) as u32
    }

    /// Minute of the local clock (0–59).
    pub fn minute(&self) -> u32 {
        (self.seconds.div_euclid(60.0).rem_euclid(60.0)) as u32
    }

    /// Sets the local clock, keeping the date.
    pub fn set_clock(&mut self, hour: u32, minute: u32) {
        self.seconds = f64::from(hour * 3600 + minute * 60);
    }

    /// Julian date of the moment — the one number all the astronomy takes.
    pub fn julian_date(&self) -> f64 {
        sun::julian_date(
            self.year,
            self.month,
            self.day,
            self.seconds - self.utc_offset * 3600.0,
        )
    }
}

/// The directional light that is the sun. Its disk is drawn by the atmosphere.
#[derive(Component)]
pub struct Sun {
    /// What the graphics setting said. The cover switches the sun's shadows off
    /// and on again, and without this the switch would only work once.
    pub shadows: bool,
}

/// The second, dim directional light: what the moon puts on the ground.
#[derive(Component)]
pub struct Moon;

/// The celestial sphere the stars sit on. Follows the camera's position and
/// carries the whole sky's rotation.
#[derive(Component)]
struct Stars;

/// The moon's disk in the sky, half a degree wide.
#[derive(Component)]
struct MoonDisk;

/// Radius the star sphere and the moon disk are placed at \[m\].
///
/// Far enough that terrain in front of them occludes them, near enough to stay
/// inside both programs' far planes (20 km in the simulator, 60 km in the editor).
/// The bodies are unit-sized in the mesh and scaled here, so their angular size
/// does not depend on it.
pub(crate) const SKY_RADIUS: f32 = 9_000.0;

/// Illuminance the sun is given \[lx\].
///
/// Physically this is [`lux::RAW_SUNLIGHT`] (130 klx above the atmosphere), and
/// that is what the scattering model wants: Bevy hands the atmosphere the light's
/// own colour and applies the transmittance itself, so anything taken off the light
/// is taken off the sky with it. The value below is the raw figure scaled by 0.15 —
/// an exposure choice written into the light, because the camera's exposure is
/// fixed at Bevy's default and the rest of the night lighting is tuned to it.
///
/// ponytail: one constant instead of an EV curve over the whole day. The correct
/// version is `lux::RAW_SUNLIGHT` here plus an `Exposure` that rides from EV 13 at
/// noon down to the night — worth doing once the artificial lights (headlights, cab
/// lamps, the `_NIGHT` emissives in the mods) are in physical units too.
pub(crate) const SUN_ILLUMINANCE: f32 = lux::RAW_SUNLIGHT * 0.15;

/// Illuminance of a full moon at the zenith \[lx\].
///
/// ponytail: a real full moon is 0.25 lx and would be pure black without eye
/// adaptation — the night is lit artistically bright instead, as it already was.
const MOON_ILLUMINANCE: f32 = 40.0;

/// Luminance of the fully lit moon's disk \[cd/m²\] — the real figure, which at
/// the camera's fixed exposure lands almost exactly where it should: a disk that
/// clips to white where the sun is on it, an earthshine grey where it is not.
const MOON_LUMINANCE: f32 = 2_500.0;

/// Rendered peak luminance of a magnitude-0 star's sprite \[cd/m²\]. Every other
/// star is this times `10^(-0.4 · magnitude)`, so the catalogue's own brightness
/// ratios survive and a constellation reads by its shape.
///
/// ponytail: physically the sprite would be about 1.7 cd/m², and at the camera's
/// fixed exposure that is black. The stars are the one thing here that has to be
/// lifted wholesale — the moon's own figure needs no such help — and the lift is
/// a constant rather than a curve because the exposure never moves either.
const STAR_LUMINANCE: f32 = 12_000.0;

/// Angular diameter a star's point sprite is drawn at \[rad\]. A star is a point
/// source; this is the width of the blur a lens gives it, chosen at about two
/// pixels so it neither shimmers nor turns into a blob.
const STAR_SIZE: f32 = 0.0016;

/// Mean angular diameter of the moon \[rad\] — 31 arcminutes.
const MOON_SIZE: f32 = 0.009_04;

/// Spawns the whole sky: atmosphere, the two lights, the stars and the moon's disk.
/// The camera additionally needs [`camera_settings`].
pub fn spawn(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    media: &mut Assets<ScatteringMedium>,
    star_materials: &mut Assets<StarMaterial>,
    moon_materials: &mut Assets<MoonMaterial>,
    shadows: bool,
) {
    // One planet. Its default transform puts the ground at y = 0, which is where
    // the render origin sits.
    commands.spawn(Atmosphere::earth(
        media.add(ScatteringMedium::earth(LUT_RESOLUTION, LUT_RESOLUTION)),
    ));

    commands.spawn((
        Sun { shadows },
        DirectionalLight {
            illuminance: SUN_ILLUMINANCE,
            shadow_maps_enabled: shadows,
            ..default()
        },
        SunDisk::EARTH,
        Transform::default(),
    ));
    commands.spawn((
        Moon,
        DirectionalLight {
            illuminance: 0.0,
            color: Color::srgb(0.75, 0.82, 1.0),
            // The moon's own disk is drawn below; a second sun disk would be a
            // second sun.
            ..default()
        },
        SunDisk::OFF,
        Transform::default(),
    ));

    commands.spawn((
        Stars,
        Mesh3d(meshes.add(star_mesh())),
        MeshMaterial3d(star_materials.add(StarMaterial::default())),
        Transform::from_scale(Vec3::splat(SKY_RADIUS)),
        // `Mesh3d` asks for a `Transform` and nothing else — an entity without a
        // `Visibility` never reaches a render phase at all.
        Visibility::default(),
    ));
    commands.spawn((
        MoonDisk,
        Mesh3d(meshes.add(disk_mesh())),
        MeshMaterial3d(moon_materials.add(MoonMaterial::default())),
        Transform::default(),
        Visibility::default(),
    ));
}

/// What a camera needs to see the sky: the atmosphere for this view, and the
/// image-based light the sky itself casts back into the scene.
pub fn camera_settings() -> (AtmosphereSettings, AtmosphereEnvironmentMapLight) {
    (
        AtmosphereSettings::default(),
        AtmosphereEnvironmentMapLight::default(),
    )
}

/// Transform and light of one celestial body, disjoint from the other one.
type BodyLight<'w, 's, B, Other> = Query<
    'w,
    's,
    (
        &'static B,
        &'static mut Transform,
        &'static mut DirectionalLight,
    ),
    Without<Other>,
>;

/// Transform and material of one of the two drawn bodies. `Other` is the body it
/// is not — all four queries here write `Transform`, so every one of them has to be
/// provably disjoint from the rest or the schedule refuses to run.
type BodyMesh<'w, 's, B, Other, M> = Query<
    'w,
    's,
    (&'static mut Transform, &'static MeshMaterial3d<M>),
    (With<B>, Without<Sun>, Without<Moon>, Without<Other>),
>;

/// Puts sun, moon and stars where the date, the clock and the place say, and
/// writes [`Daylight`] for everything that only wants to know whether it is dark.
// A Bevy system takes its resources as parameters — the argument count says nothing here.
#[allow(clippy::too_many_arguments)]
fn update(
    sky: Res<Sky>,
    mut daylight: ResMut<Daylight>,
    atmosphere: Query<&Atmosphere>,
    mut media: ResMut<Assets<ScatteringMedium>>,
    mut settings: Query<&mut AtmosphereSettings>,
    // Visibility the medium was last built for — see below.
    mut built: Local<f32>,
    mut star_materials: ResMut<Assets<StarMaterial>>,
    mut moon_materials: ResMut<Assets<MoonMaterial>>,
    camera: Query<&GlobalTransform, (With<Camera3d>, With<AtmosphereSettings>)>,
    mut sun: BodyLight<Sun, Moon>,
    mut moon: BodyLight<Moon, Sun>,
    mut stars: BodyMesh<Stars, MoonDisk, StarMaterial>,
    mut disk: BodyMesh<MoonDisk, Stars, MoonMaterial>,
) {
    // The weather's sight, as a scattering term of the atmosphere itself: fog is
    // then made of the same integral as the sky, so it is blue at dusk and bright
    // around the sun without anything here saying so (plan 14.1). Rebuilding the
    // medium costs a LUT, so it happens on a real change and not every frame.
    let visibility = sky.weather.visibility.max(50.0);
    if (visibility / *built - 1.0).abs() > 0.03 {
        *built = visibility;
        if let Ok(atmosphere) = atmosphere.single()
            && let Some(mut medium) = media.get_mut(&atmosphere.medium)
        {
            *medium = ScatteringMedium::earth(LUT_RESOLUTION, LUT_RESOLUTION);
            if let Some(term) = haze(visibility, sky.weather.fog_depth) {
                medium.terms.push(term);
            }
        }
    }
    for mut settings in &mut settings {
        // The aerial-perspective LUT spreads its slices linearly out to this
        // distance and clamps beyond it. Left at the default 32 km over 32 slices,
        // a 300 m fog would sit inside the first slice and never be integrated;
        // six visibilities is where a target is down to 10^-10 of its contrast, so
        // nothing is left visible past the end of the table.
        settings.aerial_view_lut_max_distance = (visibility * 6.0).clamp(2_000.0, 32_000.0);
        // The look-up path builds the extinction of a piece of air as one
        // transmittance sample divided by another
        // (`sample_transmittance_lut_segment`). Under the weather's haze both of
        // them underflow along a grazing ray, and 0/0 comes back out as a stepped
        // rainbow ring around the observer — plainly a rainbow in a fog, a
        // hairline at the horizon in a clear sky. The ray march integrates the
        // same air in one pass and never divides, and it costs no more than the
        // tables here: the sky is a handful of samples per pixel either way.
        settings.rendering_method = AtmosphereMode::Raymarched;
    }

    let jd = sky.julian_date();
    let (lat, lon) = (sky.latitude, sky.longitude);
    let (sun_az, sun_el) = sun::sun_position(jd, lat, lon);
    let sun_dir = body_direction(sun_az, sun_el);
    let elevation = sun_el.to_degrees() as f32;

    // Daylight: up through civil twilight, 1 once the sun is properly up. The
    // headlights, the night windows and the cab lamps hang off this alone.
    daylight.0 = ((elevation + 6.0) / 12.0).clamp(0.0, 1.0);

    let cover = sky.weather.cover;
    if let Ok((marker, mut transform, mut light)) = sun.single_mut() {
        *transform = Transform::default().looking_to(-sun_dir, Vec3::Y);
        // The light keeps the raw above-atmosphere value whatever the elevation:
        // the atmosphere reddens and dims it on its own, and a light dimmed here
        // would take the twilight sky down with it.
        light.illuminance = SUN_ILLUMINANCE * (1.0 - 0.85 * cover);
        // A closed deck casts no shadow of its own. It comes back when it clears,
        // which is why this reads the setting and not the light's own state.
        light.shadow_maps_enabled = marker.shadows && cover < 0.5;
    }

    let (moon_az, moon_el, phase) = sun::moon_position(jd, lat, lon);
    let moon_dir = body_direction(moon_az, moon_el);
    // The moon's brightness is far from linear in its phase: half moon is a
    // twelfth of full, not a half, because the shadows of its own relief fill in
    // as the sun comes round behind us.
    let moonlight = if moon_el > 0.0 {
        (phase as f32).powi(3) * (1.0 - daylight.0)
    } else {
        0.0
    };
    if let Ok((_, mut transform, mut light)) = moon.single_mut() {
        *transform = Transform::default().looking_to(-moon_dir, Vec3::Y);
        light.illuminance = MOON_ILLUMINANCE * moonlight * (1.0 - cover);
    }

    // Stars and moon ride at the camera: they are a background, not scenery, and
    // the render origin moves under them.
    let eye = camera.iter().next().map_or(Vec3::ZERO, |t| t.translation());
    // Clouds swallow the stars long before they swallow the moon.
    let clear = 1.0 - cover;
    if let Ok((mut transform, material)) = stars.single_mut() {
        *transform = Transform::from_translation(eye)
            .with_rotation(sky_rotation(sun::local_sidereal(jd, lon), lat))
            .with_scale(Vec3::splat(SKY_RADIUS));
        if let Some(mut material) = star_materials.get_mut(&material.0) {
            material.sky.x = clear * clear;
        }
    }
    if let Ok((mut transform, material)) = disk.single_mut() {
        let facing = Quat::from_rotation_arc(Vec3::Z, moon_dir);
        *transform = Transform::from_translation(eye + moon_dir * SKY_RADIUS)
            .with_rotation(facing)
            // The mesh is a unit disk; a body of angular diameter θ seen from
            // `SKY_RADIUS` has the radius below.
            .with_scale(Vec3::splat(SKY_RADIUS * MOON_SIZE * 0.5));
        if let Some(mut material) = moon_materials.get_mut(&material.0) {
            // The shader shades a sphere in the disk's own frame, so the sun
            // goes in there too — that is what makes the phase come out right.
            material.params.sun = (facing.inverse() * sun_dir).extend(0.0);
            material.params.moon = Vec3::splat(MOON_LUMINANCE).extend(clear);
        }
    }
}

/// Resolution of the medium's falloff and phase look-up tables. The default 256
/// resolves a 1.2 km scale height in five samples, which is enough for the haze
/// terms below and cheap enough to rebuild while the weather moves.
const LUT_RESOLUTION: u32 = 256;

/// The scattering the weather adds to clear air, or `None` when the air is already
/// clearer than the model's own.
///
/// Koschmieder: a dark target is lost against the horizon after `3.912 / β` metres,
/// which turns a meteorological visibility straight into an extinction coefficient.
pub(crate) fn haze_extinction(visibility: f32) -> f32 {
    (3.912 / visibility.max(50.0) - CLEAR_AIR).max(0.0)
}

/// Scale height of the haze \[m\] — how far up the air the weather thickened
/// reaches. A fog lies low, a summer haze fills the boundary layer.
pub(crate) fn haze_height(fog_depth: f32) -> f32 {
    if fog_depth > 0.0 { 600.0 } else { 1_500.0 }
}

/// What the earth medium's own Mie term already extinguishes \[1/m\].
const CLEAR_AIR: f32 = 8.4e-6;

fn haze(visibility: f32, fog_depth: f32) -> Option<ScatteringTerm> {
    let beta = haze_extinction(visibility);
    if beta <= 0.0 {
        return None;
    }
    // The scale is that height in kilometres over the 60 km the falloff parameter
    // spans — the same convention the earth medium's own terms are written in.
    let height = haze_height(fog_depth) / 1_000.0;
    Some(ScatteringTerm {
        // Water droplets scatter almost everything they take out of the beam,
        // which is why fog is bright and not dark.
        absorption: Vec3::splat(beta * 0.02),
        scattering: Vec3::splat(beta * 0.98),
        falloff: Falloff::Exponential {
            scale: height / 60.0,
        },
        phase: PhaseFunction::Mie { asymmetry: 0.7 },
    })
}

/// Rotation from J2000 equatorial coordinates into render space (+X east, +Y up,
/// −Z north) for an observer at `latitude` whose meridian carries the right
/// ascension `sidereal`.
///
/// Two turns: the sidereal time swings the sphere about the celestial axis, the
/// latitude tips that axis down from overhead to the pole star's altitude.
fn sky_rotation(sidereal: f64, latitude: f64) -> Quat {
    let (sin_lat, cos_lat) = (latitude.sin() as f32, latitude.cos() as f32);
    let tilt = Mat3::from_cols(
        Vec3::new(0.0, cos_lat, sin_lat),
        Vec3::X,
        Vec3::new(0.0, sin_lat, -cos_lat),
    );
    Quat::from_mat3(&(tilt * Mat3::from_rotation_z(-sidereal as f32)))
}

/// Unit vector towards a body at `azimuth` (from north through east) and
/// `elevation`, in render space.
fn body_direction(azimuth: f64, elevation: f64) -> Vec3 {
    let (sin_az, cos_az) = azimuth.sin_cos();
    let (sin_el, cos_el) = elevation.sin_cos();
    Vec3::new(
        (cos_el * sin_az) as f32,
        sin_el as f32,
        (-cos_el * cos_az) as f32,
    )
    .normalize()
}

/// The star sprites: colour and brightness in the vertex colour, so the whole sky
/// is one draw call and one material.
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone, Default)]
pub struct StarMaterial {
    /// `x` = how much of the star light the weather lets through; rest reserved.
    #[uniform(0)]
    sky: Vec4,
}

impl Material for StarMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://world_render/stars.wgsl".into()
    }

    /// Added, not blended: a star is light on top of the sky, and two stars in one
    /// pixel are brighter than one.
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
    }

    fn enable_shadows() -> bool {
        false
    }

    fn enable_prepass() -> bool {
        false
    }
}

/// The moon's disk. The sun direction arrives in the disk's own frame, which is
/// all the shader needs to shade a sphere it never has to build.
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone, Default)]
pub struct MoonMaterial {
    #[uniform(0)]
    params: MoonParams,
}

/// What the moon shader is told.
#[derive(ShaderType, Debug, Clone, Default)]
pub struct MoonParams {
    /// Direction from the moon towards the sun, in the disk's local frame.
    sun: Vec4,
    /// `rgb` = luminance of the fully lit disk, `a` = what the weather lets through.
    moon: Vec4,
}

impl Material for MoonMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://world_render/moon.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
    }

    fn enable_shadows() -> bool {
        false
    }

    fn enable_prepass() -> bool {
        false
    }
}

/// The naked-eye stars, baked from the HYG database by `tools/gen_stars.py`:
/// right ascension, declination \[rad\], apparent magnitude and colour index B-V,
/// four `f32` each, brightest first.
const STAR_CATALOGUE: &[u8] = include_bytes!("stars.bin");

/// Bytes per catalogue record.
const RECORD: usize = 16;

/// One quad per star, in J2000 equatorial coordinates on the unit sphere.
///
/// The quads lie in the sphere's tangent plane, so they face its centre — and the
/// centre is where the camera sits. That saves billboarding them in a shader.
fn star_mesh() -> Mesh {
    let mut positions = Vec::new();
    let mut uvs = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();

    let mut sprite = |direction: Vec3, colour: Vec3, luminance: f32, size: f32| {
        // Any two vectors across the line of sight will do; the sprite is round.
        let right = direction.cross(Vec3::Z).normalize_or(Vec3::X);
        let up = direction.cross(right);
        let base = positions.len() as u32;
        for (u, v) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            positions.push((direction + (right * u + up * v) * size * 0.5).to_array());
            uvs.push([(u + 1.0) * 0.5, (v + 1.0) * 0.5]);
            colors.push((colour * luminance).extend(1.0).to_array());
        }
        // Wound so the face looks back at the sphere's centre, where the camera
        // is. The other way round every sprite is a back face and the whole sky
        // disappears without a word from anybody.
        indices.extend([base, base + 2, base + 1, base, base + 3, base + 2]);
    };

    for record in STAR_CATALOGUE.as_chunks::<RECORD>().0 {
        let value = |i: usize| f32::from_le_bytes(record[i * 4..i * 4 + 4].try_into().unwrap());
        let (ra, dec, magnitude, colour_index) = (value(0), value(1), value(2), value(3));
        let (sin_dec, cos_dec) = dec.sin_cos();
        let (sin_ra, cos_ra) = ra.sin_cos();
        sprite(
            Vec3::new(cos_dec * cos_ra, cos_dec * sin_ra, sin_dec),
            star_colour(colour_index),
            STAR_LUMINANCE * 10.0_f32.powf(-0.4 * magnitude),
            STAR_SIZE,
        );
    }
    milky_way(&mut sprite);

    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_indices(Indices::U32(indices))
}

/// How many faint stars the Milky Way is made of.
const MILKY_WAY_STARS: usize = 20_000;

/// Rows of the J2000 equatorial-to-galactic rotation. Transposed — the band is
/// built in galactic coordinates and has to come back the other way.
const GALACTIC: [Vec3; 3] = [
    Vec3::new(-0.054_875_6, 0.494_109_4, -0.867_666_2),
    Vec3::new(-0.873_437_1, -0.444_829_6, -0.198_076_4),
    Vec3::new(-0.483_835, 0.746_982_2, 0.455_983_8),
];

/// The Milky Way: the band the catalogue cannot hold, because it is made of stars
/// far fainter than an eye resolves. Scattering that many faint sprites along the
/// galactic plane is what it physically is, and it costs one mesh.
///
/// ponytail: procedural, not a photograph — no Sagittarius star cloud, no dust
/// lanes. Swap in a baked cube map if the band ever has to be recognisable.
fn milky_way(sprite: &mut impl FnMut(Vec3, Vec3, f32, f32)) {
    let mut seed = 0x2545_f491_4f6c_dd1d_u64;
    let mut random = move || {
        // xorshift64*, so the same band comes back every start.
        seed ^= seed >> 12;
        seed ^= seed << 25;
        seed ^= seed >> 27;
        ((seed.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 40) as f32) / 16_777_216.0
    };
    for _ in 0..MILKY_WAY_STARS {
        let longitude = random() * TAU;
        // Two-sided exponential about the plane: a thin disc, a few degrees thick.
        let sign = if random() < 0.5 { -1.0 } else { 1.0 };
        let latitude = sign * -random().max(1e-4).ln() * 0.06;
        // Towards the galactic centre the disc is deeper and therefore brighter.
        let towards_centre = 0.5 + 0.5 * longitude.cos();
        if random() > 0.25 + 0.75 * towards_centre {
            continue;
        }
        let (sin_b, cos_b) = latitude.sin_cos();
        let (sin_l, cos_l) = longitude.sin_cos();
        let galactic = Vec3::new(cos_b * cos_l, cos_b * sin_l, sin_b);
        let direction =
            GALACTIC[0] * galactic.x + GALACTIC[1] * galactic.y + GALACTIC[2] * galactic.z;
        let magnitude = 6.0 + random() * 2.0;
        sprite(
            direction.normalize(),
            star_colour(0.7 + random() * 0.6),
            STAR_LUMINANCE * 10.0_f32.powf(-0.4 * magnitude),
            STAR_SIZE * 1.4,
        );
    }
}

/// Linear RGB of a star from its colour index B-V, normalised to unit luminance so
/// the magnitude alone decides how bright it comes out. Blue-white O stars at one
/// end, orange-red M stars at the other; the sun sits at 0.65.
fn star_colour(colour_index: f32) -> Vec3 {
    const TABLE: [(f32, Vec3); 5] = [
        (-0.4, Vec3::new(0.61, 0.70, 1.00)),
        (0.0, Vec3::new(0.83, 0.87, 1.00)),
        (0.6, Vec3::new(1.00, 0.96, 0.92)),
        (1.4, Vec3::new(1.00, 0.80, 0.60)),
        (2.0, Vec3::new(1.00, 0.62, 0.40)),
    ];
    let mut colour = TABLE[TABLE.len() - 1].1;
    if colour_index <= TABLE[0].0 {
        colour = TABLE[0].1;
    }
    for pair in TABLE.windows(2) {
        let ((low, from), (high, to)) = (pair[0], pair[1]);
        if (low..high).contains(&colour_index) {
            colour = from.lerp(to, (colour_index - low) / (high - low));
        }
    }
    let luminance = colour.dot(Vec3::new(0.2126, 0.7152, 0.0722));
    colour / luminance.max(1e-3)
}

/// A unit quad in the local XY plane, facing +Z — what the moon is drawn on.
fn disk_mesh() -> Mesh {
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![
            [-1.0, -1.0, 0.0],
            [1.0, -1.0, 0.0],
            [1.0, 1.0, 0.0],
            [-1.0, 1.0, 0.0],
        ],
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
    )
    // Front face towards −Z, which is where the camera stands (see `star_mesh`).
    .with_inserted_indices(Indices::U32(vec![0, 2, 1, 0, 3, 2]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every record is four floats, and the catalogue is sorted brightest first.
    #[test]
    fn catalogue_parses() {
        assert_eq!(STAR_CATALOGUE.len() % RECORD, 0);
        let count = STAR_CATALOGUE.len() / RECORD;
        assert!(count > 8_000, "only {count} stars");
        let magnitude = |i: usize| {
            f32::from_le_bytes(
                STAR_CATALOGUE[i * RECORD + 8..i * RECORD + 12]
                    .try_into()
                    .unwrap(),
            )
        };
        // Sirius leads at -1.44, and nothing fainter than the naked-eye limit is in.
        assert!(magnitude(0) < -1.0, "brightest is {}", magnitude(0));
        assert!(magnitude(count - 1) <= 6.5);
    }

    /// A single vertex off to infinity makes the whole sky vanish: the bounding
    /// box goes with it and the mesh is culled from every view.
    #[test]
    fn star_mesh_is_finite() {
        let mesh = star_mesh();
        let Some(positions) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {
            panic!("no positions");
        };
        let positions = positions.as_float3().expect("three floats");
        assert!(positions.len() > 8_000 * 4, "{} vertices", positions.len());
        for p in positions {
            assert!(
                p[0].is_finite() && p[1].is_finite() && p[2].is_finite(),
                "{p:?}"
            );
            let length = Vec3::from_array(*p).length();
            assert!(
                (0.9..1.1).contains(&length),
                "off the unit sphere: {length}"
            );
        }
        let bevy::mesh::VertexAttributeValues::Float32x4(colours) =
            mesh.attribute(Mesh::ATTRIBUTE_COLOR).expect("colours")
        else {
            panic!("colours are not four floats");
        };
        for c in colours {
            assert!(c.iter().all(|v| v.is_finite()), "{c:?}");
        }
    }

    /// The band has to lie along the galactic plane, or it is not the Milky Way
    /// but a stripe of noise somewhere in the sky.
    #[test]
    fn the_milky_way_lies_in_the_galactic_plane() {
        let mut directions = Vec::new();
        milky_way(&mut |direction, _, _, _| directions.push(direction));
        assert!(directions.len() > 5_000, "{} sprites", directions.len());

        // Back into galactic coordinates: the rows of `GALACTIC` are the axes.
        let latitudes: Vec<f32> = directions
            .iter()
            .map(|d| {
                GALACTIC[2]
                    .dot(*d)
                    .clamp(-1.0, 1.0)
                    .asin()
                    .to_degrees()
                    .abs()
            })
            .collect();
        let within = |limit: f32| {
            latitudes.iter().filter(|b| **b < limit).count() as f32 / latitudes.len() as f32
        };
        assert!(within(10.0) > 0.85, "only {:.2} inside ±10°", within(10.0));
        assert!(within(3.0) > 0.4, "only {:.2} inside ±3°", within(3.0));

        // And brighter towards the centre of the galaxy than away from it.
        let centre = GALACTIC[0];
        let towards = directions.iter().filter(|d| centre.dot(**d) > 0.0).count();
        let away = directions.len() - towards;
        assert!(
            towards > away * 3 / 2,
            "{towards} towards the centre, {away} away"
        );
    }

    /// Every sprite has to face the centre of the sphere. A quad wound the other
    /// way is a back face, the rasteriser drops it, and nothing anywhere says so.
    #[test]
    fn sprites_face_the_camera() {
        for (mesh, name) in [(star_mesh(), "stars"), (disk_mesh(), "moon")] {
            let positions = mesh
                .attribute(Mesh::ATTRIBUTE_POSITION)
                .expect("positions")
                .as_float3()
                .expect("three floats");
            let Some(bevy::mesh::Indices::U32(indices)) = mesh.indices() else {
                panic!("{name}: no u32 indices");
            };
            let corner = |i: usize| Vec3::from_array(positions[indices[i] as usize]);
            // A sample is enough: they are all built by the same two lines.
            for i in (0..indices.len().min(3 * 64)).step_by(3) {
                let (a, b, c) = (corner(i), corner(i + 1), corner(i + 2));
                let normal = (b - a).cross(c - a);
                // The camera sits at the origin for the star sphere and, for the
                // moon's unit quad, on the −Z side of it.
                let towards_camera = if name == "stars" { -a } else { Vec3::NEG_Z };
                assert!(
                    normal.dot(towards_camera) > 0.0,
                    "{name}: triangle {} faces away",
                    i / 3
                );
            }
        }
    }

    /// The pole star has to stand at the observer's latitude, due north, whatever
    /// the time of day — that is the one thing everybody checks.
    #[test]
    fn celestial_pole_stands_at_the_latitude() {
        let latitude = 52.0_f64.to_radians();
        // Polaris is within a degree of the pole; use the pole itself.
        let pole = Vec3::Z;
        for hours in 0..24 {
            let rotation = sky_rotation(f64::from(hours) * TAU as f64 / 24.0, latitude);
            let direction = rotation * pole;
            let elevation = direction.y.asin().to_degrees();
            let azimuth = f32::atan2(direction.x, -direction.z).to_degrees();
            assert!((elevation - 52.0).abs() < 0.01, "elevation {elevation}");
            assert!(azimuth.abs() < 0.01, "azimuth {azimuth}");
        }
    }

    /// The sun stands where the star sphere puts a star of the same coordinates —
    /// the two paths (direct horizontal formulas, and the sphere's rotation) have
    /// to agree, or the constellations sit next to the sun instead of behind it.
    #[test]
    fn star_sphere_agrees_with_the_sun() {
        let sky = Sky {
            year: 2026,
            month: 3,
            day: 20,
            seconds: 15.0 * 3600.0,
            ..Sky::default()
        };
        let jd = sky.julian_date();
        let (azimuth, elevation) = sun::sun_position(jd, sky.latitude, sky.longitude);
        let direct = body_direction(azimuth, elevation);

        // The sun's own right ascension and declination, run through the sphere.
        let n = jd - 2_451_545.0;
        let lambda = (280.460 + 0.985_647_4 * n).to_radians()
            + 0.033_42 * (357.528 + 0.985_600_3 * n).to_radians().sin()
            + 0.000_349 * (2.0 * (357.528 + 0.985_600_3 * n).to_radians()).sin();
        let eps = (23.439 - 0.000_000_4 * n).to_radians();
        let ra = f64::atan2(lambda.sin() * eps.cos(), lambda.cos());
        let dec = (eps.sin() * lambda.sin()).asin();
        let (sin_dec, cos_dec) = (dec.sin() as f32, dec.cos() as f32);
        let (sin_ra, cos_ra) = (ra.sin() as f32, ra.cos() as f32);
        let equatorial = Vec3::new(cos_dec * cos_ra, cos_dec * sin_ra, sin_dec);
        let rotated =
            sky_rotation(sun::local_sidereal(jd, sky.longitude), sky.latitude) * equatorial;

        assert!(
            direct.distance(rotated) < 0.01,
            "sun at {direct:?}, star sphere puts it at {rotated:?}"
        );
    }

    /// A midsummer noon is daylight, midnight is not, and the sky knows the date.
    #[test]
    fn daylight_follows_the_clock() {
        let noon = Sky::default();
        let (_, elevation) = sun::sun_position(noon.julian_date(), noon.latitude, noon.longitude);
        assert!(elevation.to_degrees() > 55.0);

        let midnight = Sky {
            seconds: 0.0,
            ..noon
        };
        let (_, elevation) = sun::sun_position(
            midnight.julian_date(),
            midnight.latitude,
            midnight.longitude,
        );
        assert!(elevation.to_degrees() < -5.0);

        // Winter noon is much lower — the season is in the date, not in a setting.
        let winter = Sky {
            month: 12,
            day: 21,
            utc_offset: 1.0,
            ..noon
        };
        let (_, elevation) =
            sun::sun_position(winter.julian_date(), winter.latitude, winter.longitude);
        assert!((10.0..20.0).contains(&elevation.to_degrees()));
    }

    #[test]
    fn clock_roundtrip() {
        let mut sky = Sky::default();
        sky.set_clock(23, 45);
        assert_eq!((sky.hour(), sky.minute()), (23, 45));
    }
}
