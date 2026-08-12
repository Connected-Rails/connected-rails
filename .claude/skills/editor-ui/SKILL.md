---
name: editor-ui
description: Design system and workflow for the desktop editor UIs (vehicle editor, route editor, bevy_egui). Use whenever building or changing editor UI — panels, forms, styling, colors, fonts, spacing — or when reviewing editor screens for visual consistency.
---

# Editor UI design system

Both desktop editors (vehicle editor, route editor) share one look, defined in
the `editor-ui` crate (`crates/editor-ui/src/lib.rs`). **Never** hard-code a
color, font size or magic spacing in editor code — take the token or helper
from `editor-ui`. If a token is missing, add it there so both editors get it.

## Applying the theme

Each editor's `draw` system applies the theme once via a `Local<bool>`:

```rust
if !*themed {
    editor_ui::apply(&ctx);   // fonts + style
    *themed = true;
    return Ok(());            // fonts activate next pass — skip one frame
}
```

The early return is required: text drawn with the `semibold` family in the
same frame panics, because `set_fonts` only takes effect on the next pass.

## Color tokens (`editor_ui::colors`)

Dark, neutral surfaces; one restrained blue accent. All text ≥ WCAG AA on its
surface (body ~12:1, secondary ~6.4:1).

| Token | Hex | Use |
|---|---|---|
| `BG_INPUT` | `#15161A` | text edits, slider rails — the "wells" |
| `BG_PANEL` | `#1D1F24` | side panels, menu bar, status bar |
| `BG_CARD` | `#24262C` | list-entry cards, popups/menus |
| `BG_WIDGET` | `#2B2E36` | buttons and drag values at rest |
| `BG_HOVER` / `BG_ACTIVE` | `#353942` / `#3F444F` | interaction states |
| `BORDER` / `BORDER_SUBTLE` | `#3A3E47` / `#2E3139` | strokes |
| `TEXT` | `#E6E8EC` | body text, values |
| `TEXT_STRONG` | `#FFFFFF` | headings, hovered text |
| `TEXT_SECONDARY` | `#A6ACB8` | form labels, hints, de-emphasis |
| `ACCENT` | `#5C9CF5` | focus ring, links, text cursor |
| `ACCENT_BG` / `ACCENT_TEXT` | `#2F5DA8` / `#EAF2FF` | selected items and their text |
| `WARN` / `ERROR` | `#E2B44C` / `#E86E66` | unsaved marker, errors |

The 3D viewport clear color (`ClearColor(Color::srgb(0.16, 0.17, 0.19))` in
the vehicle editor) sits slightly lighter than `BG_PANEL` so panel edges stay
readable without a border.

## Typography

Inter, bundled in `crates/editor-ui/fonts/` (OFL license file next to it).
Two weights only:

- **Inter Regular** — everything (Body/Button 13 px, Small 11 px).
- **Inter SemiBold** — headings and titles only, via `editor_ui::semibold()`
  (Heading 15 px, section titles 13 px, subheadings 11.5 px).
- **Monospace 12.5 px** (egui's Hack) — file paths, glTF node names,
  identifiers. Never for prose.

Do not add weights or sizes; hierarchy comes from these plus color
(`TEXT_STRONG` > `TEXT` > `TEXT_SECONDARY`).

## Spacing (`editor_ui::space`)

4 px base grid: `XS 4 · S 8 · M 12 · L 16 · XL 24`. Panels have 12 px padding
(`panel_frame`), bars 8×5 (`bar_frame`), cards 8 (`card_frame`). Grid spacing
is 12×6, `item_spacing` 8×6, widget min size 84×22, `LABEL_COL 168`.
Use `ui.add_space(space::…)` — never a bare float.

## Building a form panel

The vehicle editor's left panel is the reference implementation
(`crates/vehicle-editor/src/ui.rs`, `powertrain.rs`):

- Panel: `egui::Panel::left(..).frame(editor_ui::panel_frame())`, whole
  content in **one** `ScrollArea` (`.auto_shrink([false; 2])`). Never nest
  scroll areas.
- Panel title: `ui.label(editor_ui::heading(t!(…)))`.
- Section: `editor_ui::section(ui, "id", t!("group-…"), |ui| …)` — a
  collapsible header, default open. Sub-groups inside a section use
  `editor_ui::subheading` (no upper-casing — titles may carry units).
- Rows: `row(ui, "key", |ui| …)` inside `editor_ui::form_grid("id")`. The
  label comes from i18n key `key`, the tooltip from `key-hint`, and
  `form_label` gives every grid the same 168 px label column so fields align
  across sections.
- Numbers: `editor_ui::drag(&mut v, speed, range, "unit")`. The unit is a
  symbol (`"kg"`, `"km/h"`, `"N·m"`, `"l/min"`, `""` for ratios) — symbols are
  names, not prose, so they stay literal in code. Fields stepped in whole
  numbers (`speed >= 1`) automatically get digit grouping (`1 840 000`).
- Editable lists (moving parts, converter circuits): one
  `editor_ui::card_frame()` per entry, header row with identifier left and a
  small `"×"` delete button right (`Layout::right_to_left`).
- Status bar: message left; on the right the path in `TEXT_SECONDARY` and the
  unsaved marker in `colors::WARN`.

## Hard-won rules

- **Glyphs:** use `"×"` (U+00D7) for delete, not `"✕"` (U+2715) — Inter lacks
  the latter and fallback is unreliable. Between digits and before units use
  a no-break space `\u{A0}` (`editor_ui::NBSP`), not the narrow `\u{202F}`:
  the shaper drops the narrow one after some digits.
- **i18n:** every visible string through `i18n::t!`, keys in *both*
  `crates/i18n/locales/{en,de}/main.ftl` in the same commit
  (`cargo test -p i18n` enforces parity). Tooltips are `<key>-hint`.
- **Units visible, not hidden:** the unit lives on the field
  (`drag(…, "bar")`); the tooltip explains provenance, not the unit alone.
- **No per-widget styling** beyond the helpers. If something needs a new
  look, extend `editor-ui` and document it here.

## Verifying changes (screenshot loop)

Both editors render headless screenshots — iterate visually, don't guess:

```
cargo run -p vehicle-editor --features dev -- \
    mods/example/vehicles/br101_afb.ron \
    --window 1380x2400 --frames 90 --screenshot target/ui-iter/step.png
```

`--window WxH` sizes the window (tall windows show the whole form),
`--frames 90` exits after 90 frames. The route editor takes the same flags.
Look at the PNG at high zoom and check:

1. Field columns align across *all* sections (168 px label column).
2. Every physical quantity shows its unit; big integers are digit-grouped.
3. Hierarchy reads: heading > section title > subheading > label.
4. Spacing is from the 4 px scale — no accidental 3 px or 17 px gaps.
5. No tofu/missing glyphs (watch delete buttons and unit suffixes).
