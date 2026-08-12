//! Desktop UI of the route editor: menu bar, info panel, status bar.
//!
//! The editor is an application, not a game screen — everything reachable through the
//! keyboard is in the menu as well, and the file dialogs are the operating system's own.

use crate::overlay::Overlay;
use crate::{Focus, Line, Request, focus_degrees};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use i18n::t;
use imagery::ZoomMode;

/// One frame of UI. Panels live inside a background `Ui` (egui 0.35).
pub fn draw(
    mut contexts: EguiContexts,
    mut request: ResMut<Request>,
    overlay: Res<Overlay>,
    focus: Res<Focus>,
    line: Res<Line>,
    mut themed: Local<bool>,
) -> Result {
    let ctx = contexts.ctx_mut()?.clone();
    if !*themed {
        // Fonts installed by `apply` become active with the next pass — skip
        // one frame so nothing draws with a font family that is not there yet.
        editor_ui::apply(&ctx);
        *themed = true;
        return Ok(());
    }
    let mut root = egui::Ui::new(
        ctx.clone(),
        "viewport".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );

    egui::Panel::top("menu")
        .frame(editor_ui::bar_frame())
        .show(&mut root, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button(t!("menu-file"), |ui| {
                    if ui.button(t!("action-open-line")).clicked() {
                        request.open_line = rfd::FileDialog::new()
                            .add_filter(t!("filter-line-ron"), &["ron"])
                            .pick_file();
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(t!("action-load-imagery")).clicked() {
                        request.load_config = true;
                        ui.close();
                    }
                    if ui.button(t!("action-save-imagery")).clicked() {
                        request.save_config = true;
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(t!("action-quit")).clicked() {
                        std::process::exit(0);
                    }
                });
                ui.menu_button(t!("menu-overlay"), |ui| {
                    if ui.button(t!("overlay-toggle")).clicked() {
                        request.toggle_overlay = true;
                        ui.close();
                    }
                    if ui.button(t!("overlay-next-provider")).clicked() {
                        request.cycle_provider = true;
                        ui.close();
                    }
                    if ui.button(t!("overlay-offline")).clicked() {
                        request.toggle_offline = true;
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(t!("overlay-clear-cache")).clicked() {
                        request.clear_cache = true;
                        ui.close();
                    }
                    if ui.button(t!("overlay-retry")).clicked() {
                        request.retry_failed = true;
                        ui.close();
                    }
                });
                ui.menu_button(t!("menu-view"), language_menu);
                ui.menu_button(t!("menu-help"), |ui| {
                    ui.label(t!("help-pan"));
                    ui.label(t!("help-opacity"));
                    ui.label(t!("help-offset"));
                });
            });
        });

    egui::Panel::bottom("status")
        .frame(editor_ui::bar_frame())
        .show(&mut root, |ui| {
            ui.horizontal(|ui| {
                ui.label(if overlay.status.is_empty() {
                    t!("status-ready")
                } else {
                    overlay.status.clone()
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (lat, lon) = focus_degrees(focus.position);
                    ui.label(t!(
                        "status-position",
                        lat = format!("{lat:.5}"),
                        lon = format!("{lon:.5}"),
                        height = format!("{:.0}", focus.height),
                    ));
                });
            });
        });

    egui::Panel::left("info")
        .default_size(360.0)
        .resizable(true)
        .frame(editor_ui::panel_frame())
        .show(&mut root, |ui| {
            let config = overlay.config();
            let provider = config.provider();
            let stats = overlay.source.cache_stats();

            ui.heading(t!("heading-line"));
            ui.label(t!(
                "line-summary",
                name = line.name,
                edges = line.net.edges().len()
            ));
            if let Some(path) = &line.path {
                ui.small(path);
            }

            ui.separator();
            ui.heading(t!("heading-imagery"));
            egui::Grid::new("imagery").num_columns(2).show(ui, |ui| {
                ui.label(t!("img-provider"));
                ui.label(
                    provider
                        .map(|p| p.name.clone())
                        .unwrap_or_else(|| t!("common-none")),
                );
                ui.end_row();
                ui.label(t!("img-status"));
                ui.label(if config.enabled {
                    t!("common-on")
                } else {
                    t!("common-off")
                });
                ui.end_row();
                ui.label(t!("img-opacity"));
                ui.label(format!("{:.0} %", config.opacity * 100.0));
                ui.end_row();
                ui.label(t!("img-zoom"));
                ui.label(format!(
                    "{} ({})",
                    overlay.zoom,
                    match config.zoom {
                        ZoomMode::Fixed(_) => t!("zoom-fixed"),
                        ZoomMode::Resolution(m) =>
                            t!("zoom-resolution", metres = format!("{m:.2}")),
                    }
                ));
                ui.end_row();
                ui.label(t!("img-tiles"));
                ui.label(t!(
                    "tiles-summary",
                    shown = overlay.tiles_shown(),
                    pending = overlay.source.pending()
                ));
                ui.end_row();
                ui.label(t!("img-offset"));
                ui.label(format!(
                    "{:+.1} / {:+.1} m",
                    config.offset.0, config.offset.1
                ));
                ui.end_row();
                ui.label(t!("img-mode"));
                ui.label(if config.cache.offline {
                    t!("mode-offline")
                } else {
                    t!("mode-online")
                });
                ui.end_row();
            });

            ui.separator();
            ui.heading(t!("heading-cache"));
            ui.label(t!(
                "cache-summary",
                hits = stats.hits_memory + stats.hits_disk,
                disk = stats.hits_disk,
                stored = stats.stored,
                evicted = stats.evicted
            ));
            ui.label(t!(
                "cache-size",
                megabytes = format!("{:.1}", overlay.source.disk_usage() as f64 / 1e6),
                directory = config.cache.directory.display()
            ));

            if let Some(provider) = provider
                && !provider.attribution.is_empty()
            {
                ui.separator();
                ui.small(format!("© {}", provider.attribution));
            }
            let errors: Vec<&String> = overlay.source.errors.iter().rev().take(3).collect();
            if !errors.is_empty() {
                ui.separator();
                ui.label(egui::RichText::new(t!("group-errors")).strong());
                for error in errors {
                    ui.small(error);
                }
            }
        });
    Ok(())
}

/// Language picker.
fn language_menu(ui: &mut egui::Ui) {
    ui.menu_button(t!("menu-language"), |ui| {
        let current = i18n::language();
        for (code, name) in i18n::LANGUAGES {
            if ui.selectable_label(current == *code, *name).clicked() {
                i18n::set_language(code);
                ui.close();
            }
        }
    });
}
