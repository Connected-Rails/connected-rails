//! Frame profiler: what costs frame budget and what causes lags (plan 16.3).
//!
//! Bevy's own diagnostics say how long a frame took, but not where it went. This
//! module closes that gap with per-system CPU timings on the main thread: every
//! expensive driving system records its time into [`Profiler`], which keeps a
//! rolling history, per-span averages, the worst spikes with their breakdown,
//! and an ASCII history graph for the F6 overlay. The console's `prof` command
//! prints the same numbers and exports the history as CSV.
//!
//! What it does not do on purpose: GPU timings (those need timestamp queries on
//! Vulkan/DX12 — Bevy's `RenderDiagnosticsPlugin` — or an external viewer via
//! `trace_tracy`; see README). Whatever the CPU spans do not account for shows
//! as `rest` and is render, present and vsync. A high `rest` next to high
//! triangle/entity counts points at the GPU, a high named span at the CPU side.
//!
//! **Multiplayer.** The profiler measures the local machine only — frame times
//! are nothing the simulation owns and nothing any peer could use. Like the
//! camera it is client-owned state (`CLAUDE.md` ch. 20): no setpoint, no
//! message, nothing replicated.

use bevy::prelude::*;
use std::collections::VecDeque;
use std::time::Instant;

/// Frames of history kept — five seconds at 60 fps, enough to catch a hitch
/// after it happened and still have the context around it.
pub const HISTORY: usize = 300;
/// A frame slower than this counts as a hitch — one and a half frames at 60 Hz,
/// where a single dropped vsync stops being noise.
pub const HITCH_MS: f64 = 25.0;
/// Worst spikes remembered with their breakdown.
pub const SPIKES: usize = 5;
/// Full scale of the history graph [ms] — everything slower clamps to `@`.
pub const GRAPH_MAX_MS: f64 = 50.0;
/// Columns of the history graph. 56 columns of 11 px mono fit the F6 panel.
pub const GRAPH_WIDTH: usize = 56;

/// One finished frame: its total wall time, how many fixed sim steps it ran,
/// and what the instrumented systems cost inside it [ms].
#[derive(Debug, Clone)]
pub struct FrameSample {
    pub frame: u64,
    pub total_ms: f64,
    pub sim_steps: usize,
    pub spans: Vec<(&'static str, f64)>,
}

impl FrameSample {
    fn span(&self, name: &str) -> f64 {
        self.spans
            .iter()
            .find(|(n, _)| *n == name)
            .map_or(0.0, |(_, ms)| *ms)
    }
}

/// Frame totals over the history: what the budget looks like.
#[derive(Debug, Clone, Copy, Default)]
pub struct FrameStats {
    pub count: usize,
    pub avg: f64,
    pub p95: f64,
    pub max: f64,
    pub hitches: usize,
}

/// One span aggregated over the history, sorted by `avg` where it is read.
#[derive(Debug, Clone, Copy)]
pub struct SpanStat {
    pub name: &'static str,
    pub avg: f64,
    pub max: f64,
}

/// The profiler itself. A resource from `main`, filled by the driving systems,
/// read by the F6 overlay, the console and the `--frames` log.
#[derive(Resource, Debug, Default)]
pub struct Profiler {
    frames: VecDeque<FrameSample>,
    current: Vec<(&'static str, f64)>,
    current_steps: usize,
    frame: u64,
    paused: bool,
    worst: Vec<FrameSample>,
}

impl Profiler {
    /// Adds `ms` to the current frame's span, accumulating where a span is
    /// recorded twice in one frame.
    pub fn record(&mut self, name: &'static str, ms: f64) {
        if let Some(slot) = self.current.iter_mut().find(|(n, _)| *n == name) {
            slot.1 += ms;
        } else {
            self.current.push((name, ms));
        }
    }

    /// How many fixed sim steps the last `advance` ran — the number that says
    /// whether a slow `sim` span is one expensive step or catch-up after a hitch.
    pub fn set_sim_steps(&mut self, steps: usize) {
        self.current_steps = steps;
    }

