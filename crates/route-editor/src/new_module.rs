//! The "new module" dialog: a name, the anchor the module hangs on, and a map
//! to find that anchor on.
//!
//! A module starts as a place, not as a track — the anchor decides which
//! elevation tiles, which aerial imagery and which neighbours it will meet, and
//! it is where the envelope ([`crate::envelope`]) is built around. Typing two
//! coordinates blind is how that goes wrong, so the dialog carries an
//! OpenStreetMap picker with a place search: search "Göttingen", click the spot,
//! and the fields fill themselves in.
//!
//! Drawn by its own system rather than from `ui::draw`, which is already at
//! Bevy's system-parameter limit.

use crate::tools::{EditorState, Selection};
use crate::{Focus, History, Line, Request};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use content::route::GeoPoint;
use editor_ui::{colors, space};
use i18n::t;
use imagery::{ImageryConfig, ImagerySource, TileId, geocode};
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::mpsc::Receiver;

/// Zoom level the picker opens at — a town with its surroundings.
const START_ZOOM: u8 = 12;
/// Zoom level a search hit is shown at — close enough to pick a spot on it.
const HIT_ZOOM: u8 = 14;
/// Size of the map inside the dialog [px].
const MAP: egui::Vec2 = egui::vec2(520.0, 300.0);
/// Years a module can be set in: from the first German railway to a little
/// beyond today, which is as far as a plan for a line reaches.
const YEARS: std::ops::RangeInclusive<u32> = 1835..=2100;
/// Edge length of a tile as drawn [px]; the tiles themselves are 256².
const TILE: f32 = 256.0;
/// Scroll [points] a wheel notch reports where the platform counts in pixels
/// rather than lines — egui's own default for the length of a line.
const NOTCH: f32 = 50.0;

/// The answer of a running place search, once it arrives. Behind a mutex so the
/// dialog works as a Bevy resource: a `Receiver` is `Send`, but not `Sync`.
type Search = Mutex<Receiver<Result<Vec<geocode::Place>, String>>>;

/// State of the dialog. `open` is what the menu sets.
#[derive(Resource, Default)]
pub struct NewModule {
    pub open: bool,
    name: String,
    lat: f64,
    lon: f64,
    zoom: u8,
    /// Edge length of the envelope the module starts with [km].
    size_km: f64,
    /// The year the module portrays.
    year: u32,
    /// Invented rather than a rebuild of a real place.
    fictional: bool,
    query: String,
    hits: Vec<geocode::Place>,
    /// The running search, if there is one.
    search: Option<Search>,
    searching: bool,
    error: String,
    /// Wheel scroll that has not yet added up to a whole zoom level [points].
    scroll: f32,
    map: Option<Map>,
}

/// The picker's own tile source. Separate from the overlay's, so picking a place
/// does not change what the editor map itself shows.
struct Map {
    source: ImagerySource,
    textures: HashMap<TileId, egui::TextureHandle>,
    attribution: String,
    /// Licence page the credit links to, where the provider names one.
    attribution_url: Option<String>,
}

impl Map {
    fn new() -> Self {
        let mut config = ImageryConfig {
            active: "osm_standard".into(),
            ..Default::default()
        };
        // A 520×300 map is nine tiles at most, and OSM's tile policy asks for
        // restraint — two workers keep it moving without hammering the service.
        config.request.parallel = 2;
        let provider = config.provider();
        let attribution = provider.map(|p| p.attribution.clone()).unwrap_or_default();
        let attribution_url = provider.and_then(|p| p.attribution_url.clone());
        Self {
            source: ImagerySource::new(config),
            textures: HashMap::new(),
            attribution,
            attribution_url,
        }
    }
}

impl NewModule {
    /// Opens the dialog on a fresh module centred where the view already is.
    pub fn open_at(&mut self, lat: f64, lon: f64) {
        self.open = true;
        self.name = String::new();
        self.lat = lat;
        self.lon = lon;
        self.zoom = START_ZOOM;
        self.size_km = content::route::DEFAULT_ENVELOPE_HALF_SIZE * 2.0 / 1000.0;
        self.year = Self::this_year();
        self.fictional = false;
        self.query.clear();
        self.hits.clear();
        self.search = None;
        self.searching = false;
        self.error.clear();
        self.scroll = 0.0;
    }

