//! Digital terrain models from the state survey offices (plan ch. 14/15).
//!
//! Supported are the two formats in which the federal states deliver their DGM1:
//!
//! * **XYZ**: one line per grid point, `easting northing height` in UTM
//!   (EPSG:25832 or 25833).
//! * **ESRI ASCII Grid** (`.asc`): header with `ncols`/`nrows`/`xllcorner`/`yllcorner`/
//!   `cellsize`/`NODATA_value`, then the heights row by row from north to south.
//!
//! A DGM1 tile sheet (1 km², 1 m grid) has one million points. Therefore:
//! heights are stored **densely as `f32`** (4 MB per tile instead of ~48 MB as a hashmap),
//! and [`TerrainSource`] loads tiles only on demand and keeps only the most recently used
//! ones.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use world_coords::geo;

/// A height tile: dense grid in UTM coordinates.
#[derive(Debug, Clone)]
pub struct HeightTile {
    /// UTM zone of the source data.
    pub zone: u8,
    /// Grid spacing [m].
    pub cell: f64,
    /// South-west corner of the grid (easting/northing of point `(0, 0)`).
    pub origin: (f64, f64),
    pub cols: usize,
    pub rows: usize,
    /// Heights row by row from south to north; `NaN` = no value (NODATA).
    data: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DgmError {
    Empty,
    /// Grid spacing could not be determined.
    IrregularGrid,
    /// Header of an ASCII grid is incomplete.
    BadHeader,
    /// Tile would be absurdly large (protection against broken files).
    TooLarge,
}

impl std::fmt::Display for DgmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DgmError::Empty => write!(f, "DGM file contains no points"),
            DgmError::IrregularGrid => write!(f, "DGM points do not form a regular grid"),
            DgmError::BadHeader => write!(f, "ASCII grid header incomplete"),
            DgmError::TooLarge => write!(f, "grid implausibly large"),
        }
    }
}

/// Upper limit per tile (corresponds to 4096², i.e. 16 M points = 64 MB).
const MAX_CELLS: usize = 4096 * 4096;

impl HeightTile {
    /// Reads a file; the format is detected from the extension or the content.
    pub fn parse(text: &str, zone: u8) -> Result<Self, DgmError> {
        if text.trim_start().to_ascii_lowercase().starts_with("ncols") {
            Self::parse_asc(text, zone)
        } else {
            Self::parse_xyz(text, zone)
        }
    }

    /// Reads an XYZ grid.
    pub fn parse_xyz(text: &str, zone: u8) -> Result<Self, DgmError> {
        // A DGM1 sheet is a million lines; growing the vector by doubling
        // copies it twenty times over. The line count is the upper bound.
        let mut points: Vec<(f64, f64, f32)> = Vec::with_capacity(text.len() / 24);
        let (mut min_x, mut max_x) = (f64::INFINITY, f64::NEG_INFINITY);
        let (mut min_y, mut max_y) = (f64::INFINITY, f64::NEG_INFINITY);
        for line in text.lines() {
            if let Some(p) = parse_xyz_line(line) {
                min_x = min_x.min(p.0);
                max_x = max_x.max(p.0);
                min_y = min_y.min(p.1);
                max_y = max_y.max(p.1);
                points.push(p);
            }
        }
        if points.len() < 2 {
            return Err(DgmError::Empty);
        }

        // Grid spacing from the smallest distance between two different eastings.
        let mut xs: Vec<f64> = points.iter().map(|p| p.0).collect();
        xs.sort_unstable_by(f64::total_cmp);
        xs.dedup();
        let cell = xs
            .windows(2)
            .map(|w| w[1] - w[0])
            .filter(|d| *d > 1e-6)
            .fold(f64::INFINITY, f64::min);
        if !cell.is_finite() || cell <= 0.0 {
            return Err(DgmError::IrregularGrid);
        }

        let cols = ((max_x - min_x) / cell).round() as usize + 1;
        let rows = ((max_y - min_y) / cell).round() as usize + 1;
        if cols * rows > MAX_CELLS {
            return Err(DgmError::TooLarge);
        }

        let mut data = vec![f32::NAN; cols * rows];
        for (x, y, z) in points {
            let ix = ((x - min_x) / cell).round() as usize;
            let iy = ((y - min_y) / cell).round() as usize;
            if ix < cols && iy < rows {
                data[iy * cols + ix] = z;
            }
        }
        Ok(Self {
            zone,
            cell,
            origin: (min_x, min_y),
            cols,
            rows,
            data,
        })
    }

