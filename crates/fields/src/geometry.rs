//! Turning a cadastral outline into something that can be drawn.
//!
//! What comes out of a register is not a mesh. A parcel boundary is surveyed to
//! the centimetre and has a vertex every couple of metres, it may double back
//! on itself where a service simplified it badly, it runs across the track and
//! over the module boundary, and it says nothing about which way the tractor
//! went. This module is the four answers to that (plan ch. 4, ch. 7):
//! thinning, clipping, repair, and the long axis the furrows follow.
//!
//! Everything works in metres — UTM eastings and northings — because that is
//! what a tolerance in metres and an area in hectares mean. Degrees never get
//! this far.

use glam::DVec2;

/// Signed area of a ring [m²]. Positive means counter-clockwise.
pub fn area(ring: &[DVec2]) -> f64 {
    if ring.len() < 3 {
        return 0.0;
    }
    let mut total = 0.0;
    let mut j = ring.len() - 1;
    for i in 0..ring.len() {
        total += (ring[j].x - ring[i].x) * (ring[j].y + ring[i].y);
        j = i;
    }
    total / 2.0
}

/// Centre of mass of a ring. Falls back to the average of the vertices for a
/// degenerate ring, which is what a two-point "polygon" from a broken service
/// gives.
pub fn centroid(ring: &[DVec2]) -> DVec2 {
    let a = area(ring);
    if ring.len() < 3 || a.abs() < 1e-9 {
        if ring.is_empty() {
            return DVec2::ZERO;
        }
        return ring.iter().copied().sum::<DVec2>() / ring.len() as f64;
    }
    let mut c = DVec2::ZERO;
    let mut j = ring.len() - 1;
    for i in 0..ring.len() {
        let cross = ring[j].x * ring[i].y - ring[i].x * ring[j].y;
        c += (ring[j] + ring[i]) * cross;
        j = i;
    }
    c / (6.0 * a)
}

/// Turns the ring counter-clockwise, whichever way it arrived.
pub fn to_ccw(ring: &mut [DVec2]) {
    if area(ring) < 0.0 {
        ring.reverse();
    }
}

/// Whether a point lies inside a ring.
pub fn contains(ring: &[DVec2], p: DVec2) -> bool {
    if ring.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = ring.len() - 1;
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Bounding box `(min, max)` of a set of points.
pub fn bounds(points: &[DVec2]) -> (DVec2, DVec2) {
    let mut lo = DVec2::splat(f64::MAX);
    let mut hi = DVec2::splat(f64::MIN);
    for p in points {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    (lo, hi)
}

/// Drops what no eye resolves and no mesh should pay for: repeated points,
/// vertices that lie on the line between their neighbours, and spikes where a
/// ring doubles back on itself within `tolerance`.
///
/// Douglas-Peucker over a closed ring needs two fixed points, not one, or the
/// whole ring can collapse onto its own first vertex. The two chosen are the
/// vertices furthest apart, so the halves are of similar length.
pub fn simplify(ring: &[DVec2], tolerance: f64) -> Vec<DVec2> {
    let ring = dedupe(ring, tolerance.min(0.25));
    if ring.len() < 4 || tolerance <= 0.0 {
        return ring;
    }
    let (a, b) = furthest_pair(&ring);
    let (first, second) = if a < b { (a, b) } else { (b, a) };
    // The ring, cut at the two anchors into two open runs: `first` round to
    // `second`, and `second` round to `first` again.
    let mut out = douglas_peucker(&ring[first..=second], tolerance);
    let mut wrap: Vec<DVec2> = ring[second..].to_vec();
    wrap.extend_from_slice(&ring[..=first]);
    let back = douglas_peucker(&wrap, tolerance);
    // Both runs begin and end on an anchor, which is therefore in `out` already.
    if back.len() > 2 {
        out.extend_from_slice(&back[1..back.len() - 1]);
    }
    if out.len() < 3 { ring } else { out }
}

/// Removes consecutive points closer together than `epsilon`, and the closing
/// repeat of the first point that most services write.
pub fn dedupe(ring: &[DVec2], epsilon: f64) -> Vec<DVec2> {
    let mut out: Vec<DVec2> = Vec::with_capacity(ring.len());
    for &p in ring {
        if out
            .last()
            .is_none_or(|q| q.distance_squared(p) > epsilon * epsilon)
        {
            out.push(p);
        }
    }
    while out.len() > 1 {
        let (first, last) = (out[0], out[out.len() - 1]);
        if first.distance_squared(last) <= epsilon * epsilon {
            out.pop();
        } else {
            break;
        }
    }
    out
}

/// The two vertices of a ring furthest apart, by index.
fn furthest_pair(ring: &[DVec2]) -> (usize, usize) {
    // Over the hull rather than every pair: a parcel can have a thousand
    // vertices, and the diameter is always a pair of hull points.
    let hull = convex_hull(ring);
    let mut ends = (DVec2::ZERO, DVec2::ZERO);
    let mut far = -1.0;
    for i in 0..hull.len() {
        for j in i + 1..hull.len() {
            let d = hull[i].distance_squared(hull[j]);
            if d > far {
                far = d;
                ends = (hull[i], hull[j]);
            }
        }
    }
    let index_of = |p: DVec2| ring.iter().position(|q| *q == p);
    match (index_of(ends.0), index_of(ends.1)) {
        (Some(a), Some(b)) if a != b => (a, b),
        // A ring with no hull to speak of: cut it in half and be done.
        _ => (0, ring.len() / 2),
    }
}

/// Douglas-Peucker over an open polyline, both ends kept.
pub fn douglas_peucker(points: &[DVec2], tolerance: f64) -> Vec<DVec2> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0usize, points.len() - 1)];
    while let Some((lo, hi)) = stack.pop() {
        if hi - lo < 2 {
            continue;
        }
        let (a, b) = (points[lo], points[hi]);
        let axis = b - a;
        let length = axis.length();
        let mut worst = 0.0;
        let mut index = lo;
        for (offset, p) in points[lo + 1..hi].iter().enumerate() {
            let d = if length < 1e-12 {
                p.distance(a)
            } else {
                (axis.x * (a.y - p.y) - (a.x - p.x) * axis.y).abs() / length
            };
            if d > worst {
                worst = d;
                index = lo + 1 + offset;
            }
        }
        if worst > tolerance {
            keep[index] = true;
            stack.push((lo, index));
            stack.push((index, hi));
        }
    }
    points
        .iter()
        .zip(keep)
        .filter_map(|(p, k)| k.then_some(*p))
        .collect()
}

