//! Metadata, variants and loads section of the data panel (plan ch. 15.2).
//!
//! Who built the vehicle, which liveries and running numbers it comes in, and
//! what it can carry. None of it reaches the physics — it is what a vehicle
//! browser lists and what the train composer draws from.

use crate::ui::row;
use bevy_egui::egui;
use editor_ui::{colors, field, space};
use i18n::t;
use sim_core::train::{LoadSpec, VehicleSpec, VehicleVariant};

/// The countries the project has a train protection and signal package for.
/// Everything else is typed in beside the combo as its own ISO 3166-1 alpha-2
/// code — a free-text field on its own is where `Deutschland` ends up in the
/// file, and no browser filter finds the vehicle again.
const COUNTRIES: [&str; 3] = ["DE", "AT", "CH"];

/// Width of the free-text country code beside the combo — two letters.
const CODE_W: f32 = 44.0;

pub fn panel(
    ui: &mut egui::Ui,
    spec: &mut VehicleSpec,
    nodes: &[String],
    window: Option<&bevy::window::RawHandleWrapper>,
) {
    data_sheet(ui, spec, window);
    variants(ui, spec);
    loads(ui, spec, nodes);
}

/// What the data sheet says: class, builder, era, owner.
fn data_sheet(
    ui: &mut egui::Ui,
    spec: &mut VehicleSpec,
    window: Option<&bevy::window::RawHandleWrapper>,
) {
    let meta = &mut spec.meta;
    editor_ui::form_grid("meta").show(ui, |ui| {
        row(ui, "meta-class", |ui| text(ui, &mut meta.class));
        row(ui, "meta-manufacturer", |ui| {
            text(ui, &mut meta.manufacturer)
        });
        row(ui, "meta-year", |ui| year(ui, &mut meta.build_year));
        row(ui, "meta-epoch", |ui| text(ui, &mut meta.epoch));
        row(ui, "meta-country", |ui| country(ui, &mut meta.country));
        row(ui, "meta-operator", |ui| text(ui, &mut meta.operator));
        row(ui, "meta-author", |ui| text(ui, &mut meta.author));
        row(ui, "meta-thumbnail", |ui| {
            thumbnail(ui, &mut meta.thumbnail, window)
        });
    });
    // Outside the grid: prose needs the whole panel width, not the field column.
    editor_ui::subheading(ui, t!("meta-description"));
    ui.add(
        egui::TextEdit::multiline(&mut meta.description)
            .desired_width(f32::INFINITY)
            .desired_rows(3)
            .hint_text(t!("meta-description-placeholder")),
    );
}

/// A plain text field at the shared field width, so the column has one right edge.
fn text(ui: &mut egui::Ui, value: &mut String) {
    ui.add(egui::TextEdit::singleline(value).desired_width(space::FIELD));
}

/// Year of building. 0 means "not stated" and must not read as the year 0 — a
/// vehicle built in antiquity is a wrong answer where the user gave none.
fn year(ui: &mut egui::Ui, value: &mut u16) {
    ui.spacing_mut().interact_size.x = space::FIELD;
    ui.add(
        egui::DragValue::new(value)
            .speed(1.0)
            .range(0..=2100)
            .custom_formatter(|v, _| {
                if v < 1.0 {
                    t!("meta-year-unset")
                } else {
                    format!("{v:.0}")
                }
            })
            .custom_parser(|text| text.trim().parse().ok()),
    );
}

/// Country of use. The combo carries the ones the simulator has signals and train
/// protection for; the field beside it takes the code of everything else.
fn country(ui: &mut egui::Ui, code: &mut String) {
    egui::ComboBox::from_id_salt("meta-country")
        .selected_text(if code.is_empty() {
            t!("meta-country-unset")
        } else {
            code.clone()
        })
        .width(space::FIELD - CODE_W - space::S)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(code.is_empty(), t!("meta-country-unset"))
                .clicked()
            {
                code.clear();
            }
            for option in COUNTRIES {
                if ui.selectable_label(code == option, option).clicked() {
                    *code = option.to_string();
                }
            }
        });
    let response = ui.add(
        egui::TextEdit::singleline(code)
            .char_limit(2)
            .desired_width(CODE_W)
            .hint_text(t!("meta-country-other")),
    );
    if response.changed() {
        *code = code.to_uppercase();
    }
}

