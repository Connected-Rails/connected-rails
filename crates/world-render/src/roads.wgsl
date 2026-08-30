// Roads (the road plan): the carriageways the line carries as `RoadSource`
// centre lines, cut to the terrain tiles and handed out with them (see
// `content::roads`). The mesh is only the surface, draped where the elevation
// data put it; everything that makes it read as *that* road is here.
//
// Three ingredients, layered:
//
//   1. The carriageway. The surface texture (asphalt or concrete, the two
//      materials the line's roads fall into) tiles in metres along and
//      across; the wear that keeps a road from looking like paint is value
//      noise over it, the same trick the fields use.
//   2. The markings. White paint where the vertex colours say the road
//      carries them: the edge lines at a hand's width from the kerb, and the
//      centre line — dashed in the stroke the German rulebook paints (the
//      6 m stroke outside towns, the 3 m one inside), or solid where
//      overtaking is forbidden. The paint rides *on* the texture, so the
//      road beneath shows its grain through it.
//   3. The weather. Rain darkens and polishes the carriageway, a cloud's
//      shadow lies on it, and a wet road mirrors the sky — the same
//      `weather_pbr` every other ground reads, so a road never disagrees
//      with the field it crosses.
//
// The marking data comes in through the vertex colour — r the centre line
// (0 none, 1 dashed, 2 dashed innerorts, 3 solid), g the edge lines, b the
// half-width in metres — so one material draws every road there is, and the
// mesh alone says which stripes to paint where.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    mesh_view_bindings::globals,
}
#import world_render::weather::{Weather, weather_pbr}

struct Road {
    weather: Weather,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> road: Road;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var surface_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var surface_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var surface_normal: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var surface_normal_sampler: sampler;

// A cheap hash for the wear that keeps a long road from looking like one
// repeating tile. Same function the fields use.
fn noise(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2(12.9898, 78.233))) * 43758.5453);
}

// German road markings per the RMS (Richtlinien für die Markierung von
// Straßen), in metres: the stroke is the 12 cm Schmalstrich, the edge lines
// sit 25 cm from the kerb. The centre dashes run 6 m on and 12 m off outside
// built-up areas, and the innerorts streets get the 3-and-6 the rulebook
// paints there — the ratio is 1:2 in both.
const MARK_WIDTH: f32 = 0.12;
const EDGE_OFFSET: f32 = 0.25;
const DASH: f32 = 6.0;
const GAP: f32 = 12.0;
const DASH_URBAN: f32 = 3.0;
const GAP_URBAN: f32 = 6.0;

/// The dashed centre line's stroke and gap, in metres: the 6-and-12 outside
/// towns, the 3-and-6 inside them. `urban` is the vertex colour's word for
/// the innerorts line.
fn dash_period(urban: bool) -> vec2<f32> {
    return select(vec2(DASH, GAP), vec2(DASH_URBAN, GAP_URBAN), urban);
}

/// A white stripe `centre_m` from the near kerb, `fade` widening it as it
/// approaches a pixel — the anti-aliasing the fields give their furrows.
fn stripe(across_m: f32, centre_m: f32, fade: f32) -> f32 {
    let d = abs(across_m - centre_m) / MARK_WIDTH;
    return 1.0 - smoothstep(0.6, 1.2, d * max(fade, 1.0));
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    // Across (0 at one kerb, 1 at the other) and along (metres from the
    // road's own start). The markings are measured in real metres — the
    // half-width rides in the vertex colour — and the texture tiles in real
    // metres on both axes, 4 m to the repeat, so the grain keeps its shape
    // whatever the carriageway's width: a 2 m path and a 15 m motorway
    // carriageway read the same asphalt, only more of it.
    let half_w = in.color.b;
    let width = 2.0 * half_w;
    let across_m = in.uv.x * width;
    let along_m = in.uv.y;
    let tex = vec2(across_m, along_m) / 4.0;
    var surface = textureSample(surface_texture, surface_sampler, tex).rgb;
    let bump = textureSample(surface_normal, surface_normal_sampler, tex).xyz * 2.0 - 1.0;
    surface = surface * (0.92 + 0.16 * (bump.x + bump.y));

    // How many metres of carriageway a pixel covers: below one mark-width it
    // is all edge, and the stripes fade out rather than turn to moire.
    let fade = fwidth(across_m) / MARK_WIDTH;

    var paint = 0.0;
    if in.color.g > 0.5 {
        paint = max(paint, stripe(across_m, EDGE_OFFSET, fade));
        paint = max(paint, stripe(across_m, width - EDGE_OFFSET, fade));
    }
    if in.color.r > 0.5 {
        // r = 1 the 6-and-12 outside towns, r = 2 the 3-and-6 inside them,
        // r = 3 the solid line that never lifts. The stroke keeps its
        // rulebook length — the anti-aliasing softens each dash end by about
        // a *pixel*, never by a share of the pattern — and once the gap
        // itself is sub-pixel the line reads as continuous rather than as a
        // picket fence.
        let urban = in.color.r > 1.5 && in.color.r < 2.5;
        let marks = dash_period(urban);
        let stroke = marks.x;
        let cycle = marks.x + marks.y;
        let phase = fract(along_m / cycle) * cycle;
        let aa = max(fwidth(along_m), 1e-4);
        let dash = smoothstep(-aa, aa, phase) * (1.0 - smoothstep(stroke - aa, stroke + aa, phase));
        let merged = smoothstep(0.75, 1.5, aa / marks.y);
        let on = max(dash, merged);
        paint = max(paint, stripe(across_m, half_w, fade) * on);
    }
    // Worn paint: greyer, and gone at the speckles the weather left.
    let wear = 0.82 + 0.18 * noise(vec2(along_m * 0.09, across_m * 1.7));
    let paint_color = vec3(0.72) * wear;

    var color = surface;
    color = mix(color, paint_color, paint * 0.92);
    pbr_input.material.base_color = vec4(color, 1.0);
    // Asphalt is dull; the paint is a touch glossier where it is fresh.
    pbr_input.material.perceptual_roughness = clamp(0.88 - 0.25 * paint, 0.5, 1.0);
    // Rain, snow and the shadow of a cloud, the same way the fields get them.
    pbr_input = weather_pbr(road.weather, globals.time, pbr_input);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
