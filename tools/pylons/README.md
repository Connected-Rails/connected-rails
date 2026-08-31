# Pylons: the atlas of German overhead line supports, and the kit that builds them

Every German railway line runs past three grids at once, and they look nothing
alike: the transmission grid on lattice towers crossing the fields, the
medium-voltage grid on grey concrete poles feeding the villages, and the
railway's own 110 kV, 16.7 Hz line that follows the track for hundreds of
kilometres.

This directory is the catalogue of the shapes those three produce and the
generator that turns the catalogue into models:

```
pylons.json ──build_pylons.mjs──▶ mods/pylons/assets/<type>_<role>.gltf
            │      (lib/kit)                       + <type>.bin
            └──────────────────▶ mods/pylons/objects/<type>_<role>.ron
                                 mods/pylons/mod.ron
```

Eighteen types, thirty-three variants, four levels of detail each, and nine
painted textures between them. Nothing is modelled and nothing is downloaded:
every triangle in `mods/pylons/` comes out of `lib/kit.mjs`, every texel out of
`lib/texture.mjs`, and the numbers out of `pylons.json`. A mast whose crossarm
is 40 cm too short is a one-line edit to the catalogue and a re-run.

## Running it

```bash
node tools/pylons/build_pylons.mjs
node tools/pylons/build_pylons.mjs --only donaumast-380,bahnstrommast-110
node tools/pylons/build_pylons.mjs --report     # triangle budget, writes nothing
node tools/pylons/build_pylons.mjs --preview    # picture sheets into /tmp/pylon-preview
node tools/pylons/build_pylons.mjs --ink        # what each level does to the mast's ink
```

Node 20 or newer and nothing else — no npm dependency of this repository, no
modelling tool, no image library. The glTF writer is `lib/gltf.mjs`, the
geometry primitives are `lib/geom.mjs`, and the whole run takes under a second.
`--preview` borrows the PNG encoder from `tools/trees/lib/png.mjs` — the only
thing this tool takes from another, and it never ships.

**Look at `--preview` after editing the catalogue.** A Donaumast whose upper
crossarm came out wider than its lower one passes every check in this pipeline
and is wrong at a glance; `/tmp/pylon-preview/alle.png` puts every type side by
side at one scale, and each type gets its own sheet with the fine level from the
front and from along the line, plus the two coarse levels.

`<type>-handover.png` is the sheet that matters for the levels of detail: each
pair of levels is drawn at the metres per pixel of the distance it hands over
at, then magnified without filtering, so what is on the sheet is what the screen
gets. If the two halves of a pair look like the same mast, the hand-over is
invisible in the game.

## The kit

`lib/kit.mjs` has five builders, because everything a German overhead line
stands on is the same five pieces in different proportions:

| piece | what it is |
| --- | --- |
| **body** | four battered legs with X bracing to a waist, then a parallel shaft (`lattice`); or a tapered tube (`pole`, `tube`); or two legs under a beam (`portal`) |
| **crossarm** | two parallel trusses joined across, the top chord horizontal and the bottom chord running up to the tip |
| **fitting** | a cap-and-pin string hanging under the arm, a standing pin insulator, or the double-bell porcelain of a telegraph pole |
| **earth peak** | a small pyramid of the same lattice on the body top; one, two or none |
| **foundation** | four concrete stubs — the part of a mast actually seen from a train, because the grass grows up to them |

A Donaumast and a Bahnstrommast differ in that file by **nothing at all**. They
differ in the catalogue by a crossarm.

### Variants

Two per type, from the `roles` the atlas gives it: `_trag` (Tragmast,
suspension) and `_abspann` (Abspannmast, tension). The tension mast is stouter —
a wider base and heavier members — and its insulator strings lie along the
conductors instead of hanging under them, which is what a viewer actually reads
an Abspannmast off from a moving train.

**Height is not a variant.** The atlas gives a band, the placement gives a
scale, and a mast scaled by a tenth either way is the same drawing built taller
— which is what a manufacturer does too. Two files per type instead of eight,
and a line of Donaumasten still varies along its length.

### Levels of detail

