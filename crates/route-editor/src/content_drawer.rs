//! Content drawer: everything the installed mods bring, in one place.
//!
//! Modelled on Unreal's drawer — a panel that comes up from the bottom edge over
//! the full width, holds the catalogue, and goes away again. The editor's own
//! pickers are combo boxes inside the tool they belong to, which answer "which
//! object does this tool stamp" but never "what is installed at all". A modder
//! who adds a mod has no other place to see whether the editor found it.
//!
//! Categories left, entries right as cards. Picking an object arms the object
//! tool with it, which is the one action a catalogue entry has here; the other
//! categories are read-only listings, and say so by not reacting to a click.

use bevy_egui::egui;
use editor_ui::{Icon, colors, space};
use i18n::t;

use crate::Catalogs;
use crate::tools::{EditorState, Tool};

/// What the drawer is showing.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Category {
    #[default]
    Objects,
    SignalTypes,
    SignalModels,
    TrackTypes,
}

impl Category {
    /// In the order they are listed: the ones a line is built from first.
    const ALL: [Category; 4] = [
        Category::Objects,
        Category::SignalTypes,
        Category::SignalModels,
        Category::TrackTypes,
    ];

    fn icon(self) -> Icon {
        match self {
            Category::Objects => Icon::Object,
            Category::SignalTypes | Category::SignalModels => Icon::Signal,
            Category::TrackTypes => Icon::Track,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Category::Objects => "drawer-objects",
            Category::SignalTypes => "drawer-signal-types",
            Category::SignalModels => "drawer-signal-models",
            Category::TrackTypes => "drawer-track-types",
        }
    }
}

/// Session state of the drawer — open, category, filter. Not saved: a drawer
/// that comes back open after a restart hides the map it was opened over.
#[derive(Default)]
pub struct Drawer {
    pub open: bool,
    pub category: Category,
    pub filter: String,
}

/// One catalogue entry as the cards need it.
struct Entry {
    /// `"<mod>:<stem>"` — what a `.ron` writes to refer to this.
    key: String,
    /// The line the card leads with: the prose name where the data has one,
    /// the file stem otherwise.
    title: String,
    /// The line under it. The mod it came from, unless the data says something
    /// more useful about this kind — a signal type names its system.
    detail: String,
    /// What the card is marked with — its category's icon, or the colour the
    /// entry itself is.
    mark: editor_ui::Mark,
}

/// The entries of a category, in the catalogue's own order.
///
/// Takes the catalogues mutably because asking for a model's preview is what
/// queues it for rendering — a category nobody opens costs nothing.
fn entries(category: Category, catalogs: &mut Catalogs) -> Vec<Entry> {
    // Every key is `"<mod>:<stem>"`; the drawer splits that back apart so the
    // mod reads as provenance rather than as part of the name.
    let split = |key: &str| {
        let (source, stem) = key.split_once(':').unwrap_or(("", key));
        (source.to_string(), stem.to_string())
    };
    match category {
        Category::Objects => {
            // Collected first: asking for a preview borrows the thumbnails,
            // and the catalogue is borrowed for the iteration.
            let objects: Vec<(String, String, String)> = catalogs
                .objects
                .map
                .iter()
                .map(|(key, object)| (key.clone(), object.name.clone(), object.model.clone()))
                .collect();
            objects
                .into_iter()
                .map(|(key, name, model)| {
                    let (source, stem) = split(&key);
                    Entry {
                        title: if name.is_empty() { stem } else { name },
                        detail: source,
                        mark: mark_for(&model, category, &mut catalogs.thumbnails),
                        key,
                    }
                })
                .collect()
        }
        Category::SignalTypes => catalogs
            .signal_types
            .map
            .iter()
            .map(|(key, ty)| {
                let (source, stem) = split(key);
                Entry {
                    key: key.clone(),
                    title: stem,
                    detail: format!("{source} · {}", crate::ui::signal_system_label(ty.system)),
                    mark: editor_ui::Mark::Icon(category.icon()),
                }
            })
            .collect(),
        Category::SignalModels => {
            let models: Vec<(String, Option<String>)> = catalogs
                .signal_models
                .map
                .iter()
                // A signal is assembled from parts mounted on one another; the
                // first is the screen, which is what a catalogue picture of it
                // has to show.
                .map(|(key, model)| (key.clone(), model.parts.first().map(|p| p.file.clone())))
                .collect();
            models
                .into_iter()
                .map(|(key, file)| {
                    let (source, stem) = split(&key);
                    Entry {
                        title: stem,
                        detail: source,
                        mark: match file {
                            Some(file) => mark_for(&file, category, &mut catalogs.thumbnails),
                            None => editor_ui::Mark::Icon(category.icon()),
                        },
                        key,
                    }
                })
                .collect()
        }
        Category::TrackTypes => catalogs
            .types
            .map
            .iter()
            .map(|(key, ty)| {
                let (source, stem) = split(key);
                let (r, g, b) = ty.color;
                Entry {
                    key: key.clone(),
                    title: if ty.name.is_empty() {
                        stem
                    } else {
                        ty.name.clone()
                    },
                    detail: source,
                    // A track type is what it looks like on the map; the icon
                    // would say "track type" a third time.
                    mark: editor_ui::Mark::Color(egui::Color32::from_rgb(
                        (r * 255.0) as u8,
                        (g * 255.0) as u8,
                        (b * 255.0) as u8,
                    )),
                }
            })
            .collect(),
    }
}

