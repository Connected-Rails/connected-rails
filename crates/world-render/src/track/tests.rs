//! What the track has to be true to. The dimensions themselves are checked in
//! `track_model::oberbau` — here it is about what the meshes make of them:
//! the gauge after the rails have been tipped 1:40, the bed's cross-section,
//! that every face points out of the solid it belongs to, and that the chunks
//! hang where the distance cull expects them.

use super::*;
use bevy::mesh::VertexAttributeValues;
use track_model::{EdgeId, NodeId, RailProfile, Segment};
use world_coords::geo::to_ecef_deg;

/// A 100 m straight edge at constant height.
fn straight_edge() -> TrackEdge {
    TrackEdge::new(
        EdgeId(0),
        NodeId(0),
        NodeId(1),
        to_ecef_deg(52.0, 10.0, 100.0),
        90.0f64.to_radians(),
        vec![Segment {
            len: 100.0,
            k0: 0.0,
            dk: 0.0,
        }],
    )
}

fn positions_of(mesh: &Mesh) -> Vec<[f32; 3]> {
    match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
        Some(VertexAttributeValues::Float32x3(positions)) => positions.to_vec(),
        _ => panic!("positions"),
    }
}

fn normals_of(mesh: &Mesh) -> Vec<[f32; 3]> {
    match mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
        Some(VertexAttributeValues::Float32x3(normals)) => normals.to_vec(),
        _ => panic!("normals"),
    }
}

fn uv1_of(mesh: &Mesh) -> Vec<[f32; 2]> {
    match mesh.attribute(Mesh::ATTRIBUTE_UV_1) {
        Some(VertexAttributeValues::Float32x2(uvs)) => uvs.to_vec(),
        _ => panic!("uv1"),
    }
}

fn indices_of(mesh: &Mesh) -> Vec<u32> {
    match mesh.indices() {
        Some(bevy::mesh::Indices::U32(indices)) => indices.to_vec(),
        _ => panic!("indices"),
    }
}

/// The track's own axes in render space, so a test can say "lateral" and
/// "height" instead of guessing which way the example edge happens to point.
struct Axes {
    right: Vec3,
    along: Vec3,
    up: Vec3,
    centre: Vec3,
}

impl Axes {
    fn of(e: &TrackEdge, frame: &EnuFrame) -> Self {
        let (centre, right, tangent, up) = cross_section(e, frame, 0.0);
        Self {
            right: Vec3::from(to_render(right)),
            along: Vec3::from(to_render(tangent)),
            up: Vec3::from(to_render(up)),
            centre: Vec3::from(to_render(centre)),
        }
    }

    /// `(lateral, along the track, height over the top of rail)` of a mesh
    /// vertex, given the offset its chunk is hung at.
    fn of_point(&self, p: [f32; 3], offset: Vec3) -> (f32, f32, f32) {
        let d = Vec3::from(p) + offset - self.centre;
        (d.dot(self.right), d.dot(self.along), d.dot(self.up))
    }
}

