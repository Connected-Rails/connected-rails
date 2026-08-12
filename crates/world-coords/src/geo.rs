//! Geodätische Umrechnungen auf dem GRS80-Ellipsoid (ETRS89 nutzt GRS80).

use crate::EcefPos;
use glam::DVec3;

/// Große Halbachse GRS80 [m].
pub const A: f64 = 6_378_137.0;
/// Abplattung GRS80.
pub const F: f64 = 1.0 / 298.257_222_101;
/// Kleine Halbachse [m].
pub const B: f64 = A * (1.0 - F);
/// Erste numerische Exzentrizität im Quadrat.
pub const E2: f64 = 2.0 * F - F * F;
/// Zweite numerische Exzentrizität im Quadrat.
pub const EP2: f64 = E2 / (1.0 - E2);

/// Geodätisch (Breite/Länge im Bogenmaß, ellipsoidische Höhe in m) → ECEF.
pub fn to_ecef(lat: f64, lon: f64, h: f64) -> EcefPos {
    let (sla, cla) = lat.sin_cos();
    let (slo, clo) = lon.sin_cos();
    let n = A / (1.0 - E2 * sla * sla).sqrt();
    EcefPos(DVec3::new(
        (n + h) * cla * clo,
        (n + h) * cla * slo,
        (n * (1.0 - E2) + h) * sla,
    ))
}

/// ECEF → geodätisch (Breite, Länge, ellipsoidische Höhe). Bowring-Verfahren,
/// eine Iteration reicht für Zentimetergenauigkeit; wir nehmen zwei.
pub fn from_ecef(p: EcefPos) -> (f64, f64, f64) {
    let DVec3 { x, y, z } = p.0;
    let r = (x * x + y * y).sqrt();
    if r < 1e-9 {
        // Pol: Länge undefiniert, wir liefern 0.
        let lat = z.signum() * std::f64::consts::FRAC_PI_2;
        return (lat, 0.0, z.abs() - B);
    }
    let lon = y.atan2(x);
    let theta = (z * A).atan2(r * B);
    let (st, ct) = theta.sin_cos();
    let mut lat = (z + EP2 * B * st.powi(3)).atan2(r - E2 * A * ct.powi(3));
    for _ in 0..2 {
        let sla = lat.sin();
        let n = A / (1.0 - E2 * sla * sla).sqrt();
        let h = r / lat.cos() - n;
        lat = (z / r).atan2(1.0 - E2 * n / (n + h));
    }
    let sla = lat.sin();
    let n = A / (1.0 - E2 * sla * sla).sqrt();
    let h = if lat.cos().abs() > 0.1 {
        r / lat.cos() - n
    } else {
        z / sla - n * (1.0 - E2)
    };
    (lat, lon, h)
}

/// Bequemer Einstieg mit Grad.
pub fn to_ecef_deg(lat_deg: f64, lon_deg: f64, h: f64) -> EcefPos {
    to_ecef(lat_deg.to_radians(), lon_deg.to_radians(), h)
}

/// Höhenbezug: DHHN2016-Normalhöhen (Quelldaten) → ellipsoidische Höhe.
///
/// ponytail: konstanter Geoid-Offset pro Strecke (Plan 4.2). In Deutschland liegt die
/// Quasigeoidundulation zwischen ~ 39 m (Nordwesten) und ~ 50 m (Süden); der Fehler eines
/// konstanten Werts über 100 km Strecke liegt im Dezimeterbereich. Echtes GCG-Raster
/// nachrüsten, sobald DGM-Import über mehrere Bundesländer geht.
pub fn ellipsoidal_height(normal_height: f64, geoid_offset: f64) -> f64 {
    normal_height + geoid_offset
}

// ---------------------------------------------------------------------------
// UTM (ETRS89 / EPSG:25832 und 25833) — nur für den Streckenimport (Plan 4.2).
//
// Bewusst ohne CRS-Bibliothek (`proj4rs`, `geodesy`): gebraucht werden für Deutschland
// genau zwei Projektionen — UTM-Zone 32N und 33N auf GRS80. Das ist die Snyder-Reihe
// unten, millimetergenau innerhalb einer Zone und ohne Abhängigkeit. Sobald Quelldaten
// in anderen Systemen dazukommen (Gauß-Krüger/DHDN, Nachbarländer), tritt hinter
// derselben Signatur `proj4rs` an die Stelle dieser Funktionen.
// ---------------------------------------------------------------------------

