// Rain and snow (plan 14.1). The field itself is a column of quads that rides on
// the camera and scrolls downwards (`app::update_precipitation`); this is what
// one of those quads looks like.
//
// What makes rain read as rain rather than as a 2010 overlay:
//
// * **A drop is a lens, not a light.** The streak carries the sky's own
//   luminance and is *blended*: against a bright sky it all but vanishes,
//   against a dark cutting it stands out, and looking towards the sun the whole
//   curtain glows — water forward-scatters hard (the same Henyey-Greenstein the
//   clouds use).
// * **Soft, thin, many.** The profile across a streak is a Gaussian, not an
//   edge; the far ones melt into the haze instead of staying crisp lines.
// * **Sheets, not a raster.** A slow noise over world space modulates how many
//   drops are alive, so the rain drifts past in swathes the way a gusty shower
//   does.
//
// Each particle carries three random numbers in its vertex colour: whether it
// falls at this intensity, how brightly it glints, and how long it draws.

#import bevy_pbr::{forward_io::VertexOutput, mesh_view_bindings::{view, globals}}

struct Precipitation {
    // x = intensity 0…1, y = 1 for snow and 0 for rain, z = opacity,
    // w = length of the streak within its quad, 0…1.
    state: vec4<f32>,
    // rgb = the light the drops carry, a = distance the near fade ends at [m].
    light: vec4<f32>,
    // xyz = direction towards the sun, w = daylight 0…1.
    sun: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> precipitation: Precipitation;

fn hash21(p: vec2<f32>) -> f32 {
    var h = fract(p.xyx * vec3(0.1031, 0.1030, 0.0973));
    h += dot(h, h.yzx + 33.33);
    return fract((h.x + h.y) * h.z);
}

/// Value noise in 0…1 — the swathes a shower falls in.
fn noise21(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash21(i);
    let b = hash21(i + vec2(1.0, 0.0));
    let c = hash21(i + vec2(0.0, 1.0));
    let d = hash21(i + vec2(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
#ifdef VERTEX_COLORS
    let particle = in.color.r;
    // Each drop catches the light its own way and draws its own length — a
    // field of identical streaks reads as a pattern, not as rain.
    let glint = mix(0.6, 1.5, in.color.g);
    let len_vary = mix(0.55, 1.0, in.color.b);
#else
    let particle = 0.0;
    let glint = 1.0;
    let len_vary = 1.0;
#endif
    // Sheets: rain does not fall evenly — a slow noise over the world, drifting
    // with time, decides how much of the field is alive *here*, so the shower
    // passes in swathes instead of standing as a raster.
    let world = in.world_position.xyz;
    let t = globals.time;
    let sheet = 0.55 + 0.9 * noise21(world.xz * 0.06 + vec2(t * 0.55, t * 0.23));
    // Thinning the field is how the intensity is drawn: the mesh is built once
    // for the heaviest rain, and everything lighter is the same mesh with fewer
    // of its drops alive.
    if particle > precipitation.state.x * sheet {
        discard;
    }

    // The nearest drops fade out as a *sphere* around the eye — there is no lens
    // here to blur a drop half a metre away into bokeh — and the far ones melt
    // into the haze instead of hanging as crisp lines at every distance.
    let dist = distance(world, view.world_position.xyz);
    let near_end = precipitation.light.a;
    let near_fade = smoothstep(near_end * 0.45, near_end, dist);
    let far_fade = 1.0 - smoothstep(9.0, 22.0, dist) * 0.75;
    if near_fade <= 0.0 {
        discard;
    }

    let uv = in.uv;
    // Clamped: multisampling evaluates the shader at sample positions that can
    // lie outside the triangle, and the maths below must not see a negative.
    let across = saturate(1.0 - abs(uv.x - 0.5) * 2.0);
    var shape: f32;
    if precipitation.state.y > 0.5 {
        // A flake: round, soft, and never quite opaque.
        let d = length((uv - 0.5) * 2.0);
        shape = 1.0 - smoothstep(0.1, 1.0, d);
    } else {
        // A streak: a Gaussian across its width — motion blur has no edges —
        // fading out at both ends, and only as long as its drop draws it.
        let length = precipitation.state.w * len_vary;
        let along = smoothstep(0.0, 0.2, uv.y) * (1.0 - smoothstep(length * 0.6, length, uv.y));
        let x = (uv.x - 0.5) * 2.0;
        shape = exp(-x * x * 3.5) * along * across;
    }
    if shape <= 0.01 {
        discard;
    }

    // Water forward-scatters hard: looking towards the sun, every drop is a lens
    // that lights up; looking away, the same curtain nearly disappears. The lobe
    // is soft — a drop tumbles — and it dies with the daylight.
    let view_dir = normalize(world - view.world_position.xyz);
    let towards = dot(view_dir, precipitation.sun.xyz);
    let phase = 0.55 + 1.5 * pow(saturate(towards * 0.5 + 0.5), 4.0) * precipitation.sun.w;

    let color = precipitation.light.rgb * view.exposure * phase * glint;
    let alpha = shape * precipitation.state.z * near_fade * far_fade;
    return vec4(color, alpha);
}
