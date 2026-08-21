//! Icons for the viewport bars, drawn from line segments.
//!
//! Not typed as glyphs: Inter carries no symbol set, and the emoji fallback
//! renders tofu on some machines — the same reason `×` is spelled U+00D7
//! everywhere else in the editors. Drawn shapes also take the theme colours,
//! so an active button's icon turns with its fill.

use bevy_egui::egui::{
    self, Color32, CornerRadius, Painter, Pos2, Rect, Response, RichText, Sense, Shape, Stroke, Ui,
    Vec2,
};

use crate::colors;

/// Symbols of the viewport bar.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
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
    /// Scenery object — a crate.
    Object,
    /// Signal — a mast carrying two lamps.
    Signal,
    /// Track — two rails on their sleepers.
    Track,
    /// The content drawer — a drawer with its handle.
    Drawer,
    /// Select — the arrow cursor.
    Select,
    /// Draw track — a polyline over its support points.
    DrawTrack,
    /// Place device — a box mounted beside the rail.
    Device,
    /// Place switch — a track branching off.
    Switch,
    /// Mark area — a stretch hatched over the rail.
    Area,
    /// Place tree — a crown on its trunk.
    Tree,
    /// Forest brush — a stand of three.
    Forest,
    /// Marking brush — a brush with its bristles.
    Brush,
    /// Place marker — a flag on its pole.
    Marker,
    /// Terrain brush — a hill under a brush stroke.
    TerrainBrush,
    /// Pick DGM tiles — a grid with one cell taken.
    Tiles,
    /// Module envelope — a closed polygon on its corner points.
    Envelope,
}

