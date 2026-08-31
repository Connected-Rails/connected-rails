// The conductors of an overhead line: the wires between the masts.
//
// A conductor is the one thing in this world that is *always* too thin to draw.
// A 380 kV bundle is about 40 cm across and a 110 kV single conductor 2 cm; at
// the distance a power line is looked at — a kilometre and more, because the
// masts that carry it are landmarks — that is a fraction of a pixel. A fraction
// of a pixel is not a thin line. It is a line that hits some pixel centres and
// misses others, so it comes out as a crawling dotted seam that changes pattern
// every time the camera moves, and no amount of geometry fixes it: the
// rasteriser only ever answers yes or no.
//
// So the wire is not drawn as geometry of its true size. The mesh carries the
// **centre line** and nothing else (`content::power`), and this shader does two
// things with it:
//
//   1. **The vertex stage spreads it into a band facing the camera**, as wide
//      as the wire really is or a pixel and a half, whichever is more. Facing
//      the camera means it can never be seen edge-on — the reason the geometry
//      used to be a cross of two quads — and it costs half the triangles.
//   2. **The fragment stage gives back exactly what the widening took.** A band
//      drawn four times too wide is drawn at a quarter of the coverage, so the
//      *ink* on the screen stays what the wire is worth. That is the whole
//      trick: a 380 kV line two kilometres off becomes a grey hairline you can
//      lose against a bright sky — which is what it does in life — instead of a
//      black net drawn across it, and it never breaks into dashes on the way.
//
// Across the band the wire is shaded as the cylinder it is: the coverage
// follows the chord of a circle and the normal turns from the camera towards
// the edge, so a wire keeps a lit side and a dark side under a low sun.

#import bevy_pbr::{
    mesh_functions,
    mesh_view_bindings::{view, lights},
    view_transformations::position_world_to_clip,
}
// The fog binding and the function that reads it only exist where a camera
// carries `DistanceFog` — the simulator's does, the route editor's does not, and
// a shader that imports them unconditionally fails to compile in the editor.
#ifdef DISTANCE_FOG
#import bevy_pbr::{
    mesh_view_bindings::fog,
    mesh_view_types,
    pbr_functions::apply_fog,
}
#endif

struct Conductor {
    /// x = the least the wire may be drawn [px], y = ambient share of the
    /// shading, z and w free.
    params: vec4<f32>,
    /// The metal. `a` scales the coverage — a hand on the whole line.
    color: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> conductor: Conductor;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    /// A point on the wire's centre line.
    @location(0) position: vec3<f32>,
    /// x = which side of the centre line (−1 or +1), y = the wire's true
    /// half-width [m].
    @location(2) across: vec2<f32>,
    /// The wire's own direction here; `w` is unused. Location 4 because that
    /// is where Bevy's mesh pipeline puts `ATTRIBUTE_TANGENT` (position 0,
    /// normal 1, UV_0 2, UV_1 3, tangent 4) — the mesh carries no normal and no
    /// second UV, but the numbering is fixed, not packed.
    @location(4) tangent: vec4<f32>,
}

struct Output {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    /// x = where across the drawn band this is (−1 … 1), y = how much of the
    /// band the wire actually fills (0 … 1).
    @location(1) band: vec2<f32>,
    /// The band's own across direction, and the direction to the camera — the
    /// two the cylinder's normal is built from.
    @location(2) side: vec3<f32>,
    @location(3) to_camera: vec3<f32>,
}

@vertex
fn vertex(v: Vertex) -> Output {
    let world_from_local = mesh_functions::get_world_from_local(v.instance_index);
    let spine = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(v.position, 1.0),
    ).xyz;
    let along = normalize((world_from_local * vec4<f32>(v.tangent.xyz, 0.0)).xyz);

    let to_camera = view.world_position.xyz - spine;
    let distance = max(length(to_camera), 0.01);
    let to_camera_dir = to_camera / distance;

    // Across the wire and towards the camera.
    var side = cross(along, to_camera_dir);
    let side_length = length(side);
    if side_length < 1.0e-4 {
        // Looking straight down the wire. Any perpendicular will do — the whole
        // piece is a point on the screen either way.
        side = normalize(cross(along, vec3<f32>(0.0, 1.0, 0.0) + vec3<f32>(1.0e-3, 0.0, 0.0)));
    } else {
        side = side / side_length;
    }

    // What one pixel is worth in metres out here. The projection's own vertical
    // scale is `1 / tan(fov / 2)`, so this follows the field of view and the
    // window's height without either being passed in — a wire keeps its width
    // when the player changes the resolution, and the route editor's camera
    // gets the same treatment as the cab's.
    let metres_per_pixel = distance * 2.0 / (view.clip_from_view[1][1] * view.viewport.w);
    let half_true = v.across.y;
    let half = max(half_true, conductor.params.x * 0.5 * metres_per_pixel);

    var out: Output;
    let world = spine + side * v.across.x * half;
    out.clip_position = position_world_to_clip(world);
    out.world_position = world;
    out.band = vec2<f32>(v.across.x, half_true / half);
    out.side = side;
    out.to_camera = to_camera_dir;
    return out;
}

const PI: f32 = 3.14159265359;

@fragment
fn fragment(in: Output) -> @location(0) vec4<f32> {
    let across = in.band.x;
    // How much of the band is wire. One where the band is the wire's own size,
    // less wherever it had to be widened to stay visible.
    let fill = in.band.y;

    // A wire is round, so what a pixel sees of it across the band is the chord
    // of a circle: full in the middle, nothing at the edge. Without this a
    // widened wire is a hard-edged bar, which reads as a strip of tape.
    let profile = sqrt(max(0.0, 1.0 - across * across));

    // Give back what the widening took. The mean of the chord over the band is
    // π/4, so `4/π` puts the average coverage back at exactly `fill` — the
    // share of the band the metal really occupies.
    let alpha = clamp(fill * profile * (4.0 / PI), 0.0, 1.0) * conductor.color.a;
    if alpha < 0.002 {
        discard;
    }

    // The cylinder's normal: straight at the camera in the middle of the band,
    // turning out to the side at its edges.
    let normal = normalize(in.side * across + in.to_camera * max(profile, 1.0e-3));

    // Sun plus sky. A conductor has no texture and no microstructure worth
    // modelling — it is a dark rope — so this is a lambert term against the
    // first directional light and a sky/ground ambient, and nothing more. There
    // is no case where a pixel and a half of aluminium wants a BRDF.
    var sun = 0.0;
    if lights.n_directional_lights > 0u {
        sun = max(dot(normal, lights.directional_lights[0].direction_to_light), 0.0);
    }
    let sky = clamp(normal.y * 0.5 + 0.5, 0.0, 1.0);
    let ambient = conductor.params.y;
    let shade = ambient * mix(0.7, 1.15, sky) + (1.0 - ambient) * sun;

    var out = vec4<f32>(conductor.color.rgb * shade, alpha);
#ifdef DISTANCE_FOG
    if fog.mode != mesh_view_types::FOG_MODE_OFF {
        out = apply_fog(
            fog,
            out,
            in.world_position,
            view.world_position.xyz,
            in.clip_position.xy,
        );
    }
#endif
    return out;
}
