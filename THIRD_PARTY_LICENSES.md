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

## Federal state boundaries in `crates/fields/src/laender.bin`

Which German state a place lies in decides which agricultural service the field
import asks, which schema comes back and which crop code list applies. The
boundaries are the **VG2500** of the Bundesamt für Kartographie und Geodäsie —
the 1:2 500 000 administrative areas — fetched from the BKG's open WFS and
thinned to about half a kilometre by `tools/gen_laender.py`.

The data is published under the *Datenlizenz Deutschland – Namensnennung – 2.0*
(<https://www.govdata.de/dl-de/by-2-0>).

Attribution: *© GeoBasis-DE / BKG (2023), dl-de/by-2-0.* The file is data, not
code: it is read by `fields::land` and shipped inside the binary.

## Crop code tables in `crates/fields/src/crops/`

`nw.csv` maps North Rhine-Westphalia's InVeKoS crop codes onto the render groups
the simulator draws; `groups.csv` and `arable.csv` hold the weights the import
draws from where a state publishes no crop. All three were read off the
**Teilschläge** layer of the Landwirtschaftskammer Nordrhein-Westfalen's WFS by
`tools/gen_crop_codes.py` — the code, its text, its InVeKoS group and the share
of the sampled area it covers.

The data is published under the *Datenlizenz Deutschland – Namensnennung – 2.0*.

Attribution: *© Landwirtschaftskammer Nordrhein-Westfalen, dl-de/by-2-0.*

The rest of the field data is **not** shipped: the import fetches it from each
state's own service at build time and records what it used in the module's
`field_sources`, so the source note follows the line rather than the program.
The licences differ per state — see *Fields* in MODS.md, and note that three
states have not stated one. Rhineland-Palatinate has no service at all, and a
module outside Germany has no German register under it; both fall back to
OpenStreetMap, which is **ODbL 1.0** — share-alike, so a module built on it
carries the obligation on. That is why it is the fallback and not the first
choice, and why the import names it in the module's attribution and warns in
its summary.

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

## Track textures in `mods/example/assets/track/`

The example mod's track textures — the photographed Schotter the ballast bed
is skinned with, the worn gravel of the jointed branch lines, the weathered
concrete of the sleepers and slab track, and the creosoted planks the wooden
sleepers show — are surface scans by two libraries that publish everything
under the **Creative Commons CC0 1.0 Universal** licence
(<https://creativecommons.org/publicdomain/zero/1.0/>), so they may be
copied, modified and redistributed with the repository without attribution.
CC0 needs none, but the sources are recorded here:

- `mods/example/assets/track/ballast.jpg` and `ballast_nor.jpg` —
  [Gravel043](https://ambientcg.com/a/Gravel043) from
  [ambientCG](https://ambientcg.com) (photogrammetry, CC0 1.0).
- `ballast-worn.jpg` — [Gravel024](https://ambientcg.com/a/Gravel024)
  (ambientCG, CC0).
- `sleeper-concrete.jpg` / `sleeper-concrete_nor.jpg` —
  [concrete_floor_worn_001](https://polyhaven.com/a/concrete_floor_worn_001)
  (Poly Haven, CC0).
- `sleeper-wood.jpg` / `sleeper-wood_nor.jpg` —
  [dark_wooden_planks](https://polyhaven.com/a/dark_wooden_planks)
  (Poly Haven, CC0).

## Road textures in `crates/world-render/src/roads/`

The roads the simulator and the editors drape over the terrain are skinned
with the same kind of scans as the track, from the same CC0 source. They are
**program assets** — a module carries no road bitmaps, the look is the
program's, so every module and every client of a multiplayer run draws the
same carriageway. Recompressed from the ambientCG 1K JPEGs (resize, quality
80–82); CC0 needs no attribution, but the sources are recorded here:

- `roads/asphalt.jpg` and `roads/asphalt_nor.jpg` —
  [Asphalt010](https://ambientcg.com/a/Asphalt010) from
  [ambientCG](https://ambientcg.com) (photogrammetry, CC0 1.0).
- `roads/concrete.jpg` and `roads/concrete_nor.jpg` —
  [Concrete030](https://ambientcg.com/a/Concrete030) (ambientCG, CC0).

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

## Sounds in `mods/example/assets/sounds/`

Everything the example BR 101's sound table plays that is not a `synth:`
source is a recording somebody else made and published under a licence that
allows this use. Three groups: the cab sounds are CC0 recordings from
freesound.org; the driving noise — what the loco itself sounds like — is cut
out of Creative Commons trainspotting videos and two more Freesound
recordings; the electric brake and the Makrofon come from two CC BY-SA
recordings on Wikimedia Commons, and those files stay CC BY-SA.
[`tools/sounds/br101_sounds.py`](tools/sounds/br101_sounds.py) holds every
cut (source, position, length, filters) and rebuilds the files from the
sources.

### Cab sounds (CC0)

The nine samples of the desk — the operating clicks of the Sifa pedal, the
three PZB buttons, the screen softkeys and the desk switches, and the two
buzzers of the Sifa and the train protection — are recordings from
[freesound.org](https://freesound.org), each released under Creative Commons
Zero 1.0 Universal (<https://creativecommons.org/publicdomain/zero/1.0/>).
CC0 puts the recordings into the public domain as far as the law allows: they
may be used, changed and redistributed for any purpose, including
commercially, with no conditions. The credit below is given because it is
deserved, not because it is owed.

The files in the repository are processed copies: each click was trimmed to
the one event a sound table entry needs, normalised to −1.5 dBFS and
converted to mono Ogg Vorbis; the two buzzers, which play as loops, are cut
and folded over their own tails like the driving noise below. The site's
preview files (128 kbit/s MP3) were used, as the originals sit behind a
login; the processing does not change the licence.

| File | Recording | Author |
| ---- | --------- | ------ |
| `sifa-buzzer.ogg` | [Buzzer_Alarm.wav](https://freesound.org/s/524909/) — a steady alarm buzzer, standing in for the Sifa's, of which no free recording exists | Engineer_815 |
| `pzb-buzzer.ogg` | [buzzer.wav](https://freesound.org/s/332563/) — an electromechanical door buzzer, standing in for the PZB/LZB buzzer, likewise | 011-_11919_1-1011111 |
| `button-stiff.ogg` | [Button_05.wav](https://freesound.org/people/deleted_user_2104797/sounds/346709/) | deleted_user_2104797 |
| `button-soft.ogg` | [rubber button on electric device](https://freesound.org/people/ShJafari/sounds/747199/) | ShJafari |
| `switch-toggle.ogg` | [Toggle switch On Off](https://freesound.org/people/cookies+policy/sounds/556636/) | cookies+policy |
| `switch-breaker.ogg` | [Kill Switch (Large Breaker Switch) .WAV](https://freesound.org/people/EchoCinematics/sounds/131599/) | EchoCinematics |
| `switch-main.ogg` | [Industrial Switch With Spring](https://freesound.org/people/cookies+policy/sounds/556635/) | cookies+policy |
| `switch-detent.ogg` | [fan switch.ogg](https://freesound.org/people/BoboTheEpic/sounds/411422/) | BoboTheEpic |
| `reverser.ogg` | [rotary switch on and off.wav](https://freesound.org/people/ProdMultimediasHQI/sounds/512502/) | ProdMultimediasHQI |

### Driving noise of the loco (CC BY 3.0 and CC0)

The loops that make the 101 sound like a 101 are cut out of videos that
railway enthusiasts filmed on the platform and published on YouTube under the
Creative Commons Attribution licence — YouTube's "reuse allowed" option, which
is CC BY 3.0 (<https://creativecommons.org/licenses/by/3.0/>) — and out of two
CC0 recordings from freesound.org. CC BY asks that the author is named, the
source and the licence are given and changes are stated: this section is that
notice, and what follows is what was changed. Each loop is a window of a few
seconds out of the video's audio track, decoded to mono at 48 kHz, filtered
(a high-pass against wind and handling noise), for the whines reduced to the
lines of their spectrogram — the converter's tones are kept, the station
between them is dropped — folded over its own tail so it repeats without a
seam, normalised to the loudness of the generated loops and encoded as Ogg
Vorbis. `aux-idle.ogg` is resampled by half a percent so that its 500 Hz comb
sits on the one in the traction loops; `traction-mid.ogg` and
`traction-high.ogg` are the `traction-low.ogg` cut resampled 1.32× and 1.74×,
because no free recording of the loco under power at speed exists. The
videos' pictures are not used.

| File | Window | Video | Author |
| ---- | ------ | ----- | ------ |
| `aux-idle.ogg` | 8.5–16.5 s | [DB 101 070-1 mit IC 2442 nach Hannover Hbf](https://www.youtube.com/watch?v=W6yEO0nOb2g) | TheZugBox |
| `traction-low.ogg`, `traction-mid.ogg`, `traction-high.ogg` | 12.0–15.5 s | [BR 101 Abfahrt Bochum Hbf](https://www.youtube.com/watch?v=Eb51FqjK31o) | Zugfan 110 |
| `rolling-low.ogg` | 36–48 s | [BR101 Abfahrt Düsseldorf Hbf](https://www.youtube.com/watch?v=qhlQ0r_I3z0) | Zugfan 110 |
| `rolling-mid.ogg` | 17.5–23.5 s | [101 094-1 mit IC 2261 durch A-Oberhausen](https://www.youtube.com/watch?v=7Q374vGrWuo) | AugsburgerTrainspotter |
| `rolling-high.ogg` | 68–73 s | [Züge am Pfingstbergtunnel (SFS Mannheim Stuttgart) mit ICE 1, 3, 4, Velaro D und ICs BR101](https://www.youtube.com/watch?v=bRNnhwc2FZM) | Paul |

The two CC0 recordings, processed the same way and again taken from the
site's preview files:

| File | Recording | Author |
| ---- | --------- | ------ |
| `air.ogg` | [Train Air Brake 01.wav](https://freesound.org/s/388568/) | totalcult |
| `compressor.ogg` | [Tilt Train Compressor](https://freesound.org/s/349145/) | Yoyodaman234 |

### Electric brake and Makrofon (CC BY-SA)

Two recordings on Wikimedia Commons are published under Creative Commons
Attribution-ShareAlike. ShareAlike binds the adaptation: the four files cut
from them are themselves CC BY-SA and may only be passed on under that
licence — which is a condition on these files, not on the rest of the mod or
on the simulator. Anyone copying `ebrake-*.ogg` or `horn.ogg` into a mod of
their own takes the licence with them.

| File | Window | Recording | Author | Licence |
| ---- | ------ | --------- | ------ | ------- |
| `ebrake-low.ogg`, `ebrake-mid.ogg`, `ebrake-high.ogg` | 837.5–843.5 s | [Cab Ride Hamburg Hbf to Hamburg Altona BR 101 NJ 470](https://commons.wikimedia.org/wiki/File:Cab_Ride_Hamburg_Hbf_to_Hamburg_Altona_BR_101_NJ_470.webm) | IC-Lokführer | [CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/) |
| `horn.ogg` | 0.55–2.05 s | [Makrofon DB Baureihe 628](https://commons.wikimedia.org/wiki/File:Makrofon_DB_Baureihe_628.ogg) | MdE | [CC BY-SA 3.0](https://creativecommons.org/licenses/by-sa/3.0/) |

The cab ride is the author's own recording, released with a permission on
file at Wikimedia (VRT ticket 2025071110002506) and filmed with DB's
permission; the site's 480p transcode was used, whose audio is the
original's. Processing as above (mono, filters, reduction to the
spectrogram's lines, seam, normalisation, Vorbis), with a treble shelf on the
brake loops since the cab wall had taken it off and the simulator's own cab
wall takes it again; `ebrake-mid.ogg` and `ebrake-high.ogg` are the low band
resampled 1.32× and 1.74×. What the pieces are: the electric brake is the
loco's converters as it brakes into Hamburg-Altona; the Makrofon is the DB
signal horn as blown by a class 628, the same instrument at 621 Hz where the
101 in Bochum was measured at 608 Hz, and resampled those two per cent down.

Every sound table entry that plays one of these files carries a
`see THIRD_PARTY_LICENSES.md` remark in `mods/example/vehicles/br101_afb.ron`,
so the notice travels with the file a mod would copy out. What the table
still generates — brake squeal and rail joints — it generates because no
free recording of the 101 doing either turned up.

## Plant models in `crates/world-render/src/plants/`

The standing crop's near level — the wheat, corn, lettuce, grass, clover,
turnip, flower, hay, vine and tree models the fields grow — comes from the
**Ultimate Crops** and **Nature** packs by
[Quaternius](https://quaternius.com), fetched through
[Poly Pizza](https://poly.pizza) and published under the **Creative Commons
CC0 1.0 Universal** licence
(<https://creativecommons.org/publicdomain/zero/1.0/>): public domain, they
may be copied, modified and redistributed with the repository for any
purpose, including commercially, with no attribution required. The credit
here is given because it is deserved.

The files in the directory are **adaptations**: `tools/plants/normalise.mjs`
bakes the node transforms in, splits a pack's scene into one variant per
plant, stands each on its own foot, scales the set so the tallest is exactly
one metre, cuts the leaf sheets out rather than blending them, drops the
images no material samples, re-indexes the vertices and writes one compact GLB
per crop (see `tools/plants/README.md` and the `fetch.mjs` that re-downloads
the sources). At runtime the renderer scales each model by the phenology's
stand height and repaints its untextured parts in the day's stand colour, so
one wheat model serves every stage from sprout to ripe.

## Rust dependencies

Crates pulled in by Cargo are not vendored here; their licences are those
declared in [Cargo.lock](Cargo.lock) and can be listed with
`cargo license` or `cargo about`.
