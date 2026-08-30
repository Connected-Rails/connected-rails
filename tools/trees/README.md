# Trees: the ez-tree pipeline

The vegetation in `mods/trees/` — twenty-eight species of Central Europe, three
individuals of each, four levels of detail, three seasons — is generated, not
modelled. One catalogue and one script turn it out:

```
species.json ──build_trees.mjs──▶ mods/trees/assets/<species>_<a|b|c>[_herbst|_winter].gltf
             │   (ez-tree)                        + <species>_<variant>.bin
             │                                    + <species>_<season>.png
             └───────────────────▶ mods/trees/objects/<species>_<variant>.ron
                                   mods/trees/mod.ron
```

## Running it

```
node tools/trees/fetch_ez_tree.mjs           # once: clone and build ez-tree
node tools/trees/fetch_foliage.mjs           # once: the CC0 leaf and twig scans
node tools/trees/fetch_foliage.mjs --all     # every leaf set, for browsing
node tools/trees/build_trees.mjs             # everything: about 50 s
node tools/trees/build_trees.mjs --only rotbuche,fichte
node tools/trees/build_trees.mjs --report    # triangle budget, writes nothing
node tools/trees/build_trees.mjs --preview   # a sheet per species into /tmp/tree-preview
```

Node 20 or newer plus `curl` and `unzip`, nothing else — no npm dependency of
this repository, no Python, no image library. The PNG codec, the texture
painters, the leaf segmentation, the rasteriser and the glTF writer are all in
`lib/`. `build_trees.mjs` fetches whatever it is missing by itself.

