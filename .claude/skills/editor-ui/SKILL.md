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
readable without a border. On top of it a one-metre gizmo grid
(`srgb(0.26, 0.28, 0.31)`, `ground_grid`) gives the eye something to measure
against — length over buffers and axle base are what the form is about, and a
plain grey field shows neither. It sizes itself to the vehicle and switches
off under View.

Until the user has moved the camera, `viewport_hint` puts the mouse controls
in the bottom-left of that free space (`root.available_rect_before_wrap()`,
`.small()` `TEXT_SECONDARY`). Right-drag to orbit is a modelling-tool
convention, not something a modder arriving from a text editor knows, and the
viewport is the one region of the editor with no visible control at all. It
disappears on the first orbit or zoom — an onboarding hint, not furniture.
Reading the free rect this way is fine; what follows is not.

**Do not set `Camera::viewport` to the free space between the panels.**
`bevy_egui` hangs its context on that same camera (`PrimaryEguiContext`), so
the UI is squeezed into the viewport along with the 3D scene and the whole
window collapses into a strip. Separating the two would need a second camera.

## Viewport bar and icons

Controls that belong to *looking* rather than to the document — view angle,
gizmo mode, what the ground shows, camera speed — sit in a bar above the
viewport (`viewport_bar` in the route editor), not in the form panel. Build it
as an `egui::Panel::top` **after** the side panel, inside the same background
`Ui`: it then takes its width from the free space, and `state.viewport`
shrinks by itself, so a click on the bar can never also reach the tool
underneath. Floating it over the map would need its own rect excluded from
every hit test.

Icons are **drawn, not typed** (`editor_ui::Icon` + `icon_button`): Inter
carries no symbol set and the emoji fallback renders tofu on some machines —
the same reason `×` is spelled U+00D7. Drawn shapes also take the theme
colours, so an active button's icon turns with its fill. New icons go into
`crates/editor-ui/src/icon.rs` as unit-coordinate line segments; keep them to
the existing `Stroke` width so a row of them has one weight.

- `icon_button(ui, icon, active, tooltip)` — 26×22, pressed-in (`ACCENT_BG` +
  `ACCENT_TEXT`) while active, so a pair reads as a choice rather than as two
  commands. The tooltip is the only text an icon has: name the function *and*
  its key (`view-imagery = Luftbild auf dem Gelände`).
- `icon_label(ui, icon)` — the icon alone in `TEXT_SECONDARY`, as the label of
  the control beside it. Never an `icon_button` that ignores its click.
- `bar_divider(ui)` — hairline between groups. `ui.separator()` in a
  horizontal layout stretches to the full row height and reads as a panel edge.
- `tool_button(ui, icon, label, active, tooltip)` — icon *and* name, fixed
  156 px so a palette lays out as a grid. A dozen tools cannot be icons alone
  (three of the route editor's are brushes, and no 22 px drawing tells forest
  from marking from terrain), nor text chips alone, which is what wrapped into
  a different shape on every panel width. Group them under `subheading`s.
- `card_entry(ui, mark, title, detail, selected, clickable)` — a catalogue
  entry at a **fixed** 208×60, laid out by hand rather than as a frame around
  labels. A card that sizes itself to its text gives every entry its own width
  and baseline, and a wall of them reads as scattered; text that does not fit
  is truncated with an ellipsis and the caller puts the whole of it in the
  tooltip. Its `Mark` is a drawn icon, a colour (where the entry *is* one — a
  track type), or a `TextureId` (a rendered preview of a model).
- `bar_value(ui, …)` — the compact numeric control of a bar. `field` is a
  fixed 150 px, which is a column width, not a toolbar width; use `field` in
  forms and `bar_value` only in bars.

## Typography

Inter, bundled in `crates/editor-ui/fonts/` (OFL license file next to it).
Two weights only:

- **Inter Regular** — everything (Body/Button 13 px, Small 11 px).
- **Inter SemiBold** — headings and titles only, via `editor_ui::semibold()`
  (Heading 15 px, section titles 13 px, subheadings 11.5 px).
