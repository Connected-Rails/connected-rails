//! Content drawer: everything the installed mods bring, in one place.
//!
//! Modelled on Unreal's drawer — a panel that comes up from the bottom edge over
//! the full width, holds the catalogue, and goes away again. The editor's own
//! pickers are combo boxes inside the tool they belong to, which answer "which
//! object does this tool stamp" but never "what is installed at all". A modder
//! who adds a mod has no other place to see whether the editor found it.
//!
//! Categories left, entries right as cards. Picking arms the tool the entry
//! belongs to: an object the object tool, a signal type or model the
//! place-device tool set to signals. Track types are a read-only listing, and
//! say so by not reacting to a click.

use bevy_egui::egui;
use editor_ui::{Icon, colors, space};
use i18n::t;
use sim_core::interlock::SignalSystem;

use crate::Catalogs;
use crate::tools::{EditorState, Tool};
use track_model::DeviceKind;

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

    /// The category `--drawer <name>` names — the i18n key without its prefix
    /// (`objects`, `signal-types`, …), so the two can never drift apart.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|c| c.key().trim_start_matches("drawer-") == name)
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

/// Session state of the drawer — open, category, filters. Not saved: a drawer
/// that comes back open after a restart hides the map it was opened over.
#[derive(Default)]
pub struct Drawer {
    pub open: bool,
    pub category: Category,
    pub filter: String,
    /// Mod the listing is narrowed to; `None` = every mod. Kept across a
    /// category switch — "show me what this mod brought" is a question about
    /// the mod, not about one kind of it.
    pub source: Option<String>,
    /// Signal system the signal types are narrowed to; `None` = every system.
    /// Dropped when the category leaves the signal types it belongs to.
    pub system: Option<SignalSystem>,
    /// Tag the listing is narrowed to; `None` = every tag. Kept across a
    /// category switch, because a tag is what crosses the kinds: `epoch-3`
    /// names objects, signals and track types at once.
    pub tag: Option<String>,
}

impl Drawer {
    /// Opens or closes the drawer. The filter field is deliberately *not*
    /// focused with it: the viewport is no egui widget, so a field that takes
    /// the keyboard on opening keeps it, and every key meant for the camera
    /// lands in the search instead.
    pub fn toggle(&mut self) {
        self.open = !self.open;
    }

    fn filtering(&self) -> bool {
        !self.filter.is_empty()
            || self.source.is_some()
            || self.system.is_some()
            || self.tag.is_some()
    }

