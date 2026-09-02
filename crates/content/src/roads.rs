//! Roads as ground to look at: centre line in, draped carriageway out.
//!
//! A [`crate::route::RoadSource`] is what OSM maps a street as — a centre
//! line with a class (`highway=*`), plus the width, surface and markings that
//! turn the line into a carriageway. What the run needs is a ribbon of
//! carriageway lying on the terrain: the width either side of the line, the
//! surface texture of it, the markings a German road carries.
//!
//! The centre line becomes a **ribbon**: a half-width to each side of it, one
//! quad per segment of the way, mitered where two segments meet so the quads
//! share their corners exactly. Each quad is then cut into cells as fine as
//! the height grid it is draped on, cut to the terrain tiles and laid on the
//! ground exactly as the fields are — a road of two kilometres crosses a
//! dozen tiles and is streamed as one patch per tile it touches, and the
//! pieces two neighbouring tiles cut out of one road meet without a seam.
//!
//! A ribbon and not one buffered outline, because a curve offset to both
//! sides in one piece *crosses itself* on the inside of the bend, and a ring
//! that crosses itself is what neither the polygon clip nor the ear clipping
//! can take: torn kerbs, stray triangles, markings in the wrong lane. Segment
//! by segment the shape is a trapezoid that cannot cross itself, and the
//! metre along the road and across it is known at every corner by
//! construction instead of being searched for afterwards.
//!
//! What the run needs beyond the shape rides in the mesh: the markings (which
//! of them to draw) and the half-width travel in the vertex colours, the
//! metre across and the metre along in the UVs.
//!
//! A road flagged as a bridge (`bridge=*`) flies: where the ground dips
//! below the straight line between the way's own ends, the carriageway holds
//! that line — the deck — instead of following the hollow, and its ends are
//! measured on the shaped ground, so the deck meets the drape at the
//! abutments and both tiles at a seam cut the same chord.
//!
//! Nothing here knows what asphalt looks like; the renderer's shader makes
//! the markings out of the vertex colours and the wear out of the weather.
//! So a module carries no road bitmaps, and two clients of a multiplayer run
//! agree on what a road looks like without a byte crossing the network.

use crate::route::{CenterLine, LineSource, RoadSource, RoadSurface};
use crate::terrain::{HeightGrid, TileKey};
use glam::{DVec2, DVec3, Vec3};
use std::collections::HashMap;
use world_coords::{EnuFrame, geo};

/// How far a road's surface is lifted off the terrain [m].
///
/// It has to clear not the ground but everything else draped on it: the
/// fields lie a hand's width up ([`crate::farmland::LIFT`]) and are cut into
/// triangles of their own, so their surface stands centimetres off the height
/// grid wherever it is not flat. A road that only cleared the grid would win
/// half a field's triangles and lose the other half, which is the speckle a
/// draped surface shows when two meshes share a depth value. A hand's width
/// is invisible from a cab and decides the question.
pub const LIFT: f64 = 0.12;

/// The step of the road's own mesh, from the step of the height grid it is
/// draped on [m]. Half the grid, so the carriageway follows every fold the
/// ground under it has — and no finer, because it cannot follow what the grid
/// does not carry. Bounded so a coarse LOD does not pay for a fine road and a
/// fine LOD does not build a vertex per centimetre.
fn drape_step(grid_step: f64) -> f64 {
    (grid_step * 0.5).clamp(1.0, 8.0)
}

/// The most cells one segment of a road is cut into, along it and across it.
/// A guard against a broken file, not a budget: at [`drape_step`] it takes a
/// 500 m OSM segment or a 250 m carriageway to reach it.
const MAX_CELLS: usize = 256;

/// How far past the other carriageway a junction keeps the markings off [m].
/// A stripe that stops exactly at the crossing road's kerb still reads as
/// running into it; a metre of clear asphalt is what a junction looks like.
const MARK_CLEAR: f64 = 1.0;

/// How long the markings take to come back after a junction [m]. Short — a
/// road mark starts, it does not dawn — but not nothing, or the line switches
/// on at whichever vertex the tessellation happened to put there.
const MARK_FADE: f64 = 1.5;

/// The shallowest crossing a junction is measured at [as the sine of the
/// angle]. Two carriageways that meet at a hand's breadth cover each other
/// for an arithmetically endless stretch; past this the blank stops growing.
const MARK_GRAZE: f64 = 0.4;

/// How straight on two ways have to run at a shared point to count as one
/// road rather than a junction [as the cosine of the angle between them].
/// About 45°: an extract splits a street at every change of tagging and at
/// every side road, so most of the points where two ways touch are joins, not
/// junctions — and a street that lost its markings at each of them would be a
/// dashed street rather than a dashed line.
const JOIN_COS: f64 = 0.7;

/// How many layers the carriageways are sorted into, and how far apart they
/// lie [m]. Roads that cover each other — a junction, a slip road that runs
/// beside the road it leaves, two ways of one street that overlap — go on
/// different layers, so the one on top is on top everywhere instead of
/// trading fragments with the other along a torn edge. Eight is more than a
/// German road network needs; a millimetre apiece is under what a cab sees.
const LAYERS: usize = 12;
const LAYER_LIFT: f64 = 0.006;

/// The road presets the editor offers — the widths of the German road system,
/// from the Autobahn carriageway down to the footpath, asphalt, concrete and
/// the gravel of the field tracks, with and without the centre line. The widths are the planning values of
/// the German road system (RASt-06, rounded to what a builder will actually
/// pick); each carriageway of a divided road is its own preset, because OSM
/// maps the two directions of an Autobahn as their own ways.
//
// ponytail: the widths are planning values, not a law of nature — a 1970s
// Kreisstraße is 5.5 m where the rulebook wanted 6.5. The presets are a
// starting point; the width stays editable in the panel, as it is.
pub struct RoadPreset {
    /// The i18n key suffix (`road-preset-<id>`).
    pub id: &'static str,
    /// Carriageway width, kerb to kerb [m].
    pub width: f64,
    pub surface: RoadSurface,
    pub center_line: CenterLine,
    pub edge_lines: bool,
}

/// The roads a German module meets, in the order a driver meets them. Each
/// carriageway of a divided road is its own preset — OSM maps them as their
/// own ways, so the Autobahn preset is one *Fahrbahn*.
pub const PRESETS: &[RoadPreset] = &[
    RoadPreset {
        id: "motorway-3",
        width: 15.0,
        surface: RoadSurface::Asphalt,
        center_line: CenterLine::None,
        edge_lines: true,
    },
    RoadPreset {
        id: "motorway",
        width: 11.0,
        surface: RoadSurface::Asphalt,
        center_line: CenterLine::None,
        edge_lines: true,
    },
    RoadPreset {
        id: "motorway-concrete",
        width: 11.0,
        surface: RoadSurface::Concrete,
        center_line: CenterLine::None,
        edge_lines: true,
    },
    RoadPreset {
        id: "federal",
        width: 7.5,
        surface: RoadSurface::Asphalt,
        center_line: CenterLine::Dashed,
        edge_lines: true,
    },
    RoadPreset {
        id: "federal-solid",
        width: 7.0,
        surface: RoadSurface::Asphalt,
        center_line: CenterLine::Solid,
        edge_lines: true,
    },
    RoadPreset {
        id: "secondary",
        width: 6.5,
        surface: RoadSurface::Asphalt,
        center_line: CenterLine::Dashed,
        edge_lines: true,
    },
    RoadPreset {
        id: "residential",
        width: 5.5,
        surface: RoadSurface::Asphalt,
        center_line: CenterLine::DashedUrban,
        edge_lines: true,
    },
    RoadPreset {
        id: "residential-narrow",
        width: 4.5,
        surface: RoadSurface::Asphalt,
        center_line: CenterLine::None,
        edge_lines: true,
    },
    RoadPreset {
        id: "living",
        width: 3.0,
        surface: RoadSurface::Asphalt,
        center_line: CenterLine::None,
        edge_lines: false,
    },
    RoadPreset {
        id: "service",
        width: 3.5,
        surface: RoadSurface::Asphalt,
        center_line: CenterLine::None,
        edge_lines: false,
    },
    RoadPreset {
        id: "farm-gravel",
        width: 3.0,
        surface: RoadSurface::Gravel,
        center_line: CenterLine::None,
        edge_lines: false,
    },
    RoadPreset {
        id: "farm-concrete",
        width: 3.0,
        surface: RoadSurface::Concrete,
        center_line: CenterLine::None,
        edge_lines: false,
    },
    RoadPreset {
        id: "path",
        width: 2.0,
        surface: RoadSurface::Asphalt,
        center_line: CenterLine::None,
        edge_lines: false,
    },
];

/// The preset an id names.
pub fn preset(id: &str) -> Option<&'static RoadPreset> {
    PRESETS.iter().find(|p| p.id == id)
}

/// One preset as a road entry — what the editor's tool stamps.
pub fn preset_source(preset: &RoadPreset) -> RoadSource {
    RoadSource {
        name: String::new(),
        points: Vec::new(),
        width: preset.width,
        surface: preset.surface,
        center_line: preset.center_line,
        edge_lines: preset.edge_lines,
        bridge: false,
        tags: Vec::new(),
    }
}

/// The roads of a line, indexed by the terrain tiles they reach.
#[derive(Debug, Clone, Default)]
pub struct Roads {
    /// Per tile, the roads whose carriageway reaches it.
    by_tile: HashMap<TileKey, Vec<usize>>,
    roads: Vec<Road>,
    /// The merged surfaces where roads meet ([`merge_junctions`]), and the
    /// tiles each of them reaches.
    junctions: Vec<Junction>,
    junctions_by_tile: HashMap<TileKey, Vec<usize>>,
}

/// One road, ready to be cut up: the centre line, the two kerbs beside it,
/// and what the shader needs to know about it.
#[derive(Debug, Clone)]
struct Road {
    /// The centre line [m UTM] — the geometry the markings are measured
    /// against.
    centre: Vec<DVec2>,
    /// Arc length at each centre point [m], from the road's own start — the
    /// dash phase runs in it, and the bridge chord is measured on it.
    s: Vec<f64>,
    /// The kerbs, one point per centre point: `left` a half-width to the
    /// right-hand side of the direction of travel, `right` to the left of it
    /// — the sides `u = 0` and `u = 1` run along. Mitered where two edges
    /// meet, so the carriageway is one gap-free ribbon of quads and never the
    /// self-crossing outline a curve buffered in one piece would be.
    left: Vec<DVec2>,
    right: Vec<DVec2>,
    surface: RoadSurface,
    center_line: CenterLine,
    edge_lines: bool,
    /// Whether the way flies (`bridge=*`): where the ground dips below the
    /// line between the way's own ends, the carriageway holds that line —
    /// the deck of a bridge — instead of following the hollow.
    bridge: bool,
    /// Half the carriageway width [m], as the file said it — clamped only
    /// where a bad file would make the kerbs explode.
    half: f64,
    /// The road's own share of the lift off the ground [m] — its layer times
    /// [`LAYER_LIFT`], so two carriageways that cover each other never fight
    /// over the same fragment, on any machine (see [`layers`]).
    lift: f64,
    /// Spans of this road's own arc length [m] where the **edge lines** do
    /// not run: the mouth of every other carriageway that crosses or joins
    /// it. Sorted and merged.
    blank_edges: Vec<Span>,
    /// The same for the **centre line**, and only against a road at least as
    /// wide: a field track crossing a Bundesstraße breaks its edge line, as
    /// it does on the ground, but the through line keeps running.
    blank_centre: Vec<Span>,
    /// Spans of arc length the ribbon is **not** drawn over: a junction has
    /// taken that ground and carries it as one surface of its own
    /// ([`merge_junctions`]). Sorted and merged.
    holes: Vec<Span>,
    /// Index in [`LineSource::roads`] — what the editor selects.
    index: u32,
}

impl Road {
    /// Length of the centre line [m].
    fn length(&self) -> f64 {
        self.s.last().copied().unwrap_or(0.0)
    }

    /// The unit direction of segment `i` — the axis the deck's slope is read
    /// along, and the axis the ribbon's `v` runs on.
    fn direction(&self, i: usize) -> DVec2 {
        (self.centre[i + 1] - self.centre[i]).normalize_or_zero()
    }

