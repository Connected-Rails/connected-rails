//! Displays section of the model panel: screens rendered to texture on a glTF
//! node (plan ch. 12). The editor binds name, node and resolution, and edits
//! the widget list that fills the screen without a line of script.

use crate::Editor;
use crate::ui::row;
use bevy_egui::egui;
use editor_ui::{colors, field, space};
use i18n::t;
use sim_core::cab::{CabControl, DisplaySource, DisplaySpec, Widget};
use sim_core::sound::Quantity;

/// The three widget kinds, by their i18n key — the combo's options and the
/// argument [`as_kind`] takes.
const KINDS: [&str; 3] = ["disp-widget-label", "disp-widget-value", "disp-widget-bar"];

pub fn panel(ui: &mut egui::Ui, editor: &mut Editor) {
    let names: Vec<String> = editor.nodes.iter().map(|n| n.name.clone()).collect();
    let mut changed = false;

    if ui
        .button(t!("action-add-display"))
        .on_hover_text(t!("action-add-display-hint"))
        .clicked()
    {
        editor.model_mut().displays.push(DisplaySpec {
            name: String::new(),
            node: names.first().cloned().unwrap_or_default(),
            width: 256,
            height: 160,
            widgets: Vec::new(),
            html: None,
        });
        changed = true;
    }
    ui.add_space(space::XS);

    let mut remove = None;
    if let Some(model) = editor.spec.model.as_mut() {
        for (i, display) in model.displays.iter_mut().enumerate() {
            let missing = !names.iter().any(|n| n == &display.node);
            editor_ui::card_frame().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    // The bound node is the card's identity, like on a part card.
                    node_combo(ui, i, &mut display.node, &names, missing, &mut changed);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("×").clicked() {
                            remove = Some(i);
                        }
                    });
                });
                editor_ui::form_grid(&format!("display-{i}")).show(ui, |ui| {
                    row(ui, "disp-name", |ui| {
                        changed |= ui.text_edit_singleline(&mut display.name).changed();
                    });
                    row(ui, "disp-size", |ui| {
                        ui.spacing_mut().interact_size.x = 64.0;
                        changed |= ui
                            .add(egui::DragValue::new(&mut display.width).range(16..=1024))
                            .changed();
                        ui.label(
                            egui::RichText::new("×")
                                .small()
                                .color(colors::TEXT_SECONDARY),
                        );
                        changed |= ui
                            .add(egui::DragValue::new(&mut display.height).range(16..=1024))
                            .changed();
                    });
                    // Optional HTML content path (plan ch. 12): a path below
                    // `mods/`; an empty field means the display keeps its
                    // widget or script content.
                    row(ui, "disp-html", |ui| {
                        let mut html = display.html.clone().unwrap_or_default();
                        if ui
                            .text_edit_singleline(&mut html)
                            .on_hover_text(t!("disp-html-hint"))
                            .changed()
                        {
                            display.html = (!html.is_empty()).then_some(html);
                            changed = true;
                        }
                    });
                });
                widgets(ui, i, display, &mut changed);
            });
        }
        if let Some(i) = remove {
            model.displays.remove(i);
            changed = true;
        }
    }
    editor.dirty |= changed;
}

/// The glTF node the texture is rendered onto. A binding whose node the
/// current model no longer has is drawn in red — the screen would silently
/// stay dark in the simulator.
fn node_combo(
    ui: &mut egui::Ui,
    id: usize,
    node: &mut String,
    names: &[String],
    missing: bool,
    changed: &mut bool,
) {
    let selected = egui::RichText::new(node.as_str()).monospace();
    let selected = if missing {
        selected.color(colors::ERROR)
    } else {
        selected
    };
    let width = ui.available_width() - 40.0;
    let response = egui::ComboBox::from_id_salt(("display-node", id))
        .selected_text(selected)
        .width(width)
        .truncate()
        .show_ui(ui, |ui| {
            for name in names {
                if ui
                    .selectable_label(node == name, egui::RichText::new(name).monospace())
                    .clicked()
                {
                    *node = name.clone();
                    *changed = true;
                }
            }
        })
        .response
        .on_hover_text(t!("disp-node"));
    if missing {
        response.on_hover_text(t!("part-node-missing-hint"));
    }
}

// --- The widget list -------------------------------------------------------

/// One deferred edit of the widget list — collected while the list is borrowed
/// for drawing, applied once it is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edit {
    Remove(usize),
    /// Swap with a neighbour. Order is depth: a later widget draws over an
    /// earlier one, and this is the only control over what covers what.
    Swap(usize, usize),
}

