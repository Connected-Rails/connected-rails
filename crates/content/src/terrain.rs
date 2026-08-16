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

use crate::import::dgm::TerrainSource;
use crate::route::{LineSource, TerrainEdit, TerrainEditSource, TreeSource};
use glam::DVec2;
use std::collections::HashMap;
use track_model::TrackNetwork;
use world_coords::{EcefPos, EnuFrame, geo};

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
    /// Up to here the terrain follows the track exactly [m].
    pub flatten: f64,
    /// How far the ground beside the track lies **below** the top of rail [m].
    /// The track is drawn as a ballast bed 30 cm under the rail head, and
    /// terrain pulled to rail height would bury it — the formation is lower
    /// than the rail, on the line as much as in the model. The remaining
    /// centimetres keep the bed off the ground plane, which would otherwise
    /// z-fight with it.
    pub rail_offset: f64,
    /// Up to here rail and terrain height are blended [m].
    pub blend: f64,
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
            flatten: 10.0,
            rail_offset: 0.4,
            blend: 45.0,
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
    /// Grid spacing used [m].
    pub step: f64,
    /// LOD level (0 = finest).
    pub lod: u8,
    /// Bounding radius around the anchor [m] — for view distance and culling.
    pub radius: f32,
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
    /// Accelerated neighbourhood index.
    grid: HashMap<(i64, i64), Vec<usize>>,
    cell: f64,
}

impl Centerline {
    fn build(net: &TrackNetwork, options: &TerrainOptions) -> Self {
        let mut points = Vec::new();
        let mut heights = Vec::new();
        for edge in net.edges() {
            let steps = (edge.length() / options.centerline_step).ceil().max(1.0) as usize;
            for i in 0..=steps {
                let s = edge.length() * i as f64 / steps as f64;
                let pose = edge.eval(s);
                let (lat, lon, h) = geo::from_ecef(pose.pos);
                let (e, n) = geo::to_utm(lat, lon, options.zone);
                points.push(DVec2::new(e, n));
                heights.push(h);
            }
        }

        let cell = options.blend.max(50.0);
        let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        for (i, p) in points.iter().enumerate() {
            grid.entry(key(*p, cell)).or_default().push(i);
        }
        Self {
            points,
            heights,
            grid,
            cell,
        }
    }

    /// Nearest centreline point in the neighbourhood: `(distance, height)`.
    fn nearest(&self, p: DVec2) -> Option<(f64, f64)> {
        let (kx, ky) = key(p, self.cell);
        let mut best: Option<(f64, f64)> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                let Some(bucket) = self.grid.get(&(kx + dx, ky + dy)) else {
                    continue;
                };
                for &i in bucket {
                    let d = (self.points[i] - p).length();
                    if best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, self.heights[i]));
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

fn key(p: DVec2, cell: f64) -> (i64, i64) {
    ((p.x / cell).floor() as i64, (p.y / cell).floor() as i64)
}

/// SplitMix64 — deterministic scatter without a `rand` dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in [0, 1).
    fn f64(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
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
}

impl Vegetation {
    pub fn from_line(line: &LineSource, zone: u8) -> Self {
        Self::from_parts(&line.trees, zone)
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
        }
    }

