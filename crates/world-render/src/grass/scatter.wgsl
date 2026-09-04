// Grass scatter: lays out the meadow's blades for this frame.
//
// One workgroup per 4 m patch of ground around the camera. A patch that is
// out of the frustum or off the ground cache costs one test; a patch in view
// walks its blade slots, and each slot is a blade whenever its rank is below
// the density its distance asks for. Survivors go into one of three instance
// lists, one per level of detail, and the instance counts land straight in
// the indirect draw arguments.

#import world_render::weather::Weather

struct GrassUniform {
    // World-space half spaces, normal xyz and distance w; the far plane is
    // left open because the range is the far limit.
    frustum: array<vec4<f32>, 6>,
    // xyz = camera position in render space.
    camera: vec4<f32>,
    // xy = ground cache centre xz, z = half side [m], w = metres per texel.
    ground: vec4<f32>,
    // x = patch side [m], y = patches per side, zw = first patch's grid index.
    grid: vec4<f32>,
    // x = blades/m² at the camera, y = falloff distance [m], z = range [m],
    // w = blade slots per patch.
    density: vec4<f32>,
    // x = end of the fine level [m], y = end of the middle level [m],
    // z = width of the range fade [m], w = 1 while enabled.
    lods: vec4<f32>,
    // x = stand height [m], yzw = unused.
    look: vec4<f32>,
    // x = snow 0…1, y = autumn 0…1, z = the ground cache's "nothing drawn"
    // sentinel, w = unused.
    season: vec4<f32>,
    // Instance capacity per level of detail.
    capacity: vec4<u32>,
    weather: Weather,
}

// 32 bytes a blade. Everything but the foot is packed to 16 or 8 bits.
struct Blade {
    pos: vec3<f32>,
    // facing angle (turns), height / BLADE_MAX_HEIGHT
    a: u32,
    // width / BLADE_MAX_WIDTH, bend
    b: u32,
    // hue, light, dry, stiffness
    c: u32,
    // ground normal xz, snorm
    d: u32,
    // clump, phase
    e: u32,
}

struct IndirectArgs {
    index_count: u32,
    instance_count: atomic<u32>,
    first_index: u32,
    base_vertex: i32,
    first_instance: u32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
}

@group(0) @binding(0) var<uniform> grass: GrassUniform;
@group(0) @binding(1) var ground: texture_2d<f32>;
@group(0) @binding(2) var<storage, read_write> blades: array<Blade>;
@group(0) @binding(3) var<storage, read_write> indirect: array<IndirectArgs, 3>;

const WORKGROUP: u32 = 64u;
const BLADE_MAX_HEIGHT: f32 = 0.6;
const BLADE_MAX_WIDTH: f32 = 0.1;
// Roberts' R2 sequence: any prefix of it is evenly spread, which is what
// lets a prefix be the thinned stand.
const R2: vec2<f32> = vec2(0.7548776662466927, 0.5698402909980532);

