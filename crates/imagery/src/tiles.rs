//! Kachelrechnung für Web-Mercator-Kachelsätze (Slippy Map / WMTS).
//!
//! Alle üblichen Luftbilddienste liefern Kacheln in EPSG:3857 nach dem Schema
//! `z/x/y`: Zoomstufe 0 ist eine Kachel für die ganze Welt, jede weitere Stufe
//! vervierfacht die Anzahl. Y zählt von Nord nach Süd (bei TMS umgekehrt).

use serde::{Deserialize, Serialize};

/// Erdumfang am Äquator [m] — Bezugsgröße der Web-Mercator-Projektion.
pub const EARTH_CIRCUMFERENCE: f64 = 40_075_016.685_578_49;
/// Nördlichster/südlichster in Web-Mercator darstellbarer Breitengrad.
pub const MAX_LATITUDE: f64 = 85.051_128_779_806_59;

/// Eine Kachel im Web-Mercator-Raster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TileId {
    pub z: u8,
    pub x: u32,
    pub y: u32,
}

impl TileId {
    pub fn new(z: u8, x: u32, y: u32) -> Self {
        Self { z, x, y }
    }

    /// Anzahl Kacheln je Achse auf dieser Zoomstufe.
    pub fn count(z: u8) -> u32 {
        1u32 << z.min(30)
    }

    /// Y-Index in TMS-Zählung (von Süd nach Nord).
    pub fn tms_y(&self) -> u32 {
        Self::count(self.z).saturating_sub(1).saturating_sub(self.y)
    }

    /// Geografische Ausdehnung `(west, süd, ost, nord)` in Grad.
    pub fn bounds(&self) -> (f64, f64, f64, f64) {
        let n = Self::count(self.z) as f64;
        let west = self.x as f64 / n * 360.0 - 180.0;
        let east = (self.x + 1) as f64 / n * 360.0 - 180.0;
        let north = mercator_lat(1.0 - 2.0 * self.y as f64 / n);
        let south = mercator_lat(1.0 - 2.0 * (self.y + 1) as f64 / n);
        (west, south, east, north)
    }

    /// Ausdehnung in Web-Mercator-Metern `(west, süd, ost, nord)` — für WMS-Anfragen.
    pub fn bounds_meters(&self) -> (f64, f64, f64, f64) {
        let size = EARTH_CIRCUMFERENCE / Self::count(self.z) as f64;
        let origin = -EARTH_CIRCUMFERENCE / 2.0;
        let west = origin + self.x as f64 * size;
        let north = -origin - self.y as f64 * size;
        (west, north - size, west + size, north)
    }

    /// Mittelpunkt in Grad `(lat, lon)`.
    pub fn center(&self) -> (f64, f64) {
        let (west, south, east, north) = self.bounds();
        ((south + north) / 2.0, (west + east) / 2.0)
    }

    /// Kachel, die diesen Punkt enthält.
    pub fn from_lat_lon(lat: f64, lon: f64, z: u8) -> Self {
        let n = Self::count(z) as f64;
        let lat = lat.clamp(-MAX_LATITUDE, MAX_LATITUDE);
        let x = ((lon + 180.0) / 360.0 * n).floor().clamp(0.0, n - 1.0) as u32;
        let sin = lat.to_radians().sin();
        let y_fraction = 0.5 - ((1.0 + sin) / (1.0 - sin)).ln() / (4.0 * std::f64::consts::PI);
        let y = (y_fraction * n).floor().clamp(0.0, n - 1.0) as u32;
        Self::new(z, x, y)
    }
}

/// Umkehrung der Mercator-Abbildung: normierte y-Koordinate (−1…1) → Breitengrad.
fn mercator_lat(y: f64) -> f64 {
    let a = std::f64::consts::PI * y;
    a.sinh().atan().to_degrees()
}

/// Alle Kacheln, die den Ausschnitt `(west, süd, ost, nord)` [Grad] überdecken.
///
/// `limit` begrenzt die Anzahl — bei zu großem Ausschnitt und hoher Zoomstufe kämen
/// sonst Millionen Kacheln zusammen.
pub fn covering(bounds: (f64, f64, f64, f64), z: u8, limit: usize) -> Vec<TileId> {
    let (west, south, east, north) = bounds;
    let north_west = TileId::from_lat_lon(north, west, z);
    let south_east = TileId::from_lat_lon(south, east, z);
    let mut tiles = Vec::new();
    for y in north_west.y..=south_east.y.max(north_west.y) {
        for x in north_west.x..=south_east.x.max(north_west.x) {
            if tiles.len() >= limit {
                return tiles;
            }
            tiles.push(TileId::new(z, x, y));
        }
    }
    tiles
}

/// Bodenauflösung einer Kachel [m/Pixel].
pub fn ground_resolution(lat: f64, z: u8, tile_size: u32) -> f64 {
    EARTH_CIRCUMFERENCE * lat.to_radians().cos().abs()
        / (TileId::count(z) as f64 * tile_size.max(1) as f64)
}

