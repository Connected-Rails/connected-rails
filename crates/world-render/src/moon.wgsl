// The moon's disk (plan ch. 14): half a degree of sphere, shaded from the real
// direction of the sun. The phase, and which way the lit edge points, therefore
// come out of the almanac rather than out of a curve — a waxing crescent in the
// evening leans away from where the sun went down, and so does this one.

#import bevy_pbr::{
    mesh_view_bindings::view,
    forward_io::VertexOutput,
}

struct MoonParams {
    // Direction from the moon towards the sun, in the disk's own frame.
    sun: vec4<f32>,
    // rgb = luminance of the fully lit disk, a = what the weather lets through.
    moon: vec4<f32>,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> params: MoonParams;

// Kept in step with `stars.wgsl` and `sky.rs`.
const ZENITH_OPTICAL_DEPTH = vec3(0.081, 0.150, 0.315);

// Sunlight bounced off a full earth onto the moon's night side — the ashen glow
// in the arms of a young crescent. About a hundredth of the lit side.
const EARTHSHINE = 0.012;
const EARTHSHINE_COLOUR = vec3(0.55, 0.68, 1.0);

fn extinction(direction: vec3<f32>) -> vec3<f32> {
    let sin_elevation = direction.y;
    let degrees_above = degrees(asin(clamp(sin_elevation, -1.0, 1.0)));
    let air_mass = 1.0 / (max(sin_elevation, 0.0)
        + 0.50572 * pow(max(degrees_above + 6.07995, 0.5), -1.6364));
    return exp(-ZENITH_OPTICAL_DEPTH * air_mass) * smoothstep(-0.02, 0.01, sin_elevation);
}

fn hash(cell: vec3<f32>) -> f32 {
    let scattered = fract(cell * 0.3183099 + vec3(0.71, 0.113, 0.419)) * 27.13;
    return fract(scattered.x * scattered.y * scattered.z * (scattered.x + scattered.y));
}

// Value noise on the sphere — dark maria against bright highlands.
fn noise(point: vec3<f32>) -> f32 {
    let cell = floor(point);
    let f = fract(point);
    let weight = f * f * (3.0 - 2.0 * f);
    var sum = 0.0;
    for (var i = 0u; i < 8u; i++) {
        let corner = vec3(f32(i & 1u), f32((i >> 1u) & 1u), f32((i >> 2u) & 1u));
        let blend = mix(1.0 - weight, weight, corner);
        sum += hash(cell + corner) * blend.x * blend.y * blend.z;
    }
    return sum;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let offset = in.uv * 2.0 - 1.0;
    let radius = length(offset);
    // The near half of the sphere, in the disk's own frame: +Z points away from us.
    let normal = vec3(offset.x, offset.y, -sqrt(max(1.0 - radius * radius, 0.0)));

    // Hapke, not Lambert: the moon is dust and backscatters, so a full moon is a
    // flat white disk rather than a shaded ball. The exponent is what flattens it.
    let lit = pow(max(dot(normal, normalize(params.sun.xyz)), 0.0), 0.45);
    // Maria at the large scale, a dusting of craters over them.
    let surface = mix(0.45, 1.0, smoothstep(0.32, 0.62, noise(normal * 2.3)))
        * (0.85 + 0.15 * noise(normal * 11.0));

    let luminance = params.moon.rgb * surface
        * (lit + EARTHSHINE * EARTHSHINE_COLOUR * (1.0 - lit));

    let direction = normalize(in.world_position.xyz - view.world_position.xyz);
    // One pixel of limb, so the edge of the disk is a curve and not a staircase.
    let edge = 1.0 - smoothstep(1.0 - fwidth(radius), 1.0, radius);
    let out = luminance * edge * extinction(direction) * params.moon.a * view.exposure;
    return vec4(out, 0.0);
}