fn apply(list: &mut Vec<Widget>, edit: Edit) {
    match edit {
        Edit::Remove(i) => {
            list.remove(i);
        }
        Edit::Swap(a, b) => list.swap(a, b),
    }
}

/// Preview, one card per widget, and the way to add one.
fn widgets(ui: &mut egui::Ui, id: usize, display: &mut DisplaySpec, changed: &mut bool) {
    editor_ui::subheading(ui, t!("disp-widget-list"));
    // An HTML page draws the screen alone, so a widget list kept beside it
    // never reaches the texture. Say so rather than offer an editor for it.
    if display.html.as_deref().is_some_and(|p| !p.is_empty()) {
        ui.label(
            egui::RichText::new(t!("disp-html-overrides"))
                .small()
                .color(colors::WARN),
        );
        ui.label(
            egui::RichText::new(t!("disp-widget-count", count = display.widgets.len()))
                .small()
                .color(colors::TEXT_SECONDARY),
        );
        return;
    }

    preview(ui, id, display, changed);

    let mut edit = None;
    let count = display.widgets.len();
    for (i, widget) in display.widgets.iter_mut().enumerate() {
        editor_ui::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            // The header is exactly as wide as the form below it, and the
            // controls are placed from its right edge with the title taking
            // what is left. Sizing it to the card instead would move the
            // controls with the card's widest row, so no two cards would line
            // up — and a source name as long as "Hauptluftleitung [bar]" would
            // push them off the edge.
            let header = space::LABEL_COL + space::M + space::FIELD;
            ui.allocate_ui_with_layout(
                egui::vec2(header, ui.spacing().interact_size.y),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    if ui.small_button("×").clicked() {
                        edit = Some(Edit::Remove(i));
                    }
                    if ui
                        .add_enabled(i + 1 < count, egui::Button::new("↓").small())
                        .on_hover_text(t!("action-widget-down"))
                        .clicked()
                    {
                        edit = Some(Edit::Swap(i, i + 1));
                    }
                    if ui
                        .add_enabled(i > 0, egui::Button::new("↑").small())
                        .on_hover_text(t!("action-widget-up"))
                        .clicked()
                    {
                        edit = Some(Edit::Swap(i, i - 1));
                    }
                    ui.add(
                        egui::Label::new(egui::RichText::new(title(widget)).color(colors::TEXT))
                            .truncate(),
                    );
                },
            );
            editor_ui::form_grid(&format!("disp-widget-{id}-{i}")).show(ui, |ui| {
                row(ui, "disp-widget-kind", |ui| {
                    kind_combo(ui, (id, i), widget, changed);
                });
                fields(ui, (id, i), widget, changed);
            });
        });
    }
    if let Some(edit) = edit {
        apply(&mut display.widgets, edit);
        *changed = true;
    }
    if count == 0 {
        ui.label(
            egui::RichText::new(t!("disp-widgets-empty"))
                .small()
                .color(colors::TEXT_SECONDARY),
        );
    }
    ui.add_space(space::XS);
    if ui
        .button(t!("action-add-widget"))
        .on_hover_text(t!("action-add-widget-hint"))
        .clicked()
    {
        display.widgets.push(blank(count, display.height));
        *changed = true;
    }
}

/// A fresh widget: the speed, in the colour a screen is usually drawn in, one
/// line below the last one. A widget that starts out empty and transparent
/// looks broken until three more fields have been filled in, and one that
/// starts where the previous one sits cannot be told from it.
fn blank(count: usize, height: u32) -> Widget {
    Widget::Value {
        x: 8.0,
        y: (8 + count as u32 * 20).min(height.saturating_sub(20)) as f32,
        size: 16.0,
        source: DisplaySource::Quantity(Quantity::Speed),
        decimals: 0,
        unit: "km/h".into(),
        scale: 1.0,
        color: [1.0, 1.0, 1.0, 1.0],
    }
}

/// Heading of a widget card — what the widget is about, the way the bound node
/// heads a display card. A label is its text, a value or a bar its source.
fn title(widget: &Widget) -> String {
    match widget {
        Widget::Label { text, .. } if !text.is_empty() => text.clone(),
        Widget::Label { .. } => t!("disp-widget-untitled"),
        Widget::Value { source, .. } | Widget::Bar { source, .. } => match source {
            DisplaySource::Quantity(q) => t!(q.key()),
            DisplaySource::Indicator(name) if !name.is_empty() => name.clone(),
            DisplaySource::Indicator(_) => t!("disp-source-indicator"),
        },
    }
}

