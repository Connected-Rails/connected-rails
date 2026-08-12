# Text & fonts — 0.18 → 0.19

0.19 replaced the cosmic-text layout backend with **Parley** (font selection via
`fontique`). This is the most concentrated source of compile breakage in the release.

## `TextLayout` constructors lost the `new_with_` prefix

| 0.18 | 0.19 |
|---|---|
| `TextLayout::new_with_justify(j)` | `TextLayout::justify(j)` |
| `TextLayout::new_with_linebreak(lb)` | `TextLayout::linebreak(lb)` |
| `TextLayout::new_with_no_wrap()` | `TextLayout::no_wrap()` |

(PR #24049.)

## `TextFont::font` is now `FontSource`

`font` changed from `Handle<Font>` to `FontSource` (PRs #22156, #22614).
`FontSource: From<Handle<Font>>`, so the minimal fix is appending `.into()`:

```rust
TextFont {
    font: asset_server.load("fonts/FiraSans-Bold.ttf").into(),
    ..default()
}
```

`FontSource` can also name a font instead of holding a handle:

- **By family name:** `FontSource::family("Fira Sans")`
- **By semantic category:** `Serif`, `SansSerif`, `Cursive`, `Fantasy`, `Monospace`,
  and UI categories like `SystemUi`, `Emoji`, `Math`.

Family/semantic lookup only finds *installed system fonts* when the
`bevy/system_font_discovery` feature is enabled — without it, `FontSource::family("…")`
resolves only fonts you explicitly loaded as Bevy assets. On Linux that feature needs
fontconfig headers: `sudo apt install libfontconfig1-dev`.

## `TextFont::font_size` is now `FontSize`

`font_size` changed from `f32` to the `FontSize` enum, mirroring CSS units:

```rust
TextFont { font_size: FontSize::Px(24.0), ..default() }   // fixed logical pixels
// also: FontSize::Vw, ::Vh, ::VMin, ::VMax, ::Rem (relative to the RemSize resource)
```

"Logical pixels" is still the meaning of `FontSize::Px` — it is no longer a bare `f32`.

## The `DefaultFontHandle` fallback trick is obsolete

The 0.18 pattern of inserting a font at `TextFont::default().font` so every
`..default()` entity inherits it **no longer behaves the same** — `TextFont::default().font`
is a `FontSource`, not a `Handle<Font>`, and Parley/fontique now handle fallback
automatically. Remove the bespoke fallback resource and rely on system discovery
(plus the feature flag above) or per-`TextFont` `FontSource::family(...)`.

The explicit fallback setters (`set_serif_family`, `set_sans_serif_family`,
`set_monospace_family`) now return `Result`, and in most apps you should not need to
call them at all.

## Related new surface (additive, not breakage)

- **EditableText / text input** widgets are new in 0.19 — worth covering in UI docs.
