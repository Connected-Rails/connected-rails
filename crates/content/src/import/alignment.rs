//! Alignment: turning a noisy point sequence into real design elements.
//!
//! The naive variant (smooth the curvature and emit one segment per sampling step) yields
//! a drivable but invented curvature sequence. Instead, what an alignment engineer would
//! have designed is reconstructed here:
//!
//! 1. **Separate sections**: straight stretches and curves based on the smoothed
//!    curvature.
//! 2. **Fit the radius**: circle fit (Kåsa) over the whole curve — the noise of the
//!    support points averages out with √n, while a local difference fails at this (versine
//!    of a 50 m chord at R = 1000 m: 31 cm against metres of point noise).
//! 3. **Preserve the change of direction**: the measured total turn per curve is kept, so
//!    that the alignment does not drift away from the original.
//! 4. **Compute transition curves and cant**: what cannot be measured from the data comes
//!    from the rulebook — cant from radius and line speed, ramp length from that.
//!
//! The result is a chain of straight – clothoid – circular arc – clothoid – straight,
//! that is exactly the representation `track-model` keeps anyway.

use super::fit::SamplePoint;
use glam::DVec2;
use track_model::Segment;

/// Rulebook for the cant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CantRules {
    /// Highest permitted cant [mm].
    pub max_cant: f64,
    /// Permitted cant deficiency [mm] — that much less than the equilibrium value is
    /// applied, so that slower trains do not ride the curve "inwards".
    pub deficiency: f64,
    /// Rounding of the applied cant [mm].
    pub round_to: f64,
    /// Ramp gradient 1:(factor·v) — at 160 km/h and 100 mm that is 1:1600, i.e. 160 m.
    pub ramp_factor: f64,
    /// Shortest transition ramp [m].
    pub min_ramp: f64,
}

impl Default for CantRules {
    fn default() -> Self {
        Self {
            max_cant: 160.0,
            deficiency: 60.0,
            round_to: 5.0,
            ramp_factor: 10.0,
            min_ramp: 20.0,
        }
    }
}

impl CantRules {
    /// Equilibrium cant [mm]: `u = 11.8 · v²/R`.
    ///
    /// Derivation: `u = G·v²/(g·R)` with wheel contact width `G = 1500 mm`;
    /// with `v` in km/h the prefactor becomes `1500/(9.81·3.6²) = 11.8`.
    pub fn equilibrium(radius: f64, v_kmh: f64) -> f64 {
        if radius.abs() < 1.0 {
            return 0.0;
        }
        11.8 * v_kmh * v_kmh / radius.abs()
    }

    /// Cant that is actually applied [mm].
    pub fn applied(&self, radius: f64, v_kmh: f64) -> f64 {
        let raw = (Self::equilibrium(radius, v_kmh) - self.deficiency).clamp(0.0, self.max_cant);
        (raw / self.round_to).round() * self.round_to
    }

    /// Length of the cant ramp [m] — at the same time the minimum length of the
    /// transition curve. Takes the magnitude: a right-hand curve carries its
    /// cant as a negative number and still needs the full ramp.
    pub fn ramp_length(&self, cant_mm: f64, v_kmh: f64) -> f64 {
        (cant_mm.abs() / 1000.0 * self.ramp_factor * v_kmh).max(self.min_ramp)
    }

    /// Round to the installation step.
    pub fn round(&self, cant_mm: f64) -> f64 {
        (cant_mm.clamp(0.0, self.max_cant) / self.round_to).round() * self.round_to
    }
}

