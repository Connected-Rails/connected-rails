# Characters: the MakeHuman 2 pipeline

The people in `mods/people/` — the walker's body and the passengers — are generated,
not modelled by hand, and move like the people they were recorded from. Four scripts
turn a roster and a shelf of motion capture recordings into game-ready glTF files:

```
roster.json ──mh2_export.py──▶ raw .glb + .meta.json ──build_character.py──▶ mods/people/assets/<id>.glb
clips.json  ──fetch_mocap.py─▶ ~/.cache/…/mocap/*.bvh ──────┘  (mocap.py)      mods/people/characters/<id>.ron
                              (build_all.py runs all of it for every character)
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
   exports a binary glTF with the rest pose plus MakeHuman's own clips (`idle`, `idle2`,
   `walk`, `stand`, `stand2`, `stand3`, `sit`) — the fallback for a build without
   recordings. The raw export is 15–20 MB per person (full PNG textures, one mesh per
   garment) and never ships.
3. **`clips.json` and `fetch_mocap.py`** are the recordings: which files of which
   dataset each clip comes out of, under which licence, and where to download them
   (see *The recordings* below). `fetch_mocap.py` puts them into
   `~/.cache/connected-rails/mocap/<source>/`; nothing of the raw data is checked in.
4. **`build_character.py`** makes the game file out of that: one texture atlas each for
   the opaque parts (skin, clothes, shoes — JPEG) and the cut-out parts (hair, eyebrows,
   eyelashes, eyes — PNG with alpha), four levels of detail out of `meshoptimizer`
   (`char_LOD0` … `char_LOD3`, about 30 000 / 6 000 / 1 600 / 500 triangles; the garments
   are Loop-subdivided twice before the finest level, because MakeHuman's suits are a
   handful of flat facets over the bust) skinned to the same skeleton, **the clips
   replaced by the recordings** retargeted onto this character's skeleton (`mocap.py`),
   the clips re-grounded (the lowest vertex of every clip on y = 0), and the whole thing
   turned to face −Z with its feet on y = 0 — the frame every model in the game uses.
   `--check` re-reads the result and verifies all of that, the seat height and knee
   position of the seated clips and the pace of every walk included. Without `--plan`
   the MakeHuman clips stay, with the `walk` replaced by a procedural in-place cycle and
   the `sit` by a chair pose built out of the rest pose — the way the people were made
   before the recordings.

## The recordings

Two motion capture collections whose licences ask for attribution and nothing else
(`THIRD_PARTY_LICENSES.md` gives it):

- **100STYLE** (Ian Mason, University of Edinburgh; CC BY 4.0): one actor walking,
  running and standing in a hundred styles, 60 fps, Xsens suit. The everyday ones are
  used — `Neutral`, `Rushed`, `OnPhoneLeft`/`Right`, `HandsInPockets`, `ArmsFolded`,
  `ArmsBehindBack`, `Depressed`, `Proud`, `Elated` as walks, those and `Old`, `LookUp`,
  `Akimbo`, `Followed` as idles. `Frame_Cuts.csv` trims the T-poses off each file. The
  `Old` walk is a shuffle at a quarter of a metre a second and is not used as a walk.
- **Open Motion Project** (ACCAD, The Ohio State University; CC BY 3.0): three actors
  — `Female1`, `Male1`, `Male2` — walking, standing, swaying, looking around and, for
  the woman, waiting for 38 seconds. MotionBuilder exports at 30 or 120 fps.

`mocap.py` reads the BVH files and puts the motion onto the game rig without trusting
either skeleton's rest pose (the ACCAD men's is not a T-pose but a heap of pre-rotations
baked into the joint offsets). For every bone it wants two anatomical directions — the
bone itself and a twist reference that means the same thing on both skeletons: the
elbow's hinge axis for the arm, the knee's for the leg, the way the body faces for the
trunk — and reads them off the motion: where the bones point, and about which axis the
elbows and knees bend. That gives every source joint a constant anatomical frame in its
own coordinates, the game rig's come out of its rest pose the same way, and a frame maps
as *R_target = R_source · C_source · C_targetᵀ*. Bone lengths stay the character's own;
only the pelvis travels, scaled by the ratio of the leg lengths, and the clip is
re-grounded afterwards.

Out of each recording the pipeline takes:

- **Walks:** one gait cycle, left heel strike to left heel strike, out of the steadiest
  straight stretch of brisk walking, turned so that the line this very cycle went along
  is the front (an actor who curves through the stretch would otherwise step sideways),
  made in-place and closed into a loop, 30 fps. The
  clip is named after the pace it was walked at *on this character's legs* —
  `walk_<cm/s>`, so a short woman's `Neutral` is `walk_81` and a tall man's `walk_103` —
  and the game speeds it up or down from there (`world_render::people::gait`).
- **Idles:** ten to twenty seconds wherever the recording closes best on itself (pose and
  motion alike at both ends), the join worked off over the last second, 15 fps: `idle`,
  `idle2`, ….
- **Seated clips:** the upper body of an idle over the chair pose — a passenger folding
  her arms or looking at her phone in her seat — as `sit`, `sit2`, ….

`build_all.py` decides who gets what (`clip_plan`, `--plan` prints it): one of the actor's
own clips of each kind (a real woman for the women, one of the two men for the men), the
neutral walk and idle everybody has, and the rest drawn from the age band's styles —
phones and pockets for the young, folded arms and the departure board for the middle-aged,
hands behind the back and a bowed head for the old — four walks, four idles and three
seated clips per person, the same draw every time for the same character id. That is
about 0.8 MB of animation per person on top of the 1.7 MB of mesh and texture.

## Running it

MakeHuman 2 lives in `~/.local/share/makehuman2` with its own virtual environment;
the asset packs (skins, clothes, hair, rigs, poses — all CC0 from the MakeHuman
project) in `~/Documents/makehuman2/data`. The scripts need that interpreter:

```
PY=~/.local/share/makehuman2/venv/bin/python
$PY -m pip install meshoptimizer pillow      # once
python3 tools/characters/fetch_mocap.py      # once: the recordings, ~120 MB into ~/.cache
$PY tools/characters/build_all.py            # everything: ~2 min for 24 people
$PY tools/characters/build_all.py --only f01_lena,m07_michael
$PY tools/characters/build_all.py --skip-export   # reuse the raw exports, rebuild the game files
$PY tools/characters/build_all.py --no-mocap      # MakeHuman's own clips instead of the recordings
```

`mh2_export.py` builds its own MakeHuman home under `~/.cache/connected-rails/mh2home`
with symbolic links to the asset packs in the `<category>/hm08/<asset>` layout MakeHuman
2 expects (the packs are laid out the MakeHuman 1 way) and points MakeHuman at it through
`MH_HOME_LOCATION`, so the user's own MakeHuman installation is never touched. The
exported `.mhm` files land next to the raw exports; MakeHuman 2's GUI opens them, which
is the way to tune a face by hand — copy the modifier lines back into the roster.

The 100STYLE files come one by one from the links on the dataset's page (Google Drive);
should those go away, the whole dataset is archived on Zenodo (`archive_url` in
`clips.json`) — unpack its BVH files into the cache folder of the source by hand.

## Git LFS

`mods/**/*.glb` is tracked by Git LFS (`.gitattributes`), so a rebuilt roster is committed
like any other file — git stores the pointer, LFS the 2.5 MB objects. `git lfs install`
once per machine is all it takes; without it a checkout holds pointer files and the mod
loader warns about each of them.

## What the game expects

The conventions are documented with the content type in
`crates/content/src/characters.rs`: origin on the ground between the feet, face towards
−Z, metres, the LOD nodes and the clip families — `idle`, `idle2`, … and `sit`, `sit2`, …
as the poses a person is spawned in, `walk_<cm/s>` (or a plain `walk` at 1.5 m/s) as the
gaits, `stand`, `stand2`, … as held frames a model without idles falls back to.
`mods/people/characters/<id>.ron` names the file, gender, roles (`Player`, `Passenger`)
and tags; the app picks from that registry.
