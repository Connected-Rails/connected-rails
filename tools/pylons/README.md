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

Eighteen types, thirty-three variants, four levels of detail each, and twelve
shared PBR maps. The construction surfaces are procedural; the printed warning
face is a dedicated raster source so its lettering and lightning symbol remain
stable through the mip chain. Geometry comes out of `lib/kit.mjs`, construction
materials out of `lib/texture.mjs`, and dimensions out of `pylons.json`.

## Running it

```bash
node tools/pylons/build_pylons.mjs
node tools/pylons/build_pylons.mjs --only donaumast-380,bahnstrommast-110
node tools/pylons/build_pylons.mjs --check      # every face outward? writes nothing
node tools/pylons/build_pylons.mjs --report     # triangle budget, writes nothing
node tools/pylons/build_pylons.mjs --preview    # picture sheets into /tmp/pylon-preview
node tools/pylons/build_pylons.mjs --ink        # what each level does to the mast's ink
```

Node 20 or newer and nothing else — no npm dependency of this repository, no
modelling tool, no image library. The glTF writer is `lib/gltf.mjs`, the
geometry primitives are `lib/geom.mjs`, and the whole run takes under a second.
`--preview` borrows the PNG encoder from `tools/trees/lib/png.mjs` — the only
thing this tool takes from another, and it never ships. A full texture and
model rebuild takes a few seconds on a development machine.

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

`lib/kit.mjs` assembles the recurring pieces of a German overhead line support
in type-specific proportions:

| piece | what it is |
| --- | --- |
| **body** | four battered legs with X bracing: a steep lower batter changes at a knee into a shallow taper to the head (`lattice`); or a tapered tube (`pole`, `tube`); or two legs under a beam (`portal`) |
| **crossarm** | two parallel side trusses tied into a torsionally stiff arm; the lower chord is horizontal at conductor level and the upper chord slopes down from its deep body joint to the tip |
| **fitting** | a cap-and-pin string, a grey composite long rod, a standing medium-voltage pin, a low-voltage porcelain spool, or the double-bell porcelain of a telegraph pole |
| **tube** | anything genuinely round: a concrete or wooden pole, the body of a compact mast, an insulator shed |
| **earth peak** | a small pyramid of the same lattice on the body top; one, two or none |
| **foundation** | four concrete stubs — the part of a mast actually seen from a train, because the grass grows up to them |
| **detail** | LOD0 angle sections, gusset plates and bolt heads; climbing irons; a rail-mounted W012 warning/legend plate and readable mast-number field where the prototype carries them |

A Donaumast and a Bahnstrommast differ in that file by **nothing at all**. They
differ in the catalogue by a crossarm.

### Variants

Two per type, from the `roles` the atlas gives it: `_trag` (Tragmast,
suspension) and `_abspann` (Abspannmast, tension). The tension mast is stouter —
a wider base and heavier members — and its insulator strings lie along the
conductors instead of hanging under them, which is what a viewer actually reads
an Abspannmast off from a moving train.

**Every face has to point outward, and for the whole life of the kit they did
not.** `member` — the box that four hundred of a lattice mast are made of — was
wound inside out on all four sides and both caps. Nothing caught it, because the
*silhouette* is the same either way: with a single-sided material the near face
of a bar is culled and what the eye gets is the inside of the far one, so a mast
looks right in a picture and is wrong by the thickness of a bar in depth and lit
from the wrong side. In a lattice, where four hundred members cross, that reads
as bars being in front of others they are behind. `--check` asserts it now, per
piece, over every level of every variant — a shape whose normals point at its
own centre fails the build.

**Flat normals everywhere straight, smooth ones round a `tube`.** Angle steel
has edges and smoothing them costs vertices to hide the one thing that makes a
lattice read as steel — but a round section is the opposite case, and the same
rule applied to it gave the compact mast a body that read as folded sheet: a
ten-sided prism with seventy-centimetre faces, each its own flat grey. The
corner normals of a `tube` are now the cone's true normals, so ten sides read as
round and the silhouette is out by five centimetres on a two-metre tube. The
concrete and wooden poles were faceted the same way and are fixed with it.

**Height is not a variant.** The atlas gives a band, the placement gives a
scale, and a mast scaled by a tenth either way is the same drawing built taller
— which is what a manufacturer does too. Two files per type instead of eight,
and a line of Donaumasten still varies along its length.

