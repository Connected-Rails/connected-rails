// Water (plan ch. 14): the lakes and rivers the line carries as
// `WaterSource` polygons, cut to the terrain tiles and handed out with them
// (see `content::water`). The mesh is only the surface, standing where the
// elevation data put it; everything that makes it read as water is here.
//
// Three ingredients, layered:
//
//   1. The body. The vertex colour carries the depth of the water column in
//      metres — what the shoreline level made of the bed. Shallow water is
//      murky and translucent, showing the ground beneath; deep water goes
//      dark and opaque. That is the shore reading as a shore, for free.
//   2. The waves. A handful of directional waves — wind-aligned, spread
//      around it, each running at its own deep-water phase speed — tilt the
//      normal, and they die out in the shallows. The PBR path does the rest:
//      the atmosphere's environment probe gives the sky its reflection, the
//      sun draws the glitter, the fresnel makes it all angle-dependent.
//   3. The weather. Rain rings its drops into the surface (the same
//      `ripple_slope` the wet ground uses), a cloud's shadow lies on it like
//      it lies on the fields, and a dying wind calms it towards glass.
//
// The time comes from the view's own globals, so nothing is uploaded per
// frame; the uniform only changes when the weather does.

#define_import_path world_render::water

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    pbr_types::PbrInput,
    mesh_view_bindings::globals,
}
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

// Seven waves over two decades of wavelength [m]: (wavelength, amplitude at a
// fresh breeze [m], angle off the wind [rad]). The short ones travel steeper
// angles — a real water surface is a spectrum, not a washboard, and it is the
// crossing of the short waves that makes a reflection break up.
const WAVES = array<vec3<f32>, 7>(
    vec3(31.0, 0.062,  0.00),
    vec3(17.0, 0.041,  0.55),
    vec3(9.0,  0.026, -0.80),
    vec3(5.0,  0.016,  0.35),
    vec3(2.8,  0.010, -0.45),
    vec3(1.5,  0.006,  0.95),
    vec3(0.8,  0.003, -1.10),
);

/// How the surface tilts at `xz` (world metres), from the waves.
///
/// Only the slope is wanted, not the height — the mesh is flat, and a tilted
/// normal is what moves a reflection. The amplitude grows with the wind and
/// dies in shallow water, so a pond stays a mirror and a shore stays a shore
/// whatever the weather is doing offshore.
fn wave_slope(xz: vec2<f32>, depth: f32) -> vec2<f32> {
    let wind = water.weather.wind.xy;
    let speed = length(wind);
    // No wind: no direction to spread around, and nothing to spread.
    if speed < 0.05 {
        return vec2(0.0);
    }
    let dir = wind / speed;
    // The wind's fetch — waves need room to grow — is folded into a saturating
    // power law: a breeze raises ripples quickly, a storm adds little beyond.
    let grown = pow(clamp(speed / 9.0, 0.0, 1.0), 1.4);
    let shore = smoothstep(0.0, 0.6, depth);
    let amp = grown * shore;
    if amp <= 0.001 {
        return vec2(0.0);
    }

    var slope = vec2(0.0);
    for (var i = 0u; i < 7u; i++) {
        let wave = WAVES[i];
        let wavenumber = 6.2831853 / wave.x;
        let s = sin(wave.z);
        let c = cos(wave.z);
        let k = vec2(dir.x * c - dir.y * s, dir.x * s + dir.y * c) * wavenumber;
        // Deep-water dispersion: long waves run faster, as they do on a lake.
        let phase_speed = sqrt(9.81 / wavenumber);
        let phase = dot(k, xz) - phase_speed * wavenumber * globals.time;
        slope += k * (wave.y * amp * cos(phase));
    }
    return slope;
}

/// The whole look, laid over a finished `PbrInput` before it is lit. `column`
/// is the water depth under the fragment, in metres, from the vertex colour.
fn water_pbr(column: f32, input: PbrInput) -> PbrInput {
    var out = input;
    let depth = max(column, 0.0);
    let xz = out.world_position.xz;

    // --- The body --------------------------------------------------------------
    // Murky green-brown in the shallows — the colour of a German river over
    // its sandy bed — going near-black with depth. `exp` is how light dies in
    // water; the rate is tuned so a metre-deep ditch still shows its bottom
    // and a lake past three metres reads as deep.
    var murky = vec3(0.040, 0.062, 0.048);
    let deep = vec3(0.003, 0.012, 0.015);
    let clear = 1.0 - exp(-depth * 0.30);
    // A ripple's crest catches the sky and comes back lighter — a large-scale
    // dapple keeps a wide surface from looking like one painted colour.
    let dapple = noise21(xz * 0.11) * 0.35 + noise21(xz * 0.43) * 0.15;
    murky = murky * (1.0 + dapple);
    let color = mix(murky, deep, clear);

    // The ground shows through only where the column is thin — the shore,
    // where a hard edge would read as a filled polygon. Deeper in, the water
    // is opaque: its reflection is, too, and a blend would dim the sky it
    // mirrors along with the body.
    let depth_factor = 1.0 - exp(-depth * 0.55);
    out.material.base_color = vec4(color, mix(0.82, 0.97, depth_factor));

    // --- The microsurface ------------------------------------------------------
    // Glassy when the wind is gone, rougher as it picks up: the micro-facets
    // a wave spectrum leaves behind blur the reflection into the horizon haze
    // every sailor knows.
    let wind = length(water.weather.wind.xy);
    out.material.perceptual_roughness = mix(0.055, 0.22, clamp(wind / 14.0, 0.0, 1.0));
    out.material.metallic = 0.0;
    out.material.reflectance = vec3(WATER_REFLECTANCE, WATER_REFLECTANCE, WATER_REFLECTANCE);

    // --- The surface -----------------------------------------------------------
    var slope = wave_slope(xz, depth);
    // Rain rings the surface where it lands, whatever the wind is doing.
    slope += ripple_slope(xz, globals.time, water.weather.state.z);
    out.N = normalize(vec3(out.N.x + slope.x, out.N.y, out.N.z + slope.y));

    // The clouds shade the water like they shade everything else.
    let shade = cloud_shade(xz, water.weather, globals.time);
    out.diffuse_occlusion *= shade;
    out.specular_occlusion *= shade;
    return out;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input = water_pbr(in.color.r, pbr_input);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