fn pcg(v: u32) -> u32 {
    let state = v * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

fn rand(seed: u32) -> f32 {
    return f32(pcg(seed)) / 4294967296.0;
}

fn hash2(p: vec2<i32>) -> f32 {
    return rand(bitcast<u32>(p.x) * 0x8DA6B343u ^ bitcast<u32>(p.y) * 0xD8163841u);
}

// Value noise 0…1 — the clumps of a meadow, patches of richer and drier
// grass a few metres across.
fn noise(p: vec2<f32>) -> f32 {
    let i = vec2<i32>(floor(p));
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash2(i);
    let b = hash2(i + vec2(1, 0));
    let c = hash2(i + vec2(0, 1));
    let d = hash2(i + vec2(1, 1));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// --- Ground cache ---------------------------------------------------------

fn texel_of(xz: vec2<f32>) -> vec2<f32> {
    let size = f32(textureDimensions(ground).x);
    return ((xz - grass.ground.xy) / (2.0 * grass.ground.z) + 0.5) * size - 0.5;
}

fn load(t: vec2<i32>) -> vec2<f32> {
    let dims = vec2<i32>(textureDimensions(ground));
    if any(t < vec2(0)) || any(t >= dims) {
        return vec2(grass.season.z, 0.0);
    }
    return textureLoad(ground, t, 0).xy;
}

fn written(sample: vec2<f32>) -> bool {
    return sample.x > grass.season.z * 0.5;
}

struct Ground {
    height: f32,
    mask: f32,
    valid: bool,
}

// Bilinear over the texels that were drawn; a texel nothing reached takes no
// part, so the edge of the drawn ground does not slope down to the sentinel.
fn sample_ground(xz: vec2<f32>) -> Ground {
    let t = texel_of(xz);
    let i = vec2<i32>(floor(t));
    let f = fract(t);
    let s00 = load(i);
    let s10 = load(i + vec2(1, 0));
    let s01 = load(i + vec2(0, 1));
    let s11 = load(i + vec2(1, 1));
    let w00 = select(0.0, (1.0 - f.x) * (1.0 - f.y), written(s00));
    let w10 = select(0.0, f.x * (1.0 - f.y), written(s10));
    let w01 = select(0.0, (1.0 - f.x) * f.y, written(s01));
    let w11 = select(0.0, f.x * f.y, written(s11));
    let sum = w00 + w10 + w01 + w11;
    var out: Ground;
    out.valid = sum > 1e-4;
    if !out.valid {
        return out;
    }
    let total = s00 * w00 + s10 * w10 + s01 * w01 + s11 * w11;
    out.height = total.x / sum;
    out.mask = total.y / sum;
    return out;
}

fn ground_normal(xz: vec2<f32>, height: f32) -> vec3<f32> {
    let step = grass.ground.w * 1.5;
    let px = sample_ground(xz + vec2(step, 0.0));
    let nx = sample_ground(xz - vec2(step, 0.0));
    let pz = sample_ground(xz + vec2(0.0, step));
    let nz = sample_ground(xz - vec2(0.0, step));
    let hx0 = select(height, nx.height, nx.valid);
    let hx1 = select(height, px.height, px.valid);
    let hz0 = select(height, nz.height, nz.valid);
    let hz1 = select(height, pz.height, pz.valid);
    return normalize(vec3(hx0 - hx1, 2.0 * step, hz0 - hz1));
}

// --- Culling and thinning -------------------------------------------------

fn density_at(d: f32) -> f32 {
    let q = d / grass.density.y;
    let falloff = grass.density.x / (1.0 + q * q);
    let fade = 1.0 - smoothstep(grass.density.z - grass.lods.z, grass.density.z, d);
    return falloff * fade;
}

fn sphere_visible(centre: vec3<f32>, radius: f32) -> bool {
    for (var i = 0u; i < 5u; i++) {
        let plane = grass.frustum[i];
        if dot(plane.xyz, centre) + plane.w < -radius {
            return false;
        }
    }
    return true;
}

fn box_visible(lo: vec3<f32>, hi: vec3<f32>) -> bool {
    for (var i = 0u; i < 5u; i++) {
        let plane = grass.frustum[i];
        let corner = select(lo, hi, plane.xyz > vec3(0.0));
        if dot(plane.xyz, corner) + plane.w < 0.0 {
            return false;
        }
    }
    return true;
}

@compute @workgroup_size(64)
fn scatter(
    @builtin(workgroup_id) workgroup: vec3<u32>,
    @builtin(local_invocation_index) local: u32,
) {
    if grass.lods.w < 0.5 {
        return;
    }
    let patch_side = grass.grid.x;
    let cell = grass.grid.zw + vec2<f32>(workgroup.xy);
    let origin = cell * patch_side;

    // The patch's height range, off five samples of the ground. A patch the
    // cache holds nothing of grows nothing.
    var lo = 1e9;
    var hi = -1e9;
    var any_valid = false;
    for (var k = 0u; k < 5u; k++) {
        var at = origin + vec2(0.5, 0.5) * patch_side;
        if k > 0u {
            let corner = vec2(f32(k & 1u), f32((k >> 1u) & 1u));
            at = origin + corner * patch_side;
        }
        let g = sample_ground(at);
        if g.valid {
            lo = min(lo, g.height);
            hi = max(hi, g.height);
            any_valid = true;
        }
    }
    if !any_valid {
        return;
    }
    let box_lo = vec3(origin.x, lo - 1.0, origin.y);
    let box_hi = vec3(origin.x + patch_side, hi + BLADE_MAX_HEIGHT + 1.0, origin.y + patch_side);
    if !box_visible(box_lo, box_hi) {
        return;
    }

    let camera = grass.camera.xyz;
    let nearest = clamp(camera, box_lo, box_hi);
    let d_min = distance(camera, nearest);
    let slots = grass.density.w;
    // Everything a blade of this patch can be is within the first `count`
    // slots: a blade's own distance is at least the patch's nearest point.
    let keep_patch = density_at(d_min) / grass.density.x;
    let count = u32(ceil(keep_patch * slots));
    if count == 0u {
        return;
    }

    let patch_seed = pcg(
        bitcast<u32>(i32(cell.x)) * 0x9E3779B9u ^ bitcast<u32>(i32(cell.y)) * 0x85EBCA6Bu,
    );
    let offset = vec2(rand(patch_seed), rand(patch_seed ^ 0x68E31DA4u));
    let spacing = patch_side / sqrt(slots);

    for (var s = local; s < count; s += WORKGROUP) {
        let rank = (f32(s) + 0.5) / slots;
        let seed = pcg(patch_seed ^ (s * 0x2545F491u + 0x1B56C4E9u));
        // The R2 point, jittered by most of a spacing: the sequence alone has
        // a lattice in it that a meadow does not.
        let r2 = fract(offset + f32(s) * R2);
        let jitter = (vec2(rand(seed), rand(seed ^ 0x1u)) - 0.5) * spacing * 0.9;
        let xz = origin + r2 * patch_side + jitter;
        let g = sample_ground(xz);
        if !g.valid {
            continue;
        }
        let pos = vec3(xz.x, g.height, xz.y);
        let d = distance(camera, pos);
        let keep = density_at(d) / grass.density.x;
        if rank >= keep {
            continue;
        }
        // The splat's edge, dithered: a verge thins into the gravel rather
        // than stopping at a line.
        if g.mask < 0.15 + 0.55 * rand(seed ^ 0x2u) {
            continue;
        }

        let clump = noise(xz * 0.35 + 3.7);
        let base = grass.look.x;
        var height = base * (0.55 + 0.9 * rand(seed ^ 0x3u)) * (0.75 + 0.5 * clump);
        // A blade at the thinning threshold grows in instead of popping.
        let edge = clamp((keep - rank) / (0.2 * keep), 0.0, 1.0);
        height *= 0.55 + 0.45 * edge;
        // The stand keeps its cover as it thins: what the missing blades
        // hid, the remaining ones grow wide enough to hide.
        let widen = clamp(inverseSqrt(max(keep, 1e-3)), 1.0, 3.5);
        let width = (0.011 + 0.010 * rand(seed ^ 0x4u)) * (0.6 + 0.4 * height / base) * widen;
        let facing = rand(seed ^ 0x5u);
        let bend = 0.12 + 0.6 * rand(seed ^ 0x6u);
        let hue = rand(seed ^ 0x7u);
        let light = rand(seed ^ 0x8u);
        let dry = clamp(noise(xz * 0.11 + 11.3) * 1.2 - 0.62 + grass.season.y * 0.7, 0.0, 1.0);
        let stiffness = 0.4 + 0.6 * rand(seed ^ 0x9u);

        if !sphere_visible(pos + vec3(0.0, height * 0.5, 0.0), height * 0.8 + 0.1) {
            continue;
        }

        var lod = 2u;
        if d < grass.lods.x {
            lod = 0u;
        } else if d < grass.lods.y {
            lod = 1u;
        }
        let slot = atomicAdd(&indirect[lod].instance_count, 1u);
        if slot >= grass.capacity[lod] {
            continue;
        }
        var first = 0u;
        if lod >= 1u {
            first += grass.capacity.x;
        }
        if lod >= 2u {
            first += grass.capacity.y;
        }

        let normal = ground_normal(xz, g.height);
        var blade: Blade;
        blade.pos = pos;
        blade.a = pack2x16unorm(vec2(facing, clamp(height / BLADE_MAX_HEIGHT, 0.0, 1.0)));
        blade.b = pack2x16unorm(vec2(clamp(width / BLADE_MAX_WIDTH, 0.0, 1.0), bend));
        blade.c = pack4x8unorm(vec4(hue, light, dry, stiffness));
        blade.d = pack2x16snorm(normal.xz);
        blade.e = pack2x16unorm(vec2(clump, rand(seed ^ 0xAu)));
        blades[first + slot] = blade;
    }
}

// The counters run past the capacity when a list overflows; the draw must
// not.
@compute @workgroup_size(1)
fn finish() {
    for (var lod = 0u; lod < 3u; lod++) {
        atomicMin(&indirect[lod].instance_count, grass.capacity[lod]);
    }
}
