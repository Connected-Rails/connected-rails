//! Bausteine der Trassierung: Abtastung, Richtungs- und Krümmungsverlauf, Profile.
//!
//! Die eigentliche Rekonstruktion der Entwurfselemente steht in [`super::alignment`];
//! hier liegt nur, was sie an Vorarbeit braucht.

use glam::DVec2;
use track_model::Segment;

/// Ein abgetasteter Stützpunkt mit Zusatzdaten.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SamplePoint {
    /// Lage im lokalen ENU-Frame [m].
    pub pos: DVec2,
    /// Höhe über NHN [m].
    pub height: f64,
    /// Zulässige Geschwindigkeit [km/h].
    pub speed: f64,
}

/// Tastet eine Polylinie in gleichen Abständen ab (lineare Interpolation).
///
/// Liefert je Stützpunkt `(Position, Bogenlänge, Index des Quellsegments)`.
pub fn resample(points: &[DVec2], step: f64) -> Vec<(DVec2, f64, usize)> {
    let mut out = Vec::new();
    if points.len() < 2 {
        return out;
    }
    let mut lengths = Vec::with_capacity(points.len());
    let mut total = 0.0;
    lengths.push(0.0);
    for w in points.windows(2) {
        total += (w[1] - w[0]).length();
        lengths.push(total);
    }

    let count = (total / step).floor() as usize;
    let mut segment = 0usize;
    for i in 0..=count {
        let s = i as f64 * step;
        while segment + 2 < points.len() && lengths[segment + 1] < s {
            segment += 1;
        }
        let span = (lengths[segment + 1] - lengths[segment]).max(1e-9);
        let t = ((s - lengths[segment]) / span).clamp(0.0, 1.0);
        out.push((points[segment].lerp(points[segment + 1], t), s, segment));
    }
    out
}

/// Richtungsverlauf aus **Ausgleichsgeraden** über ein gleitendes Fenster.
///
/// Die naheliegende Variante — Richtung aus der Differenz zweier Nachbarpunkte — ist bei
/// realen Daten unbrauchbar: bei ±2 m Punktrauschen und 20 m Abstand schwankt die so
/// bestimmte Richtung um mehrere Grad, während der Unterschied zwischen einer Geraden und
/// einem 8000-m-Bogen im Promillebereich liegt. Über ein Fenster von `2·w+1` Punkten
/// (mehrere hundert Meter) mittelt sich das Rauschen dagegen weg.
pub(super) fn headings(points: &[SamplePoint], window: usize) -> Vec<f64> {
    let n = points.len();
    let w = window.max(1);
    let mut headings = Vec::with_capacity(n);
    for i in 0..n {
        // An den Enden wird das Fenster nach innen verschoben statt einseitig verkürzt:
        // ein asymmetrisches Fenster verdreht die Richtung und täuscht dort Bögen vor.
        let lo = i.saturating_sub(w).min(n.saturating_sub(2 * w + 1));
        let hi = (lo + 2 * w).min(n - 1);
        headings.push(principal_direction(&points[lo..=hi]));
    }
    unwrap_angles(&mut headings);
    headings
}

/// Hauptachse einer Punktwolke — die Ausgleichsgerade in Richtung der Punktfolge.
fn principal_direction(points: &[SamplePoint]) -> f64 {
    if points.len() < 2 {
        return 0.0;
    }
    let mean = points.iter().fold(DVec2::ZERO, |a, p| a + p.pos) / points.len() as f64;
    let (mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0);
    for p in points {
        let d = p.pos - mean;
        sxx += d.x * d.x;
        syy += d.y * d.y;
        sxy += d.x * d.y;
    }
    let mut angle = 0.5 * (2.0 * sxy).atan2(sxx - syy);
    // Die Hauptachse ist richtungslos — auf die Fahrtrichtung drehen.
    let along = points[points.len() - 1].pos - points[0].pos;
    if DVec2::new(angle.cos(), angle.sin()).dot(along) < 0.0 {
        angle += std::f64::consts::PI;
    }
    angle
}

