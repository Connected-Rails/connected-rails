// Terrain texture splatting (plan ch. 14): the fragment blends three ground
// textures by the per-vertex weights `content::terrain` computed — vertex
// color r = grass, g = rock, b = gravel. Everything else (lighting, fog,
// shadows) is the standard PBR path of the base material.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    mesh_view_bindings::globals,
}
#import world_render::weather::{Weather, weather_pbr}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var grass_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var grass_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var grass_normal_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var grass_normal_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var grass_arm_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(105) var grass_arm_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(106) var rock_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(107) var rock_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(108) var gravel_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(109) var gravel_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(110) var<uniform> weather: Weather;

struct GroundParams {
    season: vec4<f32>,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(111) var<uniform> ground: GroundParams;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    let w = in.color.rgb / max(dot(in.color.rgb, vec3(1.0)), 0.0001);

    // The scan is two metres wide. A second, quarter-turned lookup removes
    // the most recognisable clumps while retaining the scan's crisp detail;
    // both repeat an integer number of times across a 512 m tile, so their
    // blend remains seamless at streamed tile borders.
    let grass_uv = in.uv * 16.0;
    let grass_a = textureSample(grass_texture, grass_sampler, grass_uv).rgb;
    let grass_b = textureSample(
        grass_texture,
        grass_sampler,
        vec2(-grass_uv.y, grass_uv.x) + vec2(0.37, 0.61),
    ).rgb;
    let scanned_grass = mix(grass_a, grass_b, 0.12);
    let scan_luma = dot(scanned_grass, vec3(0.2126, 0.7152, 0.0722));
    // This layer represents the simulator's default living turf. Re-colour by
    // luminance in linear space so the photographed relief stays intact while
    // matching the brighter Central-European meadow palette of the simulation.
    let relief = clamp(0.55 + scan_luma * 2.2, 0.55, 1.38);
    var grass = vec3(0.055, 0.15, 0.025) * relief
        + scanned_grass * vec3(0.14, 0.08, 0.04);
    // Broad colour movement at 32 m stops the two-metre scan reading as a
    // stamp. Integer-period waves keep the value continuous across tiles.
    let macro_variation = sin(in.uv.x * 6.2831853) * sin(in.uv.y * 6.2831853);
    grass *= 0.96 + 0.07 * macro_variation;

    // Turn the scan with the scenario without corrupting its normal or ARM
    // data. Autumn keeps the luminance variation; snow leaves a trace of the
    // relief underneath instead of becoming a flat white sheet.
    let luma = dot(grass, vec3(0.2126, 0.7152, 0.0722));
    let autumn = vec3(luma) * vec3(1.18, 0.94, 0.42);
    grass = mix(grass, autumn, ground.season.y);
    grass = mix(grass, vec3(0.78, 0.82, 0.90) + (luma - 0.35) * 0.12, ground.season.x);

    // Different tilings per generated layer, so their repetition never
    // lines up with the scan or with one another.
    let rock = textureSample(rock_texture, rock_sampler, in.uv * 0.63).rgb;
    let gravel = textureSample(gravel_texture, gravel_sampler, in.uv * 1.37).rgb;
    pbr_input.material.base_color = vec4(grass * w.r + rock * w.g + gravel * w.b, 1.0);

    // PBR surface response from the scan. ARM is AO / roughness / metallic;
    // grass is dielectric, so its blue channel is intentionally unused.
    let arm = textureSample(grass_arm_texture, grass_arm_sampler, grass_uv).rgb;
    let grass_roughness = clamp(arm.g, 0.62, 0.98);
    pbr_input.material.perceptual_roughness =
        grass_roughness * w.r + 0.91 * w.g + 0.94 * w.b;
    pbr_input.diffuse_occlusion *= vec3(mix(1.0, arm.r, w.r * 0.72));

    // Derivative-built tangent frame: terrain vertices do not need to carry
    // tangents, and the normal remains correct on slopes and ENU-rotated
    // tiles. OpenGL (+Y) normals match Bevy's convention.
    var tangent_normal = textureSample(
        grass_normal_texture,
        grass_normal_sampler,
        grass_uv,
    ).xyz * 2.0 - 1.0;
    tangent_normal = normalize(vec3(tangent_normal.xy * 0.62, tangent_normal.z));
    let q1 = dpdx(in.world_position.xyz);
    let q2 = dpdy(in.world_position.xyz);
    let st1 = dpdx(grass_uv);
    let st2 = dpdy(grass_uv);
    let tangent = normalize(q1 * st2.y - q2 * st1.y);
    let bitangent = normalize(-q1 * st2.x + q2 * st1.x);
    let mapped_normal = normalize(
        tangent * tangent_normal.x
        + bitangent * tangent_normal.y
        + pbr_input.N * tangent_normal.z,
    );
    pbr_input.N = normalize(mix(pbr_input.N, mapped_normal, w.r * (1.0 - ground.season.x * 0.7)));
    // Rain, snow and the shadow of a cloud, the same way the objects get them.
    pbr_input = weather_pbr(weather, globals.time, pbr_input);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
