# Third-party licences

The simulator itself is licensed under the EUPL v. 1.2, see [LICENSE](LICENSE).
Material from other projects that is checked into this repository keeps its own
licence; this file lists it.

## Agent skills in `.claude/skills/`

The `bevy*` skills and `similarity-rs` are taken from
[chrisgliddon/bevy-skills](https://github.com/chrisgliddon/bevy-skills) and are
used under the MIT licence. They are documentation for AI coding agents and are
not compiled into any binary.

```
MIT License

Copyright (c) 2026 Chris Gliddon

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

The `editor-ui` and `screenshot` skills are project-specific and covered by the
EUPL like the rest of the repository.

## Star catalogue in `crates/world-render/src/stars.bin`

The night sky is the real one. `stars.bin` holds the naked-eye stars — right
ascension, declination, apparent magnitude and colour index — filtered out of
the [HYG database](https://github.com/astronexus/HYG-Database) by
`tools/gen_stars.py`. The database is compiled by David Nash (Astronomy Nexus)
from the Hipparcos, Yale Bright Star and Gliese catalogues and is used here
under the Creative Commons Attribution-ShareAlike 4.0 International licence
(<https://creativecommons.org/licenses/by-sa/4.0/>).

Attribution: *The HYG database, © David Nash / Astronomy Nexus, CC BY-SA 4.0.*
The file is data, not code: it is read by `world_render::sky` and shipped inside
the binary, and any redistribution of it stays under CC BY-SA 4.0.

## Phosphor icons in the editor bars

The sun on the status bar's day rail, the calendar leaf on its date button and
the two carets in the calendar are glyphs of the
[Phosphor](https://github.com/phosphor-icons/homepage) icon font by Helena
Zhang and Tobias Fried, bundled through the `egui-phosphor` crate and used
under the MIT licence. Everything else in the editors is drawn from line
segments (`crates/editor-ui/src/icon.rs`); the font is for the symbols a
22-pixel drawing cannot carry.

```
MIT License

Copyright (c) 2023 Phosphor Icons

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

The `egui-phosphor` crate that bundles the font (© 2023 Romet Tagobert) is MIT
OR Apache-2.0; the font file it ships is compiled into the editor binaries.

## People in `mods/people/`

The characters — the walker's body and the passengers — are generated out of
[MakeHuman 2](https://github.com/makehumancommunity/makehuman2) by
`tools/characters/` (see its README). The glTF files check in the geometry of
the MakeHuman base mesh with its morph targets applied, the `game_engine` rig,
and the meshes and textures of the MakeHuman system asset packs: skins, eyes,
eyebrows, eyelashes, hair, the casual/elegant/work/sport suits, the shoes, the
hats and the poses. All of that was released by the MakeHuman project under
CC0 1.0 Universal (<https://creativecommons.org/publicdomain/zero/1.0/>) —
the asset files say so in their headers: *"This asset was explicitly released
as CC0 in september 2020. The copyright holders at the point of the release to
CC0 were: Data Collection AB, Joel Palmius, Jonas Hauquier."* The MakeHuman
base mesh and targets are CC0 as well (MakeHuman's `makehuman_license.txt`).

Nothing of MakeHuman's program code is included; the pipeline runs it, the
repository only keeps what it exported. The generated files are content of
this repository and, being derived from CC0 material, carry no further
attribution requirement — the credit above is given because it is deserved.

## Rust dependencies

Crates pulled in by Cargo are not vendored here; their licences are those
declared in [Cargo.lock](Cargo.lock) and can be listed with
`cargo license` or `cargo about`.