/// Size of an icon button: the design system's widget height, a little wider
/// than tall so a row of them keeps the rhythm of the text buttons beside it.
pub(crate) const BUTTON: Vec2 = Vec2::new(26.0, 22.0);
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
        // A crate: front face, lid and right side, the three faces a box shows.
        Icon::Object => vec![
            Shape::closed_line(
                vec![
                    at(0.10, 0.38),
                    at(0.62, 0.38),
                    at(0.62, 0.92),
                    at(0.10, 0.92),
                ],
                stroke,
            ),
            Shape::closed_line(
                vec![
                    at(0.10, 0.38),
                    at(0.38, 0.12),
                    at(0.90, 0.12),
                    at(0.62, 0.38),
                ],
                stroke,
            ),
            line(vec![at(0.62, 0.92), at(0.90, 0.66), at(0.90, 0.12)]),
        ],
        // A signal: the screen with its two lamps, on a mast with a foot. The
        // upper lamp is filled — a signal shows one aspect, not two.
        Icon::Signal => vec![
            Shape::closed_line(
                vec![
                    at(0.30, 0.04),
                    at(0.70, 0.04),
                    at(0.70, 0.46),
                    at(0.30, 0.46),
                ],
                stroke,
            ),
            Shape::circle_filled(at(0.50, 0.16), rect.width() * 0.07, color),
            Shape::circle_stroke(at(0.50, 0.34), rect.width() * 0.07, stroke),
            line(vec![at(0.50, 0.46), at(0.50, 0.94)]),
            line(vec![at(0.32, 0.94), at(0.68, 0.94)]),
        ],
        // Track seen from above: two rails across their sleepers.
        Icon::Track => vec![
            line(vec![at(0.28, 0.04), at(0.28, 0.96)]),
            line(vec![at(0.72, 0.04), at(0.72, 0.96)]),
            line(vec![at(0.10, 0.22), at(0.90, 0.22)]),
            line(vec![at(0.10, 0.50), at(0.90, 0.50)]),
            line(vec![at(0.10, 0.78), at(0.90, 0.78)]),
        ],
        // The arrow cursor, as every editor draws its select tool.
        Icon::Select => vec![Shape::closed_line(
            vec![
                at(0.22, 0.06),
                at(0.22, 0.82),
                at(0.40, 0.64),
                at(0.52, 0.94),
                at(0.66, 0.88),
                at(0.54, 0.58),
                at(0.78, 0.56),
            ],
            stroke,
        )],
        // A polyline over the points it was drawn through — track is drawn
        // point by point, not swept.
        Icon::DrawTrack => {
            let points = [
                at(0.08, 0.80),
                at(0.38, 0.34),
                at(0.66, 0.62),
                at(0.94, 0.18),
            ];
            let mut shapes = vec![line(points.to_vec())];
            shapes.extend(
                points
                    .iter()
                    .map(|p| Shape::circle_filled(*p, rect.width() * 0.075, color)),
            );
            shapes
        }
        // A box on a mast beside the rail: a track device seen from the side.
        Icon::Device => vec![
            line(vec![at(0.06, 0.88), at(0.94, 0.88)]),
            line(vec![at(0.62, 0.88), at(0.62, 0.52)]),
            Shape::closed_line(
                vec![
                    at(0.42, 0.16),
                    at(0.82, 0.16),
                    at(0.82, 0.52),
                    at(0.42, 0.52),
                ],
                stroke,
            ),
        ],
        // A track branching off: the straight and the diverging route.
        Icon::Switch => vec![
            line(vec![at(0.08, 0.74), at(0.92, 0.74)]),
            line(vec![at(0.32, 0.74), at(0.60, 0.36), at(0.92, 0.36)]),
        ],
        // A stretch of rail with the marking laid over it.
        Icon::Area => {
            let mut shapes = vec![line(vec![at(0.04, 0.74), at(0.96, 0.74)])];
            // Hatching, the gesture that paints an area.
            for i in 0..4 {
                let x = 0.16 + 0.22 * i as f32;
                shapes.push(line(vec![at(x, 0.52), at(x - 0.12, 0.18)]));
            }
            shapes.push(line(vec![at(0.10, 0.52), at(0.90, 0.52)]));
            shapes
        }
        // A trunk under a round crown.
        Icon::Tree => vec![
            line(vec![at(0.5, 0.96), at(0.5, 0.60)]),
            Shape::circle_stroke(at(0.5, 0.38), rect.width() * 0.30, stroke),
        ],
        // Three of them, the middle one taller — a stand, not one tree.
        Icon::Forest => vec![
            line(vec![at(0.18, 0.94), at(0.18, 0.66)]),
            Shape::circle_stroke(at(0.18, 0.52), rect.width() * 0.16, stroke),
            line(vec![at(0.50, 0.94), at(0.50, 0.54)]),
            Shape::circle_stroke(at(0.50, 0.36), rect.width() * 0.20, stroke),
            line(vec![at(0.82, 0.94), at(0.82, 0.66)]),
            Shape::circle_stroke(at(0.82, 0.52), rect.width() * 0.16, stroke),
        ],
        // The envelope: a closed run of sides with its corners marked, which is
        // what the tool edits — the corners, not the area.
        Icon::Envelope => {
            let corners = [
                at(0.12, 0.30),
                at(0.62, 0.10),
                at(0.92, 0.62),
                at(0.34, 0.92),
            ];
            let mut shapes = vec![Shape::closed_line(corners.to_vec(), stroke)];
            shapes.extend(
                corners
                    .iter()
                    .map(|c| Shape::circle_filled(*c, LINE, stroke.color)),
            );
            shapes
        }
        // A brush: handle, ferrule and the bristles that make it one.
        Icon::Brush => vec![
            line(vec![at(0.78, 0.10), at(0.44, 0.48)]),
            Shape::closed_line(
                vec![
                    at(0.30, 0.44),
                    at(0.50, 0.62),
                    at(0.38, 0.76),
                    at(0.18, 0.58),
                ],
                stroke,
            ),
            line(vec![at(0.28, 0.72), at(0.10, 0.94)]),
            line(vec![at(0.40, 0.82), at(0.24, 0.96)]),
        ],
        // A flag on its pole — a marker names a place.
        Icon::Marker => vec![
            line(vec![at(0.28, 0.06), at(0.28, 0.96)]),
            Shape::closed_line(vec![at(0.28, 0.10), at(0.86, 0.26), at(0.28, 0.46)], stroke),
        ],
        // A hill with a stroke swept over it: the terrain brush shapes ground.
        Icon::TerrainBrush => vec![
            Shape::convex_polygon(
                vec![at(0.04, 0.94), at(0.44, 0.42), at(0.84, 0.94)],
                color,
                Stroke::NONE,
            ),
            line(vec![at(0.14, 0.26), at(0.52, 0.10), at(0.94, 0.30)]),
        ],
        // A grid with one cell picked out — the height import takes tiles.
        Icon::Tiles => vec![
            Shape::rect_filled(
                Rect::from_min_max(at(0.06, 0.06), at(0.48, 0.48)),
                CornerRadius::ZERO,
                color,
            ),
            Shape::rect_stroke(
                Rect::from_min_max(at(0.52, 0.06), at(0.94, 0.48)),
                CornerRadius::ZERO,
                stroke,
                egui::StrokeKind::Inside,
            ),
            Shape::rect_stroke(
                Rect::from_min_max(at(0.06, 0.52), at(0.48, 0.94)),
                CornerRadius::ZERO,
                stroke,
                egui::StrokeKind::Inside,
            ),
            Shape::rect_stroke(
                Rect::from_min_max(at(0.52, 0.52), at(0.94, 0.94)),
                CornerRadius::ZERO,
                stroke,
                egui::StrokeKind::Inside,
            ),
        ],
        // A drawer pulled out of its cabinet: the case, the drawer front and
        // its handle.
        Icon::Drawer => vec![
            Shape::closed_line(
                vec![
                    at(0.06, 0.10),
                    at(0.94, 0.10),
                    at(0.94, 0.90),
                    at(0.06, 0.90),
                ],
                stroke,
            ),
            line(vec![at(0.06, 0.56), at(0.94, 0.56)]),
            line(vec![at(0.40, 0.73), at(0.60, 0.73)]),
        ],
    };
    painter.extend(shapes);
}