    /// Reads an ESRI ASCII grid.
    pub fn parse_asc(text: &str, zone: u8) -> Result<Self, DgmError> {
        let mut cols = 0usize;
        let mut rows = 0usize;
        let mut xll = f64::NAN;
        let mut yll = f64::NAN;
        let mut cell = f64::NAN;
        let mut nodata = -9999.0f64;
        let mut body = text;

        for line in text.lines() {
            let lower = line.trim().to_ascii_lowercase();
            let mut parts = lower.split_whitespace();
            let (Some(key), Some(value)) = (parts.next(), parts.next()) else {
                break;
            };
            let parsed = value.parse::<f64>().ok();
            match (key, parsed) {
                ("ncols", Some(v)) => cols = v as usize,
                ("nrows", Some(v)) => rows = v as usize,
                ("xllcorner" | "xllcenter", Some(v)) => xll = v,
                ("yllcorner" | "yllcenter", Some(v)) => yll = v,
                ("cellsize", Some(v)) => cell = v,
                ("nodata_value", Some(v)) => nodata = v,
                _ => break,
            }
            // The body starts after the last header line.
            let offset = line.as_ptr() as usize - text.as_ptr() as usize + line.len();
            body = &text[offset.min(text.len())..];
        }

        if cols == 0 || rows == 0 || !cell.is_finite() || !xll.is_finite() || !yll.is_finite() {
            return Err(DgmError::BadHeader);
        }
        if cols * rows > MAX_CELLS {
            return Err(DgmError::TooLarge);
        }

        let mut data = vec![f32::NAN; cols * rows];
        let mut values = body.split_whitespace();
        // ASCII grids run from north to south, our grid from south to north.
        for row in 0..rows {
            let target = rows - 1 - row;
            for col in 0..cols {
                let Some(v) = values.next().and_then(|v| v.parse::<f64>().ok()) else {
                    break;
                };
                data[target * cols + col] = if (v - nodata).abs() < 1e-6 {
                    f32::NAN
                } else {
                    v as f32
                };
            }
        }
        Ok(Self {
            zone,
            cell,
            origin: (xll, yll),
            cols,
            rows,
            data,
        })
    }

    /// Extent `(min_x, min_y, max_x, max_y)` [m].
    pub fn bounds(&self) -> (f64, f64, f64, f64) {
        (
            self.origin.0,
            self.origin.1,
            self.origin.0 + (self.cols - 1) as f64 * self.cell,
            self.origin.1 + (self.rows - 1) as f64 * self.cell,
        )
    }

    pub fn contains(&self, easting: f64, northing: f64) -> bool {
        let (x0, y0, x1, y1) = self.bounds();
        easting >= x0 && easting <= x1 && northing >= y0 && northing <= y1
    }

    fn value(&self, ix: i64, iy: i64) -> Option<f64> {
        if ix < 0 || iy < 0 || ix as usize >= self.cols || iy as usize >= self.rows {
            return None;
        }
        let v = self.data[iy as usize * self.cols + ix as usize];
        (!v.is_nan()).then_some(v as f64)
    }

