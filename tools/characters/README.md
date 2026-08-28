# Characters: the MakeHuman 2 pipeline

The people in `mods/people/` — the walker's body and the passengers — are generated,
not modelled by hand. Three scripts turn a roster into game-ready glTF files:

```
roster.json ──mh2_export.py──▶ raw .glb + .meta.json ──build_character.py──▶ mods/people/assets/<id>.glb
                                                                              mods/people/characters/<id>.ron
                              (build_all.py runs both for every character)
```

1. **`roster.json`** is the source of truth: one entry per character with the
   MakeHuman modifiers (gender, age, ethnicity, height, weight, a handful of face
   shapes), the skin, eye colour, eyebrows, eyelashes, hair, clothes, shoes, an optional
   hat, and optional HSV tints per garment so one suit mesh serves in several colours.
   Twenty-four people, twelve of each gender, across three age bands. `height_m` is the
   height the person is meant to have; the Height modifier was calibrated against it
   (MakeHuman's height macro depends on gender, age and proportions, so the modifier
   alone says little — measure the export, not the slider). Mind the age macro when
   adding people: 0.5 is 25 years, 0.25 is a child of 14. The eyebrow textures are not
   labelled by gender: `eyebrow002`, `003`, `006` and `007` are thin and arched (women),
   `005` and `008`–`012` are full (men), `001` and `004` go either way; the eyelash assets
   read as make-up, so men carry none. A hat wearer goes without hair — the hair meshes
   are not fitted to the hats and poke through the crown.
2. **`mh2_export.py`** drives MakeHuman 2 headlessly: it imports the program's own
   modules, runs Qt on the `offscreen` platform, stubs the OpenGL window away, loads each
   character as a MakeHuman model file, attaches the `game_engine` rig (53 joints) and
   exports a binary glTF with the rest pose plus the clips `idle`, `idle2`, `walk`
   (`walk_normal` for men, `walk_female` for women), `stand`, `stand2`, `stand3` and
   `sit`. The raw export is 15–20 MB per person (full PNG textures, one mesh per
   garment) and never ships.
3. **`build_character.py`** makes the game file out of that: one texture atlas each for
   the opaque parts (skin, clothes, shoes — JPEG) and the cut-out parts (hair, eyebrows,
   eyelashes, eyes — PNG with alpha), four levels of detail out of `meshoptimizer`
   (`char_LOD0` … `char_LOD3`, about 30 000 / 6 000 / 1 600 / 500 triangles; the garments
   are Loop-subdivided twice before the finest level, because MakeHuman's suits are a
   handful of flat facets over the bust) skinned to
   the same skeleton, the clips re-grounded (the MakeHuman exporter leaves them floating
   when it scales to metres), the `sit` clip replaced by a chair pose built out of the
   rest pose (MakeHuman's `sit01` sits on the floor with the legs stretched out — no
   use in a coach), the `walk` clip replaced by a procedural in-place cycle (one second,
   two steps, about 1.5 m/s: hip swing ±28°, knees bent in the swing, level feet, the
   thighs turned in from MakeHuman's wide A-pose stance so the feet walk 0.15 m apart,
   arms hanging by the body with a small swing — MakeHuman's own walk comes out
   narrow-kneed and hunched on the game rig), the arms of the standing clips replaced
   by the same relaxed hang (MakeHuman holds them out and bent), and the whole thing
   turned to face −Z with its feet on
   y = 0 — the frame every model in the game uses. `--check` re-reads the result and
   verifies all of that, the seat height and knee position of the chair pose included.

## Running it

MakeHuman 2 lives in `~/.local/share/makehuman2` with its own virtual environment;
the asset packs (skins, clothes, hair, rigs, poses — all CC0 from the MakeHuman
project) in `~/Documents/makehuman2/data`. The scripts need that interpreter:

```
PY=~/.local/share/makehuman2/venv/bin/python
$PY -m pip install meshoptimizer pillow      # once
$PY tools/characters/build_all.py            # everything: ~2 min for 24 people
$PY tools/characters/build_all.py --only f01_lena,m07_michael
```

`mh2_export.py` builds its own MakeHuman home under `~/.cache/connected-rails/mh2home`
with symbolic links to the asset packs in the `<category>/hm08/<asset>` layout MakeHuman
2 expects (the packs are laid out the MakeHuman 1 way) and points MakeHuman at it through
`MH_HOME_LOCATION`, so the user's own MakeHuman installation is never touched. The
exported `.mhm` files land next to the raw exports; MakeHuman 2's GUI opens them, which
is the way to tune a face by hand — copy the modifier lines back into the roster.

## Git LFS

`mods/**/*.glb` is tracked by Git LFS (`.gitattributes`), so a rebuilt roster is committed
like any other file — git stores the pointer, LFS the 1.7 MB objects. `git lfs install`
once per machine is all it takes; without it a checkout holds pointer files and the mod
loader warns about each of them.

## What the game expects

The conventions are documented with the content type in
`crates/content/src/characters.rs`: origin on the ground between the feet, face towards
−Z, metres, the LOD nodes and the clip names. `mods/people/characters/<id>.ron` names the
file, gender, roles (`Player`, `Passenger`) and tags; the app picks from that registry.
