//! Marked track areas: the list, and the properties of the selected one.
//!
//! A step profile per property is right for a compiler and wrong for a person. Marking a
//! stretch of track and giving it properties is the same job the way a builder thinks
//! about it — "this is the station, it runs at 40, it is not electrified" — and it stays
//! one thing afterwards, so changing it means changing one thing.
//!
//! Everything here edits `LineSource::areas`; `content::route` bakes them down into the
//! same step profiles the tracks have always carried, so nothing downstream knows.

use crate::tools::{self, EditorState, Selection, Tool};
use crate::ui::{power_label, row};
use crate::{Focus, Line, TrackTypes};
use bevy_egui::egui;
use content::route::TrackAreaSource;
use editor_ui::{colors, space};
use i18n::t;
use world_coords::EcefPos;

/// Middle of the first stretch an area covers — where the list jumps to.
fn area_position(line: &Line, area: &TrackAreaSource) -> Option<EcefPos> {
    let span = area.spans.first()?;
    let edge = line.net.edges().get(span.edge as usize)?;
    let s = ((span.from + span.to) / 2.0).clamp(0.0, edge.length());
    Some(edge.eval(s).pos)
}

/// The list of marked areas: each with its colour, its name and how much track it covers.
/// This is where an area is found again once the map has been panned somewhere else.
pub fn area_list(ui: &mut egui::Ui, line: &mut Line, state: &mut EditorState, focus: &mut Focus) {
    if line.source.areas.is_empty() {
        ui.small(t!("sel-area-list-empty"));
    }
    let positions: Vec<Option<EcefPos>> = line
        .source
        .areas
        .iter()
        .map(|area| area_position(line, area))
        .collect();
    // A grid, not one `horizontal` per row: the names and lengths then sit in
    // columns whatever width the colour square and the name happen to have.
    editor_ui::form_grid("area-list")
        .num_columns(3)
        // The 84 px widget minimum would hold the colour-square column open
        // that wide.
        .min_col_width(0.0)
        .show(ui, |ui| {
            for (i, area) in line.source.areas.iter().enumerate() {
                ui.label(
                    egui::RichText::new("\u{25a0}").color(egui::Color32::from_rgb(
                        (area.color.0.clamp(0.0, 1.0) * 255.0) as u8,
                        (area.color.1.clamp(0.0, 1.0) * 255.0) as u8,
                        (area.color.2.clamp(0.0, 1.0) * 255.0) as u8,
                    )),
                );
                let here = state.selection == Selection::TrackArea(i);
                let mut label = egui::RichText::new(if area.name.is_empty() {
                    t!("area-unnamed")
                } else {
                    area.name.clone()
                });
                if here {
                    label = label.color(colors::TEXT_STRONG);
                }
                if ui.selectable_label(here, label).clicked() {
                    state.selection = Selection::TrackArea(i);
                    state.jump_to = Some("selection");
                    if let Some(Some(p)) = positions.get(i) {
                        focus.position = *p;
                    }
                }
                ui.label(
                    egui::RichText::new(t!(
                        "sel-area-list-covers",
                        length = format!("{:.0}", area.length())
                    ))
                    .small()
                    .color(colors::TEXT_SECONDARY),
                );
                ui.end_row();
            }
        });
    if ui
        .small_button(t!("action-add-area"))
        .on_hover_text(t!("action-add-area-hint"))
        .clicked()
    {
        state.tool = Tool::MarkArea;
        state.selection = Selection::None;
        state.area_stroke = None;
    }
}