// --- Kind and fields -------------------------------------------------------

fn kind_key(widget: &Widget) -> &'static str {
    match widget {
        Widget::Label { .. } => KINDS[0],
        Widget::Value { .. } => KINDS[1],
        Widget::Bar { .. } => KINDS[2],
    }
}

/// Position and colour, the three fields every variant carries.
fn common(widget: &Widget) -> (f32, f32, [f32; 4]) {
    match widget {
        Widget::Label { x, y, color, .. }
        | Widget::Value { x, y, color, .. }
        | Widget::Bar { x, y, color, .. } => (*x, *y, *color),
    }
}

fn position_mut(widget: &mut Widget) -> (&mut f32, &mut f32) {
    match widget {
        Widget::Label { x, y, .. } | Widget::Value { x, y, .. } | Widget::Bar { x, y, .. } => {
            (x, y)
        }
    }
}

fn color_mut(widget: &mut Widget) -> &mut [f32; 4] {
    match widget {
        Widget::Label { color, .. } | Widget::Value { color, .. } | Widget::Bar { color, .. } => {
            color
        }
    }
}

/// Height of the drawn glyphs, in texture pixels. A bar has no font, so it
/// lends its own height — that keeps the figure across a kind switch.
fn text_size(widget: &Widget) -> f32 {
    match widget {
        Widget::Label { size, .. } | Widget::Value { size, .. } => *size,
        Widget::Bar { h, .. } => *h,
    }
}

/// Rebuilds a widget as `kind`, keeping what the variants share: position,
/// colour, glyph height, and the source between the two value-driven ones.
/// The kind says how a widget is drawn, not what it is about — the user picked
/// the rest for a reason, the same rule the sound trigger combo follows.
fn as_kind(widget: &Widget, kind: &str) -> Widget {
    let (x, y, color) = common(widget);
    let size = text_size(widget);
    let source = match widget {
        Widget::Value { source, .. } | Widget::Bar { source, .. } => source.clone(),
        Widget::Label { .. } => DisplaySource::Quantity(Quantity::Speed),
    };
    match kind {
        "disp-widget-value" => Widget::Value {
            x,
            y,
            size,
            source,
            decimals: 0,
            unit: String::new(),
            scale: 1.0,
            color,
        },
        "disp-widget-bar" => Widget::Bar {
            x,
            y,
            w: 80.0,
            h: size,
            source,
            max: 100.0,
            color,
        },
        _ => Widget::Label {
            x,
            y,
            size,
            text: String::new(),
            color,
        },
    }
}

fn kind_combo(ui: &mut egui::Ui, id: (usize, usize), widget: &mut Widget, changed: &mut bool) {
    let current = kind_key(widget);
    egui::ComboBox::from_id_salt(("disp-widget-kind", id))
        .selected_text(t!(current))
        .width(space::FIELD)
        .show_ui(ui, |ui| {
            for kind in KINDS {
                if ui.selectable_label(current == kind, t!(kind)).clicked() && kind != current {
                    let next = as_kind(widget, kind);
                    *widget = next;
                    *changed = true;
                }
            }
        });
}