/// Preview image, picked the way `import_model` picks a model: the dialog opens in
/// `mods/`, and a file outside it is refused — the simulator addresses it as
/// `mods://<mod>/…` and would never find it again.
fn thumbnail(
    ui: &mut egui::Ui,
    path: &mut String,
    window: Option<&bevy::window::RawHandleWrapper>,
) {
    ui.add(
        egui::TextEdit::singleline(path)
            .desired_width(space::FIELD - 28.0)
            .hint_text(t!("meta-thumbnail-placeholder")),
    );
    if ui
        .small_button("…")
        .on_hover_text(t!("meta-thumbnail-pick"))
        .clicked()
    {
        pick_thumbnail(path, window);
    }
}

fn pick_thumbnail(path: &mut String, window: Option<&bevy::window::RawHandleWrapper>) {
    // Owned by the editor window: on Windows a dialog without an owner is free to open
    // behind the editor, which is the one thing every other dialog site here avoids.
    let Some(picked) = crate::ui::file_dialog_for(window)
        .add_filter(t!("filter-image"), &["png", "jpg", "jpeg", "webp"])
        .set_directory(crate::mods_dir())
        .pick_file()
    else {
        return;
    };
    match picked.strip_prefix(crate::mods_dir()) {
        Ok(relative) => *path = relative.to_string_lossy().replace('\\', "/"),
        Err(_) => {
            rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Error)
                .set_title(t!("dialog-error-title"))
                .set_description(t!("status-outside-mods", path = picked.display()))
                .show();
        }
    }
}

/// The same vehicle in another dress: one card per livery or number series.
fn variants(ui: &mut egui::Ui, spec: &mut VehicleSpec) {
    editor_ui::subheading(ui, t!("var-heading"));
    if ui
        .button(t!("action-add-variant"))
        .on_hover_text(t!("action-add-variant-hint"))
        .clicked()
    {
        spec.variants.push(VehicleVariant::default());
    }
    if spec.variants.is_empty() {
        hint(ui, t!("var-empty"));
        return;
    }

    // The file each variant really draws with, taken before the list is borrowed —
    // `model_file` reads the vehicle's own model to fall back on. A change made in
    // the loop shows one frame later, which no one can see.
    let files: Vec<String> = (0..spec.variants.len())
        .map(|i| spec.model_file(Some(i)).unwrap_or_default().to_string())
        .collect();

    let mut remove = None;
    for (i, variant) in spec.variants.iter_mut().enumerate() {
        editor_ui::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut variant.name)
                        .desired_width(ui.available_width() - 30.0)
                        .hint_text(t!("var-name-placeholder")),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("×").clicked() {
                        remove = Some(i);
                    }
                });
            });
            editor_ui::form_grid(&format!("var-{i}")).show(ui, |ui| {
                row(ui, "var-model", |ui| {
                    optional_text(ui, &mut variant.model, "var-model-placeholder");
                });
                row(ui, "var-epoch", |ui| text(ui, &mut variant.epoch));
            });
            hint(
                ui,
                if files[i].is_empty() {
                    t!("var-model-none")
                } else {
                    t!("var-model-effective", file = files[i])
                },
            );
            editor_ui::subheading(ui, t!("var-numbers"));
            numbers_field(ui, i, &mut variant.numbers);
            editor_ui::subheading(ui, t!("var-description"));
            ui.add(
                egui::TextEdit::multiline(&mut variant.description)
                    .desired_width(f32::INFINITY)
                    .desired_rows(2)
                    .hint_text(t!("var-description-placeholder")),
            );
        });
    }
    if let Some(i) = remove {
        spec.variants.remove(i);
    }
}

/// The running numbers, one per line — thirty numbers are thirty lines, not
/// thirty text fields.
///
/// While the field has focus the text is what egui remembers, not what the list
/// says: rebuilding the list on every keystroke would swallow the blank line the
/// user just opened with Return, and take the cursor with it.
fn numbers_field(ui: &mut egui::Ui, id: usize, numbers: &mut Vec<String>) {
    let key = ui.make_persistent_id(("var-numbers", id));
    let mut buffer = ui
        .data(|d| d.get_temp::<String>(key))
        .unwrap_or_else(|| numbers_text(numbers));
    let response = ui
        .add(
            egui::TextEdit::multiline(&mut buffer)
                .desired_width(f32::INFINITY)
                .desired_rows(3)
                .hint_text(t!("var-numbers-placeholder")),
        )
        .on_hover_text(t!("var-numbers-hint"));
    if response.changed() {
        *numbers = parse_numbers(&buffer);
    }
    if response.has_focus() {
        ui.data_mut(|d| d.insert_temp(key, buffer));
    } else {
        // Off focus the list is the truth again, so an undo shows up in the field.
        ui.data_mut(|d| d.remove::<String>(key));
    }
}

