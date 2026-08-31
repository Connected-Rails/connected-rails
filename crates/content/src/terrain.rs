//! Terrain meshes from the DGM (plan ch. 14).
//!
//! Only the corridor around the line is generated, split into tiles and with a
//! distance-dependent resolution. Everything is computed in UTM — that is the system of
//! the DGM, so each support point costs exactly one projection instead of three.
//!
//! The number that matters: one square kilometre of DGM1 has two million triangles.
//! Hence
//!
//! * **tiles** (512 m by default) → frustum culling and view distance limit per tile,
//! * **LOD by track distance** → 4 m at the track, 32 m at the edge of the corridor,
//! * **skirts** at the tile borders → no cracks between different levels,
//! * **cutting/embankment**: close to the track the terrain is pulled to the formation
//!   (`rail_offset` below the top of rail), otherwise the alignment would sit inside
//!   the hill — or the ballast bed inside its own ground.
//!
//! [`build`] creates the whole corridor at once (tests, tools). For a line of any real
//! length the app uses [`TerrainBuilder`] instead and builds single tiles by key while
//! driving (plan 4.3).

use crate::import::dgm::{HeightTile, TerrainSource};
use crate::people::{Crowd, PersonInstance, Walkway, scatter_people, scatter_walkways};
use crate::route::{LineSource, ObjectSource, TerrainEdit, TerrainEditSource, TreeSource};
use glam::{DQuat, DVec2, DVec3};
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};
use std::sync::Arc;
use track_model::TrackNetwork;
use world_coords::{EcefPos, EnuFrame, geo};

/// Hasher for the integer cell keys of the grids below. The standard one is
/// SipHash, built to survive hostile input; a tile build asks the centreline
/// grid nine times per vertex, and sixteen thousand vertices times nine
/// SipHash rounds is a measurable slice of the build for no gain.
#[derive(Default)]
pub(crate) struct CellHasher(u64);

impl Hasher for CellHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(b as u64);
        }
    }

    fn write_u64(&mut self, value: u64) {
        // One round of splitmix over the running state: cheap, and the
        // neighbouring cells a build visits do not collide.
        let mut z = self.0 ^ value.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        self.0 = z ^ (z >> 31);
    }

    fn write_i64(&mut self, value: i64) {
        self.write_u64(value as u64);
    }
}

pub(crate) type CellMap<V> = HashMap<(i64, i64), V, BuildHasherDefault<CellHasher>>;

/// Settings for the terrain generation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerrainOptions {
    /// UTM zone used for the computation (that of the DGM).
    pub zone: u8,
    /// Geoid undulation of the line [m] (NHN → ellipsoidal).
    pub geoid_offset: f64,
    /// Edge length of a terrain tile [m].
    pub tile_size: f64,
    /// How far beside the track terrain is generated [m].
    pub radius: f64,
    /// Finest grid spacing [m] (applies inside the corridor).
    pub base_step: f64,
    /// Up to here the finest level applies [m].
    pub corridor: f64,
    /// Up to here the terrain follows the track exactly [m] — the half width
    /// of the formation (Planum) of a single track: the 4.85 m ballast body
    /// (2.6 m of sleeper, a 0.4 m shoulder each side and the 1:1.5 slopes)
    /// plus the Randweg beside it. Edges without a formation
    /// ([`TrackEdge::formation`](track_model::TrackEdge::formation)) shape
    /// nothing here.
    pub flatten: f64,
    /// How far the ground beside the track lies **below** the top of rail [m].
    /// This is the Planum, and it is not a made-up clearance: the DB
    /// Regeloberbau stacks 172 mm of rail, a 10 mm pad, a 214 mm sleeper and
    /// 300 mm of ballast under it, so the formation lies **696 mm** under the
    /// top of rail ([`track_model::REGEL_PLANUM`]). Ground pulled up any
    /// higher than that buries the bed it is supposed to carry — and a bed
    /// buried to its crest is exactly what makes a track read as a ladder
    /// lying on a road.
    pub rail_offset: f64,
    /// Up to here rail and terrain height are blended [m] — the foot of the
    /// embankment or cutting. From the formation edge to here the ground runs
    /// to its natural height, eight metres of run: roughly a 1:2 slope at the
    /// heights a main line embankment has, steeper where the ground falls
    /// away. An edge without a formation is not part of this.
    pub blend: f64,
    /// Up to here the gravel texture reaches [m] — full weight on the
    /// formation, fading out over the upper slope; the embankment itself is
    /// grass. Has to lie beyond [`TerrainOptions::flatten`] for the fade to
    /// make sense.
    pub gravel: f64,
    /// Height of the skirt at the tile borders [m].
    pub skirt: f64,
    /// Height where no DGM is available [m] (NHN).
    pub fallback_height: f64,
    /// Sampling distance along the track centreline [m].
    pub centerline_step: f64,
}

impl Default for TerrainOptions {
    fn default() -> Self {
        Self {
            zone: 32,
            geoid_offset: 46.0,
            tile_size: 512.0,
            radius: 1_200.0,
            base_step: 4.0,
            corridor: 96.0,
            flatten: 3.0,
            rail_offset: track_model::REGEL_PLANUM,
            blend: 12.0,
            gravel: 5.5,
            skirt: 8.0,
            fallback_height: 100.0,
            centerline_step: 25.0,
        }
    }
}

/// A finished terrain tile — raw mesh data, so that `content` works without Bevy.
#[derive(Debug, Clone)]
pub struct TerrainTile {
    /// Origin of the local ENU frame (tile centre) in world coordinates.
    pub anchor: EcefPos,
    /// Positions in render axes (x = east, y = up, z = −north), relative to the anchor.
    pub positions: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    /// Splat weights per vertex, summing to 1: grass, rock (steep ground) and
    /// gravel (the strip beside the track). The renderer blends its ground
    /// textures by them (plan ch. 14).
    pub splat: Vec<[f32; 4]>,
    /// Vegetation scattered on this tile — streamed instances (plan ch. 14).
    pub trees: Vec<Tree>,
    /// Scenery objects standing on this tile, their feet on its ground where
    /// they snap to it — streamed with the tile like the trees.
    pub objects: Vec<SceneryInstance>,
    /// The people waiting on this tile's platforms (plan ch. 12) — streamed
    /// with the tile like the objects, and derived like the trees of a forest:
    /// nothing about them is stored in the line.
    pub people: Vec<PersonInstance>,
    /// The ways people walk on this tile, in the tile's frame with their
    /// agents on them (plan ch. 12) — a walker's place at any moment is
    /// [`crate::people::stroll_pose`] of these, the seed and the clock.
    pub walkways: Vec<Walkway>,
    /// The farmland on this tile, one surface per crop (see
    /// [`crate::farmland`]) — draped on this tile's own ground, so it follows
    /// every hollow the terrain has.
    pub fields: Vec<crate::farmland::FieldPatch>,
    /// The water on this tile (see [`crate::water`]) — the surfaces of the
    /// lakes and rivers whose waterline reaches it, standing at the height
    /// the elevation data gives them.
    pub waters: Vec<crate::water::WaterPatch>,
    /// The roads on this tile (see [`crate::roads`]) — one surface per
    /// surface kind, draped on this tile's own ground like the fields.
    pub roads: Vec<crate::roads::RoadPatch>,
    /// The overhead line conductors crossing this tile (see [`crate::power`]) —
    /// the wires between the masts, hung as catenaries and cut to the tile. The
    /// masts themselves are instances and travel in `trees`.
    pub conductors: Vec<crate::power::ConductorPatch>,
    /// Grid spacing used [m].
    pub step: f64,
    /// LOD level (0 = finest).
    pub lod: u8,
    /// Bounding radius around the anchor [m] — for view distance and culling.
    pub radius: f32,
    /// South-west corner [m UTM] and the height grid the mesh was built from,
    /// row by row from the south — kept so the trees and objects of a tile can
    /// be placed anew without building its ground again (the editor moves an
    /// object; the hill under it has not changed).
    pub min: DVec2,
    pub heights: Vec<f32>,
}

/// One scenery object on a tile, in the tile's own frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneryInstance {
    /// Base point in render axes, relative to the tile anchor.
    pub pos: [f32; 3],
    /// Orientation in render axes (`x, y, z, w`): the model's front along
    /// the track plus the placement's yaw, its up the local vertical.
    pub rotation: [f32; 4],
    /// Index into [`Scenery::objects`].
    pub object: u16,
    /// Index of the placement in the line file — what the editor selects.
    pub index: u32,
}

/// One tree instance on a tile — a hand-placed [`TreeSource`] or one grown out
/// of a [`ForestSource`] polygon, deterministic from tile position and seed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tree {
    /// Foot point in render axes, relative to the tile anchor.
    pub pos: [f32; 3],
    /// Uniform scale on the object's own size.
    pub scale: f32,
    /// Rotation about up [rad].
    pub rot: f32,
    /// Index into [`Vegetation::objects`]; `None` is the renderer's placeholder tree.
    pub object: Option<u16>,
}

impl TerrainTile {
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }

    /// Grid points per side, less one.
    fn n(&self, tile_size: f64) -> usize {
        (tile_size / self.step).round().max(1.0) as usize
    }
}

/// Key figures of a terrain build.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TerrainStats {
    pub tiles: usize,
    pub vertices: usize,
    pub triangles: usize,
    /// Support points without a DGM value (fallback height used).
    pub missing: usize,
    /// How often a DGM tile was read from disk.
    pub tile_loads: usize,
}

impl TerrainStats {
    /// Rough size of the mesh data in memory [bytes].
    pub fn memory(&self) -> usize {
        self.vertices * 12 + self.triangles * 12
    }
}

/// The support points of the track centreline in UTM.
struct Centerline {
    points: Vec<DVec2>,
    /// Ellipsoidal height of the top of rail [m].
    heights: Vec<f64>,
    /// Whether the point's edge carries a formation. Points without one —
    /// track the builder laid on their own constructions — take part in the
    /// corridor and the level of detail, but not in the embankment or the
    /// gravel: nothing there is the terrain's business.
    formation: Vec<bool>,
    /// The samples as segments `(a, b)` of consecutive points of one edge.
    /// Distance queries measure against these, so the answer is the distance
    /// to the **line** and not to the nearest sample — at 25 m sampling the
    /// difference is up to 12.5 m along the track, more than the blend zone
    /// is wide.
    segments: Vec<(usize, usize)>,
    /// Accelerated neighbourhood index over the segments.
    grid: CellMap<Vec<u32>>,
    cell: f64,
}

impl Centerline {
    fn build(net: &TrackNetwork, options: &TerrainOptions) -> Self {
        let mut points = Vec::new();
        let mut heights = Vec::new();
        let mut formation = Vec::new();
        let mut segments = Vec::new();
        for edge in net.edges() {
            let first = points.len();
            let steps = (edge.length() / options.centerline_step).ceil().max(1.0) as usize;
            for i in 0..=steps {
                let s = edge.length() * i as f64 / steps as f64;
                let pose = edge.eval(s);
                let (lat, lon, h) = geo::from_ecef(pose.pos);
                let (e, n) = geo::to_utm(lat, lon, options.zone);
                points.push(DVec2::new(e, n));
                heights.push(h);
                formation.push(edge.formation);
            }
            for i in first..points.len() - 1 {
                segments.push((i, i + 1));
            }
        }

        let cell = options.blend.max(50.0);
        let mut grid: CellMap<Vec<u32>> = CellMap::default();
        for (i, &(a, b)) in segments.iter().enumerate() {
            // A segment is short against the cell, but it may still cross a
            // cell corner — insert it into every cell its box touches.
            let (min, max) = (points[a].min(points[b]), points[a].max(points[b]));
            let (x0, y0) = key(min, cell);
            let (x1, y1) = key(max, cell);
            for x in x0..=x1 {
                for y in y0..=y1 {
                    grid.entry((x, y)).or_default().push(i as u32);
                }
            }
        }
        Self {
            points,
            heights,
            formation,
            segments,
            grid,
            cell,
        }
    }