/// Alignment settings.
#[derive(Debug, Clone, PartialEq)]
pub struct AlignmentOptions {
    /// Sampling distance of the support points [m].
    pub sample: f64,
    /// Window of the heading estimate (points per side) — at 20 m spacing 5 points
    /// correspond to a baseline of 200 m, over which point noise averages out.
    pub window: usize,
    /// Baseline of the curvature computation (points per side). Large = noise-resistant,
    /// small = sharp section boundaries.
    pub curvature_span: usize,
    /// Smoothing window of the curvature (points per side).
    pub smoothing: usize,
    /// From this radius on the section counts as straight [m].
    ///
    /// In terms of running dynamics everything from ~8 km on would be straight —
    /// geometrically not: a 15 km curve runs more than 100 m away from the straight over
    /// two kilometres. Therefore curves are classified generously; a 30 km curve gets no
    /// cant anyway.
    pub straight_radius: f64,
    /// Shortest standalone element [m].
    ///
    /// Acts as a noise filter at the same time: shorter "curves" in noisy source data are
    /// almost always digitisation errors and are merged into the neighbouring section.
    pub min_element: f64,
    /// Round radii to the standard series.
    pub snap_radii: bool,
    /// Only round if the standard radius lies within this relative deviation. Otherwise
    /// the measured value stays — a forced standard radius that is several percent off
    /// distorts the whole curve.
    pub snap_tolerance: f64,
    /// Standard radii that are rounded to [m].
    pub preferred_radii: Vec<f64>,
    pub cant: CantRules,
}

impl Default for AlignmentOptions {
    fn default() -> Self {
        Self {
            sample: 20.0,
            window: 5,
            curvature_span: 2,
            smoothing: 2,
            straight_radius: 30_000.0,
            min_element: 120.0,
            snap_radii: true,
            snap_tolerance: 0.04,
            preferred_radii: preferred_radii(),
            cant: CantRules::default(),
        }
    }
}

/// Common design radii: finely stepped in the tight range, coarser for large radii.
/// The standard radii an alignment is rounded to [m] — also what the editor's
/// lay tool snaps a drawn arc onto.
pub fn preferred_radii() -> Vec<f64> {
    let mut radii = vec![150.0, 180.0, 190.0, 200.0, 225.0, 250.0, 275.0];
    let mut r = 300.0;
    while r < 2000.0 {
        radii.push(r);
        r += 50.0;
    }
    while r < 5000.0 {
        radii.push(r);
        r += 250.0;
    }
    while r <= 25_000.0 {
        radii.push(r);
        r += 500.0;
    }
    radii
}

/// Kind of a design element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementKind {
    Straight,
    /// Transition curve (clothoid).
    Transition,
    Arc,
}

/// A design element — what would be written in the alignment plan.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Element {
    pub kind: ElementKind,
    /// Start measured from the beginning of the line [m].
    pub start_s: f64,
    pub length: f64,
    /// Radius [m], positive = left-hand curve.
    pub radius: Option<f64>,
    /// Cant at the end of the element [mm].
    pub cant: f64,
    /// Permitted speed that was used in the computation [km/h].
    pub speed: f64,
}

/// Result of the alignment.
#[derive(Debug, Clone, PartialEq)]
pub struct Alignment {
    pub segments: Vec<Segment>,
    /// Design elements — the basis for the editor and the line diagram.
    pub elements: Vec<Element>,
    /// Cant steps `(s, mm)`.
    pub cant: Vec<(f64, f64)>,
    /// Gradient steps `(s, ‰)`.
    pub grade: Vec<(f64, f64)>,
    /// Speed steps `(s, km/h)`.
    pub speed: Vec<(f64, f64)>,
    pub start_heading: f64,
    /// Largest deviation from the point sequence [m].
    pub max_deviation: f64,
}

impl Alignment {
    pub fn length(&self) -> f64 {
        self.segments.iter().map(|s| s.len).sum()
    }

    /// Number of curves — a metric for the import report.
    pub fn arcs(&self) -> usize {
        self.elements
            .iter()
            .filter(|e| e.kind == ElementKind::Arc)
            .count()
    }
}

/// A contiguous section of the same kind within the point sequence.
#[derive(Debug, Clone, Copy)]
struct Run {
    start: usize,
    end: usize,
    curved: bool,
}

impl Run {
    fn len(&self, sample: f64) -> f64 {
        (self.end - self.start) as f64 * sample
    }
}

