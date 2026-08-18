# -*- coding: utf-8 -*-
"""One-shot: the area tool becomes a brush that paints a wide stroke over the track."""

import re

TOOLS = "crates/route-editor/src/tools.rs"
MAIN = "crates/route-editor/src/main.rs"


def replace(path, pairs):
    s = open(path, encoding="utf-8").read()
    for old, new in pairs:
        assert old in s, (path, old[:80])
        s = s.replace(old, new, 1)
    open(path, "w", encoding="utf-8").write(s)


# --- 1. Split nearest_on_network so one edge can be probed on its own ---------
replace(
    TOOLS,
    [
        (
            """pub fn nearest_on_network(net: &TrackNetwork, p: EcefPos) -> Option<(usize, f64, f64)> {
    let mut best: Option<(usize, f64, f64)> = None;
    for (i, edge) in net.edges().iter().enumerate() {
        let length = edge.length();
        let mut step = 10.0_f64.min(length.max(0.01));
        let mut s_best = 0.0;
        let mut d_best = f64::MAX;
        let probe = |s: f64, d_best: &mut f64, s_best: &mut f64| {
            let d = edge.eval(s).pos.distance(p);
            if d < *d_best {
                *d_best = d;
                *s_best = s;
            }
        };
        let coarse = (length / step).ceil() as usize;
        for j in 0..=coarse {
            probe((j as f64 * step).min(length), &mut d_best, &mut s_best);
        }
        for _ in 0..2 {
            let fine = step / 10.0;
            let mut s = (s_best - step).max(0.0);
            let hi = (s_best + step).min(length);
            while s <= hi {
                probe(s, &mut d_best, &mut s_best);
                s += fine;
            }
            step = fine;
        }
        if best.is_none_or(|(_, _, d)| d_best < d) {
            best = Some((i, s_best, d_best));
        }
    }
    best
}""",
            """pub fn nearest_on_network(net: &TrackNetwork, p: EcefPos) -> Option<(usize, f64, f64)> {
    let mut best: Option<(usize, f64, f64)> = None;
    for (i, edge) in net.edges().iter().enumerate() {
        let (s, d) = nearest_on_edge(edge, p);
        if best.is_none_or(|(_, _, best)| d < best) {
            best = Some((i, s, d));
        }
    }
    best
}

/// The arc length of one edge nearest `p`, and how far away it is [m].
///
/// Coarse scan then two refinements — the same probe `nearest_on_network` uses, pulled
/// out so a brush that has hold of one track can keep asking that track alone.
pub fn nearest_on_edge(edge: &track_model::TrackEdge, p: EcefPos) -> (f64, f64) {
    let length = edge.length();
    let mut step = 10.0_f64.min(length.max(0.01));
    let mut s_best = 0.0;
    let mut d_best = f64::MAX;
    let probe = |s: f64, d_best: &mut f64, s_best: &mut f64| {
        let d = edge.eval(s).pos.distance(p);
        if d < *d_best {
            *d_best = d;
            *s_best = s;
        }
    };
    let coarse = (length / step).ceil() as usize;
    for j in 0..=coarse {
        probe((j as f64 * step).min(length), &mut d_best, &mut s_best);
    }
    for _ in 0..2 {
        let fine = step / 10.0;
        let mut s = (s_best - step).max(0.0);
        let hi = (s_best + step).min(length);
        while s <= hi {
            probe(s, &mut d_best, &mut s_best);
            s += fine;
        }
        step = fine;
    }
    (s_best, d_best)
}""",
        )
    ],
)