### Levels of detail

| level | what it is | 380 kV Donaumast |
| --- | --- | --- |
| `mast_LOD0` | L-angle members with **closed ends**, every insulator cap, gussets, bolts, signs and foundations | 15 942 triangles |
| `mast_LOD1` | half the bracing panels, box substitutes for sub-pixel angles, open ends, a third of the caps | 2 680 |
| `mast_LOD2` | a quarter of the panels, one truss per crossarm instead of two, insulators without caps | 472 |
| `mast_LOD3` | the outline: legs, arm bars, a stub where each insulator was | 240 |

A tension mast is heavier again — 22 780 at `LOD0` for the same type — because it
carries two strings at every conductor point instead of one.

**The ends are closed at `LOD0` and open below it.** A member is an open box by
default and every one of the 276 pieces of a Donaumast was left that way; while
the sides were wound inside out it never showed, because the interior of the far
face stood in for the exterior of the near one. Closing the winding opened every
end at once and a joint became a bar you could see down. Two quads an end is
four triangles and 276 of them is half the structure again — worth it at `LOD0`
and nowhere else, because `LOD1` starts at 278 m on a Donaumast, where a 15 cm
bar is half a pixel and the hole in it a good deal less.

**And a member runs half its own width past both nodes.** Drawn node to node,
two bars meeting at an angle leave a wedge between their square ends — shallow
at a right angle, deep at the twenty degrees a diagonal makes with a chord.
Riveted steel does not have those; the bars lap and are bolted through. The
overlap costs no triangles and is capped at a fifth of the length so a short
stub cannot double.

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
| Donaumast 380 kV | 278 m | 617 m | 1 825 m | 4 000 m |
| Kompaktmast 380 kV | 869 m | 1 928 m | 3 314 m | 4 000 m |
| Bahnstrommast 110 kV | 156 m | 347 m | 1 564 m | 4 000 m |
| Betonmast 20 kV | 156 m | 347 m | 603 m | 2 086 m |
| Streckenfernmeldemast | 122 m | 270 m | 482 m | 1 304 m |

**A coarse level is drawn thicker than the fine one it replaces.** Dropping half
the bracing does not just cost triangles, it costs *ink*: a lattice at six
hundred metres is a grey haze made of a hundred sub-pixel bars, and a level that
throws half of them away and draws the rest at their true width halves the grey.
The mast then visibly darkens as the train approaches, which is the one thing a
level of detail must not do. So `lib/kit.mjs` widens what is left
(`MEMBER_SCALE`, `ARM_SCALE`), and **`--ink` is what says by how much**: it
supersamples each level at the distance it hands over at and reports the
coverage against the level before it. Most hand-overs are inside ±10 %.

**Every drawn hand-over stays inside ±20 %.** `--ink` enforces that limit and
fails the command if a generator change crosses it; 44 of the 54 transitions
are inside ±10 %, and the worst is 19 %. The remaining correction is split
between `MEMBER_SCALE` and `ARM_SCALE`, because legs and crossarms shed
different geometry. The LOD3 suspension stub keeps the real rod diameter rather
than scaling with string length — the old rule made a 3.4 m EHV string a 34 cm
beam — and the Masttransformator keeps its transformer at every level. That box
is the object's identity and a quarter of its ink, not expendable detail.

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
is what stops a tiled material from reading as wallpaper. `v` runs **along** a
piece and `u` across or around it, on a `member` as on a `tube`, so a material
can draw something that follows the steel. Nine tiling PNGs of 1024 px serve all
thirty-three models. The non-tiling warning face adds a colour, ORM and normal
map; its text therefore filters as one coherent printed surface at a distance.
The construction maps are generated (`lib/texture.mjs`) rather than scanned:

| material | what is painted |
| --- | --- |
| **verzinkt** | a weathered, matte zinc-carbonate skin; 10 mm residual **spangle** only as a subtle roughness change, plus fine rolling marks, orange peel and pitting — never embossed crystal facets |
| **beton** | 5–25 mm aggregate showing through a centrifuged pole's skin and the blow-holes beside it, the mould's seam running down it, and the slow grey-green of a surface that is wet more often than dry |
| **holz** | fine irregular longitudinal grain, long drying **checks** and explicitly placed 5–10 cm **knots**, under dark brown creosote |

