# Wind: the catalogue of German wind turbines, and the kit that builds them

A German railway line runs past wind turbines the way it runs past power
lines: in the Börde and on the coast there are dozens on every horizon, and a
landscape without them is a landscape of the 1980s. This directory is the
catalogue of what stands there and the generator that turns it into models:

```
wind.json ──build_wind.mjs──▶ mods/wind/assets/<class>_<build>.gltf
          │      (lib/kit)                    + <class>.bin
          └─────────────────▶ mods/wind/objects/<class>_<build>.ron
                              mods/wind/mod.ron
```

Four classes, ten builds, four levels of detail each, three shared PBR maps.
Geometry comes out of `lib/kit.mjs`, the coating out of `lib/texture.mjs`,
the concrete and the zinc out of the masts' painters, and dimensions out of
`wind.json`. The glTF writer, the geometry primitives and the rasteriser are
`tools/pylons/lib`'s — a turbine is built the way a mast is, and the two tools
share what they share.

## Running it

```bash
node tools/wind/build_wind.mjs
node tools/wind/build_wind.mjs --only wea-80
node tools/wind/build_wind.mjs --check      # every face outward, every node there? writes nothing
node tools/wind/build_wind.mjs --report     # triangle budget and LOD bands, writes nothing
node tools/wind/build_wind.mjs --preview    # picture sheets into /tmp/wind-preview
```

Node 20 or newer and nothing else. `--preview` writes one sheet per build
(front and side at the finest level, then the coarser levels), a `-kopf.png`
close-up of nacelle, spinner and blade roots, a `-handover.png` per class with
each pair of levels drawn at the distance it hands over at, and `alle.png` with
every build side by side at one scale. **Look at it after editing the
catalogue** — a blade whose widest chord sits at the wrong place passes every
check in this pipeline and is wrong at a glance.

## What a turbine is

Five things, and only the tower differs much between makers:

| piece | what it is |
| --- | --- |
| **tower** | a tapered steel tube on nearly every machine since 2000 — with the flanges where its sections were bolted, a door at the foot and, on the tall ones, the red band of the day marking; a four-legged lattice on some of the 1990s machines |
| **nacelle** | the box that holds the drive train, on a yaw bearing over the tower: a rounded box on a Vestas, a Nordex, a Senvion or a GE, Enercon's drop-shaped shell on an Enercon; the anemometer mast and the lamps sit on its tail |
| **rotor** | three blades on a hub with a spinner over it, the axis tilted up five degrees, the blades coned three degrees upwind so they clear the tower when they bend |
| **foundation** | a concrete disc the grass grows up to; four stubs under a lattice |
| **marking** | on anything whose tip reaches over a hundred metres (AVV Kennzeichnung): three six-metre bands — red, white, red — from each blade tip inwards, a three-metre red band round the tower at forty metres, and two red lamps on the nacelle by night |

### The blade

The blade is a loft through aerofoil sections: a NACA four-digit thickness
distribution with a little camber, round at the root where the bolt circle is,
widest at a quarter of the length, tapering to the tip, and twisted eighteen
degrees at the root down to nothing at the tip so the leading edge faces the
wind it actually meets. The pitch axis is at thirty per cent of the chord,
which is where a blade's own axis runs, so a twisted section turns about the
right line. Twenty points round a section and nineteen stations along it at
the finest level; four points — a diamond, which still has a silhouette — and
three stations at the coarsest.

The day marking's bands end **on a ring**: the stations are placed at the band
edges, so a band stops where the regulation says rather than fading across a
face. The leading edge of the outer half is a shade darker and matte — it is
sand-blasted by a hundred kilometres an hour of rain until it is.

### Levels of detail

What stops resolving first on a turbine is the blade: a section is a line
once its chord is a couple of pixels, and past that a level with an aerofoil
in it is triangles for nothing. So the bands come off the outer chord, the
masts' rule applied to the member a turbine actually has:

| level | what it is | 2 MW class |
| --- | --- | --- |
| `_LOD0` | twenty-point sections, flanges, door, louvres, anemometer, two lamps | 4 436 triangles |
| `_LOD1` | twelve-point sections, half the stations, plain box | 1 724 |
| `_LOD2` | eight-point sections, ten-sided tower, one big lamp | 698 |
| `_LOD3` | diamond blades, six-sided tower, no spinner | 344 |

- **LOD0 → LOD1** where the outer chord is three pixels — the section's shape
  has gone, only its width is left.
- **LOD1 → LOD2** where it is a pixel and a quarter — now it is a line.
- **LOD2 → LOD3** where the tower's foot is under two pixels: there is no
  room for a taper, and a stick with a diamond on it is the honest picture.
- **Culled** at eight kilometres. A 200 m machine is still forty pixels tall
  there and its lamp is what a night landscape *is*; in practice the terrain
  decides, because a tile that is not built carries no turbine.

