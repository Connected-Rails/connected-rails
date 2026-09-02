// Roads (the road plan): the carriageways the line carries as `RoadSource`
// centre lines, cut to the terrain tiles and handed out with them (see
// `content::roads`). The mesh is only the surface, draped where the elevation
// data put it; everything that makes it read as *that* road is here.
//
// Three ingredients, layered:
//
//   1. The carriageway. The surface texture (asphalt or concrete, the two
//      materials the line's roads fall into) tiles in metres along and
//      across, and its normal map gives the grain its relief — the tangent
//      frame for it is read off the screen-space derivatives, so the mesh
//      does not have to carry one. The wear that keeps a road from looking
//      like paint is value noise over it, the same trick the fields use.
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
// (0 none, 1 dashed, 2 dashed innerorts, 3 solid), g how much of the edge
// lines runs here, b the half-width in metres, a how much of the centre line
// runs here — so one material draws every road there is, and the mesh alone
// says which stripes to paint where, and where a junction stops them.

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

// Value noise: the hash above, smoothed. Smoothed and not raw, because raw
// hash on a *fragment* position is white noise, and white noise on a road
// that runs to the horizon sparkles rather than weathers.
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

// How many metres of carriageway one repeat of the surface texture covers, on
// both axes. Fixed in metres rather than stretched over the width, so the
// grain keeps its shape whatever the carriageway is: a 2 m path and a 15 m
// motorway carriageway read the same asphalt, only more of it. Four metres
// is what the two 1k photographs were shot at.
const SURFACE_METRES: f32 = 4.0;

// How much of the normal map's relief the asphalt keeps. The photographs are
// of a flat road, so the map is a gentle one to begin with; this is the knob
// that decides whether the grain reads at all at a hundred metres.
const RELIEF: f32 = 0.7;

/// The dashed centre line's stroke and gap, in metres: the 6-and-12 outside
/// towns, the 3-and-6 inside them. `urban` is the vertex colour's word for
/// the innerorts line.
fn dash_period(urban: bool) -> vec2<f32> {
    return select(vec2(DASH, GAP), vec2(DASH_URBAN, GAP_URBAN), urban);
}

/// How much of a pixel a white stripe centred `centre_m` from the near kerb
/// covers. `pixel` is how many metres across the carriageway the pixel spans.
///
/// Coverage, not a smoothstep on the distance: a 12 cm stripe at two hundred
/// metres is a fifth of a pixel wide, and a smoothstep paints it either fully
/// white or not at all depending on where the pixel centre happens to fall.
/// That is what made the edge lines of a distant road a row of sparks and
/// left the far one out altogether. The share of the pixel the stripe
/// actually covers falls off smoothly instead, which is what a line does as
/// it goes away.
fn stripe(across_m: f32, centre_m: f32, pixel: f32) -> f32 {
    let reach = MARK_WIDTH * 0.5;
    let d = abs(across_m - centre_m);
    return clamp((reach - d) / max(pixel, 1e-5) + 0.5, 0.0, 1.0);
}

/// Metres of painted stroke before `x` in a dash pattern of `stroke` on and
/// `cycle` in all — the running sum the coverage of a dash is a difference
/// of. `x` has to be small (under two cycles), or the difference of two of
/// them loses its digits.
fn dash_before(x: f32, stroke: f32, cycle: f32) -> f32 {
    let n = floor(x / cycle);
    return n * stroke + min(x - n * cycle, stroke);
}

/// How much of a pixel the dashed centre line covers at `along_m`, over a
/// pixel that spans `pixel` metres along the road — the exact share, so a
/// dashed line does not turn into a picket fence as the dashes go sub-pixel
/// but fades towards the third of itself that is paint.
fn dash_coverage(along_m: f32, stroke: f32, cycle: f32, pixel: f32) -> f32 {
    let width = max(pixel, 1e-4);
    if width >= cycle {
        return stroke / cycle;
    }
    // Both ends of the pixel measured from the same cycle, so the two running
    // sums stay small and their difference keeps its precision.
    let start = along_m - 0.5 * width;
    let phase = start - floor(start / cycle) * cycle;
    let painted = dash_before(phase + width, stroke, cycle) - dash_before(phase, stroke, cycle);
    return clamp(painted / width, 0.0, 1.0);
}

