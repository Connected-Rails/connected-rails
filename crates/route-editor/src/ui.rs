//! Desktop UI of the route editor: menu bar, info panel, status bar.
//!
//! The editor is an application, not a game screen — everything reachable through the
//! keyboard is in the menu as well, and the file dialogs are the operating system's own.

use crate::overlay::Overlay;
use crate::{Focus, Line, Request, focus_degrees};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use imagery::ZoomMode;

/// One frame of UI. Panels live inside a background `Ui` (egui 0.35).
pub fn draw(
    mut contexts: EguiContexts,
    mut request: ResMut<Request>,
    overlay: Res<Overlay>,
    focus: Res<Focus>,
    line: Res<Line>,
) -> Result {
    let ctx = contexts.ctx_mut()?.clone();
    let mut root = egui::Ui::new(
        ctx.clone(),
        "viewport".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );

    egui::Panel::top("menu").show(&mut root, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Open line…").clicked() {
                    request.open_line = rfd::FileDialog::new()
                        .add_filter("Line (RON)", &["ron"])
                        .pick_file();
                    ui.close();
                }
                ui.separator();
                if ui.button("Load imagery configuration (F5)").clicked() {
                    request.load_config = true;
                    ui.close();
                }
                if ui.button("Save imagery configuration (F2)").clicked() {
                    request.save_config = true;
                    ui.close();
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    std::process::exit(0);
                }
            });
            ui.menu_button("Overlay", |ui| {
                if ui.button("On/off (O)").clicked() {
                    request.toggle_overlay = true;
                    ui.close();
                }
                if ui.button("Next provider (P)").clicked() {
                    request.cycle_provider = true;
                    ui.close();
                }
                if ui.button("Offline mode (L)").clicked() {
                    request.toggle_offline = true;
                    ui.close();
                }
                ui.separator();
                if ui.button("Clear cache (C)").clicked() {
                    request.clear_cache = true;
                    ui.close();
                }
                if ui.button("Reset failed attempts (R)").clicked() {
                    request.retry_failed = true;
                    ui.close();
                }
            });
            ui.menu_button("Help", |ui| {
                ui.label("WASD/arrows pan · PgUp/PgDn height");
                ui.label("[ ] opacity · , . zoom level · Z automatic");
                ui.label("Numpad 4/6/8/2 image offset, 5 reset");
            });
        });
    });

    egui::Panel::bottom("status").show(&mut root, |ui| {
        ui.horizontal(|ui| {
            ui.label(if overlay.status.is_empty() {
                "Ready"
            } else {
                overlay.status.as_str()
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (lat, lon) = focus_degrees(focus.position);
                ui.label(format!(
                    "{lat:.5}°, {lon:.5}°   height {:.0} m",
                    focus.height
                ));
            });
        });
    });

    egui::Panel::left("info")
        .default_size(360.0)
        .resizable(true)
        .show(&mut root, |ui| {
            let config = overlay.config();
            let provider = config.provider();
            let stats = overlay.source.cache_stats();

            ui.heading("Line");
            ui.label(format!("{} · {} edges", line.name, line.net.edges().len()));
            if let Some(path) = &line.path {
                ui.small(path);
            }

            ui.separator();
            ui.heading("Aerial imagery");
            egui::Grid::new("imagery").num_columns(2).show(ui, |ui| {
                ui.label("Provider");
                ui.label(provider.map(|p| p.name.as_str()).unwrap_or("—"));
                ui.end_row();
                ui.label("Status");
                ui.label(if config.enabled { "on" } else { "off" });
                ui.end_row();
                ui.label("Opacity");
                ui.label(format!("{:.0} %", config.opacity * 100.0));
                ui.end_row();
                ui.label("Zoom");
                ui.label(format!(
                    "{} ({})",
                    overlay.zoom,
                    match config.zoom {
                        ZoomMode::Fixed(_) => "fixed".to_string(),
                        ZoomMode::Resolution(m) => format!("{m:.2} m/px"),
                    }
                ));
                ui.end_row();
                ui.label("Tiles");
                ui.label(format!(
                    "{} shown, {} in flight",
                    overlay.tiles_shown(),
                    overlay.source.pending()
                ));
                ui.end_row();
                ui.label("Offset");
                ui.label(format!(
                    "{:+.1} / {:+.1} m",
                    config.offset.0, config.offset.1
                ));
                ui.end_row();
                ui.label("Mode");
                ui.label(if config.cache.offline {
                    "offline"
                } else {
                    "online"
                });
                ui.end_row();
            });

            ui.separator();
            ui.heading("Cache");
            ui.label(format!(
                "{} hits ({} from disk), {} stored, {} evicted",
                stats.hits_memory + stats.hits_disk,
                stats.hits_disk,
                stats.stored,
                stats.evicted
            ));
            ui.label(format!(
                "{:.1} MB in {}",
                overlay.source.disk_usage() as f64 / 1e6,
                config.cache.directory.display()
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
                ui.label(egui::RichText::new("Errors").strong());
                for error in errors {
                    ui.small(error);
                }
            }
        });
    Ok(())
}
