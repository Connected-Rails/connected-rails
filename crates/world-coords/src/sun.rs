//! Sun and moon position for day/night lighting (plan ch. 14).
//!
//! Low-precision astronomy: ~0.1° for the sun and a few degrees for the moon —
//! plenty for a light source, far from an ephemeris. All angles are radians;
//! azimuth counts from north through east, matching the ENU convention.

use std::f64::consts::{PI, TAU};

/// Julian date for a Gregorian calendar day plus seconds since midnight UT.
/// `seconds` may exceed one day — multi-day runs just keep counting.
pub fn julian_date(year: i32, month: u32, day: u32, seconds: f64) -> f64 {
    // Fliegel & Van Flandern day number; relies on truncating integer division.
    let (y, m, d) = (i64::from(year), i64::from(month), i64::from(day));
    let a = (m - 14) / 12;
    let jdn = (1461 * (y + 4800 + a)) / 4 + (367 * (m - 2 - 12 * a)) / 12
        - (3 * ((y + 4900 + a) / 100)) / 4
        + d
        - 32075;
    jdn as f64 - 0.5 + seconds / 86_400.0
}

/// Sun azimuth (from north, clockwise) and elevation \[rad\] as seen from
/// latitude/longitude \[rad\] at Julian date `jd`.
pub fn sun_position(jd: f64, lat: f64, lon: f64) -> (f64, f64) {
    let n = jd - 2_451_545.0;
    horizontal(n, lat, lon, equatorial(n, sun_ecliptic_longitude(n), 0.0))
}

/// Moon azimuth, elevation \[rad\] and illuminated fraction (0 = new, 1 = full).
pub fn moon_position(jd: f64, lat: f64, lon: f64) -> (f64, f64, f64) {
    let n = jd - 2_451_545.0;
    // Largest perturbation term only (evection and friends dropped): ~2° in longitude.
    let lambda = (218.316 + 13.176_396 * n).to_radians()
        + 0.109_76 * (134.963 + 13.064_993 * n).to_radians().sin();
    let beta = 0.089_50 * (93.272 + 13.229_350 * n).to_radians().sin();
    let (az, el) = horizontal(n, lat, lon, equatorial(n, lambda, beta));
    // Phase from the elongation to the sun; the moon's ecliptic latitude is small
    // enough to ignore for a brightness factor.
    let fraction = 0.5 * (1.0 - (lambda - sun_ecliptic_longitude(n)).cos());
    (az, el, fraction)
}

/// Ecliptic longitude of the sun \[rad\] at `n` days since J2000.
fn sun_ecliptic_longitude(n: f64) -> f64 {
    let l = (280.460 + 0.985_647_4 * n).to_radians();
    let g = (357.528 + 0.985_600_3 * n).to_radians();
    l + 0.033_42 * g.sin() + 0.000_349 * (2.0 * g).sin()
}

/// Ecliptic → equatorial: right ascension and declination \[rad\].
fn equatorial(n: f64, lambda: f64, beta: f64) -> (f64, f64) {
    let eps = (23.439 - 0.000_000_4 * n).to_radians();
    let ra = f64::atan2(
        lambda.sin() * eps.cos() - beta.tan() * eps.sin(),
        lambda.cos(),
    );
    let dec = (beta.sin() * eps.cos() + beta.cos() * eps.sin() * lambda.sin()).asin();
    (ra, dec)
}

/// Equatorial → horizontal coordinates at an observer.
fn horizontal(n: f64, lat: f64, lon: f64, (ra, dec): (f64, f64)) -> (f64, f64) {
    // Greenwich mean sidereal time as an angle.
    let gmst = (280.460_618_37 + 360.985_647_366_29 * n).to_radians();
    let h = gmst + lon - ra;
    let el = (lat.sin() * dec.sin() + lat.cos() * dec.cos() * h.cos()).asin();
    // Measured from south; the shift by π counts from north through east.
    let az = f64::atan2(h.sin(), h.cos() * lat.sin() - dec.tan() * lat.cos());
    ((az + PI).rem_euclid(TAU), el)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEG: f64 = PI / 180.0;

    #[test]
    fn sun_follows_date_and_place() {
        // 52°N 10°E; mean solar noon there is 11:20 UT.
        let (lat, lon) = (52.0 * DEG, 10.0 * DEG);
        let noon = 11.0 * 3600.0 + 20.0 * 60.0;

        // Summer solstice: culmination at 90° − 52° + 23.4° ≈ 61.4°, due south.
        let (az, el) = sun_position(julian_date(2026, 6, 21, noon), lat, lon);
        assert!((el - 61.4 * DEG).abs() < 0.5 * DEG, "el {}", el / DEG);
        assert!((az - 180.0 * DEG).abs() < 5.0 * DEG, "az {}", az / DEG);

        // Winter solstice: only 90° − 52° − 23.4° ≈ 14.6°.
        let (_, el) = sun_position(julian_date(2026, 12, 21, noon), lat, lon);
        assert!((el - 14.6 * DEG).abs() < 0.5 * DEG, "el {}", el / DEG);

        // Midnight: below the horizon, due north.
        let (az, el) = sun_position(julian_date(2026, 6, 21, noon - 43_200.0), lat, lon);
        assert!(el < -10.0 * DEG, "el {}", el / DEG);
        assert!(!(10.0 * DEG..=350.0 * DEG).contains(&az), "az {}", az / DEG);
    }

    #[test]
    fn moon_phase_cycles() {
        let (lat, lon) = (52.0 * DEG, 10.0 * DEG);
        for i in 0..12 {
            // Half a synodic month apart, the illuminated fractions are complementary.
            let jd = julian_date(2026, 1, 1, 0.0) + f64::from(i) * 5.3;
            let (_, el, f) = moon_position(jd, lat, lon);
            let (_, _, f2) = moon_position(jd + 14.765, lat, lon);
            assert!((0.0..=1.0).contains(&f), "fraction {f}");
            assert!(el.is_finite());
            assert!((f + f2 - 1.0).abs() < 0.25, "f {f} f2 {f2}");
        }
    }
}
