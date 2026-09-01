# Cars: the road vehicles beside the track

Every German station has a car park, every goods shed has a lorry at it, and
the imagery detection (`crates/vision`) needs something to place when it finds
one. This directory turns generated vehicle models into that content:

```
~/Downloads/*.zip ──import_cars.mjs──▶ cache/cars/manual/src-<id>/    (FBX)
                  │                    cache/cars/tex/<id>-read.png   (atlas)
                  └──build_cars.mjs──▶ mods/cars/assets/<id>.{gltf,bin,dds}
cars.json ───────────────────────────▶ mods/cars/objects/<id>.ron
                                       mods/cars/mod.ron
```

```bash
node tools/cars/import_cars.mjs           # once per batch: the zips out of ~/Downloads
node tools/cars/build_cars.mjs
node tools/cars/build_cars.mjs --report   # the budget, writes nothing
node tools/cars/build_cars.mjs --preview  # picture sheets into /tmp/car-preview
node tools/cars/build_cars.mjs --only kompaktwagen
node tools/cars/inspect_pack.mjs <file>   # what is inside a model, before cataloguing it
node tools/cars/preview_mod.mjs           # what the built mod looks like, atlas and all
```

Node 20 or newer, plus **ImageMagick** — the atlases arrive as JPEG and Node
has no JPEG decoder, and the shipped atlas has to be block-compressed with a
mip chain. Everything else is in `lib/`: an FBX reader, a PNG reader, the
cleaner, the atlas packer, the visibility test and the decimator.

Seven vehicles, four levels of detail each, about 10 000 triangles at the
finest and 14 MB of mod. Five have their windows replaced with glass; two keep
the ones baked into their atlas, because on those it is the better picture. An eighth is catalogued and skipped: its archive came
without a colour atlas, and the build refuses to ship a white shell with the
glass guessed from shape alone.

## Where the models come from

Generated with a 3D tool from photographs and exported as FBX — a Golf, a Polo,
a Tipo, a Kamiq, a Kuga, a Caddy, a Transporter and a Sprinter, which between
them is what stands in a German station car park. **A vehicle without its atlas
is skipped, not degraded**: next to seven textured cars, a white shell reads as
broken, because it is. They are the project's own
assets, so nothing here is anybody else's licence.

Drop the archives in `~/Downloads` and run `import_cars.mjs`. It unpacks them
into `cache/cars/` (git-ignored — five megabytes of source per car has no
business in the history) and writes the atlas out as a PNG at the size it will
ship at. The build both reads that PNG — to ask what colour a face is — and
packs it, because packing it needs the mesh.

## What arrives, and why it cannot ship as it is

One nameless mesh apiece, one baked atlas, eleven thousand triangles, and
everything the generator thought it saw. There are no object names to go by, so
every rule below is a rule about geometry.

### 1. The plinth comes off

The generator stands its subject on a slab. Left in, every car in the module
floats a few centimetres above the tarmac and drags a grey rectangle around
with it. Found as horizontal faces in the bottom two per cent of the model —
which is *below the tyres*, because a plinth is what the tyres rest on, while a
car's own floor pan sits a hand's width up. Three of the eight had one, one of
them nearly a fifth of the model's surface.

### 2. The winding is put right

Three per cent of the triangles in these models arrive wound inside out. With a
single-sided material — which is what a car wants, since nothing should ever
see the inside of one — each of those is a hole, and through the hole you see
the far side of the shell lit from behind. On screen that reads as a shard of
the wrong shade lying on the paint.

The test costs nothing extra, because the visibility pass below has already
looked at the model from every direction there is: a face that was *seen from*
somewhere has its outward side facing that somewhere. Where it does not, two of
its corners are swapped. Between forty and a hundred and eighty faces per
vehicle.

**The file's own normals are not read at all.** They disagree with the surface
they sit on for about one triangle in ten — nine per cent of the Polo's are
more than sixty degrees off, two per cent point the exact opposite way — and a
normal pointing into the panel it belongs to is a black patch on the paint
under any light. Every level's normals are computed here from the faces that
meet at each corner within the crease angle.