/// Krümmungsverlauf [1/m] aus dem Richtungsverlauf, geglättet.
///
/// `span` ist die Basislänge in Punkten, über die differenziert wird — sie muss zum
/// Fenster der Richtungsschätzung passen, sonst wird nur Rauschen differenziert.
pub(super) fn curvature(
    headings: &[f64],
    step: f64,
    span: usize,
    smoothing: usize,
    edge: usize,
) -> Vec<f64> {
    let n = headings.len();
    let d = span.max(1);
    let mut curvature = Vec::with_capacity(n);
    for i in 0..n {
        let lo = i.saturating_sub(d);
        let hi = (i + d).min(n - 1);
        let baseline = (hi - lo) as f64 * step;
        curvature.push(if baseline > 0.0 {
            (headings[hi] - headings[lo]) / baseline
        } else {
            0.0
        });
    }
    let mut curvature = smooth(&curvature, smoothing);

    // An den Rändern steht das Richtungsfenster still (es kann nicht über die Daten
    // hinausgreifen), dort käme sonst Krümmung null heraus. Führt ein Bogen bis ans
    // Datenende, fehlten dadurch die letzten hundert Meter Drehung — deshalb wird der
    // letzte belastbare Wert nach außen fortgeschrieben.
    let n = curvature.len();
    let edge = edge.min(n / 2);
    for i in 0..edge {
        curvature[i] = curvature[edge];
        curvature[n - 1 - i] = curvature[n - 1 - edge];
    }
    curvature
}

/// Neigungsstufen `(s, ‰)` aus den Höhen, auf 0,5 ‰ gerundet.
pub(super) fn grade_profile(points: &[SamplePoint], step: f64) -> Vec<(f64, f64)> {
    let mut grade = Vec::new();
    let mut last = f64::NAN;
    for i in 0..points.len().saturating_sub(1) {
        let g = (points[i + 1].height - points[i].height) / step * 1000.0;
        let g = (g * 2.0).round() / 2.0;
        if last.is_nan() || (g - last).abs() >= 0.5 {
            grade.push((i as f64 * step, g));
            last = g;
        }
    }
    if grade.is_empty() {
        grade.push((0.0, 0.0));
    }
    grade
}

/// Geschwindigkeitsstufen `(s, km/h)`.
pub(super) fn speed_profile(points: &[SamplePoint], step: f64) -> Vec<(f64, f64)> {
    let mut speed = Vec::new();
    let mut last = f64::NAN;
    for (i, p) in points.iter().enumerate() {
        if last.is_nan() || (p.speed - last).abs() > 0.1 {
            speed.push((i as f64 * step, p.speed));
            last = p.speed;
        }
    }
    speed
}

/// Größte Abweichung zwischen der gebauten Kette und den Stützpunkten [m].
///
/// Die Kette wird einmal durchlaufen (nicht je Punkt neu ausgewertet) — sonst wäre die
/// Prüfung bei 30 km Strecke quadratisch. Da Entwurfselemente andere Längen haben als
/// der Abtastschritt, wird zu jeder Stelle der nächstgelegene Stützpunkt verglichen.
pub(super) fn deviation(segments: &[Segment], heading0: f64, points: &[SamplePoint]) -> f64 {
    if points.len() < 2 {
        return 0.0;
    }
    let step = (points[1].pos - points[0].pos).length().max(1e-6);
    let origin = points[0].pos;
    let mut max: f64 = 0.0;
    let mut pos = DVec2::ZERO;
    let mut heading = heading0;
    let mut travelled = 0.0;

    for segment in segments {
        let sub = (segment.len / step).ceil().max(1.0) as usize;
        let (sh, ch) = heading.sin_cos();
        for i in 0..sub {
            let local = segment.len * i as f64 / sub as f64;
            let off = segment.offset(local);
            let p = pos + DVec2::new(ch * off.x - sh * off.y, sh * off.x + ch * off.y);
            // Zwischen den Stützpunkten interpolieren: rundete man auf den nächsten,
            // ginge der halbe Abtastschritt als scheinbare Abweichung in die Messung ein.
            let t = (travelled + local) / step;
            let index = t.floor() as usize;
            if let (Some(a), Some(b)) = (points.get(index), points.get(index + 1)) {
                let reference = a.pos.lerp(b.pos, t - index as f64);
                max = max.max((p + origin - reference).length());
            }
        }
        let off = segment.offset(segment.len);
        pos += DVec2::new(ch * off.x - sh * off.y, sh * off.x + ch * off.y);
        heading += segment.heading_delta(segment.len);
        travelled += segment.len;
    }
    max
}