/// A tool button: the icon plus its name, pressed-in while `active`.
///
/// A palette of a dozen tools cannot be icons alone — three of these are
/// brushes, and no drawing tells "forest" from "marking" from "terrain" at
/// 22 px. It cannot be text alone either, which is what the route editor had:
/// twelve chips of different widths wrapping wherever they happened to fit,
/// with no way to see the groups. Icon and name together give the row a fixed
/// height and a shape the eye can find again.
pub fn tool_button(
    ui: &mut Ui,
    icon: Icon,
    label: impl Into<String>,
    active: bool,
    tooltip: Option<String>,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width().min(TOOL_WIDTH), BUTTON.y + 6.0),
        Sense::click(),
    );
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
    let icon_rect = Rect::from_min_size(
        rect.left_top() + Vec2::new(PADDING, 0.0),
        Vec2::new(rect.height(), rect.height()),
    );
    draw(painter, icon_rect.shrink(PADDING), icon, color);
    painter.text(
        Pos2::new(icon_rect.right() + 4.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label.into(),
        egui::FontId::proportional(13.0),
        color,
    );
    match tooltip {
        Some(text) => response.on_hover_text(text),
        None => response,
    }
}

/// What a catalogue entry is marked with: a drawn symbol for its kind, or a
/// colour where the entry *is* one (a track type is what it looks like).
#[derive(Clone, Copy)]
pub enum Mark {
    Icon(Icon),
    Color(Color32),
    /// A rendered preview of the thing itself.
    Image(egui::TextureId),
}