| level | what it is | 380 kV Donaumast |
| --- | --- | --- |
| `mast_LOD0` | every member, every insulator disc, the foundations | 3 024 triangles |
| `mast_LOD1` | half the bracing panels, half the discs, coarser round sections | 1 416 |
| `mast_LOD2` | a quarter of the panels, one truss per crossarm instead of two, insulators without discs | 432 |
| `mast_LOD3` | the outline: legs, arm bars, a stub where each insulator was | 240 |

**The bands come off the members, not off the mast.** The first cut of this
scaled them by the mast's height, which puts a 12 m concrete pole with a 9 cm
crossarm and a 60 m Donaumast with a 16 cm brace a factor of five apart when
they in fact stop resolving at almost the same distance. What decides whether a
level is worth its triangles is how thin its thinnest member is:

- **`LOD0` → `LOD1`** where the thinnest member drops below a pixel. Past that
  it is not drawn but *sampled* — some pixel centres hit, others missed, and the
  pattern crawls as the camera moves.
- **`LOD1` → `LOD2`** where half the panels, drawn a fifth thicker, have gone
  the same way; about two and a half times further out.
- **`LOD2` → `LOD3`** where the mast's **body** is under two pixels wide. Below
  that there is no room for two legs and a gap between them, so there is nothing
  inside the outline left to draw and the bracing finally goes.
- **Culled** where the whole mast is ten pixels tall, capped at four kilometres
  — past where the terrain under it is still built.

The reference is **1440 lines at the simulator's 45° vertical field of view**.
Judge a level at the resolution it will be played at, or it will be judged
wrong; that is the same assumption `tools/trees` states and the same trap.

| type | LOD1 from | LOD2 | LOD3 | culled |
| --- | --- | --- | --- | --- |
| Donaumast 380 kV | 278 m | 617 m | 2 955 m | 4 000 m |
| Bahnstrommast 110 kV | 156 m | 347 m | 1 564 m | 4 000 m |
| Betonmast 20 kV | 156 m | 347 m | 1 133 m | 2 086 m |
| Streckenfernmeldemast | 122 m | 270 m | 800 m | 1 304 m |

**A coarse level is drawn thicker than the fine one it replaces.** Dropping half
the bracing does not just cost triangles, it costs *ink*: a lattice at six
hundred metres is a grey haze made of a hundred sub-pixel bars, and a level that
throws half of them away and draws the rest at their true width halves the grey.
The mast then visibly darkens as the train approaches, which is the one thing a
level of detail must not do. So `lib/kit.mjs` widens what is left
(`MEMBER_SCALE`, `ARM_SCALE`), and **`--ink` is what says by how much**: it
supersamples each level at the distance it hands over at and reports the
coverage against the level before it. Every hand-over in the catalogue is inside
±20 %, most inside ±10 %.

Two things `--ink` caught that no triangle count would have:

- The medium-voltage lattice pole lost **70 % of its ink** at `LOD3`, because
  that level dropped the fittings and its six pin insulators are most of what
  there is to see on a 15 m mast. They are a twentieth of a 60 m Donaumast and
  free to drop there — the same rule is right for one and wrong for the other.
  `LOD3` now keeps one bar per conductor point.
- **Ink is necessary and not sufficient.** An earlier `LOD2` dropped the bracing
  entirely and put the grey back by making the legs thick. The numbers matched;
  the picture was a bare A-frame, because a lattice mast at a kilometre is a
  haze *with structure in it*. `LOD2` keeps a quarter of the panels, and the
  bracing only goes where the body is under two pixels wide.

### The materials

Three painted maps per material, tiling **once per metre** — the UVs are in
metres, so the grain is the same size on a 36 cm leg as on a 6 cm brace, which
is what stops a tiled material from reading as wallpaper. Nine PNGs of 256 px
serve all thirty-three models, under a megabyte for the lot, and they are
generated (`lib/texture.mjs`) rather than scanned:

| material | what is painted |
| --- | --- |
| **verzinkt** | the zinc **spangle** — a Voronoi of crystal facets a few centimetres across, each with its own shade and its own gloss — over a slow weathering field, plus rolling marks and pitting |
| **beton** | the sand and the odd larger stone a centrifuged pole throws to its outer wall, the faint long streaks of the mould, and a slow dirt field |
| **holz** | the grain running the length of the pole, the fibre across it, and the checks a pole dries into |

**Weathered zinc is not a mirror**, and getting that wrong is what a first
attempt did: fresh galvanising is a metal with a reflectance near 0.85, so
`metallic = 1` and a bright base colour gave a mast that reflected the whole sky
and blew out white against it. What a mast on a line actually wears is zinc
*carbonate*, the chalky grey skin that forms in the first year or two — and that
is a **dielectric** layer over the metal. So the weathering field drives the
metallic as well as the roughness: a facet still bright reads as metal, one gone
chalky reads as the mineral covering it, and the mast comes out the mid grey a
mast is.

On top of the maps every vertex carries a **colour** (`geom.mjs`, `weather`):
per-member shade, because four hundred bars galvanised in the same bath still
come out a few per cent apart, and the dirt that gathers towards a mast's foot —
above about six metres a mast is clean, at the concrete it is brown-grey and a
fifth darker. That belongs in the vertex colour rather than the texture because
it is a property of the *mast*: the texture tiles every metre and knows nothing
about which end of the leg it is on.

**Three materials, not two.** The structure, the fittings — and the concrete a
lattice mast stands on. A galvanised-steel material on the foundation stubs is
the sort of thing nobody notices until they walk up to one, at which point the
mast is standing on four metal blocks.

The fittings stay constants: an insulator is glazed porcelain, smooth enough
that there is no microstructure worth a map and small enough that there would be
no room for one to show. `insulator_m` follows the **fitting**, not the body —
a 20 kV lattice pole carries the same 35 cm pin insulator a concrete pole does,
and keying it on the body kind gave it the 3.4 m suspension string of a
transmission tower, which is a metre and a half of porcelain hanging off a
15 m mast.

## The catalogue

`pylons.json` is data only; its `_comment` block documents every field. Two are
worth knowing about before using the file:

**`osm.design`.** OpenStreetMap tags every power tower and pole with `design=*`,
and the value set is exactly this taxonomy: `one-level`, `donau`, `barrel`,
`three-level`, `portal`, `triangle`, `asymmetric`. The import keys on it
directly, so a `power=line` way comes in with the right mast attached and
nobody has to choose one.

**`confidence`.** `sourced` means every number in the entry is backed by a
citation below. `estimated` means the shape is documented but the dimensions are
read off photographs and interpolated from the sourced types; the 110 kV and
medium-voltage heights are the main cases. Treat an estimated crossarm width as
a starting point, not as a drawing.

### Transmission, 110–380 kV — steel lattice

| type | `design` | height | crossarms | where |
| --- | --- | --- | --- | --- |
| **Donaumast** 380 kV | `donau` | 55–65 m | 2 (narrow over wide) | nationwide, the default |
| **Einebenenmast** 380 kV | `one-level` | 37–47 m | 1 long | eastern states |
| **Tonnenmast** 380 kV | `barrel` | 66–76 m | 3, middle widest | rare in DE; UK/CH/PL norm |
| **Donaumast** 220 kV | `donau` | 40–50 m | 2 | the ageing backbone |
| **Tannenbaummast** 220 kV | `three-level` | 40–55 m | 3, widening downwards | 1920s–30s survivors |
| **Donaumast** 110 kV | `donau` | 25–35 m | 2 | old federal states |
| **Einebenenmast** 110 kV | `one-level` | 22–30 m | 1 | new federal states |
| **Kombinationsmast** 380/110 | `donau` | 65–80 m | 3 | two levels on one mast |
| **Portalmast** | `portal` | 25–40 m | 1 beam on two legs | substation entries |
| **Kompaktmast** | `asymmetric` | 45–60 m | 3, tight | present day only |

