// Water (plan ch. 14): the lakes and rivers the line carries as
// `WaterSource` polygons, cut to the terrain tiles and handed out with them
// (see `content::water`). The mesh is only the surface, standing where the
// elevation data put it; everything that makes it read as water is here.
//
// The mesh brings two numbers per vertex, and both of them are pure functions
// of where the vertex is — the vertex colour carries the depth of the water
// column in metres (`r`) and how far the waterline is (`g`). What is made of
// them, in layers:
//
//   1. The surface. Ten octaves of directional waves, from a lake's swell down
//      to a hand's-breadth ripple, running with the wind — the long ones along
//      it, the short ones spread around it — each one grown by as much wind,
//      room and water as it has, and each one drawn only while it is bigger
//      than the pixel that would have to hold it. What falls below that is
//      not dropped but folded into the roughness, which is why a lake a
//      kilometre off glitters instead of crawling with aliasing.
//   2. What is under it. The material is opaque with *specular transmission*:
//      Bevy draws it after the opaque world with a copy of the picture bound,
//      and the bed is read back out of that copy along the refracted ray —
//      bent by the waves, attenuated by Beer's law over the column the mesh
//      carries. A shallow shore shows its sand through green water; a lake
//      past a few metres shows nothing but the water itself.
//   3. What is over it. The same copy is what the reflection rays march into,
//      against the depth the world was drawn with: the bank, the trees on it
//      and the train stand in the water where they ought to, wobbling with
//      the waves. Where a ray leaves the screen, or the surface is too rough
//      to mirror anything sharply, the atmosphere's environment probe takes
//      over — the sky, the sun's own glitter, the fresnel between them.
//   4. The shore. Where the water runs out, the wave leaves a band of foam and
//      stirred sand behind it — torn up by a noise and breathing with the
//      swell, because a waterline is not a stroke around a polygon. In a gale
//      the crests break out on the open water too.
//   5. The weather. Rain rings its drops into the surface (the same
//      `ripple_slope` the wet ground uses), a cloud's shadow lies on it like
//      it lies on the fields, and a dying wind calms it towards glass.
//
// A sixth thing falls out of the mesh alone: a river runs. Nothing says which
// body is one, but a lake's surface is level where a river's follows the fall
// of the valley, so the surface normal the tiles were built with is the
// gradient, and Manning's formula turns it into a current the whole wave field
// is carried downstream at.
//
// The waves run in the mesh's own `uv` — metres east and north of the body's
// centre — rather than in world coordinates. Two tiles of one lake then agree
// on every crest across the seam between them, and the floating origin can
// rebase under the camera without the surface changing pattern under it. The
// weather's own effects (the rain rings, the cloud shadow) stay in world
// coordinates, where the ground has them too.
//
// The time comes from the view's own globals, so nothing is uploaded per
// frame; the uniform only changes when the weather does.

#define_import_path world_render::water

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    pbr_types::PbrInput,
    mesh_view_bindings::{globals, view, view_transmission_texture, view_transmission_sampler},
    view_transformations::depth_ndc_to_view_z,
    utils::interleaved_gradient_noise,
}
#ifdef DEPTH_PREPASS
#import bevy_pbr::prepass_utils::prepass_depth
#endif
#ifdef TONEMAP_IN_SHADER
#import bevy_core_pipeline::tonemapping::approximate_inverse_tone_mapping
#endif
#import world_render::weather::{Weather, ripple_slope, cloud_shade, noise21}