/// The preview of `model` if one has been rendered, the category's icon until
/// then — asking is what puts it in the queue.
fn mark_for(
    model: &str,
    category: Category,
    thumbnails: &mut crate::thumbnails::Thumbnails,
) -> editor_ui::Mark {
    match thumbnails.get(model) {
        Some(texture) => editor_ui::Mark::Image(texture),
        None => editor_ui::Mark::Icon(category.icon()),
    }
}

/// How many a category holds, without building its entries.
fn entries_count(category: Category, catalogs: &Catalogs) -> usize {
    match category {
        Category::Objects => catalogs.objects.map.len(),
        Category::SignalTypes => catalogs.signal_types.map.len(),
        Category::SignalModels => catalogs.signal_models.map.len(),
        Category::TrackTypes => catalogs.types.map.len(),
    }
}

/// Height of the open drawer [px] — the four category rows plus its header.
/// Resizable from there.
const HEIGHT: f32 = 300.0;
/// Padding of a category row — the card's own is right for a card and too much
/// for a list of four.
const ROW_PADDING: egui::Margin = egui::Margin {
    left: space::S as i8,
    right: space::S as i8,
    top: space::XS as i8,
    bottom: space::XS as i8,
};
/// Width of the category column [px].
const CATEGORIES: f32 = 176.0;
/// Width of one entry card [px].
const CARD: f32 = 208.0;

/// The drawer, over the status bar and under everything else.
pub fn draw(root: &mut egui::Ui, state: &mut EditorState, catalogs: &mut Catalogs) {
    if !state.drawer.open {
        return;
    }
    egui::Panel::bottom("content-drawer")
        .default_size(HEIGHT)
        .resizable(true)
        .frame(editor_ui::panel_frame())
        .show(root, |ui| {
            let entries = entries(state.drawer.category, catalogs);
            let filter = state.drawer.filter.to_lowercase();
            let shown: Vec<Entry> = entries
                .into_iter()
                .filter(|entry| {
                    filter.is_empty()
                        || entry.key.to_lowercase().contains(&filter)
                        || entry.title.to_lowercase().contains(&filter)
                })
                .collect();

            let total = entries_count(state.drawer.category, catalogs);
            header(ui, state, total, shown.len());
            ui.add_space(space::S);
            ui.separator();
            ui.add_space(space::S);

            ui.horizontal_top(|ui| {
                categories(ui, state, catalogs);
                ui.add_space(space::M);
                cards(ui, state, &shown);
            });
        });
}