    /// The segment an arc length falls in, and how far along it.
    fn segment_at(&self, s: f64) -> (usize, f64) {
        let last = self.centre.len() - 2;
        let i = self
            .s
            .partition_point(|at| *at <= s)
            .saturating_sub(1)
            .min(last);
        let span = self.s[i + 1] - self.s[i];
        let t = if span > 1e-9 {
            ((s - self.s[i]) / span).clamp(0.0, 1.0)
        } else {
            0.0
        };
        (i, t)
    }

    /// One kerb of the road at an arc length — the *mitered* kerb the ribbon
    /// itself is built on, not a line offset from the centre. A junction has
    /// to meet the carriageway on the very point the ribbon starts at, or a
    /// crescent of ground shows between them wherever the road bends.
    fn kerb_at(&self, right_side: bool, s: f64) -> DVec2 {
        let kerb = if right_side { &self.right } else { &self.left };
        let (i, t) = self.segment_at(s);
        kerb[i].lerp(kerb[i + 1], t)
    }

    /// The kerb from one arc length to another, ends included and every
    /// corner of the way in between — the outline of a junction follows it
    /// rather than cutting the corner.
    fn kerb_between(&self, right_side: bool, from: f64, to: f64) -> Vec<DVec2> {
        let kerb = if right_side { &self.right } else { &self.left };
        let mut out = vec![self.kerb_at(right_side, from)];
        let (lo, hi) = (from.min(to), from.max(to));
        let mut inner: Vec<DVec2> = self
            .s
            .iter()
            .enumerate()
            .filter(|(_, at)| **at > lo + 1e-6 && **at < hi - 1e-6)
            .map(|(i, _)| kerb[i])
            .collect();
        if from > to {
            inner.reverse();
        }
        out.append(&mut inner);
        out.push(self.kerb_at(right_side, to));
        out
    }
}

impl Roads {
    pub fn from_line(line: &LineSource, zone: u8, tile_size: f64) -> Self {
        Self::from_parts(&line.roads, zone, tile_size)
    }

    pub fn from_parts(sources: &[RoadSource], zone: u8, tile_size: f64) -> Self {
        let mut out = Roads::default();
        for (index, source) in sources.iter().enumerate() {
            let centre: Vec<DVec2> = source
                .points
                .iter()
                .map(|p| {
                    let (e, n) = geo::to_utm(p.lat.to_radians(), p.lon.to_radians(), zone);
                    DVec2::new(e, n)
                })
                .collect();
            let centre = dedupe(centre);
            if centre.len() < 2 {
                continue;
            }
            let s = arc_lengths(&centre);
            let half = (source.width.max(0.5) / 2.0).min(15.0);
            let (left, right) = kerbs(&centre, half);
            let at = out.roads.len();
            // Tile by tile along the road, not one box around all of it: a
            // road that crosses a module on the diagonal touches a handful of
            // tiles and would otherwise claim every tile of its bounding box,
            // and the builder would cut it against each of them for nothing.
            for i in 0..centre.len() - 1 {
                let (lo, hi) =
                    fields::geometry::bounds(&[left[i], left[i + 1], right[i], right[i + 1]]);
                let grow = DVec2::splat(1.0);
                let (kx0, ky0) = key(lo - grow, tile_size);
                let (kx1, ky1) = key(hi + grow, tile_size);
                for ky in ky0..=ky1 {
                    for kx in kx0..=kx1 {
                        let on = out.by_tile.entry((kx, ky)).or_default();
                        if on.last() != Some(&at) {
                            on.push(at);
                        }
                    }
                }
            }
            out.roads.push(Road {
                centre,
                s,
                left,
                right,
                surface: source.surface,
                center_line: source.center_line,
                edge_lines: source.edge_lines,
                bridge: source.bridge,
                half,
                lift: 0.0,
                blank_edges: Vec::new(),
                blank_centre: Vec::new(),
                holes: Vec::new(),
                index: index as u32,
            });
        }
        junctions(&mut out.roads);
        out.junctions = merge_junctions(&mut out.roads);
        for (at, junction) in out.junctions.iter().enumerate() {
            let (lo, hi) = fields::geometry::bounds(&junction.outline);
            let grow = DVec2::splat(1.0);
            let (kx0, ky0) = key(lo - grow, tile_size);
            let (kx1, ky1) = key(hi + grow, tile_size);
            for ky in ky0..=ky1 {
                for kx in kx0..=kx1 {
                    out.junctions_by_tile.entry((kx, ky)).or_default().push(at);
                }
            }
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.roads.is_empty()
    }

    pub fn len(&self) -> usize {
        self.roads.len()
    }

    /// Whether any road reaches this tile — the cheap question the tile
    /// builder asks before doing any of the work below.
    pub fn touches(&self, k: TileKey) -> bool {
        self.by_tile.contains_key(&k) || self.junctions_by_tile.contains_key(&k)
    }
}

/// One road's carriageway on one tile, in the tile's own frame — all the
/// roads of one surface on the tile in one patch, so a tile costs one draw
/// per surface it carries.
#[derive(Debug, Clone, PartialEq)]
pub struct RoadPatch {
    pub surface: RoadSurface,
    /// Render axes (x = east, y = up, z = −north), relative to the tile anchor.
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// `u` across the carriageway (0 at one kerb, 1 at the other, and
    /// *straight* in between — the shader multiplies it by the width to get
    /// the metre across, so a u that is not the position across stretches the
    /// surface and paints the edge lines into the driving lane), `v` along
    /// the road in metres from its own start — the dash phase runs in it, so
    /// the markings of one road line up across the tile boundaries it
    /// crosses, and the texture repeats without a seam between the tiles.
    pub uvs: Vec<[f32; 2]>,
    /// Per-vertex data: `r` the centre line ([`crate::route::CenterLine`] as
    /// a number — 1 dashed außerorts, 2 dashed innerorts, 3 solid), `g` how
    /// much of the edge lines runs here, `b` the half-width [m], `a` how much
    /// of the centre line runs here — everything the shader needs to draw the
    /// markings of the road this vertex belongs to, and to stop them where a
    /// junction breaks them ([`junctions`]).
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
    /// The roads that went into this patch, in line order — what a click on
    /// it selects, and what the editor highlights.
    pub sources: Vec<u32>,
}

impl RoadPatch {
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }
}

/// How close two meeting points have to be to be the same node [m]. An
/// extract shares its nodes exactly, so this only has to catch the rounding
/// a way's coordinates went through on the way to UTM.
const NODE_SNAP: f64 = 0.6;

/// How far the merged surface of a junction reaches past the kerb of the
/// carriageway it took over [m]. The two meet along one line, but they are
/// draped on the height grid separately, and a hairline of ground between
/// them would show. A finger's overlap and the junction on the layer above
/// closes it for good.
const JUNCTION_LAP: f64 = 0.1;

/// The kerb radius a junction's corners are rounded with [m], from the width
/// of the narrower of the two roads that make the corner. A German
/// Eckausrundung is 6 m and more on a main road; the smaller radius here is
/// what keeps a junction from eating the roads that lead into it when they
/// are short, which OSM ways beside a junction usually are.
fn corner_radius(narrower_width: f64) -> f64 {
    (narrower_width * 0.5).clamp(2.0, 8.0)
}

/// The most a junction may take of a road either side of its node [m] — a
/// junction that ate a way whole would leave a hole in the network.
const JUNCTION_REACH: f64 = 25.0;

/// How much of the road between two nodes one of them may take [as a share].
/// Under a half, so two junctions on one stretch always leave a piece of road
/// between them — which is what keeps a village roundabout, whose mouths are
/// a few metres apart, from being eaten by its own junctions.
const JUNCTION_ROOM: f64 = 0.45;

/// How near in width the roads at a node have to be for their surfaces to be
/// merged [as a share of the widest].
///
/// Peers make a junction: a crossroads of two Landstraßen is one square of
/// asphalt, and neither of them carries a marking across it. A field track
/// running out on a Bundesstraße is not that — the B-road runs *through*, its
/// Leitlinie with it, and only the track's mouth opens the Randlinie. Merging
/// there would take the through line out for the width of a track, which is
/// not what the road is marked like, so those are left to the layering
/// instead: the track's mouth lies on the road rather than in it.
const JUNCTION_PEER: f64 = 0.6;

/// One road arriving at a node: which road, where on it, and which way it
/// leaves. A road that ends at the node has one arm; a road that runs
/// through it has two, one each way.
#[derive(Debug, Clone, Copy)]
struct Arm {
    road: usize,
    /// Arc length on the road at the node [m].
    at: f64,
    /// Unit direction away from the node, along the centre line — what the
    /// arms are sorted round the node by.
    away: DVec2,
    /// Which way the road's own arc length runs from the node.
    forward: bool,
    half: f64,
    /// The road's own kerbs at the node, left and right of the way out, each
    /// as the point and the direction it runs from there.
    left: (DVec2, DVec2),
    right: (DVec2, DVec2),
}

impl Arm {
    /// Which of the road's two kerbs is on the arm's left. A road walked
    /// backwards has them the other way round.
    fn right_side(&self, arm_left: bool) -> bool {
        arm_left == self.forward
    }

    /// The arc length a distance out along the arm.
    fn out(&self, distance: f64) -> f64 {
        if self.forward {
            self.at + distance
        } else {
            self.at - distance
        }
    }
}

/// A place where carriageways meet, drawn as **one** surface: the roads stop
/// at its edge and it carries the ground between them itself, so a junction
/// is a square of asphalt rather than two ribbons lying over one another.
#[derive(Debug, Clone)]
pub(crate) struct Junction {
    surface: RoadSurface,
    /// The outline, counter-clockwise [m UTM].
    outline: Vec<DVec2>,
    lift: f64,
    /// The roads that made it, in line order — what a click on it selects.
    sources: Vec<u32>,
}

/// Merges the carriageways where they meet: finds the nodes of the road
/// network, gives each one a surface of its own, and takes that stretch out
/// of the roads that lead into it.
///
/// This is what tells a junction from an overlap. Two ribbons crossing are
/// two surfaces at slightly different heights, and however carefully they are
/// layered the eye still reads the seam where one ends inside the other. One
/// surface has no seam: the roads are cut back to its edge, its corners are
/// rounded the way a kerb is, and what is left is the shape a junction has.
///
/// Anything the construction cannot vouch for is left alone — a junction with
/// a bridge in it, an outline that crosses itself, a stretch longer than the
/// road that leads into it — and those places fall back on the layered
/// overlap, which is untidy but never wrong.
fn merge_junctions(roads: &mut [Road]) -> Vec<Junction> {
    let found = nodes(roads);
    // Where every road meets a node, so a junction can tell how much of the
    // road it may take before it runs into the next one.
    let mut on_road: Vec<Vec<f64>> = vec![Vec::new(); roads.len()];
    for (_, arms) in &found {
        for arm in arms {
            on_road[arm.road].push(arm.at);
        }
    }
    for road in &mut on_road {
        road.sort_by(f64::total_cmp);
        road.dedup_by(|a, b| (*a - *b).abs() < 0.01);
    }

    let mut junctions = Vec::new();
    let mut holes: Vec<Vec<Span>> = vec![Vec::new(); roads.len()];
    for (at, arms) in found {
        if arms.len() < 2 || arms.iter().any(|arm| roads[arm.road].bridge) {
            continue;
        }
        // Two arms running straight through one another are a way split, not
        // a junction — an extract is full of them.
        if arms.len() == 2 && arms[0].away.dot(arms[1].away) < -JOIN_COS {
            continue;
        }
        let (narrowest, widest) = arms.iter().fold((f64::MAX, 0.0f64), |(lo, hi), arm| {
            (lo.min(arm.half), hi.max(arm.half))
        });
        if narrowest < JUNCTION_PEER * widest {
            continue;
        }
        // How far each arm can be cut back before it reaches the next node
        // on its road. A junction that took more than the road between two of
        // them would leave nothing of the road at all.
        let rooms: Vec<f64> = arms
            .iter()
            .map(|arm| room(&on_road[arm.road], roads[arm.road].length(), arm))
            .collect();
        let Some((outline, trims)) = junction_outline(roads, at, &arms, &rooms) else {
            continue;
        };
        // What each road loses to the junction. A stretch it cannot spare, or
        // one already taken by a neighbouring junction, and the whole node is
        // left to the layering instead.
        let mut taken: Vec<(usize, Span)> = Vec::new();
        for (arm, trim) in arms.iter().zip(&trims) {
            let road = &roads[arm.road];
            let far = if arm.forward {
                arm.at + trim
            } else {
                arm.at - trim
            };
            let span = (arm.at.min(far), arm.at.max(far));
            if *trim > JUNCTION_REACH || span.1 - span.0 > road.length() * 0.6 {
                taken.clear();
                break;
            }
            if holes[arm.road]
                .iter()
                .any(|(lo, hi)| span.0 < *hi && *lo < span.1)
            {
                taken.clear();
                break;
            }
            taken.push((arm.road, span));
        }
        if taken.is_empty() {
            continue;
        }
        // The widest road decides what the junction is paved with, and it
        // sits one layer above everything it took over, so the finger of
        // overlap at each kerb is never a hairline of ground.
        let widest = arms
            .iter()
            .max_by(|a, b| a.half.total_cmp(&b.half))
            .expect("checked non-empty");
        let mut sources: Vec<u32> = taken.iter().map(|(i, _)| roads[*i].index).collect();
        sources.sort_unstable();
        sources.dedup();
        let lift = taken
            .iter()
            .map(|(i, _)| roads[*i].lift)
            .fold(0.0f64, f64::max)
            + LAYER_LIFT;
        junctions.push(Junction {
            surface: roads[widest.road].surface,
            outline,
            lift,
            sources,
        });
        for (road, span) in taken {
            holes[road].push(span);
        }
    }
    for (road, holes) in roads.iter_mut().zip(holes) {
        road.holes = merge_spans(holes);
        // The markings keep out of what the junction took, whatever the
        // mouths worked out on their own.
        let clear: Vec<Span> = road
            .holes
            .iter()
            .map(|(lo, hi)| (lo - MARK_CLEAR, hi + MARK_CLEAR))
            .collect();
        road.blank_edges = merge_spans([road.blank_edges.clone(), clear.clone()].concat());
        road.blank_centre = merge_spans([road.blank_centre.clone(), clear].concat());
    }
    junctions
}

