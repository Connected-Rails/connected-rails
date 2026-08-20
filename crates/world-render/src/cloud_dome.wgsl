// The dome that shows the cloud panorama (`clouds.wgsl`) in the world: one
// texture lookup per pixel, the direction being the whole address. Drawn in the
// transparent phase, so it lands on the finished atmosphere and behind anything
// solid, and premultiplied, because what the march returns is an integral of
// light along a ray and not a colour with a coverage.

#import bevy_pbr::{forward_io::VertexOutput, mesh_view_bindings::view}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var panorama: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var panorama_sampler: sampler;
// x = extinction of the weather's haze [1/m], y = its scale height [m].
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var<uniform> haze: vec4<f32>;

const PI = 3.14159265;

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
    let cloud = textureSample(panorama, panorama_sampler, vec2(u, 1.0 - v));
    // The air between: a cloud seen through fog is not a cloud. The path through
    // a layer of scale height h at elevation e is h / sin(e), and what survives
    // it is Beer's law — the same coefficient the atmosphere's own haze term is
    // built from (`sky::haze`).
    let path = haze.y / max(dir.y, 0.02);
    let through = exp(-haze.x * path);
    return vec4(cloud.rgb * view.exposure * through, cloud.a * through);
}
