# Vegetation asset licences

The broadleaf meshes and some source photographs in this mod are modified
versions of free CG tree packs by **Midge “Mantissa” Sinnaeve** from the
official [Mantissa Freebies page](https://mantissa.xyz/free.html). Every source
archive contains a `LICENSE-CC0.txt` and is released under
[Creative Commons CC0 1.0 Universal](https://creativecommons.org/publicdomain/zero/1.0/legalcode):

- Free Fir Trees — `mantissa.xyz_free_firs.zip`
- Free Spruce Tree Pack — `mantissa_spruce_trees_pack.zip`
- Free Generic Tree Pack — `mantissa_generic_tree_pack_fbx.zip`
- Free Birch Tree Pack — `mantissa_birch_tree_pack.zip`
- Free Cherry Tree Pack — `mantissa_cherry_tree_pack.zip`
- Free Japanese Maple Pack — `mantissa_japanese_maple_pack.zip`

The conifer meshes, foliage and bark maps are modified versions of these
official **Poly Haven** assets, also published under CC0 1.0:

- [Fir Tree 01](https://polyhaven.com/a/fir_tree_01) — Rico Cilliers (model), Rob Tuytel (photography)
- [Pine Tree 01](https://polyhaven.com/a/pine_tree_01) — Rico Cilliers (model), Rob Tuytel (photography)
- [Fir Sapling](https://polyhaven.com/a/fir_sapling)

The shrubs and understorey use modified forms and atlases from these additional
official Poly Haven assets:

- [Shrub 01](https://polyhaven.com/a/shrub_01) — Rico Cilliers
- [Searsia Lucida](https://polyhaven.com/a/searsia_lucida) — James Ray Cock (modeling), Jenelle van Heerden (photography)
- [Wild Rooibos Bush](https://polyhaven.com/a/wild_rooibos_bush) — James Ray Cock (modeling), Jenelle van Heerden (photography)
- [Fern 02](https://polyhaven.com/a/fern_02) — Rob Tuytel (scanning), Rico Cilliers (modeling)
- [Nettle Plant](https://polyhaven.com/a/nettle_plant) — Rob Tuytel (photography), Rico Cilliers (modeling)
- [Periwinkle Plant](https://polyhaven.com/a/periwinkle_plant) — Amal Kumar

Poly Haven's licence terms are recorded on its official
[licence page](https://polyhaven.com/license).

The original author waived copyright and related rights to the extent permitted
by law. CC0 permits copying, modification and commercial distribution without
attribution. Attribution is nevertheless retained here for provenance.

Connected Rails changes include geometry reduction, removal of underground
roots, metric scaling, PBR material conversion, texture downsampling, seasonal
colour grading and whole-plant LOD impostors. Low vegetation keeps the complete
artist mesh because its scanned stem and foliage share one alpha atlas. The
logical catalogue maps the artist packs to Central-European species by growth
form; generic source forms are used where a species-specific CC0 scan was not
available. The catalogue's German/common and botanical names describe game
content; they are not a claim that every generic source form is a botanical
reference specimen.

The hedge objects (`feldhecke_*`, `weissdornhecke_*`, `ligusterhecke_*`,
`hainbuchenhecke_*`, `immergruene_hecke_*`) introduce no additional source
material. They are repeatable compositions of the Mantissa and Poly Haven CC0
models listed above, built by `tools/trees/build_hedges.py`.

These assets are separate mod assets under the additional permission in the
repository’s root `LICENSE`; their CC0 status is compatible with the EUPL-1.2
codebase and with commercial redistribution.