The three 380 kV shapes are the ones a viewer will name, and the difference
between them is a trade of height against width: the Einebenenmast is the
shortest and needs the widest cleared strip, the Tonnenmast the tallest and
narrowest, the Donaumast the compromise that won. **West versus east is a real
distinction and cheap to use**: 110 kV on Donaumasten reads as the Rhineland,
the same circuits in one level read as Saxony.

### Railway power, 110 kV / 16.7 Hz

The Bahnstromleitung is the one this project cannot skip. It is 110 kV run as
2 × 55 kV against earth at 16.7 Hz, two two-pole circuits, so **four conductors
on one crossarm** — never three or six — on a lattice mast of about 28 m at
roughly 300 m spacing. Lines built up to about 1927 had two crossarms with the
upper one wider; after that the single level became the rule, and the two-level
form survives where four circuits run into a substation.

Four conductors on one arm is the single detail that makes a Bahnstrom mast
recognisable. Getting it wrong turns the railway's own line into a generic
distribution line.

### Distribution, 20 kV and 0.4 kV

Concrete, not wood. German medium-voltage lines run on spun-concrete poles of
10–14 m or on steel tube; wooden poles are the exception here, which is why
imported American or Scandinavian "utility pole" assets look wrong on a German
route the moment they are placed. Standing insulators are the rule, hanging ones
the exception, and in the north and east single insulators mounted alternately
on the pole replace the crossarm entirely.

The `Masttransformatorstation` — a transformer on a platform halfway up the pole
— is the piece of this level worth having. One per hamlet, and it says *rural
Germany* faster than any building.

### And the one that is not a power line

`fernmeldemast-bahn`, the lineside telegraph pole: creosoted wood every sixty
metres beside the track, two or three short crossarms, six to twenty white
porcelain double-bell insulators. It carried the block and telephone circuits
until cable and radio replaced it from the 1970s. It is in the atlas because it
occupies the same visual slot as a pole and because no epoch III route looks
right without it.

## The wires between them

Not this tool's — the conductors are geometry
([`content::power`](../../crates/content/src/power.rs)) and a shader
([`world_render::conductors`](../../crates/world-render/src/conductors.wgsl)) —
but they are the other half of what makes a line read as a line, and they are
solved the same way the mast's levels are: by asking how many pixels the thing
actually covers.

A conductor is **always** too thin to draw. A 380 kV bundle is 40 cm across and
a 110 kV single conductor 2 cm; at the kilometre and more a power line is looked
at, that is a fraction of a pixel, and a fraction of a pixel is not a thin
line — it is a line that hits some pixel centres and misses others, so it comes
out as a crawling dotted seam. No amount of geometry fixes it, because the
rasteriser only ever answers yes or no.

So the mesh carries the wire's **centre line** and its true width, and the
shader spreads it into a band that faces the camera and is never under a pixel
and a half wide — then hands back exactly what the widening took, as coverage.
The *ink* stays what the wire is worth: a 380 kV line at two kilometres becomes a
grey hairline you can lose against a bright sky, which is what it does in life,
instead of a black net drawn across it. Facing the camera also means it cannot
be seen edge-on, which is what the geometry used to spend a second crossed quad
on, so it is half the triangles as well.

## What the models are used by

**Placed by hand.** `mods/pylons/objects/*.ron` are ordinary track objects, so
the route editor's object tool and its content drawer find all thirty-three of
them under the tags `freileitung`, `hochspannung`, `bahnstrom`, `mittelspannung`
and the `epoch-*` tags.

**Placed by the OSM import.** A `power=line` or `power=minor_line` way becomes a
[`PowerLineSource`](../../crates/content/src/route.rs) in the line file:
`design=*`, `voltage=*` and `frequency=*` pick the type,
[`content::power::PRESETS`](../../crates/content/src/power.rs) stamps the mast
objects, the crossarm heights and the conductor positions into it, and the
terrain build puts a mast at every vertex with the conductors hanging between
them. Both ends of a way and every vertex the line turns more than fifteen
degrees at get the tension variant.

`PRESETS` is a copy of this atlas, and a copy drifts — so
`content::power::tests::the_table_matches_the_atlas` reads `pylons.json` and
fails the moment a crossarm here stops agreeing with the table there. **Re-run
the build and `cargo test -p content` together** after editing the catalogue;
changing a crossarm width without regenerating the models puts the conductors
beside the insulator strings instead of on them.

