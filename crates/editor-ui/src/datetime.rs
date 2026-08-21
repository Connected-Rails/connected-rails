//! Date and time of day as bar controls: a month calendar behind a button, and
//! the day on a rail with the sun as its handle.
//!
//! They edit the same two values as the sky section of the form panel, and the
//! bar is where they belong while the map is being looked at: what they change
//! is the picture, not the document. Dragging the sun over the rail is how a
//! builder finds out that the platform lies in the shadow of its own canopy all
//! morning — with the map in front of them rather than a panel.
//!
//! The two symbols are typed, not drawn: they come from the Phosphor icon font
//! bundled by [`crate::apply`] (see `icon.rs` for why everything else is drawn,
//! and THIRD_PARTY_LICENSES.md for the licence).

use bevy_egui::egui::{
    self, Align2, CornerRadius, Rect, Response, RichText, Sense, Ui, Vec2, pos2, vec2,
};
use i18n::t;

use crate::colors;
use crate::icon::BUTTON;

/// Diameter of the sun on the rail. The rail is this much shorter than the
/// widget, so the sun stays inside it at midnight as well as at noon.
const SUN: f32 = 18.0;
/// Thickness of the rail the sun runs on.
const RAIL: f32 = 4.0;
/// A day of the calendar grid: two digits with air around them, and the height
/// of every other control in a bar.
const CELL: Vec2 = Vec2::new(26.0, 20.0);
/// Width of the calendar: seven days and the gaps between them. The popup
/// takes its width from this, so the header above the grid gets the same.
const GRID: f32 = CELL.x * 7.0 + GAP * 6.0;
/// Air between two days of the calendar.
const GAP: f32 = 2.0;
/// Width of the date button — four groups of digits and the calendar leaf.
const DATE: f32 = 112.0;
/// Width of the clock beside the rail. Fixed, so the rail does not jump when
/// the hour goes from 9 to 10.
const CLOCK: f32 = 36.0;
/// Length of the rail: a full day, still narrow enough to leave the status
/// message the width it needs.
const DAY: f32 = 132.0;

/// Date, clock and the day on its rail — the light over the module, as one
/// group for a bar. `true` while any of the three was moved.
///
/// The caller passes the same fields its form panel edits; the sizes stay in
/// here, so every bar that shows the sky shows it at the same width.
pub fn day_controls(
    ui: &mut Ui,
    year: &mut i32,
    month: &mut u32,
    day: &mut u32,
    hours: &mut f64,
) -> bool {
    let mut changed = date_picker(ui, year, month, day);
    ui.add_sized(
        vec2(CLOCK, BUTTON.y),
        egui::Label::new(RichText::new(clock(*hours)).color(colors::TEXT)),
    );
    changed |= sun_slider(ui, hours, DAY).changed();
    changed
}

/// A symbol as button text: the icon family at the size of the text beside it.
fn glyph(symbol: &str) -> RichText {
    RichText::new(symbol).font(crate::icon_font(14.0))
}

