// The cloud march (plan 14.1). One fragment per panorama texel, one texel per
// direction: the shader turns a direction into a ray, walks it through a shell of
// cloud around the earth, and returns what came back along it.
//
// The model is Nubis': a low-frequency Perlin-Worley shape eroded by a Worley
// detail, a height profile that gives a cumulus its flat base and cauliflower
// top, Beer-Powder attenuation for the dark edges, and a two-lobe
// Henyey-Greenstein phase for the silver lining when the sun is behind them.

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
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> params: CloudParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var shape_texture: texture_3d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var shape_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var detail_texture: texture_3d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var detail_sampler: sampler;

const PI = 3.14159265;
const EARTH_RADIUS = 6360000.0;

/// Beyond this the layer is edge-on and adds nothing but cost [m].
const MAX_DISTANCE = 60000.0;

/// Longest step along the view ray [m]. Without a cap, a ray that runs down the
/// layer towards the horizon would divide 60 km by the step count and stride
/// straight through every cloud on the way.
const MAX_STEP = 400.0;

/// Steps along the view ray, and towards the sun from each of them.
const VIEW_STEPS = 48;
const LIGHT_STEPS = 5;

/// Extinction per metre of full-density cloud. A cumulus is opaque after a
/// hundred metres or so, which this is the coefficient for.
const EXTINCTION = 0.045;

/// Period of the shape noise on the ground plane [m]. A cumulus is a kilometre
/// across, and the volume holds a few of them.
const SHAPE_SCALE = 1.0 / 20000.0;
const DETAIL_SCALE = 1.0 / 2000.0;

/// Octaves of the multiple-scattering approximation. A cloud is bright because
/// light bounces inside it many times; single scattering alone renders it as a
/// dark smudge, whatever the sun does.
const SCATTER_OCTAVES = 3;

/// Period of the coverage field — the weather map, in kilometres rather than
/// metres, so a front is bigger than the clouds it carries.
const COVER_SCALE = 1.0 / 42000.0;

fn remap(v: f32, from_min: f32, from_max: f32, to_min: f32, to_max: f32) -> f32 {
    return to_min + (v - from_min) / max(from_max - from_min, 1e-5) * (to_max - to_min);
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

/// How much cloud the weather has put over this piece of ground, 0…1.
fn coverage(xz: vec2<f32>) -> f32 {
    let drift = params.layer.zw * params.light.a;
    let map = textureSampleLevel(
        shape_texture,
        shape_sampler,
        vec3((xz + drift * 3.0) * COVER_SCALE, 0.5),
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

fn density_at(p: vec3<f32>, height: f32, cover: f32, detailed: bool) -> f32 {
    let drift = params.layer.zw * params.light.a;
    let q = vec3(p.x + drift.x, p.y, p.z + drift.y);
    let shape = textureSampleLevel(shape_texture, shape_sampler, q * SHAPE_SCALE, 0.0);
    // The three Worley octaves as one fbm, eroding the Perlin-Worley base.
    let fbm = shape.g * 0.625 + shape.b * 0.25 + shape.a * 0.125;
    var density = remap(shape.r, fbm - 1.0, 1.0, 0.0, 1.0);
    // The coverage is a threshold on that shape, and a soft one: a hard edge here
    // is a sky either empty or overcast, with nothing in between.
    density = remap(density, (1.0 - cover) * 0.75, 1.0, 0.0, 1.0) * height_profile(height, cover);
    if density <= 0.0 {
        return 0.0;
    }
    if detailed {
        // Erosion, strongest at the base where the wisps hang.
        let detail = textureSampleLevel(detail_texture, detail_sampler, q * DETAIL_SCALE, 0.0);
        let curl = detail.g * 0.625 + detail.b * 0.25 + detail.a * 0.125;
        density = remap(density, curl * mix(0.45, 0.1, height), 1.0, 0.0, 1.0);
    }
    return clamp(density, 0.0, 1.0);
}

/// The optical depth of the cloud between `p` and the sun.
fn light_depth(p: vec3<f32>, base: f32, thickness: f32, cover: f32) -> f32 {
    var sum = 0.0;
    var step = thickness / f32(LIGHT_STEPS);
    var pos = p;
    for (var i = 0; i < LIGHT_STEPS; i++) {
        pos += params.sun.xyz * step;
        let height = (length(pos) - EARTH_RADIUS - base) / thickness;
        if height < 0.0 || height > 1.0 {
            break;
        }
        // No detail on the light ray: it costs a sample and shows as noise, not
        // as shape.
        sum += density_at(pos, height, cover, false) * step;
        // Widening steps reach further for the same cost.
        step *= 1.5;
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
        attenuation *= 0.6;
        contribution *= 0.5;
        eccentricity *= 0.5;
    }
    // The lobes are normalised over the sphere; a cloud is lit by a disc, so the
    // integral has to come back out of the phase function.
    return vec3(luminance * 4.0 * PI);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
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
    let origin = vec3(0.0, EARTH_RADIUS, 0.0);
    let start = sphere_exit(origin, dir, EARTH_RADIUS + base);
    let end = sphere_exit(origin, dir, EARTH_RADIUS + base + thickness);
    if start < 0.0 || end <= start {
        return vec4(0.0);
    }
    let far = min(end, start + MAX_DISTANCE);
    let span = far - start;

    let cos_sun = dot(dir, params.sun.xyz);
    // A per-texel offset breaks the step pattern into noise the eye reads as
    // texture rather than as rings.
    let jitter = fract(sin(dot(in.uv, vec2(12.9898, 78.233))) * 43758.5453);
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
        let density = density_at(p, height, cover, true);
        if density <= 0.001 {
            continue;
        }

        let extinction = EXTINCTION * density;
        let depth = light_depth(p, base, thickness, cover);
        let light = params.light.rgb * scattered_light(cos_sun, depth, density) * params.sun.w;
        // The rest of the sky lights the top of a cloud more than its base.
        let ambient = params.ambient.rgb * mix(0.25, 1.0, height);

        // Energy-conserving integration over the step (Hillaire): the analytic
        // integral, not a Riemann sum, so 48 steps look like many more.
        let scatter = (light + ambient) * extinction;
        let attenuation = exp(-extinction * step);
        scattered += transmittance * (scatter - scatter * attenuation) / extinction;
        transmittance *= attenuation;
    }

    // Near the horizon the layer runs out into haze rather than ending in a line.
    let horizon = smoothstep(0.0, 0.06, dir.y);
    let alpha = (1.0 - transmittance) * horizon;
    return vec4(scattered * horizon, alpha);
}