/// The bed is the DB Regelquerschnitt: its crest is level with the sleeper
/// tops (three centimetres under them), 2.6 m of sleeper plus a 0.40 m
/// shoulder each side, and the shoulder falls 1:1.5 to a Planum 696 mm under
/// the top of rail.
///
/// The crest height is the point of the whole test. A bed whose top lies at
/// the sleeper *underside* leaves the sleepers standing on a plate, which is
/// what the track looked like before and why it read as a ladder.
#[test]
fn the_ballast_bed_is_the_regelquerschnitt() {
    let edge = straight_edge();
    let frame = EnuFrame::at(edge.anchor);
    let ob = Oberbau::default();
    let axes = Axes::of(&edge, &frame);
    let mesh = ballast::build(&edge, &frame, 0.0, 100.0, &ob, 1.5);
    let points: Vec<(f32, f32, f32)> = positions_of(&mesh)
        .iter()
        .map(|p| axes.of_point(*p, Vec3::ZERO))
        .collect();

    // The crest: the highest points of the bed, three centimetres under the
    // sleeper top, and the wobble is a couple of centimetres at most.
    let crest = points.iter().map(|p| p.2).fold(f32::MIN, f32::max);
    let want_crest = -(ob.sleeper_top() + ob.crib_drop) as f32;
    assert!(
        (crest - want_crest).abs() < 0.04,
        "bed crest {crest} m under the rail, wanted {want_crest}"
    );
    assert!(
        crest > -(ob.sleeper_base() as f32),
        "the crest is under the sleepers — they stand on a plate"
    );

    // The toe: on the Planum, 696 mm down.
    let toe = points.iter().map(|p| p.2).fold(f32::MAX, f32::min);
    assert!(
        (toe + ob.planum() as f32).abs() < 1e-3,
        "bed toe {toe}, Planum {}",
        -ob.planum()
    );

    // Crest width: sleeper plus two shoulders. Measured on the points that
    // are up at crest height.
    let crest_half = points
        .iter()
        .filter(|p| p.2 > want_crest - 0.05)
        .map(|p| p.0.abs())
        .fold(0.0f32, f32::max);
    let want_half = (ob.sleeper_length / 2.0 + ob.ballast_overhang) as f32;
    assert!(
        (crest_half - want_half).abs() < 1e-3,
        "crest half width {crest_half}, wanted {want_half}"
    );

    // The shoulder falls 1:1.5 from there to the Planum.
    let toe_half = points
        .iter()
        .filter(|p| p.2 < toe + 0.01)
        .map(|p| p.0.abs())
        .fold(f32::MAX, f32::min);
    let fall = ob.planum() - ob.sleeper_top() - ob.crib_drop;
    let want_toe = want_half + (fall * ob.ballast_slope) as f32;
    assert!(
        (toe_half - want_toe).abs() < 1e-3,
        "shoulder toe {toe_half}, wanted {want_toe}"
    );
}

/// The rails stand 1435 mm apart between the inner head faces — measured
/// after the 1:40 has tipped them, because tipping them is what could move
/// the gauge and does not.
#[test]
fn the_rails_hold_the_gauge() {
    let edge = straight_edge();
    let frame = EnuFrame::at(edge.anchor);
    let axes = Axes::of(&edge, &frame);
    for profile in [RailProfile::R49, RailProfile::R54, RailProfile::R60] {
        let (near, _) = rail::build(&edge, &frame, profile);
        let points: Vec<(f32, f32, f32)> = near
            .iter()
            .flat_map(|(mesh, offset)| {
                positions_of(mesh)
                    .into_iter()
                    .map(|p| axes.of_point(p, *offset))
                    .collect::<Vec<_>>()
            })
            .collect();

        // The inner head faces sit exactly one gauge apart, 14 mm below the
        // top of rail: the head-side vertex nearest the measuring depth.
        let want = (GAUGE / 2.0) as f32;
        for sign in [-1.0f32, 1.0] {
            let best = points
                .iter()
                .filter(|p| (p.2 + track_model::GAUGE_MEASURE as f32).abs() < 1e-4)
                .map(|p| (sign * p.0 - want).abs())
                .fold(f32::MAX, f32::min);
            assert!(
                best < 1e-4,
                "{profile:?}: inner head face {sign} is {} mm off the gauge",
                best * 1000.0
            );
        }
    }
}