    /// Times one system. The guard records on drop, so early returns are timed too:
    /// `let _scope = profiler.scope("sim");` as the first line of the system.
    pub fn scope(&mut self, name: &'static str) -> Scope<'_> {
        Scope {
            profiler: self,
            name,
            start: Instant::now(),
        }
    }

    /// Whether new frames are recorded. While paused the overlay and the console
    /// keep showing the frozen history.
    pub fn paused(&self) -> bool {
        self.paused
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// Forgets every frame and spike sampled so far.
    pub fn reset(&mut self) {
        self.frames.clear();
        self.current.clear();
        self.current_steps = 0;
        self.worst.clear();
    }

    /// Frames sampled, oldest first.
    pub fn frames(&self) -> &VecDeque<FrameSample> {
        &self.frames
    }

    /// Worst spikes, slowest first.
    pub fn spikes(&self) -> &[FrameSample] {
        &self.worst
    }

    /// Frame totals over the history.
    pub fn frame_stats(&self) -> FrameStats {
        let mut totals: Vec<f64> = self.frames.iter().map(|f| f.total_ms).collect();
        if totals.is_empty() {
            return FrameStats::default();
        }
        totals.sort_by(|a, b| a.total_cmp(b));
        let count = totals.len();
        let avg = totals.iter().sum::<f64>() / count as f64;
        let p95 = totals[(count * 95 / 100).min(count - 1)];
        FrameStats {
            count,
            avg,
            p95,
            max: totals[count - 1],
            hitches: totals.iter().filter(|ms| **ms >= HITCH_MS).count(),
        }
    }

    /// Every span seen in the history, slowest average first. A span missing
    /// from a frame counts as zero there — an average over present frames only
    /// would hide how rarely it runs.
    pub fn span_stats(&self) -> Vec<SpanStat> {
        if self.frames.is_empty() {
            return Vec::new();
        }
        let mut names: Vec<&'static str> = Vec::new();
        for frame in &self.frames {
            for (name, _) in &frame.spans {
                if !names.contains(name) {
                    names.push(name);
                }
            }
        }
        let count = self.frames.len() as f64;
        let mut stats: Vec<SpanStat> = names
            .into_iter()
            .map(|name| {
                let mut sum: f64 = 0.0;
                let mut max: f64 = 0.0;
                for frame in &self.frames {
                    let ms = frame.span(name);
                    sum += ms;
                    max = max.max(ms);
                }
                SpanStat {
                    name,
                    avg: sum / count,
                    max,
                }
            })
            .collect();
        stats.sort_by(|a, b| b.avg.total_cmp(&a.avg));
        stats
    }

    /// What the spans do not account for [ms]: render, present and vsync, plus
    /// the systems too small to instrument. Negative by an epsilon where the
    /// clocks disagree — clamped to zero, which is what it means.
    pub fn rest_ms(&self) -> f64 {
        let stats = self.frame_stats();
        let accounted: f64 = self.span_stats().iter().map(|s| s.avg).sum();
        (stats.avg - accounted).max(0.0)
    }

    /// The history as one line of ` .:-=+*#@`, oldest to newest, bucketed down
    /// to [`GRAPH_WIDTH`] columns by maximum. Pure ASCII: it has to survive the
    /// HUD font on every platform.
    pub fn graph(&self) -> String {
        const LEVELS: &[u8] = b" .:-=+*#@";
        if self.frames.is_empty() {
            return String::new();
        }
        let bucket = self.frames.len().div_ceil(GRAPH_WIDTH);
        self.frames
            .iter()
            .collect::<Vec<_>>()
            .chunks(bucket.max(1))
            .map(|chunk| {
                let peak = chunk.iter().map(|f| f.total_ms).fold(0.0, f64::max);
                let level = ((peak / GRAPH_MAX_MS * (LEVELS.len() - 1) as f64) as usize)
                    .min(LEVELS.len() - 1);
                LEVELS[level] as char
            })
            .collect()
    }

    /// The slowest spans as `name avg …`, for log lines (the F6 overlay and the
    /// console word the same numbers through i18n instead).
    pub fn top_spans(&self, n: usize) -> String {
        self.span_stats()
            .into_iter()
            .take(n)
            .map(|span| format!("{} {:.2}", span.name, span.avg))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The history as CSV: one row per frame, one column per span seen. Spans a
    /// frame did not record stay empty, so a missing column reads as missing
    /// rather than as free.
    pub fn csv(&self) -> String {
        let mut names: Vec<&'static str> = Vec::new();
        for frame in &self.frames {
            for (name, _) in &frame.spans {
                if !names.contains(name) {
                    names.push(name);
                }
            }
        }
        names.sort_unstable();
        let mut out = String::from("frame,total_ms,sim_steps");
        for name in &names {
            out.push(',');
            out.push_str(name);
        }
        out.push('\n');
        for frame in &self.frames {
            out.push_str(&format!(
                "{},{:.3},{}",
                frame.frame, frame.total_ms, frame.sim_steps
            ));
            for name in &names {
                match frame.spans.iter().find(|(n, _)| n == name) {
                    Some((_, ms)) => out.push_str(&format!(",{ms:.3}")),
                    None => out.push(','),
                }
            }
            out.push('\n');
        }
        out
    }

    pub(crate) fn begin(&mut self) {
        self.current.clear();
        self.current_steps = 0;
    }

    pub(crate) fn end(&mut self, total_ms: f64) {
        if self.paused {
            self.current.clear();
            return;
        }
        self.frame += 1;
        let sample = FrameSample {
            frame: self.frame,
            total_ms,
            sim_steps: self.current_steps,
            spans: std::mem::take(&mut self.current),
        };
        self.worst.push(sample.clone());
        self.worst.sort_by(|a, b| b.total_ms.total_cmp(&a.total_ms));
        self.worst.truncate(SPIKES);
        self.frames.push_back(sample);
        while self.frames.len() > HISTORY {
            self.frames.pop_front();
        }
    }
}

/// The guard [`Profiler::scope`] hands out. Records on drop, so the timed
/// system needs no bookkeeping at its exits.
pub struct Scope<'a> {
    profiler: &'a mut Profiler,
    name: &'static str,
    start: Instant,
}

impl Drop for Scope<'_> {
    fn drop(&mut self) {
        self.profiler
            .record(self.name, self.start.elapsed().as_secs_f64() * 1000.0);
    }
}

/// Opens the current frame. `PreUpdate` while driving, before every timed system.
pub fn begin_frame(mut profiler: ResMut<Profiler>) {
    profiler.begin();
}

/// Closes the current frame. `PostUpdate` while driving, after every timed
/// system: the wall time between frames is the budget, render and vsync included.
pub fn end_frame(time: Res<Time>, mut profiler: ResMut<Profiler>) {
    profiler.end(time.delta_secs_f64() * 1000.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profiler_with(totals: &[f64]) -> Profiler {
        let mut profiler = Profiler::default();
        for (i, total) in totals.iter().enumerate() {
            profiler.record("sim", *total * 0.6);
            profiler.record("rest-span", *total * 0.2);
            profiler.set_sim_steps(i % 3);
            profiler.end(*total);
            profiler.begin();
        }
        profiler
    }

    #[test]
    fn a_span_recorded_twice_accumulates() {
        let mut profiler = Profiler::default();
        profiler.record("sim", 1.5);
        profiler.record("sim", 2.5);
        profiler.end(10.0);
        assert_eq!(profiler.frames().back().unwrap().span("sim"), 4.0);
    }

    #[test]
    fn totals_average_and_percentile_over_the_history() {
        let profiler = profiler_with(&[10.0, 20.0, 30.0, 40.0]);
        let stats = profiler.frame_stats();
        assert_eq!(stats.count, 4);
        assert_eq!(stats.avg, 25.0);
        assert_eq!(stats.max, 40.0);
        // p95 of four samples is the slowest one.
        assert_eq!(stats.p95, 40.0);
        assert_eq!(stats.hitches, 2, "30 and 40 ms clear 25 ms");
    }

    #[test]
    fn an_empty_profiler_reports_zeroes() {
        let profiler = Profiler::default();
        let stats = profiler.frame_stats();
        assert_eq!(stats.count, 0);
        assert_eq!(stats.avg, 0.0);
        assert!(profiler.span_stats().is_empty());
        assert!(profiler.spikes().is_empty());
        assert_eq!(profiler.graph(), "");
        assert_eq!(profiler.csv(), "frame,total_ms,sim_steps\n");
    }

    #[test]
    fn history_is_bounded_and_spikes_keep_the_worst() {
        let mut profiler = Profiler::default();
        for i in 0..HISTORY + 50 {
            profiler.record("sim", 1.0);
            profiler.end(if i == HISTORY + 10 { 100.0 } else { 5.0 });
            profiler.begin();
        }
        assert_eq!(profiler.frames().len(), HISTORY);
        assert_eq!(profiler.spikes().len(), SPIKES.min(HISTORY + 50));
        assert_eq!(profiler.spikes()[0].total_ms, 100.0);
    }

    #[test]
    fn pausing_freezes_the_history() {
        let mut profiler = Profiler::default();
        profiler.record("sim", 2.0);
        profiler.end(16.0);
        profiler.set_paused(true);
        profiler.begin();
        profiler.record("sim", 9.0);
        profiler.end(60.0);
        assert_eq!(profiler.frames().len(), 1);
        assert!(profiler.paused());
        profiler.set_paused(false);
        profiler.begin();
        profiler.end(16.0);
        assert_eq!(profiler.frames().len(), 2);
    }

    #[test]
    fn spans_sort_by_average_and_missing_counts_as_zero() {
        let mut profiler = Profiler::default();
        profiler.record("often", 4.0);
        profiler.record("rare", 10.0);
        profiler.end(20.0);
        profiler.begin();
        profiler.record("often", 4.0);
        profiler.end(20.0);
        profiler.begin();
        let stats = profiler.span_stats();
        assert_eq!(stats[0].name, "rare", "10/2 beats 8/2");
        assert_eq!(stats[0].avg, 5.0);
        assert_eq!(stats[1].avg, 4.0);
        assert_eq!(stats[1].max, 4.0);
    }

    #[test]
    fn the_scope_guard_times_the_enclosing_block() {
        let mut profiler = Profiler::default();
        {
            let _scope = profiler.scope("work");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        profiler.end(16.0);
        let took = profiler.frames().back().unwrap().span("work");
        assert!(took >= 4.0, "the guard records wall time, got {took:.1} ms");
    }

    #[test]
    fn the_graph_buckets_the_history_into_columns() {
        // 112 frames at bucket 2 land exactly on the column count.
        let totals: Vec<f64> = (0..112).map(|i| i as f64).collect();
        let profiler = profiler_with(&totals);
        let graph = profiler.graph();
        assert_eq!(graph.len(), GRAPH_WIDTH);
        assert!(
            graph.ends_with('@'),
            "the newest bucket peaks far past {GRAPH_MAX_MS} ms"
        );
        assert!(graph.starts_with(' '), "the oldest bucket is near zero");
        assert!(
            graph.bytes().all(|b| b" .:-=+*#@".contains(&b)),
            "ASCII only, for the HUD font"
        );
    }

    #[test]
    fn top_spans_lists_the_slowest_averages_first() {
        let profiler = profiler_with(&[10.0, 20.0]);
        assert_eq!(profiler.top_spans(1), "sim 9.00");
        assert_eq!(profiler.top_spans(5), "sim 9.00 rest-span 3.00");
    }

    #[test]
    fn csv_lists_every_span_seen() {
        let mut profiler = Profiler::default();
        profiler.record("sim", 2.5);
        profiler.end(16.25);
        profiler.begin();
        profiler.record("stream", 6.0);
        profiler.end(20.0);
        profiler.begin();
        let csv = profiler.csv();
        let mut lines = csv.lines();
        assert_eq!(lines.next().unwrap(), "frame,total_ms,sim_steps,sim,stream");
        let first = lines.next().unwrap();
        assert!(
            first.starts_with("1,16.250,0,2.500,"),
            "missing spans stay empty, got {first:?}"
        );
        assert!(lines.next().unwrap().ends_with(",6.000"));
    }
}
