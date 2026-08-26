// The cloud march (plan 14.1). One fragment per panorama texel, one texel per
// direction: the shader turns a direction into a ray and returns what came back
// along it. Two ways of answering that, one switch (`frame.y`):
//
//   * **Volumetric** — Nubis' model: a low-frequency Perlin-Worley shape eroded
//     by a Worley detail, a height profile that gives a cumulus its flat base
//     and cauliflower top, Beer-Powder attenuation for the dark edges, and a
//     two-lobe Henyey-Greenstein phase for the silver lining when the sun is
//     behind them.
//   * **Layered** — the same shape field and the same lighting, but sampled
//     where the ray crosses the middle of the deck, with the self-shadow walked
//     across that height field instead of marched through a volume. A dozen
//     texture fetches against several hundred. What it loses is the billows and
//     the parallax through a cloud; the silver lining, the dark base and the
//     colour of the hour all survive.
//
// Either way only **one texel in sixteen** is written a frame — `frame.x` names
// the 4×4 Bayer slot, and the panorama is never cleared, so the other fifteen
// keep what earlier passes left. That is what pays for the resolution: at
// 2048 × 1024 a texel is 0.18° of sky, where 768 × 384 was 0.47° and showed
// every one of them. Sixteen frames is a quarter of a second, in which a cloud
// at five kilometres drifts a fifth of a texel.

#import bevy_sprite::mesh2d_vertex_output::VertexOutput