**The tile has to be seamless, and for a long time it was not.** `fbm` took the
frequency and the wrap period as two separate arguments, and of the twelve terms
the three materials are made of, exactly one happened to match — the other
eleven broke at the tile boundary, so every surface carried a **grid of seams a
metre apart**. A lattice hid it, because no member is a metre wide in both
directions; the compact mast's two-metre tube showed it as a brick wall. The
period is derived from the frequency now, per axis, so it cannot come apart
again.

**Nothing is drawn finer than the map can hold, and nothing finer than the eye
can keep**, which is a budget: the highest octave of any term stays at or below
a quarter of `TEXTURE_SIZE` in cycles per metre — four texels a cycle. Both
halves of that rule were broken and each broke a material in its own direction.
The concrete drew its sand at 120 cycles a metre over three octaves, so the top
octave was half a texel wide and the map filled with the aliasing of it: a pole
read as **sandpaper** close up. The wood drew its fibre the same way and lost it
the other way round — the detail was so fine that the first mip level averaged
it flat, and a creosoted pole ten metres off was a **smooth pink stick**. What
carries a material at the distance it is actually seen from is centimetres, not
millimetres: the checks, the knots, the mould streaks, the aggregate.

Three numbers in `galvanised` decide whether a mast reads as steel, and all
three were wrong to begin with:

**Weathered zinc is not a mirror.** Fresh galvanising is a metal with a
reflectance near 0.85, and a `metallic` near one over a bright base colour gives
a mast that reflects the whole sky — under the image-based light Bevy's
atmosphere casts back (`world_render::sky`) that is not a subtle error, it is a
blue-white mast blowing out against the very sky it is mirroring. What a mast on
a line actually wears is zinc *carbonate*, the chalky grey skin of its first
years, and that is a **dielectric** over the metal. So the weathering field
drives the metallic into a narrow band low down — about `0.055` where the skin
is thickest to `0.20` on a facet still bright — and the base colour sits at the
medium grey a mast is (sRGB 82–101), not at the 169 it started on. What is left
is a broad dim sheen rather than a reflection, which is what galvanising has.

**An old coating is matte grey, not a fresh spangle sample.** At the 11 cells
per metre the spangle started on, a facet was wider than most braces on the
mast: a bar carried one or two of them, so the spangle stopped being a surface
finish and became the shape of the bar. Even 24 mm cells with seven per cent
colour contrast remained a polygonal camouflage in a close camera. The final
map keeps 10 mm residual cells at below one per cent colour contrast, mainly in
roughness; they disappear within a few metres as real weathered spangle does.

**And it is flat.** The spangle is a pattern in *reflectance*, not in relief —
the crystals freeze level with one another. Putting them in the height field,
where they were at four fifths of it, embossed every crystal and turned a bar of
angle iron into beaten metal at the first raking light. What relief the surface
does have is the orange peel a dipped coating freezes into, the rolling marks
and the pitting, all of it under a millimetre.

On top of the maps every vertex carries a **colour** (`geom.mjs`, `weather`):
per-member shade, because four hundred bars galvanised in the same bath still
come out a few per cent apart, and the dirt that gathers towards a mast's foot —
above about six metres a mast is clean, at the concrete it is brown-grey and a
fifth darker. That belongs in the vertex colour rather than the texture because
it is a property of the *mast*: the texture tiles every metre and knows nothing
about which end of the leg it is on.

**Material follows the part, not the object.** Structure, fitting, concrete
foundation, galvanised hardware, printed warning face and black cable/marking have separate
PBR materials. That keeps a concrete pole's crossarm metallic, a lattice mast's
foundation dielectric and a wooden pole's climbing irons zinc-coated. Every
exported material carries explicit glTF base colour, metallic and roughness;
`--check` audits those fields and all twelve referenced 1024 px maps in the
generated files, in addition to checking the faces.

**A cap is 146 mm**, which is the standard cap-and-pin unit, and the number of
them is what the voltage decides: about 9 at 110 kV, 15 at 220 kV, 23 at 380 kV.
The catalogue gave *every* type the same 3.4 m and 16 caps — a 213 mm pitch,
half again what a cap is, so each had to be drawn fat to close the gap and a
string read as a screw thread; and a 110 kV mast wore the insulation of a
380 kV one. `insulator_m` and `insulator_discs` now follow the voltage, and
`LOD0` draws every cap rather than a fixed five-eighths of them.