/// Aligns the point sequence.
pub fn fit(points: &[SamplePoint], options: &AlignmentOptions) -> Alignment {
    assert!(points.len() >= 3, "at least three support points required");
    let h = options.sample;
    let headings = super::fit::headings(points, options.window);
    let curvature = super::fit::curvature(
        &headings,
        h,
        options.curvature_span,
        options.smoothing,
        options.window + options.curvature_span,
    );

    let runs = segment(&curvature, options);

    // Per curve: fit the radius and measure the turn angle.
    let mut plan: Vec<PlannedRun> = Vec::new();
    for (index, run) in runs.iter().enumerate() {
        if !run.curved {
            plan.push(PlannedRun {
                run: *run,
                radius: None,
                turn: 0.0,
                core: (run.start as f64 * h, run.end as f64 * h),
                cant: 0.0,
                ramp: 0.0,
                speed: run_speed(points, run),
            });
            continue;
        }
        let turn = measure_turn(&runs, index, &headings, &curvature, h);
        // Radius from length and turn angle — always determinable and self-consistent.
        let from_turn = if turn.abs() > 1e-9 {
            run.len(h) / turn.abs()
        } else {
            options.straight_radius
        };
        // The circle fit is more accurate as long as it matches the section. If the arc
        // length R·Δθ deviates by more than a quarter from the measured section length,
        // radius and turn angle do not fit together — the line would then be shorter or
        // longer by that amount when built. In that case the consistent value applies.
        // A section consists of a curve and two transition curves, so its length is
        // larger than R·Δθ — but not arbitrarily: if the curve lies outside this range,
        // radius and turn angle do not fit together (for instance with steadily
        // increasing curvature), and the self-consistent value wins.
        let measured = match fit_radius(points, run) {
            Some(fitted)
                if (0.4 * run.len(h)..1.25 * run.len(h)).contains(&(fitted * turn.abs())) =>
            {
                fitted
            }
            _ => from_turn,
        };
        let radius = if options.snap_radii {
            snap(measured, &options.preferred_radii, options.snap_tolerance)
        } else {
            measured
        };
        let speed = run_speed(points, run);

        // Cant and transition length come from the rulebook, not from the data: the
        // length of a transition curve cannot be recovered from noisy support points (the
        // section boundary is uncertain by more than a hundred metres), whereas the
        // standard ramp for the applied cant is uniquely determined. Position, radius and
        // turn angle of the curve come from the data.
        // Signed like the roll it produces: positive cant tips the track left
        // (`TrackEdge::eval`), so a right-hand curve carries the minus.
        let cant = options.cant.applied(radius, speed) * turn.signum();
        let ramp = options.cant.ramp_length(cant, speed).min(run.len(h) * 0.45);
        plan.push(PlannedRun {
            run: *run,
            radius: Some(radius * turn.signum()),
            turn,
            core: arc_core(&curvature, run, 1.0 / radius, h),
            cant,
            ramp,
            speed,
        });
    }

    build(&plan, points, &headings, options)
}

/// A section with the design values already determined.
#[derive(Debug, Clone, Copy)]
struct PlannedRun {
    run: Run,
    /// Signed radius [m]; `None` = straight.
    radius: Option<f64>,
    /// Measured change of direction [rad].
    turn: f64,
    /// Start and end of the curve core as arc length from the start of the line [m] —
    /// there the curvature reaches at least half of the curve curvature.
    core: (f64, f64),
    cant: f64,
    ramp: f64,
    speed: f64,
}

/// Core range of a curve: there the curvature reaches at least half of the curve
/// curvature.
///
/// Unlike the section boundaries, its boundaries lie in the middle of the respective
/// transition curves and are insensitive to the smearing of the estimate, because that
/// acts symmetrically. They are thus the most reliable clue to the position of the curve.
fn arc_core(curvature: &[f64], run: &Run, k_arc: f64, step: f64) -> (f64, f64) {
    let threshold = k_arc.abs() * 0.5;
    let core: Vec<usize> = (run.start..=run.end.min(curvature.len() - 1))
        .filter(|i| curvature[*i].abs() >= threshold)
        .collect();
    match (core.first(), core.last()) {
        (Some(a), Some(b)) => (*a as f64 * step, *b as f64 * step),
        _ => (run.start as f64 * step, run.end as f64 * step),
    }
}