/// Andrew's monotone chain, counter-clockwise, without the repeated last point.
pub fn convex_hull(points: &[DVec2]) -> Vec<DVec2> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut sorted = points.to_vec();
    sorted.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    sorted.dedup();
    if sorted.len() < 3 {
        return sorted;
    }
    let cross = |o: DVec2, a: DVec2, b: DVec2| (a - o).perp_dot(b - o);
    let half = |points: &mut dyn Iterator<Item = DVec2>| {
        let mut chain: Vec<DVec2> = Vec::new();
        for p in points {
            while chain.len() >= 2
                && cross(chain[chain.len() - 2], chain[chain.len() - 1], p) <= 0.0
            {
                chain.pop();
            }
            chain.push(p);
        }
        // The last point of each half is the first of the other.
        chain.pop();
        chain
    };
    let mut hull = half(&mut sorted.iter().copied());
    hull.extend(half(&mut sorted.iter().rev().copied()));
    hull
}

/// The rectangle of least area around a ring, as rotating calipers finds it.
///
/// Its long side is the direction the field was worked in, and that is the
/// single biggest thing the eye picks up about a field — furrows, tramlines and
/// the swath of a combine all run along it (plan ch. 7).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MinRect {
    /// Angle of the long side against east [rad], in `-PI/2 ..= PI/2`.
    pub angle: f64,
    pub length: f64,
    pub width: f64,
    pub centre: DVec2,
}