/// The weather, in the layout `WeatherParams` (Rust) writes.
struct Water {
    weather: Weather,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> water: Water;

// Fresnel reflectance of water at normal incidence is 0.02, which is
// `sqrt(0.02 / 0.16)` in the reflectance parametrisation of `StandardMaterial`
// — the same number the wet ground polishes itself towards.
const WATER_REFLECTANCE = 0.354;
const WATER_F0 = 0.02;

/// Refractive index of water.
const WATER_IOR = 1.333;

/// Octaves of the wave spectrum, longest first. Ten of them at [`RATIO`] reach
/// from a lake's swell to the ripple a puff of wind raises.
const OCTAVES = 10u;

/// The longest wave the field carries [m].
const LONGEST = 34.0;

/// Wavelength from one octave to the next.
const RATIO = 0.62;

/// Gradient one octave contributes at a full sea, rise over run. Ten of them
/// out of phase make an rms slope of about 0.2 rad, and a fifth of that in a
/// light breeze — which is roughly the mean square slope Cox and Munk read off
/// the sun's glitter from an aeroplane in 1954, and it is what decides whether
/// a surface reads as water or as a sheet of glass.
const OCTAVE_SLOPE = 0.09;

/// Steps a reflection ray takes across the screen before it is given up, and
/// how many times a hit is halved to find the surface exactly.
const MIRROR_STEPS = 20u;
const MIRROR_REFINE = 4u;

/// What the wave field is doing at one point.
struct Surface {
    /// −∇h: how the normal leans, from the octaves coarse enough to draw.
    slope: vec2<f32>,
    /// Variance of the gradient of everything too fine to draw — the sharpness
    /// the reflection has to give up in exchange.
    lost: f32,
    /// 0 in a trough … 1 on a crest, weighted by the octaves' amplitudes.
    crest: f32,
    /// How much sea there is at all, 0 = glass … 1 = whipped up.
    energy: f32,
}

/// How fast the water runs here, and which way [m/s].
///
/// Nothing tells the shader whether a body is a lake or a river — but the
/// mesh's own normal does. `content::water` never lets a surface sink below
/// the level its shoreline settled at, and a lake's level is level: its normal
/// stands straight up. A river's does not, because there the surface follows
/// the fall of the valley, and how far the normal leans is the gradient the
/// water runs down.
///
/// Manning's formula turns that gradient into a speed, with the roughness of a
/// natural bed and the depth for the hydraulic radius: a metre and a half of
/// water on a fall of one in a thousand runs at a good metre a second, which
/// is what a German lowland river does.
fn flow(normal: vec3<f32>, depth: f32) -> vec2<f32> {
    // For a height field, the normal is `(−∂h/∂x, 1, −∂h/∂z)` — so this is the
    // way downhill, and its length is the fall.
    let downhill = vec2(normal.x, normal.z) / max(normal.y, 0.05);
    // Elevation data is never quite flat, not even under a lake. Below a tenth
    // of a per mille nothing is running; that is the DGM's own noise.
    let grade = length(downhill) - 1.0e-4;
    if grade <= 0.0 || depth < 0.05 {
        return vec2(0.0);
    }
    let speed = min(28.0 * pow(min(depth, 3.0), 0.667) * sqrt(grade), 3.0);
    return normalize(downhill) * speed;
}

/// The wave field at `p` (metres in the body's own frame), for water `depth`
/// metres deep and `shore` metres from the waterline, running at `stream`.
/// `footprint` is how much ground one pixel covers here — the resolution the
/// field is drawn to.
fn waves(p: vec2<f32>, depth: f32, shore: f32, stream: vec2<f32>, footprint: f32) -> Surface {
    let time = globals.time;
    let wind = water.weather.wind.xy;
    let speed = length(wind);
    let current = length(stream);
    // A river makes its own chop, whatever the air above it is doing.
    let stir = max(speed, current * 1.6);
    // The waves run with the wind. With no wind there is no direction to run
    // in; the current gives one, and failing that any fixed one will do,
    // because there is nothing left to see anyway.
    var dir = vec2(0.786, 0.618);
    if speed > 0.05 {
        dir = wind / speed;
    } else if current > 0.05 {
        dir = stream / current;
    }
    let across = vec2(-dir.y, dir.x);
    // Sea state. The square root is the energy: a breeze already covers the
    // water in ripples, and more wind mostly makes them longer, not steeper.
    // Never quite zero — perfectly still water is a thing photographs almost
    // never show.
    let sea = sqrt(clamp(stir / 7.0, 0.04, 1.0));
    // The cat's paws that walk over a lake: the short waves are what a gust
    // raises and a lull lays flat again. A gust is a streak, long along the
    // wind and narrow across it, and it drifts downwind.
    let blown_by = p - wind * time * 0.6;
    let gust = 0.5 + 0.9 * noise21(vec2(dot(blown_by, dir) * 0.006, dot(blown_by, across) * 0.02));
    // Ten sines summed on a straight coordinate make a washboard, whatever
    // their directions are. Bending the ground they are read from — a slow
    // noise, drifting with the water — is what turns their crests into the
    // wandering, interrupted lines a lake actually shows.
    let warp = vec2(
        noise21(p * 0.021 - stream * time * 0.02),
        noise21(p * 0.021 + 41.7 - stream * time * 0.02),
    ) - 0.5;
    let at = p + warp * 9.0;

    var out: Surface;
    out.slope = vec2(0.0);
    out.lost = 0.0;
    out.crest = 0.0;
    out.energy = sea;
    var weight = 1.0e-6;
    var wavelength = LONGEST;
    // What every octave so far has pushed the ground under the next one by.
    var bend = vec2(0.0);
    for (var i = 0u; i < OCTAVES; i++) {
        // 1 for the longest octave, 0 for the shortest.
        let band = f32(OCTAVES - 1u - i) / f32(OCTAVES - 1u);
        let k = 6.2831853 / wavelength;

        // What lets this octave grow: the wind — a long wave needs far more of
        // it than a ripple does — the room between here and the shore, and the
        // water under it, since a wave feels the bed at a fraction of its own
        // length and dies as it shoals.
        let blown = pow(sea, mix(0.45, 2.6, band));
        let room = smoothstep(wavelength * 0.22, wavelength * 0.95, shore);
        let bed = smoothstep(wavelength * 0.015, wavelength * 0.14, depth);
        let amp = OCTAVE_SLOPE * blown * room * bed * mix(1.0, gust, 1.0 - band);

        // Below the pixel it would have to fit in, an octave is not drawn but
        // remembered: its gradient goes into the roughness instead of into the
        // normal. That is the whole difference between a far lake that
        // glitters and one that boils.
        let seen = smoothstep(footprint * 1.0, footprint * 4.0, wavelength);
        out.lost += (1.0 - seen) * amp * amp * 0.5;
        if seen > 0.002 {
            // Spread around the wind: the long waves run with it, within ten
            // degrees or so, the short ones cross it at up to fifty — and it is
            // that crossing that breaks a reflection.
            let angle = mix(0.9, 0.2, band) * (fract(f32(i) * 0.61803) - 0.5) * 2.0;
            let s = sin(angle);
            let c = cos(angle);
            let along = vec2(dir.x * c - dir.y * s, dir.x * s + dir.y * c);
            // Waves come in groups — a few crests together, then a smooth
            // patch — so every octave is let through a noise of its own,
            // three or four wavelengths across and travelling with the water.
            let drift = at - stream * time;
            let groups = 0.45 + 1.1 * noise21(drift * (0.3 / wavelength) + f32(i) * 31.0);
            // And a crest is not a straight line across a lake. A phase
            // shifted by a noise a couple of wavelengths wide lets every
            // octave's crests wander, break and pick up again — without it,
            // ten straight wave trains cross into a woven cloth.
            let wander = noise21(drift * (0.36 / wavelength) + f32(i) * 7.3) * 6.2831853;
            // Deep-water dispersion — a long wave runs faster than a short one
            // — over a pattern that travels with the current.
            let omega = sqrt(9.81 * k);
            let phase = k * dot(along, at + bend - stream * time) - omega * time + wander;
            // Not a sine: water stands on sharp crests and lies in long
            // troughs, and the difference is what a photograph shows.
            let bulge = exp(1.4 * (sin(phase) - 1.0));
            let drawn = amp * groups;
            out.slope -= along * (drawn * seen * 1.9 * cos(phase) * bulge);
            out.crest += bulge * drawn / k;
            weight += drawn / k;
        }
        // Short waves ride on the long ones and are dragged about by them.
        // Reading each octave at a point the ones above it have pushed aside
        // is what turns a sum of sines into a sea instead of a corduroy.
        bend += out.slope * wavelength * 0.22;
        wavelength *= RATIO;
    }
    out.crest /= weight;
    return out;
}

/// The whole look, laid over a finished `PbrInput` before it is lit. `column`
/// is the water depth under the fragment and `edge` its distance from the
/// waterline, both in metres, both from the vertex colour; `uv` is where the
/// fragment lies in the body's own frame, in metres east and north of its
/// centre.
fn water_pbr(column: f32, edge: f32, uv: vec2<f32>, input: PbrInput) -> PbrInput {
    var out = input;
    let depth = max(column, 0.0);
    let shore = max(edge, 0.0);
    // The body's frame, turned to lie parallel to the render axes (+x east,
    // +z south) so a wind vector means the same thing in it.
    let here = vec2(uv.x, -uv.y);
    let xz = out.world_position.xz;
    let time = globals.time;
    let wind = length(water.weather.wind.xy);

    // How much ground one pixel covers here [m] — the finest the surface can
    // be drawn to before it turns into noise.
    let pixel = fwidth(here);
    let footprint = max(max(abs(pixel.x), abs(pixel.y)), 0.008);

    let stream = flow(out.world_normal, depth);
    let sea = waves(here, depth, shore, stream, footprint);

    // --- The surface -----------------------------------------------------------
    var slope = sea.slope;
    // Rain rings the surface where it lands, whatever the wind is doing.
    slope += ripple_slope(xz, time, water.weather.state.z);
    var normal = normalize(vec3(out.N.x + slope.x, out.N.y, out.N.z + slope.y));

    // --- The microsurface ------------------------------------------------------
    // Glassy when the wind is gone, rougher as it picks up, and rougher again
    // for every octave of waves too small to draw at this distance:
    // α² = α₀² + 2σ², the usual way a distribution of normals is folded into a
    // reflection lobe.
    let calm = mix(0.04, 0.13, clamp(wind / 14.0, 0.0, 1.0));
    let alpha = sqrt(calm * calm * calm * calm + 2.0 * sea.lost);
    var roughness = clamp(sqrt(alpha), 0.03, 0.5);

    // --- What comes up through it ----------------------------------------------
    // Two things happen to light in water: it is absorbed, and it is scattered
    // back out. Absorption is Beer's law on the way down and up, red first —
    // the attenuation the transmission applies over the column. Scatter is
    // what makes deep water a colour of its own instead of black: the diffuse
    // part of the material, which is what is left once the transmission has
    // taken its share.
    //
    // `clear` is how much of what lies under the surface still reaches the eye
    // unscattered: everything at the waterline, a sixth past five metres.
    let clear = exp(-depth * 0.35);
    // A bank is more than thin water. It is where the weed grows, where the
    // wave stirs the sand up and where what the water carries settles out —
    // turbid, and green-brown with it.
    let margin = (1.0 - smoothstep(1.0, 16.0, shore)) * (0.55 + 0.6 * noise21(here * 0.09));
    let transmission = mix(0.55, 0.94, clear) * (1.0 - margin * 0.3);
    // The albedo the scatter is worth: two to seven per cent, blue-green in
    // deep water, grey-green over a sandy shallow, brown at a weedy edge. Bevy
    // draws the diffuse part as `base × (1 − transmission)` and tints the
    // transmitted light with `base × transmission`, so the base colour is the
    // scatter divided back out — which leaves the bed seen through thin water
    // nearly untinted (the column's own tint is the attenuation), and through
    // deep water dimmed to almost nothing, as it is.
    let scatter = mix(
        mix(vec3(0.022, 0.058, 0.066), vec3(0.036, 0.050, 0.040), clear),
        vec3(0.062, 0.070, 0.038),
        margin * 0.7,
    );
    var color = min(scatter / max(1.0 - transmission, 0.06), vec3(1.0));
    // A wave is not lit like the flat it stands on: a crest is thin water with
    // the sky behind it, passing the light instead of stopping it, and a
    // trough lies in the shade of the crests around it.
    let lifted = clamp(length(sea.slope) * 2.5, 0.0, 1.0);
    color *= 1.0 + (sea.crest - 0.45) * 0.6 * lifted;
    color += vec3(0.030, 0.075, 0.058) * smoothstep(0.35, 1.0, sea.crest) * lifted;

    // --- The shore -------------------------------------------------------------
    // The band the water works: foam where a wave runs out. It breathes with
    // the swell rather than lying on the polygon's edge like a drawn line, it
    // comes in stretches — a bank foams where the wave runs in and stays quiet
    // in the lee — and two noises tear up what is left of it, because a
    // waterline is ragged.
    let surge = mix(0.5, 1.0, sea.crest);
    let reach = (0.4 + 2.6 * sea.energy) * surge;
    let stretch = smoothstep(0.3, 0.8, noise21(here * 0.02 + 13.0));
    let torn = noise21(here * 0.5 - water.weather.wind.xy * time * 0.05) * 0.6
        + noise21(here * 1.9) * 0.4;
    let run = 1.0 - smoothstep(0.0, reach, shore);
    var foam = clamp(run * run * (0.25 + stretch) * torn * 2.2, 0.0, 1.0)
        * smoothstep(0.15, 0.7, sea.energy);
    // Out on the open water a gale breaks the crests too. From about eight
    // metres a second the tops blow off and leave the streaks of spume that
    // are a storm's whole signature on a lake — patches drifting downwind,
    // and it is the patches rather than the crests they sit on that still
    // read from half a kilometre away.
    let spume = noise21(here * 0.11 - water.weather.wind.xy * time * 0.25) * 0.6
        + noise21(here * 0.6 - water.weather.wind.xy * time * 0.3) * 0.4;
    foam = max(
        foam,
        smoothstep(0.30, 0.68, spume * 0.6 + sea.crest * 0.4)
            * smoothstep(8.0, 16.0, wind),
    );
    color = mix(color, vec3(0.70, 0.745, 0.755), foam);
    roughness = mix(roughness, 0.62, foam);
    // Foam is bubbles, not water: it lies on the surface, flattens it, and
    // nothing shows through it.
    normal = normalize(mix(normal, vec3(0.0, 1.0, 0.0), foam * 0.7));

    out.N = normal;
    out.material.base_color = vec4(color, 1.0);
    out.material.perceptual_roughness = roughness;
    out.material.metallic = 0.0;
    out.material.reflectance = vec3(WATER_REFLECTANCE, WATER_REFLECTANCE, WATER_REFLECTANCE);
    out.material.specular_transmission = transmission * (1.0 - foam);
    out.material.diffuse_transmission = 0.0;
    out.material.ior = WATER_IOR;
    // The refracted ray is followed down through the column to where it meets
    // the bed, and the picture is read there — which is what a refraction of
    // a bed is. Capped, because past a couple of metres the column has taken
    // the bed out of the picture anyway, and a long ray only ever wanders off
    // the screen.
    out.material.thickness = clamp(depth, 0.03, 1.6);
    // Beer's law per metre: red goes first, green and blue last, and a lake
    // is the colour of what is left.
    out.material.attenuation_color = vec4(0.30, 0.64, 0.58, 1.0);
    out.material.attenuation_distance = 1.4;

    // The clouds shade the water like they shade everything else — and so does
    // the bank, where nothing better is known. Trees, reeds and the ground
    // behind them stand exactly where the water at the edge of a body would
    // otherwise mirror the bright horizon; the reflection rays find them
    // where they can, and this is what stands in where they cannot.
    let open = smoothstep(0.0, 11.0, shore);
    let shade = cloud_shade(xz, water.weather, time);
    out.diffuse_occlusion *= shade * mix(0.8, 1.0, open);
    out.specular_occlusion *= shade * mix(0.5, 1.0, open);
    return out;
}

/// Projects a point on a reflection ray into the screen. `xy` is where on the
/// viewport (0…1), `z` the depth the ray has there, `w` = 1 while the point is
/// in front of the camera and on the screen at all.
fn on_screen(p: vec3<f32>) -> vec4<f32> {
    let clip = view.clip_from_world * vec4(p, 1.0);
    if clip.w <= 0.01 {
        return vec4(0.0);
    }
    let ndc = clip.xyz / clip.w;
    if max(abs(ndc.x), abs(ndc.y)) > 1.0 {
        return vec4(0.0);
    }
    return vec4(ndc.xy * vec2(0.5, -0.5) + 0.5, ndc.z, 1.0);
}

#ifdef DEPTH_PREPASS
/// The depth the world was drawn with at a point of the viewport (0…1).
fn world_depth(uv: vec2<f32>) -> f32 {
    return prepass_depth(vec4(view.viewport.xy + uv * view.viewport.zw, 0.0, 0.0), 0u);
}

/// Whether a point on the ray has gone behind the world — and not so far
/// behind that it has passed through a pole or a tree and out the back: a
/// hit is a surface within a distance that grows with the ray, since a far
/// pixel is a large piece of world.
fn behind(screen: vec4<f32>, t: f32) -> bool {
    let scene = world_depth(screen.xy);
    if scene <= screen.z {
        return false;
    }
    let gap = depth_ndc_to_view_z(scene) - depth_ndc_to_view_z(screen.z);
    return gap < max(1.2, t * 0.14);
}
#endif

/// Where a reflection ray from `origin` along `dir` meets the world on the
/// screen: `xy` the viewport point to read the picture at, `w` how far to
/// believe it (0 = it met nothing, or nothing the screen can say).
fn trace(origin: vec3<f32>, dir: vec3<f32>, frag_coord: vec2<f32>) -> vec4<f32> {
#ifdef DEPTH_PREPASS
    // Each pixel starts its march a different fraction of a step in, so the
    // steps of neighbouring pixels do not line up into bands.
#ifdef TEMPORAL_JITTER
    let jitter = interleaved_gradient_noise(frag_coord, globals.frame_count);
#else
    let jitter = interleaved_gradient_noise(frag_coord, 0u);
#endif
    var step = 0.35;
    var t = step * (0.3 + jitter);
    var before = 0.0;
    var hit = -1.0;
    for (var i = 0u; i < MIRROR_STEPS; i++) {
        let screen = on_screen(origin + dir * t);
        if screen.w == 0.0 {
            // Off the screen: nothing more to be found there.
            return vec4(0.0);
        }
        if behind(screen, t) {
            hit = t;
            break;
        }
        before = t;
        step *= 1.32;
        t += step;
    }
    if hit < 0.0 {
        return vec4(0.0);
    }
    // The surface lies between the last free step and the first hit; halve
    // the interval a few times to land on it.
    var lo = before;
    var hi = hit;
    for (var i = 0u; i < MIRROR_REFINE; i++) {
        let mid = (lo + hi) * 0.5;
        let screen = on_screen(origin + dir * mid);
        if screen.w > 0.0 && behind(screen, mid) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    let screen = on_screen(origin + dir * hi);
    // Trust fades at the edge of the screen, where the reflection would be cut
    // off with a straight line, and for rays that come back towards the
    // camera, which only ever find the near side of things.
    let ndc = abs(screen.xy * 2.0 - 1.0);
    let edge = smoothstep(1.0, 0.8, max(ndc.x, ndc.y));
    let away = smoothstep(-0.25, 0.15, dot(dir, normalize(origin - view.world_position)));
    return vec4(screen.xy, 0.0, edge * away * screen.w);
#else
    return vec4(0.0);
#endif
}

/// The world's reflection in the surface, ready to add to the lit colour:
/// `rgb` the reflected picture weighted by fresnel and by how far it is to be
/// believed, `a` that belief — what the environment probe's specular is
/// stood down by.
fn mirror_world(input: PbrInput, frag_coord: vec2<f32>) -> vec4<f32> {
    // A rough surface's reflection is the blur the probe already is; the
    // march only pays off while the mirror is sharp.
    let sharp = 1.0 - smoothstep(0.18, 0.42, input.material.perceptual_roughness);
    // Foam is not a mirror.
    let glassy = input.material.specular_transmission;
    if sharp * glassy <= 0.002 {
        return vec4(0.0);
    }
    var dir = reflect(-input.V, input.N);
    // A wave's face can lean far enough that a grazing view reflects into the
    // water — which is the bed in the picture, and not what is wanted.
    dir.y = max(dir.y, 0.03);
    dir = normalize(dir);
    let found = trace(input.world_position.xyz, dir, frag_coord);
    if found.w <= 0.0 {
        return vec4(0.0);
    }
    let full = vec2<f32>(textureDimensions(view_transmission_texture));
    let at = (view.viewport.xy + found.xy * view.viewport.zw) / full;
    var picture = textureSampleLevel(view_transmission_texture, view_transmission_sampler, at, 0.0);
#ifdef TONEMAP_IN_SHADER
    picture = approximate_inverse_tone_mapping(picture, view.color_grading);
#endif
    // The same weight the probe's specular carries: Schlick's fresnel between
    // the water's 2 % and the grazing mirror.
    let cos_theta = clamp(dot(input.N, input.V), 0.0, 1.0);
    let fresnel = WATER_F0 + (1.0 - WATER_F0) * pow(1.0 - cos_theta, 5.0);
    let belief = found.w * sharp * clamp(glassy / 0.3, 0.0, 1.0);
    return vec4(picture.rgb * fresnel * belief, belief);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input = water_pbr(in.color.r, in.color.g, in.uv, pbr_input);

    // Where the world is found in the surface, the sky's probe steps back by
    // as much; the picture is already lit and exposed, so it goes in after
    // the lighting, on the same footing.
    let mirror = mirror_world(pbr_input, in.position.xy);
    pbr_input.specular_occlusion *= 1.0 - mirror.a;

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = vec4(out.color.rgb + mirror.rgb, out.color.a);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