/// The rows a widget needs beyond its kind. Position and colour bracket the
/// variant's own fields, so they keep one place in the form whichever kind is
/// selected.
fn fields(ui: &mut egui::Ui, id: (usize, usize), widget: &mut Widget, changed: &mut bool) {
    row(ui, "disp-widget-pos", |ui| {
        ui.spacing_mut().interact_size.x = 64.0;
        let (x, y) = position_mut(widget);
        for (name, value) in [("X", x), ("Y", y)] {
            ui.label(
                egui::RichText::new(name)
                    .small()
                    .color(colors::TEXT_SECONDARY),
            );
            *changed |= ui
                .add(egui::DragValue::new(value).speed(1.0).range(0.0..=4096.0))
                .changed();
        }
    });
    match widget {
        Widget::Label { size, text, .. } => {
            row(ui, "disp-widget-text", |ui| {
                *changed |= ui
                    .add(egui::TextEdit::singleline(text).desired_width(space::FIELD))
                    .changed();
            });
            row(ui, "disp-widget-size", |ui| {
                *changed |= field(ui, size, 0.5, 1.0..=512.0, "px").changed();
            });
        }
        Widget::Value {
            size,
            source,
            decimals,
            unit,
            scale,
            ..
        } => {
            row(ui, "disp-widget-source", |ui| {
                *changed |= source_combo(ui, id, source);
            });
            indicator_row(ui, source, changed);
            row(ui, "disp-widget-size", |ui| {
                *changed |= field(ui, size, 0.5, 1.0..=512.0, "px").changed();
            });
            row(ui, "disp-widget-decimals", |ui| {
                *changed |= field(ui, decimals, 1.0, 0.0..=6.0, "").changed();
            });
            row(ui, "disp-widget-unit", |ui| {
                *changed |= ui
                    .add(egui::TextEdit::singleline(unit).desired_width(space::FIELD))
                    .changed();
            });
            row(ui, "disp-widget-scale", |ui| {
                *changed |= field(ui, scale, 0.01, -1e6..=1e6, "").changed();
            });
        }
        Widget::Bar {
            w, h, source, max, ..
        } => {
            row(ui, "disp-widget-source", |ui| {
                *changed |= source_combo(ui, id, source);
            });
            indicator_row(ui, source, changed);
            row(ui, "disp-widget-box", |ui| {
                ui.spacing_mut().interact_size.x = 64.0;
                *changed |= ui
                    .add(egui::DragValue::new(w).speed(1.0).range(1.0..=4096.0))
                    .changed();
                ui.label(
                    egui::RichText::new("×")
                        .small()
                        .color(colors::TEXT_SECONDARY),
                );
                *changed |= ui
                    .add(egui::DragValue::new(h).speed(1.0).range(1.0..=4096.0))
                    .changed();
            });
            row(ui, "disp-widget-max", |ui| {
                *changed |= field(ui, max, 0.1, 0.0..=1e6, "").changed();
            });
        }
    }
    row(ui, "disp-widget-color", |ui| {
        *changed |= color_button(ui, color_mut(widget));
    });
}

/// Where the widget reads its value: a sound-table quantity — including the
/// cab control positions — or a named indicator of the train protection.
///
/// The quantity list is the one `sounds.rs` offers; it is spelled out again
/// here because that combo is private to its module.
fn source_combo(ui: &mut egui::Ui, id: (usize, usize), source: &mut DisplaySource) -> bool {
    let mut changed = false;
    let selected = match source {
        DisplaySource::Quantity(q) => t!(q.key()),
        DisplaySource::Indicator(_) => t!("disp-source-indicator"),
    };
    // Switching to an indicator keeps the name that is already typed.
    let indicator = match source {
        DisplaySource::Indicator(name) => name.clone(),
        DisplaySource::Quantity(_) => String::new(),
    };
    egui::ComboBox::from_id_salt(("disp-widget-source", id))
        .selected_text(selected)
        .width(space::FIELD)
        .truncate()
        .show_ui(ui, |ui| {
            let mut pick = |ui: &mut egui::Ui, option: DisplaySource, key: &str| {
                if ui.selectable_label(*source == option, t!(key)).clicked() {
                    *source = option;
                    changed = true;
                }
            };
            for quantity in Quantity::ALL {
                pick(ui, DisplaySource::Quantity(quantity), quantity.key());
            }
            ui.separator();
            for control in CabControl::ALL {
                let quantity = Quantity::Control(control);
                pick(ui, DisplaySource::Quantity(quantity), quantity.key());
            }
            ui.separator();
            pick(
                ui,
                DisplaySource::Indicator(indicator),
                "disp-source-indicator",
            );
        });
    changed
}

/// The indicator's name — free text, because which ones exist depends on the
/// train protection the vehicle carries and a mod may add its own.
fn indicator_row(ui: &mut egui::Ui, source: &mut DisplaySource, changed: &mut bool) {
    let DisplaySource::Indicator(name) = source else {
        return;
    };
    row(ui, "disp-widget-indicator", |ui| {
        *changed |= ui
            .add(
                egui::TextEdit::singleline(name)
                    .desired_width(space::FIELD)
                    .hint_text(t!("disp-widget-indicator-placeholder")),
            )
            .changed();
    });
}