**The brown was written as if it were sRGB.** `baseColorFactor` is linear:
0.40, 0.30, 0.24 is sRGB 168, a milky cream, and under the sky's image-based
light at a roughness of 0.24 the strings came out pale pink. Fired brown glaze
is sRGB 94, 58, 44. The atlas's `finish.insulators` is the reference — porcelain
brown or glass green on old lines, grey silicone composite on anything after
about 1995. The epoch-VI compact mast now uses its own four-metre composite
long-rod geometry: one narrow core under 64 alternating silicone sheds at about
62 mm pitch, rather than a cap chain recoloured grey. Its matte material is
separate from glazed porcelain, and the close levels carry the field-grading
ring required on EHV composite strings.

**Where the insulators go** (`conductorPoints`) had two faults, and both showed
on the small masts, because that is where an arm is short. The inner limit was a
fixed 90 cm, and a low-voltage pole's crossarm is 1.6 m across — so both points
on a side were pushed to the same place and the four conductors of a village
line came out as two insulators. And an odd count was rounded up: three
conductors on one level, the commonest medium-voltage arrangement in Germany,
was drawn as four. The limit now scales with the arm and the odd conductor
stands over the pole, which is where it stands on the prototype.

The fitting materials stay constants: glazed porcelain and matte silicone are
smooth enough that there is no microstructure worth a map and small enough that
there would be no room for one to show. `insulator_m` follows the **fitting**,
not the body — a 20 kV lattice pole carries the same 35 cm pin insulator a
concrete pole does, and keying it on the body kind gave it the 3.4 m suspension
string of a transmission tower, which is a metre and a half of porcelain
hanging off a 15 m mast.

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
| **Donaumast** 380 kV | `donau` | 53.5–56.5 m | 2 (22 m over 32 m) | nationwide, the default |
| **Einebenenmast** 380 kV | `one-level` | 37–47 m | 1 long | eastern states |
| **Tonnenmast** 380 kV | `barrel` | 66–76 m | 3, middle widest | rare in DE; UK/CH/PL norm |
| **Donaumast** 220 kV | `donau` | 40–50 m | 2 | the ageing backbone |
| **Tannenbaummast** 220 kV | `three-level` | 43 m reference type | 3, widening downwards | 1920s–30s survivors |
| **Donaumast** 110 kV | `donau` | 25–35 m | 2 | old federal states |
| **Einebenenmast** 110 kV | `one-level` | 22–30 m | 1 | new federal states |
| **Kombinationsmast** 380/110 | `donau` | 60–70 m | 3 | Donau crown plus a wide 110 kV level |
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
on one crossarm** — never three or six — on a lattice mast of about 31 m at
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