    fn clear_filters(&mut self) {
        self.filter.clear();
        self.source = None;
        self.system = None;
        self.tag = None;
    }
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
    /// The mod the entry came from — the key's own prefix, kept apart so the
    /// mod filter does not have to parse the key back a second time.
    source: String,
    /// Signal system, for the entries that have one. What the system filter
    /// matches against; `None` everywhere else.
    system: Option<SignalSystem>,
    /// The mod author's own tags, normalised — a hand-written `Mast` and an
    /// editor-written `mast` have to be the same tag in the filter.
    tags: Vec<String>,
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
            let objects: Vec<(String, String, String, Vec<String>)> = catalogs
                .objects
                .map
                .iter()
                .map(|(key, object)| {
                    (
                        key.clone(),
                        object.name.clone(),
                        object.model.clone(),
                        object.tags.clone(),
                    )
                })
                .collect();
            objects
                .into_iter()
                .map(|(key, name, model, tags)| {
                    let (source, stem) = split(&key);
                    Entry {
                        title: if name.is_empty() { stem } else { name },
                        detail: source.clone(),
                        source,
                        system: None,
                        tags: normalize(&tags),
                        mark: mark_for(&model, category, &mut catalogs.thumbnails),
                        key,
                    }
                })
                .collect()
        }
        Category::SignalTypes => {
            let types: Vec<(String, SignalSystem, Option<String>, Vec<String>)> = catalogs
                .signal_types
                .map
                .iter()
                .map(|(key, ty)| (key.clone(), ty.system, ty.model.clone(), ty.tags.clone()))
                .collect();
            types
                .into_iter()
                .map(|(key, system, model, tags)| {
                    let (source, stem) = split(&key);
                    // A type is drawn as the model it defaults to: a column of
                    // identical mast icons says nothing about which signal a
                    // card would put on the line.
                    let file = model
                        .as_deref()
                        .and_then(|name| catalogs.signal_models.map.get(name))
                        .and_then(|model| model.parts.first())
                        .map(|part| part.file.clone());
                    Entry {
                        title: stem,
                        detail: format!("{source} · {}", crate::ui::signal_system_label(system)),
                        source,
                        system: Some(system),
                        tags: normalize(&tags),
                        mark: match file {
                            Some(file) => mark_for(&file, category, &mut catalogs.thumbnails),
                            None => editor_ui::Mark::Icon(category.icon()),
                        },
                        key,
                    }
                })
                .collect()
        }
        Category::SignalModels => {
            let models: Vec<(String, Option<String>, Vec<String>)> = catalogs
                .signal_models
                .map
                .iter()
                // A signal is assembled from parts mounted on one another; the
                // first is the screen, which is what a catalogue picture of it
                // has to show.
                .map(|(key, model)| {
                    (
                        key.clone(),
                        model.parts.first().map(|p| p.file.clone()),
                        model.tags.clone(),
                    )
                })
                .collect();
            models
                .into_iter()
                .map(|(key, file, tags)| {
                    let (source, stem) = split(&key);
                    Entry {
                        title: stem,
                        detail: source.clone(),
                        source,
                        system: None,
                        tags: normalize(&tags),
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
                    detail: source.clone(),
                    source,
                    system: None,
                    tags: normalize(&ty.tags),
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

/// The tags of an entry as the filter needs them: normalised and deduplicated.
/// A mod file is hand-written as often as it is editor-written, so the drawer
/// cannot assume what it reads is already in one form.
fn normalize(tags: &[String]) -> Vec<String> {
    let mut tags: Vec<String> = tags
        .iter()
        .filter_map(|tag| editor_ui::normalize_tag(tag))
        .collect();
    tags.sort();
    tags.dedup();
    tags
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
            // The mods behind this category, before any filter — a mod that the
            // filter has just emptied has to stay pickable, or the only way back
            // is to guess which entry to search for.
            let mut sources: Vec<String> = entries.iter().map(|e| e.source.clone()).collect();
            sources.sort();
            sources.dedup();
            let mut tags: Vec<String> = entries.iter().flat_map(|e| e.tags.clone()).collect();
            tags.sort();
            tags.dedup();

            let filter = state.drawer.filter.to_lowercase();
            let shown: Vec<Entry> = entries
                .into_iter()
                .filter(|entry| {
                    (filter.is_empty()
                        || entry.key.to_lowercase().contains(&filter)
                        || entry.title.to_lowercase().contains(&filter)
                        || entry.tags.iter().any(|tag| tag.contains(&filter)))
                        && state
                            .drawer
                            .source
                            .as_ref()
                            .is_none_or(|s| &entry.source == s)
                        && state.drawer.system.is_none_or(|s| entry.system == Some(s))
                        && state
                            .drawer
                            .tag
                            .as_ref()
                            .is_none_or(|t| entry.tags.contains(t))
                })
                .collect();

            let total = entries_count(state.drawer.category, catalogs);
            header(ui, state, &sources, &tags, total, shown.len());
            ui.add_space(space::S);
            ui.separator();
            ui.add_space(space::S);

            ui.horizontal_top(|ui| {
                categories(ui, state, catalogs);
                ui.add_space(space::S);
                // A full-height rule: the categories are a column of the
                // drawer, not a group of rows stacked above the cards.
                ui.separator();
                ui.add_space(space::S);
                cards(ui, state, &shown, total);
            });
        });
}

/// Title, filters and the count — the count is what tells a modder whether the
/// editor found their mod at all.
fn header(
    ui: &mut egui::Ui,
    state: &mut EditorState,
    sources: &[String],
    tags: &[String],
    total: usize,
    shown: usize,
) {
    ui.horizontal(|ui| {
        editor_ui::icon_label(ui, Icon::Drawer);
        ui.label(editor_ui::heading(t!("drawer-title")));
        ui.add_space(space::L);
        ui.add(
            egui::TextEdit::singleline(&mut state.drawer.filter)
                .hint_text(t!("drawer-filter-placeholder"))
                .desired_width(240.0),
        );
        // Which mod an entry came from is the one thing every category has and
        // the substring filter answers badly: a mod named after its region
        // matches half the object names in it.
        egui::ComboBox::from_id_salt("drawer-source")
            .width(space::FIELD)
            .selected_text(match &state.drawer.source {
                Some(source) => source.clone(),
                None => t!("drawer-source-all"),
            })
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(state.drawer.source.is_none(), t!("drawer-source-all"))
                    .clicked()
                {
                    state.drawer.source = None;
                }
                for source in sources {
                    if ui
                        .selectable_label(state.drawer.source.as_deref() == Some(source), source)
                        .clicked()
                    {
                        state.drawer.source = Some(source.clone());
                    }
                }
            });
        // The tags the mod authors themselves gave their entries. Only offered
        // where there are any: a catalogue nobody has tagged must not carry a
        // combo whose one entry is "every tag".
        if !tags.is_empty() {
            egui::ComboBox::from_id_salt("drawer-tag")
                .width(space::FIELD)
                .selected_text(match &state.drawer.tag {
                    Some(tag) => tag.clone(),
                    None => t!("drawer-tag-all"),
                })
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(state.drawer.tag.is_none(), t!("drawer-tag-all"))
                        .clicked()
                    {
                        state.drawer.tag = None;
                    }
                    for tag in tags {
                        if ui
                            .selectable_label(state.drawer.tag.as_deref() == Some(tag), tag)
                            .clicked()
                        {
                            state.drawer.tag = Some(tag.clone());
                        }
                    }
                });
        }
        // Only the signal types carry a system, and picking one is how a line
        // built to one rulebook keeps the others out of the way.
        if state.drawer.category == Category::SignalTypes {
            egui::ComboBox::from_id_salt("drawer-system")
                .width(space::FIELD)
                .selected_text(match state.drawer.system {
                    Some(system) => crate::ui::signal_system_label(system).to_string(),
                    None => t!("drawer-system-all"),
                })
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(state.drawer.system.is_none(), t!("drawer-system-all"))
                        .clicked()
                    {
                        state.drawer.system = None;
                    }
                    for system in [SignalSystem::HV, SignalSystem::Ks, SignalSystem::Hl] {
                        if ui
                            .selectable_label(
                                state.drawer.system == Some(system),
                                crate::ui::signal_system_label(system),
                            )
                            .clicked()
                        {
                            state.drawer.system = Some(system);
                        }
                    }
                });
        }
        if state.drawer.filtering() && ui.small_button(t!("action-reset")).clicked() {
            state.drawer.clear_filters();
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
                    // The system filter belongs to the signal types; carried
                    // into a category that has no system it would silently
                    // empty the list.
                    if category != Category::SignalTypes {
                        state.drawer.system = None;
                    }
                }
                ui.add_space(space::XS);
            }
        });
    });
}