/// The clock as it is read off the rail: whole minutes, 24 hours, both digits.
fn clock(hours: f64) -> String {
    let minutes = (hours * 60.0).floor().rem_euclid(24.0 * 60.0) as u32;
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

/// The clock as a rail with the sun on it; `hours` is local time in `0..24`.
///
/// Not an `egui::Slider`: its handle is a rectangle, and the point here is that
/// the thing being dragged *is* the sun. A click anywhere on the rail jumps to
/// that hour, the same way a slider does.
fn sun_slider(ui: &mut Ui, hours: &mut f64, width: f32) -> Response {
    let (rect, mut response) =
        ui.allocate_exact_size(vec2(width, BUTTON.y), Sense::click_and_drag());
    let rail = Rect::from_center_size(rect.center(), vec2(rect.width() - SUN, RAIL));
    if let Some(pointer) = response.interact_pointer_pos() {
        let fraction = ((pointer.x - rail.left()) / rail.width()).clamp(0.0, 1.0);
        let dragged = f64::from(fraction) * 24.0;
        if dragged != *hours {
            *hours = dragged;
            response.mark_changed();
        }
    }

    let fraction = (*hours / 24.0).clamp(0.0, 1.0) as f32;
    let sun = pos2(rail.left() + rail.width() * fraction, rect.center().y);
    let painter = ui.painter();
    painter.rect_filled(rail, CornerRadius::same(2), colors::BG_INPUT);
    // The part of the day already run, the way egui's own trailing fill draws it.
    painter.rect_filled(
        Rect::from_min_max(rail.min, pos2(sun.x, rail.max.y)),
        CornerRadius::same(2),
        colors::ACCENT_BG,
    );
    painter.text(
        sun,
        Align2::CENTER_CENTER,
        egui_phosphor::regular::SUN,
        crate::icon_font(SUN),
        if response.hovered() || response.dragged() {
            colors::TEXT_STRONG
        } else {
            colors::TEXT
        },
    );
    response.on_hover_text(t!("sky-scrub"))
}

/// The date as a button that opens a month calendar; `true` while the popup
/// changed it.
///
/// Browsing a month already moves the date, so the sky follows the calendar
/// while it is open — a builder pages through the year and watches the shadows,
/// instead of picking a day and finding out afterwards.
fn date_picker(ui: &mut Ui, year: &mut i32, month: &mut u32, day: &mut u32) -> bool {
    let date = t!(
        "cal-date",
        day = format!("{day:02}"),
        month = format!("{month:02}"),
        year = year.to_string(),
    );
    let button = ui
        .add_sized(
            vec2(DATE, BUTTON.y),
            egui::Button::new((glyph(egui_phosphor::regular::CALENDAR_BLANK), date)),
        )
        .on_hover_text(t!("sky-date-hint"));
    let mut changed = false;
    egui::Popup::menu(&button)
        .layout(egui::Layout::top_down(egui::Align::Min))
        .show(|ui| changed = calendar(ui, year, month, day));
    changed
}

/// The popup: a month at a time, weeks running Monday to Sunday.
fn calendar(ui: &mut Ui, year: &mut i32, month: &mut u32, day: &mut u32) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        if ui
            .add_sized(
                CELL,
                egui::Button::new(glyph(egui_phosphor::regular::CARET_LEFT)),
            )
            .clicked()
        {
            step_month(year, month, day, -1);
            changed = true;
        }
        // The month fills what the two carets leave, so it sits over the
        // middle of the grid below rather than wherever its name ends.
        let width = GRID - 2.0 * CELL.x - 2.0 * ui.spacing().item_spacing.x;
        ui.add_sized(
            vec2(width, CELL.y),
            egui::Label::new(
                RichText::new(format!("{} {year}", t!(&format!("cal-month-{month}"))))
                    .color(colors::TEXT),
            )
            .halign(egui::Align::Center),
        );
        if ui
            .add_sized(
                CELL,
                egui::Button::new(glyph(egui_phosphor::regular::CARET_RIGHT)),
            )
            .clicked()
        {
            step_month(year, month, day, 1);
            changed = true;
        }
    });

    egui::Grid::new("calendar")
        .num_columns(7)
        // A day is as wide as it is, not as wide as the widest widget of the
        // style — `interact_size` would give each column the width of a text
        // field and the month five hundred pixels.
        .min_col_width(CELL.x)
        .max_col_width(CELL.x)
        .spacing(Vec2::splat(GAP))
        .show(ui, |ui| {
            for weekday in 1..=7 {
                ui.add_sized(
                    CELL,
                    egui::Label::new(
                        RichText::new(t!(&format!("cal-weekday-{weekday}")))
                            .small()
                            .color(colors::TEXT_SECONDARY),
                    )
                    .halign(egui::Align::Center),
                );
            }
            ui.end_row();

            let mut column = weekday(*year, *month, 1);
            for _ in 0..column {
                ui.add_sized(CELL, egui::Label::new(""));
            }
            for candidate in 1..=days_in_month(*year, *month) {
                let cell = egui::Button::selectable(candidate == *day, candidate.to_string());
                if ui.add_sized(CELL, cell).clicked() {
                    *day = candidate;
                    changed = true;
                    ui.close();
                }
                column += 1;
                if column.is_multiple_of(7) {
                    ui.end_row();
                }
            }
        });
    changed
}

/// A month forward or back, carrying into the year. A day past the end of the
/// month it lands in moves back to that month's last — the 31st of a January
/// paged into February is the 28th, not a date that does not exist.
fn step_month(year: &mut i32, month: &mut u32, day: &mut u32, delta: i32) {
    let months = *month as i32 - 1 + delta;
    *year += months.div_euclid(12);
    *month = months.rem_euclid(12) as u32 + 1;
    *day = (*day).min(days_in_month(*year, *month));
}

/// Length of a month, Gregorian leap years included.
fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        _ => 28,
    }
}

/// Day of the week of a civil date, `0` = Monday — which column the first of
/// the month starts in. Days since the epoch after Howard Hinnant's
/// `days_from_civil`; 1 January 1970 was a Thursday, hence the offset.
fn weekday(year: i32, month: u32, day: u32) -> u32 {
    let (year, month, day) = (i64::from(year), i64::from(month), i64::from(day));
    // March-based years: the leap day then falls at the end, where it costs no
    // case of its own.
    let year = year - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    (days + 3).rem_euclid(7) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weekdays_of_known_dates() {
        assert_eq!(weekday(1970, 1, 1), 3, "Thursday");
        assert_eq!(weekday(2000, 1, 1), 5, "Saturday");
        assert_eq!(weekday(2024, 2, 29), 3, "Thursday");
        assert_eq!(weekday(1969, 7, 20), 6, "Sunday");
    }

    #[test]
    fn the_clock_reads_off_the_rail() {
        assert_eq!(clock(0.0), "00:00");
        assert_eq!(clock(6.5), "06:30");
        assert_eq!(clock(23.999), "23:59");
        assert_eq!(clock(24.0), "00:00");
    }

    #[test]
    fn month_lengths() {
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2025, 2), 28);
        assert_eq!(days_in_month(1900, 2), 28);
        assert_eq!(days_in_month(2000, 2), 29);
        assert_eq!(days_in_month(2025, 4), 30);
    }

    #[test]
    fn paging_carries_the_year_and_keeps_the_day_real() {
        let (mut year, mut month, mut day) = (2025, 1, 31);
        step_month(&mut year, &mut month, &mut day, 1);
        assert_eq!((year, month, day), (2025, 2, 28));
        let (mut year, mut month, mut day) = (2025, 1, 15);
        step_month(&mut year, &mut month, &mut day, -1);
        assert_eq!((year, month, day), (2024, 12, 15));
    }
}