/// The colour picker. Both sides are linear RGBA, so nothing goes through
/// sRGB or a byte; the value is written back only when the picker was actually
/// moved, so an untouched widget keeps its numbers exactly as the file has
/// them.
fn color_button(ui: &mut egui::Ui, color: &mut [f32; 4]) -> bool {
    let mut rgba = egui::Rgba::from_rgba_unmultiplied(color[0], color[1], color[2], color[3]);
    // The swatch takes its size from `interact_size`, which is narrower than a
    // field — set it, or the colour row alone breaks the column's right edge.
    ui.spacing_mut().interact_size.x = space::FIELD;
    let changed = egui::color_picker::color_edit_button_rgba(
        ui,
        &mut rgba,
        egui::color_picker::Alpha::OnlyBlend,
    )
    .changed();
    if changed {
        *color = rgba.to_rgba_unmultiplied();
    }
    changed
}

// --- Preview ---------------------------------------------------------------

/// The screen, to scale, with its widgets where they will sit.
///
/// A layout built from four numbers per widget is otherwise built blind — the
/// same reason a sound card carries a ▶. It is a preview of the *layout*: the
/// values are placeholders, because what a widget reads is simulation state
/// and the editor has no train. Dragging a widget writes back into its x/y.
fn preview(ui: &mut egui::Ui, id: usize, display: &mut DisplaySpec, changed: &mut bool) {
    let (width, height) = (display.width.max(1) as f32, display.height.max(1) as f32);
    // Scale down to fit the panel, never up: the preview is measured in the
    // texture's own pixels, and past 1:1 it would promise a sharpness the
    // texture does not have.
    let scale = (ui.available_width() / width).clamp(0.05, 1.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height) * scale, egui::Sense::hover());
    let corner = egui::CornerRadius::same(4);
    let painter = ui.painter_at(rect);
    // The simulator clears a display to black before the widgets go on.
    painter.rect_filled(rect, corner, egui::Color32::BLACK);
    painter.rect_stroke(
        rect,
        corner,
        egui::Stroke::new(1.0, colors::BORDER_SUBTLE),
        egui::StrokeKind::Inside,
    );

    let mut moved = None;
    for (i, widget) in display.widgets.iter().enumerate() {
        let (x, y, color) = common(widget);
        let origin = rect.min + egui::vec2(x, y) * scale;
        let color = to_color32(color);
        let drawn = match widget {
            Widget::Bar { w, h, .. } => {
                let box_rect = egui::Rect::from_min_size(origin, egui::vec2(*w, *h) * scale);
                // Half full — an empty outline says nothing about how a bar
                // reads when the value moves.
                painter.rect_filled(
                    egui::Rect::from_min_size(
                        box_rect.min,
                        egui::vec2(box_rect.width() * 0.5, box_rect.height()),
                    ),
                    egui::CornerRadius::ZERO,
                    color,
                );
                painter.rect_stroke(
                    box_rect,
                    egui::CornerRadius::ZERO,
                    egui::Stroke::new(1.0, color),
                    egui::StrokeKind::Inside,
                );
                box_rect
            }
            _ => painter.text(
                origin,
                egui::Align2::LEFT_TOP,
                preview_text(widget),
                egui::FontId::proportional((text_size(widget) * scale).max(1.0)),
                color,
            ),
        };
        // A handle small enough to miss is a widget that cannot be placed —
        // an empty label draws nothing at all.
        let handle = egui::Rect::from_min_size(drawn.min, drawn.size().max(egui::vec2(12.0, 10.0)));
        let response = ui
            .interact(
                handle.intersect(rect),
                ui.id().with(("disp-widget-drag", id, i)),
                egui::Sense::drag(),
            )
            .on_hover_cursor(egui::CursorIcon::Grab);
        if response.hovered() || response.dragged() {
            painter.rect_stroke(
                drawn.expand(1.0),
                egui::CornerRadius::ZERO,
                egui::Stroke::new(1.0, colors::ACCENT),
                egui::StrokeKind::Outside,
            );
        }
        if response.dragged() {
            moved = Some((i, response.drag_delta() / scale));
        }
    }
    if let Some((i, delta)) = moved.filter(|(_, d)| *d != egui::Vec2::ZERO) {
        move_widget(
            &mut display.widgets[i],
            delta,
            display.width,
            display.height,
        );
        *changed = true;
    }
    ui.label(
        egui::RichText::new(t!("disp-preview-note"))
            .small()
            .color(colors::TEXT_SECONDARY),
    );
}

/// What a widget draws in the preview. A value shows eights rather than a
/// reading — they are the widest digits, so the box is the one the layout has
/// to make room for, and nothing suggests the editor knows the train's speed.
fn preview_text(widget: &Widget) -> String {
    match widget {
        Widget::Label { text, .. } => text.clone(),
        Widget::Value { decimals, unit, .. } => {
            let mut text = "888".to_owned();
            if *decimals > 0 {
                text.push('.');
                text.extend(std::iter::repeat_n('8', usize::from(*decimals)));
            }
            if !unit.is_empty() {
                text.push(' ');
                text.push_str(unit);
            }
            text
        }
        Widget::Bar { .. } => String::new(),
    }
}