/// The nodes of the road network and the arms that arrive at each: every
/// place a way ends, and every place two centre lines cross. Roads that
/// merely run *through* such a point join it too — a side road ending on a
/// through road shares its node, and the through road has to be part of the
/// junction it makes.
fn nodes(roads: &[Road]) -> Vec<(DVec2, Vec<Arm>)> {
    let mut meeting: Vec<DVec2> = Vec::new();
    for road in roads {
        meeting.push(road.centre[0]);
        meeting.push(road.centre[road.centre.len() - 1]);
    }
    for a in 0..roads.len() {
        for b in a + 1..roads.len() {
            for i in 0..roads[a].centre.len() - 1 {
                for j in 0..roads[b].centre.len() - 1 {
                    let (p0, p1) = (roads[a].centre[i], roads[a].centre[i + 1]);
                    let (q0, q1) = (roads[b].centre[j], roads[b].centre[j + 1]);
                    if let Some((t, _)) = crossing(p0, p1, q0, q1) {
                        meeting.push(p0 + (p1 - p0) * t);
                    }
                }
            }
        }
    }
    // One node per cluster of meeting points, in a fixed order — the same
    // line gives the same junctions on every machine.
    meeting.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    let mut out: Vec<(DVec2, Vec<Arm>)> = Vec::new();
    for point in meeting {
        if out
            .last()
            .is_some_and(|(seen, _)| seen.distance(point) < NODE_SNAP)
        {
            continue;
        }
        let arms = arms_at(roads, point);
        if arms.len() >= 2 {
            out.push((point, arms));
        }
    }
    out
}

/// How much of a road an arm may be cut back by: the stretch from the node to
/// the next one along it, or to the road's own end.
fn room(nodes: &[f64], length: f64, arm: &Arm) -> f64 {
    let next = if arm.forward {
        nodes
            .iter()
            .find(|at| **at > arm.at + 0.01)
            .copied()
            .unwrap_or(length)
    } else {
        nodes
            .iter()
            .rev()
            .find(|at| **at < arm.at - 0.01)
            .copied()
            .unwrap_or(0.0)
    };
    (next - arm.at).abs()
}

/// The arms of every road that reaches a node.
fn arms_at(roads: &[Road], node: DVec2) -> Vec<Arm> {
    let mut out = Vec::new();
    for (index, road) in roads.iter().enumerate() {
        let Some((distance, at, segment, t)) = nearest_on(road, node) else {
            continue;
        };
        if distance > NODE_SNAP {
            continue;
        }
        let last = road.centre.len() - 2;
        // Which segment each way out of the node runs along. At a corner of
        // the way the two are different, and taking the wrong one puts the
        // arm's kerb at an angle to the ribbon that leaves it.
        let ahead = if t > 0.98 && segment < last {
            segment + 1
        } else {
            segment
        };
        let behind = if t < 0.02 && segment > 0 {
            segment - 1
        } else {
            segment
        };
        let arm = |forward: bool, seg: usize| {
            let away = if forward {
                road.direction(seg)
            } else {
                -road.direction(seg)
            };
            // The kerb of the segment the arm runs along, from the node out.
            let kerb = |right_side: bool| {
                let line = if right_side { &road.right } else { &road.left };
                let (from, to) = if forward {
                    (road.kerb_at(right_side, at), line[seg + 1])
                } else {
                    (road.kerb_at(right_side, at), line[seg])
                };
                (from, (to - from).normalize_or_zero())
            };
            // `arm_left` is left of the way *out*, which is the road's right
            // when the arm runs forward and its left when it runs back.
            Arm {
                road: index,
                at,
                away,
                forward,
                half: road.half,
                left: kerb(forward),
                right: kerb(!forward),
            }
        };
        if !(segment == last && t > 0.98) {
            out.push(arm(true, ahead));
        }
        if !(segment == 0 && t < 0.02) {
            out.push(arm(false, behind));
        }
    }
    out
}

/// The outline of a junction and how far back each arm's road is cut, or
/// `None` where the arms make no shape worth trusting.
///
/// Round the node counter-clockwise: out along each arm's right kerb to where
/// its road is cut, across, back along its left kerb, and round the corner to
/// the next arm. The corner is where the two kerbs meet, rounded off with the
/// radius a kerb has — which is the difference between a junction and two
/// rectangles laid across one another.
fn junction_outline(
    roads: &[Road],
    node: DVec2,
    arms: &[Arm],
    rooms: &[f64],
) -> Option<(Vec<DVec2>, Vec<f64>)> {
    let mut order: Vec<usize> = (0..arms.len()).collect();
    order.sort_by(|&a, &b| {
        let bearing = |arm: &Arm| arm.away.y.atan2(arm.away.x);
        bearing(&arms[a]).total_cmp(&bearing(&arms[b]))
    });

    // Corner `i` joins the left kerb of arm `order[i]` to the right kerb of
    // the next one round, over the wedge of grass between them. Two arms
    // straight through one another have no corner — their kerbs are the same
    // line, and the outline runs along it.
    let mut meets: Vec<Option<(DVec2, f64)>> = Vec::with_capacity(order.len());
    for i in 0..order.len() {
        let (here, next) = (&arms[order[i]], &arms[order[(i + 1) % order.len()]]);
        let Some(point) = lines_meet(here.left.0, here.left.1, next.right.0, next.right.1) else {
            meets.push(None);
            continue;
        };
        if (point - node).length() > JUNCTION_REACH {
            return None;
        }
        // The grass wedge, counter-clockwise from one arm to the next.
        let bearing = |d: DVec2| d.y.atan2(d.x);
        let mut gap = bearing(next.away) - bearing(here.away);
        if gap < 0.0 {
            gap += std::f64::consts::TAU;
        }
        meets.push(Some((point, gap)));
    }

    // How far back each road is cut before the corners are rounded, and how
    // much further it could go before it ran into the next node along it.
    // The kerb radius is what has to give where two junctions are close —
    // a village roundabout is a handful of mouths a few metres apart.
    let reach = |arm: &Arm, point: &DVec2| (*point - node).dot(arm.away);
    let mut trims = vec![0.0f64; arms.len()];
    let mut spare = vec![0.0f64; arms.len()];
    for i in 0..order.len() {
        let arm = &arms[order[i]];
        let mut trim: f64 = arm.half * 0.5;
        for (point, _) in [meets[(i + order.len() - 1) % order.len()], meets[i]]
            .into_iter()
            .flatten()
        {
            trim = trim.max(reach(arm, &point));
        }
        let allowed = (rooms[order[i]] * JUNCTION_ROOM).min(JUNCTION_REACH);
        if !trim.is_finite() || trim > allowed {
            return None;
        }
        trims[order[i]] = trim;
        spare[order[i]] = allowed - trim;
    }

    // Now the corners, rounded as far as the spare room allows.
    let mut corners: Vec<Option<Vec<DVec2>>> = Vec::with_capacity(order.len());
    for i in 0..order.len() {
        let (here, next) = (&arms[order[i]], &arms[order[(i + 1) % order.len()]]);
        corners.push(meets[i].map(|(point, gap)| {
            let along = spare[order[i]].min(spare[order[(i + 1) % order.len()]]);
            let radius = corner_radius(2.0 * here.half.min(next.half))
                .min(along * (gap / 2.0).tan().max(0.0));
            round_corner(point, here.left.1, next.right.1, gap, radius)
        }));
    }
    for i in 0..order.len() {
        let arm = &arms[order[i]];
        if let Some(before) = &corners[(i + order.len() - 1) % order.len()] {
            trims[order[i]] =
                trims[order[i]].max(reach(arm, before.last().expect("a corner has points")));
        }
        if let Some(after) = &corners[i] {
            trims[order[i]] = trims[order[i]].max(reach(arm, &after[0]));
        }
        if trims[order[i]] > (rooms[order[i]] * JUNCTION_ROOM).min(JUNCTION_REACH) {
            return None;
        }
    }

    // Round the node: out along each arm's right kerb to where its road is
    // cut, across, back along its left kerb, and round the corner to the next
    // arm. The kerbs are the road's own, so the junction meets the ribbon on
    // the very points the ribbon starts at.
    let mut outline: Vec<DVec2> = Vec::with_capacity(order.len() * 6);
    for i in 0..order.len() {
        let arm = &arms[order[i]];
        let road = &roads[arm.road];
        let cap = arm.out(trims[order[i]] + JUNCTION_LAP);
        // The kerb is walked from where the corner before it left off, so
        // every bend of the way between there and the cut is in the outline.
        let from = |corner: &Option<Vec<DVec2>>, take_last: bool| {
            corner.as_ref().map_or(arm.at, |points| {
                let point = if take_last {
                    points.last().expect("a corner has points")
                } else {
                    &points[0]
                };
                arm.out(reach(arm, point))
            })
        };
        let before = from(&corners[(i + order.len() - 1) % order.len()], true);
        let after = from(&corners[i], false);
        outline.extend(road.kerb_between(arm.right_side(false), before, cap));
        outline.extend(road.kerb_between(arm.right_side(true), cap, after));
        if let Some(corner) = &corners[i] {
            outline.extend(corner.iter().copied());
        }
    }
    let outline = fields::geometry::dedupe(&outline, 0.01);
    (outline.len() >= 3 && fields::geometry::area(&outline) > 1.0 && is_simple(&outline))
        .then_some((outline, trims))
}

/// A corner rounded off to `radius`: the points to put in the outline, the
/// first on the incoming arm's kerb and the last on the outgoing arm's.
///
/// `gap` is the angle of the grass wedge outside the corner, counter-
/// clockwise from one arm to the next. A wedge wider than a half turn is the
/// *outside* of a bend, where the kerbs make a sharp convex corner and there
/// is nothing to round; a wedge so narrow that the arc would run off along
/// the arms keeps its corner too.
fn round_corner(
    corner: DVec2,
    incoming: DVec2,
    outgoing: DVec2,
    gap: f64,
    radius: f64,
) -> Vec<DVec2> {
    if gap >= std::f64::consts::PI - 0.05 {
        return vec![corner];
    }
    let along = radius / (gap / 2.0).tan();
    if !along.is_finite() || along > 4.0 * radius {
        return vec![corner];
    }
    // The arc is tangent to both kerbs, its centre in the grass on the
    // bisector. One point on it is enough at a junction's scale — the drape
    // cuts the outline finer than the eye reads the curve anyway.
    let bisector = (incoming + outgoing).normalize_or_zero();
    let bulge = radius / (gap / 2.0).sin() - radius;
    vec![
        corner + incoming * along,
        corner + bisector * bulge,
        corner + outgoing * along,
    ]
}