/// The list as the field shows it: one number per line.
fn numbers_text(numbers: &[String]) -> String {
    numbers.join("\n")
}

/// And back. Blank lines and stray spaces are the user's formatting, not numbers.
fn parse_numbers(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(String::from)
        .collect()
}

/// A string that may be absent: an empty field is `None`, which is what "inherits"
/// means in the file. Not trimmed — a trim on every keystroke would eat the space
/// the user is in the middle of typing.
fn optional_text(ui: &mut egui::Ui, slot: &mut Option<String>, placeholder: &str) {
    let mut value = slot.clone().unwrap_or_default();
    let response = ui.add(
        egui::TextEdit::singleline(&mut value)
            .desired_width(space::FIELD)
            .hint_text(t!(placeholder)),
    );
    if response.changed() {
        *slot = (!value.is_empty()).then_some(value);
    }
}

/// What a wagon can carry: one card per goods, each with the node that shows it.
fn loads(ui: &mut egui::Ui, spec: &mut VehicleSpec, nodes: &[String]) {
    editor_ui::subheading(ui, t!("load-heading"));
    if ui
        .button(t!("action-add-load"))
        .on_hover_text(t!("action-add-load-hint"))
        .clicked()
    {
        spec.loads.push(LoadSpec::default());
    }
    if spec.loads.is_empty() {
        hint(ui, t!("load-empty"));
        return;
    }

    // Both read the whole spec, so they are taken before the list is borrowed.
    let totals: Vec<f64> = (0..spec.loads.len())
        .map(|i| spec.mass_with_load(Some(i)))
        .collect();
    let max_payload = spec.max_payload;

    let mut remove = None;
    for (i, load) in spec.loads.iter_mut().enumerate() {
        editor_ui::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut load.name)
                        .desired_width(ui.available_width() - 30.0)
                        .hint_text(t!("load-name-placeholder")),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("×").clicked() {
                        remove = Some(i);
                    }
                });
            });
            editor_ui::form_grid(&format!("load-{i}")).show(ui, |ui| {
                row(ui, "load-mass", |ui| {
                    field(ui, &mut load.mass, 100.0, 0.0..=500_000.0, "kg");
                });
                row(ui, "load-node", |ui| {
                    node_combo(ui, i, &mut load.node, nodes);
                });
            });
            hint(
                ui,
                t!("load-total", mass = i18n::decimal(totals[i] / 1000.0, 1)),
            );
            // What the vehicle really hauls, where the anscribed payload is smaller
            // than the load — otherwise 30 t are typed in and 20 t are pulled.
            if is_capped(load.mass, max_payload) {
                ui.label(
                    egui::RichText::new(t!(
                        "load-capped",
                        max = i18n::decimal(max_payload / 1000.0, 1)
                    ))
                    .small()
                    .color(colors::WARN),
                );
            }
        });
    }
    if let Some(i) = remove {
        spec.loads.remove(i);
    }
}

/// A load heavier than the anscribed payload is hauled only up to it —
/// [`VehicleSpec::payload`] caps it. A vehicle that anscribes no payload (0) hauls
/// whatever it is given.
fn is_capped(mass: f64, max_payload: f64) -> bool {
    max_payload > 0.0 && mass > max_payload
}

/// A quiet line under a card or a list: a derived value, never an input.
fn hint(ui: &mut egui::Ui, text: String) {
    ui.label(
        egui::RichText::new(text)
            .small()
            .color(colors::TEXT_SECONDARY),
    );
}

