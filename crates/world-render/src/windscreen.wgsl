// Rain on the cab glass (plan 14.1). The pane keeps whatever the model made it —
// this adds the water: beads that sit, grow and are drunk back into the film,
// drops that break loose and run — down the glass at a stand, up and over it once
// the airflow beats gravity — each dragging a trail of beads it leaves behind,
// and the arc the wiper keeps clear around its own pivot, with the bulge of
// pushed water riding at the blade's edge.
//
// Everything works in the pane's metric frame: UVs times the pane size, u across
// (driver's left to right), v up. A drop is drawn as a spherical cap and what the
// eye gets is the cap's *slope*: the normal tilts into the PBR path, so the rim
// of every bead catches the sky's reflection and goes dark against it — the
// refraction ring — without ever sampling the scene.
//
// The wiper is not a texture and not a history: the blade's travel is a known
// function of the clock (`app::models::wiper_position`, mirrored here), so the
// moment any point of the swept arc was last crossed is *computed*, and the film
// regrows from exactly that moment, faster in heavier rain.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    mesh_view_bindings::globals,
}

struct Windscreen {
    // x = water on the pane 0…1, y = rain rate [mm/h], z = speed [m/s],
    // w = simulation time [s] — the clock the 3D blade is posed by.
    state: vec4<f32>,
    // x = wiper period [s], y = duty (the share of it the blade moves),
    // z = 1 while the wiper is engaged on this pane, w = film regrow time [s].
    wiper: vec4<f32>,
    // xy = the blade's pivot in pane UV, zw = pane size [m].
    geom: vec4<f32>,
    // x = blade rest angle [rad from the up axis, +u positive], y = sweep [rad],
    // z = inner radius [m], w = outer radius [m].
    blade: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> windscreen: Windscreen;

fn hash21(p: vec2<f32>) -> f32 {
    var h = fract(p.xyx * vec3(0.1031, 0.1030, 0.0973));
    h += dot(h, h.yzx + 33.33);
    return fract((h.x + h.y) * h.z);
}

/// A spherical cap of radius `r` at distance `d` from its centre: how much water
/// stands here (x) and how steep its surface is (y). The slope runs away at the
/// rim — that is the ring that makes a drop read as a lens — and is clamped only
/// where the maths would.
fn cap(d: f32, r: f32) -> vec2<f32> {
    if d >= r {
        return vec2(0.0);
    }
    let h = sqrt(r * r - d * d);
    return vec2(h / r, min(d / max(h, r * 0.18), 5.0));
}

/// Water as a height field: x = how much stands here (0…1), yz = the slope of
/// its surface in the pane's frame.
///
/// `flow` is signed: negative and the water creeps down the glass, positive and
/// the airflow drags it up; its magnitude is how fast. `wipe` is how recently the
/// blade was here, 1 = just now — a freshly wiped patch has no standing water
/// and regrows from beads.
fn beads(p: vec2<f32>, cell: f32, t: f32, density: f32, regrow: f32, seed: f32) -> vec3<f32> {
    let g = p / cell;
    let base = floor(g);
    var best = vec3(0.0);
    // Each drop sits *anywhere* in its cell, so the neighbours have to be
    // checked too — a drop confined to its own cell keeps a minimum distance
    // from every other, and a minimum distance is what reads as a grid.
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            let id = base + vec2(f32(dx), f32(dy));
            let rnd = hash21(id + seed);
            if rnd > density {
                continue;
            }
            // Each bead lives its own slow life: it pops in, sits, and is
            // drunk back into the glass - nothing on a windscreen is static.
            let life = 4.0 + 8.0 * hash21(id + seed + 31.0);
            let age = fract(t / life + rnd * 7.13);
            let envelope =
                smoothstep(0.0, 0.04, age) * (1.0 - smoothstep(0.55, 1.0, age)) * regrow;
            if envelope <= 0.0 {
                continue;
            }
            let centre = id + vec2(hash21(id + seed + 3.7), hash21(id + seed + 9.1));
            let local = (g - centre) * cell;
            // Real windscreen drops are millimetres: many small, the odd bigger
            // one. The size distribution is skewed by squaring the hash.
            let size = hash21(id + seed + 17.0);
            let r = cell * (0.12 + 0.24 * size * size) * envelope;
            let c = cap(length(local), r);
            if c.x > best.x {
                best = vec3(c.x, normalize(local + 1e-5) * c.y);
            }
        }
    }
    return best;
}

