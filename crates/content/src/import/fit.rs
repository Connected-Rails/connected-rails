//! Building blocks of the alignment: sampling, heading and curvature profiles, profiles.
//!
//! The actual reconstruction of the design elements lives in [`super::alignment`];
//! here is only the preparatory work it needs.

use glam::DVec2;
use track_model::Segment;

/// A sampled support point with extra data.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SamplePoint {
    /// Position in the local ENU frame [m].
    pub pos: DVec2,
    /// Height above NHN [m].
    pub height: f64,
    /// Permitted speed [km/h].
    pub speed: f64,
}

/// Samples a polyline at equal spacing (linear interpolation).
///
/// Returns per support point `(position, arc length, index of the source segment)`.
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

/// Heading profile from **best-fit lines** over a sliding window.
///
/// The obvious variant — heading from the difference of two neighbouring points — is
/// useless on real data: with ±2 m point noise and 20 m spacing the heading determined
/// that way varies by several degrees, while the difference between a straight and an
/// 8000 m curve is in the per-mille range. Over a window of `2·w+1` points (several
/// hundred metres) the noise averages out instead.
pub(super) fn headings(points: &[SamplePoint], window: usize) -> Vec<f64> {
    let n = points.len();
    let w = window.max(1);
    let mut headings = Vec::with_capacity(n);
    for i in 0..n {
        // At the ends the window is shifted inwards instead of shortened on one side:
        // an asymmetric window twists the heading and fakes curves there.
        let lo = i.saturating_sub(w).min(n.saturating_sub(2 * w + 1));
        let hi = (lo + 2 * w).min(n - 1);
        headings.push(principal_direction(&points[lo..=hi]));
    }
    unwrap_angles(&mut headings);
    headings
}

/// Principal axis of a point cloud — the best-fit line along the point sequence.
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
    // The principal axis has no direction — turn it to the direction of travel.
    let along = points[points.len() - 1].pos - points[0].pos;
    if DVec2::new(angle.cos(), angle.sin()).dot(along) < 0.0 {
        angle += std::f64::consts::PI;
    }
    angle
}

/// Curvature profile [1/m] from the heading profile, smoothed.
///
/// `span` is the baseline length in points over which the derivative is taken — it has
/// to match the window of the heading estimate, otherwise only noise is differentiated.
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

    // At the borders the heading window stands still (it cannot reach beyond the data),
    // so zero curvature would come out there. If a curve runs up to the end of the data,
    // the last hundred metres of turning would be missing — therefore the last reliable
    // value is extrapolated outwards.
    let n = curvature.len();
    let edge = edge.min(n / 2);
    for i in 0..edge {
        curvature[i] = curvature[edge];
        curvature[n - 1 - i] = curvature[n - 1 - edge];
    }
    curvature
}

/// Gradient steps `(s, ‰)` from the heights, rounded to 0.5 ‰.
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

/// Speed steps `(s, km/h)`.
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

/// Largest deviation between the built chain and the support points [m].
///
/// The chain is walked once (not re-evaluated per point) — otherwise the check would be
/// quadratic on a 30 km line. Since design elements have lengths other than the sampling
/// step, the nearest support point is compared at every position.
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
            // Interpolate between the support points: rounding to the nearest one would
            // let half the sampling step enter the measurement as apparent deviation.
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

/// Make an angle sequence continuous (no 2π jumps).
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

/// Moving average with window width `2·radius + 1`.
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
    fn resample_yields_equal_spacing() {
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
    fn curvature_of_a_straight_is_zero() {
        let pts: Vec<DVec2> = (0..40).map(|i| DVec2::new(i as f64 * 20.0, 0.0)).collect();
        let s = samples(&pts);
        let k = curvature(&headings(&s, 5), 20.0, 5, 3, 7);
        assert!(k.iter().all(|v| v.abs() < 1e-9), "{k:?}");
    }

    #[test]
    fn heading_survives_noise() {
        // Straight to the east with ±2 m lateral offset — the heading must still be right.
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
        assert!(mid.abs() < 0.02, "window estimate: {mid} rad");

        // For comparison: from neighbour differences the error would be an order of
        // magnitude larger.
        let naive = (pts[31] - pts[29]).y.atan2((pts[31] - pts[29]).x);
        assert!(naive.abs() > mid.abs(), "naive {naive} vs window {mid}");
    }

    #[test]
    fn curvature_of_a_circular_arc_matches_the_reciprocal() {
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
    fn gradient_profile_comes_from_the_heights() {
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