/// The glTF node that shows this load. A binding whose node the loaded model does not
/// have is drawn in red — the goods would silently stay invisible in the simulator.
///
/// Without a model loaded there is nothing to choose from, so the field stays text: a
/// combo with no entries would refuse an entry the user has every right to make.
fn node_combo(ui: &mut egui::Ui, id: usize, slot: &mut Option<String>, nodes: &[String]) {
    if nodes.is_empty() {
        optional_text(ui, slot, "load-node-placeholder");
        return;
    }
    let current = slot.clone().unwrap_or_default();
    let missing = !current.is_empty() && !nodes.iter().any(|n| n == &current);
    let label = if current.is_empty() {
        egui::RichText::new(t!("load-node-placeholder")).color(colors::TEXT_SECONDARY)
    } else {
        let text = egui::RichText::new(current.as_str()).monospace();
        if missing {
            text.color(colors::ERROR)
        } else {
            text
        }
    };
    let response = egui::ComboBox::from_id_salt(("load-node", id))
        .selected_text(label)
        .width(space::FIELD)
        .truncate()
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(current.is_empty(), t!("load-node-placeholder"))
                .clicked()
            {
                *slot = None;
            }
            for name in nodes {
                if ui
                    .selectable_label(current == *name, egui::RichText::new(name).monospace())
                    .clicked()
                {
                    *slot = Some(name.clone());
                }
            }
        })
        .response;
    if missing {
        response.on_hover_text(t!("part-node-missing-hint"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every label, button and placeholder the panel draws. A key missing from the
    /// locales shows up as its own name in the middle of the form.
    const KEYS: [&str; 37] = [
        "meta-class",
        "meta-manufacturer",
        "meta-year",
        "meta-year-unset",
        "meta-epoch",
        "meta-country",
        "meta-country-unset",
        "meta-country-other",
        "meta-operator",
        "meta-author",
        "meta-thumbnail",
        "meta-thumbnail-placeholder",
        "meta-thumbnail-pick",
        "meta-description",
        "meta-description-placeholder",
        "filter-image",
        "var-heading",
        "var-empty",
        "action-add-variant",
        "var-name-placeholder",
        "var-model",
        "var-model-placeholder",
        "var-model-none",
        "var-epoch",
        "var-numbers",
        "var-numbers-placeholder",
        "var-description",
        "var-description-placeholder",
        "load-heading",
        "load-empty",
        "action-add-load",
        "load-name-placeholder",
        "load-mass",
        "load-node-placeholder",
        // Tooltips the panel asks for by name; the rest `row` picks up itself and
        // leaves out where the locales have none.
        "action-add-variant-hint",
        "action-add-load-hint",
        "var-numbers-hint",
    ];

    /// The lines that carry a figure — a mistyped placeholder drops it silently and
    /// leaves a line that says nothing.
    const VALUE_KEYS: [(&str, &str); 3] = [
        ("var-model-effective", "file"),
        ("load-total", "mass"),
        ("load-capped", "max"),
    ];

    #[test]
    fn every_key_the_panel_draws_exists() {
        for key in KEYS {
            assert!(i18n::maybe(key).is_some(), "{key}");
        }
    }

    /// `VALUE_KEYS` cannot go through `maybe` — a message with a placeholder does
    /// not resolve without its argument. Asking for the figure covers both: a key
    /// the locales do not have comes back as its own name, without the figure.
    #[test]
    fn the_derived_lines_name_their_figure() {
        for (key, placeholder) in VALUE_KEYS {
            let mut args = i18n::Args::new();
            args.insert(
                std::borrow::Cow::Borrowed(placeholder),
                i18n::FluentValue::from("42"),
            );
            let line = i18n::lookup_args(key, &args);
            assert!(line.contains("42"), "{key}: {line}");
        }
    }

    /// The multiline field is the list, only written down.
    #[test]
    fn the_number_field_round_trips_the_list() {
        let numbers = vec!["101 001-6".to_string(), "101 002-4".to_string()];
        assert_eq!(parse_numbers(&numbers_text(&numbers)), numbers);
        // Blank lines and stray spaces are formatting, not running numbers.
        assert_eq!(parse_numbers(" 101 001-6\n\n\t\n  101 002-4  \n"), numbers);
        assert!(parse_numbers("\n \n").is_empty());
    }

    #[test]
    fn a_load_over_the_payload_reads_as_capped() {
        assert!(is_capped(30_000.0, 20_000.0));
        assert!(!is_capped(20_000.0, 20_000.0));
        // 0 = no payload anscribed, so nothing is capped away.
        assert!(!is_capped(30_000.0, 0.0));
    }
}