/// The running drops: one column per stripe of glass, a drop breaking loose and
/// crossing the pane once per period, wiggling as it goes and leaving a trail of
/// shrinking beads behind it.
fn running(p: vec2<f32>, t: f32, flow: f32, rate: f32, speed_f: f32) -> vec3<f32> {
    let cw = 0.055;
    let col = floor(p.x / cw);
    let seed = hash21(vec2(col, 4.2));
    // Heavier rain sheds drops sooner; a fast pane sheds them constantly.
    let period = clamp(7.0 / (0.4 + rate * 0.35) / (1.0 + speed_f * 2.5), 0.6, 12.0);
    let phase = fract(t / period + seed);
    // A drop gathers weight as it runs: it accelerates.
    let travelled = pow(phase, 1.4) * (windscreen.geom.w + 0.3);
    let y_head = select(windscreen.geom.w + 0.15 - travelled, travelled - 0.15, flow > 0.0);

    // The wiggle: a drop hunts for the path of least resistance, less so when
    // the airflow pins it straight.
    let amp = cw * 0.3 * (1.0 - 0.8 * speed_f);
    // The column's own resting place is random too, or the rivulets stand in rank.
    let x0 = (col + 0.25 + 0.5 * hash21(vec2(col, 12.5))) * cw;
    let x_at = x0 + sin(p.y * 9.0 + seed * 40.0) * amp;

    // The head: a running drop is a few millimetres of water, stretched along
    // the flow the faster the pane moves through the air.
    let rx = 0.0025 + 0.0022 * hash21(vec2(col, 8.8));
    let stretch = 1.5 + 3.0 * speed_f;
    let d_head = length((p - vec2(x_at, y_head)) * vec2(1.0, 1.0 / stretch));
    var water = vec3(0.0);
    let head = cap(d_head, rx);
    if head.x > 0.0 {
        let dir = normalize((p - vec2(x_at, y_head)) * vec2(1.0, 1.0 / stretch) + 1e-5);
        water = vec3(head.x, dir * head.y);
    }

    // The trail: beads pinched off behind the head, shrinking as they age.
    let step = max(rx * 4.0, 0.011);
    let k = floor(p.y / step);
    let y_bead = (k + 0.5) * step;
    let behind = (y_bead - y_head) * -sign(flow);
    let trail_len = 0.18 + 0.30 * speed_f + rate * 0.012;
    if behind > 0.0 && behind < min(trail_len, travelled) {
        let x_trail = x0 + sin(y_bead * 9.0 + seed * 40.0) * amp;
        let fade = 1.0 - behind / trail_len;
        let r = rx * (0.35 + 0.40 * fade) * (0.6 + 0.4 * hash21(vec2(col, k)));
        let local = p - vec2(x_trail, y_bead);
        let c = cap(length(local), r);
        if c.x * fade > water.x {
            water = vec3(c.x * fade, normalize(local + 1e-5) * c.y);
        }
    }
    return water;
}

/// The blade's travel 0…1 at time `t` — the same triangle sweep the 3D blade is
/// posed by (`wiper_position`): one full sweep in the first `duty` share of every
/// period, parked for the rest of it.
fn wiper_travel(t: f32, period: f32, duty: f32) -> f32 {
    let s = fract(t / period) / duty;
    if s >= 1.0 {
        return 0.0;
    }
    return 1.0 - abs(2.0 * s - 1.0);
}

