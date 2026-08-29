// The cloud march (plan 14.1). One fragment per panorama texel, one texel per
// direction: the shader turns a direction into a ray and returns what came back
// along it. Two ways of answering that, one switch (`frame.y`):
//
//   * **Volumetric** — Nubis' model: a low-frequency Perlin-Worley shape eroded
//     by a Worley detail that is wispy at the base and billowy above it, a
//     height profile that gives a cumulus its flat base and domed top, Beer
//     attenuation with a two-lobe Henyey-Greenstein phase for the silver lining
//     when the sun is behind them, and an ambient term read off the sky itself.
//   * **Layered** — the same shape field and the same lighting, but sampled
//     where the ray crosses the middle of the deck, with the self-shadow walked
//     across that height field instead of marched through a volume. A dozen
//     texture fetches against several hundred. What it loses is the billows and
//     the parallax through a cloud; the silver lining, the dark base and the
//     colour of the hour all survive.
//
// Either way only **one texel in sixteen** is marched a frame — `frame.x` names
// the 4×4 Bayer slot — and the other fifteen are carried over from last frame's
// panorama, bound here as `history_texture` (`clouds.rs` swaps the two
// buffers). That is what pays for the resolution: at 2048 × 1024 a texel is
// 0.18° of sky, where 768 × 384 was 0.47° and showed every one of them. Sixteen
// frames is a quarter of a second, in which a cloud at five kilometres drifts a
// fifth of a texel.
//
// A march is not written over its texel but **blended into it** (`history.x`),
// and each turn it sends its ray through a new point of the texel, starts it a
// new way into the first step and aims its light cone somewhere new
// (`history.yz`, `history2.x`), so the blend converges on the integral over the
// texel, the step and the cone: an edge filtered over the texel's footprint of
// sky, and a body without noise. One sample of either — a stepped edge, and a
// jitter that used to be the Bayer slot itself, a 4×4 pattern that repeated
// exactly — is what a 2K screen, stretching a texel over five pixels, showed as
// a raster. The deck drifts while the panorama remembers, so the history is
// read where the cloud *was* a turn ago (`reprojected`, `history.w`): the blend
// follows the cloud rather than smearing it along its path.
//
// The sky the clouds hang in is not guessed: Bevy's atmosphere writes its
// sky-view table into a cubemap every frame (`AtmosphereEnvironmentMapLight`),
// and that is bound here. It is what lights the shaded side of a cloud — blue
// at noon, orange towards a setting sun — without a second model of the
// atmosphere saying so. A far cloud fades into the sky behind it by the air's
// own optical depth (`aerial`), so the two never disagree by a shade.

#import bevy_sprite::mesh2d_vertex_output::VertexOutput