    /// The 3D object names (`"<mod>:<name>"`) that [`Tree::object`] indexes.
    pub fn objects(&self) -> &[String] {
        &self.objects
    }
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
/// the default [`TerrainOptions`] plus a margin, so no tree stands on the
/// embankment the terrain pulls up to rail height.
pub const TREE_TRACK_CLEARANCE: f64 = 55.0;

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
fn point_in_polygon(p: DVec2, polygon: &[DVec2]) -> bool {
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
pub struct TerrainBuilder {
    centerline: Centerline,
    sources: Vec<TerrainSource>,
    options: TerrainOptions,
    vegetation: Vegetation,
    edits: TerrainEdits,
}

impl TerrainBuilder {
    pub fn new(net: &TrackNetwork, sources: Vec<TerrainSource>, options: TerrainOptions) -> Self {
        Self {
            centerline: Centerline::build(net, &options),
            sources,
            options,
            vegetation: Vegetation::default(),
            edits: TerrainEdits::default(),
        }
    }

    /// Trees and forests of the line — tiles built afterwards carry them.
    pub fn with_vegetation(mut self, vegetation: Vegetation) -> Self {
        self.vegetation = vegetation;
        self
    }

    /// Terrain brush strokes of the line — tiles built afterwards are shaped
    /// by them.
    pub fn with_edits(mut self, edits: TerrainEdits) -> Self {
        self.edits = edits;
        self
    }

    /// Takes over an edited line — track geometry, vegetation and brush
    /// strokes — while the elevation sources and their tile cache stay. The
    /// route editor rebuilds after every edit; re-indexing the DGM each time
    /// would read the delivery off disk again.
    pub fn set_line(&mut self, net: &TrackNetwork, vegetation: Vegetation, edits: TerrainEdits) {
        self.centerline = Centerline::build(net, &self.options);
        self.vegetation = vegetation;
        self.edits = edits;
    }

    /// The 3D object names of the vegetation ([`Tree::object`] indexes them).
    pub fn tree_objects(&self) -> &[String] {
        self.vegetation.objects()
    }

    pub fn options(&self) -> &TerrainOptions {
        &self.options
    }

    /// Ellipsoidal height of the terrain surface at `pos` — the DGM blended
    /// towards the rail near the track, exactly as the tile meshes are built.
    /// For objects that snap to the terrain.
    pub fn surface_height(&mut self, pos: EcefPos) -> f64 {
        let (lat, lon, _) = geo::from_ecef(pos);
        let (e, n) = geo::to_utm(lat, lon, self.options.zone);
        let p = DVec2::new(e, n);
        let ground = sample_height(&mut self.sources, p, lat, lon, self.options.zone)
            .map(|h| h + self.options.geoid_offset)
            .unwrap_or(self.options.fallback_height + self.options.geoid_offset);
        let ground = self.edits.apply(p, ground);
        blend_height(self.centerline.nearest(p), ground, &self.options)
    }

    /// Builds a single tile; `None` if it lies outside the line corridor.
    pub fn build_key(&mut self, k: TileKey, stats: &mut TerrainStats) -> Option<TerrainTile> {
        build_key(
            k,
            &self.centerline,
            &mut self.sources,
            &self.options,
            &self.vegetation,
            &self.edits,
            stats,
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
fn build_key(
    k: TileKey,
    centerline: &Centerline,
    sources: &mut [TerrainSource],
    options: &TerrainOptions,
    vegetation: &Vegetation,
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
        min, step, lod, centerline, sources, options, vegetation, &edits, stats,
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
    sources: &mut [TerrainSource],
    options: &TerrainOptions,
) -> (Vec<TerrainTile>, TerrainStats) {
    let centerline = Centerline::build(net, options);
    if centerline.points.is_empty() {
        return (Vec::new(), TerrainStats::default());
    }

    let mut tiles = Vec::new();
    let mut stats = TerrainStats::default();
    let vegetation = Vegetation::default();

    for k in corridor_keys(&centerline, options) {
        // Tiles that do not touch the corridor are dropped entirely.
        if let Some(tile) = build_key(
            k,
            &centerline,
            sources,
            options,
            &vegetation,
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

/// Height at a grid-zone UTM point from the first source that has one. A source in the
/// grid zone is asked in UTM directly; one in another zone through the geodetic detour
/// (`lat`/`lon` are the same point, already converted).
fn sample_height(
    sources: &mut [TerrainSource],
    p: DVec2,
    lat: f64,
    lon: f64,
    grid_zone: u8,
) -> Option<f64> {
    sources.iter_mut().find_map(|s| {
        if s.zone == grid_zone {
            s.height_at_utm(p.x, p.y)
        } else {
            s.height_at(lat.to_degrees(), lon.to_degrees())
        }
    })
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

/// Builds a single tile.
#[allow(clippy::too_many_arguments)]
fn build_tile(
    min: DVec2,
    step: f64,
    lod: u8,
    centerline: &Centerline,
    sources: &mut [TerrainSource],
    options: &TerrainOptions,
    vegetation: &Vegetation,
    edits: &TerrainEdits,
    stats: &mut TerrainStats,
) -> TerrainTile {
    let n = (options.tile_size / step).round().max(1.0) as usize;
    let center = min + DVec2::splat(options.tile_size / 2.0);

    // Anchor at the tile centre, so that the local f32 coordinates stay small.
    let (clat, clon) = geo::from_utm(center.x, center.y, options.zone);
    let anchor = geo::to_ecef(clat, clon, 0.0);
    let frame = EnuFrame::at(anchor);

    let mut positions = Vec::with_capacity((n + 1) * (n + 1));
    let mut heights = Vec::with_capacity((n + 1) * (n + 1));
    let mut track_dist = Vec::with_capacity((n + 1) * (n + 1));

    for iy in 0..=n {
        for ix in 0..=n {
            let p = min + DVec2::new(ix as f64 * step, iy as f64 * step);
            let (lat, lon) = geo::from_utm(p.x, p.y, options.zone);
            let ground = sample_height(sources, p, lat, lon, options.zone)
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
    let trees = scatter_trees(min, &heights, step, n, &frame, options, vegetation);

    // Regular triangulation. The winding faces **up**: +x is east and +z is
    // south in render axes, so a→b→c (east, then north) is the order whose
    // normal comes out of the ground — the other way round the whole surface
    // is a backface and gets culled away (pinned by a test).
    let row = n + 1;
    let mut indices = Vec::with_capacity(n * n * 6);
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
    add_skirt(
        &mut positions,
        &mut indices,
        &mut splat,
        &heights,
        min,
        step,
        n,
        &frame,
        options,
    );

    let radius = (options.tile_size * 0.75) as f32;
    TerrainTile {
        anchor,
        positions,
        indices,
        splat,
        trees,
        step,
        lod,
        radius,
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
            let gravel = ((options.blend - d) / (options.blend - options.flatten)).clamp(0.0, 1.0)
                * (1.0 - rock);
            let grass = 1.0 - rock - gravel;
            splat.push([grass as f32, rock as f32, gravel as f32, 1.0]);
        }
    }
    splat
}

/// Height of the tile's height grid at the UTM point `p` (bilinear).
fn ground_at(p: DVec2, min: DVec2, heights: &[f64], step: f64, n: usize) -> f64 {
    let row = n + 1;
    let gx = ((p.x - min.x) / step).clamp(0.0, n as f64 - 1e-9);
    let gy = ((p.y - min.y) / step).clamp(0.0, n as f64 - 1e-9);
    let (ix, iy) = (gx as usize, gy as usize);
    let (fx, fy) = (gx - ix as f64, gy - iy as f64);
    heights[iy * row + ix] * (1.0 - fx) * (1.0 - fy)
        + heights[iy * row + ix + 1] * fx * (1.0 - fy)
        + heights[(iy + 1) * row + ix] * (1.0 - fx) * fy
        + heights[(iy + 1) * row + ix + 1] * fx * fy
}

/// Places the line's trees on the tile, their feet on the height grid. Every
/// tree stands where the file says — the forest fill already ran in the editor
/// (see [`fill_polygon`]), so there is nothing to filter here.
fn scatter_trees(
    min: DVec2,
    heights: &[f64],
    step: f64,
    n: usize,
    frame: &EnuFrame,
    options: &TerrainOptions,
    vegetation: &Vegetation,
) -> Vec<Tree> {
    let tile_max = min + DVec2::splat(options.tile_size);
    let mut trees = Vec::new();
    for tree in &vegetation.trees {
        let inside = tree.pos.x >= min.x
            && tree.pos.x < tile_max.x
            && tree.pos.y >= min.y
            && tree.pos.y < tile_max.y;
        if !inside {
            continue;
        }
        let h = ground_at(tree.pos, min, heights, step, n);
        let (lat, lon) = geo::from_utm(tree.pos.x, tree.pos.y, options.zone);
        trees.push(Tree {
            pos: to_render(frame.to_local(geo::to_ecef(lat, lon, h))),
            scale: tree.scale,
            rot: tree.rot,
            object: tree.object,
        });
    }
    trees
}

/// Attaches a vertical skirt to the tile border.
#[allow(clippy::too_many_arguments)]
fn add_skirt(
    positions: &mut Vec<[f32; 3]>,
    indices: &mut Vec<u32>,
    splat: &mut Vec<[f32; 4]>,
    heights: &[f64],
    min: DVec2,
    step: f64,
    n: usize,
    frame: &EnuFrame,
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
    for &index in &border {
        let ix = index % row;
        let iy = index / row;
        let p = min + DVec2::new(ix as f64 * step, iy as f64 * step);
        let (lat, lon) = geo::from_utm(p.x, p.y, options.zone);
        let world = geo::to_ecef(lat, lon, heights[index] - options.skirt);
        positions.push(to_render(frame.to_local(world)));
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
fn to_render(p: glam::DVec3) -> [f32; 3] {
    [p.x as f32, p.z as f32, -p.y as f32]
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
        let (tiles, stats) = build(&net, &mut [test_source()], &options());

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
        let (tiles, _) = build(&net, &mut [test_source()], &options());

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
        let (_, stats) = build(&net, &mut [test_source()], &options());

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
        let (tiles, _) = build(&net, &mut [test_source()], &options());
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
        let (tiles, _) = build(&net, &mut [test_source()], &options);

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
        let mut source = test_source();
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
    fn streamed_tiles_match_the_batch_build() {
        let net = test_net();
        let options = options();
        let (batch, batch_stats) = build(&net, &mut [test_source()], &options);

        let mut builder = TerrainBuilder::new(&net, vec![test_source()], options);
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
        let mut builder = TerrainBuilder::new(&net, vec![test_source()], options);
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
        let mut shaped = TerrainBuilder::new(&net, vec![test_source()], options).with_edits(edits);
        let mut plain = plain;

        // At the centre the ground is 20 m higher, at the edge of the stroke
        // untouched, and 400 m away nothing has happened.
        let at = |b: &mut TerrainBuilder, north: f64| {
            b.surface_height(geo::to_ecef_deg(lat + north / 111_320.0, lon, 0.0))
        };
        assert!(
            (at(&mut shaped, 200.0) - at(&mut plain, 200.0) - 20.0).abs() < 0.01,
            "centre rose by {:.2} m",
            at(&mut shaped, 200.0) - at(&mut plain, 200.0)
        );
        assert!((at(&mut shaped, 400.0) - at(&mut plain, 400.0)).abs() < 1e-6);
        // On the track itself the height stays the formation's — the blend runs last.
        let on_track = shaped.surface_height(start);
        let formation = rail - options.rail_offset;
        assert!(
            (on_track - formation).abs() < 0.05,
            "track moved to {on_track:.2} instead of {formation:.2}"
        );
    }

    /// The route editor re-reads the edited line into the standing builder
    /// after every stroke — the elevation sources stay where they are.
    #[test]
    fn set_line_takes_over_a_new_stroke() {
        let net = test_net();
        let options = options();
        let (lat, lon, _) = geo::from_ecef(net.edges()[0].eval(0.0).pos);
        let (lat, lon) = (lat.to_degrees(), lon.to_degrees());
        let hill_lat = lat + 200.0 / 111_320.0;
        let at = |b: &mut TerrainBuilder, north: f64| {
            b.surface_height(geo::to_ecef_deg(lat + north / 111_320.0, lon, 0.0))
        };

        let mut builder = TerrainBuilder::new(&net, vec![test_source()], options);
        let before = at(&mut builder, 200.0);
        builder.set_line(
            &net,
            Vegetation::default(),
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
            (at(&mut builder, 200.0) - before - 20.0).abs() < 0.01,
            "stroke not taken over: {before:.2} → {:.2}",
            at(&mut builder, 200.0)
        );
    }

    #[test]
    fn without_a_dgm_the_terrain_is_flat() {
        let net = test_net();
        let (tiles, stats) = build(&net, &mut [], &options());
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
        let (_, west_only) = build(&net, &mut [west()], &options());
        assert!(west_only.missing > 0, "east of 12° must be uncovered");
        // …with one source per zone every support point has a height.
        let (_, both) = build(&net, &mut [west(), east()], &options());
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
        let (tiles, _) = build(&net, &mut [cliff], &options);

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

        let mut builder = TerrainBuilder::new(&net, vec![test_source()], options)
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

        // Without vegetation no tile carries a tree.
        let (bare, _) = build(&net, &mut [test_source()], &options);
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
        let (tiles, _) = build(&test_net(), &mut [test_source()], &options());
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
        let (tiles, _) = build(&net, &mut [test_source()], &options());
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