The editor's entry is **File ▸ Import overhead lines…**, the headless one is
`import-module --line <file.ron>` (turn it off with `--no-power`).

## Where the geometry comes from, and why none of it is downloaded

It is generated because **there is no correctly shaped German Donaumast,
Tonnenmast or Bahnstrommast available under a licence this project can use.**
That is the result of searching Sketchfab (through its API, every relevant
English and German term, with and without the CC0 filter), Poly Pizza, Poly
Haven, BlendSwap, OpenGameArt, BlenderKit, ambientCG and Quaternius. The German
terms — `Donaumast`, `Tonnenmast`, `Einebenenmast`, `Hochspannungsmast`,
`Gittermast`, `Bahnstrom` — return **zero** downloadable models on Sketchfab
under any licence.

What does exist is foreign shapes: British and Swedish supergrid towers, Korean
765 kV, American wooden utility poles. A few are CC0 and several more are CC BY,
and every one of them would need the crossarms rebuilt to be a German mast —
which is the whole model. Two findings are worth recording rather than
repeating the search:

- A set of nine lattice towers marked *Public Domain (CC0 1.0)* on Poly Pizza
  (by `iPoly3D`) is the closest thing to a usable family. It was **rejected**:
  the account is unclaimed and carries Poly Pizza's note that the profile "was
  migrated from Google Poly or other web sources", so the CC0 rests on the
  site's metadata and nothing else. Not a basis to build a mod on.
- A "400kV H-Frame Steel Supergrid Power Tower" on Sketchfab is tagged CC BY and
  says in its own description that it was *"made and ported from Brick Rigs"*.
  The uploader cannot license a commercial game's asset; the tag is worth
  nothing.

A lattice tower is a parametric truss, the atlas already holds the heights, the
crossarm widths and the conductor counts, and this repository generates its
trees, its plants and its signal parts the same way. Generating the masts is
cheaper than cleaning up a download would have been, and it is the only route
that gets a Donaumast and a Bahnstrommast whose dimensions match the line they
stand on.

## Sources

Mast types, arrangements and history:

- [Freileitungsmast](https://de.wikipedia.org/wiki/Freileitungsmast), Wikipedia — construction types, materials, functional roles, voltage levels, medium- and low-voltage practice, wooden pole dimensions.
- [Mastbild](https://de.wikipedia.org/wiki/Mastbild), Wikipedia — the silhouette taxonomy and what each arrangement costs in height or width.
- [Donaumast](https://de.wikipedia.org/wiki/Donaumast), Wikipedia — origin on the 1927 Regensburg–Kachlet line, west/east distribution, variants.
- [Tonnenmast](https://de.wikipedia.org/wiki/Tonnenmast), Wikipedia — three crossarms with the middle widest, distribution outside Germany.
- [Bahnstromleitung](https://de.wikipedia.org/wiki/Bahnstromleitung), Wikipedia — 110 kV as 2 × 55 kV at 16.7 Hz, four conductors, one crossarm from about 1927, two for substation feeds.

Dimensions:

- [Masttypen](https://www.nabu.de/downloads/Masttypen.pdf), NABU — the 380 kV height bands: Einebenenmast 37–47 m, Donaumast 55–65 m, Tonnenmast 66–76 m.
- [Freileitungen](https://www.netzausbau.de/SharedDocs/Downloads/DE/Infomaterial/BroschuereFreileitungen.pdf), Bundesnetzagentur — corridor widths and construction practice.
- [Freileitung](https://www.amprion.net/Übertragungsnetz/Technologie/Freileitung/), Amprion — 300–500 m span at transmission level.
- [Modern und naturverträglich – Neue Strommasten](https://blogs.nabu.de/modern-und-naturvertraeglich-neue-strommasten/), NABU — compact masts and bird protection.

Taxonomy:

- [Key:design](https://wiki.openstreetmap.org/wiki/Key:design), OpenStreetMap wiki — the `design=*` value set the atlas and the import key on.