    /// Height at a UTM point [m], bilinearly interpolated.
    /// If a corner is missing (NODATA/border), the nearest available grid point is used.
    pub fn height_at_utm(&self, easting: f64, northing: f64) -> Option<f64> {
        let fx = (easting - self.origin.0) / self.cell;
        let fy = (northing - self.origin.1) / self.cell;
        let (ix, iy) = (fx.floor() as i64, fy.floor() as i64);
        let (tx, ty) = (fx - ix as f64, fy - iy as f64);

        let corners = [
            self.value(ix, iy),
            self.value(ix + 1, iy),
            self.value(ix, iy + 1),
            self.value(ix + 1, iy + 1),
        ];
        if let [Some(a), Some(b), Some(c), Some(d)] = corners {
            let bottom = a * (1.0 - tx) + b * tx;
            let top = c * (1.0 - tx) + d * tx;
            return Some(bottom * (1.0 - ty) + top * ty);
        }
        corners.into_iter().flatten().next()
    }

    /// Number of occupied grid points.
    pub fn len(&self) -> usize {
        self.data.iter().filter(|v| !v.is_nan()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.data.iter().all(|v| v.is_nan())
    }

    /// Memory footprint of the tile [bytes].
    pub fn memory(&self) -> usize {
        self.data.len() * std::mem::size_of::<f32>()
    }

    /// Builds a tile by sampling `sources` over a square — the cut-out a module
    /// ships instead of the state's whole DGM1. Points without data become
    /// NODATA, so a square that only half touches the delivery still works.
    pub fn sample(
        sources: &[TerrainSource],
        zone: u8,
        origin: (f64, f64),
        size: f64,
        cell: f64,
    ) -> Self {
        let cell = cell.max(0.5);
        // One point past the end, so neighbouring cut-outs share their border
        // row and no seam appears between them.
        let n = (size / cell).round().max(1.0) as usize + 1;
        let mut data = vec![f32::NAN; n * n];
        for iy in 0..n {
            for ix in 0..n {
                let e = origin.0 + ix as f64 * cell;
                let north = origin.1 + iy as f64 * cell;
                let height = sources.iter().find_map(|s| {
                    if s.zone == zone {
                        s.height_at_utm(e, north)
                    } else {
                        // Another zone: the same point, through geodetic.
                        let (lat, lon) = geo::from_utm(e, north, zone);
                        s.height_at(lat.to_degrees(), lon.to_degrees())
                    }
                });
                if let Some(h) = height {
                    data[iy * n + ix] = h as f32;
                }
            }
        }
        Self {
            zone,
            cell,
            origin,
            cols: n,
            rows: n,
            data,
        }
    }

    /// The tile as an ESRI ASCII grid — the format [`Self::parse_asc`] reads,
    /// so a module's height data needs no reader of its own.
    pub fn to_asc(&self) -> String {
        let mut text = format!(
            "ncols {}\nnrows {}\nxllcorner {}\nyllcorner {}\ncellsize {}\nNODATA_value -9999\n",
            self.cols, self.rows, self.origin.0, self.origin.1, self.cell
        );
        // ASCII grids run from north to south, our grid the other way.
        for row in (0..self.rows).rev() {
            for col in 0..self.cols {
                let v = self.data[row * self.cols + col];
                if col > 0 {
                    text.push(' ');
                }
                if v.is_nan() {
                    text.push_str("-9999");
                } else {
                    // Centimetres are beyond what a DGM1 promises.
                    text.push_str(&format!("{v:.2}"));
                }
            }
            text.push('\n');
        }
        text
    }
}

fn parse_xyz_line(line: &str) -> Option<(f64, f64, f32)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let mut it = line.split(|c: char| c.is_whitespace() || c == ';' || c == ',');
    let x = it.find_map(|v| v.parse::<f64>().ok())?;
    let y = it.find_map(|v| v.parse::<f64>().ok())?;
    let z = it.find_map(|v| v.parse::<f32>().ok())?;
    Some((x, y, z))
}

/// A known tile that has not been loaded yet.
#[derive(Debug, Clone)]
struct TileEntry {
    path: PathBuf,
    /// `(min_x, min_y, max_x, max_y)` [m] — from the file name where it
    /// carries the sheet corner, otherwise read out of the file.
    bounds: (f64, f64, f64, f64),
}