- **Monospace 12.5 px** (egui's Hack) — file paths, glTF node names,
  identifiers. Never for prose.

Do not add weights or sizes; hierarchy comes from these plus color. Size and
colour have to agree, or a level does not outrank the one below it:

| Level | Size | Colour |
|---|---|---|
| Panel heading | 15 semibold | `TEXT_STRONG` |
| Section title | 13 semibold | `TEXT_STRONG` |
| Subheading | 11.5 semibold | `TEXT` |
| Form label | 13 regular | `TEXT_SECONDARY` |
| Value | 13 regular | `TEXT` |

A subheading is *smaller* than the labels beneath it, so it cannot also share
their colour — that was the case until it moved to `TEXT`, and the group
headings read as quieter than their own rows.

## Spacing (`editor_ui::space`)

The scroll bar stays floating (it costs no panel width) but its handle is
visible at rest — egui's default `dormant_handle_opacity` is 0.0, which leaves
a panel that scrolls for pages with no sign that it does, or of how far. The
jump bar names the sections; only the bar shows the distance.

4 px base grid: `XS 4 · S 8 · M 12 · L 16 · XL 24`. Panels have 12 px padding
(`panel_frame`), bars 8×5 (`bar_frame`), cards 8 (`card_frame`). Grid spacing
is 12×6, `item_spacing` 8×6, widget min size 84×22, `LABEL_COL 168`.
`space::FIELD` (150) is the one width of every control in a value column —
numeric fields and combo boxes share it, so each column has a single clean
right edge. Use `ui.add_space(space::…)` — never a bare float.

## Building a form panel

The vehicle editor's left panel is the reference implementation
(`crates/vehicle-editor/src/ui.rs`, `powertrain.rs`):

- Panel: `egui::Panel::left(..).frame(editor_ui::panel_frame())`, the
  **sections** in one `ScrollArea` (`.auto_shrink([false; 2])`). Never nest
  scroll areas.
- Panel title: `ui.label(editor_ui::heading(t!(…)))`.
- Above the scroll area, and staying put while it scrolls: the heading, the
  name field, and a jump bar — a `horizontal_wrapped` of `small_button`s, one
  per section, from a `SECTIONS` list of (id, title key). A click records the
  id; the matching section calls `scroll_to_me` on the header response that
  `editor_ui::section` hands back. The form runs two to three panel-heights
  long, so without the bar the only way to find out that a section exists is
  to scroll past it, and the name of the thing being edited leaves the screen.
  The section being read wears `BG_ACTIVE` **and** `TEXT_STRONG` — a single
  step of fill is not findable among seven chips. It comes from the previous
  frame (the bar is drawn before the sections that decide it), which is
  invisible. Keep `ACCENT_BG` out of it: the accent marks what the user chose
  (the LOD in the viewport), not where they happen to have scrolled.
- Section: `editor_ui::section(ui, "id", t!("group-…"), |ui| …)` — a
  collapsible header, default open, preceded by a hairline rule and 12 px of
  air so each section reads as its own region instead of one long list. Never
  add a separator by hand next to one. Sub-groups inside a section use
  `editor_ui::subheading` (no upper-casing — titles may carry units).
- Rows: `row(ui, "key", |ui| …)` inside `editor_ui::form_grid("id")`. The
  label comes from i18n key `key`, the tooltip from `key-hint`, and
  `form_label` gives every grid the same 168 px label column so fields align
  across sections.
- Numbers: `editor_ui::field(ui, &mut v, speed, range, "unit")` — a drag
  field at the shared `space::FIELD` width, its value against the **left**
  edge. egui centres a drag value by default, which makes the gap between
  label and number depend on the number's length; left-aligned, the whole
  value column starts at one x — the same one egui already gives combo box
  text. Use `field`, never a bare `DragValue`, or that edge breaks. The unit
  is a symbol (`"kg"`,
  `"km/h"`, `"N·m"`, `"l/min"`, `""` for ratios) — symbols are names, not
  prose, so they stay literal in code. Fields stepped in whole numbers
  (`speed >= 1`) automatically get digit grouping (`1 840 000`). Numbers in
  prose notes use `editor_ui::group_digits` for the same look.
- **Every choice is a labelled row**, including the ones with only two
  options. A bare pair of `selectable_label`s (the diesel governor was one)
  names its alternatives but not what they are alternatives *for*, and it sits
  in no column. Use a combo in a `row`; per-option tooltips still work inside
  `show_ui`. The exception is a card header, where the entry's own identifier
  ("1.", "LOD0") supplies the missing label.
- Sub-groups opened by `optional()` are indented, so their field column sits
  one indent right of the top level. That is deliberate — it is the only
  signal of nesting — but it means "all fields line up" holds *within* a
  level, not across the whole panel.
- A row's field comes **first** in the value cell; auxiliary controls
  (suggest button, mode checkbox like cw·A) sit to its right, so the field
  keeps the column edge. Every choice gets a labelled row — no free-floating
  combo boxes (see the drive type row).
- Checkbox-toggled values ("Magnetic track brake" → force): the checkbox owns
  its line, the value follows as a normal labelled row below it — never put
  long checkbox labels into a grid's label column, they stretch it.
- **A stored table of `(x, y)` points goes through `editor_ui::curve_editor`**
  — the one component for every editable curve in every editor. In the form it
  is only a sparkline well: the curve in `ACCENT` on a `BG_INPUT` well, a dot
  per typed point, no axes and no numbers. That well is not an analysis tool —
  it exists because a point typed one digit wrong reads as an obvious kink in
  the shape and as a plausible number in a column. **Hovering reads the value
  out**, interpolated between the points, plus the hint that a click opens the
  editor; an empty curve draws the well with `curve-empty` in it, so the way
  in never disappears. **A click opens a modal** with the room editing needs:
  a plot with axes, round ticks (k/M notation above ten thousand — the axis
  names the scale, the table has the figures) and points dragged with the
  mouse — double-click adds one, right-click removes one — next to a column
  of drag fields for the exact values. `CurveSpec` carries id, title, units,
  drag speeds and ranges; points sort by x when the interaction ends, never
  during it, and the plot's scale freezes while a point is dragged so the
  picture cannot slide under the cursor. **The y axis starts at zero**, not at
  the smallest value: these are physical magnitudes, and normalised to their
  own range a friction factor falling from 1.0 to 0.6 fills the plot exactly
  like one falling to nothing — which is the one question the picture is
  asked. **A dot marks a point the user typed** — `sparkline` and the wells
  draw them, `sparkline_fn` does not, because there a dot per sample says
  nothing about the vehicle and turns the line into a dotted one. The well is
  the same `BG_INPUT` with the same `BORDER_SUBTLE` edge as a text field.
- **A curve the vehicle computes rather than stores gets `sparkline_fn`** —
  running resistance from three Davis coefficients, friction from the pairing,
  tractive effort from a handful of limits. Sample **`sim-core`'s own
  function** (`VehicleSpec::resistance`, `BrakeKind::friction_factor`,
  `TractionSpec::available_force`), never a copy: a plot that reimplements the
  physics drifts from it. Where a variant stores points instead (effort curve,
  custom friction) `curve_editor`'s well already shows them — draw one or the
  other, not both.
- **Derived readouts close a section**, as a `.small()` `TEXT_SECONDARY`
  label: running resistance at 100 km/h, braked weight percentage. They turn
  coefficients the user cannot judge into a figure they know from the real
  thing, and they cost nothing to keep true. Compute them in `sim-core` next
  to the quantity they belong to, never in the editor — `VehicleSpec::
  brake_percentage` sits beside `Train::brake_percentage` and is tested there.
  Do not invent one whose definition you would have to guess.
- Editable lists (moving parts, converter circuits): one
  `editor_ui::card_frame()` per entry, header row with identifier left and a
  small `"×"` delete button right (`Layout::right_to_left`). A reference that
  no longer resolves — a bound part whose glTF node the current model does not
  have — is drawn in `colors::ERROR` with a tooltip saying so. Such an entry
  is indistinguishable from a working one until the vehicle is driven, and the
  editor is the only place it can still be fixed.
- **Jump bar or filter, by the shape of the data.** A fixed, known, short set
  of destinations (the seven form sections) gets the jump bar: it is complete,
  it sits in the same place every time, and it works before you know what you
  are looking for. An unbounded list whose entries are named by the user (the
  glTF nodes — a real locomotive brings a few hundred, alphabetical, mostly
  scenery) gets a substring filter instead, because there the user already
  knows the name. A filtered list always states `n of m`, so it cannot be
  mistaken for a short file.
- A list of short uniform rows (the LOD list) is a `form_grid` with as many
  `num_columns` as it needs, one `end_row` per entry — never one
  `ui.horizontal` per row. Leading controls rarely measure the same (a
  selected chip is a button, "1" is narrower than "0"), and a horizontal
  gives each row its own x.
- Status bar: message left; on the right the path in `TEXT_SECONDARY` and the
  unsaved marker in `colors::WARN`. The message carries its severity with it
  (`Status::Info` / `Status::Error`) and a failure is drawn in `colors::ERROR`
  — a load that did not happen must not read exactly like one that did. The
  label is `.truncate()`d with the full text on hover, so a long path cannot
  wrap the bar into two lines.

## Hard-won rules

- **Glyphs:** use `"×"` (U+00D7) for delete, not `"✕"` (U+2715) — Inter lacks
  the latter and fallback is unreliable. Between digits and before units use
  a no-break space `\u{A0}` (`editor_ui::NBSP`), not the narrow `\u{202F}`:
  the shaper drops the narrow one after some digits.
- **A counted noun never goes straight after `{ $n }`.** `t!` stringifies
  every argument (`FluentValue::from(value.to_string())`), so a Fluent plural
  selector never matches its `[one]` branch — `{ $count ->` compiles, passes
  the parity test, and still prints "1 Einträge". Write the label form instead
  (`Gleise: { $edges }`, `Strokes in this module: { $count }`), which is right
  at every number in both languages and reads as the counter it is. Where a
  count is genuinely prose, phrase it so the noun does not inflect.
- **i18n:** every visible string through `i18n::t!`, keys in *both*
  `crates/i18n/locales/{en,de}/main.ftl` in the same commit
  (`cargo test -p i18n` enforces parity). Tooltips are `<key>-hint`; a text
  field's placeholder is `<key>-placeholder` and never `-hint` — they are
  different lengths for different purposes, and the parity test cannot tell
  them apart if one ends up in the other's slot.
- **A free-text field states its vocabulary in its tooltip.** The part
  function accepts anything (mods invent their own), but the forms the app
  maps — `door_<name>`, `pantograph`, `switch:<name>`, `gauge:<name>`,
  `lamp:<name>`, `wheel` — are only written down in the model panel's empty
  state, which is gone by the time the field is on screen.
- **Units visible, not hidden:** the unit lives on the field
  (`drag(…, "bar")`); the tooltip explains provenance, not the unit alone.
  An awkward unit is not a reason to leave one off — the Davis c term carries
  `N·s²/m²` because the b term next to it carries `N·s/m`. Only genuinely
  dimensionless values (ratios, factors) go without.
- **The editor remembers what the user would otherwise redo by hand**
  (`settings.rs`): the recent vehicles, the language and view toggles picked
  under View, the window size and the panel widths, in
  `%APPDATA%\Connected Rails\` or
  `$XDG_CONFIG_HOME`. Deriving that path is eight lines of `env::var_os` — it
  does not need a crate. Settings are a convenience: a missing or unreadable
  file falls back to defaults silently, and a failed write never interrupts
  what the user was doing. Layout is tracked in memory every frame and written
  only when the user leaves (close button, Quit) — saving on each frame of a
  resize drag would hammer the disk, and it keeps `--frames` screenshot runs
  from writing their throwaway window size into the real settings. `--window`
  and `TRAINSIM_LANG` override the stored values for that one run.
- **A button that would change nothing is disabled**, with
  `on_disabled_hover_text` saying why ("every suggested node is bound
  already"). Save, "Take over all suggestions" and "Read from node names" all
  work this way. Enabled, they cost a click to discover the answer and leave
  the file marked unsaved for nothing; disabled, the state is readable without
  pressing anything. The enabled tooltip carries the count, so the two
  together answer "would this do something, and what".
- **A "Suggest" button names its figure in the tooltip** (`… — ergäbe
  { $value } N`), computed once and reused for the click. Pressing a button
  to find out what it does is a guess; the user should be able to compare it
  with what is in the field first.
  `-hint` is **optional** — `row()` leaves the tooltip off when the key is
  absent (via `i18n::maybe`). Write no hint rather than one that repeats the
  unit: every empty hover teaches the user that hovering here does not pay,
  and the hints that do carry something stop being found.
- **Labels are self-contained.** Two sections must not both call a field
  "Brake force" — a reader who scrolled past the section title has no way
  back. Say "Dynamic brake force" and "v max Antrieb".
- **Undo is a snapshot of the spec, taken once per interaction.** `draw`
  clones the spec after the menu bar (so opening a file is not an edit), draws
  every panel, and `track_changes` compares. A drag changes the value in every
  frame it lasts, so the step is only recorded when the previous frame did
  *not* change — one step per interaction, not per frame. `undo`/`redo` must
  clear that "was changing" flag, or the next edit is folded into the
  interaction before it and records nothing. Because the snapshot wraps *all*
  panels, model edits are undoable too; a partial undo would be worse than
  none.
- **The window title names the document**, plus the unsaved marker
  (`window-vehicle-editor-named` / `-unsaved`). It is the only part of the
  editor still readable from the task bar; a fixed product name on every
  window tells the user nothing about which one holds their work.
- **Nothing throws work away silently.** New, Open, Quit and the window's
  close button all go through `confirm_discard` first, which asks
  Save / Discard / Cancel via `rfd::MessageDialog` (already a dependency — no
  hand-built modal). **Every native dialog names the editor window as its
  owner** — build it through `message_dialog`/`file_dialog` (vehicle editor),
  which take the parent from `Editor::window` (a `RawHandleWrapper` clone the
  `draw` system refreshes each frame). A parentless dialog is free to open
  *behind* the editor on Windows, where a modal that blocks all input reads
  as a hang. The close button needs `close_when_requested: false` on
  the `WindowPlugin` plus a `WindowCloseRequested` handler, otherwise the one
  route most people take stays unguarded. A "• unsaved" marker reports the
  state; it does not protect it.
- **A failed open or save also raises a dialog** (`report_failure`), not just
  the red status line: the bar is in the corner furthest from the menu the
  action came from, and a RON with a syntax error otherwise fails invisibly —
  the previous vehicle simply stays on screen. Only user-triggered paths
  report this way. `main` opens the file from the command line without it, so
  a headless screenshot run never blocks on a modal; check that after touching
  either path.
- **Saving over a commented file warns first** (`confirm_comment_loss`, once
  per session). `ron::ser` writes the struct, not the file, so every comment a
  hand-kept vehicle carries is gone — that is how `mods/example/vehicles/
  br101_afb.ron` lost its own. The warning only fires when the file on disk
  actually has comment lines, so ordinary saving stays silent. A real fix
  needs a comment-preserving RON round-trip, which the crate cannot do.
- **Save is disabled when it would do nothing** (`needs_saving`: dirty, or no
  file yet). Re-writing an unchanged vehicle is not a no-op — `ron::ser`
  re-serialises the struct, so the comments a hand-written file carries are
  gone. Open, Ctrl+S out of habit, and it is stripped for nothing.
- **No per-widget styling** beyond the helpers. If something needs a new
  look, extend `editor-ui` and document it here.
- **Camera/input gating:** to keep the 3D camera from reacting under the UI,
  read `bevy_egui::input::EguiWantsInput` (`wants_any_pointer_input()`) in the
  camera system. Never query `ctx.egui_wants_pointer_input()` at the start of
  `draw` — the panels of the frame are not laid out yet, so it reports `false`
  over every panel and wheel scroll leaks through to the camera.

## Rendering a model preview to texture

The route editor's content drawer shows each mod model as a picture
(`crates/route-editor/src/thumbnails.rs`). Four things there are not optional,
and each of them cost a debugging round:

- **Read the picture back.** A `RenderTarget::Image` holds its contents only
  while an active camera points at it. Despawn the camera — or switch it off —
  and the texture comes back cleared, so the preview vanishes the moment it is
  finished. Render into a target, then `Readback::texture(target)`, and hand
  egui the ordinary `Image` the readback writes. The target needs `COPY_SRC`
  on top of the usual render-target usages.
- **Keep the scene and camera alive until the readback's observer fires.**
  Despawning them in the same frame the readback starts is a race: the target
  is cleared before the copy happens, and the picture arrives empty *sometimes*.
- **`Visibility` on the entity that carries `WorldAssetRoot`.** Without it the
  glTF's own children inherit no visibility and are never rasterised. The
  camera renders, the texture arrives, and the model is simply not in it.
- **Wait for the load state *and* the bounds, then render for a while.** The
  glTF being read does not mean its entities exist; the entities existing does
  not mean the meshes and textures are on the GPU. Four frames after both were
  still too early for a signal mast — 30 is the working figure.

Render layers keep the two worlds apart (the map is layer 0). A layer does not
reach the children a glTF spawns, so the model needs
`Propagate(RenderLayers::layer(n))` plus
`HierarchyPropagatePlugin::<RenderLayers>` on the app.

## Verifying changes (screenshot loop)

Both editors render headless screenshots — iterate visually, don't guess:

```
cargo run -p vehicle-editor --features dev -- \
    mods/example/vehicles/br101_afb.ron \
    --window 1380x2400 --frames 90 --screenshot target/ui-iter/step.png
```

`--window WxH` sizes the window (tall windows show the whole form),
`--frames 90` exits after 90 frames. The route editor takes the same flags.
**Every field of `VehicleSpec` that the simulation reads belongs in the
form.** All of them do now; walk the struct against the panel whenever you add
one to `sim-core`. `adhesive_mass_fraction` was missing and defaults to 0, so
a locomotive built start to finish in the editor could not transmit a newton
and nothing on screen said why.

Where a type has named presets in `sim-core` (`CouplerSpec::screw`,
`center_buffer`), offer them as a combo above the fields they fill, and let
the combo read "own values" when the numbers match no preset. The presets are
a starting point — a modder should not have to guess 3 MN/m — and the fields
stay editable underneath.

**Every option a combo offers must have its data on screen.** Picking
"own characteristic" for the friction pairing set a curve that nothing then
drew — the points were in the file, unreachable and invisible. When a variant
carries data, render it; `editor_ui::curve_editor` does the `(x, y)` lists.

Whole branches of the form only appear for one setting — the four drive types
build four different panels, and `--screenshot` shows whichever the example
vehicle happens to carry. Copy `br101_afb.ron`, swap its `traction:` block for
the variant you want (the constructors in `drive_combo` give valid starting
values) and render that too; delete the copies afterwards. A malformed copy is
not wasted either — the status bar names the RON line and column.

Look at the PNG at high zoom and check:

1. Field columns align across *all* sections (168 px label column).
2. Every physical quantity shows its unit; big integers are digit-grouped.
3. Hierarchy reads: heading > section title > subheading > label.
4. Spacing is from the 4 px scale — no accidental 3 px or 17 px gaps.
5. No tofu/missing glyphs (watch delete buttons and unit suffixes).
