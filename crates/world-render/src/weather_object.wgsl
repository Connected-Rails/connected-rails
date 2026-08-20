// Everything in the world that is not terrain and not a vehicle: the standard PBR
// path, with the weather laid over the surface before it is lit (`weather.wgsl`).

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    mesh_view_bindings::globals,
}
#import world_render::weather::{Weather, weather_pbr}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> weather: Weather;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    // Leaves and grates are cut out by their alpha; that has to happen before the
    // weather touches the colour, or a discarded fragment would still be snowed on.
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);
    pbr_input = weather_pbr(weather, globals.time, pbr_input);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
