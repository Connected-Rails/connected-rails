//! Curves: the small sparkline wells and the modal editor they open.
//!
//! The read-only wells ([`sparkline`], [`sparkline_fn`]) stay what they were —
//! shape at rest, figures on hover, no axes. [`curve_editor`] is the one way to
//! *edit* an `(x, y)` table anywhere in the editors: the form shows only the
//! compact well, a click opens a modal with the room a real editor needs —
//! a plot with axes and draggable points, plus the exact-value table.

use crate::{colors, drag, group_digits, space};
use bevy_egui::egui::{
    self, Align2, CornerRadius, CursorIcon, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2,
    pos2, vec2,
};
use i18n::t;
use std::ops::RangeInclusive;

/// A small plot of an `(x, y)` table.
///
/// Not an analysis tool — no axes, no ticks, no numbers. It answers "does this
/// look like a tractive effort curve" at a glance, which three rows of drag
/// fields cannot: a point typed one digit wrong reads as a kink here and as a
/// plausible number there.
pub fn sparkline(ui: &mut egui::Ui, points: &[(f64, f64)], x_unit: &str, y_unit: &str) {
    plot(ui, points, x_unit, y_unit, true);
}

/// Samples `f` over `0..=x_max` and plots it with [`sparkline`].
///
/// For curves the vehicle does not store as points but computes — running
/// resistance from three Davis coefficients, tractive effort from a handful of
/// limits. Sample the simulator's own function, never a copy of it, or the
/// picture and the physics drift apart.
pub fn sparkline_fn(
    ui: &mut egui::Ui,
    x_max: f64,
    x_unit: &str,
    y_unit: &str,
    f: impl Fn(f64) -> f64,
) {
    const STEPS: usize = 40;
    if x_max <= 0.0 || x_max.is_nan() {
        return;
    }
    let points: Vec<(f64, f64)> = (0..=STEPS)
        .map(|i| {
            let x = x_max * i as f64 / STEPS as f64;
            (x, f(x))
        })
        .collect();
    plot(ui, &points, x_unit, y_unit, false);
}

/// What a curve is measured in and how its fields step — everything the editor
/// needs beyond the points themselves.
pub struct CurveSpec {
    /// Unique per curve site; keys the open state and the point widgets.
    pub id: egui::Id,
    /// Heading of the modal — the title the form already shows above the well.
    pub title: String,
    pub x_unit: &'static str,
    pub y_unit: &'static str,
    /// Drag speed of the table fields, per axis.
    pub x_speed: f64,
    pub y_speed: f64,
    pub x_range: RangeInclusive<f64>,
    pub y_range: RangeInclusive<f64>,
}

/// A clickable [`sparkline`] well that opens the modal curve editor.
///
/// Unlike the read-only wells it draws even with fewer than two points — an
/// empty curve still needs its way into the editor.
pub fn curve_editor(ui: &mut egui::Ui, spec: &CurveSpec, points: &mut Vec<(f64, f64)>) {
    let open_id = spec.id.with("open");
    if editable_well(ui, spec, points) {
        ui.data_mut(|d| d.insert_temp(open_id, true));
    }
    if ui.data(|d| d.get_temp(open_id)).unwrap_or(false) {
        show_modal(ui, spec, points, open_id);
    }
}

// --- The wells -------------------------------------------------------------

/// The mapping between curve values and a screen rectangle.
#[derive(Clone, Copy, Default)]
struct Domain {
    x0: f64,
    x1: f64,
    y0: f64,
    y1: f64,
}

impl Domain {
    /// The sparkline mapping: x spans the data, y starts at zero.
    ///
    /// The y axis starts at zero, not at the smallest value. These are physical
    /// magnitudes: normalised to their own range, a friction factor falling
    /// from 1.0 to 0.6 fills the plot exactly like one falling to nothing, and
    /// "how far does it drop" is the only question the picture is asked.
    /// `None` when there is no spread in x — no shape to show.
    fn tight(sorted: &[(f64, f64)]) -> Option<Self> {
        if sorted.len() < 2 {
            return None;
        }
        let (x0, x1) = (sorted[0].0, sorted[sorted.len() - 1].0);
        if x1 <= x0 {
            return None;
        }
        let y0 = sorted.iter().map(|p| p.1).fold(0.0_f64, f64::min);
        let mut y1 = sorted.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
        if y1 <= y0 {
            // All-zero: along the bottom, where zero is, rather than divided by it.
            y1 = y0 + 1.0;
        }
        Some(Self { x0, x1, y0, y1 })
    }

