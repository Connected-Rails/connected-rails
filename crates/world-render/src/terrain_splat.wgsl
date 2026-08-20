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
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var rock_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var rock_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var gravel_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(105) var gravel_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(106) var<uniform> weather: Weather;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    let w = in.color.rgb;
    // Different tilings per layer, so their repetition never lines up.
    let grass = textureSample(grass_texture, grass_sampler, in.uv).rgb;
    let rock = textureSample(rock_texture, rock_sampler, in.uv * 0.63).rgb;
    let gravel = textureSample(gravel_texture, gravel_sampler, in.uv * 1.37).rgb;
    pbr_input.material.base_color = vec4(grass * w.r + rock * w.g + gravel * w.b, 1.0);
    // Rain, snow and the shadow of a cloud, the same way the objects get them.
    pbr_input = weather_pbr(weather, globals.time, pbr_input);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