    /// Nearest point **of an edge with a formation**: `(distance, height)`.
    /// Edges without one are skipped — they stand on the builder's own ground
    /// and must not pull the terrain anywhere.
    fn nearest(&self, p: DVec2) -> Option<(f64, f64)> {
        let (kx, ky) = key(p, self.cell);
        let mut best: Option<(f64, f64)> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                let Some(bucket) = self.grid.get(&(kx + dx, ky + dy)) else {
                    continue;
                };
                for &i in bucket {
                    let (a, b) = self.segments[i as usize];
                    if !self.formation[a] {
                        continue;
                    }
                    let (d, h) = segment_distance(
                        self.points[a],
                        self.points[b],
                        self.heights[a],
                        self.heights[b],
                        p,
                    );
                    if best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, h));
                    }
                }
            }
        }
        best
    }

    /// Distance to the centreline without the neighbourhood index (checks only).
    #[cfg(test)]
    fn distance_scan(&self, p: DVec2) -> f64 {
        self.points
            .iter()
            .map(|q| (*q - p).length())
            .fold(f64::INFINITY, f64::min)
    }

    /// Distance from the centreline to the **area** of a tile (0 if the track runs
    /// through it).
    ///
    /// The distance to the tile centre is no good for this: a 512 m tile the track runs
    /// through still has its centre hundreds of metres to the side — it would
    /// otherwise get the coarse level.
    fn distance_to_rect(&self, min: DVec2, size: f64) -> f64 {
        let max = min + DVec2::splat(size);
        self.points
            .iter()
            .map(|q| {
                let clamped = DVec2::new(q.x.clamp(min.x, max.x), q.y.clamp(min.y, max.y));
                (*q - clamped).length()
            })
            .fold(f64::INFINITY, f64::min)
    }
}

pub(crate) fn key(p: DVec2, cell: f64) -> (i64, i64) {
    ((p.x / cell).floor() as i64, (p.y / cell).floor() as i64)
}

/// Distance from `p` to the segment `a–b`, with the rail height interpolated
/// at the foot of the perpendicular.
fn segment_distance(a: DVec2, b: DVec2, ha: f64, hb: f64, p: DVec2) -> (f64, f64) {
    let ab = b - a;
    let len2 = ab.length_squared();
    let t = if len2 > 0.0 {
        ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let foot = a + ab * t;
    ((foot - p).length(), ha + (hb - ha) * t)
}

/// SplitMix64 — deterministic scatter without a `rand` dependency.
pub(crate) struct Rng(pub(crate) u64);

impl Rng {
    pub(crate) fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in [0, 1).
    pub(crate) fn f64(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform in `lo..hi`.
    pub(crate) fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.f64()
    }

    /// Uniform in `0..n`; 0 for an empty range.
    pub(crate) fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.f64() * n as f64) as usize % n
        }
    }
}

/// A tree of the line, converted to the UTM grid of the terrain.
#[derive(Debug, Clone, Copy)]
struct PlacedTree {
    pos: DVec2,
    rot: f32,
    scale: f32,
    object: Option<u16>,
}

/// The line's trees, prepared for tile builds: positions in UTM plus the
/// catalog of 3D object names they reference.
#[derive(Debug, Clone, Default)]
pub struct Vegetation {
    objects: Vec<String>,
    trees: Vec<PlacedTree>,
    /// Tree indices by tile — a build looks at the trees of its tile, not at
    /// the ten thousand of the whole wood. Filled by [`Self::bucket`].
    by_tile: CellMap<Vec<u32>>,
}

impl Vegetation {
    /// The line's trees **and** the masts of its overhead lines.
    ///
    /// A mast is a geo-positioned instance of a mod object standing on the
    /// terrain, which is what a tree is; the tile pipeline has exactly one of
    /// those and there is no reason to build it a second time. So the masts
    /// come in here, and everything downstream — bucketing, streaming,
    /// instanced draws, levels of detail — treats a Donaumast as it treats a
    /// spruce (see [`crate::power::masts`]).
    pub fn from_line(line: &LineSource, zone: u8) -> Self {
        let mut sources = line.trees.clone();
        sources.extend(crate::power::masts(&line.power_lines));
        Self::from_parts(&sources, zone)
    }

    pub fn from_parts(trees: &[TreeSource], zone: u8) -> Self {
        let mut objects = Vec::new();
        let placed = trees
            .iter()
            .map(|t| {
                let (e, n) = geo::to_utm(t.lat.to_radians(), t.lon.to_radians(), zone);
                PlacedTree {
                    pos: DVec2::new(e, n),
                    // yaw is clockwise seen from above, the render rotation the
                    // other way round — same convention as the scenery objects.
                    rot: -t.yaw_deg.to_radians() as f32,
                    scale: t.scale as f32,
                    object: intern(&mut objects, &t.object),
                }
            })
            .collect();
        Self {
            objects,
            trees: placed,
            by_tile: CellMap::default(),
        }
    }

    /// The 3D object names (`"<mod>:<name>"`) that [`Tree::object`] indexes.
    pub fn objects(&self) -> &[String] {
        &self.objects
    }

    /// Sorts the trees into the tile grid.
    fn bucket(&mut self, tile_size: f64) {
        self.by_tile = bucket(self.trees.iter().map(|t| t.pos), tile_size);
    }
}

/// The scenery objects of a line, prepared for tile builds: where each one
/// stands in UTM, the pose the track gives it, and the catalog of object
/// names. An object that snaps to the terrain gets its height from the tile
/// it lands on, so a build needs nothing but what it already has.
#[derive(Debug, Clone, Default)]
pub struct Scenery {
    objects: Vec<String>,
    placed: Vec<PlacedObject>,
    by_tile: CellMap<Vec<u32>>,
}

/// One placement, resolved against the track: UTM position for the tile
/// lookup, the rail-plane base and the two directions that orient the model.
#[derive(Debug, Clone, Copy)]
struct PlacedObject {
    pos: DVec2,
    /// Base point on the rail plane, lateral offset included.
    base: EcefPos,
    /// Local vertical at the base.
    up: DVec3,
    /// Where the model's front points, yaw applied.
    dir: DVec3,
    /// Above the rail plane — or above the ground, where `snap` is set.
    height: f64,
    snap: bool,
    object: u16,
    index: u32,
}

impl Scenery {
    pub fn from_line(line: &LineSource, net: &TrackNetwork, zone: u8) -> Self {
        Self::from_parts(&line.objects, net, zone)
    }

    pub fn from_parts(placements: &[ObjectSource], net: &TrackNetwork, zone: u8) -> Self {
        let mut objects = Vec::new();
        let placed = placements
            .iter()
            .enumerate()
            .filter_map(|(index, placement)| {
                // Compile refused dangling indices; a guard keeps a stale
                // file harmless.
                let edge = net.edges().get(placement.edge as usize)?;
                let pose = edge.eval(placement.s.clamp(0.0, edge.length()));
                // Positive offset = right of increasing arc length.
                let right = pose.tangent.cross(pose.up).normalize();
                let base = EcefPos(pose.pos.0 + right * placement.lateral_offset);
                // Yaw is clockwise seen from above; 0 = front along increasing s.
                let dir =
                    DQuat::from_axis_angle(pose.up, -placement.yaw_deg.to_radians()) * pose.tangent;
                let (lat, lon, _) = geo::from_ecef(base);
                let (e, n) = geo::to_utm(lat, lon, zone);
                Some(PlacedObject {
                    pos: DVec2::new(e, n),
                    base,
                    up: pose.up,
                    dir,
                    height: placement.height,
                    snap: placement.snap_to_terrain,
                    // An object always names something; an empty name is a
                    // file defect the renderer shows as its placeholder.
                    object: intern(&mut objects, &placement.object)
                        .unwrap_or_else(|| intern(&mut objects, "").unwrap_or(0)),
                    index: index as u32,
                })
            })
            .collect();
        Self {
            objects,
            placed,
            by_tile: CellMap::default(),
        }
    }

    /// The object names (`"<mod>:<name>"`) that [`SceneryInstance::object`]
    /// indexes.
    pub fn objects(&self) -> &[String] {
        &self.objects
    }

    pub fn is_empty(&self) -> bool {
        self.placed.is_empty()
    }

    fn bucket(&mut self, tile_size: f64) {
        self.by_tile = bucket(self.placed.iter().map(|o| o.pos), tile_size);
    }
}

/// Indices of `positions` by the tile they fall in.
pub(crate) fn bucket(positions: impl Iterator<Item = DVec2>, tile_size: f64) -> CellMap<Vec<u32>> {
    let mut by_tile: CellMap<Vec<u32>> = CellMap::default();
    for (i, p) in positions.enumerate() {
        by_tile.entry(key(p, tile_size)).or_default().push(i as u32);
    }
    by_tile
}

/// One prepared brush stroke: centre in UTM, radius, and what it does.
#[derive(Debug, Clone, Copy)]
struct Stamp {
    pos: DVec2,
    radius: f64,
    edit: TerrainEdit,
}

/// The line's terrain brush strokes, prepared for tile builds (positions in
/// UTM). Strokes apply in file order — the later one paints over the earlier.
#[derive(Debug, Clone, Default)]
pub struct TerrainEdits {
    stamps: Vec<Stamp>,
}

impl TerrainEdits {
    pub fn from_line(line: &LineSource, zone: u8) -> Self {
        Self::from_parts(&line.terrain, zone)
    }

    pub fn from_parts(edits: &[TerrainEditSource], zone: u8) -> Self {
        let stamps = edits
            .iter()
            .map(|e| {
                let (east, north) = geo::to_utm(e.lat.to_radians(), e.lon.to_radians(), zone);
                Stamp {
                    pos: DVec2::new(east, north),
                    radius: e.radius.max(1.0),
                    edit: e.edit,
                }
            })
            .collect();
        Self { stamps }
    }

    pub fn is_empty(&self) -> bool {
        self.stamps.is_empty()
    }

    /// The strokes that reach into a tile — the per-tile prefilter, so a line
    /// with hundreds of strokes still costs a handful per grid point.
    fn in_rect(&self, min: DVec2, size: f64) -> Self {
        let stamps = self
            .stamps
            .iter()
            .copied()
            .filter(|s| distance_to_rect(s.pos, min, size) <= s.radius)
            .collect();
        Self { stamps }
    }

    /// Applies every stroke covering `p` to a ground height.
    fn apply(&self, p: DVec2, ground: f64) -> f64 {
        let mut height = ground;
        for stamp in &self.stamps {
            let w = falloff(stamp.pos.distance(p) / stamp.radius);
            if w <= 0.0 {
                continue;
            }
            height = match stamp.edit {
                TerrainEdit::Raise(by) => height + by * w,
                TerrainEdit::Level(to) => height * (1.0 - w) + to * w,
            };
        }
        height
    }
}

/// Weight of a stroke over its normalised radius: 1 at the centre, 0 at the
/// edge, flat on both ends (smoothstep), so strokes butt together without a
/// crease.
fn falloff(t: f64) -> f64 {
    if t >= 1.0 {
        return 0.0;
    }
    let t = 1.0 - t;
    t * t * (3.0 - 2.0 * t)
}

/// Keep this far from the track when baking a forest [m] — the blend zone of
/// the default [`TerrainOptions`] (the foot of the embankment) plus a margin,
/// so no tree stands on the embankment the terrain pulls up to rail height.
pub const TREE_TRACK_CLEARANCE: f64 = 16.0;

