// The rail surface. A rail is two materials on one piece of steel: the band
// the wheel treads ride is burnished to a mirror, and everything the wheel
// never touches — the head's flanks, the web, the foot — rusts. Where the
// line between them runs is a fact about the rolled section, not something a
// shader should infer from a normal, so `rail.rs` puts it in the mesh:
//
//   uv0 = (metres along the rail, depth under the running surface)
//   uv1 = (polish 0…1, gauge-side head flank 0…1)
//
// On top of that the gauge flank is laid into shadow. At the scale of a rail
// head no shadow map resolves the shade the head casts on its own inner face,
// and that shade is most of what makes a rail read as a solid section rather
// than a painted stripe. It fades out with distance, because past a few
// hundred metres the flat rust carries the look and the fragments are better
// spent elsewhere.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    mesh_view_bindings::view,
}

// Burnished steel: what a running surface reflects in. Nearly white, because
// what it mostly shows is the sky.
const STEEL: vec3<f32> = vec3(0.78, 0.78, 0.77);
// Weathered rail steel — the brown-orange of a few weeks of rain, dark enough
// that the noon sun reads it as a shaded flank and not as a glowing band.
const RUST: vec3<f32> = vec3(0.135, 0.088, 0.058);
// Brake dust and oil settle on the web and the foot: the lower the section,
// the dirtier and the flatter.
const GRIME: vec3<f32> = vec3(0.055, 0.048, 0.042);

// The baked flank shadow is fully on this close to the camera and gone past
// that — beyond it the rail head is under an arc-minute wide.
const SHADOW_NEAR: f32 = 140.0;
const SHADOW_FAR: f32 = 360.0;

// Depth under the running surface where the head ends and the web begins [m].
// Below it the section is in the shade of its own head all day.
const HEAD_DEPTH: f32 = 0.05;

// Cheap hash for the length-wise weathering — rail does not rust evenly, and
// an evenly rusted rail is a stripe again.
fn hash(x: f32) -> f32 {
    return fract(sin(x * 12.9898) * 43758.5453);
}

fn noise(x: f32) -> f32 {
    let cell = floor(x);
    let t = fract(x);
    let ease = t * t * (3.0 - 2.0 * t);
    return mix(hash(cell), hash(cell + 1.0), ease);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    let along = in.uv.x;
    let depth = in.uv.y;
    let polish = clamp(in.uv_b.x, 0.0, 1.0);
    let flank = clamp(in.uv_b.y, 0.0, 1.0);

    // Rust over grime: the web and the foot sit under the head all day and
    // catch everything the brakes throw off, so they are darker and flatter
    // than the head's own flanks.
    let low = smoothstep(0.02, HEAD_DEPTH, depth);
    var albedo = mix(RUST, GRIME, low * 0.75);
    // Two scales of weathering along the rail, a few metres and a few tens of
    // metres — patchy rust, not a gradient.
    let weather = 0.78 + 0.28 * noise(along * 0.42) + 0.16 * noise(along * 2.3);
    albedo *= weather;

    // The running band. A burnished head is one of the smoothest surfaces
    // outdoors: at 0.12 perceptual roughness the sun is a hard line on it and
    // the sky is a mirror, which is what the near band of a rail looks like
    // and what the old 0.3 could never give.
    pbr_input.material.base_color = vec4(mix(albedo, STEEL, polish), 1.0);
    pbr_input.material.metallic = mix(0.25, 1.0, polish);
    pbr_input.material.perceptual_roughness = mix(0.82, 0.12, polish * polish);

    // The head shading its own gauge face. Dark, matte steel: less metal so
    // the sky stops glancing off the face, rough enough that the sun lays
    // only a broad dim sheen on it.
    let reach = 1.0 - smoothstep(
        SHADOW_NEAR,
        SHADOW_FAR,
        distance(view.world_position, in.world_position.xyz)
    );
    let shade = flank * reach;
    pbr_input.material.base_color =
        mix(pbr_input.material.base_color, vec4(0.075, 0.065, 0.058, 1.0), shade);
    pbr_input.material.metallic = mix(pbr_input.material.metallic, 0.05, shade);
    pbr_input.material.perceptual_roughness =
        mix(pbr_input.material.perceptual_roughness, 0.95, shade);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