/// The minimum-area rectangle of a ring. `None` for anything without an area.
pub fn min_area_rect(ring: &[DVec2]) -> Option<MinRect> {
    let hull = convex_hull(ring);
    if hull.len() < 3 {
        return None;
    }
    let mut best: Option<(f64, MinRect)> = None;
    for i in 0..hull.len() {
        let edge = hull[(i + 1) % hull.len()] - hull[i];
        if edge.length_squared() < 1e-12 {
            continue;
        }
        // Every minimum-area rectangle has a side flush with a hull edge, so
        // only the hull's own directions have to be tried.
        let axis = edge.normalize();
        let across = axis.perp();
        let (mut lo_u, mut hi_u) = (f64::MAX, f64::MIN);
        let (mut lo_v, mut hi_v) = (f64::MAX, f64::MIN);
        for p in &hull {
            let u = p.dot(axis);
            let v = p.dot(across);
            lo_u = lo_u.min(u);
            hi_u = hi_u.max(u);
            lo_v = lo_v.min(v);
            hi_v = hi_v.max(v);
        }
        let (du, dv) = (hi_u - lo_u, hi_v - lo_v);
        let size = du * dv;
        if best.as_ref().is_some_and(|(b, _)| *b <= size) {
            continue;
        }
        let centre = axis * ((lo_u + hi_u) / 2.0) + across * ((lo_v + hi_v) / 2.0);
        // The long side is the one that gets to be the direction.
        let (long, short, dir) = if du >= dv {
            (du, dv, axis)
        } else {
            (dv, du, across)
        };
        best = Some((
            size,
            MinRect {
                angle: normalise_angle(dir.y.atan2(dir.x)),
                length: long,
                width: short,
                centre,
            },
        ));
    }
    best.map(|(_, r)| r)
}

/// Folds an angle into `-PI/2 ..= PI/2`: a furrow direction and its opposite
/// are the same direction.
fn normalise_angle(mut a: f64) -> f64 {
    use std::f64::consts::PI;
    while a > PI / 2.0 {
        a -= PI;
    }
    while a < -PI / 2.0 {
        a += PI;
    }
    a
}

/// What [`clip`] should do with the two rings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// Everything of `subject` that lies inside `clip` — cutting a field to the
    /// module envelope.
    Intersect,
    /// Everything of `subject` that lies outside `clip` — punching the track's
    /// formation out of a field.
    Difference,
}

/// Clips one ring against another (Greiner-Hormann), yielding the rings that
/// are left. A field cut in two by an embankment comes back as two fields,
/// which is what it is.
///
/// Both rings must be simple. Cadastral outlines and a hand-drawn envelope are;
/// where they are not, the pass that finds no usable intersection falls back to
/// the containment answer, so the caller gets a whole field rather than a hole.
//
// ponytail: no holes. A ring inside a ring — a pond in the middle of a field —
// comes back as the outer ring alone, so the pond is drawn over. Holes want a
// polygon type with holes all the way through the mesh builder; a hole in a
// German field block is rare enough to wait for that.
pub fn clip(subject: &[DVec2], clip: &[DVec2], op: Op) -> Vec<Vec<DVec2>> {
    let (rings, sure) = clip_once(subject, clip, op);
    if sure {
        return rings;
    }
    // The two touch without the pass finding a crossing, which is what
    // happens when a corner sits exactly on the other's edge or two edges run
    // along each other — and it is exactly what a cadastral register is full
    // of, because neighbouring parcels are digitised from a shared boundary.
    // Greiner-Hormann cannot tell that from one ring lying wholly inside the
    // other, and answers the wrong one of the two.
    //
    // So nudge and go round again. A tenth of a millimetre is four orders
    // under anything anybody can see and six over the last bit of an `f64` at
    // these coordinates; what it buys is a proper crossing where there was a
    // degenerate touch.
    let (nudged, sure) = clip_once(&nudge(subject), clip, op);
    if sure { nudged } else { rings }
}