/// Fills a polygon (`(lat, lon)` [deg]) with trees — the editor's forest brush
/// and forest import **bake** their strokes into single [`TreeSource`]s, so
/// every tree of a wood stays individually movable and deletable. One tree per
/// `area_per_tree` m², deterministic from `seed`; `keep` filters positions
/// (the editor rejects points within [`TREE_TRACK_CLEARANCE`] of the track).
pub fn fill_polygon(
    polygon: &[(f64, f64)],
    objects: &[String],
    area_per_tree: f64,
    seed: u64,
    zone: u8,
    mut keep: impl FnMut(f64, f64) -> bool,
) -> Vec<TreeSource> {
    if polygon.len() < 3 {
        return Vec::new();
    }
    let ring: Vec<DVec2> = polygon
        .iter()
        .map(|(lat, lon)| {
            let (e, n) = geo::to_utm(lat.to_radians(), lon.to_radians(), zone);
            DVec2::new(e, n)
        })
        .collect();
    let lo = ring.iter().copied().reduce(DVec2::min).unwrap();
    let hi = ring.iter().copied().reduce(DVec2::max).unwrap();
    // Below 10 m² per tree the fill is a wall of overlapping meshes — clamp
    // rather than let a typo freeze the editor.
    let area_per_tree = area_per_tree.max(10.0);
    // Sampling the bounding box and rejecting outside the polygon yields one
    // tree per `area_per_tree` inside it.
    let attempts = ((hi.x - lo.x) * (hi.y - lo.y) / area_per_tree).ceil() as usize;
    // ponytail: per-polygon ceiling — every baked tree is a file row and part
    // of every undo snapshot; importing a whole state forest needs a compacter
    // representation, not a bigger cap.
    const BAKE_LIMIT: usize = 10_000;

    let mut rng = Rng(seed ^ 0x666F_7265_7374);
    let mut trees = Vec::new();
    for _ in 0..attempts {
        if trees.len() >= BAKE_LIMIT {
            break;
        }
        let p = lo + DVec2::new(rng.f64(), rng.f64()) * (hi - lo);
        // All random numbers are drawn before any rejection, so the stream —
        // and with it every accepted tree — stays deterministic.
        let scale = 0.7 + rng.f64() * 0.6;
        let yaw = rng.f64() * 360.0;
        let pick = rng.next();
        if !point_in_polygon(p, &ring) {
            continue;
        }
        let (lat, lon) = geo::from_utm(p.x, p.y, zone);
        let (lat, lon) = (lat.to_degrees(), lon.to_degrees());
        if !keep(lat, lon) {
            continue;
        }
        let object = objects
            .get((pick % objects.len().max(1) as u64) as usize)
            .cloned()
            .unwrap_or_default();
        trees.push(TreeSource {
            object,
            lat,
            lon,
            yaw_deg: yaw,
            scale,
        });
    }
    trees
}

/// Index of `name` in the catalog, inserting it once; empty names are the placeholder.
fn intern(objects: &mut Vec<String>, name: &str) -> Option<u16> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let index = objects.iter().position(|o| o == name).unwrap_or_else(|| {
        objects.push(name.to_string());
        objects.len() - 1
    });
    Some(index as u16)
}

