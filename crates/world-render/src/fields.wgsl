// Farmland (the field plan, ch. 6 and 7). The mesh is the ground under a crop;
// everything that makes it read as *that* crop on *that* day is here.
//
// Three things are drawn on top of one another:
//
//   1. The crop's colour, over bare soil, mixed by how much of the ground the
//      stand covers on the day. A field in April is mostly soil with green
//      lines on it; the same field in June is a closed green surface.
//   2. The working rows. `uv.x` runs across the direction the field was worked
//      in and `uv.y` along it, both in metres from the field's own centre, so
//      the furrows of one field line up across every tile it crosses and two
//      neighbouring fields never share a phase.
//   3. The tramlines — the wheel tracks the sprayer leaves every twenty-odd
//      metres, dead straight, and the giveaway that a field is farmed rather
//      than merely green.
//
// The per-field variation comes in through the vertex colour: r is a tint, g a
// phase. Both are drawn from the field's own seed, so they are the same on
// every machine of a multiplayer run without anything being sent.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    mesh_view_bindings::{globals, view},
}
#import world_render::weather::{Weather, weather_pbr}

struct Crop {
    // rgb = the stand's colour, a = how much of the ground it covers.
    color: vec4<f32>,
    // x = how strongly the rows read, y = row spacing [m],
    // z = tramline spacing [m], w = the stand's height [m].
    rows: vec4<f32>,
    // rgb = the soil, a = roughness of the surface.
    soil: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> crop: Crop;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var<uniform> weather: Weather;

// A cheap hash for the speckle that keeps a large field from looking like paint.
fn noise(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2(12.9898, 78.233))) * 43758.5453);
}

// Value noise: the hash above, smoothed. One octave is enough — this is a
// modulation of a colour, not a texture.
fn smooth_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = noise(i);
    let b = noise(i + vec2(1.0, 0.0));
    let c = noise(i + vec2(0.0, 1.0));
    let d = noise(i + vec2(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    let tint = in.color.r;
    let phase = in.color.g;
    // Across the rows, and along them.
    let across = in.uv.x;
    let along = in.uv.y;

    let spacing = max(crop.rows.y, 0.05);
    let strength = crop.rows.x;

    // The rows themselves: a soft stripe across the working direction. `fwidth`
    // fades them out as they fall below a pixel, so a field at the horizon is a
    // flat colour rather than a moire pattern.
    let row_phase = across / spacing + phase;
    let row = abs(fract(row_phase) - 0.5) * 2.0;
    let row_fade = 1.0 - smoothstep(0.15, 0.9, fwidth(row_phase));
    let furrow = mix(1.0, 0.55 + 0.45 * row, strength * row_fade);

    // The tramlines: two wheel ruts, every `crop.rows.z` metres, that run the
    // length of the field. Bare soil, always, whatever the crop is doing.
    let tram_spacing = max(crop.rows.z, 1.0);
    let tram_phase = across / tram_spacing + phase * 0.37;
    let tram_here = abs(fract(tram_phase) - 0.5) * 2.0;
    let tram_fade = 1.0 - smoothstep(0.2, 1.0, fwidth(tram_phase));
    // Two ruts a metre and a half apart, so the pair reads as a vehicle's track.
    let rut = smoothstep(0.955, 0.999, tram_here) * tram_fade;

    // The speckle: where the crop stands thicker and thinner. Slightly
    // stretched along the rows, because a drill run is more even down its
    // length than across the field — but only slightly. Stretched hard it
    // stops reading as mottling and starts reading as a second set of rows.
    let speck = smooth_noise(vec2(across * 0.13, along * 0.08)) - 0.5;

    // Soil first, crop over it by the cover.
    let soil = crop.soil.rgb * (1.0 + 0.16 * speck) * (0.92 + 0.16 * tint);
    var stand = crop.color.rgb * (0.90 + 0.20 * tint) * (1.0 + 0.13 * speck);
    stand = stand * furrow;
    var color = mix(soil, stand, crop.color.a);
    // The ruts are soil whatever grows beside them, and the wheels polish them.
    // Not to bare earth: a tramline is two wheel widths, and from a train it is
    // a line through the crop rather than a trench.
    color = mix(color, soil * 0.9, rut * 0.55);

    pbr_input.material.base_color = vec4(color, 1.0);
    // A closed stand scatters; bare soil and ruts are duller still.
    pbr_input.material.perceptual_roughness =
        clamp(crop.soil.a - 0.10 * crop.color.a + 0.06 * rut, 0.3, 1.0);
    // Rain, snow and the shadow of a cloud, the same way the terrain gets them.
    pbr_input = weather_pbr(weather, globals.time, pbr_input);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