/// The 1:40 is a rotation of the whole section, not a shear of it. The proof
/// is the running surface: on a sheared rail it stays horizontal and both
/// rails carry the same flat band of light, on a rotated one it leans in by
/// 1:40 and the two rails catch the sun differently.
#[test]
fn the_rails_lean_towards_each_other() {
    let edge = straight_edge();
    let frame = EnuFrame::at(edge.anchor);
    let axes = Axes::of(&edge, &frame);
    let (near, _) = rail::build(&edge, &frame, RailProfile::R60);
    let (mesh, offset) = &near[0];
    let positions = positions_of(mesh);
    let normals = normals_of(mesh);
    let polish = uv1_of(mesh);

    // Over the running band the crown's own curvature is symmetric and
    // cancels, so what is left of the sideways lean is the cant itself.
    let mut lean = [(0.0f32, 0u32); 2];
    for (i, p) in positions.iter().enumerate() {
        if polish[i][0] < 0.5 {
            continue;
        }
        let (lateral, _, _) = axes.of_point(*p, *offset);
        let rail = usize::from(lateral > 0.0);
        lean[rail].0 += Vec3::from(normals[i]).dot(axes.right);
        lean[rail].1 += 1;
    }
    for (rail, (sum, count)) in lean.iter().enumerate() {
        assert!(*count > 8, "rail {rail} has no running band");
        let mean = sum / *count as f32;
        // The left rail (lateral < 0) leans towards +right, and the other
        // way round: the two heads lean towards each other.
        let want = if rail == 0 { 1.0 / 40.0 } else { -1.0 / 40.0 };
        assert!(
            (mean - want).abs() < 0.004,
            "rail {rail}: running surface leans {mean}, wanted {want}"
        );
    }
}

/// The shader is told which faces look across the gauge; if the flag ever
/// fired on the field side, the sunlit outside of the rail would be painted
/// as if it were in the head's own shadow.
#[test]
fn only_the_gauge_side_flank_is_flagged() {
    let edge = straight_edge();
    let frame = EnuFrame::at(edge.anchor);
    let axes = Axes::of(&edge, &frame);
    let (near, _) = rail::build(&edge, &frame, RailProfile::R60);
    let (mesh, offset) = &near[0];
    let positions = positions_of(mesh);
    let flags = uv1_of(mesh);

    let mut flagged = 0;
    for (i, p) in positions.iter().enumerate() {
        if flags[i][1] <= 0.0 {
            continue;
        }
        flagged += 1;
        let (lateral, _, height) = axes.of_point(*p, *offset);
        // Inside its own rail: between the rail's axis and the track centre.
        let axis = (GAUGE / 2.0 + 0.036) as f32;
        assert!(
            lateral.abs() < axis,
            "field side flagged as gauge flank at {lateral}"
        );
        // And on the head, not on the web or the foot.
        assert!(
            height > -0.05 && height < -0.010,
            "flank flag {height} m under the rail top"
        );
    }
    assert!(flagged > 8, "no gauge flank flagged at all");
}

/// Sleepers go in at the type's spacing — 60 cm on the Regeloberbau — and top
/// out just under the rail pad, sitting in the ballast bed.
#[test]
fn sleepers_keep_the_db_spacing() {
    let edge = straight_edge();
    let frame = EnuFrame::at(edge.anchor);
    let ob = Oberbau::default();
    let axes = Axes::of(&edge, &frame);
    let chunks = sleeper::build(&edge, &frame, 0.0, 10.0, &ob);
    assert_eq!(chunks.sleepers.len(), 1, "10 m is one chunk");

    let (mesh, offset) = &chunks.sleepers[0];
    let points: Vec<(f32, f32, f32)> = positions_of(mesh)
        .iter()
        .map(|p| axes.of_point(*p, *offset))
        .collect();

    // 10 m / 0.6 m = 16 full gaps, plus the first sleeper = 17. Each is one
    // ring of stations plus two end caps, so count them by their positions
    // along the track instead of by vertices.
    let mut along: Vec<f32> = points.iter().map(|p| p.1).collect();
    along.sort_by(f32::total_cmp);
    let rows = along.windows(2).filter(|w| w[1] - w[0] > 0.2).count() + 1;
    assert_eq!(rows, 17, "sleepers in 10 m at 60 cm");

    // The top face sits the rail's own height plus the pad under the top of
    // rail — bar the few millimetres of settling the wobble adds.
    let top = points.iter().map(|p| p.2).fold(f32::MIN, f32::max);
    assert!(
        (top + ob.sleeper_top() as f32).abs() < 0.006,
        "sleeper top {top}, wanted {}",
        -ob.sleeper_top()
    );
    // And the deepest point is the seat depth below that.
    let bottom = points.iter().map(|p| p.2).fold(f32::MAX, f32::min);
    assert!(
        (bottom + ob.sleeper_base() as f32).abs() < 0.008,
        "sleeper base {bottom}, wanted {}",
        -ob.sleeper_base()
    );
    // A B 70 is deeper at the rail seat than in the middle — that taper is
    // what tells a sleeper from an extruded bar.
    let middle_deep = points
        .iter()
        .filter(|p| p.0.abs() < 0.3)
        .map(|p| p.2)
        .fold(f32::MAX, f32::min);
    assert!(
        middle_deep > bottom + 0.03,
        "sleeper is the same depth end to end: {middle_deep} vs {bottom}"
    );
    // And it reaches the full 2.6 m across the track.
    let reach = points.iter().map(|p| p.0.abs()).fold(0.0f32, f32::max);
    assert!(
        (reach - (ob.sleeper_length / 2.0) as f32).abs() < 0.03,
        "sleeper reaches {reach} of {} m",
        ob.sleeper_length / 2.0
    );
}