/// Winkelfolge stetig machen (keine 2π-Sprünge).
fn unwrap_angles(angles: &mut [f64]) {
    for i in 1..angles.len() {
        let mut d = angles[i] - angles[i - 1];
        while d > std::f64::consts::PI {
            angles[i] -= std::f64::consts::TAU;
            d -= std::f64::consts::TAU;
        }
        while d < -std::f64::consts::PI {
            angles[i] += std::f64::consts::TAU;
            d += std::f64::consts::TAU;
        }
    }
}

/// Gleitender Mittelwert mit Fensterbreite `2·radius + 1`.
fn smooth(values: &[f64], radius: usize) -> Vec<f64> {
    if radius == 0 {
        return values.to_vec();
    }
    let n = values.len();
    (0..n)
        .map(|i| {
            let lo = i.saturating_sub(radius);
            let hi = (i + radius).min(n - 1);
            values[lo..=hi].iter().sum::<f64>() / (hi - lo + 1) as f64
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples(points: &[DVec2]) -> Vec<SamplePoint> {
        points
            .iter()
            .map(|p| SamplePoint {
                pos: *p,
                height: 0.0,
                speed: 100.0,
            })
            .collect()
    }

    #[test]
    fn resample_liefert_gleiche_abstaende() {
        let pts = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(100.0, 0.0),
            DVec2::new(100.0, 100.0),
        ];
        let out = resample(&pts, 25.0);
        assert_eq!(out.len(), 9); // 200 m / 25 m + 1
        for w in out.windows(2) {
            let d = (w[1].0 - w[0].0).length();
            assert!(d <= 25.0 + 1e-9);
        }
        assert!((out.last().unwrap().0 - DVec2::new(100.0, 100.0)).length() < 1e-9);
    }

    #[test]
    fn kruemmung_einer_geraden_ist_null() {
        let pts: Vec<DVec2> = (0..40).map(|i| DVec2::new(i as f64 * 20.0, 0.0)).collect();
        let s = samples(&pts);
        let k = curvature(&headings(&s, 5), 20.0, 5, 3, 7);
        assert!(k.iter().all(|v| v.abs() < 1e-9), "{k:?}");
    }

    #[test]
    fn richtung_ueberlebt_rauschen() {
        // Gerade nach Osten mit ±2 m Querversatz — die Richtung muss trotzdem stimmen.
        let mut seed = 7u64;
        let mut rand = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) as f64 / (1u64 << 31) as f64) - 1.0
        };
        let pts: Vec<DVec2> = (0..60)
            .map(|i| DVec2::new(i as f64 * 20.0 + rand() * 2.0, rand() * 2.0))
            .collect();
        let s = samples(&pts);

        let windowed = headings(&s, 5);
        let mid = windowed[30];
        assert!(mid.abs() < 0.02, "Fensterschätzung: {mid} rad");

        // Zum Vergleich: aus Nachbardifferenzen wäre der Fehler eine Größenordnung größer.
        let naive = (pts[31] - pts[29]).y.atan2((pts[31] - pts[29]).x);
        assert!(naive.abs() > mid.abs(), "naiv {naive} vs Fenster {mid}");
    }

    #[test]
    fn kruemmung_eines_kreisbogens_trifft_den_kehrwert() {
        let r = 800.0;
        let step = 20.0;
        let pts: Vec<DVec2> = (0..60)
            .map(|i| {
                let a = i as f64 * step / r;
                DVec2::new(r * a.sin(), r * (1.0 - a.cos()))
            })
            .collect();
        let s = samples(&pts);
        let k = curvature(&headings(&s, 5), step, 5, 3, 7);
        let mid = k[k.len() / 2];
        assert!((mid - 1.0 / r).abs() < 1.0 / r * 0.05, "{mid}");
    }

    #[test]
    fn neigungsprofil_kommt_aus_den_hoehen() {
        let pts: Vec<DVec2> = (0..30).map(|i| DVec2::new(i as f64 * 20.0, 0.0)).collect();
        let mut s = samples(&pts);
        for (i, p) in s.iter_mut().enumerate() {
            p.height = if i < 15 { 0.0 } else { (i - 15) as f64 * 0.2 };
        }
        let grade = grade_profile(&s, 20.0);
        assert!(grade.len() >= 2, "{grade:?}");
        assert!((grade.last().unwrap().1 - 10.0).abs() < 0.6);
    }
}