/// The entries as cards, wrapped into as many columns as the width allows.
fn cards(ui: &mut egui::Ui, state: &mut EditorState, shown: &[Entry], total: usize) {
    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            if shown.is_empty() {
                // Two different empty states: a filter that matches nothing is
                // undone with the button, an empty catalogue is a missing mod.
                let filtered = total > 0 && state.drawer.filtering();
                ui.label(
                    egui::RichText::new(if filtered {
                        t!("drawer-empty-filtered")
                    } else {
                        t!("drawer-empty")
                    })
                    .small()
                    .color(colors::TEXT_SECONDARY),
                );
                if filtered {
                    ui.add_space(space::XS);
                    if ui.small_button(t!("drawer-reset-filters")).clicked() {
                        state.drawer.clear_filters();
                    }
                }
                return;
            }
            // Everything a tool can stamp is pickable; a track type stamps
            // nothing, and a card that reacts to nothing must not look like a
            // button.
            let category = state.drawer.category;
            let pickable = category != Category::TrackTypes;
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
                        let picked = match category {
                            Category::Objects => state.object.as_ref() == Some(&entry.key),
                            Category::SignalTypes => state.signal_type.as_ref() == Some(&entry.key),
                            Category::SignalModels => {
                                state.signal_model.as_ref() == Some(&entry.key)
                            }
                            Category::TrackTypes => false,
                        };
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
                        .on_hover_text(if entry.tags.is_empty() {
                            format!(
                                "{}
{}",
                                entry.title, entry.key
                            )
                        } else {
                            format!(
                                "{}
{}
{}",
                                entry.title,
                                entry.key,
                                entry.tags.join(" · ")
                            )
                        });
                        if pickable && response.clicked() {
                            match category {
                                Category::Objects => {
                                    state.object = Some(entry.key.clone());
                                    state.tool = Tool::PlaceObject;
                                }
                                // A signal type brings its own default model, so
                                // picking one drops a model override from an
                                // earlier pick instead of pairing the two blindly.
                                Category::SignalTypes => {
                                    state.signal_type = Some(entry.key.clone());
                                    state.signal_model = None;
                                    state.device_kind = Some(DeviceKind::Signal);
                                    state.tool = Tool::PlaceDevice;
                                }
                                Category::SignalModels => {
                                    state.signal_model = Some(entry.key.clone());
                                    state.device_kind = Some(DeviceKind::Signal);
                                    state.tool = Tool::PlaceDevice;
                                }
                                Category::TrackTypes => {}
                            }
                            state.drawing = None;
                            // The drawer covers the map it just armed a tool
                            // for — Unreal's dismisses itself the same way.
                            state.drawer.open = false;
                        }
                        if (index + 1) % columns == 0 {
                            ui.end_row();
                        }
                    }
                });
        });
}
