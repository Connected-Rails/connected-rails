//! Stake-out calculator — the join tool's solver, after Zusi's Absteckrechner.
//!
//! Given a start pose and an end pose (position, heading, curvature each), it
//! computes the rule-conforming element chain between them. The simplest case
//! is Zusi's own: transition curve – circular arc – transition curve, plus a
//! **compensating straight at the start or the end** (the clothoid is
//! inflexible, the straight is what absorbs the leftover length; with the
//! radius on automatic, exactly one such straight remains). Where a single
//! arc cannot reach — parallel offsets, reversing headings — a **double arc**
//! takes over: two arcs with an intermediate straight of at least the
//! configured length between them. An end that is already curved feeds its
//! curvature into the boundary transition, which is what a compound-curve
//! (Korbbogen) start amounts to.
//!
//! Like the original, the solver mixes closed forms with numeric refinement:
//! the single arc is linear algebra plus a bisection for the automatic
//! radius; the double arc is seeded from the two-circle tangent construction
//! and polished with Newton so the transitions land the chain exactly on the
//! far pose.

use content::import::alignment::CantRules;
use glam::DVec2;
use track_model::Segment;

use crate::tools::{Easements, advance, append_cant, signed_cant};

/// What the calculator may build with — Zusi's staking parameters.
#[derive(Clone, Debug)]
pub struct StakeOptions {
    /// Design speed [km/h]; 0 = the lay options' speed. Transition lengths
    /// and cant follow it.
    pub speed: f64,
    /// Arc radius [m]; 0 = automatic — the radius then grows until exactly
    /// one compensating straight remains.
    pub radius: f64,
    /// Build transition curves at every change of curvature.
    pub easements: bool,
    /// Fixed transition length [m]; 0 = the rulebook's cant ramp.
    pub easement_length: f64,
    /// Build the rulebook's cant under the arcs (needs the transitions as
    /// its ramps).
    pub cant: bool,
    /// Shortest intermediate straight of a double arc [m].
    pub min_straight: f64,
}

impl Default for StakeOptions {
    fn default() -> Self {
        Self {
            speed: 0.0,
            radius: 0.0,
            easements: true,
            easement_length: 0.0,
            cant: true,
            min_straight: 20.0,
        }
    }
}

impl StakeOptions {
    /// The easement construction the calculator works with: the rulebook at
    /// its own design speed, falling back to `lay_speed`.
    pub fn easement_rules(&self, lay_speed: f64) -> Easements {
        Easements {
            rules: CantRules::default(),
            speed: if self.speed > 0.0 { self.speed } else { lay_speed },
        }
    }
}

/// Why the calculator refused — worded like the original's messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StakeError {
    /// No geometry fits the two poses (Zusi: "nicht plausibel").
    NotPlausible,
    /// The fixed radius leaves no room for its tangent lengths.
    RadiusTooBig,
    /// The arc between the transitions would be shorter than they are.
    ArcTooShort,
    /// No double arc converged either.
    DoubleImpossible,
}

/// The staked-out chain: its segments, and the cant band under them.
#[derive(Debug)]
pub struct Staked {
    pub segments: Vec<Segment>,
    pub cant: Vec<(f64, f64)>,
}

/// Wraps an angle into (−π, π].
fn wrap(a: f64) -> f64 {
    let mut a = a.rem_euclid(std::f64::consts::TAU);
    if a > std::f64::consts::PI {
        a -= std::f64::consts::TAU;
    }
    a
}

/// The chain's end offset and change of heading, run from the origin with
/// heading `h0`.
fn chain_end(segments: &[Segment], h0: f64) -> (DVec2, f64) {
    let mut p = DVec2::ZERO;
    let mut h = h0;
    for segment in segments {
        let (q, g) = advance(p, h, segment, segment.len);
        p = q;
        h = g;
    }
    (p, h - h0)
}

/// Tightest radius the calculator lays [m] — below that is turnout ground.
const MIN_RADIUS: f64 = 50.0;
/// Widest radius the automatic search tries [m] — flatter is a straight.
const MAX_RADIUS: f64 = 50_000.0;
/// A compensating straight shorter than this is dropped rather than built.
const STRAIGHT_EPS: f64 = 0.02;

