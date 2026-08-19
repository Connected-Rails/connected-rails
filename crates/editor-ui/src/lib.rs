//! Shared look and feel of the Connected Rails desktop editors.
//!
//! One place defines the colors, typography and spacing of every editor
//! window; both editors call [`apply`] once on their egui context and get the
//! same appearance. The design tokens and the rules for using them are
//! documented in `.claude/skills/editor-ui/SKILL.md`.

use bevy_egui::egui::{
    self, Color32, CornerRadius, FontFamily, FontId, Margin, RichText, Stroke, TextStyle,
    epaint::Shadow, vec2,
};

mod curve;
mod icon;
pub use curve::{CurveSpec, Series, curve_editor, multi_plot, sparkline, sparkline_fn};
pub use icon::{Icon, bar_divider, bar_value, icon_button, icon_label};

/// Color tokens: dark neutral surfaces, one restrained blue accent.
///
/// Body text on `BG_PANEL` reaches ~12:1 contrast, `TEXT_SECONDARY` ~6.4:1 —
/// both clear WCAG AA for their sizes. Keep new colors on this scale.
pub mod colors {
    use bevy_egui::egui::Color32;

    // Surfaces, darkest to lightest.
    /// Text edits, slider rails, code — the "wells".
    pub const BG_INPUT: Color32 = Color32::from_rgb(0x15, 0x16, 0x1A);
    /// Side panels, menu bar, status bar.
    pub const BG_PANEL: Color32 = Color32::from_rgb(0x1D, 0x1F, 0x24);
    /// Grouped list entries (cards) and open combo headers.
    pub const BG_CARD: Color32 = Color32::from_rgb(0x24, 0x26, 0x2C);
    /// Buttons and drag values at rest.
    pub const BG_WIDGET: Color32 = Color32::from_rgb(0x2B, 0x2E, 0x36);
    pub const BG_HOVER: Color32 = Color32::from_rgb(0x35, 0x39, 0x42);
    pub const BG_ACTIVE: Color32 = Color32::from_rgb(0x3F, 0x44, 0x4F);

    pub const BORDER: Color32 = Color32::from_rgb(0x3A, 0x3E, 0x47);
    pub const BORDER_SUBTLE: Color32 = Color32::from_rgb(0x2E, 0x31, 0x39);

    pub const TEXT: Color32 = Color32::from_rgb(0xE6, 0xE8, 0xEC);
    pub const TEXT_STRONG: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
    /// Form labels, hints, de-emphasised values.
    pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(0xA6, 0xAC, 0xB8);

    /// Focus rings, links, selected text.
    pub const ACCENT: Color32 = Color32::from_rgb(0x5C, 0x9C, 0xF5);
    /// Fill behind selected items (list rows, combo entries).
    pub const ACCENT_BG: Color32 = Color32::from_rgb(0x2F, 0x5D, 0xA8);
    /// Text on top of `ACCENT_BG`.
    pub const ACCENT_TEXT: Color32 = Color32::from_rgb(0xEA, 0xF2, 0xFF);

    pub const WARN: Color32 = Color32::from_rgb(0xE2, 0xB4, 0x4C);
    pub const ERROR: Color32 = Color32::from_rgb(0xE8, 0x6E, 0x66);
}

/// Spacing tokens on a 4 px base grid.
pub mod space {
    pub const XS: f32 = 4.0;
    pub const S: f32 = 8.0;
    pub const M: f32 = 12.0;
    pub const L: f32 = 16.0;
    pub const XL: f32 = 24.0;
    /// Width of the label column of every form grid — one value, so the
    /// fields of all sections line up.
    pub const LABEL_COL: f32 = 168.0;
    /// Width of every control in a value column: numeric fields and combo
    /// boxes share it, so the column has one clean right edge.
    pub const FIELD: f32 = 150.0;
    /// Narrowest a resizable form panel may be dragged: the panel's own margins,
    /// the section indent, the label column and one field. Below this a form row
    /// no longer fits and the rows break out over the panel's edge.
    pub const PANEL_MIN: f32 = M + M + L + LABEL_COL + M + FIELD;
}