- [Mastprinzipzeichnungen, Vorhaben 10](https://www.netzausbau.de/SharedDocs/Downloads/DE/Vorhaben/10/B/21/Planaenderung/V10_B_Unterlage4_Mastprinzipzeichnungen-Plan.pdf?__blob=publicationFile), TenneT/Bundesnetzagentur — dimensioned German Donau, Tonne and one-level series: arm half-widths, vertical clearances, truss depths and earth peaks.
- [Technischer Vergleich der Mastbilder](https://www.netzausbau.de/SharedDocs/Downloads/DE/Veranstaltungen/2015/Infotage/Muenchen_Forum_Technik.pdf?__blob=publicationFile), Bundesnetzagentur — the 380 kV one-level/Donau/barrel conductor segments and comparative foot widths.
- [Schemazeichnungen Osterath–Angerland](https://www.amprion.net/Netzausbau/Unsere-Projekte/Anschluss-Umspannanlage-Gellep/Downloads.html), Amprion — D46 380 kV Donau support at 53.5–56.5 m, ABD47 combined support and AB63 110/220 kV three-level support used as the body and arm-proportion references.
- [Grundlagen der Bahnstromversorgung](https://www.deutschebahn.com/resource/blob/13317200/8fb73bf6992ea00981c121ca5d14cb2a/05-11-2024-Einfu-hrung-Beeinflussungsberechnung-data.pdf), DB Systemtechnik/DB Energie, p. 6 — TW 31 traction-power support, 31 m mast and the 4.8/3.8/3.8/4.8 m conductor raster.
- [Freileitungsmast](https://de.wikipedia.org/wiki/Freileitungsmast), Wikipedia — construction types, materials, functional roles, voltage levels, medium- and low-voltage practice, wooden pole dimensions.
- [Mastbild](https://de.wikipedia.org/wiki/Mastbild), Wikipedia — the silhouette taxonomy and what each arrangement costs in height or width.
- [Donaumast](https://de.wikipedia.org/wiki/Donaumast), Wikipedia — origin on the 1927 Regensburg–Kachlet line, west/east distribution, variants.
- [Tonnenmast](https://de.wikipedia.org/wiki/Tonnenmast), Wikipedia — three crossarms with the middle widest, distribution outside Germany.
- [Bahnstromleitung](https://de.wikipedia.org/wiki/Bahnstromleitung), Wikipedia — 110 kV as 2 × 55 kV at 16.7 Hz, four conductors, one crossarm from about 1927, two for substation feeds.

Dimensions:

- [Netzverstärkung Region Rostock, Scoping-Unterlage](https://www.50hertz.com/Portals/1/Dokumente/Netz/Netzverstaerkung_Region_Rostock/220407_NRR_Scoping_Text_final.pdf?ver=WYzff3MqgfI3hogZmNSfAQ%3D%3D), 50Hertz — 49.7 m D76 Donau and 32.5 m D82 one-level examples, including 32/45.2 m outer-phase widths and 6.8/7.8 m foot widths.
- [Freileitungen](https://www.netzausbau.de/N2000/DE/Technik/Freileitungen/freileitungen-node.html), Bundesnetzagentur — usual German 380 kV heights (about 40 m one-level, 54 m Donau, 61 m barrel), phase clearances and construction practice.
- [Masttypen](https://www.nabu.de/downloads/Masttypen.pdf), NABU — wider project-dependent height bands for the three 380 kV silhouettes.
- [Freileitungen](https://www.netzausbau.de/SharedDocs/Downloads/DE/Infomaterial/BroschuereFreileitungen.pdf), Bundesnetzagentur — corridor widths and construction practice.
- [Freileitung](https://www.amprion.net/Übertragungsnetz/Technologie/Freileitung/), Amprion — 300–500 m span at transmission level.
- [Modern und naturverträglich – Neue Strommasten](https://blogs.nabu.de/modern-und-naturvertraeglich-neue-strommasten/), NABU — compact masts and bird protection.
- [Design trifft Effizienz: ästhetische Freileitungsisolatoren für dänische Hochspannungsmasten](https://www.pfisterer.com/de/referenz/design-trifft-effizienz-aesthetische-freileitungsisolatoren-fuer-daenische), PFISTERER — grey silicone long rods over 4 m on 420 kV compact-line pylons; continuous pieces can be made up to 7 m.
- [S1-VX Series IEC Design Test Report N337](https://www.macleanpower.com/wp-content/uploads/MPS_S1-VX-Series-IEC-Design_Test-Report-N337.pdf), MacLean Power Systems — 51 mm composite-shed spacing and corona rings recommended from 230 kV.
- [Warnhinweis an einem RWE-Freileitungsmast](https://commons.wikimedia.org/wiki/File:Warnhinweis_Vorsicht_Hochspannung.JPG), Wikimedia Commons — aluminium carrier, rounded W012 triangle, two-line `Hochspannung / Lebensgefahr` legend, separate mast number and the visible fixing pattern used by the LOD0 sign.

Materials:

- [Feuerverzinkte Fassaden](https://www.feuerverzinken.com/fileadmin/Uploads_Glinde/Anwendung_Fassaden/Special_Feuerverzinkte_Fassaden.pdf), Institut Feuerverzinken — zinc's initially metallic surface changes to a matt grey patina and can vary with exposure and steel chemistry.
- [Atmospheric corrosion](https://galvanizing.org.uk/atmospheric-corrosion/), Galvanizers Association — zinc-carbonate patina formation and long-term atmospheric weathering of hot-dip galvanising.

Taxonomy:

- [Key:design](https://wiki.openstreetmap.org/wiki/Key:design), OpenStreetMap wiki — the `design=*` value set the atlas and the import key on.
