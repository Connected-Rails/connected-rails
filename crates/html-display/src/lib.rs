//! HTML/CSS/JS cab displays (plan ch. 12) — the MSFS authoring model without
//! the browser: one `.html` file per screen, parsed into an own DOM, laid out
//! with taffy (flexbox), scripted by an embedded ECMAScript engine (boa), and
//! painted as a flat command list that the app's existing display pipeline
//! renders to texture. No browser process, no IPC; layout and paint run only
//! when the DOM actually changed, which is what keeps it fast.
//!
//! This is the third content path of a cab display, above widgets and the Lua
//! hook: widgets for values and bars, Lua for menu logic, HTML for screens
//! with real layout (EBuLa-like pages, tables, nested menus).
//!
//! # The subset, in one place
//!
//! **HTML**: any tag names (only their style matters), `id`, `class`, `style`
//! attributes, `<style>` blocks, one `<script>` block (inline ECMAScript),
//! text nodes. Unknown attributes are ignored.
//!
//! **Live bindings without script**: `data-bind="<sim field>"` replaces the
//! element's text every tick, formatted by `data-format` (printf subset:
//! `%d`, `%s`, `%.Nf`); `data-show="<sim field>"` hides the element while the
//! value is 0/false. Fields are the flat names of [`SimFrame`]: `v_kmh`,
//! `brake_pipe`, `value.mfa_v_soll`, `lamp.pzb_1000hz`, `time`, …
//!
//! **CSS** (in `<style>` and `style=`): selectors `tag`, `.class`, `#id`,
//! compound `tag.class`, comma lists; specificity id > class > tag, then
//! source order. Properties: `display` (`flex`|`block`|`none`),
//! `flex-direction` (`row`|`column`), `justify-content` (`flex-start`|
//! `center`|`flex-end`|`space-between`), `align-items` (`flex-start`|
//! `center`|`flex-end`|`stretch`), `flex-grow`, `gap`, `width`, `height`
//! (px or %), `padding`, `margin` (px, 1/2/4 values), `position`
//! (`absolute` + `left`/`top`/`right`/`bottom` px), `background-color`,
//! `color`, `font-size` (px), `text-align` (`left`|`center`|`right`),
//! `border` (`<N>px solid <color>`), `visibility` (`hidden`), `opacity`.
//! Colors: `#rgb`, `#rrggbb`, `#rrggbbaa`, `rgb()`, `rgba()`, and the usual
//! named handful (`black`, `white`, `red`, `green`, `blue`, `yellow`,
//! `orange`, `cyan`, `magenta`, `gray`/`grey`, `darkgray`, `lightgray`).
//!
//! **Script API** (the whole surface — nothing else is injected):
//! `document.getElementById(id)`; on an element: `textContent` (get/set),
//! `getAttribute`/`setAttribute`, `classList.add/remove/toggle/contains`,
//! `style.setProperty(name, value)`, `hidden` (bool); the global `sim` object
//! carries every [`SimFrame`] number and lamp as a property plus
//! `sim.button(1..=8)`; `onFrame(fn)` runs every tick, `onButton(fn(index,
//! pressed))` on every softkey edge. Script errors disable the handler that
//! raised them and are reported through [`HtmlGauge::take_errors`]; the last
//! good picture stays on screen.
//!
//! Text metrics use the app's monospaced display font (0.6 em advance), so
//! text measures exactly and layout stays deterministic.

mod dom;
mod js;
mod layout;
mod paint;
mod style;

/// One reading of the simulation for the gauge scripts, flat names as the
/// Lua display hook uses them (`v_kmh`, `value.mfa_v_soll`, `lamp.sifa`, …).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SimFrame {
    /// Simulation time [s].
    pub time: f64,
    /// Numeric fields, name → value.
    pub numbers: Vec<(String, f64)>,
    /// Indicator lamps, name → lit.
    pub lamps: Vec<(String, bool)>,
    /// Held state of the display softkeys (`CabControl::Display`).
    pub buttons: [bool; 8],
}

/// One paint command, pixels from the top left — the app maps these 1:1 onto
/// its display draw list.
#[derive(Debug, Clone, PartialEq)]
pub enum PaintCmd {
    /// Background of the whole screen.
    Clear { color: [f32; 4] },
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: [f32; 4],
        filled: bool,
    },
    /// Text anchored at its top-left corner (alignment is already resolved
    /// into `x` by the layout).
    Text {
        x: f32,
        y: f32,
        text: String,
        size: f32,
        color: [f32; 4],
    },
}

/// A loaded HTML display: DOM, styles, script state and the last picture.
pub struct HtmlGauge {
    inner: js::Runtime,
}

impl HtmlGauge {
    /// Parses the document and runs its `<script>` once. `Err` carries a
    /// message for the mod log; a gauge that fails to load renders nothing
    /// and the display falls back to its other content paths.
    pub fn new(source: &str, width: f32, height: f32) -> Result<Self, String> {
        Ok(Self {
            inner: js::Runtime::new(source, width, height)?,
        })
    }

    /// One display tick: updates `data-bind`/`data-show`, fires `onButton`
    /// edges and `onFrame`, and — only if any of that changed the DOM —
    /// relayouts and repaints. `None` means the picture is unchanged.
    pub fn tick(&mut self, frame: &SimFrame) -> Option<Vec<PaintCmd>> {
        self.inner.tick(frame)
    }

    /// Script errors collected since the last call, each reported once.
    pub fn take_errors(&mut self) -> Vec<String> {
        self.inner.take_errors()
    }
}