/// Name of the semibold font family registered by [`apply`].
const SEMIBOLD: &str = "semibold";

/// The semibold family — for headings and section titles only.
pub fn semibold() -> FontFamily {
    FontFamily::Name(SEMIBOLD.into())
}

/// Installs fonts and style on the context. Call once at startup; a second
/// call is harmless but rebuilds the font atlas.
pub fn apply(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "Inter".to_owned(),
        egui::FontData::from_static(include_bytes!("../fonts/Inter-Regular.ttf")).into(),
    );
    fonts.font_data.insert(
        "Inter-SemiBold".to_owned(),
        egui::FontData::from_static(include_bytes!("../fonts/Inter-SemiBold.ttf")).into(),
    );
    if let Some(proportional) = fonts.families.get_mut(&FontFamily::Proportional) {
        // Inter first; the egui defaults stay behind it as glyph fallback.
        proportional.insert(0, "Inter".to_owned());
    }
    fonts.families.insert(
        semibold(),
        vec!["Inter-SemiBold".to_owned(), "NotoEmoji-Regular".to_owned()],
    );
    ctx.set_fonts(fonts);
    // One dark style for everyone — the editors do not follow the OS theme.
    ctx.set_theme(egui::Theme::Dark);
    ctx.set_style_of(egui::Theme::Dark, style());
}

fn style() -> egui::Style {
    let text_styles = [
        (
            TextStyle::Small,
            FontId::new(11.0, FontFamily::Proportional),
        ),
        (TextStyle::Body, FontId::new(13.0, FontFamily::Proportional)),
        (
            TextStyle::Button,
            FontId::new(13.0, FontFamily::Proportional),
        ),
        (TextStyle::Heading, FontId::new(15.0, semibold())),
        (
            TextStyle::Monospace,
            FontId::new(12.5, FontFamily::Monospace),
        ),
    ];
    let mut style = egui::Style {
        text_styles: text_styles.into(),
        visuals: visuals(),
        ..Default::default()
    };

    let spacing = &mut style.spacing;
    spacing.item_spacing = vec2(space::S, 6.0);
    spacing.button_padding = vec2(10.0, 4.0);
    spacing.menu_margin = Margin::same(6);
    spacing.window_margin = Margin::same(space::M as i8);
    spacing.indent = space::L;
    // Minimum widget size; the x also is the resting width of a drag value.
    spacing.interact_size = vec2(84.0, 22.0);
    spacing.slider_width = 140.0;
    spacing.combo_width = space::FIELD;
    spacing.tooltip_width = 360.0;

    // egui's floating scroll handle is fully transparent at rest
    // (`dormant_handle_opacity` 0.0). In a panel two to three screens tall
    // that leaves nothing to say there is more below, or how much — the jump
    // bar names the sections but not the distance. Stay floating, so the bar
    // costs no panel width, and just let the handle be seen.
    let mut scroll = egui::style::ScrollStyle::floating();
    scroll.dormant_handle_opacity = 0.55;
    scroll.floating_width = space::XS;
    spacing.scroll = scroll;

    style
}