    /// What the year field opens on. Whole years out of the wall clock — a
    /// leap day of drift does not matter for a number the user overwrites
    /// anyway, and it saves a date library.
    fn this_year() -> u32 {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        (1970 + secs / 31_556_952).clamp(*YEARS.start() as u64, *YEARS.end() as u64) as u32
    }

    fn anchor(&self) -> GeoPoint {
        // The height stays 0: the anchor says *where* the module is, and the
        // ground it sits on comes from the elevation data the module imports.
        GeoPoint {
            lat: self.lat,
            lon: self.lon,
            height: 0.0,
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn draw(
    mut contexts: EguiContexts,
    mut dialog: ResMut<NewModule>,
    mut request: ResMut<Request>,
    mut line: ResMut<Line>,
    mut history: ResMut<History>,
    mut state: ResMut<EditorState>,
    mut overlay: ResMut<crate::overlay::Overlay>,
    mut focus: ResMut<Focus>,
    mut skipped_first: Local<bool>,
) -> Result {
    // `ui::draw` installs the fonts on the first pass, and they only become
    // active on the next one — a heading drawn in between panics inside epaint.
    if !*skipped_first {
        *skipped_first = true;
        return Ok(());
    }
    if request.new_module {
        request.new_module = false;
        let (lat, lon) = crate::focus_degrees(focus.position);
        dialog.open_at(lat, lon);
    }
    if !dialog.open {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?.clone();
    poll_search(&mut dialog);

    let mut create = false;
    let mut cancel = false;
    // A window rather than a modal: a modal is nailed to the middle of the
    // screen, and this one covers the very map the anchor is being picked
    // against. The title bar is the handle it is moved by.
    egui::Window::new(t!("new-module-title"))
        .collapsible(false)
        .resizable(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.viewport_rect().center())
        .show(&ctx, |ui| {
            ui.set_width(MAP.x);

            egui::Grid::new("new-module-form")
                .num_columns(2)
                .spacing([space::M, space::XS + 2.0])
                .min_col_width(space::LABEL_COL)
                .show(ui, |ui| {
                    editor_ui::form_label(ui, t!("new-module-name"));
                    ui.add(
                        egui::TextEdit::singleline(&mut dialog.name)
                            .desired_width(space::FIELD * 2.0)
                            .hint_text(t!("new-module-name-placeholder")),
                    );
                    ui.end_row();

                    editor_ui::form_label(ui, t!("new-module-lat"));
                    editor_ui::field(ui, &mut dialog.lat, 0.0001, -85.0..=85.0, "°");
                    ui.end_row();

                    editor_ui::form_label(ui, t!("new-module-lon"));
                    editor_ui::field(ui, &mut dialog.lon, 0.0001, -180.0..=180.0, "°");
                    ui.end_row();

                    editor_ui::form_label(ui, t!("new-module-size"))
                        .on_hover_text(t!("new-module-size-hint"));
                    editor_ui::field(ui, &mut dialog.size_km, 0.1, 0.2..=60.0, "km");
                    ui.end_row();

                    editor_ui::form_label(ui, t!("new-module-year"))
                        .on_hover_text(t!("new-module-year-hint"));
                    // Not `editor_ui::field`: at a step of one it groups the
                    // digits, and a year is not written "2 026". The layout is
                    // that of the other fields, so the column keeps one edge.
                    let layout = ui.layout().with_main_align(egui::Align::Min);
                    ui.scope_builder(egui::UiBuilder::new().layout(layout), |ui| {
                        ui.spacing_mut().interact_size.x = space::FIELD;
                        ui.add(
                            egui::DragValue::new(&mut dialog.year)
                                .speed(1.0)
                                .range(YEARS),
                        );
                    });
                    ui.end_row();

                    editor_ui::form_label(ui, t!("new-module-kind"));
                    egui::ComboBox::from_id_salt("new-module-kind")
                        .width(space::FIELD)
                        .selected_text(kind_label(dialog.fictional))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut dialog.fictional,
                                false,
                                t!("new-module-kind-real"),
                            );
                            ui.selectable_value(
                                &mut dialog.fictional,
                                true,
                                t!("new-module-kind-fictional"),
                            );
                        });
                    ui.end_row();
                });

            ui.add_space(space::M);
            search_row(ui, &mut dialog);
            map(ui, &mut dialog);

            ui.add_space(space::S);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let ready = !dialog.name.trim().is_empty();
                    let button = egui::Button::new(t!("action-create-module"));
                    if ui
                        .add_enabled(ready, button)
                        .on_disabled_hover_text(t!("new-module-needs-name"))
                        .clicked()
                    {
                        create = true;
                    }
                    if ui.button(t!("action-cancel")).clicked() {
                        cancel = true;
                    }
                });
            });
            if ui.input(|i| i.key_pressed(egui::Key::Escape))
                && ui.memory(|m| m.focused().is_none())
            {
                cancel = true;
            }
        });

    if create {
        let anchor = dialog.anchor();
        crate::ui::new_line(
            &mut line,
            &mut history,
            &mut state,
            dialog.name.trim().to_string(),
            anchor,
            dialog.size_km * 500.0,
            dialog.year,
            dialog.fictional,
        );
        // The new module is empty, so there is no track to centre on — put the
        // view on the anchor the user just picked.
        focus.position = world_coords::geo::to_ecef_deg(anchor.lat, anchor.lon, anchor.height);
        line.recenter = false;
        state.selection = Selection::None;
        overlay.status = t!("status-module-created", name = dialog.name.trim());
        dialog.open = false;
    } else if cancel {
        dialog.open = false;
    }
    if !dialog.open {
        // The tile source and its textures are only worth holding while the
        // dialog is up; the disk cache keeps the tiles themselves.
        dialog.map = None;
    }
    Ok(())
}

