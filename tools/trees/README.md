# Artist tree import

`mods/trees` is built from hand-modelled CC0 trees by Midge “Mantissa”
Sinnaeve and Poly Haven. It contains 28 catalogue species, three distinct
individuals of each, four LOD levels and seasonal models. It no longer contains
a procedural tree generator.

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

Download the 1K Blend packages and their 1K texture maps for these Poly Haven
assets below `~/.cache/connected-rails/polyhaven/<asset>/`:

| cache directory | official CC0 asset |
| --- | --- |
| `fir_tree_01/` | [Fir Tree 01](https://polyhaven.com/a/fir_tree_01) |
| `pine_tree_01/` | [Pine Tree 01](https://polyhaven.com/a/pine_tree_01) |
| `fir_sapling/` | [Fir Sapling](https://polyhaven.com/a/fir_sapling) |

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
| `crown_LOD0` | textured source wood and source foliage cards/leaves | about 69k–162k triangles |
| `crown_LOD1` | reduced branches and foliage groups from the same source individual | about 16k–51k triangles |
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
