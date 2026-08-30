//! Roads as ground to look at: centre line in, draped carriageway out.
//!
//! A [`crate::route::RoadSource`] is what OSM maps a street as — a centre
//! line with a class (`highway=*`), plus the width, surface and markings that
//! turn the line into a carriageway. What the run needs is a ribbon of
//! carriageway lying on the terrain: the width either side of the line, the
//! surface texture of it, the markings a German road carries.
//!
//! The centre line is buffered into a closed outline — one carriageway-width
//! either side, square caps at the ends — and the outline is then cut to the
//! terrain tiles and draped on them exactly as the fields are: a road of two
//! kilometres crosses a dozen tiles and is streamed as one patch per tile it
//! touches, and the pieces two neighbouring tiles cut out of one road meet
//! without a seam. What the run needs beyond the shape rides in the mesh: the
//! markings (which of them to draw) and the half-width travel in the vertex
//! colours, the along-road metre in the UVs.
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

/// How far a road's surface is lifted off the terrain [m]. Above the fields'
/// lift, so a field track the import did not punch out of the crops still
/// wins over the crop — and above the water's lift, for the ford nobody
/// should drive into unknowingly.
pub const LIFT: f64 = 0.06;

/// Step of the resampled centre line [m]. The buffered outline is built from
/// it, and the mesh is as fine along the road as this is fine — eight metres
/// follows the sweep of a country road without a vertex per OSM node.
const ROAD_STEP: f64 = 8.0;

/// The road presets the editor offers — the widths of the German road system,
/// from the Autobahn carriageway down to the footpath, asphalt and concrete,
/// with and without the centre line. The widths are the planning values of
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
}

/// One road, ready to be cut up: the resampled centre line, the carriageway
/// buffered around it, and what the shader needs to know about it.
#[derive(Debug, Clone)]
struct Road {
    /// The centre line [m UTM], resampled to [`ROAD_STEP`] — the geometry the
    /// markings are measured against.
    centre: Vec<DVec2>,
    /// Arc length at each centre point [m], from the road's own start — the
    /// dash phase runs in it, and the bridge chord is measured on it.
    s: Vec<f64>,
    /// The carriageway as a closed ring: the centre line buffered by its own
    /// half-width, square caps at the ends.
    ring: Vec<DVec2>,
    surface: RoadSurface,
    center_line: CenterLine,
    edge_lines: bool,
    /// Whether the way flies (`bridge=*`): where the ground dips below the
    /// line between the way's own ends, the carriageway holds that line —
    /// the deck of a bridge — instead of following the hollow.
    bridge: bool,
    /// Half the carriageway width [m], as the file said it — clamped only
    /// where a bad file would make the buffer explode.
    half: f64,
    /// The road's own share of the lift off the ground [m] — index-derived,
    /// so two roads crossing at a junction never fight over the same
    /// fragment, on any machine.
    lift: f64,
    /// Index in [`LineSource::roads`] — what the editor selects.
    index: u32,
}