/// The selected area: name, colour, the properties it sets and the stretches it covers.
///
/// Every property is a checkbox and a value. Unticked means the area says nothing about
/// it and leaves whatever lies underneath alone — which is what lets a speed restriction
/// run across an electrification boundary without disturbing the wire.
pub fn area_rows(
    ui: &mut egui::Ui,
    line: &mut Line,
    index: usize,
    types: &TrackTypes,
    focus: &mut Focus,
    state: &mut EditorState,
) {
    let known: Vec<String> = types.map.keys().cloned().collect();
    let lengths: Vec<f64> = line
        .net
        .edges()
        .iter()
        .map(track_model::TrackEdge::length)
        .collect();
    let jump = line
        .source
        .areas
        .get(index)
        .and_then(|area| area_position(line, area));
    let Some(area) = line.source.areas.get_mut(index) else {
        return;
    };

    ui.label(t!(
        "sel-area-summary",
        spans = area.spans.len(),
        length = format!("{:.0}", area.length())
    ));
    if !area.sets_anything() {
        ui.small(t!("sel-area-sets-nothing"));
    }
    editor_ui::form_grid("sel-area").show(ui, |ui| {
        row(ui, "area-name", |ui| {
            ui.add(egui::TextEdit::singleline(&mut area.name).desired_width(space::FIELD));
        });
        row(ui, "area-color", |ui| {
            let mut rgb = [area.color.0, area.color.1, area.color.2];
            if egui::color_picker::color_edit_button_rgb(ui, &mut rgb).changed() {
                area.color = (rgb[0], rgb[1], rgb[2]);
            }
        });
        row(ui, "area-width", |ui| {
            editor_ui::field(ui, &mut area.width, 0.1, 0.5..=20.0, "m");
        });
    });

    editor_ui::subheading(ui, t!("sel-area-properties"));
    editor_ui::form_grid("sel-area-props")
        .num_columns(3)
        .show(ui, |ui| {
            optional_number(
                ui,
                "area-speed",
                &mut area.speed,
                160.0,
                0.0..=400.0,
                "km/h",
            );
            optional_number(ui, "area-cant", &mut area.cant, 0.0, -200.0..=200.0, "mm");
            optional_number(
                ui,
                "area-grade",
                &mut area.grade,
                0.0,
                -70.0..=70.0,
                "\u{2030}",
            );
            track_type_row(ui, index, &mut area.track_type, types, &known);
            power_row(ui, index, &mut area.electrification);
        });

    editor_ui::subheading(ui, t!("sel-area-spans"));
    if area.spans.is_empty() {
        ui.small(t!("sel-area-no-spans"));
    }
    let mut remove = None;
    editor_ui::form_grid(&format!("area-spans-{index}"))
        .num_columns(4)
        .show(ui, |ui| {
            for (k, span) in area.spans.iter_mut().enumerate() {
                let length = lengths.get(span.edge as usize).copied().unwrap_or(0.0);
                ui.label(t!("sel-area-span-track", index = span.edge));
                editor_ui::field(ui, &mut span.from, 10.0, 0.0..=length, "m")
                    .on_hover_text(t!("sel-area-span-from"));
                editor_ui::field(ui, &mut span.to, 10.0, 0.0..=length, "m")
                    .on_hover_text(t!("sel-area-span-to"));
                if ui.small_button("\u{d7}").clicked() {
                    remove = Some(k);
                }
                ui.end_row();
            }
        });
    if let Some(k) = remove {
        area.spans.remove(k);
    }

    ui.add_space(space::XS);
    ui.horizontal(|ui| {
        if ui
            .button(t!("action-mark-more"))
            .on_hover_text(t!("action-mark-more-hint"))
            .clicked()
        {
            state.tool = Tool::MarkArea;
            state.area_stroke = None;
        }
        if let Some(p) = jump
            && ui.button(t!("action-center")).clicked()
        {
            focus.position = p;
        }
        if ui.button(t!("action-delete")).clicked() {
            tools::delete_selection(line, state);
        }
    });
}

/// Track type: model and texture of the superstructure.
fn track_type_row(
    ui: &mut egui::Ui,
    index: usize,
    value: &mut Option<String>,
    types: &TrackTypes,
    known: &[String],
) {
    let mut on = value.is_some();
    // Nothing to switch the superstructure to while no installed mod defines a
    // track type — there is no built-in one to offer.
    ui.add_enabled(!known.is_empty(), egui::Checkbox::without_text(&mut on))
        .on_hover_text(t!("area-set-hint"))
        .on_disabled_hover_text(t!("track-type-none-installed"));
    ui.label(t!("area-track-type"));
    if on && value.is_none() {
        *value = known.first().cloned();
    } else if !on {
        *value = None;
    }
    match value {
        None => {
            ui.small(t!("area-unset"));
        }
        Some(name) => {
            let mut text = egui::RichText::new(name.clone());
            if !types.map.contains_key(name.as_str()) {
                // A name no installed mod answers — visible before the run.
                text = text.color(colors::ERROR);
            }
            egui::ComboBox::from_id_salt(("area-type", index))
                .width(space::FIELD)
                .selected_text(text)
                .show_ui(ui, |ui| {
                    for entry in known {
                        if ui.selectable_label(name == entry, entry).clicked() {
                            *name = entry.clone();
                        }
                    }
                });
        }
    }
    ui.end_row();
}

/// What the area hangs over the track, if it says anything about it at all.
fn power_row(ui: &mut egui::Ui, index: usize, value: &mut Option<String>) {
    let mut on = value.is_some();
    ui.checkbox(&mut on, "").on_hover_text(t!("area-set-hint"));
    ui.label(t!("sel-power"));
    if on && value.is_none() {
        *value = Some(track_model::PowerSystem::Ac15kv.id().into());
    } else if !on {
        *value = None;
    }
    match value {
        None => {
            ui.small(t!("area-unset"));
        }
        Some(id) => {
            let current = track_model::electrification_from_id(id);
            egui::ComboBox::from_id_salt(("area-power", index))
                .width(space::FIELD)
                .selected_text(power_label(current))
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(current.is_none(), power_label(None))
                        .clicked()
                    {
                        *id = "none".into();
                    }
                    for system in track_model::PowerSystem::ALL {
                        if ui
                            .selectable_label(current == Some(system), power_label(Some(system)))
                            .clicked()
                        {
                            *id = system.id().into();
                        }
                    }
                });
        }
    }
    ui.end_row();
}

/// A property that may be left unset: a checkbox and, where it is ticked, the value.
fn optional_number(
    ui: &mut egui::Ui,
    key: &str,
    value: &mut Option<f64>,
    default: f64,
    range: std::ops::RangeInclusive<f64>,
    unit: &'static str,
) {
    let mut on = value.is_some();
    ui.checkbox(&mut on, "").on_hover_text(t!("area-set-hint"));
    ui.label(t!(key));
    if on && value.is_none() {
        *value = Some(default);
    } else if !on {
        *value = None;
    }
    match value {
        Some(v) => {
            editor_ui::field(ui, v, 1.0, range, unit);
        }
        None => {
            ui.small(t!("area-unset"));
        }
    }
    ui.end_row();
}