/// Stakes out the connection from the origin (heading `h0`, curvature `k0`)
/// to `target` (arrival heading `h1`, arrival curvature `k1`), in the local
/// EN plane of the start.
pub fn stake_out(
    h0: f64,
    k0: f64,
    target: DVec2,
    h1: f64,
    k1: f64,
    opts: &StakeOptions,
    e: Easements,
) -> Result<Staked, StakeError> {
    let gamma = wrap(h1 - h0);
    let e0 = DVec2::new(h0.cos(), h0.sin());
    let ahead = (target).dot(e0);
    let across = e0.perp_dot(target);

    // Nothing to bend: collinear straight ends become one straight.
    if gamma.abs() < 1e-4
        && across.abs() < 0.05
        && ahead > 1.0
        && k0.abs() < 1e-9
        && k1.abs() < 1e-9
    {
        return finish(vec![Segment::straight(ahead)], opts, e);
    }

    // The single arc with its compensating straights, then the double arc
    // where no single one reaches.
    match single_arc(h0, k0, target, h1, k1, opts, e) {
        Ok(chain) => finish(chain, opts, e),
        // A fixed radius that does not fit is the user's to change — only
        // the cases where *no* single arc exists fall through.
        Err(err @ (StakeError::RadiusTooBig | StakeError::ArcTooShort))
            if opts.radius > 0.0 =>
        {
            Err(err)
        }
        Err(_) => match double_arc(h0, k0, target, h1, k1, opts, e) {
            Ok(chain) => finish(chain, opts, e),
            Err(err) => Err(err),
        },
    }
}

/// Cant band under the finished chain — the same 10 m steps the lay tool and
/// the importer write. Cant needs its ramps, so it only comes with the
/// transitions.
fn finish(segments: Vec<Segment>, opts: &StakeOptions, e: Easements) -> Result<Staked, StakeError> {
    let mut cant = Vec::new();
    if opts.cant && opts.easements {
        append_cant(&mut cant, 0.0, &segments, e);
    }
    Ok(Staked { segments, cant })
}

/// Transition length for a change of curvature — the configured fixed
/// length, or the rulebook's cant ramp; zero while transitions are off or
/// nothing changes.
fn ramp(k_from: f64, k_to: f64, opts: &StakeOptions, e: Easements) -> f64 {
    if !opts.easements || (k_from - k_to).abs() < 1e-12 {
        return 0.0;
    }
    if opts.easement_length > 0.0 {
        return opts.easement_length;
    }
    let du = (signed_cant(k_to, e) - signed_cant(k_from, e)).abs();
    e.rules.ramp_length(du, e.speed)
}

/// The middle of a single-arc connection for curvature `k`: entry transition,
/// arc, exit transition — `None` where the turn leaves no arc between the
/// transitions.
fn single_middle(
    k: f64,
    gamma: f64,
    k0: f64,
    k1: f64,
    opts: &StakeOptions,
    e: Easements,
) -> Option<Vec<Segment>> {
    let l_in = ramp(k0, k, opts, e);
    let l_out = ramp(k, k1, opts, e);
    let turn_ramps = (k0 + k) / 2.0 * l_in + (k + k1) / 2.0 * l_out;
    let arc = (gamma - turn_ramps) / k;
    if arc < 0.5 {
        return None;
    }
    let mut middle = Vec::with_capacity(3);
    if l_in > 0.0 {
        middle.push(Segment::transition(l_in, k0, k));
    }
    middle.push(Segment {
        len: arc,
        k0: k,
        dk: 0.0,
    });
    if l_out > 0.0 {
        middle.push(Segment::transition(l_out, k, k1));
    }
    Some(middle)
}