struct CloudParams {
    // xyz = direction towards the sun, w = 1 by day, 0 at night.
    sun: vec4<f32>,
    // rgb = the light reaching the layer, a = drift time [s].
    light: vec4<f32>,
    // rgb = the sky's own light on a cloud, a = cover 0…1.
    ambient: vec4<f32>,
    // x = base [m], y = thickness [m], zw = wind [m/s].
    layer: vec4<f32>,
    // x = the Bayer slot this frame writes, or −1 for every texel at once;
    // y = 1 volumetric, 0 layered.
    frame: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> params: CloudParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var shape_texture: texture_3d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var shape_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var detail_texture: texture_3d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var detail_sampler: sampler;

const PI = 3.14159265;
const EARTH_RADIUS = 6360000.0;

/// Beyond this the layer is edge-on and adds nothing but cost [m]. It is also
/// what `VIEW_STEPS × MAX_STEP` reaches, so nothing is marched that is then
/// thrown away.
const MAX_DISTANCE = 34000.0;

/// Longest step along the view ray [m]. Without a cap, a ray that runs down the
/// layer towards the horizon would divide the whole distance by the step count
/// and stride straight through every cloud on the way.
const MAX_STEP = 300.0;

/// Steps along the view ray, and towards the sun from each of them. Twice what a
/// full-resolution panorama could afford — the amortisation bought it, and it is
/// where the banding on a low sun went.
const VIEW_STEPS = 96;
const LIGHT_STEPS = 6;

/// Steps of the layered path's walk across the height field towards the sun.
const LAYER_STEPS = 4;

/// What that walk's answer is scaled by before it is an optical depth.
///
/// The walk crosses the deck at one height and so returns the depth at the
/// *base* of the sheet — but the sheet is what is being drawn, all of it, and
/// its top stands in no shadow at all. Taken at face value the whole sky comes
/// out in full shadow and the layered path draws clouds darker than the sky
/// behind them. A quarter is roughly what the march's own profile averages to
/// over a cloud, and it is what makes the two paths look like the same weather.
const LAYER_SHADOW = 0.25;

/// Slices through the deck the layered path averages, and where its shadow walk
/// reads it.
///
/// One slice is the obvious way to draw a sheet and the wrong one: it misses
/// every cloud whose body happens to sit above or below that one height, and the
/// sky comes out with a fraction of the cover the march finds over the same
/// weather. Three is enough to agree with it, and still an order of magnitude
/// fewer fetches. The walk keeps a single height — a shadow does not need the
/// cover to be right, only roughly where the cloud is.
const LAYER_SLICES = 3;
const LAYER_HEIGHT = 0.3;

/// Half-angle of the cone the light ray is jittered into [rad, roughly]. A cloud
/// is not lit along one line but out of a solid angle, and neighbouring texels
/// taking different lines through that cone is what makes the self-shadow soft
/// (Schneider's cone sampling, for the price of a normalize).
const CONE_SPREAD = 0.09;

/// Extinction per metre of full-density cloud. A cumulus is opaque after a
/// hundred metres or so, which this is the coefficient for.
const EXTINCTION = 0.045;

/// What the shape field's own output is multiplied by before it is a density.
///
/// The Perlin-Worley mix and the two remaps that threshold it leave a field that
/// only reaches about 0.2 where a cloud is thickest, and tails off over
/// kilometres rather than metres. A view ray still turns opaque — it crosses the
/// deck for long enough — so the clouds *look* solid, but the light ray only
/// crosses a few hundred metres, never gathers any optical depth, and every
/// cloud comes out evenly lit on all sides. Bringing the core up to something
/// near one is what gives a cumulus a dark base and a bright rim, and it is the
/// difference between a raymarch and an expensive way of drawing a blob.
const DENSITY_GAIN = 3.0;

/// Period of the shape noise **on the ground plane** [m]. A cumulus is a
/// kilometre across, and the volume holds a few of them.
///
/// The vertical axis cannot use this scale: a deck is a kilometre thick against
/// eighteen across, so the same scale in all three axes reads one horizontal
/// slice and extrudes it through the whole cloud — every cumulus comes out a wad
/// of cotton wool with a taper on it. The third texture coordinate is the
/// *fraction of the way up the deck* times a turn count instead.
const SHAPE_SCALE = 1.0 / 18000.0;
const DETAIL_SCALE = 1.0 / 1400.0;

/// How far the shape volume is walked along its own vertical axis per second.
///
/// A cloud that only drifts is a photograph on a conveyor belt: the same billow
/// crosses the whole sky and nothing is ever born or lost. The deck occupies only
/// `SHAPE_TURNS` of the volume's height, so the rest of that axis is a supply of
/// shapes nobody is using — walking into it over time turns one cloud into the
/// next. It reads as growing and dissolving rather than sliding because the height
/// profile stays where it is, so the base of the deck holds still while the body
/// above it changes.
///
/// One turn of the volume is four features at frequency 4; at this rate a cloud is
/// visibly a different cloud after about ten minutes, which is the short end of
/// what a cumulus lives.
///
/// It is also the rate at which the *cover* breathes, and that is what caps it: no
/// two heights of the volume hold quite the same amount of cloud, so walking the
/// axis makes the sky thicken and open on its own. That is welcome — weather does
/// that — but at twice this rate a `cover` of 0.45 visibly emptied the sky inside
/// ten minutes, which is a setting quietly overruling itself.
///
/// Far below what one turn of the amortisation can show, either way: a frame's
/// worth of it is a hundredth of a texel, so the sixteen-frame panorama never
/// catches it moving.
const EVOLVE = 1.0 / 2400.0;

/// Turns of each volume between the base of the deck and its top, and what
/// decides how tall a billow is against how wide.
///
/// The volumes are built at frequency 4, so one turn is four features: 0.35 of a
/// turn over 1.2 km of deck makes a billow about 850 m tall against the 4.5 km it
/// is wide, which is a cumulus rather than a pancake. Run it up to a whole turn
/// and the clouds flatten into sheets; that is the mistake this constant exists
/// to name.
const SHAPE_TURNS = 0.35;
const DETAIL_TURNS = 1.2;

/// Octaves of the multiple-scattering approximation, and how each one falls off
/// against the one before: `SCATTER_FADE` is how much light is left to it,
/// `SCATTER_REACH` how much of the extinction still applies to it, and
/// `SCATTER_SPREAD` how much of the phase function's direction it remembers.
///
/// A cloud is bright because light bounces inside it many times — a real cumulus
/// has an albedo close to 0.9 at an optical depth of ten. Single scattering
/// alone renders that as a dark smudge whatever the sun does, and octaves that
/// fade too fast render it as a smudge lit only by the sky, which is the same
/// mistake one step further on: the whole cloud comes out the flat blue-white of
/// the ambient term and the sun might as well not be there.
const SCATTER_OCTAVES = 4;
const SCATTER_FADE = 0.7;
const SCATTER_REACH = 0.4;
const SCATTER_SPREAD = 0.6;

/// What the summed octaves are scaled by to become a radiance.
///
/// The phase lobes are normalised over the sphere and a cloud is lit by a disc,
/// so the integral has to come back out of them — but not as the full 4π that
/// looks like the obvious inverse. A white Lambertian surface under an
/// illuminance E has a radiance of E/π, and no cloud can beat that; 4π puts a
/// sunlit face five times over it, everything lands past the top of the
/// tonemapping curve, and the difference between a lit face and a shaded one is
/// flattened into the same white. This is the factor that brings a fully lit
/// cloud to about E/π instead.
const SCATTER_GAIN = 2.5;

/// Period of the coverage field — the weather map, in kilometres rather than
/// metres, so a front is bigger than the clouds it carries.
const COVER_SCALE = 1.0 / 42000.0;

fn remap(v: f32, from_min: f32, from_max: f32, to_min: f32, to_max: f32) -> f32 {
    return to_min + (v - from_min) / max(from_max - from_min, 1e-5) * (to_max - to_min);
}

/// The classic 4×4 ordered dither, built out of its own bits rather than looked
/// up in a table: interleave `x ^ y` with `y`, then reverse. What it is worth
/// here is that consecutive slots are as far apart on the panorama as they can
/// be, so a texel that is fifteen frames old always has a fresh neighbour.
fn bayer4(x: u32, y: u32) -> u32 {
    let a = x ^ y;
    return ((a & 1u) << 3u) | ((y & 1u) << 2u) | (a & 2u) | ((y >> 1u) & 1u);
}

/// Three uncorrelated values in 0…1 from a texel position — the wobble on the
/// light ray, and nothing that has to be reproducible between frames.
fn hash3(v: vec2<f32>) -> vec3<f32> {
    return fract(
        sin(vec3(
            dot(v, vec2(12.9898, 78.233)),
            dot(v, vec2(39.3468, 11.135)),
            dot(v, vec2(73.1560, 52.235)),
        )) * 43758.5453,
    );
}

/// Henyey-Greenstein: how much light carries on in the direction it was going.
fn henyey_greenstein(cos_angle: f32, g: f32) -> f32 {
    let g2 = g * g;
    return (1.0 - g2) / (4.0 * PI * pow(1.0 + g2 - 2.0 * g * cos_angle, 1.5));
}

/// Distance to the far intersection of a ray from `origin` with a sphere of
/// `radius` about the earth's centre; negative when it misses.
fn sphere_exit(origin: vec3<f32>, dir: vec3<f32>, radius: f32) -> f32 {
    let b = dot(origin, dir);
    let c = dot(origin, origin) - radius * radius;
    let disc = b * b - c;
    if disc < 0.0 {
        return -1.0;
    }
    return -b + sqrt(disc);
}

/// A world position carried by the wind — where the noise is read for the point
/// the ray is actually at. Both paths sample through this, so a cloud drifts the
/// same way whichever one drew it.
fn drifted(p: vec3<f32>) -> vec3<f32> {
    let drift = params.layer.zw * params.light.a;
    return vec3(p.x + drift.x, p.y, p.z + drift.y);
}

/// How much cloud the weather has put over this piece of ground, 0…1.
fn coverage(xz: vec2<f32>) -> f32 {
    let drift = params.layer.zw * params.light.a;
    // The map travels with the clouds it carries, not faster: it used to be given
    // three times the drift to keep the sky changing, and that job belongs to
    // `EVOLVE` now — a field of cloud crosses the sky as one piece.
    let map = textureSampleLevel(
        shape_texture,
        shape_sampler,
        vec3((xz + drift) * COVER_SCALE, 0.5),
        0.0,
    );
    // The uniform's cover is the mean; the map is what makes some of the sky
    // open while the rest is closed.
    let field = map.g * 0.6 + map.b * 0.4;
    return clamp(params.ambient.a * 0.9 + (field - 0.5) * 0.5, 0.0, 1.0);
}

/// A cumulus has a flat base and a billowing top; a closed deck is a slab. The
/// cover decides which of the two this is.
fn height_profile(height: f32, cover: f32) -> f32 {
    let bottom = smoothstep(0.0, 0.12, height);
    let top = 1.0 - smoothstep(mix(0.35, 0.8, cover), 1.0, height);
    return bottom * top;
}

/// The cloud's density at an already drifted sample position, 0…1.
fn cloud_density(q: vec3<f32>, height: f32, cover: f32, detailed: bool) -> f32 {
    let shape = textureSampleLevel(
        shape_texture,
        shape_sampler,
        vec3(
            q.x * SHAPE_SCALE,
            height * SHAPE_TURNS + params.light.a * EVOLVE,
            q.z * SHAPE_SCALE,
        ),
        0.0,
    );
    // The three Worley octaves as one fbm, eroding the Perlin-Worley base.
    let fbm = shape.g * 0.625 + shape.b * 0.25 + shape.a * 0.125;
    var density = remap(shape.r, fbm - 1.0, 1.0, 0.0, 1.0);
    // The coverage is a threshold on that shape, and a soft one: a hard edge here
    // is a sky either empty or overcast, with nothing in between.
    density = remap(density, (1.0 - cover) * 0.75, 1.0, 0.0, 1.0) * height_profile(height, cover);
    if density <= 0.0 {
        return 0.0;
    }
    density *= DENSITY_GAIN;
    if detailed {
        // Erosion, strongest at the base where the wisps hang.
        let detail = textureSampleLevel(
            detail_texture,
            detail_sampler,
            vec3(q.x * DETAIL_SCALE, height * DETAIL_TURNS, q.z * DETAIL_SCALE),
            0.0,
        );
        let curl = detail.g * 0.625 + detail.b * 0.25 + detail.a * 0.125;
        density = remap(density, curl * mix(0.45, 0.1, height), 1.0, 0.0, 1.0);
    }
    return clamp(density, 0.0, 1.0);
}

/// The optical depth of the cloud between `p` and the sun.
fn light_depth(p: vec3<f32>, to_sun: vec3<f32>, base: f32, thickness: f32, cover: f32) -> f32 {
    var sum = 0.0;
    // The first step is short on purpose. A sample right at the sunlit face of a
    // cloud has no cloud between it and the sun, and a step of the full stride
    // would charge it a fifth of a kilometre of one — which is what turns a
    // terminator into a wash. Growing by 1.7 from there still reaches about one
    // and a half thicknesses, which is as far as a light ray matters.
    var step = thickness / f32(LIGHT_STEPS) * 0.3;
    var pos = p;
    for (var i = 0; i < LIGHT_STEPS; i++) {
        pos += to_sun * step;
        let height = (length(pos) - EARTH_RADIUS - base) / thickness;
        if height < 0.0 || height > 1.0 {
            break;
        }
        // No detail on the light ray: it costs a sample and shows as noise, not
        // as shape.
        sum += cloud_density(drifted(pos), height, cover, false) * step;
        // Widening steps reach further for the same cost.
        step *= 1.7;
    }
    return sum;
}

/// The light scattered towards the viewer at one sample, summed over octaves of
/// ever weaker, ever more diffuse, ever less attenuated scattering (Wrenninge's
/// approximation of the multiple scattering a real cloud lives on).
fn scattered_light(cos_sun: f32, depth: f32, density: f32) -> vec3<f32> {
    var luminance = vec3(0.0);
    var attenuation = 1.0;
    var contribution = 1.0;
    var eccentricity = 1.0;
    for (var octave = 0; octave < SCATTER_OCTAVES; octave++) {
        let phase = mix(
            henyey_greenstein(cos_sun, 0.8 * eccentricity),
            henyey_greenstein(cos_sun, -0.25 * eccentricity),
            0.35,
        );
        // Powder only on the first octave: it is a single-scattering effect, and
        // repeating it turns every cloud edge into a bruise.
        let powder = select(1.0, 1.0 - exp(-2.0 * density), octave == 0);
        luminance += attenuation * phase * powder * exp(-EXTINCTION * contribution * depth);
        attenuation *= SCATTER_FADE;
        contribution *= SCATTER_REACH;
        eccentricity *= SCATTER_SPREAD;
    }
    return vec3(luminance * SCATTER_GAIN);
}

/// The volumetric answer: walk the shell of cloud and integrate what scatters
/// back along the way.
fn march(dir: vec3<f32>, to_sun: vec3<f32>, jitter: f32, base: f32, thickness: f32) -> vec4<f32> {
    let origin = vec3(0.0, EARTH_RADIUS, 0.0);
    let start = sphere_exit(origin, dir, EARTH_RADIUS + base);
    let end = sphere_exit(origin, dir, EARTH_RADIUS + base + thickness);
    if start < 0.0 || end <= start {
        return vec4(0.0);
    }
    let far = min(end, start + MAX_DISTANCE);
    let span = far - start;

    let cos_sun = dot(dir, params.sun.xyz);
    let step = min(span / f32(VIEW_STEPS), MAX_STEP);
    var transmittance = 1.0;
    var scattered = vec3(0.0);
    var travelled = start + step * jitter;

    for (var i = 0; i < VIEW_STEPS; i++) {
        if transmittance < 0.01 {
            break;
        }
        let p = origin + dir * travelled;
        let height = (length(p) - EARTH_RADIUS - base) / thickness;
        travelled += step;
        if height < 0.0 || height > 1.0 {
            continue;
        }
        let cover = coverage(p.xz);
        let density = cloud_density(drifted(p), height, cover, true);
        if density <= 0.001 {
            continue;
        }

        let extinction = EXTINCTION * density;
        let depth = light_depth(p, to_sun, base, thickness, cover);
        let light = params.light.rgb * scattered_light(cos_sun, depth, density) * params.sun.w;
        // The rest of the sky lights the top of a cloud more than its base.
        let ambient = params.ambient.rgb * mix(0.25, 1.0, height);

        // Energy-conserving integration over the step (Hillaire): the analytic
        // integral, not a Riemann sum, so the steps look like many more.
        let scatter = (light + ambient) * extinction;
        let attenuation = exp(-extinction * step);
        scattered += transmittance * (scatter - scatter * attenuation) / extinction;
        transmittance *= attenuation;
    }
    return vec4(scattered, 1.0 - transmittance);
}

/// The cheap answer: the deck as one lit sheet rather than a volume.
///
/// The density read where the ray crosses the middle of the layer stands for the
/// whole thickness, which turns the second march into a short walk across that
/// height field towards the sun — the classic 2.5D cloud layer, but sharing the
/// march's shape field and its scattering, so the two look like the same sky.
fn layered(dir: vec3<f32>, to_sun: vec3<f32>, base: f32, thickness: f32) -> vec4<f32> {
    let origin = vec3(0.0, EARTH_RADIUS, 0.0);
    let hit = sphere_exit(origin, dir, EARTH_RADIUS + base + thickness * 0.5);
    if hit < 0.0 || hit > MAX_DISTANCE {
        return vec4(0.0);
    }
    let p = origin + dir * hit;
    let cover = coverage(p.xz);
    var density = 0.0;
    for (var i = 0; i < LAYER_SLICES; i++) {
        density += cloud_density(drifted(p), (f32(i) + 0.5) / f32(LAYER_SLICES), cover, true);
    }
    density /= f32(LAYER_SLICES);
    if density <= 0.001 {
        return vec4(0.0);
    }

    // How far across the plane one step of height goes: the sun's own slope,
    // capped, or a sun on the horizon would send the walk off to infinity.
    let stride = thickness / f32(LAYER_STEPS);
    let across = to_sun.xz / max(to_sun.y, 0.2) * stride;
    var depth = 0.0;
    for (var i = 1; i <= LAYER_STEPS; i++) {
        let q = p + vec3(across.x, 0.0, across.y) * f32(i);
        // No detail on the shadow walk, for the same reason the march leaves it
        // off its light ray.
        depth += cloud_density(drifted(q), LAYER_HEIGHT, coverage(q.xz), false) * stride;
    }
    depth *= LAYER_SHADOW;

    let cos_sun = dot(dir, params.sun.xyz);
    let light = params.light.rgb * scattered_light(cos_sun, depth, density) * params.sun.w;
    // One sheet has no top and no base to tell apart; two thirds of the sky's
    // light is what the march's profile averages to over a cloud.
    let ambient = params.ambient.rgb * 0.6;
    // The same integral the march does per step, done once over the whole
    // thickness — which is exactly what treating the deck as one sample means.
    let alpha = 1.0 - exp(-EXTINCTION * density * thickness);
    return vec4((light + ambient) * alpha, alpha);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = vec2<u32>(in.position.xy);
    let slot = bayer4(texel.x & 3u, texel.y & 3u);
    // Amortisation: this texel is written on one frame in sixteen and keeps what
    // it had on the other fifteen. `discard` before anything is marched is the
    // whole saving — the pass has to be told not to clear, or there would be
    // nothing left to keep (`clouds.rs`).
    if params.frame.x >= 0.0 && slot != u32(params.frame.x) {
        discard;
        return vec4(0.0);
    }

    // Panorama mapping: longitude round, latitude squared so the horizon — where
    // the layer is edge-on and most of the sky's detail sits — gets the samples.
    let azimuth = (in.uv.x - 0.5) * 2.0 * PI;
    let v = 1.0 - in.uv.y;
    let elevation = v * v * (PI * 0.5);
    let dir = vec3(
        cos(elevation) * sin(azimuth),
        sin(elevation),
        -cos(elevation) * cos(azimuth),
    );

    let base = params.layer.x;
    let thickness = params.layer.y;
    // The dither slot doubles as the offset into the first step: it is already
    // the most dispersed pattern there is, so neighbouring texels start as far
    // apart as they can and the step pattern reads as texture, not as rings.
    let jitter = (f32(slot) + 0.5) / 16.0;
    let wobble = hash3(in.position.xy) - 0.5;
    let to_sun = normalize(params.sun.xyz + wobble * CONE_SPREAD);

    var cloud: vec4<f32>;
    if params.frame.y > 0.5 {
        cloud = march(dir, to_sun, jitter, base, thickness);
    } else {
        cloud = layered(dir, to_sun, base, thickness);
    }

    // Near the horizon the layer runs out into haze rather than ending in a line.
    let horizon = smoothstep(0.0, 0.06, dir.y);
    return cloud * horizon;
}