# --- 2. The stroke in the editor state ---------------------------------------
replace(
    TOOLS,
    [
        (
            """    /// First end of the stretch the area tool is marking: `(edge, s)`. Set by the first
    /// click, consumed by the second — a marking in progress, not saved state.
    pub area_start: Option<(usize, f64)>,""",
            """    /// The stroke the area brush is painting right now. Held while the button is down and
    /// committed on release — a marking in progress, not saved state.
    pub area_stroke: Option<AreaStroke>,
    /// Half-width of the area brush stroke [m]; `None` = 2.5, a good deal wider than the
    /// track it is painted over so it reads as laid on top of it.
    pub area_width: Option<f64>,""",
        ),
        (
            """/// One item the marking brush swept over.""",
            """/// The stroke the area brush is painting: one stretch of one track, growing under the
/// cursor. It stays on the track it started on — a brush that jumped to the neighbouring
/// track halfway through a station would paint the wrong one.
#[derive(Clone, Copy, PartialEq)]
pub struct AreaStroke {
    pub edge: usize,
    pub from: f64,
    pub to: f64,
}

impl AreaStroke {
    pub fn span(self) -> content::route::AreaSpan {
        content::route::AreaSpan::new(self.edge as u32, self.from, self.to)
    }

    pub fn length(self) -> f64 {
        (self.to - self.from).abs()
    }
}

/// One item the marking brush swept over.""",
        ),
    ],
)

# --- 3. The brush itself ------------------------------------------------------
OLD_MARK = re.compile(
    r"/// One end of an area marking has been clicked.*?\n\}\n", re.S
)
s = open(TOOLS, encoding="utf-8").read()
new_mark = '''/// Commits the painted stroke: onto the selected area, or into a new one.
fn commit_stroke(line: &mut Line, state: &mut EditorState, stroke: AreaStroke) -> Option<String> {
    if stroke.length() < 1.0 {
        return Some(t!("status-area-too-short"));
    }
    let span = stroke.span();
    match state.selection {
        // With an area selected the stroke joins it — that is how an area comes to cover
        // several tracks, one stroke at a time.
        Selection::TrackArea(i) if i < line.source.areas.len() => {
            line.source.areas[i].spans.push(span);
        }
        _ => {
            line.source.areas.push(content::route::TrackAreaSource {
                name: t!("area-default-name", index = line.source.areas.len() + 1),
                spans: vec![span],
                ..Default::default()
            });
            state.selection = Selection::TrackArea(line.source.areas.len() - 1);
        }
    }
    None
}
'''
s2, n = OLD_MARK.subn(new_mark, s, count=1)
assert n == 1, "mark_area not found"
open(TOOLS, "w", encoding="utf-8").write(s2)

# --- 4. Drag handling, in the shape the marking brush already uses ------------
replace(
    TOOLS,
    [
        (
            """    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }""",
            """    // The area brush paints while the button is held: press takes hold of a track, the
    // drag stretches the stroke along it, release lays it down.
    if state.tool == Tool::MarkArea {
        if buttons.just_pressed(MouseButton::Left)
            && let Some(p) = picked
        {
            state.map_used = true;
            match nearest_on_network(&line.net, p) {
                Some((edge, s, d)) if d <= pick_radius(&focus) => {
                    state.area_stroke = Some(AreaStroke {
                        edge,
                        from: s,
                        to: s,
                    });
                }
                _ => overlay.status = t!("status-no-track-hit"),
            }
        }
        if buttons.pressed(MouseButton::Left)
            && let Some(stroke) = &mut state.area_stroke
            && let Some(p) = picked
            && let Some(edge) = line.net.edges().get(stroke.edge)
        {
            // Projected onto the track it started on, so the stroke follows that track
            // even where the cursor wanders off it.
            stroke.to = nearest_on_edge(edge, p).0;
        }
        if !buttons.pressed(MouseButton::Left)
            && let Some(stroke) = state.area_stroke.take()
            && let Some(status) = commit_stroke(&mut line, &mut state, stroke)
        {
            overlay.status = status;
        }
        return;
    }

    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }""",
        ),
        # the old two-click branch goes
        (
            """        Tool::MarkArea => {
            match nearest_on_network(&line.net, p) {
                Some((edge, s, distance)) if distance <= pick_radius(&focus) => {
                    if let Some(status) = mark_area(&mut line, &mut state, edge, s) {
                        overlay.status = status;
                    }
                }
                _ => overlay.status = t!("status-no-track-hit"),
            }
        }
""",
            "",
        ),
    ],
)

print("ok")
