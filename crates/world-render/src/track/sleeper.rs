//! Sleepers and what holds the rails down to them.
//!
//! A sleeper is not a brick. A B 70 is a prestressed beam carried at its two
//! rail seats: 214 mm deep under the seats and 175 mm in the middle, 300 mm
//! wide at the base and 220 mm on top because it comes out of a mould, with
//! chamfered top edges. All four of those are what tells cast concrete from a
//! painted stripe at cab distance, and the taper along the length is what
//! tells a sleeper from an extrusion. A timber sleeper is the other case and
//! is genuinely a beam: 2600 × 260 × 160 mm, sawn square.
//!
//! On top of that come the **fastenings**. They are small — a guide plate is
//! 50 mm wide — but they are the only thing that gives a sleeper a top side,
//! and without them track reads as a ladder. They are their own chunks with
//! their own, much shorter cull distance, because past a hundred metres a
//! clip is a fraction of a pixel and the sleeper's silhouette carries it.
//!
//! **Multiplayer.** Nothing here is state. The wobble that keeps a row of
//! sleepers from looking machined is hashed from the edge and the sleeper's
//! index, so every client lays the same track and a chunk rebuilt after the
//! camera came back is the chunk that was there before.

use bevy::prelude::{Mesh, Vec3};
use glam::DVec3;
use track_model::{Fastening, Oberbau, RAIL_CANT, SleeperKind, TrackEdge};
use world_coords::EnuFrame;

use super::ballast::mid_section;
use super::cross_section;
use super::mesh::{MeshBuilder, hash01, to_render};

/// Sleepers merged into one mesh — the culling granularity of the near band.
const SLEEPERS_PER_CHUNK: usize = 96;
/// Fastenings are the finer geometry and are culled closer, so they are
/// chunked finer too.
const FASTENINGS_PER_CHUNK: usize = 48;

/// Where a concrete sleeper's cross-section is taken along its own length, as
/// a fraction of the half length, with how much of the seat depth and of the
/// base width it has there. The rail seat of a 2.6 m sleeper sits 0.75 m from
/// the middle, inside the last run — so the seat is on the full section, the
/// way the drawing has it.
const STATIONS: [(f64, f64, f64); 4] = [
    (0.00, 0.0, 0.94),
    (0.34, 0.0, 0.94),
    (0.52, 1.0, 1.00),
    (1.00, 1.0, 1.00),
];

/// The chamfer along a concrete sleeper's top edges \[m\]. It is 15 mm on a
/// B 70 and it is what puts a line of light along every sleeper.
const CONCRETE_CHAMFER: f64 = 0.015;
/// Timber is sawn, not cast — only the arris is knocked off.
const TIMBER_CHAMFER: f64 = 0.006;

/// What [`build`] hands back: the sleeper solids and, separately, the
/// fastenings that stand on them. They are culled at different distances, so
/// they cannot share a mesh.
pub(super) struct SleeperChunks {
    pub(super) sleepers: Vec<(Mesh, Vec3)>,
    pub(super) fastenings: Vec<(Mesh, Vec3)>,
}