**[ez-tree](https://github.com/dgreenheck/ez-tree)** (MIT, Daniel Greenheck) is
the procedural generator the geometry comes out of. It is neither vendored nor
an npm dependency: the version published to npm predates the LOD API the
pipeline needs, so `fetch_ez_tree.mjs` clones the pinned commit into
`~/.cache/connected-rails/ez-tree` — beside the motion capture recordings of
`tools/characters` — and builds it there. Nothing of it is checked in.

## What comes out

Per species, three individuals seeded from the species id and the variant
letter. Every model carries four levels as glTF nodes named the way the app
reads them (`crown_LOD0` … `crown_LOD3`, `sim_core::train::lod_level`):

| level | what it is | triangles (mean) |
| ----- | ---------- | ---------------- |
| `crown_LOD0` | every branch and every foliage card | 5 500 |
| `crown_LOD1` | the twigs dropped, half the cards, single billboards | 1 200 |
| `crown_LOD2` | trunk and limbs only, a third of the cards | 460 |
| `crown_LOD3` | two crossed quads with a picture of the tree | 4 |

The distances they hand over at are **not** the renderer's fixed bands. Each
object names its own in `lod_distances` (`track_model::TrackObject`), scaled to
the plant's height — a level pays for its triangles only while the plant covers
enough pixels, and a thirty metre spruce covers rather more of them than a two
metre blackthorn. A blackthorn is done at 700 m, a fir is drawn to 2 500 m.

The bands are **generous**, in units of the plant's own height: 4× to `LOD1`,
14× to `LOD2`, 40× to `LOD3`. What decides whether a level is enough is how many
*pixels* the plant covers, which is `height / distance × (screen height / field
of view)` — about `height / distance × 1375` on a 1440-line screen at sixty
degrees. That puts the hand-overs at roughly 340, 100 and 35 pixels, and a
thirty metre beech at 120 m, 420 m and 1.2 km.

Judge this **at the resolution it will be played at**. The same level looks a
good deal finer in a 1200-line window than on a 1440-line screen, which is how
an earlier set of numbers less than half these passed a review and still had a
wood two hundred metres off reading as streaks.

`crown_LOD3` stays far out, at about **forty times the height**, where a tree is
thirty-five pixels tall. It is two quads crossed at a right angle, and a crossed pair has a seam:
whichever blade the camera looks along edge-on is drawn as a narrow strip
*through* the other one, showing the tree's picture squeezed into a few pixels.
At forty pixels that strip is under a pixel wide; at four hundred it looks as if
the tree had been sliced. One quad instead of two is not the answer — a single
fixed billboard vanishes the moment the camera looks along it, and on a railway
the angle to a given tree sweeps through every value as the train passes.

So `crown_LOD2` carries the middle distance on real geometry, and the trees
between two hundred and five hundred metres cost two hundred and seventy
triangles each rather than four. That is the price of the seam not showing.

**One normal per blade, the direction the blade faces.** A blade is a flat quad
and a flat quad has one normal; giving each corner its own — outwards from the
trunk, which is the obvious thing — lights the left half of the blade
differently from the right and splits every tree down the middle with a dead
straight vertical line. Straight up instead is worse in another way: a billboard
with an up normal has no front and no back, so it never darkens when the sun is
behind it — it drinks the sky, and at dawn a backlit wood glows orange. The face
direction keeps front and back, and the renderer flips it for whichever side is
seen.

No upward component, deliberately: `doubleSided` negates the whole normal on the
far side, so an upward tilt comes back as a downward one and the same wood is
lit from below when seen from the other side. What the tree loses in overhead
light it gets back from the picture, which is baked with its own relief.

**One material per tree.** Bark, two foliage cards and the impostor share a
single 1024 × 512 atlas, so a level is one primitive and the renderer spawns
one entity per level instead of two (`world_render::scatter`). The bark UVs are
meshed to wrap exactly once around a branch, which is what lets them live in an
atlas rectangle at all.

**Seasons.** `objects/*.ron` names an `autumn_model` for everything that turns
and a `winter_model` for everything that drops its leaves; evergreens get a
snowed sheet under the same geometry. Summer, autumn and an evergreen's winter
share one `.bin` — the same wood under a different sheet of leaves — and only
the season in play is ever loaded.

## The catalogue

`species.json` is the source of truth. Per species:

- **`height`** `[min, max]` in metres; the three individuals are scaled across
  that range, and `TreeSource.scale` varies it further in the line file.
- **`crown`** the crown width as a share of the height. The build measures what
  it actually grew and warns when the two disagree — that check is how the
  branch angles and lengths were tuned.
- **`cull`** how far the species is worth drawing [m].
- **`tags`** what the route editor filters on. A `stand-…` tag makes the species
  a member of that stand: the forest brush offers every `stand-…` tag the
  installed mods carry and mixes the species tagged with it. There is no stand
  file and no registry — a mod that adds a species to an existing wood needs
  nothing but the tag the others already have.
- **`tree`** ez-tree options, merged onto the base of its kind (`bases` at the
  top of the file). Branch levels, children, angles, lengths, radii, and how
  many foliage cards sit on a twig.
- **`bark`** and **`foliage`** for the texture painters.

### Leaves: scanned, arranged here

A card is a *spray*, not a leaf: a shoot with fifteen to twenty leaves, or a
handful of needle twigs. One quad per leaf would be a hundred thousand quads for
a beech; a spray is ten times fewer for the same canopy, and that is where a
wood's triangle budget is won.

The **leaves on it are photographs**, from two CC0 libraries that
`fetch_foliage.mjs` pulls into `~/.cache/connected-rails/foliage/`:

- **ambientCG** `LeafSet###` — sheets of single leaves on black with an opacity
  map. `lib/leaves.mjs` flood-fills the mask, cuts each leaf out, and stamps it
  rotated and scaled wherever the composition wants one.
- **Poly Haven** `fir_tree_01` / `pine_tree_01` twig atlases — whole needle
  sprays, which the conifers use as they are (`"whole": true`).

**The arrangement stays ours.** Which leaves sit on which shoot, at what angle,
how big and how many — that is what tells an ash from an oak, and no sheet
carries it. A species names its sheet in `species.json`:

```json
"scan":       { "set": "LeafSet016", "tint": [0.74, 0.86, 0.66] },
"autumnScan": { "set": "LeafSet012" }
"scan":       { "asset": "fir_tree_01", "map": "twig",
                "leaves": [1, 2, 3, 4, 5], "whole": true }
```

`leaves` are indices into the sheet's shapes in reading order (leave it out for
all of them), and `tint` multiplies the scan — the sheets are lit for a
catalogue and read a stop too bright for a wood. Autumn gets a *turned* sheet
rather than a tint: multiplying green towards orange makes mud. A species with
no `scan` falls back to the painted blades, which are still there
(`OUTLINES` in `lib/foliage.mjs`: ovate, lobed, palmate, pinnate, lanceolate,
deltoid, needle fascicle, needle spray, larch rosette, juniper scale).

The card's layout follows the quad's own frame: ez-tree emits `uv (0,0)` at the
attachment point and glTF puts `v = 0` on the image's top row, so the shoot
enters the card at the top edge and the leaves hang downwards.

### Bark: painted