/// Where two lines meet, each given as a point and a direction.
fn lines_meet(p0: DVec2, d0: DVec2, p1: DVec2, d1: DVec2) -> Option<DVec2> {
    let denominator = d0.perp_dot(d1);
    (denominator.abs() > 1e-9).then(|| p0 + d0 * ((p1 - p0).perp_dot(d1) / denominator))
}

/// Whether a ring crosses itself. Cheap and exact at a junction's handful of
/// corners, and the guard that keeps a shape the construction cannot make
/// sense of out of the mesh.
fn is_simple(ring: &[DVec2]) -> bool {
    let n = ring.len();
    for i in 0..n {
        for j in i + 2..n {
            if i == 0 && j == n - 1 {
                continue;
            }
            if crossing(ring[i], ring[(i + 1) % n], ring[j], ring[(j + 1) % n]).is_some() {
                return false;
            }
        }
    }
    true
}

/// Works out where the carriageways cover one another, and what that means
/// for each of them: the markings stop where roads actually meet, and any two
/// surfaces that overlap at all go on different layers so one of them is
/// cleanly on top.
///
/// A junction in OSM is not an object — it is two ways crossing, or one way
/// running out on another. What the eye wants is what the ground has: an
/// unmarked square of asphalt where two roads meet, not the two roads' own
/// stripes drawn through each other into a lattice.
///
/// The two questions are deliberately not the same one. **Overlap** is
/// geometric and generous: an extract splits every street into a way per
/// change of tagging, and where two of them meet their end caps trade
/// fragments unless they are told apart. **A junction** is narrow: a proper
/// crossing, or one road ending on another — never two ways that simply carry
/// on from one another, or a street would lose its markings at every name it
/// has ever had.
fn junctions(roads: &mut [Road]) {
    let boxes: Vec<(DVec2, DVec2)> = roads
        .iter()
        .map(|road| {
            let (lo, hi) = fields::geometry::bounds(&road.centre);
            let grow = DVec2::splat(road.half + MARK_CLEAR);
            (lo - grow, hi + grow)
        })
        .collect();

    let mut edges: Vec<Vec<Span>> = vec![Vec::new(); roads.len()];
    let mut centres: Vec<Vec<Span>> = vec![Vec::new(); roads.len()];
    let mut covers: Vec<Vec<usize>> = vec![Vec::new(); roads.len()];
    for a in 0..roads.len() {
        for b in a + 1..roads.len() {
            if boxes[a].1.x < boxes[b].0.x
                || boxes[b].1.x < boxes[a].0.x
                || boxes[a].1.y < boxes[b].0.y
                || boxes[b].1.y < boxes[a].0.y
            {
                continue;
            }
            if !overlap(&roads[a], &roads[b]) {
                continue;
            }
            covers[a].push(b);
            covers[b].push(a);
            let (on_a, on_b) = mouths(&roads[a], &roads[b]);
            // The edge lines break for anything that joins; the centre line
            // only for a road at least as wide, which is how a German
            // priority junction is marked — the through line runs, and the
            // Randlinie opens for the mouth.
            if roads[b].half + 0.01 >= roads[a].half {
                centres[a].extend(on_a.iter().copied());
            }
            if roads[a].half + 0.01 >= roads[b].half {
                centres[b].extend(on_b.iter().copied());
            }
            edges[a].extend(on_a);
            edges[b].extend(on_b);
        }
    }

    let layer = layers(&covers);
    for (i, road) in roads.iter_mut().enumerate() {
        road.blank_edges = merge_spans(std::mem::take(&mut edges[i]));
        road.blank_centre = merge_spans(std::mem::take(&mut centres[i]));
        road.lift = layer[i] as f64 * LAYER_LIFT;
    }
}

/// Whether two carriageways cover each other anywhere — the question the
/// layering asks, and the cheap one: their centre lines closer together than
/// the two half-widths put together.
fn overlap(a: &Road, b: &Road) -> bool {
    let reach = a.half + b.half;
    for i in 0..a.centre.len() - 1 {
        let (a0, a1) = (a.centre[i], a.centre[i + 1]);
        let (lo, hi) = (
            a0.min(a1) - DVec2::splat(reach),
            a0.max(a1) + DVec2::splat(reach),
        );
        for j in 0..b.centre.len() - 1 {
            let (b0, b1) = (b.centre[j], b.centre[j + 1]);
            if hi.x < b0.min(b1).x
                || b0.max(b1).x < lo.x
                || hi.y < b0.min(b1).y
                || b0.max(b1).y < lo.y
            {
                continue;
            }
            if segment_distance(a0, a1, b0, b1) < reach {
                return true;
            }
        }
    }
    false
}

/// Where two roads meet, as the span of arc length each of them loses its
/// markings over [m]: the crossings of their centre lines, and the ends
/// either of them runs out on the other at.
fn mouths(a: &Road, b: &Road) -> (Vec<Span>, Vec<Span>) {
    let (mut on_a, mut on_b) = (Vec::new(), Vec::new());
    let mut meet = |sa: f64, sb: f64, graze: f64| {
        let graze = graze.max(MARK_GRAZE);
        on_a.push((
            sa - b.half / graze - MARK_CLEAR,
            sa + b.half / graze + MARK_CLEAR,
        ));
        on_b.push((
            sb - a.half / graze - MARK_CLEAR,
            sb + a.half / graze + MARK_CLEAR,
        ));
    };
    // A crossroads: the two centre lines cross. Two ways that merely touch at
    // a shared node cross too, arithmetically — what tells a junction from a
    // join is the angle, not the touching.
    for i in 0..a.centre.len() - 1 {
        for j in 0..b.centre.len() - 1 {
            let Some((ta, tb)) =
                crossing(a.centre[i], a.centre[i + 1], b.centre[j], b.centre[j + 1])
            else {
                continue;
            };
            let (da, db) = (a.direction(i), b.direction(j));
            if da.dot(db).abs() > JOIN_COS {
                continue;
            }
            meet(
                a.s[i] + ta * (a.s[i + 1] - a.s[i]),
                b.s[j] + tb * (b.s[j + 1] - b.s[j]),
                da.perp_dot(db).abs(),
            );
        }
    }
    // A T-junction, a side road, the mouth of a roundabout: one of them runs
    // out under the other's carriageway.
    for (sa, sb, graze) in ends_on(a, b) {
        meet(sa, sb, graze);
    }
    for (sb, sa, graze) in ends_on(b, a) {
        meet(sa, sb, graze);
    }
    (on_a, on_b)
}

/// Where `a` runs out on `b`: an end of `a` under `b`'s carriageway, as the
/// arc length on each and the sine of the angle they meet at.
///
/// An end that runs on in `b`'s own direction is not a junction but a join
/// ([`JOIN_COS`]) — and so is a slip road that merges at a shallow angle,
/// which has no hard break in its markings on the ground either.
fn ends_on(a: &Road, b: &Road) -> Vec<(f64, f64, f64)> {
    let last = a.centre.len() - 2;
    let mut out = Vec::new();
    for (point, sa, heading) in [
        (a.centre[0], 0.0, a.direction(0)),
        (a.centre[a.centre.len() - 1], a.length(), a.direction(last)),
    ] {
        let Some((distance, sb, j, _)) = nearest_on(b, point) else {
            continue;
        };
        if distance > b.half + MARK_CLEAR || heading.dot(b.direction(j)).abs() > JOIN_COS {
            continue;
        }
        out.push((sa, sb, heading.perp_dot(b.direction(j)).abs()));
    }
    out
}

/// The nearest point of a centre line to `p`: the distance, the arc length
/// there, and which segment it fell on and where.
fn nearest_on(road: &Road, p: DVec2) -> Option<(f64, f64, usize, f64)> {
    let mut best: Option<(f64, f64, usize, f64)> = None;
    for i in 0..road.centre.len() - 1 {
        let (q0, q1) = (road.centre[i], road.centre[i + 1]);
        let d = q1 - q0;
        let length = d.length_squared();
        if length < 1e-12 {
            continue;
        }
        let t = ((p - q0).dot(d) / length).clamp(0.0, 1.0);
        let distance = p.distance(q0 + d * t);
        if best.is_none_or(|(closest, ..)| distance < closest) {
            best = Some((distance, road.s[i] + t * (road.s[i + 1] - road.s[i]), i, t));
        }
    }
    best
}

/// Where two segments properly cross, as the fraction along each. `None`
/// where they are parallel or only reach past one another.
fn crossing(a0: DVec2, a1: DVec2, b0: DVec2, b1: DVec2) -> Option<(f64, f64)> {
    let (da, db) = (a1 - a0, b1 - b0);
    let denominator = da.perp_dot(db);
    if denominator.abs() < 1e-12 {
        return None;
    }
    let t = (b0 - a0).perp_dot(db) / denominator;
    let u = (b0 - a0).perp_dot(da) / denominator;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some((t, u))
}

/// The closest approach of two segments.
fn segment_distance(a0: DVec2, a1: DVec2, b0: DVec2, b1: DVec2) -> f64 {
    if crossing(a0, a1, b0, b1).is_some() {
        return 0.0;
    }
    let foot = |p: DVec2, q0: DVec2, q1: DVec2| {
        let d = q1 - q0;
        let length = d.length_squared();
        if length < 1e-12 {
            return p.distance(q0);
        }
        p.distance(q0 + d * ((p - q0).dot(d) / length).clamp(0.0, 1.0))
    };
    foot(a0, b0, b1)
        .min(foot(a1, b0, b1))
        .min(foot(b0, a0, a1))
        .min(foot(b1, a0, a1))
}

/// A stretch of a road's own arc length [m] that a junction takes the
/// markings out of.
type Span = (f64, f64);

/// The two edges meeting at a point of a centre line, each as its unit
/// direction and its length. `None` at the end of a way that does not close.
type Corner = (Option<(DVec2, f64)>, Option<(DVec2, f64)>);

/// Sorts spans and runs the overlapping ones together.
fn merge_spans(mut spans: Vec<Span>) -> Vec<Span> {
    spans.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut out: Vec<Span> = Vec::with_capacity(spans.len());
    for (lo, hi) in spans {
        match out.last_mut() {
            Some(last) if lo <= last.1 => last.1 = last.1.max(hi),
            _ => out.push((lo, hi)),
        }
    }
    out
}

/// The layer each carriageway is drawn on: the lowest one none of the roads
/// it covers already holds. Greedy colouring in line order — the order the
/// file lists them — so every machine in a multiplayer run assigns the same
/// layers from the same line, without a word crossing the network.
fn layers(covers: &[Vec<usize>]) -> Vec<u8> {
    let mut layer = vec![0u8; covers.len()];
    for i in 0..covers.len() {
        let mut taken = [false; LAYERS];
        for &j in &covers[i] {
            if j < i {
                taken[layer[j] as usize] = true;
            }
        }
        layer[i] = taken.iter().position(|held| !held).unwrap_or(0) as u8;
    }
    layer
}

/// How much of a marking runs at `along` [0…1]: nothing inside a junction,
/// all of it a [`MARK_FADE`] clear of one.
fn markings(spans: &[Span], along: f64) -> f32 {
    let mut clear = f64::MAX;
    for &(lo, hi) in spans {
        if (lo..=hi).contains(&along) {
            return 0.0;
        }
        clear = clear.min((lo - along).abs().min((along - hi).abs()));
    }
    (clear / MARK_FADE).clamp(0.0, 1.0) as f32
}