/// Edge length of the cells the sheet index is kept in [m]. DGM sheets are
/// whole kilometres, so a sheet lands in one cell and a point finds its sheet
/// with one hash lookup instead of a scan over the delivery.
const INDEX_CELL: f64 = 1_000.0;

/// Height source from a directory full of DGM tiles.
///
/// Tiles are loaded **lazily**: on creation only an index of the sheet boundaries is
/// built (from the file name or the ASCII grid header); loading happens only when a
/// query falls into a tile. At most `cache_limit` tiles stay in memory (LRU).
///
/// Every method takes `&self`: the cache lives behind its own short lock, so
/// several terrain tiles can be built from the same source at the same time.
/// A sheet is read off disk **outside** that lock — a load takes seconds for
/// a DGM1 sheet, and the other builders keep sampling what is already there.
#[derive(Debug)]
pub struct TerrainSource {
    pub zone: u8,
    tiles: Vec<TileEntry>,
    /// Sheet indices by kilometre cell of their extent.
    cells: HashMap<(i64, i64), Vec<usize>>,
    state: Mutex<CacheState>,
    /// One lock per sheet, held while it is read: a second builder that needs
    /// the same sheet waits for the first instead of parsing it again.
    loading: Vec<Mutex<()>>,
}

#[derive(Debug)]
struct CacheState {
    /// How many tiles may stay in memory at the same time.
    limit: usize,
    /// Most recently used tiles, the newest at the front. Shared, so a build
    /// keeps sampling a sheet the cache has since let go of.
    cache: VecDeque<(usize, Arc<HeightTile>)>,
    /// Tiles that could not be loaded — do not try again.
    failed: Vec<bool>,
    loads: usize,
}

impl TerrainSource {
    /// Source from a single, already loaded tile (tests, small areas).
    pub fn from_tile(tile: HeightTile) -> Self {
        let zone = tile.zone;
        let entry = TileEntry {
            path: PathBuf::new(),
            bounds: tile.bounds(),
        };
        let mut source = Self::from_entries(vec![entry], zone);
        source
            .state
            .get_mut()
            .expect("fresh source")
            .cache
            .push_front((0, Arc::new(tile)));
        source
    }

    /// Search a directory (recursively) for `*.xyz`, `*.asc` and `*.txt`.
    ///
    /// `zone` is the UTM zone of the data (32 for the west, 33 for the east of Germany)
    /// — it is part of the EPSG code of the delivery.
    pub fn from_dir(dir: impl AsRef<Path>, zone: u8) -> std::io::Result<Self> {
        let mut tiles = Vec::new();
        collect_files(dir.as_ref(), &mut tiles)?;
        let entries = tiles
            .into_iter()
            .filter_map(|path| {
                let bounds = tile_bounds(&path)?;
                Some(TileEntry { path, bounds })
            })
            .collect();
        Ok(Self::from_entries(entries, zone))
    }

    fn from_entries(tiles: Vec<TileEntry>, zone: u8) -> Self {
        let mut cells: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        for (i, t) in tiles.iter().enumerate() {
            for cell in cells_of(t.bounds) {
                cells.entry(cell).or_default().push(i);
            }
        }
        Self {
            zone,
            loading: (0..tiles.len()).map(|_| Mutex::new(())).collect(),
            state: Mutex::new(CacheState {
                limit: 8,
                cache: VecDeque::new(),
                failed: vec![false; tiles.len()],
                loads: 0,
            }),
            tiles,
            cells,
        }
    }

    fn state(&self) -> MutexGuard<'_, CacheState> {
        // A panic while holding the lock leaves nothing half-written: the
        // cache is only ever pushed to and popped from.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// How many sheets may stay in memory at the same time.
    pub fn set_cache_limit(&self, limit: usize) {
        let mut state = self.state();
        state.limit = limit.max(1);
        while state.cache.len() > state.limit {
            state.cache.pop_back();
        }
    }