/// The single arc: straights at the ends absorb what the arc leaves over.
/// With the radius fixed both ends may keep one; on automatic the radius
/// grows until one of them reaches zero — Zusi's "compensating straight at
/// the start or the end".
fn single_arc(
    h0: f64,
    k0: f64,
    target: DVec2,
    h1: f64,
    k1: f64,
    opts: &StakeOptions,
    e: Easements,
) -> Result<Vec<Segment>, StakeError> {
    let gamma = wrap(h1 - h0);
    let e0 = DVec2::new(h0.cos(), h0.sin());
    let e1 = DVec2::new(h1.cos(), h1.sin());
    let det = e0.perp_dot(e1); // sin(gamma)
    if gamma.abs() < 1e-4 || det.abs() < 1e-6 {
        // Parallel or reversing headings: no single arc turns that.
        return Err(StakeError::NotPlausible);
    }

    // Straight lengths for a candidate curvature: g0·e0 + middle + g1·e1
    // has to land on the target. A curved end can carry no straight — its
    // g has to come out at zero, which only the radius search can arrange.
    let solve = |k_abs: f64| -> Option<(f64, f64, Vec<Segment>)> {
        let k = gamma.signum() * k_abs;
        let middle = single_middle(k, gamma, k0, k1, opts, e)?;
        let (v, _) = chain_end(&middle, h0);
        let w = target - v;
        let g0 = w.perp_dot(e1) / det;
        let g1 = e0.perp_dot(w) / det;
        Some((g0, g1, middle))
    };
    let free = |g: f64, k_end: f64| if k_end.abs() < 1e-9 { g } else { 0.0 };

    let (g0, g1, middle) = if opts.radius > 0.0 {
        let (g0, g1, middle) = solve(1.0 / opts.radius.max(MIN_RADIUS))
            .ok_or(StakeError::ArcTooShort)?;
        if g0 < -STRAIGHT_EPS || g1 < -STRAIGHT_EPS {
            return Err(StakeError::RadiusTooBig);
        }
        (g0, g1, middle)
    } else {
        // Automatic: the smaller of the two straights (or the one a curved
        // end forces to zero) shrinks monotonically as the radius grows —
        // a bisection lands on the radius that uses it up exactly.
        let measure = |k_abs: f64| -> f64 {
            match solve(k_abs) {
                // Too tight for its own transitions: behave like "too much
                // straight left", which sends the search toward larger radii.
                None => f64::MAX,
                // Both straights shrink as the radius grows. A curved end
                // cannot carry one, so its straight is what the bisection
                // has to use up; between two straight ends the smaller one
                // goes. Whatever the other side keeps is checked below.
                Some((g0, g1, _)) => match (k0.abs() < 1e-9, k1.abs() < 1e-9) {
                    (false, true) => g0,
                    (true, false) => g1,
                    _ => g0.min(g1),
                },
            }
        };
        let (mut lo, mut hi) = (1.0 / MAX_RADIUS, 1.0 / MIN_RADIUS);
        if measure(hi) < 0.0 {
            return Err(StakeError::NotPlausible);
        }
        if measure(lo) < 0.0 {
            for _ in 0..60 {
                let mid = 0.5 * (lo + hi);
                if measure(mid) >= 0.0 {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
        }
        let (g0, g1, middle) = solve(hi).ok_or(StakeError::NotPlausible)?;
        if g0 < -STRAIGHT_EPS || g1 < -STRAIGHT_EPS {
            return Err(StakeError::NotPlausible);
        }
        (g0, g1, middle)
    };

    // A curved end that still asks for a straight has no single-arc answer.
    if free(g0, k0) != g0 && g0.abs() > STRAIGHT_EPS {
        return Err(StakeError::NotPlausible);
    }
    if free(g1, k1) != g1 && g1.abs() > STRAIGHT_EPS {
        return Err(StakeError::NotPlausible);
    }

    let mut chain = Vec::with_capacity(middle.len() + 2);
    if g0 > STRAIGHT_EPS {
        chain.push(Segment::straight(g0));
    }
    chain.extend(middle);
    if g1 > STRAIGHT_EPS {
        chain.push(Segment::straight(g1));
    }
    Ok(chain)
}

/// The double arc: two arcs around an intermediate straight of at least the
/// configured length — what a parallel offset or a reversing heading needs.
/// Seeded from the two-circle tangent construction, polished with Newton so
/// the transitions still land the chain on the far pose.
fn double_arc(
    h0: f64,
    k0: f64,
    target: DVec2,
    h1: f64,
    k1: f64,
    opts: &StakeOptions,
    e: Easements,
) -> Result<Vec<Segment>, StakeError> {
    // With the radius on automatic the calculator does what the original
    // admits to — systematic trying: a handful of candidates around the gap
    // and the common main-line radii, the shortest converging chain wins.
    let radii: Vec<f64> = if opts.radius > 0.0 {
        vec![opts.radius.max(MIN_RADIUS)]
    } else {
        let d = target.length();
        let mut candidates: Vec<f64> = [
            d / 4.0,
            d / 2.0,
            d,
            2.0 * d,
            300.0,
            600.0,
            1_000.0,
            2_500.0,
            5_000.0,
        ]
        .into_iter()
        .map(|r| r.clamp(MIN_RADIUS, MAX_RADIUS))
        .collect();
        candidates.sort_by(f64::total_cmp);
        candidates.dedup_by(|a, b| (*a - *b).abs() < 1.0);
        candidates
    };
    let min_z = opts.min_straight.max(1.0);

    let build = |k_abs: f64, s1: f64, s2: f64, l1: f64, z: f64, l2: f64| -> Option<Vec<Segment>> {
        if l1 < 0.5 || l2 < 0.5 || z < min_z - 1e-6 {
            return None;
        }
        let (ka, kb) = (s1 * k_abs, s2 * k_abs);
        let mut chain = Vec::with_capacity(7);
        let l_in = ramp(k0, ka, opts, e);
        if l_in > 0.0 {
            chain.push(Segment::transition(l_in, k0, ka));
        }
        chain.push(Segment {
            len: l1,
            k0: ka,
            dk: 0.0,
        });
        let l_mid1 = ramp(ka, 0.0, opts, e);
        if l_mid1 > 0.0 {
            chain.push(Segment::transition(l_mid1, ka, 0.0));
        }
        chain.push(Segment::straight(z));
        let l_mid2 = ramp(0.0, kb, opts, e);
        if l_mid2 > 0.0 {
            chain.push(Segment::transition(l_mid2, 0.0, kb));
        }
        chain.push(Segment {
            len: l2,
            k0: kb,
            dk: 0.0,
        });
        let l_out = ramp(kb, k1, opts, e);
        if l_out > 0.0 {
            chain.push(Segment::transition(l_out, kb, k1));
        }
        Some(chain)
    };

    let mut best: Option<Vec<Segment>> = None;
    for (radius, (s1, s2)) in radii.iter().copied().flat_map(|radius| {
        [(1.0, -1.0), (-1.0, 1.0), (1.0, 1.0), (-1.0, -1.0)].map(|hands| (radius, hands))
    }) {
        let k_abs = 1.0 / radius;
        // Seed from the tangent between the two circles the arcs run on.
        let left0 = DVec2::new(-h0.sin(), h0.cos());
        let left1 = DVec2::new(-h1.sin(), h1.cos());
        let c1 = left0 * (s1 * radius);
        let c2 = target + left1 * (s2 * radius);
        let d = c2 - c1;
        let dist = d.length();
        if dist < 1e-6 {
            continue;
        }
        let base = d.y.atan2(d.x);
        let tangent_heading = if s1 == s2 {
            base
        } else {
            let ratio: f64 = 2.0 * radius / dist;
            if ratio > 1.0 {
                continue;
            }
            base + s1 * ratio.asin()
        };
        // Turns from the headings onto the tangent and off it again, wound
        // the way each arc bends.
        let turn = |from: f64, to: f64, s: f64| -> f64 {
            let t = wrap(to - from);
            if t * s >= -1e-9 {
                t.abs()
            } else {
                std::f64::consts::TAU - t.abs()
            }
        };
        let t1 = turn(h0, tangent_heading, s1);
        let t2 = turn(tangent_heading, h1, s2);
        if t1 > 3.5 || t2 > 3.5 {
            continue;
        }
        let z_seed = if s1 == s2 {
            dist
        } else {
            (dist * dist - 4.0 * radius * radius).max(0.0).sqrt()
        };
        let mut l1 = (t1 * radius).max(1.0);
        let mut l2 = (t2 * radius).max(1.0);
        let mut z = z_seed.max(min_z);

        // Newton on (l1, z, l2): end position and heading, numeric Jacobian.
        let eval = |l1: f64, z: f64, l2: f64| -> Option<DVec2x3> {
            let chain = build(k_abs, s1, s2, l1, z, l2)?;
            let (end, dh) = chain_end(&chain, h0);
            Some(DVec2x3 {
                x: end.x - target.x,
                y: end.y - target.y,
                h: wrap(h0 + dh - h1),
            })
        };
        let mut solved = false;
        for _ in 0..30 {
            let Some(f) = eval(l1, z, l2) else { break };
            if f.x.abs() < 0.01 && f.y.abs() < 0.01 && f.h.abs() < 1e-6 {
                solved = true;
                break;
            }
            const H: f64 = 0.05;
            let (Some(f1), Some(f2), Some(f3)) = (
                eval(l1 + H, z, l2),
                eval(l1, z + H, l2),
                eval(l1, z, l2 + H),
            ) else {
                break;
            };
            let j = [
                [(f1.x - f.x) / H, (f2.x - f.x) / H, (f3.x - f.x) / H],
                [(f1.y - f.y) / H, (f2.y - f.y) / H, (f3.y - f.y) / H],
                [(f1.h - f.h) / H, (f2.h - f.h) / H, (f3.h - f.h) / H],
            ];
            let Some([d1, d2, d3]) = solve3(j, [-f.x, -f.y, -f.h]) else {
                break;
            };
            l1 = (l1 + d1).max(0.6);
            z = (z + d2).max(min_z);
            l2 = (l2 + d3).max(0.6);
        }
        if !solved {
            continue;
        }
        let Some(chain) = build(k_abs, s1, s2, l1, z, l2) else {
            continue;
        };
        let total: f64 = chain.iter().map(|s| s.len).sum();
        if best
            .as_ref()
            .is_none_or(|b| total < b.iter().map(|s| s.len).sum::<f64>())
        {
            best = Some(chain);
        }
    }
    best.ok_or(StakeError::DoubleImpossible)
}

/// Residual of the double-arc fit: position error and heading error.
struct DVec2x3 {
    x: f64,
    y: f64,
    h: f64,
}

/// Solves a 3×3 linear system by Cramer's rule; `None` when it is singular.
fn solve3(a: [[f64; 3]; 3], b: [f64; 3]) -> Option<[f64; 3]> {
    let det = |m: [[f64; 3]; 3]| -> f64 {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let d = det(a);
    if d.abs() < 1e-12 {
        return None;
    }
    let column = |i: usize| {
        let mut m = a;
        for row in 0..3 {
            m[row][i] = b[row];
        }
        det(m) / d
    };
    Some([column(0), column(1), column(2)])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(speed: f64) -> Easements {
        Easements {
            rules: CantRules::default(),
            speed,
        }
    }

    fn end_pose(segments: &[Segment], h0: f64) -> (DVec2, f64) {
        let (p, dh) = chain_end(segments, h0);
        (p, wrap(h0 + dh))
    }

    /// Zusi's simplest case: transitions, the arc, and exactly one
    /// compensating straight with the radius on automatic.
    #[test]
    fn a_single_arc_keeps_one_compensating_straight() {
        let opts = StakeOptions::default();
        let target = DVec2::new(900.0, 250.0);
        let h1 = 0.6;
        let staked = stake_out(0.0, 0.0, target, h1, 0.0, &opts, rules(120.0)).expect("stakes");
        let (end, h) = end_pose(&staked.segments, 0.0);
        assert!(end.distance(target) < 0.05, "missed by {}", end.distance(target));
        assert!(wrap(h - h1).abs() < 1e-4);
        // One straight, two transitions, one arc.
        let straights = staked
            .segments
            .iter()
            .filter(|s| s.k0 == 0.0 && s.dk == 0.0)
            .count();
        let transitions = staked.segments.iter().filter(|s| s.dk != 0.0).count();
        assert_eq!(straights, 1, "{:?}", staked.segments);
        assert_eq!(transitions, 2);
        // The cant band exists and ramps back to nothing.
        assert!(!staked.cant.is_empty());
        assert_eq!(staked.cant.last().unwrap().1, 0.0);
    }

    /// A fixed radius keeps straights at both ends where there is room, and
    /// refuses honestly where there is none.
    #[test]
    fn a_fixed_radius_reports_when_it_is_too_big() {
        let mut opts = StakeOptions {
            radius: 800.0,
            ..Default::default()
        };
        let target = DVec2::new(900.0, 250.0);
        let staked = stake_out(0.0, 0.0, target, 0.6, 0.0, &opts, rules(120.0)).expect("stakes");
        let (end, _) = end_pose(&staked.segments, 0.0);
        assert!(end.distance(target) < 0.05);
        let straights = staked
            .segments
            .iter()
            .filter(|s| s.k0 == 0.0 && s.dk == 0.0)
            .count();
        assert_eq!(straights, 2, "room for both compensating straights");

        opts.radius = 20_000.0;
        assert_eq!(
            stake_out(0.0, 0.0, target, 0.6, 0.0, &opts, rules(120.0)).unwrap_err(),
            StakeError::RadiusTooBig
        );
    }

    /// A parallel offset needs the double arc, and the intermediate straight
    /// respects its configured minimum.
    #[test]
    fn a_parallel_offset_becomes_a_double_arc_with_intermediate_straight() {
        let opts = StakeOptions {
            radius: 600.0,
            min_straight: 30.0,
            ..Default::default()
        };
        // 60 km/h keeps the rulebook transitions short enough for a flat
        // 120 m offset — at main-line speed the same figure honestly fails.
        let target = DVec2::new(800.0, 120.0);
        let staked = stake_out(0.0, 0.0, target, 0.0, 0.0, &opts, rules(60.0)).expect("stakes");
        let (end, h) = end_pose(&staked.segments, 0.0);
        assert!(end.distance(target) < 0.05, "missed by {}", end.distance(target));
        assert!(wrap(h).abs() < 1e-4);
        // Two arcs of opposite hand around a straight of at least 30 m.
        let arcs: Vec<f64> = staked
            .segments
            .iter()
            .filter(|s| s.dk == 0.0 && s.k0.abs() > 1e-9)
            .map(|s| s.k0)
            .collect();
        assert_eq!(arcs.len(), 2);
        assert!(arcs[0] * arcs[1] < 0.0, "an S needs opposite hands");
        let straight = staked
            .segments
            .iter()
            .find(|s| s.dk == 0.0 && s.k0 == 0.0)
            .expect("the intermediate straight");
        assert!(straight.len >= 30.0 - 1e-6, "{}", straight.len);
        // Opposite arcs carry opposite cant.
        let peak = staked.cant.iter().map(|(_, c)| *c).fold(0.0, f64::max);
        let low = staked.cant.iter().map(|(_, c)| *c).fold(0.0, f64::min);
        assert!(peak > 0.0 && low < 0.0, "{peak} / {low}");
    }

    /// Without transitions the chain is bare straights and arcs — the
    /// closed-form staking of a plain survey.
    #[test]
    fn without_transitions_the_chain_is_bare() {
        let opts = StakeOptions {
            easements: false,
            cant: false,
            ..Default::default()
        };
        let target = DVec2::new(500.0, 140.0);
        let staked = stake_out(0.0, 0.0, target, 0.8, 0.0, &opts, rules(120.0)).expect("stakes");
        assert!(staked.segments.iter().all(|s| s.dk == 0.0));
        assert!(staked.cant.is_empty());
        let (end, h) = end_pose(&staked.segments, 0.0);
        assert!(end.distance(target) < 0.05);
        assert!(wrap(h - 0.8).abs() < 1e-4);
    }

    /// Collinear ends are one straight, and ends that point apart are
    /// refused as not plausible.
    #[test]
    fn degenerate_poses_are_handled() {
        let opts = StakeOptions::default();
        let staked = stake_out(
            0.0,
            0.0,
            DVec2::new(300.0, 0.0),
            0.0,
            0.0,
            &opts,
            rules(120.0),
        )
        .expect("a straight");
        assert_eq!(staked.segments.len(), 1);
        assert!(staked.segments[0].k0 == 0.0 && staked.segments[0].dk == 0.0);

        // A fixed radius far beyond the gap has no room for its tangent
        // lengths — refused, not bent into place.
        let wide = StakeOptions {
            radius: 5_000.0,
            ..Default::default()
        };
        assert!(
            stake_out(
                0.0,
                0.0,
                DVec2::new(60.0, 30.0),
                1.0,
                0.0,
                &wide,
                rules(120.0)
            )
            .is_err()
        );
    }

    /// A curved start (compound-curve ground) feeds the boundary transition
    /// and forbids a straight on its side — the one compensating straight
    /// sits at the far end.
    #[test]
    fn a_curved_start_keeps_its_straight_at_the_far_end() {
        let opts = StakeOptions::default();
        let k0 = 1.0 / 1200.0;
        let target = DVec2::new(600.0, 350.0);
        let staked =
            stake_out(0.0, k0, target, 0.9, 0.0, &opts, rules(120.0)).expect("stakes");
        let (end, h) = end_pose(&staked.segments, 0.0);
        assert!(end.distance(target) < 0.05, "missed by {}", end.distance(target));
        assert!(wrap(h - 0.9).abs() < 1e-4);
        // The chain starts curved: no leading straight, and the first
        // element picks the curvature up at k0.
        let first = staked.segments.first().unwrap();
        assert!(
            (first.k0 - k0).abs() < 1e-9,
            "the chain has to start at the given curvature"
        );
    }
}
