//! Digitale Geländemodelle der Landesvermessung (Plan Kap. 14/15).
//!
//! Unterstützt werden die beiden Formate, in denen die Bundesländer ihr DGM1 ausliefern:
//!
//! * **XYZ**: eine Zeile je Rasterpunkt, `Rechtswert Hochwert Höhe` in UTM
//!   (EPSG:25832 bzw. 25833).
//! * **ESRI ASCII Grid** (`.asc`): Kopf mit `ncols`/`nrows`/`xllcorner`/`yllcorner`/
//!   `cellsize`/`NODATA_value`, danach die Höhen zeilenweise von Nord nach Süd.
//!
//! Ein DGM1-Kachelblatt (1 km², 1 m Raster) hat eine Million Punkte. Deshalb:
//! Höhen liegen **dicht als `f32`** (4 MB je Kachel statt ~48 MB als Hashmap), und
//! [`TerrainSource`] lädt Kacheln erst bei Bedarf und hält nur die zuletzt benutzten.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use world_coords::geo;

/// Eine Höhenkachel: dichtes Raster in UTM-Koordinaten.
#[derive(Debug, Clone)]
pub struct HeightTile {
    /// UTM-Zone der Quelldaten.
    pub zone: u8,
    /// Rasterweite [m].
    pub cell: f64,
    /// Südwestecke des Rasters (Rechts-/Hochwert des Punktes `(0, 0)`).
    pub origin: (f64, f64),
    pub cols: usize,
    pub rows: usize,
    /// Höhen zeilenweise von Süd nach Nord; `NaN` = kein Wert (NODATA).
    data: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DgmError {
    Empty,
    /// Rasterweite ließ sich nicht bestimmen.
    IrregularGrid,
    /// Kopf eines ASCII-Grids unvollständig.
    BadHeader,
    /// Kachel wäre unsinnig groß (Schutz vor kaputten Dateien).
    TooLarge,
}

impl std::fmt::Display for DgmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DgmError::Empty => write!(f, "DGM-Datei enthält keine Punkte"),
            DgmError::IrregularGrid => write!(f, "DGM-Punkte bilden kein regelmäßiges Raster"),
            DgmError::BadHeader => write!(f, "ASCII-Grid-Kopf unvollständig"),
            DgmError::TooLarge => write!(f, "Raster unplausibel groß"),
        }
    }
}

/// Obergrenze je Kachel (entspricht 4096², also 16 M Punkte = 64 MB).
const MAX_CELLS: usize = 4096 * 4096;

impl HeightTile {
    /// Liest eine Datei; das Format wird an der Endung bzw. am Inhalt erkannt.
    pub fn parse(text: &str, zone: u8) -> Result<Self, DgmError> {
        if text.trim_start().to_ascii_lowercase().starts_with("ncols") {
            Self::parse_asc(text, zone)
        } else {
            Self::parse_xyz(text, zone)
        }
    }