/// One catalogue entry: its mark, the name it carries and a line of provenance
/// under it, at a **fixed** size.
///
/// Laid out by hand rather than as a frame around labels, for the same reason
/// the LOD list is a grid and not a row of `horizontal`s: a card that sizes
/// itself to its text gives every entry its own width and baseline, and a wall
/// of them reads as scattered rather than as a catalogue. Text that does not
/// fit is truncated with an ellipsis — the caller puts the whole of it in the
/// tooltip.
pub fn card_entry(
    ui: &mut Ui,
    mark: Mark,
    title: &str,
    detail: &str,
    selected: bool,
    clickable: bool,
) -> Response {
    let sense = if clickable {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(CARD, sense);
    let fill = if selected {
        colors::ACCENT_BG
    } else if clickable && response.hovered() {
        colors::BG_HOVER
    } else {
        colors::BG_CARD
    };
    let (title_color, detail_color) = if selected {
        (colors::ACCENT_TEXT, colors::ACCENT_TEXT)
    } else {
        (colors::TEXT, colors::TEXT_SECONDARY)
    };

    let painter = ui.painter();
    painter.rect(
        rect,
        CornerRadius::same(4),
        fill,
        Stroke::new(1.0, colors::BORDER_SUBTLE),
        egui::StrokeKind::Inside,
    );
    let mark_rect = Rect::from_min_size(
        rect.left_top() + Vec2::new(crate::space::XS, 0.0),
        Vec2::splat(rect.height()),
    );
    match mark {
        Mark::Icon(icon) => draw(
            painter,
            square(mark_rect).shrink(PADDING),
            icon,
            title_color,
        ),
        Mark::Color(color) => {
            let swatch = square(mark_rect).shrink(PADDING + 2.0);
            painter.rect(
                swatch,
                CornerRadius::same(3),
                color,
                Stroke::new(1.0, colors::BORDER),
                egui::StrokeKind::Inside,
            );
        }
        Mark::Image(texture) => {
            // On the input well's fill: a preview is rendered on transparency,
            // and a dark model on a card fill of nearly the same value would
            // have no edge at all.
            let well = square(mark_rect).shrink(2.0);
            painter.rect_filled(well, CornerRadius::same(3), colors::BG_INPUT);
            painter.image(
                texture,
                well,
                Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }
    }

    let text_left = mark_rect.right() + crate::space::S;
    let width = rect.right() - crate::space::S - text_left;
    let cut = |text: &str, size: f32, color: Color32| {
        ui.ctx().fonts_mut(|fonts| {
            let mut job = egui::text::LayoutJob::single_section(
                text.to_owned(),
                egui::TextFormat::simple(egui::FontId::proportional(size), color),
            );
            job.wrap = egui::text::TextWrapping {
                max_width: width,
                max_rows: 1,
                break_anywhere: false,
                overflow_character: Some('…'),
            };
            fonts.layout_job(job)
        })
    };
    let title = cut(title, 13.0, title_color);
    let detail = cut(detail, 11.0, detail_color);
    // Both lines as one block, centred in the card's height.
    let block = title.size().y + detail.size().y;
    let top = rect.center().y - block / 2.0;
    painter.galley(Pos2::new(text_left, top), title, title_color);
    painter.galley(Pos2::new(text_left, top + 16.0), detail, detail_color);
    response
}

/// Size of a catalogue card: wide enough for a mod-qualified name, and tall
/// enough that the mark is a picture of the model rather than a symbol beside
/// its name.
const CARD: Vec2 = Vec2::new(208.0, 60.0);

/// Width of a tool button — two of them fit the side panel's field column, so
/// a palette lays out as a grid instead of wrapping wherever it runs out.
const TOOL_WIDTH: f32 = 156.0;

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

/// A bar-height button carrying a value and a caret, to be handed to
/// `egui::Popup::menu`.
///
/// The caret is the whole point: a bare number on a toolbar reads as a
/// display, and nobody clicks a display. The caret comes out of the icon font,
/// whose fallback is Inter — so the value beside it stays in the body face.
pub fn bar_menu(ui: &mut Ui, value: impl Into<String>, tooltip: impl Into<String>) -> Response {
    let caret = RichText::new(egui_phosphor::regular::CARET_DOWN).font(crate::icon_font(11.0));
    ui.add_sized(
        Vec2::new(58.0, BUTTON.y),
        egui::Button::new((value.into(), caret)),
    )
    .on_hover_text(tooltip.into())
}
