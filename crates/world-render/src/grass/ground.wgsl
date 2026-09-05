// The grass ground cache: the terrain and the surfaces draped on it, drawn
// top down into a texture of (height, grass weight). One draw per surface;
// `draws[instance_index]` carries its transform and whether it grows grass or
// cuts a hole.

struct GroundUniform {
    // xz = centre of the cache in render space, y = reference height,
    // w = half the side of the cache [m].
    centre: vec4<f32>,
    // x = half the height range around the reference [m], y = the sentinel
    // written where nothing is drawn, zw = unused.
    range: vec4<f32>,
}

struct GroundDraw {
    world_from_local: mat4x4<f32>,
    // x = 1 where the surface excludes grass.
    flags: vec4<u32>,
}

@group(0) @binding(0) var<uniform> ground: GroundUniform;
@group(0) @binding(1) var<storage, read> draws: array<GroundDraw>;

struct VertexIn {
    @builtin(instance_index) instance: u32,
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
}

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) data: vec2<f32>,
}

@vertex
fn vertex(in: VertexIn) -> VertexOut {
    let draw = draws[in.instance];
    let world = draw.world_from_local * vec4(in.position, 1.0);
    let half = ground.centre.w;
    let excluded = draw.flags.x != 0u;
    // Highest surface wins (compare Greater). A field or road draped on the
    // terrain lies within centimetres of it, so it is lifted a little here,
    // or the ground it covers could win the coplanar fight.
    let lift = select(0.0, 0.6, excluded);
    let depth = clamp(
        (world.y + lift - (ground.centre.y - ground.range.x)) / (2.0 * ground.range.x),
        0.0,
        1.0,
    );
    var out: VertexOut;
    out.clip = vec4(
        (world.x - ground.centre.x) / half,
        -(world.z - ground.centre.z) / half,
        depth,
        1.0,
    );
    // The terrain's splat weights sum to one across r, g, b; the grass share
    // is the same figure `terrain_splat.wgsl` blends the ground texture by.
    var mask = 0.0;
    if !excluded {
        mask = in.color.r / max(in.color.r + in.color.g + in.color.b, 1e-4);
    }
    out.data = vec2(world.y, mask);
    return out;
}

@fragment
fn fragment(in: VertexOut) -> @location(0) vec4<f32> {
    return vec4(in.data, 0.0, 0.0);
}