/// The carriageways of one tile, one patch per surface found on it.
///
/// `ground` is the *shaped* ground height at any UTM point — DGM, brush
/// edits and the track's cutting/embankment blend, the same function that
/// sampled the tile's own grid. A bridge measures the ends of its chord on
/// it; both tiles at a seam evaluate the same ends, so both cut the same
/// chord and the decks meet without a step.
pub(crate) fn patches(
    k: TileKey,
    grid: &HeightGrid,
    frame: &EnuFrame,
    zone: u8,
    tile_size: f64,
    roads: &Roads,
    ground: &mut dyn FnMut(DVec2) -> f64,
) -> Vec<RoadPatch> {
    // The tile itself: a road is cut to it exactly, and the neighbouring tile
    // cuts the other half the same way, so the two meet without a seam.
    let min = DVec2::new(k.0 as f64 * tile_size, k.1 as f64 * tile_size);
    let max = min + DVec2::splat(tile_size);
    let step = drape_step(grid.step());

    let mut by_surface: HashMap<RoadSurface, RoadPatch> = HashMap::new();
    let blank = |surface: RoadSurface| RoadPatch {
        surface,
        positions: Vec::new(),
        normals: Vec::new(),
        uvs: Vec::new(),
        colors: Vec::new(),
        indices: Vec::new(),
        sources: Vec::new(),
    };
    for &at in roads.by_tile.get(&k).map_or(&[][..], Vec::as_slice) {
        let road = &roads.roads[at];
        // The ends of a bridge's chord, deck height included: the shaped
        // ground under the way's own first and last point. A bridge way spans
        // abutment to abutment, so its ends *are* the abutments, and the deck
        // meets the draped road there.
        let ends = road.bridge.then(|| buttress_heights(road, ground));
        let patch = by_surface
            .entry(road.surface)
            .or_insert_with(|| blank(road.surface));
        let before = patch.indices.len();
        for i in 0..road.centre.len() - 1 {
            add_segment(patch, road, i, (min, max), step, grid, frame, zone, ends);
        }
        // Only what actually landed on the tile: a click on the patch picks
        // the roads it can see, not every road whose box overlapped.
        if patch.indices.len() > before {
            patch.sources.push(road.index);
        }
    }

    // The junctions: the ground the roads gave up, carried as one surface.
    for &at in roads
        .junctions_by_tile
        .get(&k)
        .map_or(&[][..], Vec::as_slice)
    {
        let junction = &roads.junctions[at];
        let patch = by_surface
            .entry(junction.surface)
            .or_insert_with(|| blank(junction.surface));
        let before = patch.indices.len();
        add_junction(patch, junction, (min, max), step, grid, frame, zone);
        if patch.indices.len() > before {
            for source in &junction.sources {
                if !patch.sources.contains(source) {
                    patch.sources.push(*source);
                }
            }
        }
    }
    for patch in by_surface.values_mut() {
        patch.sources.sort_unstable();
    }

    let mut out: Vec<RoadPatch> = by_surface
        .into_values()
        .filter(|p| !p.indices.is_empty())
        .collect();
    // A stable order, so the same tile always builds the same entities.
    out.sort_by_key(|p| p.surface);
    out
}

/// Lays a junction's own surface on the tile: its outline cut to the tile,
/// triangulated, refined to the drape step and draped on the height grid.
///
/// It carries no markings — the roads that lead into it have already stopped
/// theirs — and its texture runs in plain metres of ground rather than along
/// any one road, because a junction belongs to none of them. `b = 0.5` in the
/// vertex colour makes the shader's "metres across" the `u` itself, so the
/// grain of the asphalt keeps its scale.
#[allow(clippy::too_many_arguments)]
fn add_junction(
    patch: &mut RoadPatch,
    junction: &Junction,
    tile: (DVec2, DVec2),
    step: f64,
    grid: &HeightGrid<'_>,
    frame: &EnuFrame,
    zone: u8,
) {
    let (min, max) = tile;
    let (lo, hi) = fields::geometry::bounds(&junction.outline);
    if hi.x <= min.x || lo.x >= max.x || hi.y <= min.y || lo.y >= max.y {
        return;
    }
    let rect = vec![min, DVec2::new(max.x, min.y), max, DVec2::new(min.x, max.y)];
    let anchor = junction.outline[0];
    for piece in fields::geometry::clip(&junction.outline, &rect, fields::geometry::Op::Intersect) {
        let mut points = piece;
        let mut tris = fields::geometry::triangulate(&points);
        if tris.is_empty() {
            continue;
        }
        // From the longest edge the triangulation actually has, not from the
        // size of the piece: ear clipping a junction's outline already leaves
        // triangles a fraction of it, and refining every one of them as
        // though it were the whole costs four times the mesh a level.
        let longest = tris
            .iter()
            .flat_map(|t| {
                let p = |i: u32| points[i as usize];
                [
                    p(t[0]).distance(p(t[1])),
                    p(t[1]).distance(p(t[2])),
                    p(t[2]).distance(p(t[0])),
                ]
            })
            .fold(0.0f64, f64::max);
        refine(&mut points, &mut tris, levels(longest, step * 2.0));
        let base = patch.positions.len() as u32;
        for p in &points {
            let height = grid.at(*p) + LIFT + junction.lift;
            let (lat, lon) = geo::from_utm(p.x, p.y, zone);
            patch
                .positions
                .push(to_render(frame.to_local(geo::to_ecef(lat, lon, height))));
            patch.normals.push(ground_normal(*p, grid));
            patch
                .uvs
                .push([(p.x - anchor.x) as f32, (p.y - anchor.y) as f32]);
            patch.colors.push([0.0, 0.0, 0.5, 0.0]);
        }
        for [a, b, c] in tris {
            patch
                .indices
                .extend_from_slice(&[base + a, base + b, base + c]);
        }
    }
}

/// How many times a piece has to be split in four for its edges to come under
/// `step`. A junction is a few metres of nearly flat ground, so it does not
/// need the ribbon's own fineness — and three levels is already sixty-four
/// triangles for every one it started with.
fn levels(size: f64, step: f64) -> u32 {
    let mut levels = 0;
    let mut edge = size;
    while edge > step && levels < 3 {
        edge /= 2.0;
        levels += 1;
    }
    levels
}

/// Splits every triangle into four, `levels` times over. Uniform rather than
/// by edge length: a mesh subdivided the same everywhere cannot crack. The
/// same subdivision the farmland and the water use.
fn refine(points: &mut Vec<DVec2>, tris: &mut Vec<[u32; 3]>, levels: u32) {
    for _ in 0..levels {
        let mut midpoints: HashMap<(u32, u32), u32> = HashMap::new();
        let mut split = Vec::with_capacity(tris.len() * 4);
        for &[a, b, c] in tris.iter() {
            let mut mid = |i: u32, j: u32, points: &mut Vec<DVec2>| -> u32 {
                let key = if i < j { (i, j) } else { (j, i) };
                *midpoints.entry(key).or_insert_with(|| {
                    let at = points.len() as u32;
                    points.push((points[i as usize] + points[j as usize]) / 2.0);
                    at
                })
            };
            let ab = mid(a, b, points);
            let bc = mid(b, c, points);
            let ca = mid(c, a, points);
            split.extend_from_slice(&[[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]]);
        }
        *tris = split;
    }
}

/// The ground's normal under a point, from the height grid's own gradient —
/// the same finite difference both sides of a tile seam, so the shading does
/// not crease where the mesh is cut.
fn ground_normal(p: DVec2, grid: &HeightGrid<'_>) -> [f32; 3] {
    const D: f64 = 1.0;
    let dx = grid.at(p + DVec2::new(D, 0.0)) - grid.at(p - DVec2::new(D, 0.0));
    let dy = grid.at(p + DVec2::new(0.0, D)) - grid.at(p - DVec2::new(0.0, D));
    // Render axes: +x east, +y up, +z south — so north is −z.
    let n = Vec3::new(-(dx / (2.0 * D)) as f32, 1.0, (dy / (2.0 * D)) as f32).normalize_or_zero();
    let n = if n == Vec3::ZERO { Vec3::Y } else { n };
    [n.x, n.y, n.z]
}

/// One vertex of the ribbon before it is draped: where it stands in the world
/// and where it stands on the road. Both travel through the cut to the tile
/// together, because the cut interpolates them the same way.
#[derive(Clone, Copy, Debug)]
struct Rib {
    p: DVec2,
    /// Across the carriageway, 0 at the left kerb and 1 at the right.
    u: f64,
    /// Along the road [m], from its own start.
    v: f64,
}

impl Rib {
    fn lerp(self, other: Rib, t: f64) -> Rib {
        Rib {
            p: self.p.lerp(other.p, t),
            u: self.u + (other.u - self.u) * t,
            v: self.v + (other.v - self.v) * t,
        }
    }
}

/// Lays one segment of a road on the tile: the quad between the two kerbs,
/// cut into cells of `step`, cut to the tile, draped on its height grid — or,
/// where the road flies, on its bridge chord — and written out with the
/// marking data and the texture coordinates the shader needs.
///
/// A ribbon of quads rather than one buffered outline: a curve offset to both
/// sides in one piece crosses itself on the inside of the bend, and a ring
/// that crosses itself is what neither the clip nor the ear clipping can take
/// — which is where the torn edges and the stray triangles came from. Segment
/// by segment, the shape is a trapezoid that cannot cross itself, and the
/// metre along and across the road is known at each corner instead of being
/// searched for afterwards.
#[allow(clippy::too_many_arguments)]
fn add_segment(
    patch: &mut RoadPatch,
    road: &Road,
    i: usize,
    tile: (DVec2, DVec2),
    step: f64,
    grid: &HeightGrid<'_>,
    frame: &EnuFrame,
    zone: u8,
    ends: Option<(f64, f64)>,
) {
    // Whatever a junction took of this road it now carries itself, as one
    // surface; the ribbon draws the rest.
    for (from, to) in outside(&road.holes, road.s[i], road.s[i + 1]) {
        add_ribbon(
            patch,
            road,
            i,
            (from, to),
            tile,
            step,
            grid,
            frame,
            zone,
            ends,
        );
    }
}

/// The stretches of `from..to` that no span covers, in order. The spans are
/// sorted and merged.
fn outside(spans: &[Span], from: f64, to: f64) -> Vec<Span> {
    let mut out = Vec::new();
    let mut at = from;
    for &(lo, hi) in spans {
        if hi <= at {
            continue;
        }
        if lo >= to {
            break;
        }
        if lo > at {
            out.push((at, lo.min(to)));
        }
        at = at.max(hi);
        if at >= to {
            return out;
        }
    }
    if at < to {
        out.push((at, to));
    }
    out
}