/// Ray casting: is `p` inside the closed polygon?
pub fn point_in_polygon(p: DVec2, polygon: &[DVec2]) -> bool {
    let mut inside = false;
    let mut j = polygon.len() - 1;
    for i in 0..polygon.len() {
        let (a, b) = (polygon[i], polygon[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Position of a terrain tile in the UTM tile grid.
pub type TileKey = (i64, i64);

/// South-west corner of a tile [m UTM].
pub fn tile_min(k: TileKey, tile_size: f64) -> DVec2 {
    DVec2::new(k.0 as f64 * tile_size, k.1 as f64 * tile_size)
}

/// Distance from a point to the area of a tile (0 inside).
fn distance_to_rect(p: DVec2, min: DVec2, size: f64) -> f64 {
    let max = min + DVec2::splat(size);
    let clamped = DVec2::new(p.x.clamp(min.x, max.x), p.y.clamp(min.y, max.y));
    (p - clamped).length()
}

/// UTM position of a world point in the zone of the elevation data.
pub fn to_utm(pos: EcefPos, options: &TerrainOptions) -> DVec2 {
    let (lat, lon, _) = geo::from_ecef(pos);
    let (e, n) = geo::to_utm(lat, lon, options.zone);
    DVec2::new(e, n)
}

/// Tile containing the UTM point `p`.
pub fn tile_at(p: DVec2, options: &TerrainOptions) -> TileKey {
    key(p, options.tile_size)
}

/// All tile keys whose area lies within `radius` of the UTM point `p`.
///
/// Purely geometric — whether a tile carries terrain at all is only decided by
/// [`TerrainBuilder::build_key`], which needs the centreline for that.
pub fn keys_near(p: DVec2, radius: f64, options: &TerrainOptions) -> Vec<TileKey> {
    let size = options.tile_size;
    let (kx, ky) = key(p, size);
    let reach = (radius / size).ceil() as i64;
    let mut keys = Vec::new();
    for dy in -reach..=reach {
        for dx in -reach..=reach {
            let k = (kx + dx, ky + dy);
            if distance_to_rect(p, tile_min(k, size), size) <= radius {
                keys.push(k);
            }
        }
    }
    keys
}

/// Distance from a tile to a UTM point [m] — for the unload check.
pub fn tile_distance(k: TileKey, p: DVec2, options: &TerrainOptions) -> f64 {
    distance_to_rect(p, tile_min(k, options.tile_size), options.tile_size)
}

/// Terrain generator that keeps line and elevation data resident and hands out single
/// tiles (plan 4.3: load radius around camera and trains, everything else discarded).
///
/// It takes **one elevation source per UTM zone**: a line across the 12° zone boundary
/// carries a zone 32 and a zone 33 source, and every support point takes its height from
/// the first source that has one. The tile grid stays in `options.zone` — it is only a
/// partitioning and continues past the zone boundary without a seam.
///
/// Every method takes `&self`, so one builder serves as many workers as there
/// are cores: the elevation sources carry their own cache lock, everything
/// else is read. An edited line is a **new** builder ([`Self::with_line`])
/// that shares the sources — a build that is still running on the old one
/// finishes undisturbed, and nothing ever waits for a lock.
pub struct TerrainBuilder {
    centerline: Centerline,
    sources: Vec<Arc<TerrainSource>>,
    options: TerrainOptions,
    vegetation: Vegetation,
    scenery: Scenery,
    crowd: Crowd,
    fields: crate::farmland::Fields,
    waters: crate::water::Waters,
    roads: crate::roads::Roads,
    power: crate::power::PowerLines,
    edits: TerrainEdits,
}

impl TerrainBuilder {
    pub fn new(net: &TrackNetwork, sources: Vec<TerrainSource>, options: TerrainOptions) -> Self {
        Self {
            centerline: Centerline::build(net, &options),
            sources: sources.into_iter().map(Arc::new).collect(),
            options,
            vegetation: Vegetation::default(),
            scenery: Scenery::default(),
            crowd: Crowd::default(),
            fields: crate::farmland::Fields::default(),
            waters: crate::water::Waters::default(),
            roads: crate::roads::Roads::default(),
            power: crate::power::PowerLines::default(),
            edits: TerrainEdits::default(),
        }
    }

    /// Trees and forests of the line — tiles built afterwards carry them.
    pub fn with_vegetation(mut self, mut vegetation: Vegetation) -> Self {
        vegetation.bucket(self.options.tile_size);
        self.vegetation = vegetation;
        self
    }

    /// Scenery objects of the line — tiles built afterwards carry them, on
    /// their ground where they snap to it.
    pub fn with_scenery(mut self, mut scenery: Scenery) -> Self {
        scenery.bucket(self.options.tile_size);
        self.scenery = scenery;
        self
    }

    /// The people on the line's platforms — tiles built afterwards carry
    /// them, on the platform's height or on their ground.
    pub fn with_crowd(mut self, mut crowd: Crowd) -> Self {
        crowd.bucket(self.options.tile_size);
        self.crowd = crowd;
        self
    }

    /// The farmland of the line — tiles built afterwards carry it, draped on
    /// their ground.
    pub fn with_fields(mut self, fields: crate::farmland::Fields) -> Self {
        self.fields = fields;
        self
    }

    /// The bodies of water of the line — tiles built afterwards carry their
    /// surfaces. The shoreline levels are sampled here, once, against this
    /// builder's elevation data; an already prepared set is left alone, so
    /// the caller can hand the same waters from one builder generation to
    /// the next.
    pub fn with_waters(mut self, mut waters: crate::water::Waters) -> Self {
        waters.prepare(
            &self.sources,
            self.options.zone,
            self.options.geoid_offset,
            self.options.fallback_height,
        );
        self.waters = waters;
        self
    }

    /// The roads of the line — tiles built afterwards carry their
    /// carriageways, draped on their ground.
    pub fn with_roads(mut self, roads: crate::roads::Roads) -> Self {
        self.roads = roads;
        self
    }

    /// The overhead lines of the line — tiles built afterwards carry the
    /// conductors crossing them. The mast feet are fixed here, once, against
    /// this builder's elevation data: a span hangs between two masts that are
    /// rarely on one tile, so a per-tile grid cannot answer where it starts.
    /// An already prepared set is left alone, like the waters'.
    pub fn with_power_lines(mut self, mut power: crate::power::PowerLines) -> Self {
        power.prepare(
            &self.sources,
            self.options.zone,
            self.options.geoid_offset,
            self.options.fallback_height,
        );
        self.power = power;
        self
    }

    pub fn with_edits(mut self, edits: TerrainEdits) -> Self {
        self.edits = edits;
        self
    }

    /// The same elevation data under an edited line — track geometry,
    /// vegetation, scenery and brush strokes anew, the sources and their
    /// sheet cache shared with this builder. The route editor makes one
    /// after every edit; re-indexing the DGM each time would read the
    /// delivery off disk again, and replacing the builder in place would
    /// make every worker wait for a lock. The editor shows no crowd, so
    /// none is carried over.
    #[allow(clippy::too_many_arguments)] // the builder's inputs, one by one
    pub fn with_line(
        &self,
        net: &TrackNetwork,
        vegetation: Vegetation,
        scenery: Scenery,
        fields: crate::farmland::Fields,
        waters: crate::water::Waters,
        roads: crate::roads::Roads,
        power: crate::power::PowerLines,
        edits: TerrainEdits,
    ) -> Self {
        Self {
            centerline: Centerline::build(net, &self.options),
            sources: self.sources.clone(),
            options: self.options,
            vegetation: Vegetation::default(),
            scenery: Scenery::default(),
            crowd: Crowd::default(),
            fields,
            waters: crate::water::Waters::default(),
            roads: crate::roads::Roads::default(),
            power: crate::power::PowerLines::default(),
            edits,
        }
        .with_vegetation(vegetation)
        .with_scenery(scenery)
        .with_waters(waters)
        .with_roads(roads)
        .with_power_lines(power)
    }

    /// The roads the line carries — what the editor hands from one builder
    /// generation to the next when the road list has not changed.
    pub fn roads(&self) -> &crate::roads::Roads {
        &self.roads
    }

    /// The waters the line carries, with their sampled shoreline levels —
    /// what the editor hands from one builder generation to the next when
    /// the water list has not changed.
    pub fn waters(&self) -> &crate::water::Waters {
        &self.waters
    }

    /// The 3D object names of the vegetation ([`Tree::object`] indexes them).
    pub fn tree_objects(&self) -> &[String] {
        self.vegetation.objects()
    }

    /// The object names of the scenery ([`SceneryInstance::object`] indexes
    /// them).
    pub fn scenery_objects(&self) -> &[String] {
        self.scenery.objects()
    }

    /// The character names of the crowd ([`PersonInstance::character`]
    /// indexes them).
    pub fn crowd_characters(&self) -> &[String] {
        self.crowd.characters()
    }

    pub fn options(&self) -> &TerrainOptions {
        &self.options
    }

    /// Ellipsoidal height of the terrain surface at `pos` — the DGM blended
    /// towards the rail near the track, exactly as the tile meshes are built.
    /// For objects that snap to the terrain.
    pub fn surface_height(&self, pos: EcefPos) -> f64 {
        let mut sampler = Sampler::new(self.sources.iter().map(Arc::as_ref), self.options.zone);
        self.sampled_height(&mut sampler, pos)
    }

    /// The same for a whole grid of points at once.
    ///
    /// A `Sampler` keeps the last height tile it read from each source, and
    /// that cache is most of what makes a lookup cheap — one per point throws
    /// it away every time. Anything that samples a surface rather than a
    /// point (the editor's imagery drape is a thousand points a tile) wants
    /// this one.
    pub fn surface_heights(&self, points: impl IntoIterator<Item = EcefPos>) -> Vec<f64> {
        let mut sampler = Sampler::new(self.sources.iter().map(Arc::as_ref), self.options.zone);
        points
            .into_iter()
            .map(|pos| self.sampled_height(&mut sampler, pos))
            .collect()
    }

    fn sampled_height(&self, sampler: &mut Sampler, pos: EcefPos) -> f64 {
        let (lat, lon, _) = geo::from_ecef(pos);
        let (e, n) = geo::to_utm(lat, lon, self.options.zone);
        let p = DVec2::new(e, n);
        let ground = sampler
            .height(p, lat, lon)
            .map(|h| h + self.options.geoid_offset)
            .unwrap_or(self.options.fallback_height + self.options.geoid_offset);
        let ground = self.edits.apply(p, ground);
        blend_height(self.centerline.nearest(p), ground, &self.options)
    }

    /// Builds a single tile; `None` if it lies outside the line corridor.
    pub fn build_key(&self, k: TileKey, stats: &mut TerrainStats) -> Option<TerrainTile> {
        let mut sampler = Sampler::new(self.sources.iter().map(Arc::as_ref), self.options.zone);
        build_key(
            k,
            &self.centerline,
            &mut sampler,
            &self.options,
            &self.vegetation,
            &self.scenery,
            &self.crowd,
            &self.fields,
            &self.waters,
            &self.roads,
            &self.power,
            &self.edits,
            stats,
        )
    }

    /// The trees, objects and people of a tile, placed on its ground anew —
    /// for a tile whose ground has not changed under an edited line. Takes the
    /// tile's own height grid, so it costs the scatter and nothing else. The
    /// people include the standing share of the tile's walk areas; the ways
    /// themselves are not handed back — the editor, which is who asks, shows
    /// no crowd and has none to hand back.
    pub fn rescatter(
        &self,
        tile: &TerrainTile,
    ) -> (Vec<Tree>, Vec<SceneryInstance>, Vec<PersonInstance>) {
        let frame = EnuFrame::at(tile.anchor);
        let n = tile.n(self.options.tile_size);
        let k = key(
            tile.min + DVec2::splat(self.options.tile_size / 2.0),
            self.options.tile_size,
        );
        let grid = HeightGrid::new(tile.min, &tile.heights, tile.step, n);
        let mut people = scatter_people(k, &grid, &frame, &self.crowd);
        people.extend(scatter_walkways(k, &grid, &frame, &self.crowd).1);
        (
            scatter_trees(k, &grid, &frame, &self.options, &self.vegetation),
            scatter_objects(k, &grid, &frame, &self.scenery),
            people,
        )
    }

    /// Every tile of the corridor, in a stable order.
    pub fn corridor_keys(&self) -> Vec<TileKey> {
        corridor_keys(&self.centerline, &self.options)
    }

    /// How often a DGM tile was read from disk.
    pub fn load_count(&self) -> usize {
        self.sources.iter().map(|s| s.load_count()).sum()
    }
}

/// Height lookup for one build. A grid runs across a sheet in long rows, so
/// the sheet the last point fell on answers the next one nearly always — the
/// sampler keeps it per source and goes back through the source's lock only
/// when a point leaves it.
pub(crate) struct Sampler<'a> {
    sources: Vec<&'a TerrainSource>,
    hot: Vec<Option<Arc<HeightTile>>>,
    grid_zone: u8,
}

impl<'a> Sampler<'a> {
    pub(crate) fn new(sources: impl IntoIterator<Item = &'a TerrainSource>, grid_zone: u8) -> Self {
        let sources: Vec<&TerrainSource> = sources.into_iter().collect();
        Self {
            hot: vec![None; sources.len()],
            sources,
            grid_zone,
        }
    }

    /// Height at a grid-zone UTM point from the first source that has one. A
    /// source in the grid zone is asked in UTM directly; one in another zone
    /// through the geodetic detour (`lat`/`lon` are the same point, already
    /// converted).
    pub(crate) fn height(&mut self, p: DVec2, lat: f64, lon: f64) -> Option<f64> {
        for (i, source) in self.sources.iter().enumerate() {
            let (e, n) = if source.zone == self.grid_zone {
                (p.x, p.y)
            } else {
                geo::to_utm(lat, lon, source.zone)
            };
            if let Some(sheet) = &self.hot[i]
                && sheet.contains(e, n)
                && let Some(h) = sheet.height_at_utm(e, n)
            {
                return Some(h);
            }
            let Some(sheet) = source.sheet_at(e, n) else {
                continue;
            };
            let h = sheet.height_at_utm(e, n);
            self.hot[i] = Some(sheet);
            if h.is_some() {
                return h;
            }
        }
        None
    }
}

/// Every tile key of the corridor, sorted — the same line state always yields the same
/// tiles in the same sequence.
fn corridor_keys(centerline: &Centerline, options: &TerrainOptions) -> Vec<TileKey> {
    let mut set: std::collections::HashSet<TileKey> = std::collections::HashSet::new();
    let reach = (options.radius / options.tile_size).ceil() as i64;
    for p in &centerline.points {
        let (kx, ky) = key(*p, options.tile_size);
        for dx in -reach..=reach {
            for dy in -reach..=reach {
                set.insert((kx + dx, ky + dy));
            }
        }
    }
    let mut keys: Vec<TileKey> = set.into_iter().collect();
    keys.sort_unstable();
    keys
}

/// Builds the tile `k`; `None` if it does not touch the corridor.
#[allow(clippy::too_many_arguments)]
fn build_key(
    k: TileKey,
    centerline: &Centerline,
    sampler: &mut Sampler,
    options: &TerrainOptions,
    vegetation: &Vegetation,
    scenery: &Scenery,
    crowd: &Crowd,
    farmland: &crate::farmland::Fields,
    waters: &crate::water::Waters,
    roads: &crate::roads::Roads,
    power: &crate::power::PowerLines,
    edits: &TerrainEdits,
    stats: &mut TerrainStats,
) -> Option<TerrainTile> {
    let min = tile_min(k, options.tile_size);
    let distance = centerline.distance_to_rect(min, options.tile_size);
    if distance > options.radius {
        return None;
    }
    let (step, lod) = level_of_detail(distance, options);
    // Only the strokes that reach this tile — the rest never see a grid point.
    let edits = edits.in_rect(min, options.tile_size);
    let tile = build_tile(
        k, step, lod, centerline, sampler, options, vegetation, scenery, crowd, farmland, waters,
        roads, power, &edits, stats,
    );
    stats.tiles += 1;
    stats.vertices += tile.positions.len();
    stats.triangles += tile.triangles();
    Some(tile)
}

/// Builds the terrain around all tracks of the network — without vegetation
/// (tests, tools); the app streams through [`TerrainBuilder`] instead.
///
/// `sources` may be empty — then flat terrain at `fallback_height` is created, which
/// is enough for test scenes and lines without a DGM. Several sources cover a line
/// across a UTM zone boundary, one per zone.
pub fn build(
    net: &TrackNetwork,
    sources: &[TerrainSource],
    options: &TerrainOptions,
) -> (Vec<TerrainTile>, TerrainStats) {
    let centerline = Centerline::build(net, options);
    if centerline.points.is_empty() {
        return (Vec::new(), TerrainStats::default());
    }

    let mut tiles = Vec::new();
    let mut stats = TerrainStats::default();
    let mut sampler = Sampler::new(sources.iter(), options.zone);

    for k in corridor_keys(&centerline, options) {
        // Tiles that do not touch the corridor are dropped entirely.
        if let Some(tile) = build_key(
            k,
            &centerline,
            &mut sampler,
            options,
            &Vegetation::default(),
            &Scenery::default(),
            &Crowd::default(),
            &crate::farmland::Fields::default(),
            &crate::water::Waters::default(),
            &crate::roads::Roads::default(),
            &crate::power::PowerLines::default(),
            &TerrainEdits::default(),
            &mut stats,
        ) {
            tiles.push(tile);
        }
    }

    stats.tile_loads = sources.iter().map(|s| s.load_count()).sum();
    (tiles, stats)
}

/// Grid spacing and LOD level by distance to the track centreline.
fn level_of_detail(distance: f64, options: &TerrainOptions) -> (f64, u8) {
    let base = options.base_step;
    if distance <= options.corridor {
        (base, 0)
    } else if distance <= options.corridor * 4.0 {
        (base * 2.0, 1)
    } else if distance <= options.corridor * 8.0 {
        (base * 4.0, 2)
    } else {
        (base * 8.0, 3)
    }
}

/// Blends the DGM height towards the rail near the track (cutting/embankment).
fn blend_height(near: Option<(f64, f64)>, ground: f64, options: &TerrainOptions) -> f64 {
    // The formation carries the ballast bed, so it lies `rail_offset` below the
    // top of rail — otherwise the track disappears into its own ground.
    match near {
        Some((d, rail)) if d <= options.flatten => rail - options.rail_offset,
        Some((d, rail)) if d <= options.blend => {
            let t = (d - options.flatten) / (options.blend - options.flatten);
            (rail - options.rail_offset) * (1.0 - t) + ground * t
        }
        _ => ground,
    }
}

/// The *shaped* ground at an arbitrary point: raw elevation plus geoid, the
/// brush edits over it, and the cutting/embankment blend towards the track —
/// exactly what the tile grid is sampled with (see [`build_tile`]), and what
/// a bridge deck measures its abutments on.
fn shaped_ground(
    p: DVec2,
    lat: f64,
    lon: f64,
    sampler: &mut Sampler<'_>,
    edits: &TerrainEdits,
    centerline: &Centerline,
    options: &TerrainOptions,
) -> f64 {
    let ground = sampler
        .height(p, lat, lon)
        .map(|h| h + options.geoid_offset)
        .unwrap_or(options.fallback_height + options.geoid_offset);
    let ground = edits.apply(p, ground);
    blend_height(centerline.nearest(p), ground, options)
}

/// The height grid of a tile: what the mesh, the trees and the objects all
/// stand on. Row by row from the south, `(n + 1)²` values.
pub(crate) struct HeightGrid<'a> {
    min: DVec2,
    heights: &'a [f32],
    step: f64,
    n: usize,
}

impl<'a> HeightGrid<'a> {
    pub(crate) fn new(min: DVec2, heights: &'a [f32], step: f64, n: usize) -> Self {
        Self {
            min,
            heights,
            step,
            n,
        }
    }

    /// The grid's south-west corner, in grid-zone UTM.
    pub(crate) fn min(&self) -> DVec2 {
        self.min
    }

    /// The grid's own spacing [m] — the tile's own level of detail, and what
    /// anything draped on it measures its own fineness against. A drape is
    /// only as true to the ground as its mesh is fine, so this is the number
    /// to cut it on.
    pub(crate) fn step(&self) -> f64 {
        self.step
    }

    /// Height at the UTM point `p` on the **mesh** — the triangles
    /// [`build_tile`] actually draws, not the bilinear surface between them.
    ///
    /// The two are the same at a grid point and along a grid line, and differ
    /// inside a cell by that cell's twist: bilinear at the middle is the mean
    /// of four corners, the mesh is the mean of the two the diagonal joins.
    /// A decimetre on rolling ground — which is a field floating over the
    /// ground or sunk into it, so anything draped *on* the terrain wants this
    /// one and anything wanting a slope (`normal_at`) wants the smooth one.
    pub(crate) fn mesh_at(&self, p: DVec2) -> f64 {
        let local = (p - self.min) / self.step;
        let (ix, iy) = (local.x.floor(), local.y.floor());
        let (fx, fy) = (local.x - ix, local.y - iy);
        let corner = self.min + DVec2::new(ix, iy) * self.step;
        let h = |dx: f64, dy: f64| self.at(corner + DVec2::new(dx, dy) * self.step);
        // `build_tile` writes [a, b, c] and [b, d, c] per cell: the diagonal
        // runs south-east to north-west, which is `fx + fy == 1`.
        if fx + fy <= 1.0 {
            let (sw, se, nw) = (h(0.0, 0.0), h(1.0, 0.0), h(0.0, 1.0));
            sw + (se - sw) * fx + (nw - sw) * fy
        } else {
            let (ne, nw, se) = (h(1.0, 1.0), h(0.0, 1.0), h(1.0, 0.0));
            ne + (nw - ne) * (1.0 - fx) + (se - ne) * (1.0 - fy)
        }
    }

    /// Height at the UTM point `p` (bilinear).
    pub(crate) fn at(&self, p: DVec2) -> f64 {
        let row = self.n + 1;
        let gx = ((p.x - self.min.x) / self.step).clamp(0.0, self.n as f64 - 1e-9);
        let gy = ((p.y - self.min.y) / self.step).clamp(0.0, self.n as f64 - 1e-9);
        let (ix, iy) = (gx as usize, gy as usize);
        let (fx, fy) = (gx - ix as f64, gy - iy as f64);
        let h = |ix: usize, iy: usize| self.heights[iy * row + ix] as f64;
        h(ix, iy) * (1.0 - fx) * (1.0 - fy)
            + h(ix + 1, iy) * fx * (1.0 - fy)
            + h(ix, iy + 1) * (1.0 - fx) * fy
            + h(ix + 1, iy + 1) * fx * fy
    }
}

/// Builds a single tile.
#[allow(clippy::too_many_arguments)]
fn build_tile(
    k: TileKey,
    step: f64,
    lod: u8,
    centerline: &Centerline,
    sampler: &mut Sampler,
    options: &TerrainOptions,
    vegetation: &Vegetation,
    scenery: &Scenery,
    crowd: &Crowd,
    farmland: &crate::farmland::Fields,
    waters: &crate::water::Waters,
    roads: &crate::roads::Roads,
    power: &crate::power::PowerLines,
    edits: &TerrainEdits,
    stats: &mut TerrainStats,
) -> TerrainTile {
    let min = tile_min(k, options.tile_size);
    let n = (options.tile_size / step).round().max(1.0) as usize;
    let center = min + DVec2::splat(options.tile_size / 2.0);

    // Anchor at the tile centre, so that the local f32 coordinates stay small.
    let (clat, clon) = geo::from_utm(center.x, center.y, options.zone);
    let anchor = geo::to_ecef(clat, clon, 0.0);
    let frame = EnuFrame::at(anchor);

    // Grid plus skirt, so the skirt never reallocates the grid.
    let grid_len = (n + 1) * (n + 1);
    let skirt_len = 4 * n;
    let mut positions = Vec::with_capacity(grid_len + skirt_len);
    let mut heights = Vec::with_capacity(grid_len);
    let mut track_dist = Vec::with_capacity(grid_len);

    for iy in 0..=n {
        for ix in 0..=n {
            let p = min + DVec2::new(ix as f64 * step, iy as f64 * step);
            let (lat, lon) = geo::from_utm(p.x, p.y, options.zone);
            let ground = sampler
                .height(p, lat, lon)
                .map(|h| h + options.geoid_offset)
                .unwrap_or_else(|| {
                    stats.missing += 1;
                    options.fallback_height + options.geoid_offset
                });

            // Brush strokes shape the ground; the cutting/embankment blend runs
            // after them, so no stroke can lift the track out of its alignment.
            let ground = edits.apply(p, ground);

            // Cutting/embankment: the formation at the track, then blend.
            let near = centerline.nearest(p);
            let height = blend_height(near, ground, options);
            heights.push(height);
            track_dist.push(near.map_or(f64::INFINITY, |(d, _)| d));
            let world = geo::to_ecef(lat, lon, height);
            positions.push(to_render(frame.to_local(world)));
        }
    }

    let mut splat = splat_weights(&heights, &track_dist, step, n, options);
    // The grid is kept with the tile in f32: a metre of terrain does not
    // carry more, and the tile is what the trees are placed on later.
    let heights: Vec<f32> = heights.iter().map(|h| *h as f32).collect();
    let grid = HeightGrid::new(min, &heights, step, n);
    let trees = scatter_trees(k, &grid, &frame, options, vegetation);
    let objects = scatter_objects(k, &grid, &frame, scenery);
    let mut people = scatter_people(k, &grid, &frame, crowd);
    // The ways of the tile, and the people who stand about on its areas
    // rather than walk them — ordinary people of the tile from here on.
    let (walkways, standing) = scatter_walkways(k, &grid, &frame, crowd);
    people.extend(standing);
    // The farmland of the tile, cut to it and draped on the same grid the trees
    // stand on — so a field follows every hollow the ground has.
    let fields =
        crate::farmland::patches(k, &grid, &frame, options.zone, options.tile_size, farmland);
    // The water of the tile, cut to it and laid at the height the elevation
    // data gives it — against the raw DGM, not the shaped grid, so an
    // embankment across a valley holds the water back like a dam.
    let waters = if waters.is_empty() || !waters.touches(k) {
        Vec::new()
    } else {
        crate::water::patches(k, sampler, &frame, options, options.tile_size, waters)
    };
    // The carriageways of the tile, cut to it and draped on the same grid
    // the fields are — so a road follows every hollow the ground has. A
    // bridge flies over the hollow: its abutment heights are measured on the
    // *shaped* ground — the same function that sampled the grid — so the
    // deck meets the drape at its ends, and both tiles at a seam cut the
    // same chord.
    let roads = if roads.is_empty() || !roads.touches(k) {
        Vec::new()
    } else {
        let mut ground = |p: DVec2| {
            let (lat, lon) = geo::from_utm(p.x, p.y, options.zone);
            shaped_ground(p, lat, lon, sampler, edits, centerline, options)
        };
        crate::roads::patches(
            k,
            &grid,
            &frame,
            options.zone,
            options.tile_size,
            roads,
            &mut ground,
        )
    };

    // The conductors crossing the tile: cut to it on the pieces' own middles,
    // already hung between mast tops fixed once for the whole line.
    let conductors = if power.is_empty() || !power.touches(k) {
        Vec::new()
    } else {
        crate::power::patches(k, &frame, options.zone, power)
    };

    // Regular triangulation. The winding faces **up**: +x is east and +z is
    // south in render axes, so a→b→c (east, then north) is the order whose
    // normal comes out of the ground — the other way round the whole surface
    // is a backface and gets culled away (pinned by a test).
    let row = n + 1;
    let mut indices = Vec::with_capacity(n * n * 6 + skirt_len * 6);
    for iy in 0..n {
        for ix in 0..n {
            let a = (iy * row + ix) as u32;
            let b = a + 1;
            let c = a + row as u32;
            let d = c + 1;
            indices.extend_from_slice(&[a, b, c, b, d, c]);
        }
    }

    // Skirt: the border is extended downwards so that no cracks become visible at
    // LOD boundaries.
    add_skirt(&mut positions, &mut indices, &mut splat, n, options);

    let radius = (options.tile_size * 0.75) as f32;
    TerrainTile {
        anchor,
        positions,
        indices,
        splat,
        trees,
        objects,
        people,
        walkways,
        fields,
        waters,
        roads,
        conductors,
        step,
        lod,
        radius,
        min,
        heights,
    }
}

/// Slope above which rock takes over from grass (rise per run, ≈ 27°).
const ROCK_SLOPE: f64 = 0.5;
/// Slope of pure rock (≈ 40°).
const ROCK_FULL: f64 = 0.85;

/// Splat weights per grid vertex: gravel on the strip the track flattens, rock
/// where the ground is steep, grass elsewhere.
fn splat_weights(
    heights: &[f64],
    track_dist: &[f64],
    step: f64,
    n: usize,
    options: &TerrainOptions,
) -> Vec<[f32; 4]> {
    let row = n + 1;
    let mut splat = Vec::with_capacity(heights.len());
    for iy in 0..=n {
        for ix in 0..=n {
            // Central differences, clamped at the tile border.
            let (x0, x1) = (ix.saturating_sub(1), (ix + 1).min(n));
            let (y0, y1) = (iy.saturating_sub(1), (iy + 1).min(n));
            let dx = (heights[iy * row + x1] - heights[iy * row + x0]) / ((x1 - x0) as f64 * step);
            let dy = (heights[y1 * row + ix] - heights[y0 * row + ix]) / ((y1 - y0) as f64 * step);
            let slope = (dx * dx + dy * dy).sqrt();

            let rock = ((slope - ROCK_SLOPE) / (ROCK_FULL - ROCK_SLOPE)).clamp(0.0, 1.0);
            let d = track_dist[iy * row + ix];
            // The formation is engineered ground: the ballast body and its
            // shoulder carry gravel whatever the slopes of the walls beside it
            // say to the rock weight. Beyond it the gravel fades out over the
            // shoulder, and steep ground takes what is left.
            let (gravel, rock) = if d <= options.flatten {
                (1.0, 0.0)
            } else {
                let fade =
                    ((options.gravel - d) / (options.gravel - options.flatten)).clamp(0.0, 1.0);
                (fade * (1.0 - rock), rock)
            };
            let grass = 1.0 - rock - gravel;
            splat.push([grass as f32, rock as f32, gravel as f32, 1.0]);
        }
    }
    splat
}

/// Places the line's trees on the tile, their feet on the height grid. Every
/// tree stands where the file says — the forest fill already ran in the editor
/// (see [`fill_polygon`]), so there is nothing to filter here.
fn scatter_trees(
    k: TileKey,
    grid: &HeightGrid,
    frame: &EnuFrame,
    options: &TerrainOptions,
    vegetation: &Vegetation,
) -> Vec<Tree> {
    let Some(indices) = vegetation.by_tile.get(&k) else {
        return Vec::new();
    };
    indices
        .iter()
        .map(|&i| {
            let tree = &vegetation.trees[i as usize];
            let h = grid.at(tree.pos);
            let (lat, lon) = geo::from_utm(tree.pos.x, tree.pos.y, options.zone);
            Tree {
                pos: to_render(frame.to_local(geo::to_ecef(lat, lon, h))),
                scale: tree.scale,
                rot: tree.rot,
                object: tree.object,
            }
        })
        .collect()
}

/// Places the line's scenery objects on the tile: on the rail plane, or on
/// the height grid where the placement snaps to the terrain.
fn scatter_objects(
    k: TileKey,
    grid: &HeightGrid,
    frame: &EnuFrame,
    scenery: &Scenery,
) -> Vec<SceneryInstance> {
    let Some(indices) = scenery.by_tile.get(&k) else {
        return Vec::new();
    };
    indices
        .iter()
        .map(|&i| {
            let object = &scenery.placed[i as usize];
            let anchor = if object.snap {
                let ground = grid.at(object.pos);
                let (lat, lon, _) = geo::from_ecef(object.base);
                geo::to_ecef(lat, lon, ground + object.height)
            } else {
                EcefPos(object.base.0 + object.up * object.height)
            };
            SceneryInstance {
                pos: to_render(frame.to_local(anchor)),
                rotation: model_rotation(frame, object.dir, object.up).to_array(),
                object: object.object,
                index: object.index,
            }
        })
        .collect()
}

/// A model's frame in the tile's render axes: its front (−Z, Bevy's
/// convention) along `dir`, its up the local vertical `up`. Identity where the
/// two do not span a frame.
pub(crate) fn model_rotation(frame: &EnuFrame, dir: DVec3, up: DVec3) -> glam::Quat {
    let f = to_render_dir(frame.dir_to_local(dir)).normalize_or_zero();
    let u = to_render_dir(frame.dir_to_local(up)).normalize_or_zero();
    let right = f.cross(u).normalize_or_zero();
    if right.length_squared() < 0.5 {
        glam::Quat::IDENTITY
    } else {
        glam::Quat::from_mat3(&glam::Mat3::from_cols(right, right.cross(f), -f))
    }
}

/// Attaches a vertical skirt to the tile border: every border vertex once
/// more, `skirt` metres down. Straight down in the tile's frame — over a
/// tile the true vertical turns by a few thousandths of a degree, which on
/// eight metres of skirt is less than a millimetre.
fn add_skirt(
    positions: &mut Vec<[f32; 3]>,
    indices: &mut Vec<u32>,
    splat: &mut Vec<[f32; 4]>,
    n: usize,
    options: &TerrainOptions,
) {
    let row = n + 1;
    // Border points once clockwise.
    let border: Vec<usize> = (0..row) // south edge
        .chain((1..row).map(|iy| iy * row + n)) // east edge
        .chain((0..n).rev().map(|ix| n * row + ix)) // north edge
        .chain((0..n).rev().map(|iy| iy * row)) // west edge
        .collect();

    let first_skirt = positions.len() as u32;
    let drop = options.skirt as f32;
    for &index in &border {
        let mut p = positions[index];
        p[1] -= drop;
        positions.push(p);
        // The skirt continues the border vertex's ground cover downwards.
        splat.push(splat[index]);
    }

    for i in 0..border.len() {
        let j = (i + 1) % border.len();
        let top_a = border[i] as u32;
        let top_b = border[j] as u32;
        let bot_a = first_skirt + i as u32;
        let bot_b = first_skirt + j as u32;
        indices.extend_from_slice(&[top_a, bot_a, top_b, top_b, bot_a, bot_b]);
    }
}

/// ENU (x = east, y = north, z = up) → render axes.
pub(crate) fn to_render(p: DVec3) -> [f32; 3] {
    [p.x as f32, p.z as f32, -p.y as f32]
}

/// ENU direction → render axes, as a vector.
pub(crate) fn to_render_dir(d: DVec3) -> glam::Vec3 {
    glam::Vec3::new(d.x as f32, d.z as f32, -d.y as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::dgm::{HeightTile, TerrainSource};
    use track_model::{EdgeId, NodeKind, Segment, TrackEdge, TrackNetwork};

    /// Straight 1 km test line at 52° N, 10° E.
    fn test_net() -> TrackNetwork {
        let mut net = TrackNetwork::new();
        let a = net.add_node(NodeKind::Buffer);
        let b = net.add_node(NodeKind::Buffer);
        net.add_edge(TrackEdge::new(
            EdgeId(0),
            a,
            b,
            geo::to_ecef_deg(52.0, 10.0, 100.0),
            0.0,
            vec![Segment::straight(1000.0)],
        ));
        net
    }

    /// DGM with a 25 m grid over the test area: a slope rising towards the north.
    fn test_source() -> TerrainSource {
        let (e0, n0) = geo::to_utm(52.0f64.to_radians(), 10.0f64.to_radians(), 32);
        let mut text = String::new();
        for iy in -60..60 {
            for ix in -20..80 {
                let x = (e0 / 25.0).round() * 25.0 + ix as f64 * 25.0;
                let y = (n0 / 25.0).round() * 25.0 + iy as f64 * 25.0;
                let z = 100.0 + (y - n0) * 0.02;
                text.push_str(&format!("{x} {y} {z}\n"));
            }
        }
        TerrainSource::from_tile(HeightTile::parse_xyz(&text, 32).unwrap())
    }

    fn options() -> TerrainOptions {
        TerrainOptions {
            radius: 400.0,
            ..Default::default()
        }
    }

    #[test]
    fn terrain_is_generated_only_in_the_corridor() {
        let net = test_net();
        let (tiles, stats) = build(&net, &[test_source()], &options());

        assert!(stats.tiles > 0);
        assert_eq!(tiles.len(), stats.tiles);
        // All tiles lie within the radius around the line.
        let centerline = Centerline::build(&net, &options());
        for tile in &tiles {
            let (lat, lon, _) = geo::from_ecef(tile.anchor);
            let (e, n) = geo::to_utm(lat, lon, 32);
            let d = centerline.distance_scan(DVec2::new(e, n));
            assert!(
                d <= options().radius + options().tile_size,
                "tile {d:.0} m off the line"
            );
        }
    }

    #[test]
    fn lod_gets_coarser_with_distance() {
        let net = test_net();
        let (tiles, _) = build(&net, &[test_source()], &options());

        let fine = tiles.iter().filter(|t| t.lod == 0).count();
        let coarse = tiles.iter().filter(|t| t.lod > 0).count();
        assert!(fine > 0 && coarse > 0, "fine {fine}, coarse {coarse}");

        // Fine tiles have the base step, coarse ones a multiple of it.
        for tile in &tiles {
            let expected = options().base_step * 2f64.powi(tile.lod as i32);
            assert_eq!(tile.step, expected, "LOD {}", tile.lod);
        }
    }

    #[test]
    fn triangle_count_stays_manageable() {
        let net = test_net();
        let (_, stats) = build(&net, &[test_source()], &options());

        // For comparison: the same corridor at full DGM1 resolution.
        let area = 1000.0 * 2.0 * options().radius; // m²
        let full_detail = area as usize * 2;
        assert!(
            stats.triangles * 5 < full_detail,
            "{} triangles versus {} at a 1 m grid",
            stats.triangles,
            full_detail
        );
        // And still enough for visible terrain.
        assert!(stats.triangles > 10_000);
        assert!(
            stats.memory() < 40 * 1024 * 1024,
            "{} bytes",
            stats.memory()
        );

        // The real lever is the grading: with each LOD level the triangle count of an
        // equally sized tile is quartered.
        let (tiles, _) = build(&net, &[test_source()], &options());
        let per_lod = |lod: u8| tiles.iter().find(|t| t.lod == lod).map(|t| t.triangles());
        if let (Some(fine), Some(coarse)) = (per_lod(0), per_lod(1)) {
            let ratio = fine as f64 / coarse as f64;
            assert!((3.0..5.0).contains(&ratio), "ratio LOD0:LOD1 = {ratio}");
        }
    }

    #[test]
    fn terrain_follows_the_rail_height_at_the_track() {
        let net = test_net();
        let options = options();
        let centerline = Centerline::build(&net, &options);
        let (tiles, _) = build(&net, &[test_source()], &options);

        // The test data must differ, otherwise the test checks nothing.
        let p = centerline.points[10];
        let (_, rail) = centerline.nearest(p).unwrap();
        let ground = test_source().height_at_utm(p.x, p.y).unwrap() + options.geoid_offset;
        assert!(
            (ground - rail).abs() > 0.5,
            "terrain and rail height must differ: {ground} vs {rail}"
        );

        // Convert every terrain point back and compare it with the expected value:
        // rail height at the track, DGM height far away.
        let source = test_source();
        let mut checked_rail = 0;
        let mut checked_far = 0;
        for tile in &tiles {
            let frame = EnuFrame::at(tile.anchor);
            // Only check the grid points — the skirt hangs below on purpose.
            let n = (options.tile_size / tile.step).round() as usize;
            let grid_vertices = (n + 1) * (n + 1);
            for pos in &tile.positions[..grid_vertices] {
                let local = glam::DVec3::new(pos[0] as f64, -pos[2] as f64, pos[1] as f64);
                let world = frame.to_ecef(local);
                let (lat, lon, height) = geo::from_ecef(world);
                let (e, n) = geo::to_utm(lat, lon, options.zone);
                let Some((d, rail)) = centerline.nearest(DVec2::new(e, n)) else {
                    continue;
                };
                if d < options.flatten * 0.5 && height > 0.0 {
                    let formation = rail - options.rail_offset;
                    assert!(
                        (height - formation).abs() < 0.05,
                        "at the track (d = {d:.1} m): {height:.2} instead of {formation:.2}"
                    );
                    checked_rail += 1;
                } else if d > options.blend * 2.0 && d < options.blend * 3.0 {
                    let ground = source.height_at_utm(e, n).unwrap() + options.geoid_offset;
                    assert!(
                        (height - ground).abs() < 0.5,
                        "off the track (d = {d:.1} m): {height:.2} instead of {ground:.2}"
                    );
                    checked_far += 1;
                }
            }
        }
        assert!(checked_rail > 10, "too few points checked at the track");
        assert!(checked_far > 10, "too few points checked off the track");
    }

    #[test]
    fn a_batch_of_heights_is_the_points_one_by_one() {
        // The batch shares one sampler across the grid, and a sampler holds
        // the height tile it last read. It has to be a saving and nothing
        // else: the editor drapes its aerial photo on what this returns, and
        // a drape that disagrees with the terrain by so much as a rounding
        // cuts through it.
        let net = test_net();
        let builder = TerrainBuilder::new(&net, vec![test_source()], options());
        let points: Vec<EcefPos> = (0..12)
            .map(|i| geo::to_ecef_deg(52.0 + i as f64 * 1e-4, 10.0 + i as f64 * 7e-5, 0.0))
            .collect();
        let batch = builder.surface_heights(points.iter().copied());
        let one_by_one: Vec<f64> = points.iter().map(|p| builder.surface_height(*p)).collect();
        assert_eq!(batch, one_by_one);
    }

    #[test]
    fn streamed_tiles_match_the_batch_build() {
        let net = test_net();
        let options = options();
        let (batch, batch_stats) = build(&net, &[test_source()], &options);

        let builder = TerrainBuilder::new(&net, vec![test_source()], options);
        let mut stats = TerrainStats::default();
        let keys = builder.corridor_keys();
        let streamed: Vec<TerrainTile> = keys
            .into_iter()
            .filter_map(|k| builder.build_key(k, &mut stats))
            .collect();

        assert_eq!(streamed.len(), batch.len());
        assert_eq!(stats.triangles, batch_stats.triangles);
        for (a, b) in streamed.iter().zip(&batch) {
            assert_eq!(a.lod, b.lod);
            assert_eq!(a.positions, b.positions);
        }
    }

    #[test]
    fn the_load_radius_only_covers_nearby_tiles() {
        let net = test_net();
        let options = options();
        let builder = TerrainBuilder::new(&net, vec![test_source()], options);
        let edge = &net.edges()[0];
        let start = to_utm(edge.eval(0.0).pos, &options);
        let end = to_utm(edge.eval(edge.length()).pos, &options);

        let radius = 300.0;
        let near = keys_near(start, radius, &options);
        for k in &near {
            assert!(tile_distance(*k, start, &options) <= radius);
        }
        // The tile the train stands on is loaded, the far end of the 1 km line is not.
        assert!(near.contains(&tile_at(start, &options)));
        assert!(!near.contains(&tile_at(end, &options)));

        let mut stats = TerrainStats::default();
        let built = near
            .iter()
            .filter_map(|k| builder.build_key(*k, &mut stats))
            .count();
        assert!(built > 0 && built <= near.len());
        assert!(built < builder.corridor_keys().len(), "{built} tiles");
    }

    /// A brush stroke lifts the ground it covers, leaves everything outside its
    /// radius alone, and never moves the track: the strip along the rails keeps
    /// rail height even under a stroke that reaches across it.
    #[test]
    fn a_terrain_stroke_shapes_the_ground_but_not_the_track() {
        let net = test_net();
        let options = options();
        // 200 m north of the line's start, well outside the flattened strip.
        let start = net.edges()[0].eval(0.0).pos;
        let (lat, lon, rail) = geo::from_ecef(start);
        let (lat, lon) = (lat.to_degrees(), lon.to_degrees());
        let hill = geo::to_ecef_deg(lat + 200.0 / 111_320.0, lon, 0.0);
        let (hill_lat, hill_lon, _) = geo::from_ecef(hill);

        let edits = TerrainEdits::from_parts(
            &[TerrainEditSource {
                lat: hill_lat.to_degrees(),
                lon: hill_lon.to_degrees(),
                radius: 150.0,
                edit: TerrainEdit::Raise(20.0),
            }],
            options.zone,
        );
        let plain = TerrainBuilder::new(&net, vec![test_source()], options);
        let shaped = TerrainBuilder::new(&net, vec![test_source()], options).with_edits(edits);

        // At the centre the ground is 20 m higher, at the edge of the stroke
        // untouched, and 400 m away nothing has happened.
        let at = |b: &TerrainBuilder, north: f64| {
            b.surface_height(geo::to_ecef_deg(lat + north / 111_320.0, lon, 0.0))
        };
        assert!(
            (at(&shaped, 200.0) - at(&plain, 200.0) - 20.0).abs() < 0.01,
            "centre rose by {:.2} m",
            at(&shaped, 200.0) - at(&plain, 200.0)
        );
        assert!((at(&shaped, 400.0) - at(&plain, 400.0)).abs() < 1e-6);
        // On the track itself the height stays the formation's — the blend runs last.
        let on_track = shaped.surface_height(start);
        let formation = rail - options.rail_offset;
        assert!(
            (on_track - formation).abs() < 0.05,
            "track moved to {on_track:.2} instead of {formation:.2}"
        );
    }

    /// The route editor derives a builder for the edited line after every
    /// stroke — the elevation sources and their sheet cache are shared.
    #[test]
    fn with_line_takes_over_a_new_stroke_and_shares_the_sheets() {
        let net = test_net();
        let options = options();
        let (lat, lon, _) = geo::from_ecef(net.edges()[0].eval(0.0).pos);
        let (lat, lon) = (lat.to_degrees(), lon.to_degrees());
        let hill_lat = lat + 200.0 / 111_320.0;
        let at = |b: &TerrainBuilder, north: f64| {
            b.surface_height(geo::to_ecef_deg(lat + north / 111_320.0, lon, 0.0))
        };

        let builder = TerrainBuilder::new(&net, vec![test_source()], options);
        let before = at(&builder, 200.0);
        let edited = builder.with_line(
            &net,
            Vegetation::default(),
            Scenery::default(),
            crate::farmland::Fields::default(),
            crate::water::Waters::default(),
            crate::roads::Roads::default(),
            crate::power::PowerLines::default(),
            TerrainEdits::from_parts(
                &[TerrainEditSource {
                    lat: hill_lat,
                    lon,
                    radius: 150.0,
                    edit: TerrainEdit::Raise(20.0),
                }],
                options.zone,
            ),
        );
        assert!(
            (at(&edited, 200.0) - before - 20.0).abs() < 0.01,
            "stroke not taken over: {before:.2} → {:.2}",
            at(&edited, 200.0)
        );
        // The old builder is untouched — a build still running on it is not
        // pulled out from under.
        assert!((at(&builder, 200.0) - before).abs() < 1e-9);
        assert!(Arc::ptr_eq(&builder.sources[0], &edited.sources[0]));
    }

    /// A road flagged `bridge` spans the hollow a brush stroke makes; an
    /// unflagged one follows it down. The full tile build — DGM, edits, grid
    /// and the road patches on top — so the wiring in [`build_tile`] is the
    /// thing under test.
    #[test]
    fn a_bridge_road_spans_the_hollow_the_ground_makes() {
        let net = test_net();
        // 500 m along the line, 60 m south of it: the hollow's centre.
        let start = net.edges()[0].eval(0.0).pos;
        let (lat, lon, _) = geo::from_ecef(start);
        let (lat, lon) = (lat.to_degrees(), lon.to_degrees());
        let dip_lat = lat - 60.0 / 111_320.0;
        let dip_lon = lon + 500.0 / (111_320.0 * lat.to_radians().cos());

        let road = |bridge: bool| crate::route::RoadSource {
            name: String::new(),
            points: vec![
                crate::route::RoadPoint {
                    lat: dip_lat,
                    lon: dip_lon - 0.004,
                },
                crate::route::RoadPoint {
                    lat: dip_lat,
                    lon: dip_lon + 0.004,
                },
            ],
            width: 6.0,
            surface: crate::route::RoadSurface::Asphalt,
            center_line: crate::route::CenterLine::None,
            edge_lines: true,
            bridge,
            tags: Vec::new(),
        };
        let heights = |bridge: bool| {
            let builder = TerrainBuilder::new(&net, vec![test_source()], options())
                .with_edits(TerrainEdits::from_parts(
                    &[TerrainEditSource {
                        lat: dip_lat,
                        lon: dip_lon,
                        radius: 100.0,
                        edit: TerrainEdit::Raise(-14.0),
                    }],
                    32,
                ))
                .with_roads(crate::roads::Roads::from_parts(&[road(bridge)], 32, 512.0));
            let mut stats = TerrainStats::default();
            let mut lo = f64::MAX;
            let mut hi = f64::MIN;
            for k in builder.corridor_keys() {
                if let Some(tile) = builder.build_key(k, &mut stats) {
                    for patch in &tile.roads {
                        for v in &patch.positions {
                            lo = lo.min(v[1] as f64);
                            hi = hi.max(v[1] as f64);
                        }
                    }
                }
            }
            (lo, hi)
        };

        // The deck never follows the hollow down: it stays at the chord
        // between its own ends, on ground the hollow did not touch. The DGM
        // under the road sits near 99 m (the test slope) + 46 geoid, the
        // hollow cuts ~14 m of that.
        let (lo, hi) = heights(true);
        assert!(lo > 140.0, "deck dived to {lo:.1}");
        assert!(hi - lo < 5.0, "deck not a chord: {lo:.1}..{hi:.1}");
        // On the ground the carriageway follows the hollow down.
        let (lo, _) = heights(false);
        assert!(lo < 136.0, "hollow not followed: {lo:.1}");
    }

    /// An object that snaps to the terrain stands on the tile's ground, one
    /// on the rail plane keeps its height over the rail — and both come with
    /// the tile, facing along the track.
    #[test]
    fn objects_are_placed_with_their_tile() {
        let net = test_net();
        let options = options();
        let edge = &net.edges()[0];
        let placements = vec![
            ObjectSource {
                object: "example:mast".into(),
                edge: 0,
                s: 300.0,
                lateral_offset: 3.0,
                yaw_deg: 0.0,
                height: 1.0,
                snap_to_terrain: false,
            },
            ObjectSource {
                object: "example:hut".into(),
                edge: 0,
                s: 300.0,
                lateral_offset: 200.0,
                yaw_deg: 90.0,
                height: 0.5,
                snap_to_terrain: true,
            },
        ];
        let scenery = Scenery::from_parts(&placements, &net, options.zone);
        assert_eq!(scenery.objects(), ["example:mast", "example:hut"]);
        let builder = TerrainBuilder::new(&net, vec![test_source()], options).with_scenery(scenery);

        let mut stats = TerrainStats::default();
        let tiles: Vec<TerrainTile> = builder
            .corridor_keys()
            .into_iter()
            .filter_map(|k| builder.build_key(k, &mut stats))
            .collect();
        let placed: Vec<(&TerrainTile, &SceneryInstance)> = tiles
            .iter()
            .flat_map(|t| t.objects.iter().map(move |o| (t, o)))
            .collect();
        assert_eq!(placed.len(), 2, "every object stands on exactly one tile");

        let world = |tile: &TerrainTile, o: &SceneryInstance| {
            let frame = EnuFrame::at(tile.anchor);
            let p = o.pos;
            frame.to_ecef(DVec3::new(p[0] as f64, -p[2] as f64, p[1] as f64))
        };
        let (tile, mast) = placed.iter().find(|(_, o)| o.index == 0).unwrap();
        let rail = edge.eval(300.0).pos;
        let (_, _, rail_h) = geo::from_ecef(rail);
        let (_, _, mast_h) = geo::from_ecef(world(tile, mast));
        assert!(
            (mast_h - rail_h - 1.0).abs() < 0.05,
            "mast at {mast_h:.2} over rail {rail_h:.2}"
        );

        let (tile, hut) = placed.iter().find(|(_, o)| o.index == 1).unwrap();
        let foot = world(tile, hut);
        let ground = builder.surface_height(foot);
        let (_, _, hut_h) = geo::from_ecef(foot);
        assert!(
            (hut_h - ground - 0.5).abs() < 0.6,
            "hut at {hut_h:.2} over ground {ground:.2}"
        );

        // Rescattering onto the built tile gives the same placement.
        let (_, again, _) = builder.rescatter(tile);
        assert_eq!(again, tile.objects);
    }

    #[test]
    fn without_a_dgm_the_terrain_is_flat() {
        let net = test_net();
        let (tiles, stats) = build(&net, &[], &options());
        assert!(!tiles.is_empty());
        assert!(stats.missing > 0, "missing heights are counted");
        assert_eq!(stats.tile_loads, 0);
    }

    /// A line across the 12° zone boundary takes its heights from one source per UTM
    /// zone — the tile grid stays in zone 32, the zone 33 source answers through the
    /// geodetic detour.
    #[test]
    fn sources_from_both_zones_cover_a_line_across_the_boundary() {
        // 1 km straight at 52° N crossing 12° E in the middle.
        let mut net = TrackNetwork::new();
        let a = net.add_node(track_model::NodeKind::Buffer);
        let b = net.add_node(track_model::NodeKind::Buffer);
        net.add_edge(TrackEdge::new(
            EdgeId(0),
            a,
            b,
            geo::to_ecef_deg(52.0, 11.9927, 100.0),
            0.0,
            vec![Segment::straight(1000.0)],
        ));

        // One flat source per zone, each ending at the boundary: west of 12° E only the
        // zone 32 data answers, east of it only the zone 33 data.
        let grid = |zone: u8, e_from: f64, e_to: f64, height: f64| {
            let (e12, n12) = geo::to_utm(52.0f64.to_radians(), 12.0f64.to_radians(), zone);
            let mut text = String::new();
            let cols = ((e_to - e_from) / 25.0) as i64;
            for iy in -80..=80 {
                for ix in 0..=cols {
                    let x = (e12 / 25.0).round() * 25.0 + e_from + ix as f64 * 25.0;
                    let y = (n12 / 25.0).round() * 25.0 + iy as f64 * 25.0;
                    text.push_str(&format!("{x} {y} {height}\n"));
                }
            }
            TerrainSource::from_tile(HeightTile::parse_xyz(&text, zone).unwrap())
        };
        let west = || grid(32, -3000.0, 50.0, 100.0);
        let east = || grid(33, -50.0, 3000.0, 200.0);

        // With the zone 32 source alone the eastern half has no data…
        let (_, west_only) = build(&net, &[west()], &options());
        assert!(west_only.missing > 0, "east of 12° must be uncovered");
        // …with one source per zone every support point has a height.
        let (_, both) = build(&net, &[west(), east()], &options());
        assert_eq!(both.missing, 0, "both zones together cover the corridor");
    }

    #[test]
    fn splat_weights_follow_track_and_slope() {
        let net = test_net();
        let options = options();
        let centerline = Centerline::build(&net, &options);
        // A cliff: 1 m of height per metre east — everything off the flattened
        // strip is steeper than `ROCK_FULL`.
        let (e0, n0) = geo::to_utm(52.0f64.to_radians(), 10.0f64.to_radians(), 32);
        let mut text = String::new();
        for iy in -60..60 {
            for ix in -20..80 {
                let x = (e0 / 25.0).round() * 25.0 + ix as f64 * 25.0;
                let y = (n0 / 25.0).round() * 25.0 + iy as f64 * 25.0;
                text.push_str(&format!("{x} {y} {}\n", 100.0 + (x - e0)));
            }
        }
        let cliff = TerrainSource::from_tile(HeightTile::parse_xyz(&text, 32).unwrap());
        let (tiles, _) = build(&net, &[cliff], &options);

        let mut gravel_near = 0;
        let mut rock_far = 0;
        for tile in &tiles {
            let frame = EnuFrame::at(tile.anchor);
            assert_eq!(tile.splat.len(), tile.positions.len());
            let n = (options.tile_size / tile.step).round() as usize;
            for (pos, w) in tile
                .positions
                .iter()
                .zip(&tile.splat)
                .take((n + 1) * (n + 1))
            {
                let sum = w[0] + w[1] + w[2];
                assert!((sum - 1.0).abs() < 1e-3, "weights sum to {sum}");
                let local = glam::DVec3::new(pos[0] as f64, -pos[2] as f64, pos[1] as f64);
                let (lat, lon, _) = geo::from_ecef(frame.to_ecef(local));
                let (e, n) = geo::to_utm(lat, lon, options.zone);
                let Some((d, _)) = centerline.nearest(DVec2::new(e, n)) else {
                    continue;
                };
                if d < options.flatten * 0.5 {
                    assert!(w[2] > 0.9, "gravel at the track, got {w:?}");
                    gravel_near += 1;
                } else if d > options.blend * 2.0 && d < options.blend * 3.0 {
                    assert!(w[1] > 0.9, "rock on the cliff, got {w:?} at d = {d:.0}");
                    rock_far += 1;
                }
            }
        }
        assert!(
            gravel_near > 10 && rock_far > 10,
            "{gravel_near}/{rock_far} checked"
        );
    }

    /// A straight 1 km edge at the test anchor, with the formation stated.
    fn single_edge_net(formation: bool) -> TrackNetwork {
        let mut net = TrackNetwork::new();
        let a = net.add_node(NodeKind::Buffer);
        let b = net.add_node(NodeKind::Buffer);
        let mut edge = TrackEdge::new(
            EdgeId(0),
            a,
            b,
            geo::to_ecef_deg(52.0, 10.0, 100.0),
            0.0,
            vec![Segment::straight(1000.0)],
        );
        edge.formation = formation;
        net.add_edge(edge);
        net
    }

    /// The main line at the anchor, 12 m to the north a track the builder
    /// laid on their own construction (`formation = false`).
    fn net_with_yard_track() -> TrackNetwork {
        let mut net = single_edge_net(true);
        let c = net.add_node(NodeKind::Buffer);
        let d = net.add_node(NodeKind::Buffer);
        net.add_edge(
            TrackEdge::new(
                EdgeId(1),
                c,
                d,
                geo::to_ecef_deg(52.0 + 12.0 / 111_320.0, 10.0, 100.0),
                0.0,
                vec![Segment::straight(1000.0)],
            )
            .with_formation(false),
        );
        net
    }

    #[test]
    fn an_edge_without_formation_shapes_nothing() {
        let net = net_with_yard_track();
        let options = options();
        let centerline = Centerline::build(&net, &options);

        // The yard track's own points have no formation: the nearest
        // formation point is the main line, metres away — and a line of
        // nothing but formation-less edges has none at all.
        let yard = *centerline.points.last().unwrap();
        assert!(!centerline.formation[centerline.points.len() - 1]);
        let (d, _) = centerline.nearest(yard).unwrap();
        assert!(
            d > 5.0,
            "the yard track must not shape the ground: {d:.1} m"
        );

        let centerline = Centerline::build(&single_edge_net(false), &options);
        assert!(
            centerline.nearest(centerline.points[10]).is_none(),
            "no formation anywhere, no nearest point"
        );
    }

    #[test]
    fn terrain_without_formation_stays_ground() {
        // Track on the builder's own construction: the terrain keeps the DGM
        // and its grass — no embankment, no gravel, not even at the track.
        let net = single_edge_net(false);
        let options = options();
        let (tiles, _) = build(&net, &[test_source()], &options);
        let source = test_source();

        let mut checked = 0;
        for tile in &tiles {
            assert_eq!(tile.splat.len(), tile.positions.len());
            let frame = EnuFrame::at(tile.anchor);
            let n = (options.tile_size / tile.step).round() as usize;
            for (pos, w) in tile
                .positions
                .iter()
                .zip(&tile.splat)
                .take((n + 1) * (n + 1))
            {
                assert_eq!(w[2], 0.0, "gravel on a line without formation: {w:?}");
                let local = glam::DVec3::new(pos[0] as f64, -pos[2] as f64, pos[1] as f64);
                let (lat, lon, height) = geo::from_ecef(frame.to_ecef(local));
                let (e, nn) = geo::to_utm(lat, lon, options.zone);
                let Some(ground) = source.height_at_utm(e, nn) else {
                    continue;
                };
                let ground = ground + options.geoid_offset;
                assert!(
                    (height - ground).abs() < 0.5,
                    "terrain pulled to {height:.2} beside a formation-less track (ground {ground:.2})"
                );
                checked += 1;
            }
        }
        assert!(checked > 1_000, "too few points checked");
    }

    #[test]
    fn placed_trees_land_on_their_tiles() {
        let net = test_net();
        let options = options();
        // One oak 55 m north of the line, one baked wood 110–440 m north.
        let mut trees = vec![crate::route::TreeSource {
            object: "example:oak".into(),
            lat: 52.0005,
            lon: 10.002,
            yaw_deg: 90.0,
            scale: 1.2,
        }];
        trees.extend(fill_polygon(
            &[
                (52.001, 10.0),
                (52.001, 10.01),
                (52.004, 10.01),
                (52.004, 10.0),
            ],
            &["example:fir".into()],
            500.0,
            7,
            options.zone,
            |_, _| true,
        ));
        let vegetation = Vegetation::from_parts(&trees, options.zone);
        assert_eq!(vegetation.objects(), ["example:oak", "example:fir"]);

        let builder = TerrainBuilder::new(&net, vec![test_source()], options)
            .with_vegetation(vegetation.clone());
        let mut stats = TerrainStats::default();
        let tiles: Vec<TerrainTile> = builder
            .corridor_keys()
            .into_iter()
            .filter_map(|k| builder.build_key(k, &mut stats))
            .collect();

        // Every tree of the file stands on exactly one tile — the corridor
        // covers the polygon, and no tile-level filter drops user content.
        let spawned: usize = tiles.iter().map(|t| t.trees.len()).sum();
        assert_eq!(spawned, trees.len(), "all trees spawn");
        let oaks: Vec<&Tree> = tiles
            .iter()
            .flat_map(|t| &t.trees)
            .filter(|t| t.object == Some(0))
            .collect();
        assert_eq!(oaks.len(), 1);
        assert_eq!(oaks[0].scale, 1.2);

        // Rescattering onto the built tiles gives the same trees — the
        // editor does that for a tile whose ground has not changed.
        for tile in &tiles {
            let (again, _, _) = builder.rescatter(tile);
            assert_eq!(again, tile.trees);
        }

        // Without vegetation no tile carries a tree.
        let (bare, _) = build(&net, &[test_source()], &options);
        assert!(bare.iter().all(|t| t.trees.is_empty()));
    }

    #[test]
    fn fill_polygon_bakes_deterministic_editable_trees() {
        let polygon = [
            (52.001, 10.0),
            (52.001, 10.01),
            (52.004, 10.01),
            (52.004, 10.0),
        ];
        let species = ["a:fir".to_string(), "a:beech".to_string()];
        let bake = || fill_polygon(&polygon, &species, 500.0, 7, 32, |_, _| true);
        let trees = bake();
        assert_eq!(trees, bake(), "bake must be deterministic");

        // Roughly one tree per 500 m² of the ~330 m × 685 m rectangle.
        let expected = 330.0 * 685.0 / 500.0;
        assert!(
            (trees.len() as f64) > expected * 0.7 && (trees.len() as f64) < expected * 1.3,
            "{} trees for ~{expected:.0} expected",
            trees.len()
        );
        for tree in &trees {
            assert!(
                (52.001..=52.004).contains(&tree.lat) && (10.0..=10.01).contains(&tree.lon),
                "tree outside the polygon at {:.4}, {:.4}",
                tree.lat,
                tree.lon
            );
            assert!(species.contains(&tree.object));
        }
        assert!(
            trees.iter().any(|t| t.object == species[0])
                && trees.iter().any(|t| t.object == species[1]),
            "both species picked"
        );

        // The keep filter drops positions — the editor uses it for the track strip.
        let kept = fill_polygon(&polygon, &species, 500.0, 7, 32, |lat, _| lat > 52.0025);
        assert!(!kept.is_empty() && kept.len() < trees.len());
        assert!(kept.iter().all(|t| t.lat > 52.0025));

        // Degenerate input is empty, not a panic.
        assert!(fill_polygon(&polygon[..2], &species, 500.0, 7, 32, |_, _| true).is_empty());
    }

    #[test]
    fn point_in_polygon_handles_concave_rings() {
        // L-shaped polygon.
        let ring = [
            DVec2::new(0.0, 0.0),
            DVec2::new(4.0, 0.0),
            DVec2::new(4.0, 2.0),
            DVec2::new(2.0, 2.0),
            DVec2::new(2.0, 4.0),
            DVec2::new(0.0, 4.0),
        ];
        assert!(point_in_polygon(DVec2::new(1.0, 3.0), &ring));
        assert!(point_in_polygon(DVec2::new(3.0, 1.0), &ring));
        assert!(!point_in_polygon(DVec2::new(3.0, 3.0), &ring), "the notch");
        assert!(!point_in_polygon(DVec2::new(5.0, 1.0), &ring));
    }

    /// Every triangle of the surface faces the sky. A tile wound the other way
    /// round renders as a backface — the ground is then simply not there, in
    /// the run as much as in the editor.
    #[test]
    fn the_surface_faces_upwards() {
        let (tiles, _) = build(&test_net(), &[test_source()], &options());
        let tile = &tiles[0];
        let n = (options().tile_size / tile.step).round() as usize;
        // The skirt hangs off the border and faces sideways on purpose.
        let surface = n * n * 6;
        for triangle in tile.indices[..surface].chunks(3) {
            let p = |i: u32| {
                let v = tile.positions[i as usize];
                glam::Vec3::new(v[0], v[1], v[2])
            };
            let (a, b, c) = (p(triangle[0]), p(triangle[1]), p(triangle[2]));
            let normal = (b - a).cross(c - a);
            assert!(normal.y > 0.0, "triangle faces down: {normal:?}");
        }
    }

    #[test]
    fn tiles_have_skirts() {
        let net = test_net();
        let (tiles, _) = build(&net, &[test_source()], &options());
        let tile = &tiles[0];
        let n = (tile.step.recip() * 512.0).round() as usize;
        let grid_vertices = (n + 1) * (n + 1);
        assert!(
            tile.positions.len() > grid_vertices,
            "skirt points are missing: {} vs {}",
            tile.positions.len(),
            grid_vertices
        );
        // The lowest point lies below the grid.
        let min_y = tile
            .positions
            .iter()
            .map(|p| p[1])
            .fold(f32::INFINITY, f32::min);
        let grid_min = tile.positions[..grid_vertices]
            .iter()
            .map(|p| p[1])
            .fold(f32::INFINITY, f32::min);
        assert!(min_y < grid_min - 1.0, "{min_y} vs {grid_min}");
    }
}