/// The sleepers of a type run, merged into chunks. Each mesh is built around
/// its own centre and comes with that centre in render axes, so the entity
/// can sit there and the distance cull measures to the chunk instead of to
/// the edge anchor.
pub(super) fn build(
    e: &TrackEdge,
    frame: &EnuFrame,
    s0: f64,
    s1: f64,
    ob: &Oberbau,
) -> SleeperChunks {
    let spacing = ob.sleeper_spacing.max(0.01);
    // A slab has no sleepers, but it does have fastenings: on the Feste
    // Fahrbahn they are bolted straight through the slab, and they are all
    // there is to see on it.
    let solid = ob.sleeper != SleeperKind::Slab;
    let fastening = ob.fastening();
    let count = (((s1 - s0) / spacing).floor() as usize) + 1;

    let mut chunks = SleeperChunks {
        sleepers: Vec::new(),
        fastenings: Vec::new(),
    };
    let mut sleepers = Batch::new(SLEEPERS_PER_CHUNK, s0);
    let mut clips = Batch::new(FASTENINGS_PER_CHUNK, s0);

    for k in 0..count {
        let s = s0 + k as f64 * spacing;
        if s > s1 {
            break;
        }
        let placed = place(
            cross_section(e, frame, s),
            ob,
            e.id.index() as u64,
            k as u64,
        );
        if solid {
            push_sleeper(&mut sleepers.mesh, &placed, ob);
            if let Some(done) = sleepers.finish_if_full(e, frame, s, spacing) {
                chunks.sleepers.push(done);
            }
        }
        if fastening.is_some() {
            push_fastenings(&mut clips.mesh, &placed, ob, fastening);
            if let Some(done) = clips.finish_if_full(e, frame, s, spacing) {
                chunks.fastenings.push(done);
            }
        }
    }
    chunks.sleepers.extend(sleepers.finish(e, frame));
    chunks.fastenings.extend(clips.finish(e, frame));
    chunks
}

/// A run of sleepers being merged into one mesh, with the arc lengths it
/// spans so the finished chunk can be hung on its own middle.
struct Batch {
    mesh: MeshBuilder,
    per_chunk: usize,
    placed: usize,
    first: f64,
    last: f64,
}

impl Batch {
    fn new(per_chunk: usize, s0: f64) -> Self {
        Self {
            mesh: MeshBuilder::new(false),
            per_chunk,
            placed: 0,
            first: s0,
            last: s0,
        }
    }

    /// Closes the chunk once it is full, and starts the next one after `s`.
    fn finish_if_full(
        &mut self,
        e: &TrackEdge,
        frame: &EnuFrame,
        s: f64,
        spacing: f64,
    ) -> Option<(Mesh, Vec3)> {
        self.placed += 1;
        self.last = s;
        if self.placed < self.per_chunk {
            return None;
        }
        let done = std::mem::replace(&mut self.mesh, MeshBuilder::new(false))
            .build(mid_section(e, frame, self.first, self.last));
        self.placed = 0;
        self.first = s + spacing;
        Some(done)
    }

    fn finish(self, e: &TrackEdge, frame: &EnuFrame) -> Option<(Mesh, Vec3)> {
        if self.mesh.is_empty() {
            return None;
        }
        let centre = mid_section(e, frame, self.first, self.last);
        Some(self.mesh.build(centre))
    }
}

/// One sleeper's own frame: where its top face sits and which way its axes
/// run, with the wobble of a laid track already in them. `(lengthwise,
/// crosswise, up)` is right-handed, which is what [`MeshBuilder::cuboid`]
/// wants of the axes it is handed.
struct Placed {
    /// Centre of the sleeper's top face.
    top: DVec3,
    /// Along the sleeper — across the track.
    lengthwise: DVec3,
    /// Across the sleeper — along the track.
    crosswise: DVec3,
    /// Up out of the sleeper's top face.
    up: DVec3,
    /// Where in its texture this sleeper reads. Without it every sleeper of a
    /// line samples the same patch of the same image and a hundred of them in
    /// a row are visibly one stamp repeated.
    uv_offset: [f32; 2],
}

impl Placed {
    /// A point of the sleeper: `t` along its length, `c` across it, `y` below
    /// its top face (negative is above it, where the fastenings are).
    fn at(&self, t: f64, c: f64, y: f64) -> DVec3 {
        self.top + self.lengthwise * t + self.crosswise * c - self.up * y
    }
}