struct CloudParams {
    // xyz = direction towards the sun, w = 1 by day, 0 at night.
    sun: vec4<f32>,
    // rgb = the light reaching the layer [lx], a = drift time [s].
    light: vec4<f32>,
    // rgb = what the ground puts back up into the base of the deck [cd/m²],
    // a = cover 0…1.
    ground: vec4<f32>,
    // rgb = the least light a cloud is ever lit by — a night cloud is a shape
    // against the stars, not a hole in them.
    floor: vec4<f32>,
    // x = base [m], y = thickness [m], zw = wind [m/s].
    layer: vec4<f32>,
    // x = the Bayer slot this frame writes, or −1 for every texel at once;
    // y = 1 volumetric, 0 layered; z = extinction of the weather's haze [1/m],
    // w = its scale height [m].
    frame: vec4<f32>,
    // x = the weight of this frame's march against what the texel held (1
    // replaces it); y = where in its golden-ratio sequence this turn's ray
    // jitter stands, 0…1, and scaled up the seed of the cone wobble; z = the
    // point in the texel this turn's ray is sent through, 0…1, on the
    // panorama's x axis; w = the scenario seconds since the texels this frame
    // marches were last marched, by which the deck has drifted since their
    // history was written.
    history: vec4<f32>,
    // x = the point in the texel on the panorama's y axis; yzw reserved.
    history2: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> params: CloudParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var shape_texture: texture_3d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var shape_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var detail_texture: texture_3d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var detail_sampler: sampler;
// The atmosphere's own view of the sky, one texel per direction and no clouds
// in it (`clouds.rs` hands over Bevy's handle once a camera has one).
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var sky_texture: texture_cube<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var sky_sampler: sampler;
// Last frame's panorama: what a texel that is not marched this frame carries
// over, texel for texel, and what a marched one is blended into — sampled where
// the cloud stood a turn ago (`reprojected`).
@group(#{MATERIAL_BIND_GROUP}) @binding(7) var history_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(8) var history_sampler: sampler;

const PI = 3.14159265;
const EARTH_RADIUS = 6360000.0;

/// The most a texel is allowed to hold. The history is kept for good, so an
/// overflow of the half float it lives in would stay in the sky until the next
/// reset instead of for one turn.
const HALF_MAX = 65000.0;

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
/// cover to be right, only roughly where the cloud is. It is also the height the
/// sheet's ambient is read at: what the ground sees of a deck is its lower part.
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
/// The remaps that carve and threshold the shape leave a field that only reaches
/// about a third where a cloud is thickest, and tails off over kilometres rather
/// than metres. A view ray still turns opaque — it crosses the deck for long
/// enough — so the clouds *look* solid, but the light ray only crosses a few
/// hundred metres, never gathers any optical depth, and every cloud comes out
/// evenly lit on all sides. Bringing the core up to one is what gives a cumulus
/// a dark base and a bright rim, and it is the difference between a raymarch and
/// an expensive way of drawing a blob.
const DENSITY_GAIN = 4.0;

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

/// How deeply the shape's own finer Worley octaves carve its surface, and how
/// deeply the detail volume carves what is left.
///
/// Both are thresholds on the field, and both are highest *between* the billows
/// the Worley cells describe: a billow's centre keeps everything, the gap beside
/// it loses this much. That direction is the whole point — carved the other way
/// round (the sign this shader once had) the cloud grows *into* the gaps between
/// the cells and the sky fills with a honeycomb of haze that no coverage setting
/// can take out.
const SHAPE_EROSION = 0.55;
const DETAIL_EROSION = 0.5;

/// Below this fraction of the way up the deck the detail erodes wisps, above it
/// billows (Nubis' `mix(fbm, 1 − fbm, …)`): a cumulus is ragged underneath and a
/// cauliflower on top, and it is the same noise doing both.
const BILLOW_HEIGHT = 0.2;

/// What `1 − cover` is scaled by before it is the threshold on the shape. The
/// field's median is a half (`clouds.rs` sums the Perlin and the coarsest Worley
/// rather than masking one with the other), so the threshold is the cover's own
/// complement; this is the knob for when the *projected* cover — the fraction
/// of the sky a cumulus field actually hides — drifts from what the weather
/// asked for.
const COVER_BIAS = 1.0;

/// How much of the shape erosion a closed sky takes back. Carving the gaps
/// between the billows is what separates one cumulus from the next, and a
/// stratus deck has no gaps: it is one sheet with the billows as texture on its
/// underside. At full cover this leaves a third of the erosion, which is the
/// mottling without the holes.
const COVER_SOFTENING = 0.7;

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

/// How much of the light that gets deep into a cloud has forgotten where it came
/// from, per optical depth: the two-stream answer for a thick, non-absorbing
/// layer of droplets, `1 / (1 + k · τ)` with `k = ¾ (1 − g)` for `g ≈ 0.85`
/// (van de Hulst). A 1.2 km cumulus is about τ = 55, and a seventh of the light
/// on its top reaches its base — which is a base about half as bright as the
/// sky beside it, the way a photograph has it.
///
/// The scattering octaves above are directional and give out a few hundred
/// metres in; this is what lights everything past that. It is the whole
/// difference between a closed deck that is a grey sky and one that is a black
/// slab: a stratus base is not lit by the sky around it — there is none — but by
/// the sun diffusing down through two kilometres of cloud, and about a seventh
/// of it arrives.
const AMBIENT_DIFFUSION = 0.16;

/// How quickly, in optical depth from the top of the cloud, the diffused sun
/// takes over from the octaves. At the sunlit surface the octaves already say
/// E/π; counting the diffused light there too would put a lit face twice over
/// white.
const DIFFUSE_ONSET = 0.15;

/// The share of a cloud's ambient that arrives from above (the sky, the sun
/// diffused through the body) against from below (the ground), for a point
/// that is occluded from neither. A thin wisp is lit more by the sky than by
/// the fields under it; the base of a thick cloud is lit by little else.
const AMBIENT_ABOVE = 0.7;
const AMBIENT_BELOW = 0.25;

/// How dark the crevice between two billows goes in the ambient. A billow's
/// core and an outer wisp both see the sky; the gap *inside* the mass sees
/// mostly cloud, and the column estimate above cannot tell the two apart —
/// this can, from the body density (deep in the mass) against the fine one
/// (thin right here). It is what puts the shadow into a cauliflower top.
const CREVICE = 0.7;

/// Bevy's earth medium, as far as a cloud layer reaches: Rayleigh and Mie
/// extinction at sea level [1/m] and their scale heights [m]. The ozone term
/// sits at 25 km and never comes between a ground camera and a cloud.
const RAYLEIGH = vec3(5.802e-6, 13.558e-6, 33.1e-6);
const RAYLEIGH_HEIGHT = 8000.0;
const MIE = 4.44e-6;
const MIE_HEIGHT = 1200.0;

/// Period of the coverage field — the weather map, in kilometres rather than
/// metres, so a front is bigger than the clouds it carries.
const COVER_SCALE = 1.0 / 60000.0;

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

/// Three uncorrelated values in 0…1 from a texel and a turn — the wobble on the
/// light ray. An integer hash (Jarzynski & Olano's pcg3d) rather than the sine
/// trick, which stops being random once a turn count pushes its argument into
/// the hundreds of thousands.
fn hash3(v: vec3<u32>) -> vec3<f32> {
    var p = v * 1664525u + 1013904223u;
    p.x += p.y * p.z;
    p.y += p.z * p.x;
    p.z += p.x * p.y;
    p ^= p >> vec3(16u);
    p.x += p.y * p.z;
    p.y += p.z * p.x;
    p.z += p.x * p.y;
    return vec3<f32>(p) / 4294967295.0;
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

/// The atmosphere's radiance in a world direction. Bevy writes the cubemap with
/// z flipped (`environment.wgsl`, "cubemaps are left-handed") and reads it back
/// the same way, so this does too.
fn sky(dir: vec3<f32>) -> vec3<f32> {
    return textureSampleLevel(sky_texture, sky_sampler, vec3(dir.x, dir.y, -dir.z), 0.0).rgb;
}

/// What the sky puts onto the top of a cloud: the radiance a white surface
/// facing up would have under it, E/π, from nine taps of the cubemap — the
/// zenith for the cap of the hemisphere and a ring at 30° for the rest, which is
/// where the cosine-weighted bulk of the irradiance comes from. The same for
/// every texel, so it is read once per fragment and never inside the march.
fn sky_light() -> vec3<f32> {
    var ring = vec3(0.0);
    for (var i = 0; i < 8; i++) {
        let azimuth = (f32(i) + 0.5) * PI / 4.0;
        ring += sky(vec3(sin(azimuth) * 0.866, 0.5, -cos(azimuth) * 0.866));
    }
    return sky(vec3(0.0, 1.0, 0.0)) * 0.25 + ring * (0.75 / 8.0);
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
    return clamp(params.ground.a * 0.9 + (field - 0.5) * 0.5, 0.0, 1.0);
}

/// A cumulus has a flat base and a domed top; a closed deck is a slab. The cover
/// decides which of the two this is.
fn height_profile(height: f32, cover: f32) -> f32 {
    let bottom = smoothstep(0.0, 0.06, height);
    let top = 1.0 - smoothstep(mix(0.35, 0.8, cover), 1.0, height);
    return bottom * top;
}

/// The cloud at one already drifted sample position.
struct Density {
    /// What the ray meets, 0…1.
    fine: f32,
    /// The same before the detail took its wisps and billows off — the body of
    /// the cloud rather than its edge, which is what the ambient estimates how
    /// much cloud stands above and below the sample from.
    body: f32,
}

fn cloud_density(q: vec3<f32>, height: f32, cover: f32, detailed: bool) -> Density {
    var out: Density;
    out.fine = 0.0;
    out.body = 0.0;
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
    // The three Worley octaves as one fbm, high at the centre of a billow and low
    // in the gap beside it; the gap is where the surface is carved.
    let billows = shape.g * 0.625 + shape.b * 0.25 + shape.a * 0.125;
    let carve = SHAPE_EROSION * (1.0 - COVER_SOFTENING * cover);
    var density = remap(shape.r, (1.0 - billows) * carve, 1.0, 0.0, 1.0);
    // The profile goes on *before* the threshold, so a cloud narrows towards its
    // top instead of fading there: a fade leaves every cumulus a cylinder with a
    // soft lid, a narrowing gives it a dome. The threshold itself is soft — a hard
    // edge here is a sky either empty or overcast, with nothing in between.
    density = remap(
        density * height_profile(height, cover),
        (1.0 - cover) * COVER_BIAS,
        1.0,
        0.0,
        1.0,
    );
    if density <= 0.0 {
        return out;
    }
    out.body = min(density * DENSITY_GAIN, 1.0);
    if detailed {
        let detail = textureSampleLevel(
            detail_texture,
            detail_sampler,
            vec3(q.x * DETAIL_SCALE, height * DETAIL_TURNS, q.z * DETAIL_SCALE),
            0.0,
        );
        let fine = detail.g * 0.625 + detail.b * 0.25 + detail.a * 0.125;
        // Wisps low down, billows higher up: at the base the *centres* of the
        // detail cells are taken out and the strands between them hang on, above
        // it the gaps are taken out and the cells stand as cauliflower.
        let carve = mix(fine, 1.0 - fine, smoothstep(0.0, BILLOW_HEIGHT, height));
        density = remap(density, carve * DETAIL_EROSION, 1.0, 0.0, 1.0);
    }
    out.fine = clamp(density * DENSITY_GAIN, 0.0, 1.0);
    return out;
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
        sum += cloud_density(drifted(pos), height, cover, false).body * step;
        // Widening steps reach further for the same cost.
        step *= 1.7;
    }
    return sum;
}

/// The light scattered towards the viewer at one sample, summed over octaves of
/// ever weaker, ever more diffuse, ever less attenuated scattering (Wrenninge's
/// approximation of the multiple scattering a real cloud lives on).
fn scattered_light(cos_sun: f32, depth: f32, density: f32) -> vec3<f32> {
    // The powder term — thin cloud scatters less back at the viewer than Beer's
    // law alone says, which is what puts the dark crevices into a front-lit
    // cumulus. It is a *front-lit* effect: with the sun behind the cloud the same
    // thin edge is the brightest thing in the sky, the forward lobe below sees to
    // that, and darkening it there turns every silver lining into a bruise. So it
    // is weighted by how far the sun is behind the viewer.
    let front = 0.5 - 0.5 * cos_sun;
    let powder = 1.0 - front * exp(-2.0 * density);
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
        // the later octaves are the light that has already lost its direction.
        let sugar = select(1.0, powder, octave == 0);
        luminance += attenuation * phase * sugar * exp(-EXTINCTION * contribution * depth);
        attenuation *= SCATTER_FADE;
        contribution *= SCATTER_REACH;
        eccentricity *= SCATTER_SPREAD;
    }
    return vec3(luminance * SCATTER_GAIN);
}

/// How much of a diffuse light is left after an optical depth of cloud, all
/// orders of scattering together.
fn diffused(tau: f32) -> f32 {
    return 1.0 / (1.0 + AMBIENT_DIFFUSION * tau);
}

/// The light a sample gets from everything that is not the sun's own beam: the
/// sky above the deck and the sun diffused down through it, and the ground
/// below — each seen through however much cloud stands in the way.
///
/// The amount in the way is an estimate, not a march: the body density at the
/// sample, taken for the whole column above and below it. Inside a cumulus that
/// is about right, at its edge it says "little", and both are what a photograph
/// shows — a dark base that brightens out to its rim, a top that is lit all the
/// way in.
fn ambient(sky_top: vec3<f32>, cloud: Density, height: f32, thickness: f32) -> vec3<f32> {
    let body = cloud.body;
    let column = EXTINCTION * body * thickness;
    let above = column * (1.0 - height);
    let below = column * height;
    // What the sun puts onto the top of the deck, as the radiance of a white
    // surface — but only counted once it is deep enough for the octaves to have
    // given out, or a lit face is white twice over.
    let sun = params.light.rgb
        * (max(params.sun.y, 0.0) / PI)
        * (1.0 - exp(-above * DIFFUSE_ONSET));
    let from_above = (sky_top + sun) * diffused(above);
    let from_below = params.ground.rgb * diffused(below);
    let crevice = 1.0 - CREVICE * body * exp(-2.0 * cloud.fine);
    return (from_above * AMBIENT_ABOVE + from_below * AMBIENT_BELOW) * crevice + params.floor.rgb;
}

/// The air between the camera and the cloud: what a far cloud loses to it, and
/// how much of the sky behind stands in for what the air put in front of it.
///
/// The optical depth is Bevy's own earth medium plus the weather's haze — the
/// same Koschmieder term `sky::haze` adds to the atmosphere — integrated up to
/// the cloud's height and along the slant. What the air scatters in on the way
/// is not painted here but *let through*: the cloud's coverage is cut by the
/// share of the sky's whole optical depth that lies in front of the cloud, and
/// the sky the dome composites over — the atmosphere's own render, not this
/// shader's cubemap copy of it, which under a 300 m fog differs by a shade and
/// showed as a band — fills that share. So a cumulus at the horizon goes warm
/// and pale into the haze the way the trees under it do, one overhead picks up
/// the faint blue of the kilometre and a half of air under it, and a fog that
/// closes the view to 300 m closes it to the clouds as well.
fn aerial(cloud: vec4<f32>, dir: vec3<f32>, distance: f32) -> vec4<f32> {
    if cloud.a <= 0.0 {
        return cloud;
    }
    let height = max(dir.y * distance, 0.0);
    // Plane-parallel: fine above the 3° the horizon fade already takes out.
    let slant = 1.0 / max(dir.y, 0.02);
    let rayleigh = RAYLEIGH * RAYLEIGH_HEIGHT;
    let mie = vec3(MIE * MIE_HEIGHT);
    let haze = vec3(params.frame.z * params.frame.w);
    let tau = (rayleigh * (1.0 - exp(-height / RAYLEIGH_HEIGHT))
        + mie * (1.0 - exp(-height / MIE_HEIGHT))
        + haze * (1.0 - exp(-height / params.frame.w))) * slant;
    let tau_sky = (rayleigh + mie + haze) * slant;
    let through = exp(-tau);
    // Coverage is one number, so the share is taken by luminance; the colour of
    // the light in front is the sky's own, per channel, for free.
    let share = (1.0 - through) / max(1.0 - exp(-tau_sky), vec3(1e-3));
    let seen = 1.0 - dot(share, vec3(0.2126, 0.7152, 0.0722));
    return vec4(cloud.rgb * through, cloud.a * seen);
}

/// Where in the panorama the cloud now seen in `dir` was seen a turn ago. The
/// deck has drifted since the texel's history was written, and the history is
/// read where the cloud *was*, so the blend follows the cloud instead of
/// smearing it along its path — at a storm's 40 m/s aloft a cumulus overhead
/// crosses a texel a turn. The deck is taken to stand at its middle height:
/// exact for the sheet, and for the volume a fraction of a texel out.
fn reprojected(dir: vec3<f32>, base: f32, thickness: f32) -> vec2<f32> {
    let origin = vec3(0.0, EARTH_RADIUS, 0.0);
    let hit = sphere_exit(origin, dir, EARTH_RADIUS + base + thickness * 0.5);
    // `drifted` adds the drift to the noise coordinate, so a feature of the
    // field moves *against* the drift vector: what is at p now was at
    // p + drift × Δt a turn ago.
    let shift = params.layer.zw * params.history.w;
    let then = normalize(dir * max(hit, 0.0) + vec3(shift.x, 0.0, shift.y));
    // The dome's mapping, run backwards.
    let azimuth = atan2(then.x, -then.z);
    let elevation = asin(clamp(then.y, -1.0, 1.0));
    let u = azimuth / (2.0 * PI) + 0.5;
    let v = sqrt(max(elevation, 0.0) / (PI * 0.5));
    return vec2(u, 1.0 - v);
}

/// The volumetric answer: walk the shell of cloud and integrate what scatters
/// back along the way.
fn march(
    dir: vec3<f32>,
    to_sun: vec3<f32>,
    jitter: f32,
    base: f32,
    thickness: f32,
    sky_top: vec3<f32>,
) -> vec4<f32> {
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
    // Where along the ray the light that came back was scattered, weighted by how
    // much of it: the distance the aerial perspective is applied at.
    var reach = 0.0;
    var travelled = start + step * jitter;

    for (var i = 0; i < VIEW_STEPS; i++) {
        if transmittance < 0.01 {
            break;
        }
        let distance = travelled;
        let p = origin + dir * distance;
        let height = (length(p) - EARTH_RADIUS - base) / thickness;
        travelled += step;
        if height < 0.0 || height > 1.0 {
            continue;
        }
        let cover = coverage(p.xz);
        let cloud = cloud_density(drifted(p), height, cover, true);
        if cloud.fine <= 0.001 {
            continue;
        }

        let extinction = EXTINCTION * cloud.fine;
        let depth = light_depth(p, to_sun, base, thickness, cover);
        let light = params.light.rgb * scattered_light(cos_sun, depth, cloud.fine) * params.sun.w;

        // Energy-conserving integration over the step (Hillaire): the analytic
        // integral, not a Riemann sum, so the steps look like many more.
        let scatter = (light + ambient(sky_top, cloud, height, thickness)) * extinction;
        let attenuation = exp(-extinction * step);
        scattered += transmittance * (scatter - scatter * attenuation) / extinction;
        reach += transmittance * (1.0 - attenuation) * distance;
        transmittance *= attenuation;
    }
    let alpha = 1.0 - transmittance;
    return aerial(vec4(scattered, alpha), dir, reach / max(alpha, 1e-4));
}

/// The cheap answer: the deck as one lit sheet rather than a volume.
///
/// The density read where the ray crosses the middle of the layer stands for the
/// whole thickness, which turns the second march into a short walk across that
/// height field towards the sun — the classic 2.5D cloud layer, but sharing the
/// march's shape field and its scattering, so the two look like the same sky.
fn layered(
    dir: vec3<f32>,
    to_sun: vec3<f32>,
    base: f32,
    thickness: f32,
    sky_top: vec3<f32>,
) -> vec4<f32> {
    let origin = vec3(0.0, EARTH_RADIUS, 0.0);
    let hit = sphere_exit(origin, dir, EARTH_RADIUS + base + thickness * 0.5);
    if hit < 0.0 || hit > MAX_DISTANCE {
        return vec4(0.0);
    }
    let p = origin + dir * hit;
    let cover = coverage(p.xz);
    var cloud: Density;
    cloud.fine = 0.0;
    cloud.body = 0.0;
    for (var i = 0; i < LAYER_SLICES; i++) {
        let slice = cloud_density(drifted(p), (f32(i) + 0.5) / f32(LAYER_SLICES), cover, true);
        cloud.fine += slice.fine;
        cloud.body += slice.body;
    }
    cloud.fine /= f32(LAYER_SLICES);
    cloud.body /= f32(LAYER_SLICES);
    if cloud.fine <= 0.001 {
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
        depth += cloud_density(drifted(q), LAYER_HEIGHT, coverage(q.xz), false).body * stride;
    }
    depth *= LAYER_SHADOW;

    let cos_sun = dot(dir, params.sun.xyz);
    let light = params.light.rgb * scattered_light(cos_sun, depth, cloud.fine) * params.sun.w;
    // The same integral the march does per step, done once over the whole
    // thickness — which is exactly what treating the deck as one sample means.
    let alpha = 1.0 - exp(-EXTINCTION * cloud.fine * thickness);
    let radiance = light + ambient(sky_top, cloud, LAYER_HEIGHT, thickness);
    return aerial(vec4(radiance * alpha, alpha), dir, hit);
}

/// The panorama's mapping: longitude round from north, latitude squared so the
/// horizon — where the layer is edge-on and most of the sky's detail sits —
/// gets the samples.
fn direction(uv: vec2<f32>) -> vec3<f32> {
    let azimuth = (uv.x - 0.5) * 2.0 * PI;
    let v = 1.0 - uv.y;
    let elevation = v * v * (PI * 0.5);
    return vec3(
        cos(elevation) * sin(azimuth),
        sin(elevation),
        -cos(elevation) * cos(azimuth),
    );
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = vec2<u32>(in.position.xy);
    let slot = bayer4(texel.x & 3u, texel.y & 3u);
    // Amortisation: this texel is marched on one frame in sixteen; on the other
    // fifteen it carries over what last frame's panorama holds. Returning before
    // anything is marched is the whole saving.
    if params.frame.x >= 0.0 && slot != u32(params.frame.x) {
        return textureLoad(history_texture, vec2<i32>(texel), 0);
    }

    // Panorama mapping: longitude round, latitude squared so the horizon — where
    // the layer is edge-on and most of the sky's detail sits — gets the samples.
    // The ray goes through a point of the texel that moves every turn, not
    // through its centre, so what the blend settles on is the texel's footprint
    // of sky and not one line through it — the difference between a cloud edge
    // filtered over the texel and one stepped along it. The history is read for
    // the texel's centre, which the blend keeps where it is.
    // Every texel has a phase of its own in the sequences (a Cranley-Patterson
    // rotation): over the turns each texel's samples are still stratified, and
    // between neighbours they are uncorrelated, so what the blend has not yet
    // averaged away is grain and not a pattern. Anything with a period — the
    // Bayer slot this once was, the interleaved gradient noise tried after it,
    // which repeats every two texels one way and three the other — blended at
    // the unequal weights of a running average shows through as a faint
    // lattice, which is the raster this whole arrangement exists to remove.
    let phase = hash3(vec3(texel, 7919u));
    let point = fract(phase.yz + vec2(params.history.z, params.history2.x)) - 0.5;
    let uv = in.uv + point / vec2<f32>(textureDimensions(history_texture));
    let dir = direction(uv);
    let base = params.layer.x;
    let thickness = params.layer.y;
    let previous = textureSampleLevel(
        history_texture,
        history_sampler,
        reprojected(direction(in.uv), base, thickness),
        0.0,
    );
    // Where along the first step the ray starts, and where in the cone the light
    // ray points. Both move with every turn — the jitter along a golden-ratio
    // sequence, the wobble by its seed — so that over the panorama's memory a
    // texel sees the whole step and the whole cone, and the blend below arrives
    // at their average.
    let jitter = fract(phase.x + params.history.y);
    let wobble = hash3(vec3(texel, u32(params.history.y * 65536.0))) - 0.5;
    let to_sun = normalize(params.sun.xyz + wobble * CONE_SPREAD);
    let sky_top = sky_light();

    var cloud: vec4<f32>;
    if params.frame.y > 0.5 {
        cloud = march(dir, to_sun, jitter, base, thickness, sky_top);
    } else {
        cloud = layered(dir, to_sun, base, thickness, sky_top);
    }

    // Near the horizon the layer runs out into haze rather than ending in a line.
    let horizon = smoothstep(0.0, 0.06, dir.y);
    // Blended into what the texel held rather than written over it.
    let fresh = clamp(cloud * horizon, vec4(0.0), vec4(HALF_MAX));
    return mix(previous, fresh, params.history.x);
}
