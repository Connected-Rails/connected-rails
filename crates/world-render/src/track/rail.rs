//! The two running rails.
//!
//! The section is not invented here — [`RailProfile::contour`] hands over the
//! rolled profile, crowned running surface and all, and this module only
//! stands it up on the track. Two things about standing it up matter:
//!
//! * **The rails are rotated 1:40, not sheared.** Shearing keeps the running
//!   surface horizontal, and a horizontal flat running surface is why track
//!   used to carry one uniform white band from the cab to the horizon. A
//!   rotated crowned head catches the sun on a narrow line that wanders
//!   across the head as the track turns, which is what a real rail does. The
//!   rotation is about the inner head face in the gauge measuring plane, so
//!   the 1435 mm stay 1435 mm.
//! * **The section is sampled by curvature.** A fixed 4 m step puts a 7 mm
//!   kink in the rail through a 300 m curve, which reads as facets along the
//!   whole outside of the bend.
//!
//! Rails are built in chunks with two levels of detail: the rolled section
//! near the camera, a plain envelope beyond a few hundred metres where a rail
//! is two pixels wide. Both are hung on their own centre, so the cull
//! measures to the chunk and not to the edge's anchor.

use bevy::prelude::{Mesh, Vec3};
use glam::DVec3;
use track_model::{GAUGE, GAUGE_MEASURE, RAIL_CANT, RailPoint, RailProfile, TrackEdge};
use world_coords::EnuFrame;

use super::ballast::mid_section;
use super::cross_section;
use super::mesh::{MeshBuilder, to_render};

/// Rails merged into one mesh over this many metres — short enough that the
/// level of detail can switch per chunk, long enough not to cost a draw call
/// every few sleepers.
const CHUNK: f64 = 96.0;
/// Beyond this the rolled section is dropped for the plain envelope \[m\]. A
/// 72 mm head is under a pixel wide there on a 1440p screen.
pub(super) const DETAIL_RANGE: f32 = 320.0;
/// How finely the head's arcs are tessellated near the camera.
const HEAD_STEPS: usize = 14;
/// Sagitta a straightened chord may leave against the real curve \[m\].
const CHORD_TOLERANCE: f64 = 0.002;
/// Longest and shortest step along the edge \[m\].
const STEP_RANGE: (f64, f64) = (1.0, 8.0);
/// The far level of detail keeps a plain step — it is drawing a line.
const FAR_STEP: f64 = 8.0;
/// Corners of the section sharper than this keep a crease; everything
/// shallower is shaded smooth, which is what the crown and the fillets are.
const CREASE: f64 = 0.55;

/// One chunk of rail: the mesh and the render-space offset of the point it
/// has to be hung on.
pub(super) type Chunk = (Mesh, Vec3);

/// Both rails over the whole edge, in chunks, at both levels of detail.
pub(super) fn build(
    e: &TrackEdge,
    frame: &EnuFrame,
    profile: RailProfile,
) -> (Vec<Chunk>, Vec<Chunk>) {
    let near = ring_of(&profile.contour(HEAD_STEPS));
    let far = ring_of(&profile.coarse_contour());
    let mut near_chunks = Vec::new();
    let mut far_chunks = Vec::new();

    let length = e.length();
    let count = ((length / CHUNK).ceil() as usize).max(1);
    for i in 0..count {
        let s0 = length * i as f64 / count as f64;
        let s1 = length * (i + 1) as f64 / count as f64;
        // Only the first and last chunk are cut ends of the rail; inside the
        // edge the chunks butt against each other and a cap would show as a
        // dark disc through the joint.
        let caps = (i == 0, i + 1 == count);
        near_chunks.push(chunk(e, frame, profile, &near, s0, s1, caps, false));
        far_chunks.push(chunk(e, frame, profile, &far, s0, s1, caps, true));
    }
    (near_chunks, far_chunks)
}

/// A vertex of the extruded ring: where it sits in the section, which way it
/// faces there, and what the surface is.
#[derive(Clone, Copy)]
struct RingVertex {
    across: f64,
    down: f64,
    /// Section normal, `(across, up)` — the extrusion lifts it into the
    /// track's frame.
    normal: (f64, f64),
    polish: f64,
    flank: f64,
}

