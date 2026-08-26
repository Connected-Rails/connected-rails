// The dome that shows the cloud panorama (`clouds.wgsl`) in the world: the
// direction is the whole address. Drawn in the transparent phase, so it lands on
// the finished atmosphere and behind anything solid, and premultiplied, because
// what the march returns is an integral of light along a ray and not a colour
// with a coverage.
//
// Two things happen here rather than in the march, and both for the same reason —
// the panorama is only a sixteenth rewritten each frame:
//
//   * **A cubic filter, not a bilinear one.** A texel of sky is stretched over
//     several pixels, and bilinear magnification is what makes a soft cloud edge
//     read as a lattice of diamonds. Four taps of a B-spline cost a quarter of a
//     texture unit and take the lattice out.
//   * **The lightning.** A strike lasts two frames; put through the panorama it
//     would light a sixteenth of the sky at a time. It is a term on the coverage
//     the panorama already carries, so it belongs on this side of the buffer.

#import bevy_pbr::{forward_io::VertexOutput, mesh_view_bindings::view}

struct DomeParams {
    // x = extinction of the weather's haze [1/m], y = its scale height [m].
    haze: vec4<f32>,
    // rgb = what a strike puts into the deck this frame, from the inside.
    flash: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var panorama: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var panorama_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var<uniform> params: DomeParams;

const PI = 3.14159265;

/// A cubic B-spline through the panorama in four bilinear taps (Sigg &
/// Hadwiger): each tap is placed off-centre so the hardware's own interpolation
/// weighs two texels the way the spline wants them, which is what turns sixteen
/// samples into four.
///
/// B-spline rather than Catmull-Rom on purpose — it blurs slightly where the
/// sharper kernel would ring, and a cloud edge is the one thing that must not
/// ring.
fn bicubic(uv: vec2<f32>) -> vec4<f32> {
    let size = vec2<f32>(textureDimensions(panorama));
    let coord = uv * size - 0.5;
    let f = fract(coord);
    let base = floor(coord);

    let f2 = f * f;
    let f3 = f2 * f;
    let w0 = (1.0 - 3.0 * f + 3.0 * f2 - f3) / 6.0;
    let w1 = (4.0 - 6.0 * f2 + 3.0 * f3) / 6.0;
    let w2 = (1.0 + 3.0 * f + 3.0 * f2 - 3.0 * f3) / 6.0;
    let w3 = f3 / 6.0;

    let s0 = w0 + w1;
    let s1 = w2 + w3;
    let t0 = (base + 0.5 - 1.0 + w1 / s0) / size;
    let t1 = (base + 0.5 + 1.0 + w3 / s1) / size;

    let a = mix(
        textureSampleLevel(panorama, panorama_sampler, vec2(t0.x, t0.y), 0.0),
        textureSampleLevel(panorama, panorama_sampler, vec2(t1.x, t0.y), 0.0),
        s1.x,
    );
    let b = mix(
        textureSampleLevel(panorama, panorama_sampler, vec2(t0.x, t1.y), 0.0),
        textureSampleLevel(panorama, panorama_sampler, vec2(t1.x, t1.y), 0.0),
        s1.x,
    );
    return mix(a, b, s1.y);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let dir = normalize(in.world_position.xyz - view.world_position.xyz);
    // The dome is a whole sphere; its lower half would hang in front of the
    // ground, and there are no clouds under the horizon anyway.
    if dir.y < 0.0 {
        discard;
    }
    // Same mapping as the march: longitude round from north, latitude squared so
    // the horizon keeps the samples.
    let azimuth = atan2(dir.x, -dir.z);
    let elevation = asin(clamp(dir.y, -1.0, 1.0));
    let u = azimuth / (2.0 * PI) + 0.5;
    let v = sqrt(max(elevation, 0.0) / (PI * 0.5));
    let cloud = bicubic(vec2(u, 1.0 - v));
    // A strike lights the whole deck from the inside, which is what a
    // thunderstorm looks like from under it — the channel itself is behind the
    // cloud far more often than not. It scales with what the ray met, so an open
    // patch of sky stays dark.
    let lit = cloud.rgb + params.flash.rgb * cloud.a;
    // The air between: a cloud seen through fog is not a cloud. The path through
    // a layer of scale height h at elevation e is h / sin(e), and what survives
    // it is Beer's law — the same coefficient the atmosphere's own haze term is
    // built from (`sky::haze`).
    let path = params.haze.y / max(dir.y, 0.02);
    let through = exp(-params.haze.x * path);
    return vec4(lit * view.exposure * through, cloud.a * through);
}