/// Title, filter and the count — the count is what tells a modder whether the
/// editor found their mod at all.
fn header(ui: &mut egui::Ui, state: &mut EditorState, total: usize, shown: usize) {
    ui.horizontal(|ui| {
        ui.label(editor_ui::heading(t!("drawer-title")));
        ui.add_space(space::L);
        ui.add(
            egui::TextEdit::singleline(&mut state.drawer.filter)
                .hint_text(t!("drawer-filter-placeholder"))
                .desired_width(240.0),
        );
        if !state.drawer.filter.is_empty() && ui.small_button(t!("action-reset")).clicked() {
            state.drawer.filter.clear();
        }
        // A filtered list always states `n of m`, so a short list cannot be
        // mistaken for a small catalogue. Unfiltered it says nothing: the
        // category row already carries the count, and `t!` stringifies its
        // arguments, so a Fluent plural selector would never match ("1 entries").
        if shown != total {
            ui.label(
                egui::RichText::new(t!("drawer-count-filtered", shown = shown, total = total))
                    .small()
                    .color(colors::TEXT_SECONDARY),
            );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(t!("action-close")).clicked() {
                state.drawer.open = false;
            }
        });
    });
}

/// The category column: icon, name and how many entries are behind it.
fn categories(ui: &mut egui::Ui, state: &mut EditorState, catalogs: &Catalogs) {
    ui.allocate_ui(egui::vec2(CATEGORIES, ui.available_height()), |ui| {
        ui.vertical(|ui| {
            for category in Category::ALL {
                let count = match category {
                    Category::Objects => catalogs.objects.map.len(),
                    Category::SignalTypes => catalogs.signal_types.map.len(),
                    Category::SignalModels => catalogs.signal_models.map.len(),
                    Category::TrackTypes => catalogs.types.map.len(),
                };
                let active = state.drawer.category == category;
                let response = ui
                    .scope_builder(egui::UiBuilder::new().sense(egui::Sense::click()), |ui| {
                        ui.set_width(CATEGORIES);
                        let fill = if active {
                            colors::ACCENT_BG
                        } else if ui.response().hovered() {
                            colors::BG_HOVER
                        } else {
                            colors::BG_CARD
                        };
                        editor_ui::card_frame()
                            .fill(fill)
                            .inner_margin(ROW_PADDING)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    editor_ui::icon_label(ui, category.icon());
                                    ui.label(egui::RichText::new(t!(category.key())).color(
                                        if active {
                                            colors::ACCENT_TEXT
                                        } else {
                                            colors::TEXT
                                        },
                                    ));
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                egui::RichText::new(count.to_string())
                                                    .small()
                                                    .color(if active {
                                                        colors::ACCENT_TEXT
                                                    } else {
                                                        colors::TEXT_SECONDARY
                                                    }),
                                            );
                                        },
                                    );
                                });
                            });
                    })
                    .response;
                if response.clicked() {
                    state.drawer.category = category;
                }
                ui.add_space(space::XS);
            }
        });
    });
}

/// The entries as cards, wrapped into as many columns as the width allows.
fn cards(ui: &mut egui::Ui, state: &mut EditorState, shown: &[Entry]) {
    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            if shown.is_empty() {
                ui.label(
                    egui::RichText::new(t!("drawer-empty"))
                        .small()
                        .color(colors::TEXT_SECONDARY),
                );
                return;
            }
            // Only the object catalogue arms a tool; the rest is a listing, and
            // a card that reacts to nothing must not look like a button.
            let pickable = state.drawer.category == Category::Objects;
            // A grid, not a wrapping row: `horizontal_wrapped` gives every card
            // its own height and baseline, and three cards of different names
            // then sit on three different lines. The column count is whatever
            // the free width holds.
            let step = CARD + space::S;
            let columns = ((ui.available_width() + space::S) / step).floor().max(1.0) as usize;
            egui::Grid::new("drawer-cards")
                .num_columns(columns)
                .spacing([space::S, space::S])
                .show(ui, |ui| {
                    for (index, entry) in shown.iter().enumerate() {
                        let picked = pickable && state.object.as_ref() == Some(&entry.key);
                        let response = editor_ui::card_entry(
                            ui,
                            entry.mark,
                            &entry.title,
                            &entry.detail,
                            picked,
                            pickable,
                        )
                        // Truncated on the card; the whole of it here, with the
                        // key a `.ron` would write.
                        .on_hover_text(format!(
                            "{}
{}",
                            entry.title, entry.key
                        ));
                        if pickable && response.clicked() {
                            state.object = Some(entry.key.clone());
                            state.tool = Tool::PlaceObject;
                            state.drawing = None;
                        }
                        if (index + 1) % columns == 0 {
                            ui.end_row();
                        }
                    }
                });
        });
}
