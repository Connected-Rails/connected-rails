//! Icons for the viewport bars, drawn from line segments.
//!
//! Not typed as glyphs: Inter carries no symbol set, and the emoji fallback
//! renders tofu on some machines — the same reason `×` is spelled U+00D7
//! everywhere else in the editors. Drawn shapes also take the theme colours,
//! so an active button's icon turns with its fill.

use bevy_egui::egui::{
    self, Color32, CornerRadius, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2,
};

use crate::colors;

/// Symbols of the viewport bar.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    /// Looking straight down — a plan grid.
    TopDown,
    /// Free 3D view — an isometric cube.
    Perspective,
    /// Move handles — a four-way arrow.
    Move,
    /// Rotate handle — an arc with an arrow head.
    Rotate,
    /// Aerial imagery — a picture frame.
    Imagery,
    /// Terrain — two hills.
    Terrain,
    /// Camera speed — a gauge.
    Speed,
}

/// Size of an icon button: the design system's widget height, a little wider
/// than tall so a row of them keeps the rhythm of the text buttons beside it.
const BUTTON: Vec2 = Vec2::new(26.0, 22.0);
/// Air between the button edge and the drawing.
const PADDING: f32 = 5.0;
const LINE: f32 = 1.5;

/// An icon button, pressed-in while `active` — a pair of them reads as a
/// choice rather than as two commands.
pub fn icon_button(ui: &mut Ui, icon: Icon, active: bool, tooltip: impl Into<String>) -> Response {
    let (rect, response) = ui.allocate_exact_size(BUTTON, Sense::click());
    let fill = if active {
        colors::ACCENT_BG
    } else if response.hovered() {
        colors::BG_HOVER
    } else {
        colors::BG_WIDGET
    };
    let color = if active {
        colors::ACCENT_TEXT
    } else {
        colors::TEXT
    };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(4), fill);
    draw(painter, square(rect).shrink(PADDING), icon, color);
    response.on_hover_text(tooltip.into())
}

/// The largest centred square of `rect` — icons are drawn undistorted whatever
/// the button's aspect ratio is.
fn square(rect: Rect) -> Rect {
    let side = rect.width().min(rect.height());
    Rect::from_center_size(rect.center(), Vec2::splat(side))
}

