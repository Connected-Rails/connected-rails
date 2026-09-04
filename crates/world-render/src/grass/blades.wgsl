// Grass blades: one indirect draw per level of detail over the instance list
// the scatter pass wrote. The vertex stage grows a blade out of its packed
// record — a quadratic Bézier bent by its own lean and the wind, tapered,
// never thinner than a pixel — and the fragment stage lights it the way the
// rest of the outdoor world is lit: Bevy's PBR with the sun's shadow, the
// atmosphere's fog and the weather's wet and snow.

#import bevy_pbr::{
    mesh_view_bindings::{view, globals},
    mesh_types::MESH_FLAGS_SHADOW_RECEIVER_BIT,
    view_transformations::position_world_to_clip,
    pbr_types,
    pbr_functions::{
        apply_pbr_lighting, main_pass_post_lighting_processing, prepare_world_normal,
        calculate_view,
    },
}
#import world_render::weather::{Weather, weather_pbr}

struct GrassUniform {
    frustum: array<vec4<f32>, 6>,
    camera: vec4<f32>,
    ground: vec4<f32>,
    grid: vec4<f32>,
    density: vec4<f32>,
    lods: vec4<f32>,
    look: vec4<f32>,
    season: vec4<f32>,
    capacity: vec4<u32>,
    weather: Weather,
}

struct Blade {
    pos: vec3<f32>,
    a: u32,
    b: u32,
    c: u32,
    d: u32,
    e: u32,
}

struct LodInfo {
    // x = segments along the blade, y = level, zw = unused.
    info: vec4<u32>,
}

@group(2) @binding(0) var<uniform> grass: GrassUniform;
@group(2) @binding(1) var<storage, read> blades: array<Blade>;
@group(2) @binding(2) var<uniform> lod: LodInfo;

const TAU: f32 = 6.28318530718;
const BLADE_MAX_HEIGHT: f32 = 0.6;
const BLADE_MAX_WIDTH: f32 = 0.1;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) world_normal: vec3<f32>,
    // x = along the blade 0…1, y = across it 0…1.
    @location(2) uv: vec2<f32>,
    @location(3) color: vec3<f32>,
    // x = perceptual roughness, y = ambient occlusion.
    @location(4) shade: vec2<f32>,
}

