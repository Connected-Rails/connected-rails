# Plant models

The standing crop's near level uses real plant models — low-poly crops by
**Quaternius**, published under **CC0 1.0** (via [Poly
Pizza](https://poly.pizza), which hosts them as glTF). This directory holds
the normalised results; nothing of the original distribution is used at
runtime.

| file | model | variants | stands in for |
|---|---|---|---|
| `wheat.glb` | Wheat | 1 | winter cereal, summer cereal |
| `corn.glb` | Corn | 1 | maize |
| `flowers.glb` | Flowers | 7 | rapeseed, fallow |
| `turnip.glb` | Turnip | 1 | sugar beet (sunk, so the root is in the soil) |
| `clover.glb` | Clover | 1 | potato, legume |
| `lettuce.glb` | Lettuce | 1 | vegetable |
| `grass.glb` | Grass | 2 | grassland, other |
| `hay.glb` | Hay | 1 | a cut field, whatever stood on it |
| `vines.glb` | Vines | 1 | vineyard |
| `tree.glb` | Tree | 1 | orchard |

## What the normaliser guarantees

The renderer bakes these models into one mesh per material per cell of a
field, so the file has to arrive in exactly the frame it draws in:

* **Y up, foot at the origin, centred on its own footprint.** A Poly Pizza
  model carries the −90° about X that turns a Z-up export into glTF's Y-up,
  and its `rotation` is a **quaternion** — reading those four numbers as
  Euler angles lays the crop on its side and blows a wheat plant's footprint
  up from 30 cm to five metres.
* **One metre tall.** The *tallest* variant is a metre; the others keep their
  height relative to it, so a small grass tuft stays smaller than a large
  one. The renderer scales by the phenology's stand height, so one wheat
  model serves every stage from sprout to ripe.
* **One variant per plant.** A pack is a *scene*: the flowers file is seven
  clumps laid out in a row, the grass file two tufts sixty metres apart.
  Merged into one mesh that is a flowerbed, not a flower — so each placed
  node becomes a glTF mesh of its own and every plant draws one of them.
* **Cut out, not blended.** A leaf sheet comes out `MASK` at 0.5 wherever the
  pack declared `BLEND`/`MASK` or a base colour alpha under one, and wherever
  the picture carries transparency and the pack said nothing: blended
  vegetation has to be sorted against every other leaf in the field, writes no
  depth, and costs a full-rate pass over a stand that is mostly holes. A sheet
  the pack calls `OPAQUE` stays opaque even if it happens to be RGBA — a bark
  texture with every texel solid is not a cut-out, and testing it would cost
  the trunk its early depth.
* **Only what is drawn.** Images no material samples are left behind (the
  tree pack ships a megabyte of normal map nothing here reads), triangles the
  vertex weld collapsed are dropped, and vertices are welded on every
  attribute rather than on position alone — welding an atlas card's corner by
  position smears one card's picture across the next.

## Rebuilding

```bash
node tools/plants/fetch.mjs
```

downloads each model from Poly Pizza and runs `tools/plants/normalise.mjs`
on it. To re-run just the normaliser:

```bash
node tools/plants/normalise.mjs in.glb crates/world-render/src/plants/out.glb
```

It prints the variant count, the triangles, the source height and the widest
variant's footprint — a plant several metres wide at a metre tall is the
transform bake having gone wrong, not a big plant.

If Poly Pizza moves a model, its id is in `fetch.mjs` — pick the new one on
the model's page.