/// The fastenings stand on the sleeper and reach up to the rail foot: below
/// the sleeper top there is nothing of them, and the clip's arm has to come
/// over the foot or it is holding nothing down.
#[test]
fn the_fastenings_stand_between_sleeper_and_rail() {
    let edge = straight_edge();
    let frame = EnuFrame::at(edge.anchor);
    let ob = Oberbau::default();
    let axes = Axes::of(&edge, &frame);
    let chunks = sleeper::build(&edge, &frame, 0.0, 10.0, &ob);
    let (mesh, offset) = &chunks.fastenings[0];
    let points: Vec<(f32, f32, f32)> = positions_of(mesh)
        .iter()
        .map(|p| axes.of_point(*p, *offset))
        .collect();

    let lowest = points.iter().map(|p| p.2).fold(f32::MAX, f32::min);
    let highest = points.iter().map(|p| p.2).fold(f32::MIN, f32::max);
    assert!(
        lowest > -(ob.sleeper_top() as f32) - 0.012,
        "a fastening reaches {lowest} — inside the sleeper"
    );
    // The clip arm has to sit over the rail foot: the foot's outer edge is
    // 11.5 mm thick and the pad holds it 10 mm clear of the sleeper.
    let foot_edge = -(ob.rail.dimensions().height - ob.rail.dimensions().foot_edge_thickness
        + ob.rail_pad) as f32;
    assert!(
        highest > foot_edge,
        "the clip tops out at {highest}, under the rail foot at {foot_edge}"
    );
    // And they stand where the rails do, not in the six-foot.
    let seats: Vec<f32> = points.iter().map(|p| p.0).collect();
    let axis = ob.rail_axis() as f32;
    assert!(
        seats.iter().all(|l| (l.abs() - axis).abs() < 0.25),
        "a fastening is nowhere near a rail seat"
    );
}

/// Every chunk hangs on its own centre, not on the edge anchor: only then
/// does the distance cull measure to the chunk the camera is passing. A chunk
/// built around the anchor vanishes as soon as the anchor is out of range —
/// with all its neighbours, mid-edge.
#[test]
fn chunks_are_hung_on_their_own_centre() {
    let edge = TrackEdge::new(
        EdgeId(0),
        NodeId(0),
        NodeId(1),
        to_ecef_deg(52.0, 10.0, 100.0),
        90.0f64.to_radians(),
        vec![Segment {
            len: 400.0,
            k0: 0.0,
            dk: 0.0,
        }],
    );
    let frame = EnuFrame::at(edge.anchor);
    let ob = Oberbau::default();
    let axes = Axes::of(&edge, &frame);

    let sleepers = sleeper::build(&edge, &frame, 0.0, 400.0, &ob);
    let (rails, _) = rail::build(&edge, &frame, RailProfile::R60);
    assert!(sleepers.sleepers.len() > 1, "400 m is more than one chunk");
    assert!(rails.len() > 1, "400 m of rail is more than one chunk");

    for (mesh, offset) in sleepers.sleepers.iter().chain(rails.iter()) {
        let along: Vec<f32> = positions_of(mesh)
            .iter()
            .map(|p| Vec3::from(*p).dot(axes.along))
            .collect();
        let (lo, hi) = along
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
        assert!(
            (lo + hi).abs() < 0.6,
            "chunk not centred on its own middle: {lo}..{hi}"
        );
        // And the offset is where that middle is on the track.
        let (lateral, _, _) = axes.of_point([0.0; 3], *offset);
        assert!(lateral.abs() < 0.05, "chunk centre {lateral} off the track");
    }
}