/// Turns a closed contour into the ring the extrusion walks: two vertices per
/// contour edge, so a crease can carry two different normals at the same
/// point while a smooth corner carries the same averaged one twice.
fn ring_of(contour: &[RailPoint]) -> Vec<RingVertex> {
    let n = contour.len();
    // Outward normal of every contour edge, in `(across, up)`.
    let edge_normal: Vec<(f64, f64)> = (0..n)
        .map(|i| {
            let a = contour[i];
            let b = contour[(i + 1) % n];
            let (dx, dy) = (b.across - a.across, b.down - a.down);
            let len = dx.hypot(dy).max(1e-12);
            // The contour runs clockwise seen with `up` upwards, so the
            // outward normal of a step is the step turned a quarter to the
            // left: `(dx, dy) -> (-dy_up, dx)` with `dy_up = -dy`.
            (dy / len, dx / len)
        })
        .collect();

    let blend = |i: usize, j: usize| {
        let (a, b) = (edge_normal[i], edge_normal[j]);
        if a.0 * b.0 + a.1 * b.1 < CREASE {
            // Too sharp to smooth: the vertex keeps the face's own normal.
            return None;
        }
        let (x, y) = (a.0 + b.0, a.1 + b.1);
        let len = x.hypot(y).max(1e-12);
        Some((x / len, y / len))
    };

    let mut ring = Vec::with_capacity(n * 2);
    for i in 0..n {
        let (a, b) = (contour[i], contour[(i + 1) % n]);
        let start = blend((i + n - 1) % n, i).unwrap_or(edge_normal[i]);
        let end = blend(i, (i + 1) % n).unwrap_or(edge_normal[i]);
        ring.push(RingVertex {
            across: a.across,
            down: a.down,
            normal: start,
            polish: a.polish,
            flank: a.flank,
        });
        ring.push(RingVertex {
            across: b.across,
            down: b.down,
            normal: end,
            polish: b.polish,
            flank: b.flank,
        });
    }
    ring
}

/// One rail's placement in the track's cross-section: where the section's
/// origin goes and how its two axes lie once the 1:40 has tipped them.
///
/// The rotation is about the inner head face at the gauge measuring plane,
/// because that is the point the 1435 mm are measured to. Rotating about the
/// rail's own axis instead would move the gauge by a millimetre and a half,
/// which is a quarter of the tolerance a track is allowed.
struct Seat {
    origin: DVec3,
    across: DVec3,
    up: DVec3,
}

impl Seat {
    fn new(
        (center, right, _, up): (DVec3, DVec3, DVec3, DVec3),
        side: f64,
        head_width: f64,
    ) -> Self {
        // The whole section turns by the cant — the sign of `side` is
        // already in the angle, so both rails lean towards each other.
        let (sin, cos) = (side * RAIL_CANT).atan().sin_cos();
        let across = right * cos + up * sin;
        let seat_up = up * cos - right * sin;
        // The pivot: the inner head face, half the gauge from the centre and
        // the measuring depth under the running plane.
        let pivot = center + right * (side * GAUGE / 2.0) - up * GAUGE_MEASURE;
        // In the section's own coordinates that point is `(-side·w/2, 14 mm)`.
        Self {
            origin: pivot + across * (side * head_width / 2.0) + seat_up * GAUGE_MEASURE,
            across,
            up: seat_up,
        }
    }

    fn at(&self, across: f64, down: f64) -> DVec3 {
        self.origin + self.across * across - self.up * down
    }

    fn normal(&self, n: (f64, f64)) -> DVec3 {
        self.across * n.0 + self.up * n.1
    }
}

/// One chunk of one level of detail.
#[allow(clippy::too_many_arguments)]
fn chunk(
    e: &TrackEdge,
    frame: &EnuFrame,
    profile: RailProfile,
    ring: &[RingVertex],
    s0: f64,
    s1: f64,
    caps: (bool, bool),
    coarse: bool,
) -> Chunk {
    let head_width = profile.dimensions().head_width;
    let mut mesh = MeshBuilder::new(true);
    let stations = stations(e, s0, s1, coarse);

    for side in [-1.0f64, 1.0] {
        let seats: Vec<Seat> = stations
            .iter()
            .map(|&s| Seat::new(cross_section(e, frame, s), side, head_width))
            .collect();
        for (i, pair) in seats.windows(2).enumerate() {
            let (a, b) = (&pair[0], &pair[1]);
            let (s_a, s_b) = (stations[i] as f32, stations[i + 1] as f32);
            for step in ring.as_chunks::<2>().0 {
                let (p, q) = (step[0], step[1]);
                // Round the section first, then along the rail: that order
                // faces the quads outwards. The other way round every face
                // of both rails is a backface.
                mesh.quad_with_normals(
                    [
                        a.at(p.across, p.down),
                        a.at(q.across, q.down),
                        b.at(q.across, q.down),
                        b.at(p.across, p.down),
                    ],
                    // u runs along the rail in metres, v is the depth under
                    // the running surface: enough for the shader to weather
                    // the section without a texture of its own.
                    [
                        [s_a, p.down as f32],
                        [s_a, q.down as f32],
                        [s_b, q.down as f32],
                        [s_b, p.down as f32],
                    ],
                    [
                        to_render(a.normal(p.normal)),
                        to_render(a.normal(q.normal)),
                        to_render(b.normal(q.normal)),
                        to_render(b.normal(p.normal)),
                    ],
                    // The surface itself: how far the wheels have polished
                    // it, and whether this is the head flank on the gauge
                    // side, the one face that shades itself.
                    [
                        surface(p, side),
                        surface(q, side),
                        surface(q, side),
                        surface(p, side),
                    ],
                );
            }
        }
        // The cut ends of the rail, so a buffer stop does not look into a
        // hollow section.
        let contour: Vec<(f64, f64)> = ring
            .as_chunks::<2>()
            .0
            .iter()
            .map(|step| (step[0].across, step[0].down))
            .collect();
        let tangent = cross_section(e, frame, (s0 + s1) / 2.0).2;
        if caps.0 {
            cap(&mut mesh, &seats[0], &contour, -tangent);
        }
        if caps.1 {
            cap(
                &mut mesh,
                seats.last().expect("stations"),
                &contour,
                tangent,
            );
        }
    }
    mesh.build(mid_section(e, frame, s0, s1))
}

