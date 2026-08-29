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
repository only keeps what it exported. The generated meshes and textures are
content of this repository and, being derived from CC0 material, carry no
further attribution requirement — the credit above is given because it is
deserved.

The **animation clips** in the same files are another matter: they are motion
capture of real people from two collections licensed under Creative Commons
Attribution. The recordings themselves are not in the repository —
`tools/characters/fetch_mocap.py` downloads them into a cache, `clips.json`
says which file each clip comes from — but the clips in `mods/people/assets/*.glb`
are **adaptations** of them: `tools/characters/mocap.py` retargets the
recorded motion onto the MakeHuman rig of each character, cuts one gait cycle
or a ten-to-twenty-second loop out of a recording, resamples it and closes the
loop, and `build_character.py` lays the upper body of an idle over a seated
pose. The originals are:

- **The 100STYLE Dataset — Ian Mason.** © Ian Mason; Ian Mason, Sebastian
  Starke, Taku Komura: "Real-Time Style Modelling of Human Locomotion via
  Feature-Wise Transformations and Local Motion Phases", ACM Transactions on
  Graphics, 2022. <https://www.ianxmason.com/100style/>, archived at
  <https://zenodo.org/record/8127870>. Licensed under the Creative Commons
  Attribution 4.0 International licence
  (<https://creativecommons.org/licenses/by/4.0/>). The walks and idles in the
  `Neutral`, `Rushed`, `OnPhoneLeft`, `OnPhoneRight`, `HandsInPockets`,
  `ArmsFolded`, `ArmsBehindBack`, `Depressed`, `Proud`, `Elated`, `Old`,
  `LookUp`, `Akimbo` and `Followed` styles come from it.
- **Open Motion Project by ACCAD / The Ohio State University.** © Advanced
  Computing Center for the Arts and Design (ACCAD), The Ohio State University.
  <https://accad.osu.edu/research/motion-lab/mocap-system-and-data>. Licensed
  under the Creative Commons Attribution 3.0 Unported licence
  (<https://creativecommons.org/licenses/by/3.0/>). The `Female1`, `Male1` and
  `Male2` walks, stands, sways and look-arounds come from it.

Both licences allow the adapted clips to be used and redistributed for any
purpose, including commercially, on the conditions above: the credit, the
licence reference and the note that the material was changed have to travel
with every copy of the character files — a binary release, a mod that copies a
file out of `mods/people/`, a fork. This file is that notice; ship it
alongside. The clips are provided as they are, without warranties of any kind,
as the licences say; neither the datasets nor their authors endorse this
simulator or anything made with it. The additional permission in `LICENSE`
lets mod content be licensed freely, but it is the Licensor's permission for
the Licensor's own work — it cannot relicense these clips, which stay CC BY
wherever they go.

The [CMU Graphics Lab motion capture database](http://mocap.cs.cmu.edu/) was
considered and not used. Its terms are a bespoke notice, not a licence: free
for research, allowed inside commercially-sold products, but "you may not
resell this data directly, even in converted form". Character files that
`LICENSE` explicitly lets mods sell would put a converted copy of the data in
exactly that position, so the doubt was left out of the repository.

## Trees in `mods/trees/`

The vegetation is generated out of
[EZ-Tree](https://github.com/dgreenheck/ez-tree) by `tools/trees/` (see its
README). EZ-Tree is © 2024 Daniel Greenheck and licensed under the MIT
licence:

> Permission is hereby granted, free of charge, to any person obtaining a copy
> of this software and associated documentation files (the "Software"), to deal
> in the Software without restriction, including without limitation the rights
> to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
> copies of the Software […] THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY
> OF ANY KIND.

Nothing of its code is included and nothing of it is redistributed: the
pipeline clones the pinned commit into `~/.cache/connected-rails/ez-tree`,
runs it, and keeps only what it grew. The **branch and leaf placement** of the
models therefore comes out of EZ-Tree's generator, and
`tools/trees/lib/mesh.mjs` meshes that skeleton with the same ring-and-quad
construction the library uses — the credit in the `copyright` field of every
generated glTF says so.

The **leaves and needles are photographs**, from two public-domain libraries:

- **[ambientCG](https://ambientcg.com)** — the `LeafSet###` atlases, sheets of
  single leaves scanned on black with an opacity map. Twenty-six of the thirty
  are used, one or two per species: an oak takes `LeafSet016` in summer and
  `LeafSet012` in autumn, a beech `LeafSet024` and `LeafSet015`, an ash the
  whole compound sprays of `LeafSet002`.
- **[Poly Haven](https://polyhaven.com)** — the twig atlases of `fir_tree_01`
  and `pine_tree_01`, which is what the spruce, the silver fir, the Douglas fir
  and the pine carry. Their tree *models* are not used, only those two texture
  sets, and only the colour and opacity maps of them.

Both libraries release everything under **CC0 1.0 Universal**
(<https://creativecommons.org/publicdomain/zero/1.0/>) — public domain, no
attribution required. The credit here is given because it is deserved.

Nothing of either is redistributed: `tools/trees/fetch_foliage.mjs` downloads
them into `~/.cache/connected-rails/foliage/`, `lib/leaves.mjs` cuts the single
leaves out of the sheets, and what is checked in is the atlas the build composes
from them — cropped, rotated, tinted and arranged into a foliage card. CC0
allows the derivation without condition.

The **bark is still painted**, from the species entry by
`tools/trees/lib/bark.mjs` — a birch has to be white with black lenticels and a
Scots pine orange-red in plates, and no generic scan gives that. The impostors
are rasterised from the models themselves by `lib/impostor.mjs`. Both are
content of this repository under its own licence.

## Cab sounds in `mods/example/assets/sounds/`

The eight samples the example BR 101's sound table plays — the Sifa/PZB
warning buzzer and the operating clicks of the Sifa pedal, the three PZB
buttons, the screen softkeys and the desk switches — are recordings from
[freesound.org](https://freesound.org), each released under Creative Commons
Zero 1.0 Universal (<https://creativecommons.org/publicdomain/zero/1.0/>).
CC0 puts the recordings into the public domain as far as the law allows: they
may be used, changed and redistributed for any purpose, including
commercially, with no conditions. The credit below is given because it is
deserved, not because it is owed.

The files in the repository are processed copies: each recording was trimmed
to the one event a sound table entry needs, normalised to −1.5 dBFS, converted
to mono Ogg Vorbis and — for the buzzer, which plays as a loop — cut to a
whole number of its ~1.76 kHz periods and crossfaded at the wrap so it repeats
seamlessly. The site's preview files (128 kbit/s MP3) were used, as the
originals sit behind a login; the processing does not change the licence.

| File | Recording | Author |
| ---- | --------- | ------ |
| `buzzer.ogg` | [BiLevel Cab Car cabin buzzer](https://freesound.org/people/chungus43A/sounds/745668/) | chungus43A |
| `button-stiff.ogg` | [Button_05.wav](https://freesound.org/people/deleted_user_2104797/sounds/346709/) | deleted_user_2104797 |
| `button-soft.ogg` | [rubber button on electric device](https://freesound.org/people/ShJafari/sounds/747199/) | ShJafari |
| `switch-toggle.ogg` | [Toggle switch On Off](https://freesound.org/people/cookies+policy/sounds/556636/) | cookies+policy |
| `switch-breaker.ogg` | [Kill Switch (Large Breaker Switch) .WAV](https://freesound.org/people/EchoCinematics/sounds/131599/) | EchoCinematics |
| `switch-main.ogg` | [Industrial Switch With Spring](https://freesound.org/people/cookies+policy/sounds/556635/) | cookies+policy |
| `switch-detent.ogg` | [fan switch.ogg](https://freesound.org/people/BoboTheEpic/sounds/411422/) | BoboTheEpic |
| `reverser.ogg` | [rotary switch on and off.wav](https://freesound.org/people/ProdMultimediasHQI/sounds/512502/) | ProdMultimediasHQI |

Every sound table entry that plays one of these files carries a
`see THIRD_PARTY_LICENSES.md` remark in `mods/example/vehicles/br101_afb.ron`,
so the notice travels with the file a mod would copy out.

## Rust dependencies

Crates pulled in by Cargo are not vendored here; their licences are those
declared in [Cargo.lock](Cargo.lock) and can be listed with
`cargo license` or `cargo about`.
