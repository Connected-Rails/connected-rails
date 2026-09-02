# Artist tree import

`mods/trees` is built from hand-modelled CC0 trees by Midge “Mantissa”
Sinnaeve and Poly Haven. It contains 46 catalogue species and vegetation types,
three distinct individuals of each, plus 15 repeatable hedge sections, seasonal
models, four plant LODs and twelve hedge LODs. It no longer contains a procedural
tree generator.

The source models and textures are dedicated to the public domain under
[CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/). The checked-in
game assets are modified, reduced and re-packed versions. See
[`mods/trees/LICENSES.md`](../../mods/trees/LICENSES.md) for the precise source
archives and licence record.

## Rebuilding

Download the six free archives from the official
[Mantissa Freebies page](https://mantissa.xyz/free.html) and unpack them below
`~/.cache/connected-rails/mantissa-trees/`:

| cache directory | official archive |
| --- | --- |
| `fir/` | `https://ftp.mantissa.xyz/resources/fir_free/mantissa.xyz_free_firs.zip` |
| `spruce/` | `https://ftp.mantissa.xyz/resources/trees/mantissa_spruce_trees_pack.zip` |
| `generic/` | `https://ftp.mantissa.xyz/resources/trees/mantissa_generic_tree_pack_fbx.zip` |
| `birch/` | `https://ftp.mantissa.xyz/resources/trees/mantissa_birch_tree_pack.zip` |
| `cherry/` | `https://ftp.mantissa.xyz/resources/trees/mantissa_cherry_tree_pack.zip` |
| `maple/` | `https://ftp.mantissa.xyz/resources/trees/mantissa_japanese_maple_pack.zip` |

Fetch the pinned 1K Blend packages and their referenced 1K texture maps through
Poly Haven's public API:

```bash
python tools/trees/fetch_polyhaven.py
```

The downloader verifies Poly Haven's published MD5 sums and stores the sources
below `~/.cache/connected-rails/polyhaven/<asset>/`:

| cache directory | official CC0 asset |
| --- | --- |
| `fir_tree_01/` | [Fir Tree 01](https://polyhaven.com/a/fir_tree_01) |
| `pine_tree_01/` | [Pine Tree 01](https://polyhaven.com/a/pine_tree_01) |
| `fir_sapling/` | [Fir Sapling](https://polyhaven.com/a/fir_sapling) |
| `shrub_01/` | [Shrub 01](https://polyhaven.com/a/shrub_01) |
| `searsia_lucida/` | [Searsia Lucida](https://polyhaven.com/a/searsia_lucida) |
| `wild_rooibos_bush/` | [Wild Rooibos Bush](https://polyhaven.com/a/wild_rooibos_bush) |
| `fern_02/` | [Fern 02](https://polyhaven.com/a/fern_02) |
| `nettle_plant/` | [Nettle Plant](https://polyhaven.com/a/nettle_plant) |
| `periwinkle_plant/` | [Periwinkle Plant](https://polyhaven.com/a/periwinkle_plant) |

The importer expects Blender 4.3 or newer and ImageMagick:

```bash
blender -b --python tools/trees/import_mantissa.py
blender -b --python tools/trees/import_mantissa.py -- --only fichte,rotbuche
```

The source FBXs and Blend files are intentionally not committed: the untouched
files are several gigabytes and have millions of triangles per tree. The
importer uses Poly Haven's artist-authored clean LOD mesh for conifers, keeps
the source forms and bark UVs, and retains a spatially distributed selection of
the actual Mantissa leaves. It removes underground root meshes, scales each
form to the catalogue height/crown and decimates connected wood for the near
LOD. It does not synthesize trunks, branches, needles or crowns.

The eighteen added shrub and understorey entries use six further Poly Haven
artist assets. Their scanned stem and foliage share one alpha atlas, so the
importer keeps the complete authored mesh instead of trying to split it into a
synthetic trunk and crown. Where the source supplies `LOD1`, that authored mesh
is retained (the coarser authored LOD2 where available); Fern 02 is already small enough to keep its complete mesh in both
near bands. Catalogue mappings provide German species names, natural size and
habitat tags. They deliberately describe the role and growth form in the route,
not a claim that a generic scan is a botanical reference specimen.

## Hedges

`tools/trees/build_hedges.py` composes the already imported CC0 artist plants
into five repeatable hedge types, with three variations each: mixed field,
hawthorn, privet, hornbeam and evergreen formal hedge. The 6 m formal sections
and 8 m field sections overlap at their ends, so copies can be placed end to
end without a visible gap. Reduced source geometry supplies the visible stems
and irregular growth. A closed three-dimensional envelope of thousands of
small cards, each mapped to one individual leaf in Poly Haven's CC0 atlas,
provides clipped density without the conspicuous whole-hedge rectangle of a
near billboard. No hedge-wide or whole-plant impostor is used. The far levels
retain progressively smaller subsets of the reduced 3D source plants and enlarge
only their thousands of individual leaf cards. Their twelve audited triangle maxima are
84,351 / 81,151 / 54,303 / 54,103 / 40,579 / 40,379 / 33,517 / 33,317 /
26,455 / 26,255 / 22,624 / 18,993. Projected leaf coverage rises to 100 / 311 /
557 / 972 / 1,696 / 2,866 / 4,763 / 7,656 / 11,150 / 16,017 / 22,104 /
30,250 percent across the distance levels. Handoffs at 60, 120, 180, 250, 325,
400, 500, 600, 700, 800 and 900 m replace the former coarse jumps; the later
levels retain 4,500–5,100 spatially distributed leaf cards instead of collapsing
to a few oversized patches.
Summer, autumn and winter stay in lockstep with the source plants, and the
vegetation renderer still draws only one distance level.

No additional third-party source is used: the hedge sections are arrangements
of the Mantissa and Poly Haven material already recorded in `LICENSES.md`.
Rebuild or audit them independently with:

```bash
python tools/trees/build_hedges.py
python tools/trees/build_hedges.py --audit
```

## What the object file says

Beside the model, the seasons and the LOD distances, every `objects/*.ron` carries the
**crown as a `footprint`**, broadest span first, measured off the finished `crown_LOD0`
mesh rather than taken from the catalogue's height ratio — what the file states is what
the tree actually spans. It is what the route editor's imagery detection compares a crown
measured off an aerial photograph against, to decide which species to plant there and how
far to grow it (README, *Detecting from the aerial imagery*). A re-import rewrites it with
the geometry, so the two can never drift apart.

## LOD and materials

Every glTF has exactly these nodes:

| node | content | target budget |
| --- | --- | ---: |
| `crown_LOD0` | textured source wood and source foliage cards/leaves | 392–176k triangles |
| `crown_LOD1` | reduced branches and foliage groups from the same source individual | 392–54k triangles |
| `crown_LOD2` | four-view whole-tree impostor | 8 triangles |
| `crown_LOD3` | two-view crossed impostor | 4 triangles |

LOD distances are derived from the individual tree height. LOD0 hands to a
reduced version of the same 3D branches and foliage at 3.5× height (at least
45 m). Real geometry remains until 15× height (at least 400 m); only then does
the eight-triangle whole-tree render appear. It hands to the crossed distance
version at 30× height (at least 800 m). LOD1 keeps roughly 80% of the near
crown's leaf area: it retains more spatially distributed source leaves and may
enlarge them by at most 1.55×. This avoids winter-thin crowns without producing
the huge distance cards of the old procedural trees.
The audit verifies that LOD0 and LOD1 keep their height, crown extents and centre,
and rejects abnormally broad branch triangles caused by an over-aggressive mesh
collapse. Where the source identifies a structural trunk, it is reduced on a
separate budget before being joined back to the boughs. The audit compares six
fixed height bands and rejects a floating trunk base or a missing trunk section.
Poly Haven pine/larch keeps a slightly higher LOD1 branch floor for the same reason.
The Japanese-maple source keeps its intact trunk and major branches but omits its
nearly half-million disconnected microscopic branch faces: collapsing those open
tubes produced long triangular sails while adding no useful crown silhouette.
The leaf-size correction is capped while the branch positions still follow the
species scale, so a six-metre artist source mapped to a thirty-metre catalogue
tree does not acquire thirty-centimetre leaves.

All three materials are glTF `pbrMetallicRoughness`, dielectric
(`metallicFactor = 0`) and carry an explicit roughness. Bark and foliage use
downsampled Mantissa or Poly Haven source maps (512²/256²); Poly Haven's split
twig colour and alpha maps are combined losslessly into an RGBA foliage map.
Those cut-outs use alpha masking, not alpha blending, so leaves write depth and
cannot be sorted behind the transparent cloud dome.
The distance material uses a rendered 512×256 two-view tree with alpha masking.
Summer and autumn share the same geometry buffer. A deciduous winter model
omits foliage primitives and its impostor is rendered from the source wood only.

The importer creates `screenshots/trees-mantissa-contact-sheet.png` from every
summer individual as a mandatory visual review sheet. Geometry and seasonal
impostors are always rebuilt together; skipping the renders is intentionally not
supported because it can leave a far LOD with an obsolete silhouette. The forest stress test is
independent and remains available:

```bash
node tools/trees/bench_forest.mjs --trees 20000 --run
```