/// Kleinste Zoomstufe, die mindestens die gewünschte Bodenauflösung erreicht.
pub fn zoom_for_resolution(lat: f64, meters_per_pixel: f64, tile_size: u32, max_zoom: u8) -> u8 {
    (0..=max_zoom)
        .find(|z| ground_resolution(lat, *z, tile_size) <= meters_per_pixel)
        .unwrap_or(max_zoom)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoomstufe_null_ist_eine_weltkachel() {
        let tile = TileId::new(0, 0, 0);
        let (west, south, east, north) = tile.bounds();
        assert_eq!((west, east), (-180.0, 180.0));
        assert!((north - MAX_LATITUDE).abs() < 1e-6, "{north}");
        assert!((south + MAX_LATITUDE).abs() < 1e-6, "{south}");
        assert_eq!(TileId::count(0), 1);
    }

    #[test]
    fn nullmeridian_und_aequator_treffen_die_kachelecke() {
        // Auf Zoomstufe 1 liegt (0°, 0°) genau am Schnittpunkt der vier Kacheln.
        let tile = TileId::from_lat_lon(0.0, 0.0, 1);
        assert_eq!(tile, TileId::new(1, 1, 1));
        // Knapp nordwestlich davon liegt die Kachel (0, 0).
        assert_eq!(TileId::from_lat_lon(0.1, -0.1, 1), TileId::new(1, 0, 0));
    }

    #[test]
    fn kachel_und_mittelpunkt_gehoeren_zusammen() {
        for (lat, lon, z) in [(52.0, 10.0, 14u8), (48.1, 11.6, 18), (-33.9, 151.2, 12)] {
            let tile = TileId::from_lat_lon(lat, lon, z);
            let (clat, clon) = tile.center();
            assert_eq!(TileId::from_lat_lon(clat, clon, z), tile);
            let (west, south, east, north) = tile.bounds();
            assert!(west < clon && clon < east);
            assert!(south < clat && clat < north);
        }
    }

    #[test]
    fn tms_zaehlt_von_sueden() {
        let tile = TileId::new(2, 1, 0);
        assert_eq!(tile.tms_y(), 3);
        assert_eq!(TileId::new(2, 1, 3).tms_y(), 0);
    }

    #[test]
    fn bodenaufloesung_am_aequator() {
        // Der bekannte Wert: 156 543 m/Pixel auf Stufe 0 bei 256-Pixel-Kacheln.
        let r = ground_resolution(0.0, 0, 256);
        assert!((r - 156_543.03).abs() < 0.1, "{r}");
        // Jede Zoomstufe halbiert ihn.
        assert!((ground_resolution(0.0, 1, 256) - r / 2.0).abs() < 1e-6);
        // In unseren Breiten ist die Auflösung feiner (cos φ).
        assert!(ground_resolution(52.0, 18, 256) < ground_resolution(0.0, 18, 256));
    }

    #[test]
    fn zoomstufe_zur_wunschaufloesung() {
        // 0,5 m/Pixel bei 52° N: Stufe 18 liefert etwa 0,36 m/Pixel.
        let z = zoom_for_resolution(52.0, 0.5, 256, 22);
        assert_eq!(z, 18);
        assert!(ground_resolution(52.0, z, 256) <= 0.5);
        assert!(ground_resolution(52.0, z - 1, 256) > 0.5);
    }

    #[test]
    fn ueberdeckung_eines_ausschnitts() {
        let tile = TileId::from_lat_lon(52.0, 10.0, 14);
        let (west, south, east, north) = tile.bounds();
        // Genau eine Kachel deckt ihre eigene Ausdehnung ab.
        let tiles = covering(
            (west + 1e-9, south + 1e-9, east - 1e-9, north - 1e-9),
            14,
            100,
        );
        assert_eq!(tiles, vec![tile]);

        // Ein größerer Ausschnitt liefert ein Rechteck.
        let tiles = covering((9.9, 51.9, 10.1, 52.1), 14, 1000);
        assert!(tiles.len() > 4, "{}", tiles.len());
        assert!(tiles.contains(&tile));

        // Die Obergrenze greift.
        assert_eq!(covering((-180.0, -80.0, 180.0, 80.0), 10, 50).len(), 50);
    }

    #[test]
    fn ausdehnung_in_metern_passt_zur_zoomstufe() {
        let (west, south, east, north) = TileId::new(0, 0, 0).bounds_meters();
        assert!((east - west - EARTH_CIRCUMFERENCE).abs() < 1e-6);
        assert!((north - south - EARTH_CIRCUMFERENCE).abs() < 1e-6);
        let (w2, _, e2, _) = TileId::new(1, 0, 0).bounds_meters();
        assert!((e2 - w2 - EARTH_CIRCUMFERENCE / 2.0).abs() < 1e-6);
    }
}