impl Road {
    /// Where a point stands on the road: the metre across (`lateral`, signed,
    /// 0 on the centre line) and the metre along (`along`, from the road's
    /// own start). Measured against the resampled centre line, so both tiles
    /// at a seam read the same numbers.
    fn frame(&self, p: DVec2) -> (f64, f64) {
        let mut best = (f64::MAX, 0.0, 0.0);
        for (i, pair) in self.centre.windows(2).enumerate() {
            let (a, b) = (pair[0], pair[1]);
            let d = b - a;
            let len2 = d.length_squared();
            if len2 < 1e-12 {
                continue;
            }
            let t = ((p - a).dot(d) / len2).clamp(0.0, 1.0);
            let foot = a + d * t;
            let dist = p.distance(foot);
            if dist < best.0 {
                let along = self.s[i] + t * (self.s[i + 1] - self.s[i]).abs();
                // Positive lateral is the side the offset put on `+normal`
                // (the buffer's right list), so the u runs 0…1 against it.
                let lateral = d.perp_dot(p - foot) / d.length();
                best = (dist, lateral, along);
            }
        }
        (best.1, best.2)
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
            if centre.len() < 2 {
                continue;
            }
            let centre = resample(&centre, ROAD_STEP);
            let s = arc_lengths(&centre);
            let half = (source.width.max(0.5) / 2.0).min(15.0);
            let ring = buffer(&centre, half);
            let at = out.roads.len();
            // Half a width of margin over the bounding box: a road that only
            // reaches a tile by the width of its carriageway is still on it.
            let (lo, hi) = fields::geometry::bounds(&ring);
            let grow = DVec2::splat(half + 1.0);
            let (kx0, ky0) = key(lo - grow, tile_size);
            let (kx1, ky1) = key(hi + grow, tile_size);
            for ky in ky0..=ky1 {
                for kx in kx0..=kx1 {
                    out.by_tile.entry((kx, ky)).or_default().push(at);
                }
            }
            out.roads.push(Road {
                centre,
                s,
                ring,
                surface: source.surface,
                center_line: source.center_line,
                edge_lines: source.edge_lines,
                bridge: source.bridge,
                half,
                lift: (index % 16) as f64 * 0.0015,
                index: index as u32,
            });
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
        self.by_tile.contains_key(&k)
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
    /// `u` across the carriageway (0 at one kerb, 1 at the other), `v` along
    /// it in metres from the road's own start — the dash phase runs in it, so
    /// the markings of one road line up across the tile boundaries it
    /// crosses, and the texture repeats without a seam between the tiles.
    pub uvs: Vec<[f32; 2]>,
    /// Per-vertex data: `r` the centre line ([`crate::route::CenterLine`] as
    /// a number — 1 dashed außerorts, 2 dashed innerorts, 3 solid), `g`
    /// whether the edge lines run, `b` the half-width [m] — the three things
    /// the shader needs to draw the markings of the road this vertex belongs
    /// to.
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
    let Some(indices) = roads.by_tile.get(&k) else {
        return Vec::new();
    };
    let min = DVec2::new(k.0 as f64 * tile_size, k.1 as f64 * tile_size);
    // The tile itself: a road is cut to it exactly, and the neighbouring tile
    // cuts the other half the same way, so the two meet without a seam.
    let rect = vec![
        min,
        DVec2::new(min.x + tile_size, min.y),
        min + DVec2::splat(tile_size),
        DVec2::new(min.x, min.y + tile_size),
    ];

    let mut by_surface: HashMap<RoadSurface, RoadPatch> = HashMap::new();
    for &at in indices {
        let road = &roads.roads[at];
        for piece in fields::geometry::clip(&road.ring, &rect, fields::geometry::Op::Intersect) {
            let patch = by_surface.entry(road.surface).or_insert_with(|| RoadPatch {
                surface: road.surface,
                positions: Vec::new(),
                normals: Vec::new(),
                uvs: Vec::new(),
                colors: Vec::new(),
                indices: Vec::new(),
                sources: Vec::new(),
            });
            if !patch.sources.contains(&road.index) {
                patch.sources.push(road.index);
            }
            add_piece(patch, &piece, road, grid, frame, zone, ground);
        }
    }

    let mut out: Vec<RoadPatch> = by_surface
        .into_values()
        .filter(|p| !p.indices.is_empty())
        .collect();
    // A stable order, so the same tile always builds the same entities.
    out.sort_by_key(|p| p.surface);
    out
}

/// Triangulates one piece of a road, lays it on the tile's own height grid —
/// or, where the road flies, on its bridge chord — and writes the marking
/// data and the texture coordinates the shader needs.
fn add_piece(
    patch: &mut RoadPatch,
    ring: &[DVec2],
    road: &Road,
    grid: &HeightGrid<'_>,
    frame: &EnuFrame,
    zone: u8,
    ground: &mut dyn FnMut(DVec2) -> f64,
) {
    let mut points = ring.to_vec();
    let mut tris = fields::geometry::triangulate(&points);
    if tris.is_empty() {
        return;
    }
    // The centre line was resampled to [`ROAD_STEP`], so the buffered outline
    // is already fine along the road; one uniform split bends the triangles
    // across it where a corner drape needs it. The same conforming split the
    // fields and the water use, so it cannot crack.
    refine(&mut points, &mut tris, 1);

    let base = patch.positions.len() as u32;
    if !patch.sources.contains(&road.index) {
        patch.sources.push(road.index);
    }

    // The ends of a bridge's chord, deck height included: the shaped ground
    // under the way's own first and last point. A bridge way spans abutment
    // to abutment, so its ends *are* the abutments, and the deck meets the
    // draped road there.
    let ends = if road.bridge {
        Some(buttress_heights(road, ground))
    } else {
        None
    };

    let centre_kind = road.center_line as usize as f32;
    let edge = road.edge_lines as u32 as f32;
    for p in &points {
        let height = deck_height(road, *p, grid, ends);
        let (lat, lon) = geo::from_utm(p.x, p.y, zone);
        let world = geo::to_ecef(lat, lon, height);
        patch.positions.push(to_render(frame.to_local(world)));
        patch.normals.push(if road.bridge {
            deck_normal(*p, road, grid, ends)
        } else {
            normal_at(*p, grid)
        });
        // Where the vertex stands on the road: the metre along it (the dash
        // phase) and across it (0 at one kerb, 1 at the other). Measured
        // against the resampled centre line, so both tiles at a seam read the
        // same numbers.
        let (lateral, along) = road.frame(*p);
        patch.uvs.push([
            ((0.5 + lateral / road.half).clamp(0.0, 1.0)) as f32,
            along as f32,
        ]);
        patch
            .colors
            .push([centre_kind, edge, road.half as f32, 1.0]);
    }
    for [a, b, c] in tris {
        patch
            .indices
            .extend_from_slice(&[base + a, base + b, base + c]);
    }
}

/// The height a road vertex is laid at: the drape on the tile's grid — or,
/// where the ground dips below the straight line between the way's own ends,
/// that chord: the deck of a bridge. The drape still wins wherever the
/// ground is above it, so the deck runs exactly as far as the hollow does.
fn deck_height(road: &Road, p: DVec2, grid: &HeightGrid<'_>, ends: Option<(f64, f64)>) -> f64 {
    let drape = grid.at(p) + LIFT + road.lift;
    let Some((h0, h1)) = ends else {
        return drape;
    };
    let (_, along) = road.frame(p);
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

impl Road {
    /// Length of the resampled centre line [m].
    fn length(&self) -> f64 {
        self.s.last().copied().unwrap_or(0.0)
    }
}

/// The deck's own normal, by finite differences of the height the vertex is
/// laid at — drape and chord both — so a bridge is shaded like the span it
/// is and not like the valley it crosses. The same finite difference
/// [`normal_at`] takes on the ground.
fn deck_normal(p: DVec2, road: &Road, grid: &HeightGrid<'_>, ends: Option<(f64, f64)>) -> [f32; 3] {
    const D: f64 = 1.0;
    let dx = deck_height(road, p + DVec2::new(D, 0.0), grid, ends)
        - deck_height(road, p - DVec2::new(D, 0.0), grid, ends);
    let dy = deck_height(road, p + DVec2::new(0.0, D), grid, ends)
        - deck_height(road, p - DVec2::new(0.0, D), grid, ends);
    // Render axes: +x east, +y up, +z south — so north is −z.
    let n = Vec3::new(-(dx / (2.0 * D)) as f32, 1.0, (dy / (2.0 * D)) as f32).normalize_or_zero();
    let n = if n == Vec3::ZERO { Vec3::Y } else { n };
    [n.x, n.y, n.z]
}

/// Resamples a polyline so the in-betweens follow the road's own step — the
/// original vertices stay (a corner is a corner); between two, as many
/// in-betweens as [`ROAD_STEP`] asks for.
fn resample(line: &[DVec2], step: f64) -> Vec<DVec2> {
    let mut out = Vec::with_capacity(line.len() * 2);
    for pair in line.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let len = a.distance(b);
        out.push(a);
        // Closer than the step already: keep as is, no invention.
        let n = (len / step).floor() as usize;
        for k in 1..=n {
            let t = k as f64 * step / len;
            if t < 1.0 {
                out.push(a + (b - a) * t);
            }
        }
    }
    out.push(*line.last().expect("checked non-empty"));
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

/// The centre line buffered into a closed ring: the centre line offset a
/// half-width to each side, mitered at the corners and capped square at the
/// ends. One pass out and back, joining the two sides at the far cap — a
/// polyline of N points becomes a ring of 2N points.
fn buffer(centre: &[DVec2], half: f64) -> Vec<DVec2> {
    let n = centre.len();
    if n < 2 {
        return Vec::new();
    }
    // The unit direction of the edge(s) meeting at each point: the first
    // point takes the first edge's, the last point the last edge's, the
    // points between the average of their two — and the average is *by sum*,
    // so a 180° double-back keeps a finite length instead of cancelling.
    let mut left = Vec::with_capacity(n);
    let mut right = Vec::with_capacity(n);
    for (i, p) in centre.iter().enumerate() {
        let d = dir_of(centre, i);
        let normal = DVec2::new(-d.y, d.x);
        // A corner wider than a right angle would put its miter point as far
        // out again as the half-width — a sharp junction sprouts spikes. The
        // miter is clamped by how far it may stretch past the round join the
        // real road has.
        let miter = miter_stretch(centre, i).clamp(1.0, 2.5);
        right.push(p + normal * (half * miter));
        left.push(p - normal * (half * miter));
    }
    // One ring: up the right side, back down the left; the closing side and
    // the first edge square the two caps between them.
    let mut ring = right;
    ring.extend(left.into_iter().rev());
    ring
}

/// The averaged unit direction of the line at point `i` (see [`buffer`]).
fn dir_of(centre: &[DVec2], i: usize) -> DVec2 {
    let n = centre.len();
    if n == 2 || i == 0 {
        return (centre[1] - centre[0]).normalize_or_zero();
    }
    if i == n - 1 {
        return (centre[n - 1] - centre[n - 2]).normalize_or_zero();
    }
    ((centre[i] - centre[i - 1]).normalize_or_zero()
        + (centre[i + 1] - centre[i]).normalize_or_zero())
    .normalize_or_zero()
}

/// How far the miter at point `i` stretches past the round join [as a
/// factor]: the reciprocal of the cosine of half the corner angle, which
/// grows without bound as the turn closes — hence the clamp in [`buffer`].
fn miter_stretch(centre: &[DVec2], i: usize) -> f64 {
    let n = centre.len();
    if n < 3 || i == 0 || i == n - 1 {
        return 1.0;
    }
    let d0 = (centre[i] - centre[i - 1]).normalize_or_zero();
    let d1 = (centre[i + 1] - centre[i]).normalize_or_zero();
    // cos of half the turn: sqrt((1 + cos θ) / 2), θ the direction change.
    let cos_half = ((d0.dot(d1) / 2.0 + 0.5).clamp(0.0, 1.0)).sqrt();
    1.0 / cos_half.max(0.4)
}

/// The ground's normal under a point, from the height grid's own gradient —
/// the same finite difference both sides of a tile seam, so the shading does
/// not crease where the mesh is cut.
fn normal_at(p: DVec2, grid: &HeightGrid<'_>) -> [f32; 3] {
    const D: f64 = 1.0;
    let dx = grid.at(p + DVec2::new(D, 0.0)) - grid.at(p - DVec2::new(D, 0.0));
    let dy = grid.at(p + DVec2::new(0.0, D)) - grid.at(p - DVec2::new(0.0, D));
    // Render axes: +x east, +y up, +z south — so north is −z.
    let n = Vec3::new(-(dx / (2.0 * D)) as f32, 1.0, (dy / (2.0 * D)) as f32).normalize_or_zero();
    let n = if n == Vec3::ZERO { Vec3::Y } else { n };
    [n.x, n.y, n.z]
}

/// Splits every triangle into four, once. Uniform rather than by edge
/// length: a mesh subdivided the same everywhere cannot crack, and a road is
/// smooth enough that the wasted vertices are a few hundred bytes. The same
/// subdivision the farmland and the water use, so the three stay in step.
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
        let roads = Roads::from_parts(
            &[source(440_000.0, 5_715_000.0, 3_000.0, 6.0)],
            32,
            512.0,
        );
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
            patch
                .uvs
                .iter()
                .fold((f32::MAX, f32::MIN), |(lo, hi), uv| {
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

    #[test]
    fn the_uvs_read_the_road() {
        // Halfway along a 100 m road stands at 50 m; three metres north of
        // the centre line of a 6 m road is the kerb — u = 0.5 + 3/3 = 1.
        let road = source(440_000.0, 5_715_000.0, 100.0, 6.0);
        let roads = Roads::from_parts(&[road], 32, 512.0);
        // (A degree of tolerance: the centre line went degrees → UTM →
        // degrees, and the UTM scale factor is 0.9996.)
        let (lateral, along) = roads.roads[0].frame(DVec2::new(440_050.0, 5_715_000.0));
        assert!(lateral.abs() < 0.01, "{lateral}");
        assert!((along - 50.0).abs() < 0.5, "{along}");
        let (lateral, _) = roads.roads[0].frame(DVec2::new(440_050.0, 5_715_003.0));
        assert!((lateral - 3.0).abs() < 0.2, "{lateral}");
        let (lateral, _) = roads.roads[0].frame(DVec2::new(440_050.0, 5_714_997.0));
        assert!((lateral + 3.0).abs() < 0.2, "{lateral}");
        // And the mesh's u runs 0 at one kerb through 0.5 on the line.
        let tile = (859, 11162);
        let min = DVec2::new(tile.0 as f64 * 512.0, tile.1 as f64 * 512.0);
        let patches = patches_of(&[source(min.x + 100.0, min.y + 250.0, 200.0, 6.0)], tile);
        let us: Vec<f32> = patches[0].uvs.iter().map(|uv| uv[0]).collect();
        let lo = us.iter().copied().fold(f32::MAX, f32::min);
        let hi = us.iter().copied().fold(f32::MIN, f32::max);
        assert!(lo.abs() < 0.02, "{lo}");
        assert!((hi - 1.0).abs() < 0.02, "{hi}");
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
        let ring = buffer(&centre, 3.0);
        assert_eq!(ring.len(), 6);
        // Every ring point stays within the clamped miter's reach of the
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
        for p in &ring {
            assert!(on_line(p) <= 3.0 * 1.5 + 1e-9, "{p:?}");
        }
    }

    #[test]
    fn the_resample_keeps_the_corners() {
        let centre = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(50.0, 0.0),
            DVec2::new(50.0, 50.0),
        ];
        let fine = resample(&centre, 8.0);
        assert!(fine.contains(&DVec2::new(50.0, 0.0)), "the corner stays");
        assert!(fine.len() > 3, "{fine:?}");
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