/// Change of direction of a curve [rad].
///
/// Measured between the **midpoints of the neighbouring straights**, not at the section
/// boundaries: there the estimation window already contains curvature, which makes the
/// angle systematically too small — and a turn angle that is one percent too small pushes
/// the alignment metres sideways behind the curve.
fn measure_turn(runs: &[Run], index: usize, headings: &[f64], curvature: &[f64], step: f64) -> f64 {
    let mid = |run: &Run| (run.start + run.end) / 2;
    let previous = index
        .checked_sub(1)
        .and_then(|i| runs.get(i))
        .filter(|r| !r.curved);
    let next = runs.get(index + 1).filter(|r| !r.curved);

    if let (Some(previous), Some(next)) = (previous, next) {
        return headings[mid(next)] - headings[mid(previous)];
    }
    // At the edge of the data one of the two straights is missing. The heading at the
    // outermost support point is no good there — its estimation window is shifted inwards
    // and underestimates the turn. Instead the curvature is integrated over the section.
    let run = &runs[index];
    curvature[run.start..=run.end.min(curvature.len() - 1)]
        .iter()
        .sum::<f64>()
        * step
}

/// Split the point sequence into straight and curved sections.
///
/// Classification is done per support point (right-hand curve / straight / left-hand
/// curve); afterwards runs that are too short are swallowed by their neighbours. Without
/// this step a curve falls apart into dozens of shreds on noisy data, and the circle fit
/// gets too few points.
fn segment(curvature: &[f64], options: &AlignmentOptions) -> Vec<Run> {
    let threshold = 1.0 / options.straight_radius;
    let min_points = (options.min_element / options.sample).ceil().max(2.0) as usize;

    let mut class: Vec<i8> = curvature
        .iter()
        .map(|k| {
            if k.abs() <= threshold {
                0
            } else {
                k.signum() as i8
            }
        })
        .collect();
    // Several passes: after swallowing, new short runs can appear.
    for _ in 0..4 {
        if !despeckle(&mut class, min_points) {
            break;
        }
    }

    let mut runs = Vec::new();
    let mut start = 0usize;
    for i in 1..class.len() {
        if class[i] != class[start] {
            runs.push(Run {
                start,
                end: i,
                curved: class[start] != 0,
            });
            start = i;
        }
    }
    runs.push(Run {
        start,
        end: class.len() - 1,
        curved: class[start] != 0,
    });
    runs
}

/// Merge runs below the minimum length into the longer neighbour.
/// Returns whether anything was changed.
fn despeckle(class: &mut [i8], min_points: usize) -> bool {
    let n = class.len();
    let mut bounds: Vec<(usize, usize)> = Vec::new();
    let mut start = 0usize;
    for i in 1..n {
        if class[i] != class[start] {
            bounds.push((start, i));
            start = i;
        }
    }
    bounds.push((start, n));

    let mut changed = false;
    for (index, &(from, to)) in bounds.iter().enumerate() {
        if to - from >= min_points {
            continue;
        }
        let previous = index.checked_sub(1).map(|i| bounds[i]);
        let next = bounds.get(index + 1).copied();
        let winner = match (previous, next) {
            (Some(p), Some(nx)) => {
                if p.1 - p.0 >= nx.1 - nx.0 {
                    class[p.0]
                } else {
                    class[nx.0]
                }
            }
            (Some(p), None) => class[p.0],
            (None, Some(nx)) => class[nx.0],
            (None, None) => continue,
        };
        if winner != class[from] {
            for c in &mut class[from..to] {
                *c = winner;
            }
            changed = true;
        }
    }
    changed
}

/// Permitted speed of a section (the smallest one within it).
fn run_speed(points: &[SamplePoint], run: &Run) -> f64 {
    points[run.start..=run.end.min(points.len() - 1)]
        .iter()
        .map(|p| p.speed)
        .fold(f64::INFINITY, f64::min)
}