### 3. The interior goes

Seats, a floor, the inside of the shell: about a sixth of every mesh, never on
screen, and paid for on each of two hundred instances. Found by *looking*
(`lib/visible.mjs`): the mesh is rasterised from 128 directions with a depth
buffer that remembers which face won each pixel, and a face that never wins a
pixel from any of them is a face nobody will ever see. Nothing is looked at
from straight underneath — a car stands on the ground, and what only the
tarmac could see is as invisible as what is inside the boot.

### 4. The windows become glass

A baked window is a photograph of somebody else's street; what reads as glass
under the game's own light is a dark, smooth, untextured surface. The window
faces are the ones above the waistline, steep rather than sky-facing, part of a
large patch rather than a lone triangle — **and dark in the atlas**.

That last test is what makes it work, and it is asked of each vehicle
separately: the threshold between paint and glass comes from
[Otsu's method](https://en.wikipedia.org/wiki/Otsu%27s_method) over the
area-weighted brightness of that vehicle's own glasshouse. One atlas is a
silver car in daylight, the next a dark blue one in shade, and a fixed
threshold that suits the first calls the whole of the second a window.

**A face is asked about all of itself.** Four readings — the middle and the
three corners, pulled a little way in — instead of the middle alone. A triangle
laid across the edge of a window is dark at one corner and white at the other
two, and its centre will happily claim the whole of it is glass; if the four
readings disagree by more than half the distance between the two classes, it is
a triangle lying across the rubber and it stays with the paint.

**The colour that replaces a window is taken from the window.** A fixed
near-black was tried, and the edge of every window came out serrated: the
boundary between paint and glass does not run along triangle edges and never
will, so the classification is always a triangle out on one side or the other,
and between a nearly black flat surface and a mid-grey photograph of a
reflection that step is plainly visible. Matched to the photograph — pulled
down and towards neutral, because a window should be darker than what it
reflects — the step disappears and a misplaced triangle stops mattering.

**And it checks itself against the paint, not against the rest of the
glasshouse.** That distinction is the whole check. Asked whether what it picked
out is darker than everything else above the waistline, a black saloon answers
yes: it picked out the black boot lid, and the teal photograph of the windows
beside it is lighter. The substitution then lays glossy black over the bodywork
in a jagged band and leaves the windows as they were, which is exactly
backwards. Asked instead whether it is darker than the doors — paint on every
car there has ever been — the same saloon answers no and is left alone.

The line is four hundredths of a stop, which is the middle of a gap rather than
a tuned number: of these seven the two that come out wrong measure 0.025 and
0.023, and each ends up with one window black and the one beside it as it was
baked; the nearest one that comes out right measures 0.048.

Left alone is a perfectly good outcome, and two of the seven end there. These
windows are opaque baked photographs already; on a car whose paint is as dark
as its glass there is nothing to win and a band of glossy black to lose.
`glass: false` in the catalogue turns it off by hand.

### 5. Four levels of detail

A car park holds two hundred of these, and the mesh that is right at ten metres
is nonsense at three hundred. `lib/simplify.mjs` is Garland and Heckbert's
quadric error metric, carrying texture coordinates and treating three things as
borders it must not cross: the rim of an open shell, the seam where the texture
is cut open, and the line between paint and glass.

**Welding comes first, and it has to weld on the texture too.** Two corners at
the same point that read different parts of the atlas are not the same vertex.
Merging them by position — keeping one of the two coordinates and letting every
triangle that used the other one read a piece of some unrelated panel — is a
handful of triangles along a seam on a tidy atlas, and it is the whole car on
one of these: several hundred islands means nearly every shared corner
disagrees. Every level below the finest came out with the sheet smeared over
it, while LOD0, which is never welded, was perfect. That is worth knowing
because it looks like a *texture* fault and lives in the *geometry* code.

| level | keeps | reaches | triangles |
| --- | --- | --- | --- |
| 0 | everything | 25 m | ~10 000 |
| 1 | 40 % | 85 m | ~4 000 |
| 2 | 11 % | 240 m | ~1 100 |
| 3 | 3 % | 700 m | ~300 |

The queue is what makes this work, and it is easy to get wrong in a way that
looks like it works. A collapse re-prices every vertex around it, and a
re-priced vertex's entries in the heap are void. Re-offering only the surviving
vertex therefore drops every edge *between* its neighbours, for good: the queue
runs dry with thousands of sound collapses never reconsidered, and the level
stops wherever it happened to stall. Two of the four levels used to come out
byte for byte identical because both were stopped by that rather than by their
target — and the coarse ones were built from whatever collapses survived rather
than the cheapest ones, which is a car with slivers fanning across its roof.

## The size check

The scale comes from the **length alone** — one factor for all three axes,
because these are models of real vehicles and fitting each axis separately
would stretch away what makes them look like vehicles. That makes `width_m`,
`mirrors_m` and `height_m` in the catalogue a *prediction* rather than an
input, and the build measures what came out against them and warns past five
per cent:

```
! built 4.82 × 2.28 × 1.82 m, expected 4.82 × 1.90–2.30 × 1.99  height -9%
  — length_m 4.80 fits this model best
```

The width is checked against a **span**, from the body to the mirrors: whether
a bounding box has the wing mirrors in it depends on how the model happens to
be split, and both answers are right. The suggested length is the one that
minimises the relative error over all three dimensions at once — the number to
put in the catalogue when a model turns out to be a different size from the
vehicle it is named after.

Two things this check found, neither of which is visible by looking:

- **A speck sets the size.** A fleck of fifteen triangles floating above the
  roof — two hundredths of one per cent of the surface — made one car nineteen
  per cent too tall in every number derived from its bounding box. So the size
  is taken from the parts that *are* the car (`bodyBounds`, islands over one
  per cent of the surface) and the litter is deleted separately.
- **Four of the eight models are a different size from their namesake.** Not
  distorted — their proportions are self-consistent — just smaller or larger,
  by three to eight per cent. Their catalogue lengths are the best fit rather
  than the manufacturer's figure, which is why a Kleinwagen here is 3.93 m and
  a Kastenwagen 6.30 m.

One vehicle carries an explicit `tolerance` because it genuinely deviates: the
Transporter comes out a tenth lower than a real T6.1 and no uniform scale can
fix that. An exception with a reason beside it, rather than a loosened rule —
a build that warns every time only teaches you to look away.

## The atlas: 1024², flooded, block-compressed, with mips

The mip chain is not an optimisation, it is the difference between a car park
and a field of crawling coloured noise. Bevy generates no mipmaps for a loaded
image, and an atlas full of tail lamps and badges minified ten to one picks a
different texel every frame. DDS is the format that carries a mip chain and
that ImageMagick can write, so `crates/world-render` enables Bevy's `dds`
feature and the atlases ship as BC1 with ten levels — 683 KB apiece, against
3 MB for the source JPEG.

**These atlases are not laid out like an atlas.** They are photogrammetry
output: several hundred small scraps of photograph — a wing here, half a door
there, a wheel — packed tight, with the space between them left a flat dark
blue. Two things follow, and both were learnt the hard way.

The first is that the resolution that matters is the resolution of *one scrap*,
not of the sheet. At 512² a van's door panel comes out sixty texels across,
which is visibly soft at the distance a car park is walked past at. A thousand
and twenty-four is four times the texels and 683 KB, which for seven vehicles
is under five megabytes.

The second is that **the dead space must not stay dead** (`lib/atlas.mjs`).
Nothing samples it deliberately, and everything samples it by accident:
downsampling averages across an island edge, each mip level does it again, and
a block-compressed 4×4 that straddles an edge spends two of its four colours on
the boundary. On screen that is a white van with dark angular shards over the
panels, following the island edges — it looks exactly like crumpled foil. So
the gaps are flooded with the colour of the nearest island before anything
else happens, and every average, mip and block from then on mixes paint with
paint.

Which part of the sheet is dead space cannot be decided by looking at it — a
tyre is as dark as the background. It is decided by the model: the mesh's own
texture coordinates are rasterised into a mask, and what no triangle covers is
what gets flooded. About a third of each sheet. That is why the atlas is
finished in `build_cars.mjs` and not in `import_cars.mjs`, which has never seen
the mesh.

The material dims the atlas to 82 %. These textures were baked *with the light
already in them*, and lit a second time by the game's own sun they come out
bleached — a car park of white blobs at noon.

## Looking at what came out

```bash
node tools/cars/build_cars.mjs --preview
```

Three sheets in `/tmp/car-preview`:

- `<id>.png` — every level in three views at one scale. For a glass
  classification gone wrong or a body fitted to the wrong dimensions.
- **`<id>-close.png`** — the finest level around the middle of the glasshouse,
  at a scale where the window rubber is a hundred pixels rather than one.
  Whether the dark glass stops at the rim or reaches a triangle's width out
  over the pillar is a decision about a few centimetres, and no sheet that fits
  a whole vehicle into 260 pixels can show it.
- **`<id>-handover.png`** — each pair of levels drawn at the metres per pixel
  of the distance the first hands over at, then magnified without filtering.
  **This is the sheet that decides whether a level is good enough**: if the two
  halves of a pair look alike, the switch is invisible in the game.
- `alle.png` — the whole fleet from above, which is how a car park is seen.

Those sheets are the geometry as it is about to be written. What is actually on
disk is a different question, and `preview_mod.mjs` answers it: it reads the
built glTF back, samples the block-compressed atlas beside it through the
model's own texture coordinates, and draws that. **If a car is broken in the
game, it is broken on that sheet** — which is how one vehicle shipping without
its texture was found, after screenshots in the editor had failed to frame it
three times running.

### Two ways this renderer used to lie

Both were found the same way: the sheets said the fleet was fine and the engine
showed it in pieces. They are written down because a check that cannot fail is
worse than no check, and both mistakes are easy to make again.

**The light was `|n·l|`.** An absolute value lights a face that has been turned
inside out exactly as brightly as one that has not, so a model with a tenth of
its normals wrong drew clean sheet after clean sheet. It is `max(0, n·l)` now,
and a face pointing away from the sun gets the ambient term and nothing else,
the way it would on screen.

**The texture was sampled once per vertex** and the three results blended
across the triangle. A graphics card interpolates the *coordinates* and samples
per pixel, which is not the same thing at all: a triangle whose corners land in
three different islands of the atlas is a gentle gradient one way and a slice
cut through half the sheet the other. Every level below the finest was reading
the wrong part of its atlas for weeks, and the sheets showed a tasteful grey
car. It samples per pixel now.

The lighting stays flat, one normal per triangle, on purpose — the facets of a
coarse level should be visible. The game interpolates them and looks smoother.

## Adding a vehicle

Drop its zip in `~/Downloads`, run `import_cars.mjs`, look at it, and add an
entry to `cars.json`:

```json
{
  "id": "kombi", "name_de": "Kombi", "source": "vw-passat",
  "tags": ["car", "pkw", "kombi"],
  "length_m": 4.77, "width_m": 1.83, "height_m": 1.48,
  "nose": "+z", "waist": 0.52
}
```

`source` is the directory `import_cars.mjs` made (`src-<source>`). The three
dimensions are the real ones — the models arrive normalised into a unit box, so
these are what gives them a size at all, and the imagery detection checks a
find against the size of the class it claims to be. `nose` says which way the
model faces before the build turns it (the build lays the vehicle along Z by
itself; one of these eight was exported facing along X). `waist` is where the
glasshouse starts, as a share of the height — a van's waistline sits much lower
down its body than a saloon's.

Two tags are instructions rather than descriptions: **`car` and `lorry` are
what the imagery detection places from**. Leave them off anything that should
only ever be placed by hand.

The three dimensions are written into the object file as a `footprint` as well,
and the detection measures a find against it before choosing. That is what
keeps the 6.30 m Kastenwagen out of the spaces the 4.82 m Transporter is meant
for — both answer to `lorry`, and before the sizes were stated the choice
between them was a coin toss.