/// Moves a widget by a texture-pixel delta, keeping its origin on the screen.
/// Dragged over the edge it would be invisible in the simulator *and* out of
/// reach of the preview that could drag it back.
fn move_widget(widget: &mut Widget, delta: egui::Vec2, width: u32, height: u32) {
    let (x, y) = position_mut(widget);
    *x = (*x + delta.x).clamp(0.0, width.saturating_sub(1) as f32);
    *y = (*y + delta.y).clamp(0.0, height.saturating_sub(1) as f32);
}

fn to_color32(color: [f32; 4]) -> egui::Color32 {
    egui::Rgba::from_rgba_unmultiplied(color[0], color[1], color[2], color[3]).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn label(text: &str) -> Widget {
        Widget::Label {
            x: 12.0,
            y: 34.0,
            size: 18.0,
            text: text.to_owned(),
            color: [0.2, 0.4, 0.6, 0.8],
        }
    }

    /// The kind combo changes how a widget is drawn, not where it sits or what
    /// colour it has. Retyping those after every switch is exactly what the
    /// editor exists to spare.
    #[test]
    fn switching_kind_keeps_position_and_colour() {
        let start = label("Vmax");
        let mut widget = start.clone();
        for kind in KINDS {
            widget = as_kind(&widget, kind);
            assert_eq!(kind_key(&widget), kind);
            assert_eq!(common(&widget), common(&start), "{kind}");
        }
    }

    /// A source picked for a value must survive the step to a bar — the two
    /// are two pictures of the same quantity.
    #[test]
    fn switching_kind_keeps_the_source() {
        let value = as_kind(&label("x"), "disp-widget-value");
        let Widget::Value { .. } = value else {
            panic!("not a value: {value:?}");
        };
        let mut named = value.clone();
        if let Widget::Value { source, .. } = &mut named {
            *source = DisplaySource::Indicator("mfa_v_soll".into());
        }
        let bar = as_kind(&named, "disp-widget-bar");
        let Widget::Bar { source, .. } = &bar else {
            panic!("not a bar: {bar:?}");
        };
        assert_eq!(*source, DisplaySource::Indicator("mfa_v_soll".into()));
    }

    /// Order is depth, so a click on ↑ or × has to reach the widget it sits
    /// next to and no other.
    #[test]
    fn reordering_moves_the_widget_that_was_clicked() {
        let mut list = vec![label("a"), label("b"), label("c")];
        let titles = |list: &[Widget]| list.iter().map(title).collect::<Vec<_>>();

        apply(&mut list, Edit::Swap(2, 1));
        assert_eq!(titles(&list), ["a", "c", "b"]);
        apply(&mut list, Edit::Swap(0, 1));
        assert_eq!(titles(&list), ["c", "a", "b"]);
        apply(&mut list, Edit::Remove(1));
        assert_eq!(titles(&list), ["c", "b"]);
    }

    /// A widget dragged past the edge of the screen is invisible in the
    /// simulator and out of reach of the preview that would drag it back.
    #[test]
    fn a_dragged_widget_stays_on_the_screen() {
        let mut widget = label("Vmax");
        move_widget(&mut widget, egui::vec2(9_000.0, 9_000.0), 256, 160);
        let (x, y, _) = common(&widget);
        assert_eq!((x, y), (255.0, 159.0));

        move_widget(&mut widget, egui::vec2(-9_000.0, -9_000.0), 256, 160);
        assert_eq!(common(&widget).0, 0.0);
        assert_eq!(common(&widget).1, 0.0);
    }

    /// The file stores straight linear RGBA, the picker premultiplied linear.
    /// Going through sRGB or a byte instead — `color_edit_button_srgba` — would
    /// quantise every colour that merely passes under the cursor.
    #[test]
    fn the_colour_survives_the_picker() {
        let color = [0.2, 0.4, 0.6, 0.8];
        let back = egui::Rgba::from_rgba_unmultiplied(color[0], color[1], color[2], color[3])
            .to_rgba_unmultiplied();
        for (a, b) in back.iter().zip(color) {
            assert!((a - b).abs() < 1e-6, "{back:?}");
        }
    }
}