/// Circle fit after Kåsa over the core of the curve.
///
/// The outer 20 % are left out — that is where the transition curves lie, whose curvature
/// does not yet match the circle.
fn fit_radius(points: &[SamplePoint], run: &Run) -> Option<f64> {
    let count = run.end - run.start;
    if count < 4 {
        return None;
    }
    let margin = count / 5;
    let slice = &points[run.start + margin..=(run.end - margin).min(points.len() - 1)];
    if slice.len() < 4 {
        return None;
    }

    // Fit of x² + y² + D·x + E·y + F = 0 in the centroid system.
    let mean = slice.iter().fold(DVec2::ZERO, |a, p| a + p.pos) / slice.len() as f64;
    let (mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0);
    let (mut sxz, mut syz) = (0.0, 0.0);
    for p in slice {
        let d = p.pos - mean;
        let z = d.x * d.x + d.y * d.y;
        sxx += d.x * d.x;
        syy += d.y * d.y;
        sxy += d.x * d.y;
        sxz += d.x * z;
        syz += d.y * z;
    }
    let det = sxx * syy - sxy * sxy;
    if det.abs() < 1e-9 {
        return None;
    }
    let cx = 0.5 * (sxz * syy - syz * sxy) / det;
    let cy = 0.5 * (syz * sxx - sxz * sxy) / det;
    let radius = (cx * cx
        + cy * cy
        + slice
            .iter()
            .map(|p| (p.pos - mean).length_squared())
            .sum::<f64>()
            / slice.len() as f64)
        .sqrt();
    radius.is_finite().then_some(radius)
}

/// Nearest standard radius.
fn snap(radius: f64, preferred: &[f64], tolerance: f64) -> f64 {
    preferred
        .iter()
        .copied()
        .min_by(|a, b| (a - radius).abs().total_cmp(&(b - radius).abs()))
        .filter(|nearest| (nearest - radius).abs() <= radius * tolerance)
        .unwrap_or(radius)
}

/// Builds the segment chain including the cant band from the planned sections.
fn build(
    plan: &[PlannedRun],
    points: &[SamplePoint],
    headings: &[f64],
    options: &AlignmentOptions,
) -> Alignment {
    let h = options.sample;
    let mut segments: Vec<Segment> = Vec::new();
    let mut elements: Vec<Element> = Vec::new();
    let mut cant_steps: Vec<(f64, f64)> = vec![(0.0, 0.0)];
    let mut s = 0.0;

    // Every curve is placed around its midpoint and keeps its turn angle
    // (L_arc = R·Δθ − L_ramp); the straights fill the gaps in between. The curve midpoint
    // is the only quantity that can be reliably determined from noisy data — the start
    // and end of a curve cannot.
    let total = plan.last().map_or(0.0, |p| p.run.end as f64 * h);
    let mut lengths: Vec<f64> = vec![0.0; plan.len()];
    let mut cursor = 0.0;
    for (i, planned) in plan.iter().enumerate() {
        let Some(radius) = planned.radius else {
            continue;
        };
        let arc =
            (radius.abs() * planned.turn.abs() - planned.ramp).max(options.min_element * 0.25);
        let built = 2.0 * planned.ramp + arc;

        // The anchor is the curve core, on the side where a straight adjoins: there the
        // position is determined best (the start of the core lies in the middle of the
        // transition curve, i.e. half a ramp behind its beginning). If the curve ends at
        // the edge of the data, the other side is anchored.
        let has_previous = i > 0 && plan[i - 1].radius.is_none();
        let has_next = i + 1 < plan.len() && plan[i + 1].radius.is_none();
        let start = match (has_previous, has_next) {
            (true, _) => planned.core.0 - planned.ramp / 2.0,
            (false, true) => planned.core.1 + planned.ramp / 2.0 - built,
            (false, false) => (planned.core.0 + planned.core.1) / 2.0 - built / 2.0,
        }
        .max(cursor);

        if i > 0 {
            lengths[i - 1] = start - cursor;
        }
        lengths[i] = built;
        cursor = start + built;
    }
    // The remainder behind the last curve belongs to the last straight. If there is none
    // (the data ends inside the curve), the chain stays correspondingly shorter —
    // inventing length would be worse than losing it.
    if let Some(last) = lengths.len().checked_sub(1)
        && plan[last].radius.is_none()
    {
        lengths[last] = (total - cursor).max(0.0);
    }

    for (index, planned) in plan.iter().enumerate() {
        let run_len = lengths[index].max(0.0);
        match planned.radius {
            None => {
                // The straights keep their measured length; the transition curves lie
                // entirely within the respective curve section (see L_ramp above).
                let length = run_len.max(options.min_element * 0.25);
                push_element(
                    &mut segments,
                    &mut elements,
                    &mut s,
                    ElementKind::Straight,
                    Segment::straight(length),
                    None,
                    0.0,
                    planned.speed,
                );
            }
            Some(radius) => {
                let k = 1.0 / radius;
                // Section length and turn angle are preserved:
                // L_section = 2·L_ramp + L_arc and Δθ = (L_arc + L_ramp)/R.
                let ramp = planned.ramp.min(run_len * 0.45);
                let arc_len = (run_len - 2.0 * ramp).max(options.min_element * 0.25);

                push_element(
                    &mut segments,
                    &mut elements,
                    &mut s,
                    ElementKind::Transition,
                    Segment::transition(ramp, 0.0, k),
                    Some(radius),
                    planned.cant,
                    planned.speed,
                );
                ramp_cant(&mut cant_steps, s - ramp, ramp, 0.0, planned.cant);

                push_element(
                    &mut segments,
                    &mut elements,
                    &mut s,
                    ElementKind::Arc,
                    Segment::arc(arc_len, radius),
                    Some(radius),
                    planned.cant,
                    planned.speed,
                );

                push_element(
                    &mut segments,
                    &mut elements,
                    &mut s,
                    ElementKind::Transition,
                    Segment::transition(ramp, k, 0.0),
                    Some(radius),
                    0.0,
                    planned.speed,
                );
                ramp_cant(&mut cant_steps, s - ramp, ramp, planned.cant, 0.0);
            }
        }
    }

    let start_heading = headings[0];
    let max_deviation = super::fit::deviation(&segments, start_heading, points);

    Alignment {
        segments,
        elements,
        cant: cant_steps,
        grade: super::fit::grade_profile(points, h),
        speed: super::fit::speed_profile(points, h),
        start_heading,
        max_deviation,
    }
}