/// Puts one sleeper into the track's cross-section — and nudges it.
///
/// Track is laid by a machine and then tamped, and no two sleepers end up
/// exactly square to the rails or exactly level. A centimetre of skew and a
/// few millimetres of depth is the whole difference between a row of sleepers
/// and a comb. It is hashed from the edge and the sleeper's index, so it is
/// the same on every machine.
fn place(
    (center, right, tangent, up): (DVec3, DVec3, DVec3, DVec3),
    ob: &Oberbau,
    edge: u64,
    index: u64,
) -> Placed {
    let noise = |salt: u64| hash01(edge.wrapping_mul(0x9E37).wrapping_add(index), salt) * 2.0 - 1.0;
    let (sin, cos) = (noise(1) * 0.007).sin_cos();
    Placed {
        top: center + tangent * (noise(2) * 0.012) + right * (noise(3) * 0.018)
            - up * (ob.sleeper_top() + noise(4).abs() * 0.005),
        lengthwise: (right * cos + tangent * sin).normalize(),
        crosswise: (tangent * cos - right * sin).normalize(),
        up,
        uv_offset: [noise(5) as f32, noise(6) as f32],
    }
}

/// The sleeper's cross-section at one station: `(across, below the top)`,
/// ordered so that quads bridged from station to station face outwards.
fn section(base_half: f64, top_half: f64, height: f64, chamfer: f64) -> [(f64, f64); 6] {
    let chamfer = chamfer.min(top_half * 0.4).min(height * 0.4);
    [
        (-top_half + chamfer, 0.0),
        (top_half - chamfer, 0.0),
        (top_half, chamfer),
        (base_half, height),
        (-base_half, height),
        (-top_half, chamfer),
    ]
}

/// Extrudes one sleeper along its own length, station by station, and caps
/// both ends.
///
/// The texture wraps isotropically: one repeat spans the sleeper's length
/// along `u`, and `v` follows the true distance around the section at the
/// same scale — so a grain running along `u` in the image runs along the
/// sleeper in the world, and it does not stretch when it turns onto a flank.
fn push_sleeper(mesh: &mut MeshBuilder, placed: &Placed, ob: &Oberbau) {
    let half_len = ob.sleeper_length / 2.0;
    let cast = ob.sleeper == SleeperKind::Concrete;
    let chamfer = if cast {
        CONCRETE_CHAMFER
    } else {
        TIMBER_CHAMFER
    };
    let (seat_h, mid_h) = (ob.sleeper_height, ob.mid_height());
    let (base, top) = (ob.sleeper_width / 2.0, ob.top_width() / 2.0);

    let at = |f: f64, height_factor: f64, width_factor: f64| {
        // Only a cast sleeper is drawn in at the waist; a sawn beam is a
        // beam end to end.
        let width = if cast { width_factor } else { 1.0 };
        (
            f * half_len,
            section(
                base * width,
                top * width,
                mid_h + (seat_h - mid_h) * height_factor,
                chamfer,
            ),
        )
    };
    // End to end: the mirrored half, then the half itself.
    let mut stations: Vec<(f64, [(f64, f64); 6])> = STATIONS
        .iter()
        .rev()
        .map(|&(f, h, w)| at(-f, h, w))
        .collect();
    stations.extend(STATIONS.iter().skip(1).map(|&(f, h, w)| at(f, h, w)));

    let scale = ob.texture_scale().max(0.05);
    let (du, dv) = (placed.uv_offset[0], placed.uv_offset[1]);
    for pair in stations.windows(2) {
        let (t0, a) = pair[0];
        let (t1, b) = pair[1];
        let (u0, u1) = (
            du + ((t0 + half_len) / scale) as f32,
            du + ((t1 + half_len) / scale) as f32,
        );
        let mut around = 0.0;
        for k in 0..6 {
            let next = (k + 1) % 6;
            let ring = [
                placed.at(t0, a[k].0, a[k].1),
                placed.at(t1, b[k].0, b[k].1),
                placed.at(t1, b[next].0, b[next].1),
                placed.at(t0, a[next].0, a[next].1),
            ];
            let step = (a[next].0 - a[k].0).hypot(a[next].1 - a[k].1);
            let (v0, v1) = (
                dv + (around / scale) as f32,
                dv + ((around + step) / scale) as f32,
            );
            let normal = face_normal(&ring);
            mesh.quad_with_normals(
                ring,
                [[u0, v0], [u1, v0], [u1, v1], [u0, v1]],
                [normal; 4],
                [[0.0; 2]; 4],
            );
            around += step;
        }
    }

    // The two ends. A sleeper end is a face somebody on the platform looks
    // straight at, so it is closed properly rather than left open.
    for (station, outward) in [
        (stations.first().expect("stations"), -placed.lengthwise),
        (stations.last().expect("stations"), placed.lengthwise),
    ] {
        let (t, s) = *station;
        let u = du + ((t + half_len) / scale) as f32;
        let mut around = 0.0;
        let ring: Vec<(DVec3, [f32; 2])> = s
            .iter()
            .enumerate()
            .map(|(k, &(c, y))| {
                if k > 0 {
                    around += (c - s[k - 1].0).hypot(y - s[k - 1].1);
                }
                (placed.at(t, c, y), [u, dv + (around / scale) as f32])
            })
            .collect();
        let centre = ring.iter().map(|(p, _)| *p).sum::<DVec3>() / ring.len() as f64;
        mesh.fan((centre, [u, dv]), &ring, outward);
    }
}