    /// The editor mapping: zero-based like [`Domain::tight`], with headroom so
    /// the outermost point is not glued to the frame, and never degenerate —
    /// an empty or single-point curve still gets axes to click into.
    fn padded(points: &[(f64, f64)]) -> Self {
        let (mut x0, mut x1) = (0.0_f64, 0.0_f64);
        let (mut y0, mut y1) = (0.0_f64, 0.0_f64);
        for &(x, y) in points {
            x0 = x0.min(x);
            x1 = x1.max(x);
            y0 = y0.min(y);
            y1 = y1.max(y);
        }
        if points.is_empty() {
            (x1, y1) = (100.0, 1.0);
        }
        if x1 - x0 < 1e-9 {
            x1 = x0 + 10.0;
        } else {
            x1 += (x1 - x0) * 0.05;
        }
        if y1 - y0 < 1e-9 {
            y1 = y0 + 1.0;
        } else {
            y1 += (y1 - y0) * 0.08;
        }
        Self { x0, x1, y0, y1 }
    }

    fn to_screen(self, rect: Rect, (x, y): (f64, f64)) -> Pos2 {
        let tx = (x - self.x0) / (self.x1 - self.x0);
        let ty = (y - self.y0) / (self.y1 - self.y0);
        pos2(
            rect.left() + tx as f32 * rect.width(),
            rect.bottom() - ty as f32 * rect.height(),
        )
    }

    /// The inverse of [`Domain::to_screen`], clamped to the rectangle.
    fn value_at(self, rect: Rect, pos: Pos2) -> (f64, f64) {
        let tx = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64;
        let ty = ((rect.bottom() - pos.y) / rect.height()).clamp(0.0, 1.0) as f64;
        (
            self.x0 + tx * (self.x1 - self.x0),
            self.y0 + ty * (self.y1 - self.y0),
        )
    }
}

/// Allocates and paints the empty well — same `BG_INPUT` and hairline as a
/// text field. A clickable well answers hover with the stronger border.
fn well(ui: &mut egui::Ui, sense: Sense, clickable: bool) -> (Rect, egui::Response) {
    let width = ui.available_width().min(space::FIELD * 2.0 + space::M);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 56.0), sense);
    let border = if clickable && response.hovered() {
        colors::BORDER
    } else {
        colors::BORDER_SUBTLE
    };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(4), colors::BG_INPUT);
    painter.rect_stroke(
        rect,
        CornerRadius::same(4),
        Stroke::new(1.0, border),
        egui::StrokeKind::Inside,
    );
    (rect, response)
}

fn draw_curve(painter: &egui::Painter, rect: Rect, dom: Domain, sorted: &[(f64, f64)], marks: bool) {
    let line: Vec<Pos2> = sorted.iter().map(|&p| dom.to_screen(rect, p)).collect();
    painter.add(egui::Shape::line(
        line.clone(),
        Stroke::new(1.5, colors::ACCENT),
    ));
    if marks {
        for point in line {
            painter.circle_filled(point, 2.0, colors::ACCENT);
        }
    }
}

/// Reading a value off 56 px of line is guesswork. Hovering says it exactly,
/// and costs the plot no clutter when nobody asks.
fn hover_readout(
    response: &egui::Response,
    plot: Rect,
    sorted: &[(f64, f64)],
    x_unit: &str,
    y_unit: &str,
    extra: Option<String>,
) {
    if let Some(pointer) = response.hover_pos() {
        let t = ((pointer.x - plot.left()) / plot.width()).clamp(0.0, 1.0) as f64;
        let (x0, x1) = (sorted[0].0, sorted[sorted.len() - 1].0);
        let x = x0 + t * (x1 - x0);
        let y = interpolate(sorted, x);
        let mut text = format!("{} → {}", with_unit(x, x_unit), with_unit(y, y_unit));
        if let Some(extra) = extra {
            text.push('\n');
            text.push_str(&extra);
        }
        response.clone().on_hover_text(text);
    }
}