    /// Liest ein XYZ-Raster.
    pub fn parse_xyz(text: &str, zone: u8) -> Result<Self, DgmError> {
        let mut points: Vec<(f64, f64, f32)> = Vec::new();
        for line in text.lines() {
            if let Some(p) = parse_xyz_line(line) {
                points.push(p);
            }
        }
        if points.len() < 2 {
            return Err(DgmError::Empty);
        }

        // Rasterweite aus dem kleinsten Abstand zweier verschiedener Rechtswerte.
        let mut xs: Vec<f64> = points.iter().map(|p| p.0).collect();
        xs.sort_by(f64::total_cmp);
        xs.dedup();
        let cell = xs
            .windows(2)
            .map(|w| w[1] - w[0])
            .filter(|d| *d > 1e-6)
            .fold(f64::INFINITY, f64::min);
        if !cell.is_finite() || cell <= 0.0 {
            return Err(DgmError::IrregularGrid);
        }

        let min_x = points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
        let max_x = points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
        let min_y = points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
        let max_y = points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
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

    /// Liest ein ESRI ASCII Grid.
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
            // Der Rumpf beginnt hinter der letzten Kopfzeile.
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
        // ASCII-Grids laufen von Nord nach Süd, unser Raster von Süd nach Nord.
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

    /// Ausdehnung `(min_x, min_y, max_x, max_y)` [m].
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

    /// Höhe an einem UTM-Punkt [m], bilinear interpoliert.
    /// Fehlt eine Ecke (NODATA/Rand), wird der nächste vorhandene Rasterpunkt genommen.
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

    /// Anzahl belegter Rasterpunkte.
    pub fn len(&self) -> usize {
        self.data.iter().filter(|v| !v.is_nan()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.data.iter().all(|v| v.is_nan())
    }

    /// Speicherbedarf der Kachel [Byte].
    pub fn memory(&self) -> usize {
        self.data.len() * std::mem::size_of::<f32>()
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

/// Eine bekannte, noch nicht geladene Kachel.
#[derive(Debug, Clone)]
struct TileEntry {
    path: PathBuf,
    /// `(min_x, min_y, max_x, max_y)` [m].
    bounds: (f64, f64, f64, f64),
    /// Ausdehnung nur aus dem Dateinamen geraten (nicht aus dem Inhalt gelesen).
    guessed: bool,
}

/// Höhenquelle aus einem Verzeichnis voller DGM-Kacheln.
///
/// Kacheln werden **verzögert** geladen: beim Anlegen wird nur ein Verzeichnis der
/// Blattschnitte aufgebaut (aus dem Dateinamen oder dem ASCII-Grid-Kopf), geladen wird
/// erst, wenn eine Abfrage in eine Kachel fällt. Es bleiben höchstens
/// [`TerrainSource::cache_limit`] Kacheln im Speicher (LRU).
#[derive(Debug)]
pub struct TerrainSource {
    pub zone: u8,
    /// Wie viele Kacheln gleichzeitig im Speicher bleiben dürfen.
    pub cache_limit: usize,
    tiles: Vec<TileEntry>,
    /// Zuletzt benutzte Kacheln, vorne die jüngste.
    cache: VecDeque<(usize, HeightTile)>,
    /// Kacheln, die sich nicht laden ließen — nicht erneut versuchen.
    failed: Vec<usize>,
    loads: usize,
}

impl TerrainSource {
    /// Quelle aus einer einzelnen, bereits geladenen Kachel (Tests, kleine Gebiete).
    pub fn from_tile(tile: HeightTile) -> Self {
        let entry = TileEntry {
            path: PathBuf::new(),
            bounds: tile.bounds(),
            guessed: false,
        };
        let mut cache = VecDeque::new();
        cache.push_front((0, tile));
        Self {
            zone: cache[0].1.zone,
            cache_limit: 8,
            tiles: vec![entry],
            cache,
            failed: Vec::new(),
            loads: 0,
        }
    }

    /// Verzeichnis (rekursiv) nach `*.xyz`, `*.asc` und `*.txt` durchsuchen.
    ///
    /// `zone` ist die UTM-Zone der Daten (32 für den Westen, 33 für den Osten
    /// Deutschlands) — sie steht im EPSG-Code der Lieferung.
    pub fn from_dir(dir: impl AsRef<Path>, zone: u8) -> std::io::Result<Self> {
        let mut tiles = Vec::new();
        collect_files(dir.as_ref(), &mut tiles)?;
        let entries = tiles
            .into_iter()
            .filter_map(|path| {
                let (bounds, guessed) = tile_bounds(&path)?;
                Some(TileEntry {
                    path,
                    bounds,
                    guessed,
                })
            })
            .collect();
        Ok(Self {
            zone,
            cache_limit: 8,
            tiles: entries,
            cache: VecDeque::new(),
            failed: Vec::new(),
            loads: 0,
        })
    }

    /// Anzahl bekannter Kacheln.
    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }

    /// Wie oft eine Kachel von der Platte gelesen wurde (Kennzahl für den Cache).
    pub fn load_count(&self) -> usize {
        self.loads
    }

    /// Gesamte Ausdehnung aller Kacheln `(min_x, min_y, max_x, max_y)`.
    pub fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
        self.tiles
            .iter()
            .map(|t| t.bounds)
            .reduce(|a, b| (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)))
    }

    /// Höhe an einem UTM-Punkt [m].
    pub fn height_at_utm(&mut self, easting: f64, northing: f64) -> Option<f64> {
        // Erst im Cache suchen (der häufige Fall: aufeinanderfolgende Abfragen
        // liegen in derselben Kachel).
        for i in 0..self.cache.len() {
            if self.cache[i].1.contains(easting, northing) {
                if i > 0 {
                    let entry = self.cache.remove(i).unwrap();
                    self.cache.push_front(entry);
                }
                return self.cache[0].1.height_at_utm(easting, northing);
            }
        }

        let index = self
            .tiles
            .iter()
            .enumerate()
            .find(|(i, t)| {
                !self.failed.contains(i)
                    && easting >= t.bounds.0
                    && easting <= t.bounds.2
                    && northing >= t.bounds.1
                    && northing <= t.bounds.3
            })
            .map(|(i, _)| i)?;
        let tile = self.load(index)?;
        tile.height_at_utm(easting, northing)
    }

    /// Höhe an geodätischen Koordinaten (Grad).
    pub fn height_at(&mut self, lat_deg: f64, lon_deg: f64) -> Option<f64> {
        let (e, n) = geo::to_utm(lat_deg.to_radians(), lon_deg.to_radians(), self.zone);
        self.height_at_utm(e, n)
    }

    fn load(&mut self, index: usize) -> Option<&HeightTile> {
        let path = self.tiles[index].path.clone();
        let text = std::fs::read_to_string(&path).ok();
        let tile = text.and_then(|t| HeightTile::parse(&t, self.zone).ok());
        let Some(tile) = tile else {
            self.failed.push(index);
            return None;
        };
        self.loads += 1;
        // Die echte Ausdehnung ersetzt die geratene.
        self.tiles[index].bounds = tile.bounds();
        self.tiles[index].guessed = false;
        self.cache.push_front((index, tile));
        while self.cache.len() > self.cache_limit.max(1) {
            self.cache.pop_back();
        }
        Some(&self.cache[0].1)
    }
}

/// Alle in Frage kommenden Dateien einsammeln (rekursiv).
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

/// Blattschnitt einer Kachel bestimmen — erst über den Dateinamen, sonst über den Inhalt.
///
/// Die Länder benennen DGM1-Kacheln nach ihrer Südwestecke in Kilometern, z. B.
/// `dgm1_32_389_5711_1_nw.xyz` oder `32389_5711.xyz`. Damit steht die Ausdehnung fest,
/// ohne die Datei zu öffnen — bei 1000 Kacheln spart das Minuten.
fn tile_bounds(path: &Path) -> Option<((f64, f64, f64, f64), bool)> {
    if let Some(b) = bounds_from_name(path) {
        return Some((b, true));
    }
    // ASCII-Grid: der Kopf steht in den ersten Zeilen.
    let text = std::fs::read_to_string(path).ok()?;
    let tile = HeightTile::parse(&text, 32).ok()?;
    Some((tile.bounds(), false))
}

/// Südwestecke aus dem Dateinamen lesen (Kilometerwerte).
fn bounds_from_name(path: &Path) -> Option<(f64, f64, f64, f64)> {
    let stem = path.file_stem()?.to_str()?;
    let numbers: Vec<&str> = stem
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .collect();

    // Nordwert: 4-stellig, 5000…6100 km (Deutschland).
    let north_pos = numbers.iter().position(|n| {
        n.len() == 4
            && n.parse::<f64>()
                .is_ok_and(|v| (5000.0..=6100.0).contains(&v))
    })?;
    let north = numbers[north_pos].parse::<f64>().ok()? * 1000.0;

    // Ostwert steht direkt davor: entweder 3-stellig (389) oder mit Zonenpräfix (32389).
    let east_raw = numbers.get(north_pos.checked_sub(1)?)?;
    let east = match east_raw.len() {
        3 => east_raw.parse::<f64>().ok()?,
        5 => east_raw[2..].parse::<f64>().ok()?,
        _ => return None,
    } * 1000.0;
    if !(200_000.0..=1_000_000.0).contains(&east) {
        return None;
    }
    // DGM1-Blattschnitt: 1 km × 1 km.
    Some((east, north, east + 1000.0, north + 1000.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 3×3-Raster mit 25 m Weite, Höhe steigt nach Osten.
    fn grid_text() -> String {
        let mut s = String::from("# Testraster\n");
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
    fn rasterweite_und_ausdehnung_werden_erkannt() {
        let tile = HeightTile::parse_xyz(&grid_text(), 32).unwrap();
        assert_eq!(tile.cell, 25.0);
        assert_eq!((tile.cols, tile.rows), (3, 3));
        assert_eq!(tile.len(), 9);
        assert_eq!(
            tile.bounds(),
            (600_000.0, 5_760_000.0, 600_050.0, 5_760_050.0)
        );
        // Dichte Speicherung: 4 Byte je Rasterpunkt.
        assert_eq!(tile.memory(), 9 * 4);
    }

    #[test]
    fn bilineare_interpolation_zwischen_rasterpunkten() {
        let tile = HeightTile::parse_xyz(&grid_text(), 32).unwrap();
        assert_eq!(tile.height_at_utm(600_000.0, 5_760_000.0), Some(100.0));
        let mid = tile.height_at_utm(600_012.5, 5_760_012.5).unwrap();
        assert!((mid - 101.25).abs() < 1e-9, "{mid}");
        let east = tile.height_at_utm(600_050.0, 5_760_000.0).unwrap();
        assert!((east - 105.0).abs() < 1e-9, "{east}");
    }

    #[test]
    fn ascii_grid_wird_gelesen() {
        // 3×2-Grid, Zeilen von Nord nach Süd, ein NODATA-Wert.
        let text = "ncols 3\nnrows 2\nxllcorner 600000.0\nyllcorner 5760000.0\n\
                    cellsize 10.0\nNODATA_value -9999\n\
                    10 11 12\n20 21 -9999\n";
        let tile = HeightTile::parse_asc(text, 32).unwrap();
        assert_eq!((tile.cols, tile.rows), (3, 2));
        // Südliche Zeile steht in der Datei unten.
        assert_eq!(tile.height_at_utm(600_000.0, 5_760_000.0), Some(20.0));
        assert_eq!(tile.height_at_utm(600_000.0, 5_760_010.0), Some(10.0));
        // NODATA fällt auf den Nachbarn zurück, statt eine Höhe zu erfinden.
        let corner = tile.height_at_utm(600_020.0, 5_760_000.0).unwrap();
        assert!(corner.is_finite());
        assert_eq!(tile.len(), 5);
    }

    #[test]
    fn format_wird_automatisch_erkannt() {
        assert!(HeightTile::parse(&grid_text(), 32).is_ok());
        let asc =
            "ncols 2\nnrows 1\nxllcorner 0\nyllcorner 0\ncellsize 1\nNODATA_value -9999\n1 2\n";
        let tile = HeightTile::parse(asc, 32).unwrap();
        assert_eq!(tile.cols, 2);
    }

    #[test]
    fn blattschnitt_aus_dem_dateinamen() {
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
        assert_eq!(bounds_from_name(Path::new("beliebig.xyz")), None);
    }

    #[test]
    fn quelle_aus_einer_kachel() {
        let mut source = TerrainSource::from_tile(HeightTile::parse_xyz(&grid_text(), 32).unwrap());
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
    fn leere_datei_wird_abgelehnt() {
        assert_eq!(HeightTile::parse_xyz("", 32).unwrap_err(), DgmError::Empty);
        assert_eq!(
            HeightTile::parse_asc("ncols 3\n", 32).unwrap_err(),
            DgmError::BadHeader
        );
    }
}