/// Moves every vertex of a ring a tenth of a millimetre, deterministically.
///
/// Deterministic from the vertex itself, so one ring is always moved the same
/// way — the same field cut in two neighbouring tiles is cut along the same
/// line, and two clients of a multiplayer run agree. No trigonometry: the
/// platform's `sin` is not promised to be the same bit on two machines.
fn nudge(ring: &[DVec2]) -> Vec<DVec2> {
    const OFF: f64 = 1e-4;
    ring.iter()
        .enumerate()
        .map(|(i, p)| {
            let h = (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
                ^ (p.x.to_bits() >> 13)
                ^ (p.y.to_bits() >> 7).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
            let unit = |bits: u64| (bits & 0xFFFF) as f64 / 32_768.0 - 1.0;
            *p + DVec2::new(unit(h), unit(h >> 16)) * OFF
        })
        .collect()
}

/// One pass of the clip, and whether its answer can be trusted.
///
/// Not trusted when the pass found no crossing at all *and* the subject's own
/// corners disagree about which side of the clip they are on: they cannot
/// disagree unless the rings cross, so a pass that found no crossing has
/// missed one.
fn clip_once(subject: &[DVec2], clip: &[DVec2], op: Op) -> (Vec<Vec<DVec2>>, bool) {
    let mut subject = dedupe(subject, 1e-6);
    let mut other = dedupe(clip, 1e-6);
    if subject.len() < 3 {
        return (Vec::new(), true);
    }
    if other.len() < 3 {
        return (vec![subject], true);
    }
    to_ccw(&mut subject);
    to_ccw(&mut other);
    // A difference is an intersection with the outside, and the outside is the
    // clip ring walked the other way.
    if op == Op::Difference {
        other.reverse();
    }

    let mut s = Chain::new(&subject);
    let mut c = Chain::new(&other);
    if !intersect_chains(&mut s, &mut c) {
        // No crossing: one is wholly inside the other, or they are disjoint —
        // *if* that is really so. Corners on either side of the other ring say
        // it is not, and then this answer is a guess.
        let first = contains(&other, subject[0]);
        let sure = subject.iter().all(|p| contains(&other, *p) == first) && {
            let theirs = contains(&subject, other[0]);
            other.iter().all(|p| contains(&subject, *p) == theirs)
        };
        let inside = first == (op == Op::Intersect);
        let subject_holds_clip = contains(&subject, other[0]);
        let rings = match (op, inside, subject_holds_clip) {
            (Op::Intersect, true, _) => vec![subject],
            (Op::Intersect, false, true) => vec![other],
            (Op::Intersect, false, false) => Vec::new(),
            // Difference: `inside` is already the "outside the clip" answer.
            (Op::Difference, true, _) => vec![subject],
            (Op::Difference, false, _) => Vec::new(),
        };
        return (rings, sure);
    }
    mark_entries(&mut s, &other);
    mark_entries(&mut c, &subject);
    if op == Op::Difference {
        // The subject is being intersected with the *outside* of the clip ring,
        // so a crossing that enters the clip is where the result stops, not
        // where it starts. The clip chain needs no flip: it is walked the other
        // way round already, and its flags were read off that walk.
        for entry in &mut s.entry {
            *entry = !*entry;
        }
    }
    (walk(&s, &c), true)
}

/// A ring as a list of vertices with the intersections spliced in.
struct Chain {
    points: Vec<DVec2>,
    /// For an intersection: its index in the other chain. `usize::MAX` for an
    /// ordinary vertex.
    partner: Vec<usize>,
    /// For an intersection: whether walking forward from here goes *into* the
    /// other ring.
    entry: Vec<bool>,
    /// Whether this vertex is an intersection at all.
    crossing: Vec<bool>,
    used: std::cell::RefCell<Vec<bool>>,
}

const NONE: usize = usize::MAX;

impl Chain {
    fn new(ring: &[DVec2]) -> Self {
        let n = ring.len();
        Self {
            points: ring.to_vec(),
            partner: vec![NONE; n],
            entry: vec![false; n],
            crossing: vec![false; n],
            used: std::cell::RefCell::new(vec![false; n]),
        }
    }

    fn len(&self) -> usize {
        self.points.len()
    }
}

/// Finds every crossing of the two rings and splices it into both, keeping the
/// cross-links. `false` when they never cross.
fn intersect_chains(s: &mut Chain, c: &mut Chain) -> bool {
    // Collected first, spliced afterwards: inserting while walking would shift
    // the indices the walk is using.
    let mut hits: Vec<(usize, f64, usize, f64, DVec2)> = Vec::new();
    for i in 0..s.len() {
        let a0 = s.points[i];
        let a1 = s.points[(i + 1) % s.len()];
        for j in 0..c.len() {
            let b0 = c.points[j];
            let b1 = c.points[(j + 1) % c.len()];
            if let Some((t, u, p)) = segment_intersection(a0, a1, b0, b1) {
                hits.push((i, t, j, u, p));
            }
        }
    }
    if hits.is_empty() {
        return false;
    }
    splice(s, hits.iter().map(|(i, t, _, _, p)| (*i, *t, *p)).collect());
    splice(c, hits.iter().map(|(_, _, j, u, p)| (*j, *u, *p)).collect());
    // Re-link: after splicing, a crossing is found in both chains by its point.
    let mut linked = 0;
    for i in 0..s.len() {
        if !s.crossing[i] {
            continue;
        }
        let found = (0..c.len()).find(|&j| {
            c.crossing[j]
                && c.partner[j] == NONE
                && c.points[j].distance_squared(s.points[i]) < 1e-12
        });
        if let Some(j) = found {
            s.partner[i] = j;
            c.partner[j] = i;
            linked += 1;
        }
    }
    // An odd number of crossings means a tangency the walk cannot resolve —
    // treat it as no crossing and let the caller fall back on containment.
    linked >= 2 && linked % 2 == 0
}

/// Inserts the crossings of one chain, in order along each edge.
fn splice(chain: &mut Chain, mut hits: Vec<(usize, f64, DVec2)>) {
    hits.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    hits.dedup_by(|a, b| a.0 == b.0 && (a.1 - b.1).abs() < 1e-9);
    let mut points = Vec::with_capacity(chain.len() + hits.len());
    let mut crossing = Vec::with_capacity(points.capacity());
    let mut at = 0;
    for i in 0..chain.len() {
        points.push(chain.points[i]);
        crossing.push(false);
        while at < hits.len() && hits[at].0 == i {
            points.push(hits[at].2);
            crossing.push(true);
            at += 1;
        }
    }
    let n = points.len();
    chain.points = points;
    chain.crossing = crossing;
    chain.partner = vec![NONE; n];
    chain.entry = vec![false; n];
    chain.used = std::cell::RefCell::new(vec![false; n]);
}

/// Marks each crossing as an entry into, or an exit from, the other ring —
/// alternating along the chain, starting from where the first vertex lies.
fn mark_entries(chain: &mut Chain, other: &[DVec2]) {
    let mut inside = contains(other, chain.points[0]);
    for i in 0..chain.len() {
        if chain.crossing[i] {
            chain.entry[i] = !inside;
            inside = !inside;
        }
    }
}

/// Walks the two chains, swapping at every crossing: forward along the subject
/// from an entry, forward along the clip from the next exit.
fn walk(s: &Chain, c: &Chain) -> Vec<Vec<DVec2>> {
    let mut rings = Vec::new();
    for start in 0..s.len() {
        if !s.crossing[start] || !s.entry[start] || s.used.borrow()[start] {
            continue;
        }
        let mut ring = Vec::new();
        let mut on_subject = true;
        let mut at = start;
        // A ring cannot be longer than both chains together; the bound is what
        // stops a malformed pair of polygons from spinning here forever.
        for _ in 0..(s.len() + c.len()) * 2 + 4 {
            let chain = if on_subject { s } else { c };
            chain.used.borrow_mut()[at] = true;
            ring.push(chain.points[at]);
            at = (at + 1) % chain.len();
            if chain.crossing[at] {
                chain.used.borrow_mut()[at] = true;
                ring.push(chain.points[at]);
                let partner = chain.partner[at];
                if partner == NONE {
                    break;
                }
                at = partner;
                on_subject = !on_subject;
                if on_subject && at == start {
                    break;
                }
            }
        }
        let ring = dedupe(&ring, 1e-6);
        if ring.len() >= 3 && area(&ring).abs() > 1e-6 {
            rings.push(ring);
        }
    }
    rings
}

/// Where two segments cross, as the fractions along each and the point. Ends
/// that merely touch are not a crossing — that keeps a shared vertex from
/// splicing a zero-length edge in.
fn segment_intersection(a0: DVec2, a1: DVec2, b0: DVec2, b1: DVec2) -> Option<(f64, f64, DVec2)> {
    let r = a1 - a0;
    let s = b1 - b0;
    let denominator = r.perp_dot(s);
    if denominator.abs() < 1e-12 {
        return None;
    }
    let t = (b0 - a0).perp_dot(s) / denominator;
    let u = (b0 - a0).perp_dot(r) / denominator;
    const EPS: f64 = 1e-9;
    if (EPS..1.0 - EPS).contains(&t) && (EPS..1.0 - EPS).contains(&u) {
        Some((t, u, a0 + r * t))
    } else {
        None
    }
}

/// Ear clipping: a ring in, triangles as index triples out. The ring has to be
/// simple; the winding does not matter.
pub fn triangulate(ring: &[DVec2]) -> Vec<[u32; 3]> {
    let n = ring.len();
    if n < 3 {
        return Vec::new();
    }
    let ccw = area(ring) > 0.0;
    let mut remaining: Vec<usize> = if ccw {
        (0..n).collect()
    } else {
        (0..n).rev().collect()
    };
    let mut out = Vec::with_capacity(n.saturating_sub(2));
    let mut guard = 0;
    while remaining.len() > 3 {
        guard += 1;
        if guard > n * n + 16 {
            // A ring that is not simple after all. What is triangulated so far
            // is kept — a partly drawn field beats none, and the import warns.
            break;
        }
        let count = remaining.len();
        let mut clipped = false;
        for k in 0..count {
            let (i, j, l) = (
                remaining[(k + count - 1) % count],
                remaining[k],
                remaining[(k + 1) % count],
            );
            let (a, b, c) = (ring[i], ring[j], ring[l]);
            if (b - a).perp_dot(c - a) <= 0.0 {
                continue;
            }
            // An ear may not swallow another vertex of the ring.
            if remaining
                .iter()
                .any(|&m| m != i && m != j && m != l && in_triangle(ring[m], a, b, c))
            {
                continue;
            }
            out.push([i as u32, j as u32, l as u32]);
            remaining.remove(k);
            clipped = true;
            break;
        }
        if !clipped {
            break;
        }
    }
    if remaining.len() == 3 {
        out.push([
            remaining[0] as u32,
            remaining[1] as u32,
            remaining[2] as u32,
        ]);
    }
    out
}

fn in_triangle(p: DVec2, a: DVec2, b: DVec2, c: DVec2) -> bool {
    let d1 = (b - a).perp_dot(p - a);
    let d2 = (c - b).perp_dot(p - b);
    let d3 = (a - c).perp_dot(p - c);
    (d1 >= 0.0 && d2 >= 0.0 && d3 >= 0.0) || (d1 <= 0.0 && d2 <= 0.0 && d3 <= 0.0)
}

/// The quads a polyline sweeps out at `half_width` to each side — the shape a
/// track's formation punches out of a field.
///
/// One convex quad per segment rather than one long polygon: a curved line
/// buffered as a single ring self-intersects on the inside of the curve, and a
/// self-intersecting clip ring is exactly what [`clip`] cannot take.
pub fn corridor(points: &[DVec2], half_width: f64) -> Vec<Vec<DVec2>> {
    let mut out = Vec::new();
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let along = b - a;
        if along.length_squared() < 1e-9 {
            continue;
        }
        let across = along.normalize().perp() * half_width;
        // Stretched by half a width at each end, so consecutive quads overlap
        // and leave no wedge standing at a bend.
        let step = along.normalize() * half_width;
        let (a, b) = (a - step, b + step);
        out.push(vec![a + across, b + across, b - across, a - across]);
    }
    out
}