/// Maßstabsfaktor am Mittelmeridian einer UTM-Zone.
pub const UTM_K0: f64 = 0.9996;
/// Falscher Ostwert einer UTM-Zone [m].
pub const UTM_FALSE_EASTING: f64 = 500_000.0;

/// UTM-Zone aus einem EPSG-Code der ETRS89-/UTM-Familie (25831…25835).
pub fn utm_zone_from_epsg(epsg: u32) -> Option<u8> {
    match epsg {
        25831..=25835 => Some((epsg - 25800) as u8),
        // WGS84 / UTM Nordhalbkugel.
        32601..=32660 => Some((epsg - 32600) as u8),
        _ => None,
    }
}

/// Mittelmeridian einer UTM-Zone [rad].
fn central_meridian(zone: u8) -> f64 {
    ((zone as f64) * 6.0 - 183.0).to_radians()
}

/// Geodätisch → UTM. Liefert `(easting, northing)` [m].
pub fn to_utm(lat: f64, lon: f64, zone: u8) -> (f64, f64) {
    let lon0 = central_meridian(zone);
    let (s, c) = lat.sin_cos();
    let t = lat.tan();
    let n = A / (1.0 - E2 * s * s).sqrt();
    let tt = t * t;
    let cc = EP2 * c * c;
    let a1 = (lon - lon0) * c;

    let m = A
        * ((1.0 - E2 / 4.0 - 3.0 * E2 * E2 / 64.0 - 5.0 * E2 * E2 * E2 / 256.0) * lat
            - (3.0 * E2 / 8.0 + 3.0 * E2 * E2 / 32.0 + 45.0 * E2 * E2 * E2 / 1024.0)
                * (2.0 * lat).sin()
            + (15.0 * E2 * E2 / 256.0 + 45.0 * E2 * E2 * E2 / 1024.0) * (4.0 * lat).sin()
            - (35.0 * E2 * E2 * E2 / 3072.0) * (6.0 * lat).sin());

    let easting = UTM_FALSE_EASTING
        + UTM_K0
            * n
            * (a1
                + (1.0 - tt + cc) * a1.powi(3) / 6.0
                + (5.0 - 18.0 * tt + tt * tt + 72.0 * cc - 58.0 * EP2) * a1.powi(5) / 120.0);
    let northing = UTM_K0
        * (m + n
            * t
            * (a1 * a1 / 2.0
                + (5.0 - tt + 9.0 * cc + 4.0 * cc * cc) * a1.powi(4) / 24.0
                + (61.0 - 58.0 * tt + tt * tt + 600.0 * cc - 330.0 * EP2) * a1.powi(6) / 720.0));
    (easting, northing)
}

/// UTM → geodätisch. Liefert `(lat, lon)` [rad] (Nordhalbkugel).
pub fn from_utm(easting: f64, northing: f64, zone: u8) -> (f64, f64) {
    let lon0 = central_meridian(zone);
    let x = easting - UTM_FALSE_EASTING;
    let m = northing / UTM_K0;
    let mu = m / (A * (1.0 - E2 / 4.0 - 3.0 * E2 * E2 / 64.0 - 5.0 * E2 * E2 * E2 / 256.0));
    let e1 = (1.0 - (1.0 - E2).sqrt()) / (1.0 + (1.0 - E2).sqrt());

    let phi1 = mu
        + (3.0 * e1 / 2.0 - 27.0 * e1.powi(3) / 32.0) * (2.0 * mu).sin()
        + (21.0 * e1 * e1 / 16.0 - 55.0 * e1.powi(4) / 32.0) * (4.0 * mu).sin()
        + (151.0 * e1.powi(3) / 96.0) * (6.0 * mu).sin()
        + (1097.0 * e1.powi(4) / 512.0) * (8.0 * mu).sin();

    let (s1, c1) = phi1.sin_cos();
    let t1 = phi1.tan();
    let tt = t1 * t1;
    let cc = EP2 * c1 * c1;
    let n1 = A / (1.0 - E2 * s1 * s1).sqrt();
    let r1 = A * (1.0 - E2) / (1.0 - E2 * s1 * s1).powf(1.5);
    let d = x / (n1 * UTM_K0);

    let lat = phi1
        - (n1 * t1 / r1)
            * (d * d / 2.0
                - (5.0 + 3.0 * tt + 10.0 * cc - 4.0 * cc * cc - 9.0 * EP2) * d.powi(4) / 24.0
                + (61.0 + 90.0 * tt + 298.0 * cc + 45.0 * tt * tt - 252.0 * EP2 - 3.0 * cc * cc)
                    * d.powi(6)
                    / 720.0);
    let lon = lon0
        + (d - (1.0 + 2.0 * tt + cc) * d.powi(3) / 6.0
            + (5.0 - 2.0 * cc + 28.0 * tt - 3.0 * cc * cc + 8.0 * EP2 + 24.0 * tt * tt)
                * d.powi(5)
                / 120.0)
            / c1;
    (lat, lon)
}