/// `marks` puts a dot on every point. True where the points are the data the
/// user typed; false for a sampled curve, where a dot per sample says nothing
/// about the vehicle and only turns the line into a dotted one.
fn plot(ui: &mut egui::Ui, points: &[(f64, f64)], x_unit: &str, y_unit: &str, marks: bool) {
    if points.len() < 2 {
        return;
    }
    let mut sorted = points.to_vec();
    sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
    let Some(dom) = Domain::tight(&sorted) else {
        return;
    };
    let (rect, response) = well(ui, Sense::hover(), false);
    let plot = rect.shrink(space::S);
    draw_curve(ui.painter(), plot, dom, &sorted, marks);
    hover_readout(&response, plot, &sorted, x_unit, y_unit, None);
}

/// The inline face of [`curve_editor`]. Returns true on a click.
fn editable_well(ui: &mut egui::Ui, spec: &CurveSpec, points: &[(f64, f64)]) -> bool {
    let (rect, response) = well(ui, Sense::click(), true);
    let plot = rect.shrink(space::S);
    let mut sorted = points.to_vec();
    sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
    match Domain::tight(&sorted) {
        Some(dom) => {
            draw_curve(ui.painter(), plot, dom, &sorted, true);
            hover_readout(
                &response,
                plot,
                &sorted,
                spec.x_unit,
                spec.y_unit,
                Some(t!("curve-open-hint")),
            );
        }
        _ => {
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                t!("curve-empty"),
                FontId::proportional(11.0),
                colors::TEXT_SECONDARY,
            );
            response.clone().on_hover_text(t!("curve-open-hint"));
        }
    }
    response.clone().on_hover_cursor(CursorIcon::PointingHand);
    response.clicked()
}

// --- The modal editor ------------------------------------------------------

const CANVAS: Vec2 = Vec2::new(430.0, 300.0);

fn show_modal(ui: &mut egui::Ui, spec: &CurveSpec, points: &mut Vec<(f64, f64)>, open_id: egui::Id) {
    let mut close = false;
    let modal = egui::Modal::new(spec.id.with("modal")).show(ui.ctx(), |ui| {
        ui.label(crate::heading(spec.title.clone()));
        ui.add_space(space::XS);
        let mut settle = false;
        let mut dragging = false;
        ui.horizontal_top(|ui| {
            let (s, d) = canvas(ui, spec, points);
            settle |= s;
            dragging = d;
            ui.vertical(|ui| {
                egui::ScrollArea::vertical()
                    .max_height(CANVAS.y)
                    .show(ui, |ui| {
                        settle |= point_table(ui, spec, points);
                    });
            });
        });
        // Support points are only meaningful in ascending x. Sorting while a
        // value is being dragged would pull the point out from under the
        // cursor, so it happens once the drag ends — by which time the user
        // has stopped looking at it anyway.
        if settle && !dragging {
            points.sort_by(|a, b| a.0.total_cmp(&b.0));
        }
        ui.add_space(space::XS);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(t!("curve-editor-help"))
                    .small()
                    .color(colors::TEXT_SECONDARY),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(t!("action-close")).clicked() {
                    close = true;
                }
            });
        });
        // Esc closes the editor — unless a field has focus, where it means
        // "cancel this edit" and the field consumes it.
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) && ui.memory(|m| m.focused().is_none()) {
            close = true;
        }
    });
    if close || modal.should_close() {
        ui.data_mut(|d| d.remove_temp::<bool>(open_id));
    }
}