/// Draws `icon` into `rect`, in unit coordinates with y running down.
fn draw(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let stroke = Stroke::new(LINE, color);
    let at = |x: f32, y: f32| rect.lerp_inside(Vec2::new(x, y));
    // Point on the circle around the centre, at a maths angle (0 = right).
    let polar = |radius: f32, degrees: f32| {
        let a = degrees.to_radians();
        at(0.5 + radius * a.cos(), 0.5 - radius * a.sin())
    };
    let line = |points: Vec<Pos2>| Shape::line(points, stroke);

    let shapes: Vec<Shape> = match icon {
        // A plan grid: the frame plus its two centre lines.
        Icon::TopDown => vec![
            Shape::closed_line(
                vec![
                    at(0.05, 0.05),
                    at(0.95, 0.05),
                    at(0.95, 0.95),
                    at(0.05, 0.95),
                ],
                stroke,
            ),
            line(vec![at(0.5, 0.05), at(0.5, 0.95)]),
            line(vec![at(0.05, 0.5), at(0.95, 0.5)]),
        ],
        // An isometric cube: the hexagon outline and the three visible edges
        // meeting in its middle.
        Icon::Perspective => {
            let hexagon: Vec<Pos2> = (0..6)
                .map(|i| polar(0.48, 90.0 - 60.0 * i as f32))
                .collect();
            vec![
                Shape::closed_line(hexagon, stroke),
                line(vec![at(0.5, 0.5), polar(0.48, 90.0)]),
                line(vec![at(0.5, 0.5), polar(0.48, -30.0)]),
                line(vec![at(0.5, 0.5), polar(0.48, 210.0)]),
            ]
        }
        // A four-way arrow, one head per direction.
        Icon::Move => {
            let head = |tip: Pos2, dx: f32, dy: f32| {
                line(vec![
                    Pos2::new(tip.x + dx, tip.y + dy),
                    tip,
                    Pos2::new(tip.x - dx, tip.y + dy),
                ])
            };
            let arm = rect.width() * 0.18;
            vec![
                line(vec![at(0.5, 0.04), at(0.5, 0.96)]),
                line(vec![at(0.04, 0.5), at(0.96, 0.5)]),
                head(at(0.5, 0.04), arm, arm),
                head(at(0.5, 0.96), arm, -arm),
                // The side heads are the same three points, turned a quarter.
                line(vec![at(0.22, 0.32), at(0.04, 0.5), at(0.22, 0.68)]),
                line(vec![at(0.78, 0.32), at(0.96, 0.5), at(0.78, 0.68)]),
            ]
        }
        // An open arc with a head — a turn, not a circle.
        Icon::Rotate => {
            let arc: Vec<Pos2> = (0..=24)
                .map(|i| polar(0.42, 40.0 + 280.0 * i as f32 / 24.0))
                .collect();
            let tip = polar(0.42, 40.0);
            vec![
                line(arc),
                line(vec![polar(0.20, 32.0), tip, polar(0.60, 58.0)]),
            ]
        }
        // A photo: frame, sun, and the ground below it.
        Icon::Imagery => vec![
            Shape::closed_line(
                vec![
                    at(0.04, 0.14),
                    at(0.96, 0.14),
                    at(0.96, 0.86),
                    at(0.04, 0.86),
                ],
                stroke,
            ),
            Shape::circle_filled(at(0.30, 0.36), rect.width() * 0.08, color),
            line(vec![
                at(0.04, 0.78),
                at(0.36, 0.48),
                at(0.58, 0.68),
                at(0.72, 0.56),
                at(0.96, 0.78),
            ]),
        ],
        // Two hills — ground, without the frame that makes it a picture.
        Icon::Terrain => vec![
            Shape::convex_polygon(
                vec![at(0.44, 0.92), at(0.72, 0.42), at(0.98, 0.92)],
                color.gamma_multiply(0.55),
                Stroke::NONE,
            ),
            Shape::convex_polygon(
                vec![at(0.02, 0.92), at(0.36, 0.24), at(0.70, 0.92)],
                color,
                Stroke::NONE,
            ),
        ],
        // A gauge with its needle up and to the right — faster than half. The
        // filled pivot is what makes the second line read as a needle rather
        // than as a stray tick on the dial.
        Icon::Speed => {
            let dial: Vec<Pos2> = (0..=16)
                .map(|i| polar(0.46, 200.0 - 220.0 * i as f32 / 16.0))
                .collect();
            let pivot = at(0.5, 0.74);
            vec![
                line(dial),
                line(vec![pivot, polar(0.34, 62.0)]),
                Shape::circle_filled(pivot, rect.width() * 0.07, color),
            ]
        }
    };
    painter.extend(shapes);
}

/// The icon alone, as a label for the control next to it — no fill and no
/// hover, so it does not read as a button that does nothing.
pub fn icon_label(ui: &mut Ui, icon: Icon) {
    let (rect, _) = ui.allocate_exact_size(BUTTON, Sense::hover());
    draw(
        ui.painter(),
        square(rect).shrink(PADDING),
        icon,
        colors::TEXT_SECONDARY,
    );
}

/// A hairline between two groups of a bar — `ui.separator()` in a horizontal
/// layout stretches to the full row height and reads as a panel edge.
pub fn bar_divider(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(1.0, BUTTON.y), Sense::hover());
    ui.painter().rect_filled(
        Rect::from_center_size(rect.center(), Vec2::new(1.0, BUTTON.y * 0.7)),
        CornerRadius::ZERO,
        colors::BORDER,
    );
}

/// A compact numeric control for a bar — the form's [`crate::field`] is a
/// fixed 150 px wide, which is a column width, not a toolbar width.
pub fn bar_value(
    ui: &mut Ui,
    value: &mut f64,
    speed: f64,
    range: std::ops::RangeInclusive<f64>,
    suffix: &str,
) -> Response {
    ui.add_sized(
        Vec2::new(58.0, BUTTON.y),
        egui::DragValue::new(value)
            .speed(speed)
            .range(range)
            .max_decimals(1)
            .suffix(suffix),
    )
}