/// UTM-Punkt (mit Normalhöhe) direkt nach ECEF — der Weg, den der Streckenimport nimmt.
pub fn utm_to_ecef(
    easting: f64,
    northing: f64,
    zone: u8,
    height: f64,
    geoid_offset: f64,
) -> EcefPos {
    let (lat, lon) = from_utm(easting, northing, zone);
    to_ecef(lat, lon, ellipsoidal_height(height, geoid_offset))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utm_roundtrip_is_sub_millimeter() {
        for (lat, lon, zone) in [
            (52.0f64, 10.0f64, 32u8),
            (48.1, 11.6, 32),
            (52.52, 13.40, 33),
            (54.3, 7.5, 32),
        ] {
            let (e, n) = to_utm(lat.to_radians(), lon.to_radians(), zone);
            let (lat2, lon2) = from_utm(e, n, zone);
            // In Metern gemessen: die Snyder-Reihe ist auf Millimeter genau.
            let d = to_ecef(lat2, lon2, 0.0).distance(to_ecef_deg(lat, lon, 0.0));
            assert!(d < 0.001, "{lat}/{lon}: {d} m");
        }
    }

    #[test]
    fn central_meridian_has_false_easting() {
        // Zone 32 hat den Mittelmeridian bei 9° Ost.
        let (e, n) = to_utm(0.0, 9.0f64.to_radians(), 32);
        assert!((e - UTM_FALSE_EASTING).abs() < 1e-6, "{e}");
        assert!(n.abs() < 1e-6, "{n}");

        // Östlich davon wächst der Ostwert.
        let (e2, _) = to_utm(52.0f64.to_radians(), 10.0f64.to_radians(), 32);
        assert!(e2 > UTM_FALSE_EASTING);
    }

    #[test]
    fn northing_matches_meridian_arc() {
        // Meridianbogen bis 52° N: rund 5 763 km; UTM staucht ihn um k0.
        let (_, n) = to_utm(52.0f64.to_radians(), 9.0f64.to_radians(), 32);
        let arc = n / UTM_K0;
        assert!((arc - 5_763_000.0).abs() < 5_000.0, "{arc}");
    }

    #[test]
    fn epsg_codes_map_to_zones() {
        assert_eq!(utm_zone_from_epsg(25832), Some(32));
        assert_eq!(utm_zone_from_epsg(25833), Some(33));
        assert_eq!(utm_zone_from_epsg(32632), Some(32));
        assert_eq!(utm_zone_from_epsg(4326), None);
    }

    #[test]
    fn utm_to_ecef_matches_direct_conversion() {
        let (e, n) = to_utm(52.0f64.to_radians(), 10.0f64.to_radians(), 32);
        let a = utm_to_ecef(e, n, 32, 54.0, 46.0);
        let b = to_ecef_deg(52.0, 10.0, 100.0);
        assert!(a.distance(b) < 0.01, "{} m", a.distance(b));
    }

    /// Zonengrenze: derselbe Punkt in Zone 32 und 33 liefert dieselbe Weltposition.
    #[test]
    fn zone_boundary_is_seamless_in_ecef() {
        let (lat, lon) = (52.3f64.to_radians(), 12.0f64.to_radians());
        let p32 = to_utm(lat, lon, 32);
        let p33 = to_utm(lat, lon, 33);
        assert!(
            (p32.0 - p33.0).abs() > 100_000.0,
            "andere Zone, andere Zahlen"
        );
        let a = utm_to_ecef(p32.0, p32.1, 32, 0.0, 0.0);
        let b = utm_to_ecef(p33.0, p33.1, 33, 0.0, 0.0);
        assert!(a.distance(b) < 0.01, "Naht: {} m", a.distance(b));
    }
}