/// The plot itself: axes, grid, the curve, and the points as handles.
/// Drag moves a point, double-click adds one, right-click removes one.
/// Returns (settle, a drag is still active).
fn canvas(ui: &mut egui::Ui, spec: &CurveSpec, points: &mut Vec<(f64, f64)>) -> (bool, bool) {
    let (outer, bg) = ui.allocate_exact_size(CANVAS, Sense::click());
    let painter = ui.painter_at(outer);
    painter.rect_filled(outer, CornerRadius::same(4), colors::BG_INPUT);
    painter.rect_stroke(
        outer,
        CornerRadius::same(4),
        Stroke::new(1.0, colors::BORDER_SUBTLE),
        egui::StrokeKind::Inside,
    );
    // Margins hold the tick labels: y left, x below.
    let plot = Rect::from_min_max(
        pos2(outer.left() + 52.0, outer.top() + 14.0),
        pos2(outer.right() - 14.0, outer.bottom() - 24.0),
    );

    // While a point is dragged the mapping is frozen: the point follows the
    // pointer, and a scale recomputed from the moving value would slide the
    // whole picture under the cursor.
    let frozen_id = spec.id.with("frozen-domain");
    let dom = ui
        .data(|d| d.get_temp::<Domain>(frozen_id))
        .unwrap_or_else(|| Domain::padded(points));

    // Grid and tick labels.
    let font = FontId::proportional(10.0);
    for x in ticks(dom.x0, dom.x1) {
        let sx = dom.to_screen(plot, (x, 0.0)).x;
        painter.line_segment(
            [pos2(sx, plot.top()), pos2(sx, plot.bottom())],
            Stroke::new(1.0, colors::BORDER_SUBTLE),
        );
        painter.text(
            pos2(sx, plot.bottom() + 4.0),
            Align2::CENTER_TOP,
            tick_label(x, nice_step(dom.x1 - dom.x0)),
            font.clone(),
            colors::TEXT_SECONDARY,
        );
    }
    for y in ticks(dom.y0, dom.y1) {
        let sy = dom.to_screen(plot, (0.0, y)).y;
        painter.line_segment(
            [pos2(plot.left(), sy), pos2(plot.right(), sy)],
            Stroke::new(1.0, colors::BORDER_SUBTLE),
        );
        painter.text(
            pos2(plot.left() - 6.0, sy),
            Align2::RIGHT_CENTER,
            tick_label(y, nice_step(dom.y1 - dom.y0)),
            font.clone(),
            colors::TEXT_SECONDARY,
        );
    }
    // Units once per axis, in the label margins.
    if !spec.x_unit.is_empty() {
        painter.text(
            pos2(plot.right(), outer.bottom() - 3.0),
            Align2::RIGHT_BOTTOM,
            spec.x_unit,
            font.clone(),
            colors::TEXT_SECONDARY,
        );
    }
    if !spec.y_unit.is_empty() {
        painter.text(
            pos2(plot.left(), outer.top() + 2.0),
            Align2::LEFT_TOP,
            spec.y_unit,
            font.clone(),
            colors::TEXT_SECONDARY,
        );
    }

    // Interactions first, drawing after — the curve is painted from this
    // frame's values, not last frame's.
    let mut settle = false;
    let mut dragging = false;
    let mut remove = None;
    let mut active = None;
    for (i, point) in points.iter_mut().enumerate() {
        let pos = dom.to_screen(plot, *point);
        let response = ui.interact(
            Rect::from_center_size(pos, Vec2::splat(16.0)),
            spec.id.with(("pt", i)),
            Sense::click_and_drag(),
        );
        if response.drag_started() {
            ui.data_mut(|d| d.insert_temp(frozen_id, dom));
        }
        if response.dragged() {
            dragging = true;
            if let Some(pointer) = response.interact_pointer_pos() {
                let (x, y) = dom.value_at(plot, pointer);
                *point = (
                    x.clamp(*spec.x_range.start(), *spec.x_range.end()),
                    y.clamp(*spec.y_range.start(), *spec.y_range.end()),
                );
            }
        }
        if response.drag_stopped() {
            ui.data_mut(|d| d.remove_temp::<Domain>(frozen_id));
            settle = true;
        }
        if response.secondary_clicked() {
            remove = Some(i);
        }
        if response.hovered() || response.dragged() {
            active = Some(i);
            response.on_hover_cursor(CursorIcon::Grab);
        }
    }
    if let Some(i) = remove {
        points.remove(i);
        settle = true;
    }
    if bg.double_clicked()
        && let Some(pointer) = bg.interact_pointer_pos()
        && plot.contains(pointer)
    {
        let (x, y) = dom.value_at(plot, pointer);
        points.push((
            x.clamp(*spec.x_range.start(), *spec.x_range.end()),
            y.clamp(*spec.y_range.start(), *spec.y_range.end()),
        ));
        settle = true;
    }

    // The curve and its handles.
    let mut sorted = points.clone();
    sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
    if sorted.len() >= 2 {
        draw_curve(&painter, plot, dom, &sorted, false);
    }
    for (i, &p) in points.iter().enumerate() {
        let pos = dom.to_screen(plot, p);
        if active == Some(i) {
            painter.circle(
                pos,
                5.0,
                colors::ACCENT,
                Stroke::new(1.5, colors::TEXT_STRONG),
            );
        } else {
            painter.circle_filled(pos, 3.5, colors::ACCENT);
        }
    }
    // The touched point states its exact values right where the eye is.
    if let Some(i) = active
        && let Some(&p) = points.get(i)
    {
        let pos = dom.to_screen(plot, p) + vec2(0.0, -12.0);
        painter.text(
            pos.clamp(plot.left_top() + vec2(20.0, 8.0), plot.right_bottom()),
            Align2::CENTER_BOTTOM,
            format!(
                "{} → {}",
                with_unit(p.0, spec.x_unit),
                with_unit(p.1, spec.y_unit)
            ),
            FontId::proportional(11.0),
            colors::TEXT_STRONG,
        );
    }
    if points.is_empty() {
        painter.text(
            plot.center(),
            Align2::CENTER_CENTER,
            t!("curve-empty"),
            FontId::proportional(11.0),
            colors::TEXT_SECONDARY,
        );
    }
    if active.is_none()
        && bg.hovered()
        && let Some(pointer) = bg.hover_pos()
        && plot.contains(pointer)
    {
        ui.ctx().output_mut(|o| o.cursor_icon = CursorIcon::Crosshair);
        if sorted.len() >= 2 {
            hover_readout(&bg, plot, &sorted, spec.x_unit, spec.y_unit, None);
        }
    }
    (settle, dragging)
}