/// How many times two rings properly cross — not touch, cross. The punch uses
/// it to tell a corridor quad that cuts through a field from one that stands
/// wholly inside it.
pub fn crossings(ring: &[DVec2], other: &[DVec2]) -> usize {
    if ring.is_empty() || other.is_empty() {
        return 0;
    }
    let mut n = 0;
    for pair in edges(ring) {
        for (c0, c1) in edges(other) {
            if segment_intersection(pair.0, pair.1, c0, c1).is_some() {
                n += 1;
            }
        }
    }
    n
}

/// The edges of a ring as point pairs, last to first closed.
fn edges(ring: &[DVec2]) -> impl Iterator<Item = (DVec2, DVec2)> + '_ {
    ring.iter()
        .copied()
        .zip(ring.iter().skip(1).copied().chain(std::iter::once(ring[0])))
}

/// Stretches a corridor quad along its own axis, `length` further at its start
/// — the way the track came. The punch uses it to reach a quad that stands
/// wholly inside a field out past the field's boundary: its cut then crosses
/// the boundary after all, and the field is split or notched the normal way.
/// Only the start end moves, so a siding's end inside a field is not followed
/// by a phantom strip on the far side of the buffer stop.
pub fn stretch(quad: &[DVec2], length: f64) -> Vec<DVec2> {
    let along = (quad[1] - quad[0]).normalize_or_zero() * length;
    vec![quad[0] - along, quad[1], quad[2], quad[3] - along]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x: f64, y: f64, size: f64) -> Vec<DVec2> {
        vec![
            DVec2::new(x, y),
            DVec2::new(x + size, y),
            DVec2::new(x + size, y + size),
            DVec2::new(x, y + size),
        ]
    }

    #[test]
    fn area_and_winding() {
        let s = square(0.0, 0.0, 10.0);
        assert!((area(&s) - 100.0).abs() < 1e-9);
        let mut reversed = s.clone();
        reversed.reverse();
        assert!((area(&reversed) + 100.0).abs() < 1e-9);
        to_ccw(&mut reversed);
        assert!(area(&reversed) > 0.0);
    }

    #[test]
    fn centroid_of_a_square_is_its_middle() {
        let c = centroid(&square(10.0, 20.0, 4.0));
        assert!((c - DVec2::new(12.0, 22.0)).length() < 1e-9);
    }

    /// A square with `per_side - 1` extra points along each side — what a
    /// surveyed parcel boundary looks like next to the shape it is.
    fn dense_square(size: f64, per_side: usize) -> Vec<DVec2> {
        let corners = square(0.0, 0.0, size);
        let mut ring = Vec::new();
        for i in 0..4 {
            let (a, b) = (corners[i], corners[(i + 1) % 4]);
            for k in 0..per_side {
                ring.push(a + (b - a) * (k as f64 / per_side as f64));
            }
        }
        ring
    }

    #[test]
    fn simplify_keeps_the_corners_and_drops_the_rest() {
        let ring = dense_square(100.0, 10);
        assert_eq!(ring.len(), 40);
        let thin = simplify(&ring, 1.0);
        assert_eq!(thin.len(), 4, "{thin:?}");
        assert!((area(&thin).abs() - 10_000.0).abs() < 1e-6);
    }

    #[test]
    fn simplify_never_collapses_a_ring() {
        // A tolerance far larger than the field still has to leave a triangle.
        let thin = simplify(&dense_square(100.0, 10), 10_000.0);
        assert!(thin.len() >= 3, "{thin:?}");
    }

    #[test]
    fn dedupe_drops_the_closing_repeat() {
        let mut ring = square(0.0, 0.0, 10.0);
        ring.push(ring[0]);
        assert_eq!(dedupe(&ring, 0.01).len(), 4);
    }

    #[test]
    fn min_area_rect_finds_the_long_axis() {
        // A 200 x 40 field, turned 30 degrees.
        let angle: f64 = 30f64.to_radians();
        let (s, c) = angle.sin_cos();
        let ring: Vec<DVec2> = [(0.0, 0.0), (200.0, 0.0), (200.0, 40.0), (0.0, 40.0)]
            .into_iter()
            .map(|(x, y): (f64, f64)| DVec2::new(x * c - y * s, x * s + y * c))
            .collect();
        let rect = min_area_rect(&ring).expect("has an area");
        assert!((rect.angle - angle).abs() < 1e-6, "{}", rect.angle);
        assert!((rect.length - 200.0).abs() < 1e-6);
        assert!((rect.width - 40.0).abs() < 1e-6);
    }

    #[test]
    fn a_direction_and_its_opposite_are_one() {
        let ring: Vec<DVec2> = [(0.0, 0.0), (-200.0, 0.0), (-200.0, 40.0), (0.0, 40.0)]
            .into_iter()
            .map(|(x, y)| DVec2::new(x, y))
            .collect();
        let rect = min_area_rect(&ring).expect("has an area");
        assert!(rect.angle.abs() < 1e-9, "{}", rect.angle);
    }

    #[test]
    fn intersect_cuts_a_square_in_half() {
        let subject = square(0.0, 0.0, 100.0);
        let clip_ring = square(50.0, -10.0, 200.0);
        let out = clip(&subject, &clip_ring, Op::Intersect);
        assert_eq!(out.len(), 1, "{out:?}");
        assert!((area(&out[0]).abs() - 5_000.0).abs() < 1e-6, "{:?}", out[0]);
    }

    #[test]
    fn difference_takes_the_other_half() {
        let subject = square(0.0, 0.0, 100.0);
        let clip_ring = square(50.0, -10.0, 200.0);
        let out = clip(&subject, &clip_ring, Op::Difference);
        assert_eq!(out.len(), 1, "{out:?}");
        assert!((area(&out[0]).abs() - 5_000.0).abs() < 1e-6);
    }

    #[test]
    fn a_band_across_a_field_leaves_two() {
        let subject = square(0.0, 0.0, 100.0);
        // A 20 m strip straight through the middle, overhanging both ends.
        let band = vec![
            DVec2::new(-10.0, 40.0),
            DVec2::new(110.0, 40.0),
            DVec2::new(110.0, 60.0),
            DVec2::new(-10.0, 60.0),
        ];
        let out = clip(&subject, &band, Op::Difference);
        assert_eq!(out.len(), 2, "{out:?}");
        let total: f64 = out.iter().map(|r| area(r).abs()).sum();
        assert!((total - 8_000.0).abs() < 1e-6, "{total}");
    }

    #[test]
    fn disjoint_rings_answer_without_crossing() {
        let a = square(0.0, 0.0, 10.0);
        let b = square(100.0, 100.0, 10.0);
        assert!(clip(&a, &b, Op::Intersect).is_empty());
        let out = clip(&a, &b, Op::Difference);
        assert_eq!(out.len(), 1);
        assert!((area(&out[0]) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn a_ring_wholly_inside_survives_intersection() {
        let inner = square(10.0, 10.0, 10.0);
        let outer = square(0.0, 0.0, 100.0);
        let out = clip(&inner, &outer, Op::Intersect);
        assert_eq!(out.len(), 1);
        assert!((area(&out[0]) - 100.0).abs() < 1e-9);
        assert!(clip(&inner, &outer, Op::Difference).is_empty());
    }

    #[test]
    fn triangulation_covers_the_whole_ring() {
        // An L, so the ring is not convex and ears have to be chosen.
        let ring = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(60.0, 0.0),
            DVec2::new(60.0, 20.0),
            DVec2::new(20.0, 20.0),
            DVec2::new(20.0, 60.0),
            DVec2::new(0.0, 60.0),
        ];
        let tris = triangulate(&ring);
        assert_eq!(tris.len(), ring.len() - 2);
        let total: f64 = tris
            .iter()
            .map(|[a, b, c]| {
                let (a, b, c) = (ring[*a as usize], ring[*b as usize], ring[*c as usize]);
                ((b - a).perp_dot(c - a) / 2.0).abs()
            })
            .sum();
        assert!((total - area(&ring).abs()).abs() < 1e-6, "{total}");
    }

    #[test]
    fn a_corridor_is_one_quad_per_segment() {
        let line = [
            DVec2::new(0.0, 0.0),
            DVec2::new(100.0, 0.0),
            DVec2::new(200.0, 50.0),
        ];
        let quads = corridor(&line, 10.0);
        assert_eq!(quads.len(), 2);
        for quad in &quads {
            assert_eq!(quad.len(), 4);
            assert!(area(quad).abs() > 0.0);
        }
    }

    #[test]
    fn the_track_is_punched_out_of_a_field() {
        let field = square(0.0, 0.0, 400.0);
        let line = [DVec2::new(-50.0, 200.0), DVec2::new(450.0, 200.0)];
        let mut pieces = vec![field];
        for quad in corridor(&line, 25.0) {
            pieces = pieces
                .iter()
                .flat_map(|p| clip(p, &quad, Op::Difference))
                .collect();
        }
        assert_eq!(pieces.len(), 2, "{pieces:?}");
        let total: f64 = pieces.iter().map(|r| area(r).abs()).sum();
        // 400 x 400 less a 50 m wide swathe straight across.
        assert!((total - (160_000.0 - 20_000.0)).abs() < 1.0, "{total}");
    }

    #[test]
    fn crossings_tell_a_cut_from_an_enclosure() {
        let ring = square(0.0, 0.0, 100.0);
        // A band across the field: each long edge crosses the field's two
        // sides, so four crossings in all ...
        let band = vec![
            DVec2::new(-10.0, 40.0),
            DVec2::new(110.0, 40.0),
            DVec2::new(110.0, 60.0),
            DVec2::new(-10.0, 60.0),
        ];
        assert_eq!(crossings(&ring, &band), 4);
        // ... a quad inside it never crosses.
        let inner = square(40.0, 40.0, 20.0);
        assert_eq!(crossings(&ring, &inner), 0);
        assert_eq!(crossings(&inner, &ring), 0);
        // Disjoint rings neither.
        assert_eq!(crossings(&ring, &square(200.0, 200.0, 10.0)), 0);
    }

    #[test]
    fn stretch_reaches_back_without_touching_the_far_end() {
        let line = [DVec2::new(0.0, 0.0), DVec2::new(100.0, 0.0)];
        let quad = &corridor(&line, 10.0)[0];
        let stretched = stretch(quad, 50.0);
        // The start end moves back the way the track came ...
        assert!((stretched[0].x - (-60.0)).abs() < 1e-9, "{stretched:?}");
        assert!((stretched[3].x - (-60.0)).abs() < 1e-9);
        // ... and the far end stays where it was.
        assert!((stretched[1].x - 110.0).abs() < 1e-9, "{stretched:?}");
        assert!((stretched[2].x - 110.0).abs() < 1e-9);
    }
}