`lib/bark.mjs` paints the bark from the species entry — a birch is white with
black lenticels, a Scots pine orange-red in flat plates, a beech smooth grey —
because a library of generic bark scans gives none of that, and the bark is what
tells two trunks apart at twenty metres. There is no normal map: the relief is
shaded into the colour, which is one texture per species instead of two.

### The impostor

`lib/impostor.mjs` rasterises `crown_LOD0` orthographically, sampling the very
atlas the near levels sample, and pastes the result into the atlas's fourth
cell. The silhouette, the colour and the density of the canopy therefore agree
with what the tree looked like a metre before it switched.

**The picture carries the modelling.** A billboard has one normal per blade, so
the renderer shades the whole of it by a single number; a flat picture on it is
a flat blob. The bake therefore keeps a range of its own: bright where the
canopy faces up, dark underneath, and darker again the further back in the crown
a texel sits — the depth buffer read as the occlusion of the mass. No sun
direction, though; that one the renderer owns.

**How much of it is measured, not guessed.** Render the same wood twice, once as
`crown_LOD0` and once forced to `crown_LOD3`, and compare the mean luminance of
the tree pixels against the sky behind them (the mask comes from a third render
with no trees at all). The two have to land on the same number or the wood steps
brighter or darker the moment it hands over. As it stands: 0.510 against 0.511
with the sun ahead, 0.597 against 0.598 with the sun behind. The first attempt
at these constants was a quarter too dark and nothing in the shader would have
said so.

## Cut-outs at a distance

A canopy is mostly holes, and three things have to be right or a wood falls
apart the further away it is:

- **The mip chain keeps the alpha's coverage** (`world_render::with_mipmaps`).
  Box-filtering alpha halves it at every level, so by the fourth or fifth almost
  no texel still reaches the 0.5 cutoff — the foliage evaporates and the opaque
  trunks stay, which is what a forest at half a kilometre used to look like.
  Each level's alpha is rescaled so the share of texels passing the cutoff stays
  what it was at full size.
- **Alpha-to-coverage, not a hard test.** The renderer switches a tree's
  material over once it has loaded, so a leaf's edge is resolved by the sample
  mask instead of stepping from texel to texel. Without MSAA Bevy falls back to
  the mask by itself.
- **The levels crossfade** rather than switching in one frame
  (`world_render::scatter::crossfade`, ±8 % of the hand-over distance).

## Meshing

`lib/mesh.mjs` meshes the ez-tree skeleton itself rather than calling
`tree.createGeometry`. The construction is the same (and `verifyAgainstEzTree`
checks it against the library's own output at full detail, so a change on
either side fails the build), with two additions:

- **`minRadius`** drops every branch thinner than a share of the trunk. The
  leaves stay where they were, so the canopy keeps its shape while the twigs
  holding it up disappear. ez-tree's own coarsest level keeps one ring of three
  segments per branch, which is eleven hundred triangles of twig before a
  single leaf.
- **one bark wrap** per branch, so `u` stays inside `[0, 1]` and bark and
  foliage fit in one atlas.
- **canopy normals** on the leaf cards: seven parts the vertex's direction out
  of the middle of the canopy, three parts the card's own facing, plus an
  upward bias for the sky. Neither end works alone. All card — which is what
  ez-tree gives you, the card's normal being wherever it happened to put the
  card — means two neighbouring leaves face opposite ways, and lit from the
  side one is blown out while the other is black: a crown two hundred metres
  off reads as a mosaic of hard bright and dark blocks. All canopy means every
  card on the sunlit side shares a normal, and the tree becomes a single flat
  blob. The mixture keeps the rounded mass and leaves each leaf enough of its
  own to read as a leaf. (ez-tree's own `leaves.roundedNormals` is unused.)

## Adding a species

1. Add the entry to `species.json` — copy the nearest relative and change the
   heights, the colours, the leaf shape and the branch angles.
2. `node tools/trees/build_trees.mjs --preview --only <id>` and look at
   `/tmp/tree-preview/<id>.png`: four levels side by side and the atlas below.
3. Tag it into the stands it belongs in.
4. `node tools/trees/build_trees.mjs` for the whole set, and commit — the
   `.png` and `.bin` files go through Git LFS (`.gitattributes`).

## Git LFS

`mods/**/*.png` and `mods/**/*.bin` are tracked by Git LFS, like the characters'
`.glb`. `git lfs install` once per machine; without it a checkout holds pointer
files and the mod loader warns about each of them. The glTF files themselves are
JSON and stay ordinary git objects, so a change to a model's structure diffs.