/// What the module is: a rebuild of a real place, or invented.
fn kind_label(fictional: bool) -> String {
    match fictional {
        true => t!("new-module-kind-fictional"),
        false => t!("new-module-kind-real"),
    }
}

/// Takes the answer of a running search, if it has arrived.
fn poll_search(dialog: &mut NewModule) {
    let Some(receiver) = &dialog.search else {
        return;
    };
    let Ok(receiver) = receiver.lock() else {
        dialog.search = None;
        dialog.searching = false;
        return;
    };
    let answer = receiver.try_recv();
    drop(receiver);
    match answer {
        Ok(Ok(hits)) => {
            dialog.error = if hits.is_empty() {
                t!("new-module-no-hits")
            } else {
                String::new()
            };
            dialog.hits = hits;
            dialog.searching = false;
            dialog.search = None;
        }
        Ok(Err(message)) => {
            dialog.error = t!("new-module-search-failed", error = message);
            dialog.searching = false;
            dialog.search = None;
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => {}
        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
            dialog.searching = false;
            dialog.search = None;
        }
    }
}

/// Search field, its hits, and whatever went wrong.
fn search_row(ui: &mut egui::Ui, dialog: &mut NewModule) {
    ui.horizontal(|ui| {
        let field = ui.add(
            egui::TextEdit::singleline(&mut dialog.query)
                .desired_width(MAP.x - 120.0)
                .hint_text(t!("new-module-search-placeholder")),
        );
        let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let pressed = ui
            .add_enabled(
                !dialog.query.trim().is_empty() && !dialog.searching,
                egui::Button::new(t!("action-search")),
            )
            .clicked();
        if entered || pressed {
            let agent = ImageryConfig::default().request.user_agent;
            dialog.search = Some(Mutex::new(geocode::search(dialog.query.trim(), &agent)));
            dialog.searching = true;
            dialog.error.clear();
            dialog.hits.clear();
        }
    });

    if dialog.searching {
        ui.label(
            egui::RichText::new(t!("new-module-searching"))
                .small()
                .color(colors::TEXT_SECONDARY),
        );
    } else if !dialog.error.is_empty() {
        ui.label(
            egui::RichText::new(&dialog.error)
                .small()
                .color(colors::ERROR),
        );
    }

    if !dialog.hits.is_empty() {
        egui::ScrollArea::vertical()
            .id_salt("new-module-hits")
            .max_height(96.0)
            .show(ui, |ui| {
                let mut picked = None;
                for hit in &dialog.hits {
                    if ui
                        .selectable_label(false, egui::RichText::new(&hit.name).small())
                        .clicked()
                    {
                        picked = Some((hit.lat, hit.lon));
                    }
                }
                if let Some((lat, lon)) = picked {
                    dialog.lat = lat;
                    dialog.lon = lon;
                    dialog.zoom = HIT_ZOOM;
                    dialog.hits.clear();
                }
            });
    }
    ui.add_space(space::S);
}