@vertex
fn vertex(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
    let blade = blades[instance_index];
    let a = unpack2x16unorm(blade.a);
    let b = unpack2x16unorm(blade.b);
    let c = unpack4x8unorm(blade.c);
    let nxz = unpack2x16snorm(blade.d);
    let e = unpack2x16unorm(blade.e);
    let facing_angle = a.x * TAU;
    let height = max(a.y * BLADE_MAX_HEIGHT, 0.01);
    let width = b.x * BLADE_MAX_WIDTH;
    let bend = b.y;
    let hue = c.x;
    let light = c.y;
    let dry = c.z;
    let stiffness = c.w;
    let clump = e.x;
    let phase = e.y;
    let ground_normal = normalize(vec3(nxz.x, sqrt(max(0.0, 1.0 - dot(nxz, nxz))), nxz.y));

    // 2N + 1 vertices: two per row and the tip.
    let segments = lod.info.x;
    var row = vertex_index / 2u;
    var side = f32(vertex_index & 1u);
    if vertex_index >= 2u * segments {
        row = segments;
        side = 0.5;
    }
    let t = f32(row) / f32(segments);

    let facing = vec3(cos(facing_angle), 0.0, sin(facing_angle));
    let lateral = vec3(-facing.z, 0.0, facing.x);
    // A blade grows up, and a little out of the slope it stands on.
    let up = normalize(mix(vec3(0.0, 1.0, 0.0), ground_normal, 0.35));
    let root = blade.pos;

    // Wind: a slow gust front travelling downwind, a finer ripple on it, and
    // each blade's own flutter. Stiff blades take less of all three.
    let wind = grass.weather.wind.xy;
    let speed = length(wind);
    var wind_dir = vec2(0.72, 0.69);
    if speed > 0.05 {
        wind_dir = wind / speed;
    }
    let time = globals.time;
    let front = dot(root.xz, wind_dir) * 0.12 - time * (0.9 + speed * 0.35);
    let gust = sin(front) * 0.6
        + sin(front * 2.3 + root.x * 0.7 + root.z * 0.4) * 0.3
        + sin(time * (2.1 + phase * 1.5) + phase * 12.0) * 0.15;
    let strength = 0.04 + 0.05 * min(speed, 20.0);
    let lean_wind = (0.35 + 0.65 * gust) * strength * (1.4 - stiffness);
    let wind_offset = vec3(wind_dir.x, 0.0, wind_dir.y) * lean_wind * height;

    // The curve: root, a control point at half height, and the tip leaning
    // out along the blade's facing and the wind. Bending shortens the rise.
    let lean = bend * height;
    let tip = root + up * height * (1.0 - 0.25 * bend * bend) + facing * lean + wind_offset;
    let mid = root + up * height * 0.55 + facing * lean * 0.22 + wind_offset * 0.28;
    let omt = 1.0 - t;
    var p = omt * omt * root + 2.0 * omt * t * mid + t * t * tip;
    let tangent = normalize(2.0 * omt * (mid - root) + 2.0 * t * (tip - mid));

    // Broad at the foot, pointed at the tip — and never thinner than about a
    // pixel and a half, or a far blade shimmers in and out of existence.
    var half_width = width * 0.5 * (1.0 - smoothstep(0.2, 1.0, t) * 0.92);
    let clip_centre = position_world_to_clip(p);
    let pixel = 2.0 * max(clip_centre.w, 0.01) / (view.clip_from_view[1][1] * view.viewport.w);
    half_width = max(half_width, 0.75 * pixel);
    p += lateral * (side - 0.5) * 2.0 * half_width;

    // Flat blades light flat. Turning the normal across the blade and a
    // little towards the sky reads as a rounded stalk and lets the sward
    // take skylight.
    var normal = normalize(cross(lateral, tangent));
    if dot(normal, facing) < 0.0 {
        normal = -normal;
    }
    normal = normalize(normal + lateral * (side - 0.5) * 0.7 + up * 0.15);

    // Colour: darker and browner at the foot, alive at the tip, every blade
    // its own shade, dry straws where the clump noise says. Far away the
    // blade-to-blade variation is faded out: at a pixel a blade it is not
    // variety but speckle, and the sward has to settle into one green.
    let far = smoothstep(15.0, 110.0, distance(root, view.world_position));
    let light_here = mix(light, 0.5, far * 0.8);
    let hue_here = mix(hue, 0.5, far * 0.7);
    var color = mix(vec3(0.050, 0.100, 0.021), vec3(0.150, 0.270, 0.056), pow(t, 0.8));
    color *= 0.82 + 0.36 * light_here;
    color *= mix(vec3(0.92, 1.0, 1.12), vec3(1.14, 1.0, 0.82), hue_here);
    color *= 0.88 + 0.24 * clump;
    let straw = vec3(0.30, 0.25, 0.09) * (0.6 + 0.6 * t);
    color = mix(color, straw, dry * (0.35 + 0.65 * t) * (1.0 - 0.5 * far));

    var out: VertexOutput;
    out.position = position_world_to_clip(p);
    out.world_position = vec4(p, 1.0);
    out.world_normal = normal;
    out.uv = vec2(t, side);
    out.color = color;
    out.shade = vec2(mix(0.78, 0.6, t), mix(0.30, 1.0, pow(t, 0.7)));
    return out;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> @location(0) vec4<f32> {
    var pbr_input = pbr_types::pbr_input_new();
    pbr_input.flags = MESH_FLAGS_SHADOW_RECEIVER_BIT;
    pbr_input.material.flags = pbr_types::STANDARD_MATERIAL_FLAGS_FOG_ENABLED_BIT
        | pbr_types::STANDARD_MATERIAL_FLAGS_DOUBLE_SIDED_BIT
        | pbr_types::STANDARD_MATERIAL_FLAGS_ALPHA_MODE_OPAQUE;
    pbr_input.material.base_color = vec4(in.color, 1.0);
    pbr_input.material.perceptual_roughness = in.shade.x;
    pbr_input.material.reflectance = vec3(0.3);
    // A leaf lit from behind glows: diffuse transmission through a thin
    // blade, which is most of what makes a meadow against the sun.
    pbr_input.material.diffuse_transmission = 0.3;
    pbr_input.material.thickness = 0.01;
    pbr_input.frag_coord = in.position;
    pbr_input.world_position = in.world_position;
    pbr_input.is_orthographic = view.clip_from_view[3].w == 1.0;
    pbr_input.V = calculate_view(in.world_position, pbr_input.is_orthographic);
    pbr_input.world_normal = prepare_world_normal(in.world_normal, true, is_front);
    pbr_input.N = normalize(pbr_input.world_normal);
    // The foot of a blade stands in the shade of the whole sward.
    pbr_input.diffuse_occlusion = vec3(in.shade.y);
    pbr_input = weather_pbr(grass.weather, globals.time, pbr_input);

    var color = apply_pbr_lighting(pbr_input);
    color = main_pass_post_lighting_processing(pbr_input, color);
    return color;
}