fn visuals() -> egui::Visuals {
    use colors::*;
    let mut v = egui::Visuals::dark();

    v.panel_fill = BG_PANEL;
    v.window_fill = BG_CARD;
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.window_corner_radius = CornerRadius::same(6);
    v.menu_corner_radius = CornerRadius::same(6);
    let shadow = Shadow {
        offset: [0, 3],
        blur: 12,
        spread: 0,
        color: Color32::from_black_alpha(96),
    };
    v.window_shadow = shadow;
    v.popup_shadow = shadow;

    v.extreme_bg_color = BG_INPUT;
    v.faint_bg_color = BG_CARD;
    v.code_bg_color = BG_INPUT;

    v.selection.bg_fill = ACCENT_BG;
    v.selection.stroke = Stroke::new(1.0, ACCENT_TEXT);
    v.hyperlink_color = ACCENT;
    v.warn_fg_color = WARN;
    v.error_fg_color = ERROR;
    v.text_cursor.stroke = Stroke::new(2.0, ACCENT);

    v.collapsing_header_frame = false;
    v.indent_has_left_vline = false;
    v.striped = false;
    v.slider_trailing_fill = true;

    let corner = CornerRadius::same(4);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = BG_PANEL;
    w.noninteractive.weak_bg_fill = BG_PANEL;
    w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER_SUBTLE);
    w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    w.noninteractive.corner_radius = corner;

    w.inactive.bg_fill = BG_WIDGET;
    w.inactive.weak_bg_fill = BG_WIDGET;
    w.inactive.bg_stroke = Stroke::new(1.0, BORDER_SUBTLE);
    w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    w.inactive.corner_radius = corner;

    w.hovered.bg_fill = BG_HOVER;
    w.hovered.weak_bg_fill = BG_HOVER;
    w.hovered.bg_stroke = Stroke::new(1.0, BORDER);
    w.hovered.fg_stroke = Stroke::new(1.5, TEXT_STRONG);
    w.hovered.corner_radius = corner;
    w.hovered.expansion = 0.0;

    w.active.bg_fill = BG_ACTIVE;
    w.active.weak_bg_fill = BG_ACTIVE;
    w.active.bg_stroke = Stroke::new(1.0, ACCENT);
    w.active.fg_stroke = Stroke::new(1.5, TEXT_STRONG);
    w.active.corner_radius = corner;
    w.active.expansion = 0.0;

    w.open.bg_fill = BG_CARD;
    w.open.weak_bg_fill = BG_CARD;
    w.open.bg_stroke = Stroke::new(1.0, BORDER);
    w.open.fg_stroke = Stroke::new(1.0, TEXT);
    w.open.corner_radius = corner;

    v
}

/// Frame of a side panel: panel fill, even 12 px padding.
pub fn panel_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(colors::BG_PANEL)
        .inner_margin(Margin::same(space::M as i8))
}

/// Frame of the menu and status bars: tighter vertical padding.
pub fn bar_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(colors::BG_PANEL)
        .inner_margin(Margin::symmetric(space::S as i8, 5))
}

/// Card behind one entry of an editable list (a moving part, a circuit).
pub fn card_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(colors::BG_CARD)
        .stroke(Stroke::new(1.0, colors::BORDER_SUBTLE))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::same(space::S as i8))
}

/// Panel heading ("Vehicle", "Model").
pub fn heading(text: impl Into<String>) -> RichText {
    RichText::new(text.into())
        .font(FontId::new(15.0, semibold()))
        .color(colors::TEXT_STRONG)
}

/// Title of a collapsible section.
pub fn section_title(text: impl Into<String>) -> RichText {
    RichText::new(text.into())
        .font(FontId::new(13.0, semibold()))
        .color(colors::TEXT_STRONG)
}

/// A collapsible form section. All sections of a panel share this look.
///
/// A hairline above the title turns the panel from one long list into visible
/// chunks — the rule plus the wider gap group each section's rows together
/// (Gestalt: common region beats proximity alone in a dense form).
///
/// Returns the header's response, so a caller can scroll it into view.
pub fn section(
    ui: &mut egui::Ui,
    id: &str,
    title: impl Into<String>,
    body: impl FnOnce(&mut egui::Ui),
) -> egui::CollapsingResponse<()> {
    ui.add_space(space::M);
    ui.separator();
    ui.add_space(space::XS);
    egui::CollapsingHeader::new(section_title(title))
        .id_salt(id)
        .default_open(true)
        .show(ui, |ui| {
            ui.add_space(2.0);
            body(ui);
            ui.add_space(space::XS);
        })
}

/// Sub-group heading inside a section ("Additional brakes").
///
/// No upper-casing — titles can carry units ("1/min → N·m") and the
/// translations know their own capitalisation.
/// `TEXT`, not `TEXT_SECONDARY`: at 11.5 px it is already smaller than the
/// 13 px labels it heads, and in the same colour it would carry no more weight
/// than the rows underneath it. Each level of the hierarchy has to outrank the
/// next — `TEXT_STRONG` section title > `TEXT` subheading > `TEXT_SECONDARY`
/// label.
pub fn subheading(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.add_space(space::S);
    ui.label(
        RichText::new(text.into())
            .font(FontId::new(11.5, semibold()))
            .color(colors::TEXT),
    );
    ui.add_space(2.0);
}

