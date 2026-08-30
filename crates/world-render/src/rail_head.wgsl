// The rail surface: the running surface of the head polished to steel by the
// wheels, everything the wheel does not touch weathered to rust. One material
// over the whole section — the world normal picks the face: the head top
// catches the sun as one tight streak, flanks and foot stay matte.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    // Render space is y-up. The blend rides the head's rounded shoulder: a
    // face more than about 25° from upright already reads as side, not as
    // running surface.
    let head = smoothstep(0.55, 0.9, normalize(pbr_input.world_normal).y);

    // Polished steel over weathered rust — with a metallic surface the albedo
    // is the steel tint the streak reflects in. The rust sits dark and greyed
    // so the noon sun reads it as shadowed steel, not as a glowing band.
    pbr_input.material.base_color =
        mix(vec4(0.15, 0.11, 0.08, 1.0), vec4(0.75, 0.73, 0.70, 1.0), head);
    pbr_input.material.metallic = mix(0.1, 0.95, head);
    // Perceptual roughness squares into the GGX exponent: 0.3² is the tight
    // wheel glaze of the head, 0.9² the matte rust of the section's sides.
    pbr_input.material.perceptual_roughness = mix(0.9, 0.3, head);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