/// One stretch of one segment of a road, as above.
#[allow(clippy::too_many_arguments)]
fn add_ribbon(
    patch: &mut RoadPatch,
    road: &Road,
    i: usize,
    drawn: Span,
    tile: (DVec2, DVec2),
    step: f64,
    grid: &HeightGrid<'_>,
    frame: &EnuFrame,
    zone: u8,
    ends: Option<(f64, f64)>,
) {
    let segment = road.s[i + 1] - road.s[i];
    let length = drawn.1 - drawn.0;
    if segment < 1e-6 || length < 1e-6 {
        return;
    }
    let (min, max) = tile;
    // Where in the segment the drawn stretch starts and ends, as fractions.
    let (v0, v1) = (
        (drawn.0 - road.s[i]) / segment,
        (drawn.1 - road.s[i]) / segment,
    );
    let side = |kerb: &[DVec2], v: f64| kerb[i].lerp(kerb[i + 1], v);
    let corners = [
        side(&road.left, v0),
        side(&road.left, v1),
        side(&road.right, v0),
        side(&road.right, v1),
    ];
    let (lo, hi) = fields::geometry::bounds(&corners);
    if hi.x <= min.x || lo.x >= max.x || hi.y <= min.y || lo.y >= max.y {
        return;
    }

    // The cells: as many as the drape step asks for across the carriageway
    // and along the stretch. Both tiles at a seam cut the same segment into
    // the same cells, so the pieces they keep of it meet exactly.
    let cells = |extent: f64| ((extent / step).ceil() as usize).clamp(1, MAX_CELLS);
    let (across, along) = (cells(2.0 * road.half), cells(length));
    // The ribbon's own frame: `u` from kerb to kerb, `v` from one end of the
    // stretch to the other. Bilinear, so a point on a cell edge is exactly
    // where the cut to the tile puts it.
    let rib = |u: f64, v: f64| {
        let along = v0 + v * (v1 - v0);
        Rib {
            p: road.left[i]
                .lerp(road.right[i], u)
                .lerp(road.left[i + 1].lerp(road.right[i + 1], u), along),
            u,
            v: road.s[i] + along * segment,
        }
    };

    let inside = lo.x >= min.x && hi.x <= max.x && lo.y >= min.y && hi.y <= max.y;
    let direction = road.direction(i);
    let vertex = |patch: &mut RoadPatch, rib: Rib| -> u32 {
        let at = patch.positions.len() as u32;
        let height = deck_height(road, rib.p, rib.v, grid, ends);
        let (lat, lon) = geo::from_utm(rib.p.x, rib.p.y, zone);
        patch
            .positions
            .push(to_render(frame.to_local(geo::to_ecef(lat, lon, height))));
        patch
            .normals
            .push(deck_normal(rib.p, rib.v, direction, road, grid, ends));
        patch.uvs.push([rib.u as f32, rib.v as f32]);
        // The markings: which centre line the road carries, how much of the
        // edge lines and how much of the centre line runs here (a junction
        // takes both out), and the half-width the shader measures in.
        let edges = if road.edge_lines {
            markings(&road.blank_edges, rib.v)
        } else {
            0.0
        };
        patch.colors.push([
            road.center_line as usize as f32,
            edges,
            road.half as f32,
            markings(&road.blank_centre, rib.v),
        ]);
        at
    };

    if inside {
        // Wholly on the tile: one grid of vertices, shared between the cells.
        let base = patch.positions.len() as u32;
        for iu in 0..=across {
            for iv in 0..=along {
                vertex(
                    patch,
                    rib(iu as f64 / across as f64, iv as f64 / along as f64),
                );
            }
        }
        let row = along as u32 + 1;
        for iu in 0..across as u32 {
            for iv in 0..along as u32 {
                let a = base + iu * row + iv;
                let (b, c) = (a + 1, a + row);
                patch.indices.extend_from_slice(&[a, b, c, b, c + 1, c]);
            }
        }
        return;
    }

    // Across the tile boundary: cell by cell, each cut to the tile. The cut
    // runs on the cell's own edges, where the ribbon is straight, so both
    // tiles compute the same crossing point and the same metre on the road.
    for iu in 0..across {
        let (u0, u1) = (iu as f64 / across as f64, (iu + 1) as f64 / across as f64);
        for iv in 0..along {
            let (v0, v1) = (iv as f64 / along as f64, (iv + 1) as f64 / along as f64);
            // Counter-clockwise seen from above, so the face looks up.
            let cell = [rib(u0, v0), rib(u0, v1), rib(u1, v1), rib(u1, v0)];
            let (lo, hi) = fields::geometry::bounds(&cell.map(|r| r.p));
            if hi.x <= min.x || lo.x >= max.x || hi.y <= min.y || lo.y >= max.y {
                continue;
            }
            let kept = if lo.x >= min.x && hi.x <= max.x && lo.y >= min.y && hi.y <= max.y {
                cell.to_vec()
            } else {
                cut_to_tile(&cell, min, max)
            };
            if kept.len() < 3 {
                continue;
            }
            // A fan: the piece a rectangle cuts out of a cell is convex.
            let first = vertex(patch, kept[0]);
            let mut previous = vertex(patch, kept[1]);
            for rib in &kept[2..] {
                let next = vertex(patch, *rib);
                patch.indices.extend_from_slice(&[first, previous, next]);
                previous = next;
            }
        }
    }
}

/// What a tile keeps of one cell: the cell cut against the tile's four sides
/// (Sutherland-Hodgman), the metre along and across the road carried along
/// with the corners. The cell is convex, so the piece is one convex polygon.
fn cut_to_tile(cell: &[Rib], min: DVec2, max: DVec2) -> Vec<Rib> {
    let sides: [fn(DVec2, DVec2, DVec2) -> f64; 4] = [
        |p, min, _| p.x - min.x,
        |p, _, max| max.x - p.x,
        |p, min, _| p.y - min.y,
        |p, _, max| max.y - p.y,
    ];
    let mut poly = cell.to_vec();
    for side in sides {
        if poly.is_empty() {
            break;
        }
        let mut kept: Vec<Rib> = Vec::with_capacity(poly.len() + 1);
        for (i, &here) in poly.iter().enumerate() {
            let there = poly[(i + poly.len() - 1) % poly.len()];
            let (d0, d1) = (side(there.p, min, max), side(here.p, min, max));
            if (d0 < 0.0) != (d1 < 0.0) {
                kept.push(there.lerp(here, d0 / (d0 - d1)));
            }
            if d1 >= 0.0 {
                kept.push(here);
            }
        }
        poly = kept;
    }
    poly
}

/// The height a road vertex is laid at: the drape on the tile's grid — or,
/// where the ground dips below the straight line between the way's own ends,
/// that chord: the deck of a bridge. The drape still wins wherever the
/// ground is above it, so the deck runs exactly as far as the hollow does.
fn deck_height(
    road: &Road,
    p: DVec2,
    along: f64,
    grid: &HeightGrid<'_>,
    ends: Option<(f64, f64)>,
) -> f64 {
    let drape = grid.at(p) + LIFT + road.lift;
    let Some((h0, h1)) = ends else {
        return drape;
    };
    let t = (along / road.length()).clamp(0.0, 1.0);
    drape.max(h0 + (h1 - h0) * t)
}

/// The shaped ground under a way's own first and last point, deck height
/// included — the abutments the bridge chord is stretched between.
fn buttress_heights(road: &Road, ground: &mut dyn FnMut(DVec2) -> f64) -> (f64, f64) {
    let first = road.centre.first().copied().unwrap_or_default();
    let last = road.centre.last().copied().unwrap_or_default();
    (
        ground(first) + LIFT + road.lift,
        ground(last) + LIFT + road.lift,
    )
}

/// The normal of the surface the vertex was laid on, by finite differences of
/// the height around it — drape and chord both, so a bridge is shaded like
/// the span it is and not like the valley it crosses.
///
/// Analytic rather than accumulated from the triangles: a vertex on a tile
/// seam only ever sees the triangles of its own tile, and the two sides of
/// the seam would then be shaded differently. The metre along the road moves
/// with the segment's own direction, which both tiles at a seam agree on.
fn deck_normal(
    p: DVec2,
    along: f64,
    direction: DVec2,
    road: &Road,
    grid: &HeightGrid<'_>,
    ends: Option<(f64, f64)>,
) -> [f32; 3] {
    const D: f64 = 1.0;
    let at = |q: DVec2, along: f64| deck_height(road, q, along, grid, ends);
    let dx = at(p + DVec2::new(D, 0.0), along + D * direction.x)
        - at(p - DVec2::new(D, 0.0), along - D * direction.x);
    let dy = at(p + DVec2::new(0.0, D), along + D * direction.y)
        - at(p - DVec2::new(0.0, D), along - D * direction.y);
    // Render axes: +x east, +y up, +z south — so north is −z.
    let n = Vec3::new(-(dx / (2.0 * D)) as f32, 1.0, (dy / (2.0 * D)) as f32).normalize_or_zero();
    let n = if n == Vec3::ZERO { Vec3::Y } else { n };
    [n.x, n.y, n.z]
}

/// Drops the repeats an OSM way carries — a segment of no length has no
/// direction, and its kerbs would be a spike.
fn dedupe(line: Vec<DVec2>) -> Vec<DVec2> {
    let mut out: Vec<DVec2> = Vec::with_capacity(line.len());
    for p in line {
        if out
            .last()
            .is_none_or(|last| last.distance_squared(p) > 1e-6)
        {
            out.push(p);
        }
    }
    out
}

/// Arc length at each point of a line [m], from its own start — the dash
/// phase runs in it.
fn arc_lengths(line: &[DVec2]) -> Vec<f64> {
    let mut s = Vec::with_capacity(line.len());
    let mut total = 0.0;
    for (i, p) in line.iter().enumerate() {
        if i > 0 {
            total += line[i - 1].distance(*p);
        }
        s.push(total);
    }
    s
}

/// The two kerbs of a carriageway: the centre line offset a half-width to
/// each side, mitered where two edges meet — the corner point of the miter is
/// exactly a half-width from *both* edges, so the two quads that share it
/// meet without a gap and without an overlap, and the ribbon is one surface.
///
/// Two clamps keep the corner honest. The first is the usual miter limit: as
/// a turn closes the miter runs off to infinity, and a sharp junction would
/// sprout a spike instead of the round join the real road has. The second is
/// the length of the edges meeting there — a miter that reached further back
/// than the segment is long would fold the quad over itself, which is a
/// bow-tie and shades as a hole.
fn kerbs(centre: &[DVec2], half: f64) -> (Vec<DVec2>, Vec<DVec2>) {
    let closed = is_closed(centre);
    let mut left = Vec::with_capacity(centre.len());
    let mut right = Vec::with_capacity(centre.len());
    for i in 0..centre.len() {
        let (before, after) = edges_at(centre, i, closed);
        let direction = match (before, after) {
            (Some(a), Some(b)) => (a.0 + b.0).normalize_or_zero(),
            (Some(a), None) => a.0,
            (None, Some(b)) => b.0,
            (None, None) => DVec2::X,
        };
        // A way that doubles back on itself exactly cancels its own bisector.
        // Falling back on one of the two edges narrows the corner; letting
        // the zero through would pinch the carriageway to a point and run two
        // long wedges out of it.
        let direction = if direction == DVec2::ZERO {
            after.or(before).map_or(DVec2::X, |edge| edge.0)
        } else {
            direction
        };
        let normal = DVec2::new(-direction.y, direction.x);
        let miter = match (before, after) {
            (Some(a), Some(b)) => miter_stretch(a, b, half),
            _ => 1.0,
        };
        left.push(centre[i] - normal * (half * miter));
        right.push(centre[i] + normal * (half * miter));
    }
    (left, right)
}

/// Whether a way closes on itself — a roundabout, a loop, the aisle of a car
/// park. Its two ends are one corner of the ribbon, not two, and a ring
/// mitered as though it had ends shows a notch wherever the file happens to
/// start going round.
fn is_closed(centre: &[DVec2]) -> bool {
    centre.len() > 3 && centre[0].distance_squared(centre[centre.len() - 1]) < 1e-6
}

/// The two edges meeting at point `i`, each as its unit direction and its
/// length — wrapping round a closed way, where the last point *is* the first.
fn edges_at(centre: &[DVec2], i: usize, closed: bool) -> Corner {
    let last = centre.len() - 1;
    let edge = |from: usize, to: usize| {
        let d = centre[to] - centre[from];
        (d.normalize_or_zero(), d.length())
    };
    let before = if i > 0 {
        Some(edge(i - 1, i))
    } else if closed {
        Some(edge(last - 1, last))
    } else {
        None
    };
    let after = if i < last {
        Some(edge(i, i + 1))
    } else if closed {
        Some(edge(0, 1))
    } else {
        None
    };
    (before, after)
}

/// How far past the round join the miter at a corner may stretch [as a factor
/// of the half-width]: the reciprocal of the cosine of half the corner angle,
/// which grows without bound as the turn closes — clamped to
/// [`MITER_LIMIT`], and clamped again so the corner reaches back no further
/// than [`MITER_REACH`] of the shorter of the two edges meeting there.
fn miter_stretch(before: (DVec2, f64), after: (DVec2, f64), half: f64) -> f64 {
    // Half the turn, theta the direction change: cos sqrt((1 + cos theta) / 2),
    // sin sqrt((1 - cos theta) / 2).
    let turn = before.0.dot(after.0).clamp(-1.0, 1.0);
    let cos_half = (turn / 2.0 + 0.5).sqrt();
    let sin_half = (0.5 - turn / 2.0).sqrt();
    // A corner point at `half * m` along the bisector reaches `half * m *
    // sin(theta/2)` back along the edge it came from. That has to stay inside
    // the edge, whatever the miter would like to be — and where even the
    // plain perpendicular offset would not fit, the carriageway narrows into
    // the corner rather than folding over itself.
    let shorter = before.1.min(after.1);
    let by_length = MITER_REACH * shorter / (half * sin_half.max(1e-6));
    (1.0 / cos_half.max(1e-6))
        .min(by_length)
        .clamp(MITER_FLOOR, MITER_LIMIT)
}

/// How far past the round join a mitered corner may stretch [as a factor of
/// the half-width]. 2.5 is a turn of about 130°; past that the corner is
/// cut back to what the neighbouring edges can carry.
const MITER_LIMIT: f64 = 2.5;