/// The exact-value side of the canvas: one row of drag fields per point.
/// Returns true when a row settled (drag ended, focus left) or changed shape.
fn point_table(ui: &mut egui::Ui, spec: &CurveSpec, points: &mut Vec<(f64, f64)>) -> bool {
    let mut settle = false;
    let mut remove = None;
    // Compact fields — the canvas next door carries the shape, these carry
    // the exact figures.
    ui.spacing_mut().interact_size.x = 72.0;
    egui::Grid::new(spec.id.with("table"))
        .num_columns(3)
        .spacing(vec2(space::XS, 4.0))
        .show(ui, |ui| {
            for (i, (x, y)) in points.iter_mut().enumerate() {
                let rx = ui.add(drag(x, spec.x_speed, spec.x_range.clone(), spec.x_unit));
                ui.add(drag(y, spec.y_speed, spec.y_range.clone(), spec.y_unit));
                settle |= rx.drag_stopped() || rx.lost_focus();
                if ui.small_button("×").clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
    if let Some(i) = remove {
        points.remove(i);
        settle = true;
    }
    if ui.button(t!("action-add-point")).clicked() {
        let last = points.last().copied().unwrap_or((0.0, 0.0));
        points.push((last.0 + 10.0, last.1));
        settle = true;
    }
    settle
}

// --- Axis arithmetic -------------------------------------------------------

/// A tick step of 1, 2 or 5 times a power of ten, aiming for ~5 divisions.
fn nice_step(span: f64) -> f64 {
    let raw = span / 5.0;
    let mag = 10.0_f64.powi(raw.log10().floor() as i32);
    let n = raw / mag;
    let factor = if n < 1.5 {
        1.0
    } else if n < 3.5 {
        2.0
    } else if n < 7.5 {
        5.0
    } else {
        10.0
    };
    factor * mag
}

/// Multiples of the nice step inside `[lo, hi]`.
fn ticks(lo: f64, hi: f64) -> Vec<f64> {
    let step = nice_step(hi - lo);
    let mut t = (lo / step).ceil() * step;
    let mut out = Vec::new();
    while t <= hi + step * 1e-6 {
        out.push(t);
        t += step;
    }
    out
}

/// Axis labels stay narrow: k/M notation above ten thousand — the exact value
/// lives in the table and the hover readout, the axis only names the scale.
fn tick_label(value: f64, step: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    let compact = |v: f64| {
        let s = format!("{v:.1}");
        s.strip_suffix(".0").unwrap_or(&s).to_string()
    };
    if step >= 1_000_000.0 {
        return format!("{}M", compact(value / 1e6));
    }
    if step >= 10_000.0 {
        return format!("{}k", compact(value / 1e3));
    }
    let decimals = (-step.log10().floor()).max(0.0) as usize;
    if step >= 1.0 {
        group_digits(value)
    } else {
        format!("{value:.decimals$}")
    }
}

/// Linear between the two points that bracket `x`.
fn interpolate(sorted: &[(f64, f64)], x: f64) -> f64 {
    match sorted.iter().position(|p| p.0 >= x) {
        None => sorted[sorted.len() - 1].1,
        Some(0) => sorted[0].1,
        Some(i) => {
            let (x0, y0) = sorted[i - 1];
            let (x1, y1) = sorted[i];
            if x1 > x0 {
                y0 + (y1 - y0) * (x - x0) / (x1 - x0)
            } else {
                y1
            }
        }
    }
}

/// Digit grouping above 100, two decimals below — one formatter for forces in
/// the hundreds of thousands and friction factors below one.
fn with_unit(value: f64, unit: &str) -> String {
    let number = if value.abs() >= 100.0 {
        group_digits(value)
    } else {
        format!("{value:.2}")
    };
    if unit.is_empty() {
        number
    } else {
        format!("{number}{}{unit}", crate::NBSP)
    }
}

#[cfg(test)]
mod tests {
    /// The hover readout is only as good as this: a wrong bracket reports a
    /// plausible number for the wrong speed, and nothing looks amiss.
    #[test]
    fn hover_reads_between_the_points() {
        let curve = [(0.0, 100.0), (50.0, 200.0), (150.0, 0.0)];
        assert_eq!(super::interpolate(&curve, 0.0), 100.0);
        assert_eq!(super::interpolate(&curve, 25.0), 150.0, "half way up");
        assert_eq!(super::interpolate(&curve, 50.0), 200.0, "on a point");
        assert_eq!(super::interpolate(&curve, 100.0), 100.0, "half way down");
        assert_eq!(super::interpolate(&curve, 150.0), 0.0);
        // Outside the curve it holds the end values rather than extrapolating.
        assert_eq!(super::interpolate(&curve, -10.0), 100.0);
        assert_eq!(super::interpolate(&curve, 999.0), 0.0);
    }

    #[test]
    fn readouts_suit_both_forces_and_factors() {
        assert_eq!(super::with_unit(185_000.0, "N"), "185\u{A0}000\u{A0}N");
        assert_eq!(super::with_unit(0.6, ""), "0.60");
        assert_eq!(super::with_unit(120.0, "km/h"), "120\u{A0}km/h");
    }

    /// Axis ticks land on round numbers whatever the data span is.
    #[test]
    fn ticks_are_round_and_cover_the_span() {
        assert_eq!(super::ticks(0.0, 350_000.0), vec![
            0.0, 50_000.0, 100_000.0, 150_000.0, 200_000.0, 250_000.0, 300_000.0, 350_000.0
        ]);
        assert_eq!(super::ticks(0.0, 1.08), vec![0.0, 0.2, 0.4, 0.6000000000000001, 0.8, 1.0]);
        assert_eq!(super::tick_label(300_000.0, 50_000.0), "300k");
        assert_eq!(super::tick_label(1_500_000.0, 1_000_000.0), "1.5M");
        assert_eq!(super::tick_label(0.6000000000000001, 0.2), "0.6");
        assert_eq!(super::tick_label(120.0, 20.0), "120");
        assert_eq!(super::tick_label(2_500.0, 2_500.0), "2\u{A0}500");
    }

    /// A fresh effort curve has no points yet; the well drew `sorted[0]`
    /// anyway and took the editor down the moment "curve" was selected.
    #[test]
    fn tight_domain_survives_empty_and_flat_curves() {
        assert!(super::Domain::tight(&[]).is_none());
        assert!(super::Domain::tight(&[(0.0, 1.0)]).is_none());
        assert!(super::Domain::tight(&[(5.0, 1.0), (5.0, 2.0)]).is_none());
        assert!(super::Domain::tight(&[(0.0, 1.0), (10.0, 2.0)]).is_some());
    }

    /// The editor's mapping never divides by zero — an empty or single-point
    /// curve still has to give double-click somewhere to land.
    #[test]
    fn padded_domain_is_never_degenerate() {
        for points in [&[][..], &[(0.0, 0.0)][..], &[(50.0, 0.5), (50.0, 0.5)][..]] {
            let d = super::Domain::padded(points);
            assert!(d.x1 > d.x0, "{points:?}");
            assert!(d.y1 > d.y0, "{points:?}");
        }
    }
}