/// Seconds since the blade last crossed the point of the arc whose travel value
/// is `x` — computed from the sweep function, not remembered.
fn since_crossed(t: f32, x: f32, period: f32, duty: f32) -> f32 {
    let window = duty * period;
    let up = x * window * 0.5;
    let down = window - up;
    let tc = fract(t / period) * period;
    if tc >= down {
        return tc - down;
    }
    if tc >= up {
        return tc - up;
    }
    // Not crossed yet this period — the last touch was the down-stroke before.
    return tc + period - down;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    let pane = windscreen.geom.zw;
    let p = in.uv * pane;
    let t = globals.time;
    let rate = windscreen.state.y;
    let film_level = windscreen.state.x;
    let speed = windscreen.state.z;

    // Which way the water goes, and how hard. Below walking pace it creeps down;
    // past about 15 m/s the airflow owns it and drags it up the raked glass.
    let speed_f = clamp(speed / 45.0, 0.0, 1.0);
    let flow = clamp((speed - 4.0) / 8.0, -1.0, 1.0);

    // --- The wiper: an arc around its real pivot ------------------------------
    // No bulge of pushed water at the blade's edge: twice built, twice it drew
    // as a translucent bar riding along — from the driver's seat the real thing
    // is a hair's width at the rubber, and nothing is more honest than nothing.
    var cleared = 0.0;
    if windscreen.wiper.z > 0.5 {
        let pivot = windscreen.geom.xy * pane;
        let q = p - pivot;
        let r = length(q);
        let phi = atan2(q.x, q.y);
        let travel_here = (phi - windscreen.blade.x) / windscreen.blade.y;
        let radial = smoothstep(windscreen.blade.z - 0.03, windscreen.blade.z, r)
            * (1.0 - smoothstep(windscreen.blade.w, windscreen.blade.w + 0.03, r));
        if travel_here > 0.0 && travel_here < 1.0 && radial > 0.0 {
            let sim_t = windscreen.state.w;
            let age = since_crossed(sim_t, travel_here, windscreen.wiper.x, windscreen.wiper.y);
            // The film closes over the swept arc again from exactly the moment
            // the rubber passed — quickly in a downpour, slowly in drizzle.
            cleared = max(1.0 - age / windscreen.wiper.w, 0.0) * radial;

        }
    }
    let keep = 1.0 - cleared;

    // --- The water -------------------------------------------------------------
    // Standing beads at two scales, and the drops that run. All of it scales
    // with what is falling, and all of it is wiped away and regrows.
    // Density rides on the rate alone (plus the film for what lingers): the
    // first drizzle of a front puts single drops on the glass, not a spray.
    let density = clamp(0.06 + rate * 0.22, 0.0, 0.95) * film_level;
    var water = beads(p, 0.0065, t, density, keep, 0.0);
    // The second layer runs in a frame turned by an awkward angle, so the two
    // lattices can never line up with each other or with the pane.
    let turn = mat2x2(0.7986, -0.6018, 0.6018, 0.7986);
    var big = beads(turn * p + 13.7, 0.015, t, density * 0.55, keep, 50.0);
    if big.x > water.x {
        // The slope came out in the turned frame; turn it back.
        water = vec3(big.x, transpose(turn) * big.yz);
    }
    if rate > 0.05 {
        let run = running(p, t, flow, rate, speed_f);
        if run.x * keep > water.x {
            water = vec3(run.x * keep, run.yz);
        }
    }
    let film = film_level * 0.5 * keep;

    // --- Onto the glass ----------------------------------------------------------
    // The glass itself stays what the model made it. Water shows through three
    // things only: the tilted normal (each bead catches the sky and the lights
    // its own way), the near-mirror smoothness where it stands, and the mist of
    // the film. No painted-on grey — that was the polka-dot look.
    let n = pbr_input.N;
    let across = normalize(cross(vec3(0.0, 1.0, 0.0), n) + 1e-4);
    let up = cross(n, across);
    let tilt = (across * water.y + up * water.z) * 0.8 * water.x;
    pbr_input.N = normalize(n + tilt);

    pbr_input.material.perceptual_roughness = mix(
        // A film mists the glass over; the wiped arc is the clear one.
        mix(pbr_input.material.perceptual_roughness, 0.45, film),
        0.03,
        min(water.x * 1.5, 1.0),
    );
    pbr_input.material.reflectance = mix(pbr_input.material.reflectance, vec3(0.35), water.x);

    // The rim of a drop bends the view furthest: it darkens and its coverage
    // rises — the refraction ring, faked from the slope alone.
    // A drop's core shows the refracted world — mostly the ground, upside down —
    // so against a bright sky it reads as a dark bead with a glint on it. No
    // scene to sample here, but raising the drop's own coverage does the same
    // job: the water surface replaces the background, its dark glass tint is the
    // core, and the environment reflection off the tilted normal is the glint.
    let rim = smoothstep(1.4, 3.6, length(vec2(water.y, water.z)));
    let coverage = clamp(film * 0.06 + water.x * 0.45 + rim * 0.25, 0.0, 0.85);
    pbr_input.material.base_color = vec4(
        pbr_input.material.base_color.rgb * (1.0 - rim * 0.3),
        clamp(pbr_input.material.base_color.a + coverage, 0.0, 1.0),
    );

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