/// How many wheel notches an input event is worth, whichever unit the platform
/// reports the wheel in.
fn notches(event: &egui::Event) -> f32 {
    match event {
        egui::Event::MouseWheel { unit, delta, .. } => match unit {
            egui::MouseWheelUnit::Line => delta.y,
            egui::MouseWheelUnit::Point => delta.y / NOTCH,
            egui::MouseWheelUnit::Page => delta.y,
        },
        _ => 0.0,
    }
}

/// The map: OpenStreetMap tiles around the anchor. Click sets the anchor, drag
/// pans, the wheel zooms.
fn map(ui: &mut egui::Ui, dialog: &mut NewModule) {
    let map = dialog.map.get_or_insert_with(Map::new);
    let (rect, response) = ui.allocate_exact_size(MAP, egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, colors::BG_INPUT);

    // Pixels per tile stay fixed, so panning by a pixel means the same distance
    // on screen at every zoom.
    let scale = TILE as f64;
    let (mut cx, mut cy) = imagery::tiles::world_xy(dialog.lat, dialog.lon, dialog.zoom);

    if response.dragged() {
        let delta = response.drag_delta();
        cx -= delta.x as f64 / scale;
        cy -= delta.y as f64 / scale;
        let (lat, lon) = imagery::tiles::lat_lon_at(cx, cy, dialog.zoom);
        dialog.lat = lat;
        dialog.lon = lon;
    }
    // The map says what the pointer does: cross hairs to place the anchor, a
    // closed hand while the map is being pulled along.
    if response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if response.hovered() {
        ui.ctx()
            .set_cursor_icon(match ui.input(|i| i.pointer.any_down()) {
                true => egui::CursorIcon::Grabbing,
                false => egui::CursorIcon::Crosshair,
            });
    }
    if response.hovered() {
        // One notch of the wheel is one zoom level. `smooth_scroll_delta`
        // spreads that notch over several frames, and stepping per frame races
        // through the whole stack — so the wheel events are read directly,
        // banked, and spent a notch at a time. A trackpad's stream of small
        // deltas adds up in the same account.
        dialog.scroll += ui.input(|i| i.events.iter().map(notches).sum::<f32>());
        let step = dialog.scroll as i32;
        if step != 0 {
            dialog.scroll -= step as f32;
            dialog.zoom = (dialog.zoom as i32 + step).clamp(2, 18) as u8;
            let (nx, ny) = imagery::tiles::world_xy(dialog.lat, dialog.lon, dialog.zoom);
            cx = nx;
            cy = ny;
        }
    } else {
        dialog.scroll = 0.0;
    }

    // Decoded tiles arrive on the worker threads; upload whatever is ready.
    for tile in map.source.drain() {
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [tile.width as usize, tile.height as usize],
            &tile.pixels,
        );
        let handle = ui.ctx().load_texture(
            format!("osm-{}-{}-{}", tile.tile.z, tile.tile.x, tile.tile.y),
            image,
            egui::TextureOptions::LINEAR,
        );
        map.textures.insert(tile.tile, handle);
    }

    let centre = rect.center();
    let count = TileId::count(dialog.zoom) as i64;
    let half = (rect.size() / 2.0 / TILE).ceil();
    let (tx0, ty0) = (cx.floor() as i64, cy.floor() as i64);
    for dy in -(half.y as i64)..=(half.y as i64) {
        for dx in -(half.x as i64)..=(half.x as i64) {
            let (x, y) = (tx0 + dx, ty0 + dy);
            if y < 0 || y >= count {
                continue;
            }
            // The world wraps east to west; the tile grid does not.
            let wrapped = x.rem_euclid(count) as u32;
            let tile = TileId::new(dialog.zoom, wrapped, y as u32);
            let min = centre
                + egui::vec2(
                    ((x as f64 - cx) * scale) as f32,
                    ((y as f64 - cy) * scale) as f32,
                );
            let tile_rect = egui::Rect::from_min_size(min, egui::vec2(TILE, TILE));
            if !rect.intersects(tile_rect) {
                continue;
            }
            match map.textures.get(&tile).map(|t| t.id()) {
                Some(id) => {
                    painter.image(
                        id,
                        tile_rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }
                // Not here yet — ask for it and leave the well empty this frame.
                None => {
                    map.source.request(tile);
                }
            };
        }
    }

    // The anchor: a click puts it under the cursor, and it is drawn where the
    // fields say it is — so typing a coordinate moves the marker too.
    if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
    {
        let x = cx + (pos.x - centre.x) as f64 / scale;
        let y = cy + (pos.y - centre.y) as f64 / scale;
        let (lat, lon) = imagery::tiles::lat_lon_at(x, y, dialog.zoom);
        dialog.lat = lat;
        dialog.lon = lon;
    }
    let (ax, ay) = imagery::tiles::world_xy(dialog.lat, dialog.lon, dialog.zoom);
    let marker = centre + egui::vec2(((ax - cx) * scale) as f32, ((ay - cy) * scale) as f32);
    if rect.contains(marker) {
        painter.circle_stroke(marker, 7.0, egui::Stroke::new(2.0, colors::ACCENT));
        painter.circle_filled(marker, 2.5, colors::ACCENT);
    }

    // The credit the tile services require, on the image itself and linking to
    // the licence — OSM's attribution guidelines ask for both, and a plate
    // behind it keeps it readable over whatever the tiles happen to show.
    let text = egui::RichText::new(&map.attribution).size(10.0);
    let galley = painter.layout_no_wrap(
        map.attribution.clone(),
        egui::FontId::proportional(10.0),
        colors::TEXT,
    );
    let credit = egui::Rect::from_min_size(
        rect.right_bottom() - galley.size() - egui::vec2(space::XS, space::XS),
        galley.size(),
    );
    painter.rect_filled(credit.expand(space::XS / 2.0), 2.0, colors::BG_PANEL);
    match &map.attribution_url {
        // Registered after the map, so a click on the credit follows the link
        // instead of dropping the anchor behind it.
        Some(url) => {
            ui.put(credit, egui::Hyperlink::from_label_and_url(text, url));
        }
        None => {
            painter.galley(credit.min, galley, colors::TEXT);
        }
    }
    ui.label(
        egui::RichText::new(t!("new-module-map-hint"))
            .small()
            .color(colors::TEXT_SECONDARY),
    );
    // Tiles arrive on other threads; keep the frames coming while any are out.
    if map.source.pending() > 0 || dialog.searching {
        ui.ctx().request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A wheel notch is one zoom level, whichever unit the platform reports it
    /// in — the pixel-counting one is what used to race through the stack.
    #[test]
    fn a_notch_is_one_zoom_level() {
        let wheel = |unit, y| egui::Event::MouseWheel {
            unit,
            delta: egui::vec2(0.0, y),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        };
        assert_eq!(notches(&wheel(egui::MouseWheelUnit::Line, 1.0)), 1.0);
        assert_eq!(notches(&wheel(egui::MouseWheelUnit::Point, NOTCH)), 1.0);
        assert_eq!(notches(&wheel(egui::MouseWheelUnit::Point, -NOTCH)), -1.0);
        // Below a notch nothing happens yet; the rest is banked for later.
        let mut banked = notches(&wheel(egui::MouseWheelUnit::Point, NOTCH * 0.4));
        assert_eq!(banked as i32, 0);
        banked += notches(&wheel(egui::MouseWheelUnit::Point, NOTCH * 0.7));
        assert_eq!(banked as i32, 1);
    }
}