/// A quad's normal from its own winding — the sleeper is flat-shaded, which
/// is what a cast face and a sawn face both are.
fn face_normal(ring: &[DVec3; 4]) -> [f32; 3] {
    to_render((ring[1] - ring[0]).cross(ring[2] - ring[1]).normalize())
}

/// The fastening at both rail seats of one sleeper.
///
/// W 14 (concrete): the elastic pad under the rail foot, a guide plate each
/// side of it and the Spannklemme Skl 14 over the plate with its arm reaching
/// back in over the foot. Oberbau K (timber): the ribbed baseplate the rail
/// stands on — the plate is what carries the 1:40, which is why a timber
/// sleeper has a flat top — with a clamp plate each side.
///
/// Everything here sits *above* the sleeper's top face, so its depths are
/// negative: the rail foot is a pad's thickness clear of the sleeper, and
/// closing that gap is half of what the pad is drawn for.
fn push_fastenings(mesh: &mut MeshBuilder, placed: &Placed, ob: &Oberbau, kind: Fastening) {
    let foot_half = ob.rail.dimensions().foot_width / 2.0;
    let axis = ob.rail_axis();
    let pad = ob.rail_pad.max(0.004);

    for side in [-1.0f64, 1.0] {
        // The seat's own axes, tipped by the 1:40 the rail stands at — the
        // plate lies under the foot, not beside it.
        let tilt = -side * RAIL_CANT;
        let along = (placed.lengthwise + placed.up * tilt).normalize();
        let up = (placed.up - placed.lengthwise * tilt).normalize();
        let across = placed.crosswise;
        let seat = |t: f64, y: f64| placed.at(side * axis, 0.0, 0.0) + along * t - up * y;

        match kind {
            Fastening::W14 => {
                // The pad: as wide as the foot, its top where the foot
                // underside is. Without it the rail floats a centimetre over
                // the sleeper and every low camera sees the gap.
                mesh.cuboid(
                    seat(0.0, -pad / 2.0),
                    [along * foot_half, across * 0.090, up * (pad / 2.0)],
                );
                for edge in [-1.0f64, 1.0] {
                    mesh.cuboid(
                        seat(edge * (foot_half + 0.026), -0.011),
                        [along * 0.026, across * 0.090, up * 0.011],
                    );
                    // The clip's arm comes back in over the rail foot.
                    mesh.cuboid(
                        seat(edge * (foot_half - 0.004), -0.028),
                        [along * 0.030, across * 0.062, up * 0.007],
                    );
                }
            }
            Fastening::K => {
                // One ribbed plate under the whole seat, screwed through the
                // sleeper; the rail stands on it, so its top is the pad line.
                mesh.cuboid(
                    seat(0.0, -pad / 2.0),
                    [
                        along * (foot_half + 0.055),
                        across * 0.078,
                        up * (pad / 2.0),
                    ],
                );
                for edge in [-1.0f64, 1.0] {
                    mesh.cuboid(
                        seat(edge * (foot_half + 0.012), -(pad + 0.009)),
                        [along * 0.030, across * 0.055, up * 0.009],
                    );
                }
            }
            Fastening::None => {}
        }
    }
}
