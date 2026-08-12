//! Track geometry: straight, circular arc and clothoid in **one** representation.
//!
//! A segment is fully described by its initial curvature `k0` [1/m] and the rate of
//! change of curvature `dk` [1/m²] over the arc length:
//!
//! * `k0 = 0, dk = 0` → straight
//! * `k0 ≠ 0, dk = 0` → circular arc (R = 1/k0)
//! * `dk ≠ 0`         → clothoid (transition curve)
//!
//! Heading: `heading(s) = h0 + k0·s + dk·s²/2`, position is its integral.
//! Solvable in closed form for straight/arc, numerically for the clothoid
//! (Gauss-Legendre).

use glam::DVec2;
use serde::{Deserialize, Serialize};

/// A geometry segment of a track edge.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    /// Arc length [m].
    pub len: f64,
    /// Curvature at the segment start [1/m], positive = left-hand curve.
    pub k0: f64,
    /// Rate of change of curvature [1/m²].
    pub dk: f64,
}

impl Segment {
    pub fn straight(len: f64) -> Self {
        Self {
            len,
            k0: 0.0,
            dk: 0.0,
        }
    }

    /// Circular arc with radius `radius` [m]; positive sign = left-hand curve.
    pub fn arc(len: f64, radius: f64) -> Self {
        Self {
            len,
            k0: 1.0 / radius,
            dk: 0.0,
        }
    }

    /// Transition curve from curvature `k_start` to `k_end` over `len`.
    pub fn transition(len: f64, k_start: f64, k_end: f64) -> Self {
        Self {
            len,
            k0: k_start,
            dk: (k_end - k_start) / len,
        }
    }

    /// Curvature at position `s` within the segment.
    pub fn curvature_at(&self, s: f64) -> f64 {
        self.k0 + self.dk * s
    }

    /// Curvature at the segment end.
    pub fn end_curvature(&self) -> f64 {
        self.curvature_at(self.len)
    }

    /// Change of heading from the segment start up to `s` [rad].
    pub fn heading_delta(&self, s: f64) -> f64 {
        self.k0 * s + 0.5 * self.dk * s * s
    }

    /// Offset from the segment start up to `s`, in the frame of the segment start
    /// (x = initial heading, y = to the left of it).
    pub fn offset(&self, s: f64) -> DVec2 {
        if self.dk == 0.0 {
            if self.k0.abs() < 1e-12 {
                return DVec2::new(s, 0.0);
            }
            // Circular arc: closed-form solution.
            let r = 1.0 / self.k0;
            let a = self.k0 * s;
            return DVec2::new(r * a.sin(), r * (1.0 - a.cos()));
        }
        // Clothoid: Fresnel integral numerically. Piecewise 5-point Gauss-Legendre,
        // piece length <= 25 m — error well below 1 mm at railway radii.
        let steps = (s.abs() / 25.0).ceil().max(1.0) as usize;
        let h = s / steps as f64;
        let mut p = DVec2::ZERO;
        for i in 0..steps {
            let a = i as f64 * h;
            p += gauss_legendre5(a, a + h, |u| {
                let th = self.heading_delta(u);
                DVec2::new(th.cos(), th.sin())
            });
        }
        p
    }
}

/// 5-point Gauss-Legendre quadrature for vector-valued integrands.
fn gauss_legendre5(a: f64, b: f64, f: impl Fn(f64) -> DVec2) -> DVec2 {
    // Nodes/weights on [-1,1].
    const X: [f64; 5] = [
        0.0,
        -0.538_469_310_105_683,
        0.538_469_310_105_683,
        -0.906_179_845_938_664,
        0.906_179_845_938_664,
    ];
    const W: [f64; 5] = [
        0.568_888_888_888_889,
        0.478_628_670_499_366,
        0.478_628_670_499_366,
        0.236_926_885_056_189,
        0.236_926_885_056_189,
    ];
    let c = 0.5 * (a + b);
    let hl = 0.5 * (b - a);
    let mut sum = DVec2::ZERO;
    for i in 0..5 {
        sum += f(c + hl * X[i]) * W[i];
    }
    sum * hl
}

/// Pose within a segment chain, in the local 2D frame of the chain.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlanPose {
    pub pos: DVec2,
    /// Heading [rad], 0 = +x.
    pub heading: f64,
    /// Curvature [1/m].
    pub curvature: f64,
}

/// Evaluates a segment chain at arc length `s` (from the chain start, start pose =
/// origin/heading0).
pub fn eval_chain(segments: &[Segment], heading0: f64, s: f64) -> PlanPose {
    let mut pos = DVec2::ZERO;
    let mut heading = heading0;
    let mut rest = s;
    for seg in segments {
        let local = rest.min(seg.len).max(0.0);
        let off = seg.offset(local);
        let (sh, ch) = heading.sin_cos();
        pos += DVec2::new(ch * off.x - sh * off.y, sh * off.x + ch * off.y);
        if rest <= seg.len {
            return PlanPose {
                pos,
                heading: heading + seg.heading_delta(local),
                curvature: seg.curvature_at(local),
            };
        }
        heading += seg.heading_delta(seg.len);
        rest -= seg.len;
    }
    // Beyond the end: extrapolate in a straight line (the caller checks the bounds).
    let (sh, ch) = heading.sin_cos();
    PlanPose {
        pos: pos + DVec2::new(ch * rest, sh * rest),
        heading,
        curvature: segments.last().map_or(0.0, Segment::end_curvature),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn straight_is_straight() {
        let p = eval_chain(&[Segment::straight(100.0)], 0.0, 40.0);
        assert!((p.pos - DVec2::new(40.0, 0.0)).length() < 1e-12);
        assert_eq!(p.heading, 0.0);
    }

    #[test]
    fn quarter_circle_ends_where_expected() {
        let r = 300.0;
        let len = std::f64::consts::FRAC_PI_2 * r;
        let p = eval_chain(&[Segment::arc(len, r)], 0.0, len);
        assert!((p.pos - DVec2::new(r, r)).length() < 1e-9, "{:?}", p.pos);
        assert!((p.heading - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    }

    #[test]
    fn clothoid_matches_fresnel_reference() {
        // Clothoid from 0 to R=500 over 120 m; compared against fine rectangle integration.
        let seg = Segment::transition(120.0, 0.0, 1.0 / 500.0);
        let p = seg.offset(120.0);
        let n = 2_000_000;
        let h = 120.0 / n as f64;
        let mut r = DVec2::ZERO;
        for i in 0..n {
            let u = (i as f64 + 0.5) * h;
            let th = seg.heading_delta(u);
            r += DVec2::new(th.cos(), th.sin()) * h;
        }
        assert!((p - r).length() < 1e-6, "{p:?} vs {r:?}");
    }

    #[test]
    fn chain_is_continuous() {
        let segs = [
            Segment::straight(50.0),
            Segment::transition(60.0, 0.0, 1.0 / 400.0),
            Segment::arc(200.0, 400.0),
        ];
        let total = 310.0;
        for i in 0..1000 {
            let s = total * i as f64 / 1000.0;
            let a = eval_chain(&segs, 0.0, s);
            let b = eval_chain(&segs, 0.0, s + 0.01);
            let d = (b.pos - a.pos).length();
            assert!((d - 0.01).abs() < 1e-6, "jump at s={s}: {d}");
        }
    }
}