/// Two-column form grid. Column one is [`space::LABEL_COL`] wide via
/// [`form_label`], so fields line up across sections.
pub fn form_grid(id: &str) -> egui::Grid {
    egui::Grid::new(id.to_owned())
        .num_columns(2)
        .spacing(vec2(space::M, 6.0))
}

/// Label cell of a form row: fixed width, secondary color.
pub fn form_label(ui: &mut egui::Ui, text: impl Into<String>) -> egui::Response {
    ui.horizontal(|ui| {
        ui.set_min_width(space::LABEL_COL);
        ui.label(RichText::new(text.into()).color(colors::TEXT_SECONDARY))
    })
    .inner
}

/// Separator between digit groups and before units: a no-break space.
///
/// U+00A0, not the typographically nicer U+202F — the narrow one comes out of
/// the text shaper with an unreliable width and vanishes after some digits.
pub const NBSP: char = '\u{A0}';

/// Groups digits of integer-valued fields: `3620000` → `3 620 000`, so
/// seven-digit forces and powers stay readable.
pub fn group_digits(value: f64) -> String {
    let negative = value < 0.0;
    let mut digits = format!("{:.0}", value.abs());
    let mut i = digits.len() as isize - 3;
    while i > 0 {
        digits.insert(i as usize, NBSP);
        i -= 3;
    }
    if negative {
        digits.insert(0, '-');
    }
    digits
}

/// Parses what [`group_digits`] produces (and plain user input).
pub fn parse_grouped(text: &str) -> Option<f64> {
    let cleaned: String = text
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '\u{202F}')
        .map(|c| if c == ',' { '.' } else { c })
        .collect();
    cleaned.parse().ok()
}

/// Numeric drag field with a unit suffix. Fields stepped in whole numbers
/// (`speed >= 1`) get grouped digits.
pub fn drag<'a, N: egui::emath::Numeric>(
    value: &'a mut N,
    speed: f64,
    range: std::ops::RangeInclusive<f64>,
    unit: &'static str,
) -> egui::DragValue<'a> {
    let mut d = egui::DragValue::new(value).speed(speed).range(range);
    if !unit.is_empty() {
        d = d.suffix(format!("{NBSP}{unit}"));
    }
    if speed >= 1.0 {
        d = d
            .custom_formatter(|v, _| group_digits(v))
            .custom_parser(parse_grouped);
    }
    d
}

/// Adds a [`drag`] field at the shared [`space::FIELD`] width, so numeric
/// fields and combo boxes in a value column share one footprint.
///
/// The value sits at the field's left edge, not centred: a drag value is wider
/// than its content, and centring makes the distance from the label to the
/// number depend on how long the number is. Left-aligned, every row of the
/// column starts its value at the same x — and at the same x as the text of a
/// combo box, which egui already aligns that way.
pub fn field<N: egui::emath::Numeric>(
    ui: &mut egui::Ui,
    value: &mut N,
    speed: f64,
    range: std::ops::RangeInclusive<f64>,
    unit: &'static str,
) -> egui::Response {
    // `Button` (which a drag value paints itself as) takes its content
    // alignment from the surrounding layout, whose main axis defaults to
    // centre.
    let layout = ui.layout().with_main_align(egui::Align::Min);
    ui.scope_builder(egui::UiBuilder::new().layout(layout), |ui| {
        ui.spacing_mut().interact_size.x = space::FIELD;
        ui.add(drag(value, speed, range, unit))
    })
    .inner
}

#[cfg(test)]
mod tests {
    #[test]
    fn digit_grouping_round_trips() {
        assert_eq!(super::group_digits(3_620_000.0), "3\u{A0}620\u{A0}000");
        assert_eq!(super::group_digits(999.0), "999");
        assert_eq!(super::group_digits(-12_000.0), "-12\u{A0}000");
        assert_eq!(
            super::parse_grouped("3\u{A0}620\u{A0}000"),
            Some(3_620_000.0)
        );
        assert_eq!(super::parse_grouped("12,5"), Some(12.5));
    }
}