/// What the shader is told about a point of the section: `(polish, gauge
/// flank)`. The flank flag only fires on the side of the head that looks
/// across the gauge — the field side is open to the sky and is not shaded.
fn surface(p: RingVertex, side: f64) -> [f32; 2] {
    let gauge_side = f64::from(p.across * side < 0.0);
    [p.polish as f32, (p.flank * gauge_side) as f32]
}

/// Closes a cut rail end.
///
/// Not a fan around the section's centre: a rail is an I and its centre sits
/// in the web, from where half the head's underside is round a corner — a fan
/// lays triangles straight through the outside of the section and leaves a
/// fin standing off the end of every buffer stop. The contour is mirror
/// symmetric and one interval wide at every depth, so it closes exactly as a
/// strip rung by rung between its two halves instead.
fn cap(mesh: &mut MeshBuilder, seat: &Seat, contour: &[(f64, f64)], outward: DVec3) {
    let n = contour.len();
    // The half the contour was mirrored from: the crown, the field side, the
    // middle of the foot.
    let half = (n + 2) / 2;
    if half < 3 {
        return;
    }
    let normal = to_render(outward.normalize());
    // Which way the rungs have to be wound to face `outward`. A rung as
    // ordered below comes out facing `-(across × up)`, so it is turned round
    // wherever that points back into the rail — which is at the start of a
    // rail but not at its end, and mirrored again between the two rails.
    let flip = seat.across.cross(seat.up).dot(outward).is_sign_positive();
    let at = |i: usize| {
        let (across, down) = contour[i];
        seat.at(across, down)
    };
    let uv = |i: usize| [contour[i].0 as f32, contour[i].1 as f32];
    // Index of the mirror of a point on the field side.
    let mirror = |i: usize| n - i;

    let mut ring = |points: [usize; 3]| {
        let mut p = points.map(at);
        let mut t = points.map(uv);
        if flip {
            p.swap(1, 2);
            t.swap(1, 2);
        }
        mesh.triangle(p, t, normal);
    };
    // The crown and the middle of the foot lie on the axis and have no
    // mirror; everything between them closes as a quad, cut into two
    // triangles so the one call covers both odd ends.
    ring([0, 1, mirror(1)]);
    for i in 1..half - 2 {
        ring([i, i + 1, mirror(i)]);
        ring([i + 1, mirror(i + 1), mirror(i)]);
    }
    ring([half - 2, half - 1, mirror(half - 2)]);
}

/// Where the section is put down between `s0` and `s1`. On a straight the
/// step is the full [`STEP_RANGE`]; through a curve it shortens to whatever
/// keeps the chord within [`CHORD_TOLERANCE`] of the arc, which is what stops
/// a bend from reading as a run of facets.
fn stations(e: &TrackEdge, s0: f64, s1: f64, coarse: bool) -> Vec<f64> {
    let mut out = vec![s0];
    let mut s = s0;
    while s < s1 {
        let step = if coarse {
            FAR_STEP
        } else {
            let radius = 1.0 / e.eval(s).curvature.abs().max(1e-9);
            (8.0 * CHORD_TOLERANCE * radius)
                .sqrt()
                .clamp(STEP_RANGE.0, STEP_RANGE.1)
        };
        s = (s + step).min(s1);
        out.push(s);
    }
    // A chunk shorter than one step still needs two stations to be a rail.
    if out.len() < 2 {
        out.push(s1);
    }
    out
}
