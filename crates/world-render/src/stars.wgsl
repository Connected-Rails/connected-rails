// One point sprite per star (plan ch. 14). The mesh carries the catalogue's own
// brightness and colour in the vertex colour; all this does is round the quad off
// into a point of light and take off what the air between swallows.

#import bevy_pbr::{
    mesh_view_bindings::view,
    forward_io::VertexOutput,
}

// x = the share of the star light the weather lets through; y/z/w reserved.
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> sky: vec4<f32>;

// Zenith optical depth of clear air at 680 / 550 / 440 nm — Rayleigh plus a
// clear-air aerosol term. Kept in step with `sky.rs`.
const ZENITH_OPTICAL_DEPTH = vec3(0.081, 0.150, 0.315);

// What is left of a body's light after the atmosphere, from its elevation alone.
// Kasten & Young's air mass: one at the zenith, thirty-eight at the horizon —
// which is why a setting star goes orange and then out.
fn extinction(direction: vec3<f32>) -> vec3<f32> {
    let sin_elevation = direction.y;
    let degrees_above = degrees(asin(clamp(sin_elevation, -1.0, 1.0)));
    let air_mass = 1.0 / (max(sin_elevation, 0.0)
        + 0.50572 * pow(max(degrees_above + 6.07995, 0.5), -1.6364));
    // Below the horizon the earth itself is in the way.
    return exp(-ZENITH_OPTICAL_DEPTH * air_mass) * smoothstep(-0.02, 0.01, sin_elevation);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // A round, soft point: the blur a lens puts on a source it cannot resolve.
    let offset = in.uv * 2.0 - 1.0;
    let profile = 1.0 - smoothstep(0.0, 1.0, length(offset));

    let direction = normalize(in.world_position.xyz - view.world_position.xyz);
#ifdef VERTEX_COLORS
    let star = in.color.rgb;
#else
    let star = vec3(1.0);
#endif
    let luminance = star * profile * extinction(direction) * sky.x * view.exposure;
    // Premultiplied with zero alpha: the star is added to the sky behind it, and
    // two stars in one pixel come out brighter than one.
    return vec4(luminance, 0.0);
}