/// The tangent frame of the surface texture at this fragment, from the
/// derivatives of the world position and of the texture coordinate
/// (Schüler's method). The road mesh carries no tangents of its own: it is a
/// ribbon whose texture axes are the road's own axes, and reading them off
/// the derivatives costs two subtractions and saves an attribute on every
/// vertex of every carriageway in the module.
fn tangent_frame(n: vec3<f32>, position: vec3<f32>, uv: vec2<f32>) -> mat3x3<f32> {
    let dp_x = dpdx(position);
    let dp_y = dpdy(position);
    let duv_x = dpdx(uv);
    let duv_y = dpdy(uv);
    let perp_x = cross(dp_y, n);
    let perp_y = cross(n, dp_x);
    let t = perp_x * duv_x.x + perp_y * duv_y.x;
    let b = perp_x * duv_x.y + perp_y * duv_y.y;
    // A fragment where the frame degenerates (a silhouette, a zero-area
    // triangle) would otherwise divide by nothing and turn black.
    let scale = inverseSqrt(max(max(dot(t, t), dot(b, b)), 1e-12));
    return mat3x3(t * scale, b * scale, n);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    // Across (0 at one kerb, 1 at the other) and along (metres from the
    // road's own start). The markings are measured in real metres — the
    // half-width rides in the vertex colour, and the u is the position across
    // the carriageway, so `u · width` is the metre across it.
    let half_w = in.color.b;
    let width = 2.0 * half_w;
    let across_m = in.uv.x * width;
    let along_m = in.uv.y;
    let tex = vec2(across_m, along_m) / SURFACE_METRES;
    let surface = textureSample(surface_texture, surface_sampler, tex).rgb;

    // The grain, as relief rather than as a shade of grey: the map is a
    // normal map, and a normal map read as brightness is a road that is
    // uniformly too dark and lit from nowhere.
    let bump = textureSample(surface_normal, surface_normal_sampler, tex).xyz * 2.0 - 1.0;
    let frame = tangent_frame(pbr_input.N, in.world_position.xyz, tex);
    pbr_input.N = normalize(frame * normalize(vec3(bump.xy * RELIEF, bump.z)));

    // How many metres of carriageway one pixel spans, across the road and
    // along it. Both markings are filtered over exactly that much.
    let across_px = fwidth(across_m);
    let along_px = fwidth(along_m);

    // How much of each marking runs here. `g` is the edge lines and `a` the
    // centre line, and a junction takes them out: where another carriageway
    // covers this one, the ground has plain asphalt, not the two roads'
    // stripes drawn through one another into a lattice.
    let edges = in.color.g;
    let centre = in.color.a;

    var paint = 0.0;
    if edges > 0.001 {
        paint = max(paint, stripe(across_m, EDGE_OFFSET, across_px) * edges);
        paint = max(paint, stripe(across_m, width - EDGE_OFFSET, across_px) * edges);
    }
    if in.color.r > 0.5 && centre > 0.001 {
        // r = 1 the 6-and-12 outside towns, r = 2 the 3-and-6 inside them,
        // r = 3 the solid line that never lifts.
        let urban = in.color.r > 1.5 && in.color.r < 2.5;
        let solid = in.color.r > 2.5;
        let marks = dash_period(urban);
        let on = dash_coverage(along_m, marks.x, marks.x + marks.y, along_px);
        let line = stripe(across_m, half_w, across_px) * select(on, 1.0, solid);
        paint = max(paint, line * centre);
    }
    // Worn paint: greyer, and gone at the patches the weather left. Metres,
    // not fragments — a stretch of tired paint is a couple of metres long.
    let wear = 0.82 + 0.18 * smooth_noise(vec2(along_m * 0.35, across_m * 1.7));
    let paint_color = vec3(0.72) * wear;

    var color = surface;
    color = mix(color, paint_color, paint * 0.92);
    pbr_input.material.base_color = vec4(color, 1.0);
    // Asphalt is dull and gravel duller still — the material brings its own
    // roughness; the paint is a touch glossier where it is fresh, and a loose
    // surface carries no paint to begin with.
    pbr_input.material.perceptual_roughness =
        clamp(pbr_input.material.perceptual_roughness - 0.25 * paint, 0.5, 1.0);
    // Rain, snow and the shadow of a cloud, the same way the fields get them.
    pbr_input = weather_pbr(road.weather, globals.time, pbr_input);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
