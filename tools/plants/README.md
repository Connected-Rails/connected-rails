# Plant models

The standing crop's near level uses real plant models — low-poly crops by
**Quaternius**, published under **CC0 1.0** (via [Poly
Pizza](https://poly.pizza), which hosts them as glTF). This directory holds
the normalised results; nothing of the original distribution is used at
runtime.

| file | model | stands in for |
|---|---|---|
| `wheat.glb` | Wheat | winter cereal, summer cereal |
| `corn.glb` | Corn | maize |
| `flowers.glb` | Flowers | rapeseed (flowering), fallow |
| `turnip.glb` | Turnip | sugar beet |
| `clover.glb` | Clover | potato, legume |
| `lettuce.glb` | Lettuce | vegetable |
| `grass.glb` | Grass | grassland, other |
| `hay.glb` | Hay | stubble accents |
| `vines.glb` | Vines | vineyard |
| `tree.glb` | Tree | orchard |

## Rebuilding

```bash
node tools/plants/fetch.mjs
```

downloads each model from Poly Pizza and runs
`tools/plants/normalise.mjs`, which bakes the node transforms in, puts the
origin at the foot, scales every model to exactly one metre tall, re-indexes
the vertices and writes one compact GLB per crop. The renderer scales each
model by the phenology's stand height and tints it with the field's own
colour, so one wheat model serves every stage from sprout to ripe.

If Poly Pizza moves a model, its id is in `fetch.mjs` — pick the new one on
the model's page.