/// How far back along the shorter of the two edges at a corner the miter may
/// reach [as a share of it]. Below a half, so the two corners of one segment
/// can never meet in the middle and fold the quad over.
const MITER_REACH: f64 = 0.4;

/// How far *in* from the half-width a corner may be pulled where even the
/// perpendicular offset would not fit between two short edges — a hairpin on
/// an OSM way. The carriageway narrows into such a corner; the alternative is
/// a quad folded over itself, which shades as a hole.
const MITER_FLOOR: f64 = 0.2;
/// ENU (east, north, up) to render axes (east, up, −north).
fn to_render(v: DVec3) -> [f32; 3] {
    [v.x as f32, v.z as f32, -v.y as f32]
}

/// The tile a UTM point falls in.
fn key(p: DVec2, tile_size: f64) -> TileKey {
    (
        (p.x / tile_size).floor() as i64,
        (p.y / tile_size).floor() as i64,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A corner of a road, at the given UTM point in zone 32.
    fn point(e: f64, n: f64) -> crate::route::RoadPoint {
        let (lat, lon) = geo::from_utm(e, n, 32);
        crate::route::RoadPoint {
            lat: lat.to_degrees(),
            lon: lon.to_degrees(),
        }
    }

    /// A straight west-east road of `len` metres and `width` metres.
    fn source(e: f64, n: f64, len: f64, width: f64) -> RoadSource {
        RoadSource {
            name: "Landstraße".into(),
            points: vec![point(e, n), point(e + len, n)],
            width,
            surface: RoadSurface::Asphalt,
            center_line: CenterLine::Dashed,
            edge_lines: true,
            bridge: false,
            tags: Vec::new(),
        }
    }

    /// Builds the patches of one tile over flat ground.
    fn patches_of(sources: &[RoadSource], tile: TileKey) -> Vec<RoadPatch> {
        let tile_size = 512.0;
        let min = DVec2::new(tile.0 as f64 * tile_size, tile.1 as f64 * tile_size);
        let step = 8.0;
        let n = (tile_size / step) as usize;
        let heights = vec![100.0f32; (n + 1) * (n + 1)];
        let grid = HeightGrid::new(min, &heights, step, n);
        let centre = min + DVec2::splat(tile_size / 2.0);
        let (clat, clon) = geo::from_utm(centre.x, centre.y, 32);
        let frame = EnuFrame::at(geo::to_ecef(clat, clon, 0.0));
        let roads = Roads::from_parts(sources, 32, tile_size);
        let mut ground = |_: DVec2| 100.0;
        patches(tile, &grid, &frame, 32, tile_size, &roads, &mut ground)
    }

    #[test]
    fn a_road_lands_on_the_tiles_it_covers() {
        // 3 km across, so it spans several 512 m tiles.
        let roads = Roads::from_parts(&[source(440_000.0, 5_715_000.0, 3_000.0, 6.0)], 32, 512.0);
        assert_eq!(roads.len(), 1);
        assert!(roads.touches((859, 11162)), "{:?}", roads.by_tile.keys());
        assert!(!roads.touches((0, 0)));
    }

    #[test]
    fn a_centre_line_of_one_point_is_no_road() {
        let mut bad = source(440_000.0, 5_715_000.0, 100.0, 6.0);
        bad.points.truncate(1);
        assert!(Roads::from_parts(&[bad], 32, 512.0).is_empty());
    }

    /// Total surface area of a patch [m²], from its own triangles.
    fn patch_area(patch: &RoadPatch) -> f64 {
        patch
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| {
                let p = |i: u32| {
                    let v = patch.positions[i as usize];
                    DVec3::new(v[0] as f64, v[1] as f64, v[2] as f64)
                };
                (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0])).length() / 2.0
            })
            .sum()
    }

    #[test]
    fn a_road_becomes_a_draped_carriageway() {
        let tile = (859, 11162);
        let min = DVec2::new(tile.0 as f64 * 512.0, tile.1 as f64 * 512.0);
        let patches = patches_of(&[source(min.x + 100.0, min.y + 100.0, 200.0, 6.0)], tile);
        assert_eq!(patches.len(), 1);
        let patch = &patches[0];
        assert_eq!(patch.sources, vec![0]);
        assert!(patch.triangles() > 0);
        assert_eq!(patch.positions.len(), patch.normals.len());
        assert_eq!(patch.positions.len(), patch.uvs.len());
        assert_eq!(patch.positions.len(), patch.colors.len());
        // Flat ground: every normal points up.
        for n in &patch.normals {
            assert!((n[1] - 1.0).abs() < 1e-5, "{n:?}");
        }
        // Every index addresses a vertex that exists.
        let count = patch.positions.len() as u32;
        assert!(patch.indices.iter().all(|i| *i < count));
    }

    #[test]
    fn a_road_is_cut_at_the_tile_boundary() {
        let tile = (859, 11162);
        let min = DVec2::new(tile.0 as f64 * 512.0, tile.1 as f64 * 512.0);
        // Straddling the eastern seam, 60 m of it in this tile and 140 in the
        // next — off centre, so a bug that halved it would still show.
        let road = source(min.x + 452.0, min.y + 100.0, 200.0, 6.0);
        let here = patches_of(std::slice::from_ref(&road), tile);
        let next = patches_of(&[road], (tile.0 + 1, tile.1));
        assert_eq!(here.len(), 1);
        assert_eq!(next.len(), 1);
        // Nothing is lost and nothing is drawn twice: the pieces add up to
        // the road — 200 m of it times 6 m of carriageway. (Within a per
        // cent — UTM's scale factor is not 1.)
        let total = patch_area(&here[0]) + patch_area(&next[0]);
        assert!((total - 1_200.0).abs() < 12.0, "{total}");
        assert!(
            (patch_area(&here[0]) - 360.0).abs() < 12.0,
            "{}",
            patch_area(&here[0])
        );
    }

    #[test]
    fn the_markings_run_across_the_tile_boundary() {
        let tile = (859, 11162);
        let min = DVec2::new(tile.0 as f64 * 512.0, tile.1 as f64 * 512.0);
        // A road running east-west, its start 60 m west of the seam: the
        // seam sits at the road's own 60 m, so both pieces' v ranges meet
        // there — the way the dash phase is.
        let road = RoadSource {
            name: String::new(),
            points: vec![
                point(min.x + 452.0, min.y + 150.0),
                point(min.x + 652.0, min.y + 150.0),
            ],
            width: 6.0,
            surface: RoadSurface::Asphalt,
            center_line: CenterLine::Dashed,
            edge_lines: true,
            bridge: false,
            tags: Vec::new(),
        };
        let here = patches_of(std::slice::from_ref(&road), tile);
        let next = patches_of(&[road], (tile.0 + 1, tile.1));
        assert_eq!(here.len(), 1);
        assert_eq!(next.len(), 1);
        let v = |patch: &RoadPatch| {
            patch.uvs.iter().fold((f32::MAX, f32::MIN), |(lo, hi), uv| {
                (lo.min(uv[1]), hi.max(uv[1]))
            })
        };
        let (lo_here, hi_here) = v(&here[0]);
        let (lo_next, hi_next) = v(&next[0]);
        assert!((hi_here - 60.0).abs() < 4.0, "{hi_here}");
        assert!((lo_next - 60.0).abs() < 4.0, "{lo_next}");
        // Between them they cover the whole road, once.
        assert!(lo_here.abs() < 4.0, "{lo_here}");
        assert!((hi_next - 200.0).abs() < 4.0, "{hi_next}");
        // Both halves carry the road's marking data: the dashed centre line
        // (r = 1), the edge lines (g = 1), and the half-width for the u.
        for patch in here.iter().chain(next.iter()) {
            assert_eq!(patch.colors[0][0], 1.0, "dashed");
            assert_eq!(patch.colors[0][1], 1.0, "edge lines");
            assert!((patch.colors[0][2] - 3.0).abs() < 0.01, "the half-width");
        }
    }

    #[test]
    fn the_urban_dash_rides_in_the_mesh() {
        // The residential preset is an innerorts street: the shorter RMS
        // dash, travelling in the mesh as r = 2 so the shader paints the
        // 3-and-6 rather than the 6-and-12 of the country roads.
        assert_eq!(
            preset("residential").map(|p| p.center_line),
            Some(CenterLine::DashedUrban)
        );
        let tile = (859, 11162);
        let min = DVec2::new(tile.0 as f64 * 512.0, tile.1 as f64 * 512.0);
        let mut road = source(min.x + 100.0, min.y + 250.0, 200.0, 6.0);
        road.center_line = CenterLine::DashedUrban;
        let patches = patches_of(std::slice::from_ref(&road), tile);
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].colors[0][0], 2.0, "the urban dash");
    }

    #[test]
    fn a_bridge_spans_the_hollow() {
        let tile = (859, 11162);
        let min = DVec2::new(tile.0 as f64 * 512.0, tile.1 as f64 * 512.0);
        // A tile whose ground dips to 85 m in a band through the middle; the
        // shaped ground the abutments are measured on stays at 100 m.
        let step = 8.0;
        let n = 64;
        let mut heights = vec![100.0f32; (n + 1) * (n + 1)];
        for iy in 0..=n {
            for ix in 0..=n {
                let north = min.y + iy as f64 * step;
                if (min.y + 200.0..min.y + 312.0).contains(&north) {
                    heights[iy * (n + 1) + ix] = 85.0;
                }
            }
        }
        let grid = HeightGrid::new(min, &heights, step, n);
        let centre = min + DVec2::splat(256.0);
        let (clat, clon) = geo::from_utm(centre.x, centre.y, 32);
        let frame = EnuFrame::at(geo::to_ecef(clat, clon, 0.0));
        let mut ground = |_: DVec2| 100.0;

        // A north-south road through the dip, 400 m long: as a bridge it
        // holds the line between its own ends; on the ground it follows the
        // hollow down.
        let mut flying = source(min.x + 100.0, min.y + 56.0, 6.0, 6.0);
        flying.points = vec![
            point(min.x + 100.0, min.y + 56.0),
            point(min.x + 100.0, min.y + 456.0),
        ];
        flying.bridge = true;
        let roads = Roads::from_parts(&[flying], 32, 512.0);
        let bridge = patches(tile, &grid, &frame, 32, 512.0, &roads, &mut ground);
        assert_eq!(bridge.len(), 1);
        // The deck never dips: every vertex sits at the chord, the drape's
        // 100 m plus the lifts, also — especially — in the band of the hollow.
        for v in &bridge[0].positions {
            assert!(v[1] > 99.5, "deck dips: {}", v[1]);
        }
        // The deck of the hollow is shaded as the span it is: normals up.
        for n in &bridge[0].normals {
            assert!(n[1] > 0.99, "deck normal off: {n:?}");
        }

        // The same road without the bridge flag follows the hollow down.
        let mut grounded = source(min.x + 300.0, min.y + 56.0, 6.0, 6.0);
        grounded.points = vec![
            point(min.x + 300.0, min.y + 56.0),
            point(min.x + 300.0, min.y + 456.0),
        ];
        let roads = Roads::from_parts(&[grounded], 32, 512.0);
        let drape = patches(tile, &grid, &frame, 32, 512.0, &roads, &mut ground);
        assert_eq!(drape.len(), 1);
        let lowest = drape[0]
            .positions
            .iter()
            .map(|v| v[1])
            .fold(f32::MAX, f32::min);
        assert!(lowest < 86.5, "no hollow followed: {lowest}");
    }

    /// The u the shader multiplies by the width has to be the metre across
    /// the carriageway, or the surface is stretched and the edge lines are
    /// painted in the driving lane: at the kerbs it is 0 and 1, on the centre
    /// line 0.5, and in between it runs straight.
    #[test]
    fn the_uvs_read_the_road() {
        let tile = (859, 11162);
        let min = DVec2::new(tile.0 as f64 * 512.0, tile.1 as f64 * 512.0);
        let patches = patches_of(&[source(min.x + 100.0, min.y + 250.0, 200.0, 6.0)], tile);
        let us: Vec<f32> = patches[0].uvs.iter().map(|uv| uv[0]).collect();
        let lo = us.iter().copied().fold(f32::MAX, f32::min);
        let hi = us.iter().copied().fold(f32::MIN, f32::max);
        assert!(lo.abs() < 1e-5, "{lo}");
        assert!((hi - 1.0).abs() < 1e-5, "{hi}");
        // The u is the position across, not the position across squeezed
        // into the middle of the road: the vertices that share a v stand in
        // one rib across the carriageway, and the metres between two of them
        // are their difference in u times the width.
        let mut ribs: HashMap<u32, Vec<(f32, [f32; 3])>> = HashMap::new();
        for (uv, p) in patches[0].uvs.iter().zip(&patches[0].positions) {
            ribs.entry(uv[1].to_bits()).or_default().push((uv[0], *p));
        }
        assert!(ribs.len() > 1, "one rib is no ribbon");
        for rib in ribs.values_mut() {
            rib.sort_by(|a, b| a.0.total_cmp(&b.0));
            for pair in rib.windows(2) {
                let (a, b) = (
                    DVec3::from(pair[0].1.map(f64::from)),
                    DVec3::from(pair[1].1.map(f64::from)),
                );
                let want = (pair[1].0 - pair[0].0) as f64 * 6.0;
                assert!(
                    (a.distance(b) - want).abs() < 0.01,
                    "{} m for {want} m",
                    a.distance(b)
                );
            }
        }
    }

    /// The v is the metre along the road, so the dashes of one road keep
    /// their phase from one tile to the next, and the surface texture repeats
    /// in metres rather than in tiles.
    #[test]
    fn the_v_is_the_metre_along_the_road() {
        let tile = (859, 11162);
        let min = DVec2::new(tile.0 as f64 * 512.0, tile.1 as f64 * 512.0);
        let patches = patches_of(&[source(min.x + 100.0, min.y + 250.0, 200.0, 6.0)], tile);
        let vs: Vec<f32> = patches[0].uvs.iter().map(|uv| uv[1]).collect();
        let lo = vs.iter().copied().fold(f32::MAX, f32::min);
        let hi = vs.iter().copied().fold(f32::MIN, f32::max);
        assert!(lo.abs() < 0.05, "{lo}");
        assert!((hi - 200.0).abs() < 0.2, "{hi}");
    }

    #[test]
    fn a_corner_is_mitered_not_spiked() {
        // A right-angle corner: the miter stretches to the bisector, but the
        // clamp keeps it near the carriageway it belongs to.
        let centre = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(100.0, 0.0),
            DVec2::new(100.0, 100.0),
        ];
        let (left, right) = kerbs(&centre, 3.0);
        assert_eq!((left.len(), right.len()), (3, 3));
        // Every kerb point stays within the clamped miter's reach of the
        // centre line — 4.5 m here (3 m · 1.5), where an unclamped 90° miter
        // would reach √2 · 3 m and a spike would reach much further.
        let on_line = |p: &DVec2| -> f64 {
            let mut best = f64::MAX;
            for pair in centre.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                let d = b - a;
                let t = ((p - a).dot(d) / d.length_squared()).clamp(0.0, 1.0);
                best = best.min(p.distance(a + d * t));
            }
            best
        };
        for p in left.iter().chain(&right) {
            assert!(on_line(p) <= 3.0 * 1.5 + 1e-9, "{p:?}");
        }
    }

    /// A hairpin on short edges is what folds a mitered ribbon over itself:
    /// the corner reaches back further than the segment is long, the quad
    /// becomes a bow-tie and shades as a hole. The reach clamp is what keeps
    /// the two kerbs running the same way as the centre line they belong to.
    #[test]
    fn a_hairpin_does_not_fold_the_ribbon() {
        let centre = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(8.0, 0.0),
            DVec2::new(8.5, 3.0),
            DVec2::new(0.0, 6.0),
        ];
        let half = 4.0;
        let (left, right) = kerbs(&centre, half);
        for i in 0..centre.len() - 1 {
            let d = (centre[i + 1] - centre[i]).normalize();
            for side in [&left, &right] {
                let run = (side[i + 1] - side[i]).dot(d);
                assert!(run > 0.0, "segment {i} folds over: {run}");
            }
        }
    }
    #[test]
    fn the_repeats_of_a_way_are_dropped() {
        let line = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(0.0, 0.0),
            DVec2::new(50.0, 0.0),
            DVec2::new(50.0, 50.0),
        ];
        assert_eq!(dedupe(line).len(), 3);
    }

    /// The carriageway is cut fine enough to follow the ground it is draped
    /// on: no triangle spans more than the height grid's own step, so the
    /// road cannot cut a corner the terrain under it takes.
    #[test]
    fn the_ribbon_is_cut_to_the_height_grid() {
        let tile = (859, 11162);
        let min = DVec2::new(tile.0 as f64 * 512.0, tile.1 as f64 * 512.0);
        let patches = patches_of(&[source(min.x + 100.0, min.y + 250.0, 200.0, 7.5)], tile);
        let patch = &patches[0];
        let longest = patch
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|t| {
                let p = |i: u32| {
                    let v = patch.positions[i as usize];
                    DVec3::new(v[0] as f64, v[1] as f64, v[2] as f64)
                };
                [
                    p(t[0]).distance(p(t[1])),
                    p(t[1]).distance(p(t[2])),
                    p(t[2]).distance(p(t[0])),
                ]
            })
            .fold(0.0f64, f64::max);
        // The grid of the test tile steps 8 m, so the road steps 4 m — and a
        // cell's diagonal is the longest edge a triangle of it can have.
        assert!(longest < 4.0 * 2f64.sqrt() + 0.1, "{longest}");
    }

    /// A road running north from `(e, n)` for `len` metres.
    fn crossing_source(e: f64, n: f64, len: f64, width: f64) -> RoadSource {
        RoadSource {
            points: vec![point(e, n), point(e, n + len)],
            width,
            ..source(e, n, len, width)
        }
    }

    /// The markings of both roads stop where two carriageways cross: what the
    /// ground has at a crossroads is a square of plain asphalt, not the two
    /// roads' own stripes drawn through each other into a lattice.
    #[test]
    fn a_crossroads_takes_the_markings_out() {
        let (e, n) = (440_000.0, 5_715_000.0);
        let west_east = source(e - 100.0, n, 200.0, 7.0);
        let south_north = crossing_source(e, n - 100.0, 200.0, 7.0);
        let roads = Roads::from_parts(&[west_east, south_north], 32, 512.0);
        for road in &roads.roads {
            // Clear of the crossing the markings run, at it they are gone.
            assert_eq!(markings(&road.blank_edges, 10.0), 1.0, "clear of it");
            assert_eq!(markings(&road.blank_edges, 100.0), 0.0, "at the crossing");
            assert_eq!(markings(&road.blank_centre, 100.0), 0.0, "at the crossing");
            // And the blank is a junction's worth, not the whole road.
            let (lo, hi) = road.blank_edges[0];
            assert!(hi - lo < 20.0, "{lo}..{hi}");
        }
    }

    /// An extract splits a street at every change of tagging and at every
    /// side road. Where two ways simply carry on from one another the
    /// markings have to carry on too, or a street would be dashed rather than
    /// its centre line.
    #[test]
    fn a_split_street_keeps_its_markings() {
        let (e, n) = (440_000.0, 5_715_000.0);
        let first = source(e, n, 100.0, 7.0);
        let second = source(e + 100.0, n, 100.0, 7.0);
        let roads = Roads::from_parts(&[first, second], 32, 512.0);
        for road in &roads.roads {
            assert!(road.blank_edges.is_empty(), "{:?}", road.blank_edges);
            assert!(road.blank_centre.is_empty());
        }
        // They do share their ground at the joint, so they are told apart by
        // layer — two carriageways on one layer trade fragments along a torn
        // edge, which is the speckle a junction used to show.
        assert_ne!(roads.roads[0].lift, roads.roads[1].lift);
    }

    /// A field track crossing a Bundesstraße breaks its edge line, as it does
    /// on the ground — the mouth is a gap in the Randlinie — but the through
    /// line keeps running. Only a road at least as wide stops that.
    #[test]
    fn the_narrower_road_yields_its_centre_line() {
        let (e, n) = (440_000.0, 5_715_000.0);
        let federal = source(e - 100.0, n, 200.0, 7.5);
        let track = crossing_source(e, n - 100.0, 200.0, 3.0);
        let roads = Roads::from_parts(&[federal, track], 32, 512.0);
        let (wide, narrow) = (&roads.roads[0], &roads.roads[1]);
        assert_eq!(markings(&wide.blank_edges, 100.0), 0.0, "the mouth");
        assert_eq!(markings(&wide.blank_centre, 100.0), 1.0, "the through line");
        assert_eq!(markings(&narrow.blank_centre, 100.0), 0.0, "the side road");
    }

    /// Two roads that cross are drawn as **one** surface, not as two ribbons
    /// lying over one another: the junction takes the ground where they meet
    /// and carries it itself, so the asphalt is the union of the two
    /// carriageways rather than their sum.
    #[test]
    fn a_crossroads_is_one_surface() {
        let tile = (859, 11162);
        let min = DVec2::new(tile.0 as f64 * 512.0, tile.1 as f64 * 512.0);
        let (e, n) = (min.x + 256.0, min.y + 256.0);
        let patches = patches_of(
            &[
                source(e - 100.0, n, 200.0, 7.0),
                crossing_source(e, n - 100.0, 200.0, 7.0),
            ],
            tile,
        );
        assert_eq!(patches.len(), 1, "one surface kind, one patch");
        assert_eq!(patches[0].sources, vec![0, 1], "a click takes both roads");
        // Two 200 m carriageways of 7 m are 2800 m² laid end to end, and 49 m²
        // of that is the same ground twice where they cross. The junction's
        // rounded corners give a little of it back.
        let area = patch_area(&patches[0]);
        assert!((2740.0..2790.0).contains(&area), "{area}");
    }

    /// Two ways that carry on from one another are not a junction, and
    /// nothing is merged: an extract splits a street at every change of
    /// tagging, and a junction at each of them would be a street of squares.
    #[test]
    fn a_split_street_is_not_merged() {
        let tile = (859, 11162);
        let min = DVec2::new(tile.0 as f64 * 512.0, tile.1 as f64 * 512.0);
        let (e, n) = (min.x + 156.0, min.y + 256.0);
        let patches = patches_of(
            &[source(e, n, 100.0, 7.0), source(e + 100.0, n, 100.0, 7.0)],
            tile,
        );
        let area = patch_area(&patches[0]);
        assert!((1390.0..1410.0).contains(&area), "{area}");
    }

    /// A closed way — a roundabout, a loop — has no ends, and the ribbon has
    /// to run through the point the file happens to start at, or the ring
    /// shows a notch there.
    #[test]
    fn a_ring_closes_without_a_notch() {
        let centre: Vec<DVec2> = (0..=12)
            .map(|i| {
                let a = i as f64 / 12.0 * std::f64::consts::TAU;
                DVec2::new(20.0 * a.cos(), 20.0 * a.sin())
            })
            .collect();
        assert!(is_closed(&centre));
        let (left, right) = kerbs(&centre, 3.0);
        let last = centre.len() - 1;
        assert!(left[0].distance(left[last]) < 1e-9, "the ring closes");
        assert!(right[0].distance(right[last]) < 1e-9);
        // And it is a ring of even width, not one with a corner at its seam:
        // every kerb point stands a half-width off the centre line.
        for (i, p) in left.iter().chain(&right).enumerate() {
            let radius = p.length();
            assert!(
                (radius - 20.0).abs() < 3.0 + 0.2,
                "kerb {i} at radius {radius}"
            );
        }
    }

    #[test]
    fn the_surfaces_come_out_separately() {
        let tile = (859, 11162);
        let min = DVec2::new(tile.0 as f64 * 512.0, tile.1 as f64 * 512.0);
        let mut concrete = source(min.x + 250.0, min.y + 100.0, 150.0, 6.0);
        concrete.surface = RoadSurface::Concrete;
        let patches = patches_of(
            &[source(min.x + 50.0, min.y + 300.0, 150.0, 6.0), concrete],
            tile,
        );
        assert_eq!(patches.len(), 2);
        // Sorted by surface, so the same tile always builds the same
        // entities: asphalt before concrete.
        assert_eq!(patches[0].surface, RoadSurface::Asphalt);
        assert_eq!(patches[1].surface, RoadSurface::Concrete);
    }
}
