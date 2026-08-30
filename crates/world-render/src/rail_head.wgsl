// The rail surface: the running surface of the head polished to steel by the
// wheels, everything the wheel does not touch weathered to rust. One material
// over the whole section — the world normal picks the face: the head top
// catches the sun as one tight streak, flanks and foot stay matte.
//
// On top of that the inner side of the head — the faces looking across the
// gauge — is laid into shadow: at the scale of a rail head no shadow map
// resolves the head's own shade, so the shader bakes it on as a dark, matte
// steel. The lateral uv of the mesh tells gauge side from field side, the
// world normal keeps the running surface out of it. The shadow only pays off
// close to the camera, so it fades out over a few hundred metres — the far
// rails keep the flat look and the fragments keep the cheap one.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    mesh_view_bindings::view,
}

// Lateral uv where the gauge-side vertical faces end and the field-side ones
// begin. The faces sit at |uv.x| = axis − head/2 (gauge face), axis − web
// (inner web) and axis + web (outer web), with the 1:40 cant shearing |uv.x|
// by up to 4 mm — half gauge 0.7175 plus half a head width splits them with
// room to spare on both sides, for every rolled profile in the network.
const GAUGE_SIDE_END: f32 = 0.746;
const FIELD_SIDE_START: f32 = 0.760;

// The baked shadow is fully on this close to the camera and gone past that —
// beyond it the rail head is under an arc-minute wide and the flat rust
// carries the look, like the sleepers cull at 400 m.
const SHADOW_NEAR: f32 = 120.0;
const SHADOW_FAR: f32 = 350.0;

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

    // The baked shadow on the inner side of the rail top. Vertical face, on
    // the gauge side of the lateral uv, within reach of the camera: the head
    // shades its own gauge face — dark, matte steel without the sun streak.
    let lateral = abs(in.uv.x);
    let field_side = smoothstep(GAUGE_SIDE_END, FIELD_SIDE_START, lateral);
    let upright = smoothstep(0.45, 0.75, 1.0 - abs(normalize(pbr_input.world_normal).y));
    let reach = 1.0 - smoothstep(SHADOW_NEAR, SHADOW_FAR, distance(view.world_position, in.world_position.xyz));
    let shadow = (1.0 - field_side) * upright * reach;

    // Dark flange steel: less metal so the sky stops glancing off the face,
    // rough enough that the sun lays only a broad dim sheen on it.
    pbr_input.material.base_color =
        mix(pbr_input.material.base_color, vec4(0.085, 0.075, 0.07, 1.0), shadow);
    pbr_input.material.metallic = mix(pbr_input.material.metallic, 0.05, shadow);
    pbr_input.material.perceptual_roughness = mix(pbr_input.material.perceptual_roughness, 0.95, shadow);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