#[allow(clippy::too_many_arguments)]
fn push_element(
    segments: &mut Vec<Segment>,
    elements: &mut Vec<Element>,
    s: &mut f64,
    kind: ElementKind,
    segment: Segment,
    radius: Option<f64>,
    cant: f64,
    speed: f64,
) {
    if segment.len <= 1e-6 {
        return;
    }
    elements.push(Element {
        kind,
        start_s: *s,
        length: segment.len,
        radius,
        cant,
        speed,
    });
    *s += segment.len;
    segments.push(segment);
}

/// Cant ramp as steps — `StepProfile` does not know interpolation. Also what
/// the route editor writes under the transition curves it lays.
///
/// ponytail: 10 m steps instead of a linear profile. The jump per step is a few
/// millimetres and is not noticeable in the roll motion; once `StepProfile` can
/// interpolate, this can go away.
pub fn ramp_cant(steps: &mut Vec<(f64, f64)>, start: f64, length: f64, from: f64, to: f64) {
    if length <= 0.0 {
        return;
    }
    let count = (length / 10.0).ceil().max(1.0) as usize;
    for i in 0..=count {
        let t = i as f64 / count as f64;
        steps.push((start + length * t, from + (to - from) * t));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a track from design elements and samples it —
    /// the counterpart to what the fitter is supposed to reconstruct.
    fn design_track(radius: f64, transition: f64, arc: f64, noise: f64) -> Vec<SamplePoint> {
        let step = 20.0;
        let mut pts = Vec::new();
        let mut pos = DVec2::ZERO;
        let mut heading = 0.0f64;
        let mut seed = 12345u64;
        let mut rand = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) as f64 / (1u64 << 31) as f64) - 1.0
        };

        let plan: Vec<(f64, f64, f64)> = vec![
            // (length, k_start, k_end)
            (600.0, 0.0, 0.0),
            (transition, 0.0, 1.0 / radius),
            (arc, 1.0 / radius, 1.0 / radius),
            (transition, 1.0 / radius, 0.0),
            (600.0, 0.0, 0.0),
        ];
        for (len, k0, k1) in plan {
            let steps = (len / step).round() as usize;
            for i in 0..steps {
                let t = i as f64 / steps as f64;
                let k = k0 + (k1 - k0) * t;
                heading += k * step;
                pos += DVec2::new(heading.cos(), heading.sin()) * step;
                pts.push(SamplePoint {
                    pos: pos + DVec2::new(rand(), rand()) * noise,
                    height: 0.0,
                    speed: 160.0,
                });
            }
        }
        pts
    }

    #[test]
    fn cant_follows_the_formula() {
        let rules = CantRules::default();
        // 160 km/h at R = 2000 m: equilibrium cant 11.8·160²/2000 = 151 mm.
        let eq = CantRules::equilibrium(2000.0, 160.0);
        assert!((eq - 151.0).abs() < 1.0, "{eq}");
        // It is applied minus the permitted deficiency, rounded to 5 mm.
        assert_eq!(rules.applied(2000.0, 160.0), 90.0);
        // Tight curves run into the upper limit.
        assert_eq!(rules.applied(300.0, 100.0), rules.max_cant);
        // No cant on the straight.
        assert_eq!(rules.applied(50_000.0, 160.0), 0.0);
        // Ramp: 90 mm at 160 km/h → 1:1600, i.e. 144 m.
        assert!((rules.ramp_length(90.0, 160.0) - 144.0).abs() < 1.0);
    }

    #[test]
    fn design_elements_are_recovered() {
        // Source conforming to the rulebook: the transition length equals the ramp that
        // belongs to the cant of this curve (R = 1200 m at 160 km/h → 160 mm → 256 m).
        let rules = CantRules::default();
        let transition = rules.ramp_length(rules.applied(1200.0, 160.0), 160.0);
        let points = design_track(1200.0, transition, 400.0, 0.0);
        let alignment = fit(&points, &AlignmentOptions::default());

        // Expected: straight – transition – curve – transition – straight.
        let kinds: Vec<ElementKind> = alignment.elements.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec![
                ElementKind::Straight,
                ElementKind::Transition,
                ElementKind::Arc,
                ElementKind::Transition,
                ElementKind::Straight,
            ],
            "{:?}",
            alignment.elements
        );
        assert_eq!(alignment.arcs(), 1);

        // Radius hits the standard value.
        let arc = alignment
            .elements
            .iter()
            .find(|e| e.kind == ElementKind::Arc)
            .unwrap();
        assert_eq!(arc.radius.unwrap().abs(), 1200.0);
        // A few metres remain — the same order of magnitude as the source data itself
        // (OSM from aerial imagery: ±2…5 m). Getting more accurate would have no
        // information value here any more.
        assert!(
            alignment.max_deviation < 6.0,
            "reconstruction error {:.1} m",
            alignment.max_deviation
        );

        // The element lengths match the design.
        let built_transition = alignment.elements[1].length;
        assert!(
            (built_transition - transition).abs() < 30.0,
            "transition curve {built_transition:.0} m instead of {transition:.0} m"
        );
    }

    #[test]
    fn deviating_transition_curves_stay_within_bounds() {
        // Source with a shorter transition curve than the rulebook prescribes for the
        // cant. The reconstruction is conforming to the rulebook nevertheless — the
        // alignment thereby deviates visibly, but in a bounded way. From noisy points the
        // actual transition length cannot be recovered; the section boundary is uncertain
        // by more than a hundred metres.
        let points = design_track(1200.0, 120.0, 400.0, 0.0);
        let alignment = fit(&points, &AlignmentOptions::default());
        assert_eq!(alignment.arcs(), 1);
        assert!(
            alignment.max_deviation < 12.0,
            "deviation {:.1} m",
            alignment.max_deviation
        );
    }

    #[test]
    fn radius_survives_noisy_points() {
        // ±2 m noise — the order of magnitude of OSM from aerial imagery.
        let points = design_track(800.0, 100.0, 500.0, 2.0);
        let alignment = fit(&points, &AlignmentOptions::default());
        let arc = alignment
            .elements
            .iter()
            .find(|e| e.kind == ElementKind::Arc)
            .expect("curve detected");
        let radius = arc.radius.unwrap().abs();
        assert!(
            (radius - 800.0).abs() <= 100.0,
            "radius {radius} instead of 800 m"
        );
    }

    /// The mirror image: a right-hand curve gets its cant as a negative
    /// number, so `TrackEdge::eval` rolls the track toward the inside.
    #[test]
    fn a_right_hand_curve_gets_negative_cant() {
        let points = design_track(-1200.0, 120.0, 400.0, 0.0);
        let alignment = fit(&points, &AlignmentOptions::default());
        let low = alignment.cant.iter().map(|(_, c)| *c).fold(0.0, f64::min);
        assert!(low < -40.0, "right-hand cant must be negative: {low}");
        let high = alignment.cant.iter().map(|(_, c)| *c).fold(0.0, f64::max);
        assert!(high <= 1e-9, "no positive cant on a right-hand curve: {high}");
    }

    #[test]
    fn cant_ends_up_in_the_band() {
        let points = design_track(1200.0, 120.0, 400.0, 0.0);
        let alignment = fit(&points, &AlignmentOptions::default());

        let rules = CantRules::default();
        let max_cant = alignment.cant.iter().map(|(_, c)| *c).fold(0.0, f64::max);

        // The smaller of two values is applied: what the rulebook prescribes for radius
        // and speed, and what the available ramp allows.
        assert!(
            max_cant <= rules.applied(1200.0, 160.0),
            "above the standard value: {max_cant}"
        );
        assert!(max_cant > 40.0, "cant should be noticeable: {max_cant}");
        let ramp = alignment
            .elements
            .iter()
            .find(|e| e.kind == ElementKind::Transition)
            .unwrap()
            .length;
        assert!(
            ramp >= rules.ramp_length(max_cant, 160.0) - 1.0,
            "ramp {ramp:.0} m too short for {max_cant} mm"
        );

        // Start and end are free of cant.
        assert_eq!(alignment.cant[0], (0.0, 0.0));
        assert_eq!(alignment.cant.last().unwrap().1, 0.0);

        // The ramp rises monotonically up to the curve and falls again afterwards.
        let peak = alignment
            .cant
            .iter()
            .position(|(_, c)| *c >= max_cant)
            .unwrap();
        assert!(
            alignment.cant[..=peak].windows(2).all(|w| w[1].1 >= w[0].1),
            "ramp does not rise monotonically"
        );
    }

    #[test]
    fn straight_stays_a_single_element() {
        let points: Vec<SamplePoint> = (0..60)
            .map(|i| SamplePoint {
                pos: DVec2::new(i as f64 * 20.0, 0.0),
                height: 0.0,
                speed: 120.0,
            })
            .collect();
        let alignment = fit(&points, &AlignmentOptions::default());
        assert_eq!(alignment.elements.len(), 1);
        assert_eq!(alignment.elements[0].kind, ElementKind::Straight);
        assert!(alignment.cant.iter().all(|(_, c)| *c == 0.0));
    }

    #[test]
    fn reverse_curves_are_separated() {
        // S-curve: first left, then right.
        let step = 20.0;
        let mut pos = DVec2::ZERO;
        let mut heading = 0.0f64;
        let mut pts = Vec::new();
        for i in 0..160 {
            let k = match i {
                0..=30 => 0.0,
                31..=70 => 1.0 / 1000.0,
                71..=110 => -1.0 / 1000.0,
                _ => 0.0,
            };
            heading += k * step;
            pos += DVec2::new(heading.cos(), heading.sin()) * step;
            pts.push(SamplePoint {
                pos,
                height: 0.0,
                speed: 120.0,
            });
        }
        let alignment = fit(&pts, &AlignmentOptions::default());
        assert_eq!(alignment.arcs(), 2, "{:?}", alignment.elements);
        let radii: Vec<f64> = alignment
            .elements
            .iter()
            .filter(|e| e.kind == ElementKind::Arc)
            .map(|e| e.radius.unwrap())
            .collect();
        assert!(
            radii[0].signum() != radii[1].signum(),
            "reverse curves must be in opposite directions: {radii:?}"
        );
    }

    #[test]
    fn without_snapping_the_measured_radius_remains() {
        let points = design_track(1150.0, 120.0, 400.0, 0.0);
        let options = AlignmentOptions {
            snap_radii: false,
            ..Default::default()
        };
        let alignment = fit(&points, &options);
        let arc = alignment
            .elements
            .iter()
            .find(|e| e.kind == ElementKind::Arc)
            .unwrap();
        let radius = arc.radius.unwrap().abs();
        assert!((radius - 1150.0).abs() < 60.0, "{radius}");
        assert_ne!(radius, 1200.0, "no standard value without snapping");
    }
}