/// Every face of every solid points out of it, and every triangle is wound
/// with its face — one triangle of a quad wound backwards is culled and the
/// solid shows its inside.
#[test]
fn every_solid_face_points_outwards() {
    let edge = straight_edge();
    let frame = EnuFrame::at(edge.anchor);
    let axes = Axes::of(&edge, &frame);

    for ob in [
        Oberbau::default(),
        Oberbau {
            sleeper: SleeperKind::Wood,
            sleeper_width: 0.26,
            sleeper_height: 0.16,
            ..Oberbau::default()
        },
    ] {
        // One sleeper, so "outwards" is "away from its middle".
        let chunks = sleeper::build(&edge, &frame, 0.0, 0.1, &ob);
        for (mesh, _) in chunks.sleepers.iter().chain(chunks.fastenings.iter()) {
            let positions = positions_of(mesh);
            let normals = normals_of(mesh);
            for triangle in indices_of(mesh).chunks(3) {
                let p = |i: u32| Vec3::from(positions[i as usize]);
                let (a, b, c) = (p(triangle[0]), p(triangle[1]), p(triangle[2]));
                let winding = (b - a).cross(c - a);
                let n = Vec3::from(normals[triangle[0] as usize]);
                assert!(
                    winding.dot(n) > 0.0,
                    "{:?}: triangle wound against its normal",
                    ob.sleeper
                );
            }
        }

        // The sleeper is a closed solid: its faces point away from its middle.
        let (mesh, _) = &chunks.sleepers[0];
        let positions = positions_of(mesh);
        let normals = normals_of(mesh);
        let middle = positions
            .iter()
            .fold(Vec3::ZERO, |acc, p| acc + Vec3::from(*p))
            / positions.len() as f32;
        for (p, n) in positions.iter().zip(&normals) {
            let out = Vec3::from(*p) - middle;
            // Chamfers and the sleeper's own taper make a strict test wrong
            // for a vertex right on an edge; what must never happen is a face
            // pointing back into the solid.
            assert!(
                out.dot(Vec3::from(*n)) > -0.02,
                "sleeper face points inward at {out:?}"
            );
        }
    }

    // The rails: the running surface faces the sky.
    let (near, _) = rail::build(&edge, &frame, RailProfile::R60);
    let up = normals_of(&near[0].0)
        .iter()
        .map(|n| Vec3::from(*n).dot(axes.up))
        .fold(f32::MIN, f32::max);
    assert!(up > 0.99, "no face of the rail points at the sky: {up}");
}

/// The bed's strip is wound to face outwards — the other way round it is a
/// backface and the track is simply not there.
#[test]
fn the_track_bed_faces_outwards() {
    let edge = straight_edge();
    let frame = EnuFrame::at(edge.anchor);
    let axes = Axes::of(&edge, &frame);

    // Ballast: every face of it looks up at the sky, crest and shoulder both.
    let bed = ballast::build(&edge, &frame, 0.0, 20.0, &Oberbau::default(), 1.5);
    let positions = positions_of(&bed);
    for triangle in indices_of(&bed).chunks(3) {
        let p = |i: u32| Vec3::from(positions[i as usize]);
        let (a, b, c) = (p(triangle[0]), p(triangle[1]), p(triangle[2]));
        assert!(
            (b - a).cross(c - a).dot(axes.up) > 0.0,
            "ballast triangle faces down"
        );
    }

    // The slab has walls as well as a top, so the test there is that nothing
    // faces down and that the walls face away from the track.
    let ob = Oberbau {
        sleeper: SleeperKind::Slab,
        ..Oberbau::default()
    };
    let slab = ballast::build_slab(&edge, &frame, 0.0, 20.0, &ob, 1.5);
    let positions = positions_of(&slab);
    let mut walls = 0;
    for triangle in indices_of(&slab).chunks(3) {
        let p = |i: u32| Vec3::from(positions[i as usize]);
        let (a, b, c) = (p(triangle[0]), p(triangle[1]), p(triangle[2]));
        let normal = (b - a).cross(c - a).normalize();
        assert!(normal.dot(axes.up) > -1e-4, "slab triangle faces down");
        if normal.dot(axes.up).abs() < 0.1 {
            let lateral = axes.of_point(a.to_array(), Vec3::ZERO).0;
            assert!(
                normal.dot(axes.right) * lateral > 0.0,
                "slab wall faces into the slab"
            );
            walls += 1;
        }
    }
    assert!(walls > 0, "the slab has no sides");

    for mesh in [&bed, &slab] {
        // Tangents, or the normal map is silently ignored.
        assert!(
            mesh.attribute(Mesh::ATTRIBUTE_TANGENT).is_some(),
            "the bed has no tangents — the normal map would do nothing"
        );
    }
}