The lamp grows with the level — 35 cm at `LOD0`, 1.6 m at `LOD3` — so a
light that is a fraction of a pixel across at three kilometres survives as a
pixel of red rather than being sampled away. The reference is the masts':
1440 lines at the simulator's 45° vertical field of view.

| class | LOD1 from | LOD2 | LOD3 | culled |
| --- | --- | --- | --- | --- |
| 600 kW, 50 m rotor | 464 m | 1 112 m | 2 781 m | 8 000 m |
| 2 MW, 80 m | 637 m | 1 530 m | 3 650 m | 8 000 m |
| 3 MW, 115 m | 811 m | 1 947 m | 4 519 m | 8 000 m |
| 5 MW, 150 m | 950 m | 2 281 m | 5 389 m | 8 000 m |

### The moving parts

A turbine is the first scenery object in the game with **nodes that move**,
and the model is built for it:

```
turm_LOD0 … turm_LOD3                 the tower and the foundation
nacelle                               at hub height; yaws about Y
├─ gondel_LOD0 … gondel_LOD3
├─ feuer_NIGHT                        the lamps, off by day (the `_NIGHT` convention)
│  └─ blink                           switched on the clock: 1 s on, ½ s off
│     └─ lampe_LOD0 … lampe_LOD3
└─ rotor                              at the hub, in front; turns about its own Z
   └─ rotor_LOD0 … rotor_LOD3
```

The rotor node carries the machine in its glTF `extras` —
`{"rotor_diameter": 80, "rated_rpm": 18, "hub_height": 95}` — and
`world_render::wind` reads them off the node and turns the rotor at the speed
the wind at hub height gives it, yaws the nacelle to where the wind comes
from, and blinks the lamps on the scenario clock. Nothing about the movement
is in the line file and nothing is sent over the network: the weather is a
shared function of the clock, so every client sees the same park turning the
same way.

The rotor turns **clockwise seen from the wind**, which is how every
three-blade machine turns, and the leading edge of the blade at twelve o'clock
faces the direction it moves in. Both are the same choice, and a model that
got one without the other would be a rotor running backwards.

### The materials

One tiling map carries almost the whole machine: the **coating** — a two-pack
polyurethane on the steel tower, a gelcoat on the glass fibre of the nacelle
and the blades. Both are RAL 7035 light grey, and the map keeps it there: a
turbine that reads as mid-grey has been painted wrong, and a machine on a
bright day comes close to blowing out against the sky behind it because that
is what it does. What is painted on top of it lives in the roughness far more
than in the colour: the orange peel a sprayed coating freezes into, the rain
streaks down the tower — long along `v`, which runs *up* a tower and *out*
along a blade — and the chalking of a gloss going dull in the sun. Metallic is
zero throughout; paint over steel is a dielectric.

The concrete of the foundation and the zinc of a lattice tower are the masts'
own painters, and the lamp is a constant: Feuer W, rot, with an emissive
strength that survives the physical exposure of a dusk sky and blooms into
the light a viewer sees from kilometres away.

The things that are a property of the *machine* rather than of the coating go
into the vertex colour, the same split the masts make: the dirt at the tower
foot, the red marking bands, the darkened leading edge, and Enercon's seven
green rings shading up from the ground into the tower's own grey.

## The catalogue

`wind.json` is one entry per size class — the German fleet's own generations,
with the dimensions the Marktstammdatenregister says the fleet has (medians
over three regions, see `crates/content/src/wind.rs`) and the proportions the
makers' data sheets give the machines of that generation:

| class | generation | hub | rotor | builds |
| --- | --- | --- | --- | --- |
| **wea-50** | 1990s — Enercon E-40, Vestas V44, Tacke TW 600 | 65 m | 50 m | standard, enercon, gitter |
| **wea-80** | 2000s — Vestas V80, Enercon E-70, REpower MM82 | 95 m | 80 m | standard, enercon, gitter |
| **wea-115** | 2010s — Enercon E-101, Vestas V112, Nordex N117 | 125 m | 115 m | standard, enercon |
| **wea-150** | 2020s — Enercon E-138, Vestas V150, Nordex N149 | 140 m | 150 m | standard, enercon |

**Size is not a build.** The class gives the model's dimensions, the placement
gives a scale, and a machine scaled by a tenth either way is the same drawing
built taller — which is what a manufacturer does too. The import
(`content::wind`) picks the class by the rotor and the build by the maker, and
scales the model by the geometric mean of the two ratios, so a 101 m rotor on
a 140 m tower misses both by the same share instead of one exactly and the
other badly.

**The builds** are what a viewer names from a train: Enercon's drop-shaped
nacelle and green tower foot, a lattice tower under a Fuhrländer of the
nineties, and the box nacelle everyone else builds. Vestas, Nordex, Senvion,
GE and Siemens differ in details no train ever gets close enough to see.