    /// Number of known tiles.
    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }

    /// How often a tile was read from disk (a metric for the cache).
    pub fn load_count(&self) -> usize {
        self.state().loads
    }

    /// Total extent of all tiles `(min_x, min_y, max_x, max_y)`.
    pub fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
        self.tiles
            .iter()
            .map(|t| t.bounds)
            .reduce(|a, b| (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)))
    }

    /// Height at a UTM point [m].
    pub fn height_at_utm(&self, easting: f64, northing: f64) -> Option<f64> {
        self.sheet_at(easting, northing)?
            .height_at_utm(easting, northing)
    }

    /// The sheet covering a UTM point, loaded if need be — for a caller that
    /// samples many points and wants to skip the lookup while they stay on
    /// the same sheet (see [`HeightTile::contains`]).
    pub fn sheet_at(&self, easting: f64, northing: f64) -> Option<Arc<HeightTile>> {
        // The cache first (the common case: consecutive queries fall into the
        // same tile).
        {
            let mut state = self.state();
            let hit = state
                .cache
                .iter()
                .position(|(_, t)| t.contains(easting, northing));
            if let Some(i) = hit {
                if i > 0 {
                    let entry = state.cache.remove(i).expect("index from position");
                    state.cache.push_front(entry);
                }
                return Some(state.cache[0].1.clone());
            }
        }

        let cell = index_cell(easting, northing);
        let candidates = self.cells.get(&cell)?;
        let index = {
            let state = self.state();
            candidates.iter().copied().find(|&i| {
                let b = self.tiles[i].bounds;
                !state.failed[i]
                    && easting >= b.0
                    && easting <= b.2
                    && northing >= b.1
                    && northing <= b.3
            })?
        };
        self.load(index)
    }

    /// Height at geodetic coordinates (degrees).
    pub fn height_at(&self, lat_deg: f64, lon_deg: f64) -> Option<f64> {
        let (e, n) = geo::to_utm(lat_deg.to_radians(), lon_deg.to_radians(), self.zone);
        self.height_at_utm(e, n)
    }

    fn load(&self, index: usize) -> Option<Arc<HeightTile>> {
        let _reading = self.loading[index]
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // Whoever held that lock before us may have loaded the very sheet.
        {
            let state = self.state();
            if let Some((_, tile)) = state.cache.iter().find(|(i, _)| *i == index) {
                return Some(tile.clone());
            }
            if state.failed[index] {
                return None;
            }
        }
        let path = &self.tiles[index].path;
        let text = std::fs::read_to_string(path).ok();
        let tile = text.and_then(|t| HeightTile::parse(&t, self.zone).ok());
        let mut state = self.state();
        let Some(tile) = tile else {
            state.failed[index] = true;
            return None;
        };
        // ponytail: an extent guessed from the file name is not corrected by
        // the real one — a state's sheet is the kilometre its name says.
        let tile = Arc::new(tile);
        state.loads += 1;
        state.cache.push_front((index, tile.clone()));
        while state.cache.len() > state.limit.max(1) {
            state.cache.pop_back();
        }
        Some(tile)
    }
}

/// Kilometre cell of a UTM point.
fn index_cell(easting: f64, northing: f64) -> (i64, i64) {
    (
        (easting / INDEX_CELL).floor() as i64,
        (northing / INDEX_CELL).floor() as i64,
    )
}

/// Every kilometre cell an extent touches.
fn cells_of((x0, y0, x1, y1): (f64, f64, f64, f64)) -> Vec<(i64, i64)> {
    let (ax, ay) = index_cell(x0, y0);
    let (bx, by) = index_cell(x1, y1);
    let mut cells = Vec::with_capacity(((bx - ax + 1) * (by - ay + 1)).max(1) as usize);
    for y in ay..=by {
        for x in ax..=bx {
            cells.push((x, y));
        }
    }
    cells
}

/// Collect all candidate files (recursively).
fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out)?;
            continue;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());
        if matches!(ext.as_deref(), Some("xyz" | "asc" | "txt")) {
            out.push(path);
        }
    }
    Ok(())
}