/// A rail chunk with both its ends capped is a **closed solid**, and the
/// volume it encloses is the rolled section times its length, twice over for
/// the two rails. That one number catches everything at once: a face wound
/// inside out, a cap that does not close, and a section that is not the
/// profile it claims to be.
#[test]
fn a_capped_rail_encloses_the_section_it_claims() {
    let edge = TrackEdge::new(
        EdgeId(0),
        NodeId(0),
        NodeId(1),
        to_ecef_deg(52.0, 10.0, 100.0),
        90.0f64.to_radians(),
        vec![Segment {
            len: 50.0,
            k0: 0.0,
            dk: 0.0,
        }],
    );
    let frame = EnuFrame::at(edge.anchor);
    for profile in [RailProfile::R49, RailProfile::R54, RailProfile::R60] {
        let (near, far) = rail::build(&edge, &frame, profile);
        assert_eq!(near.len(), 1, "50 m is one chunk, so both ends are capped");
        for (name, chunks) in [("near", &near), ("far", &far)] {
            let (mesh, _) = &chunks[0];
            let positions = positions_of(mesh);
            let mut volume = 0.0f64;
            for triangle in indices_of(mesh).chunks(3) {
                let p = |i: u32| Vec3::from(positions[i as usize]).as_dvec3();
                let (a, b, c) = (p(triangle[0]), p(triangle[1]), p(triangle[2]));
                volume += a.dot(b.cross(c)) / 6.0;
            }
            // The near level of detail is the rolled section, so its volume
            // is the profile's own kilograms per metre; the coarse one is
            // deliberately a plain envelope and is only checked for being a
            // closed solid of about the right size.
            let section = profile.dimensions().mass / track_model::oberbau::RAIL_STEEL_DENSITY;
            let want = 2.0 * section * edge.length();
            let slack = if name == "near" { 0.02 } else { 0.15 };
            assert!(
                (volume - want).abs() < want * slack,
                "{profile:?} {name}: encloses {volume:.4} m³, section says {want:.4}"
            );
        }
    }
}

/// A tighter curve is sampled more finely: a fixed step leaves a visible kink
/// in the rail all the way round a bend.
#[test]
fn curves_are_sampled_by_their_radius() {
    let curved = TrackEdge::new(
        EdgeId(0),
        NodeId(0),
        NodeId(1),
        to_ecef_deg(52.0, 10.0, 100.0),
        0.0,
        vec![Segment {
            len: 200.0,
            k0: 1.0 / 300.0,
            dk: 0.0,
        }],
    );
    let straight = straight_edge();
    let frame = EnuFrame::at(curved.anchor);
    let rows = |e: &TrackEdge| {
        let (near, _) = rail::build(e, &frame, RailProfile::R60);
        near.iter()
            .map(|(m, _)| positions_of(m).len())
            .sum::<usize>() as f64
            / e.length()
    };
    assert!(
        rows(&curved) > rows(&straight) * 2.0,
        "a 300 m curve is sampled like a straight"
    );
}