/// Determine the sheet boundary of a tile — first from the file name, otherwise from the
/// content.
///
/// The states name DGM1 tiles after their south-west corner in kilometres, e.g.
/// `dgm1_32_389_5711_1_nw.xyz` or `32389_5711.xyz`. That fixes the extent without opening
/// the file — with 1000 tiles this saves minutes.
fn tile_bounds(path: &Path) -> Option<(f64, f64, f64, f64)> {
    if let Some(b) = bounds_from_name(path) {
        return Some(b);
    }
    // ASCII grid: the header is in the first lines.
    let text = std::fs::read_to_string(path).ok()?;
    let tile = HeightTile::parse(&text, 32).ok()?;
    Some(tile.bounds())
}

/// Read the south-west corner from the file name (kilometre values).
fn bounds_from_name(path: &Path) -> Option<(f64, f64, f64, f64)> {
    let stem = path.file_stem()?.to_str()?;
    let numbers: Vec<&str> = stem
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .collect();

    // Northing: 4 digits, 5000…6100 km (Germany).
    let north_pos = numbers.iter().position(|n| {
        n.len() == 4
            && n.parse::<f64>()
                .is_ok_and(|v| (5000.0..=6100.0).contains(&v))
    })?;
    let north = numbers[north_pos].parse::<f64>().ok()? * 1000.0;

    // The easting comes right before it: either 3 digits (389) or with a zone prefix
    // (32389).
    let east_raw = numbers.get(north_pos.checked_sub(1)?)?;
    let east = match east_raw.len() {
        3 => east_raw.parse::<f64>().ok()?,
        5 => east_raw[2..].parse::<f64>().ok()?,
        _ => return None,
    } * 1000.0;
    if !(200_000.0..=1_000_000.0).contains(&east) {
        return None;
    }
    // DGM1 sheet boundary: 1 km × 1 km.
    Some((east, north, east + 1000.0, north + 1000.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 3×3 grid with 25 m spacing, height rises towards the east.
    fn grid_text() -> String {
        let mut s = String::from("# test grid\n");
        for iy in 0..3 {
            for ix in 0..3 {
                let x = 600_000.0 + ix as f64 * 25.0;
                let y = 5_760_000.0 + iy as f64 * 25.0;
                let z = 100.0 + ix as f64 * 2.5;
                s.push_str(&format!("{x} {y} {z}\n"));
            }
        }
        s
    }

    #[test]
    fn grid_spacing_and_extent_are_detected() {
        let tile = HeightTile::parse_xyz(&grid_text(), 32).unwrap();
        assert_eq!(tile.cell, 25.0);
        assert_eq!((tile.cols, tile.rows), (3, 3));
        assert_eq!(tile.len(), 9);
        assert_eq!(
            tile.bounds(),
            (600_000.0, 5_760_000.0, 600_050.0, 5_760_050.0)
        );
        // Dense storage: 4 bytes per grid point.
        assert_eq!(tile.memory(), 9 * 4);
    }

    #[test]
    fn bilinear_interpolation_between_grid_points() {
        let tile = HeightTile::parse_xyz(&grid_text(), 32).unwrap();
        assert_eq!(tile.height_at_utm(600_000.0, 5_760_000.0), Some(100.0));
        let mid = tile.height_at_utm(600_012.5, 5_760_012.5).unwrap();
        assert!((mid - 101.25).abs() < 1e-9, "{mid}");
        let east = tile.height_at_utm(600_050.0, 5_760_000.0).unwrap();
        assert!((east - 105.0).abs() < 1e-9, "{east}");
    }

    /// The module cut-out: sample a square out of a source, write it as an
    /// ASCII grid, read it back — the heights survive, and an area without
    /// data stays NODATA instead of turning into zeros.
    #[test]
    fn a_sampled_cut_out_survives_the_ascii_round_trip() {
        let source = TerrainSource::from_tile(HeightTile::parse_xyz(&grid_text(), 32).unwrap());
        let cut = HeightTile::sample(
            std::slice::from_ref(&source),
            32,
            (600_000.0, 5_760_000.0),
            50.0,
            25.0,
        );
        assert_eq!((cut.cols, cut.rows), (3, 3));

        let text = cut.to_asc();
        let back = HeightTile::parse_asc(&text, 32).unwrap();
        assert_eq!(back.cell, 25.0);
        assert_eq!(back.origin, (600_000.0, 5_760_000.0));
        for (x, expected) in [(600_000.0, 100.0), (600_025.0, 102.5), (600_050.0, 105.0)] {
            let h = back.height_at_utm(x, 5_760_025.0).unwrap();
            assert!(
                (h - expected).abs() < 0.01,
                "{x}: {h} instead of {expected}"
            );
        }

        // A square outside the delivery holds no data at all — the import skips
        // such tiles instead of shipping a plate of zeros.
        let empty = HeightTile::sample(
            std::slice::from_ref(&source),
            32,
            (700_000.0, 5_760_000.0),
            50.0,
            25.0,
        );
        assert!(empty.is_empty());
    }

    #[test]
    fn ascii_grid_is_read() {
        // 3×2 grid, rows from north to south, one NODATA value.
        let text = "ncols 3\nnrows 2\nxllcorner 600000.0\nyllcorner 5760000.0\n\
                    cellsize 10.0\nNODATA_value -9999\n\
                    10 11 12\n20 21 -9999\n";
        let tile = HeightTile::parse_asc(text, 32).unwrap();
        assert_eq!((tile.cols, tile.rows), (3, 2));
        // The southern row is at the bottom of the file.
        assert_eq!(tile.height_at_utm(600_000.0, 5_760_000.0), Some(20.0));
        assert_eq!(tile.height_at_utm(600_000.0, 5_760_010.0), Some(10.0));
        // NODATA falls back to the neighbour instead of inventing a height.
        let corner = tile.height_at_utm(600_020.0, 5_760_000.0).unwrap();
        assert!(corner.is_finite());
        assert_eq!(tile.len(), 5);
    }

    #[test]
    fn format_is_detected_automatically() {
        assert!(HeightTile::parse(&grid_text(), 32).is_ok());
        let asc =
            "ncols 2\nnrows 1\nxllcorner 0\nyllcorner 0\ncellsize 1\nNODATA_value -9999\n1 2\n";
        let tile = HeightTile::parse(asc, 32).unwrap();
        assert_eq!(tile.cols, 2);
    }

    #[test]
    fn sheet_boundary_from_the_file_name() {
        let cases = [
            "dgm1_32_389_5711_1_nw.xyz",
            "dgm1_32389_5711.xyz",
            "389_5711.asc",
        ];
        for name in cases {
            let b = bounds_from_name(Path::new(name)).unwrap_or_else(|| panic!("{name}"));
            assert_eq!(
                b,
                (389_000.0, 5_711_000.0, 390_000.0, 5_712_000.0),
                "{name}"
            );
        }
        assert_eq!(bounds_from_name(Path::new("arbitrary.xyz")), None);
    }

    #[test]
    fn source_from_a_single_tile() {
        let source = TerrainSource::from_tile(HeightTile::parse_xyz(&grid_text(), 32).unwrap());
        assert_eq!(source.tile_count(), 1);
        assert_eq!(source.height_at_utm(600_000.0, 5_760_000.0), Some(100.0));
        assert_eq!(source.height_at_utm(0.0, 0.0), None);
        let (lat, lon) = geo::from_utm(600_000.0, 5_760_000.0, 32);
        let h = source
            .height_at(lat.to_degrees(), lon.to_degrees())
            .unwrap();
        assert!((h - 100.0).abs() < 0.01);
    }

    #[test]
    fn empty_file_is_rejected() {
        assert_eq!(HeightTile::parse_xyz("", 32).unwrap_err(), DgmError::Empty);
        assert_eq!(
            HeightTile::parse_asc("ncols 3\n", 32).unwrap_err(),
            DgmError::BadHeader
        );
    }
}
