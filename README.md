# TrainSim-DE

German train simulator built on Bevy — implementation of [PLAN.md](PLAN.md).
Current state and open points: [STATUS.md](STATUS.md).

## Build and run

```bash
cargo test --workspace     # all acceptance tests (headless, no GPU)
cargo run -p app           # start the simulator
cargo run -p app -- --frames 120   # rendering smoke test (CI)
cargo run -p app -- --screenshot screenshots/hud.png   # capture an image and exit
```

`--screenshot` is available in the editor as well; `--frames N` sets after how many frames
the capture happens (60 frames ≈ 1 s of simulation time).

## Importing a line

Export track data from [Overpass Turbo](https://overpass-turbo.eu) as JSON:

```overpassql
[out:json];
way["railway"="rail"](50.90,10.00,51.00,10.30);
(._;>;);
out body;
```

**Taken from OSM** are the geometry of the `railway=rail` ways, `maxspeed` and `name`.
Switches, signals, platforms and level crossings are not carried over (yet) — the line is
created as a single strand and is then equipped in the RON file.

The point sequence does not become a smoothed curve but an **alignment**: straight sections
and curves are separated, the radius is averaged over the whole curve (point noise cancels
out with √n) and rounded to the nearest standard radius if it is close enough. Transition
curves and **cant** cannot be measured from OSM and therefore come from the rulebook:
`c = 11.8 · v²/R` minus the permitted cant deficiency, capped at 160 mm, ramp length 1:10·v.
The result is a chain of straight – clothoid – circular arc – clothoid – straight.

Limits worth knowing: OSM is accurate to ±2…5 m from aerial imagery, and the start and end
of a curve can only be determined to about ten metres from a point sequence. Radius, turn
angle and cant, on the other hand, are hit precisely — exactly the quantities you feel while
driving. The import report lists radii, cant and the deviation from the OSM line.

**Elevations** come from the state DGM data. `--dgm` takes a file *or an entire directory*
of tile sheets (subdirectories included):

```bash
cargo run -p content --bin import-line -- line.json --dgm ./dgm1_niedersachsen --epsg 25832 --name "Musterbahn" --out line.ron
```

Supported are XYZ (`x y z`, UTM) and ESRI ASCII Grid (`.asc`). Sheet boundaries are read from
the file name (`dgm1_32_389_5711_1_ni.xyz`), so nothing is loaded at startup; each tile only
enters memory once a query falls into it, and at most eight stay loaded at a time. This makes
even a DGM1 of an entire federal state (several thousand tiles) usable.

The tool reports length, edge count, elevation coverage and the largest deviation of the
alignment from the OSM points.

## Workspace

| Crate | Contents |
|---|---|
| `world-coords` | ECEF f64 world coordinates, floating origin, geodesy (plan ch. 4) |
| `track-model` | Track geometry (straight/curve/clothoid), topology, switches, lineside equipment (ch. 5) |
| `sim-core` | Driving dynamics, air brake, electrics, train protection, interlocking, timetable, scenario and scoring — **without Bevy**, deterministic (ch. 6–11) |
| `content` | Vehicle database, line source format (RON) + compiler, scenarios, OSM/DGM importer (ch. 15) |
| `ai-driver` | AI train driver, look-ahead (ch. 11) |
| `imagery` | Aerial imagery tiles: providers, Web Mercator maths, cache, fetching (ch. 15) |
| `app` | Bevy app: rendering, cameras, input, HUD (ch. 12) |
| `editor` | Line editor: top-down view with aerial imagery overlay (ch. 15) |

`sim-core` is a pure Rust library with a fixed time step (200 Hz). The Bevy app ticks it and
mirrors the state into ECS components — simulation logic does not belong there.

## Key bindings

| Key | Function |
|---|---|
| `W` / `S` | Power controller up/down (negative = electric brake), `X` = zero |
| `R` / `F` / `T` | Reverser forward / reverse / neutral |
| `A` / `D` | Driver's brake valve release / brake |
| `Q` / `E` / `Z` | Lap / emergency brake / fill |
| `C` / `V` | Direct brake apply / release |
| `G` | Sanding |
| `Space` | Sifa (driver's safety device) |
| `Page Down` / `End` / `Delete` | PZB acknowledge / release / override |
| `N` / `M` | LZB takeover / end |
| `H` | Horn |
| `1`–`4` | Battery / pantograph / main switch / air compressor |
| `F1`–`F3` | Camera: cab / external / lineside |
| Arrow keys | View direction, `Numpad +/-` camera distance |

## Example line

`content::musterbahn()` — 7 km: 3 km straight (160 km/h), 1 km curve R = 1200 m with cant
ramp (130 km/h), 3 km at 8 ‰ gradient. Block signal at km 2.0 with distant signal,
1000/500/2000 Hz magnets and LZB loop cable in the final section.

## Terrain

From the same DGM, `content::terrain` builds the terrain meshes — only within the corridor
around the line and at graded resolution:

| Distance from track | Grid spacing | Triangles per km² |
|---|---|---|
| up to 96 m | 4 m | 125,000 |
| up to 384 m | 8 m | 31,000 |
| up to 768 m | 16 m | 8,000 |
| beyond | 32 m | 2,000 |

For comparison: unmodified DGM1 would be 2,000,000 triangles per km². On top of that come
512 m tiles (one entity per tile → frustum culling, plus a view distance limit per LOD level),
skirts at the tile edges against cracks between levels, and a cutting/embankment profile that
pulls the terrain near the track up to rail level.

The app shows the terrain automatically (flat without DGM):

```bash
cargo run -p app -- --dgm ./dgm1_niedersachsen --epsg 25832
```

## Editor with aerial imagery overlay

```bash
cargo run -p editor                              # example line
cargo run -p editor -- line.ron --imagery my_imagery.ron
```

The overlay configuration (`imagery.ron`) is created on first start and is fully editable:
provider, opacity, zoom level or target resolution, load radius, tile limit, image offset
against the track position, overlay height, cache (location, budget, memory tiles, offline
mode, maximum age) and fetch behaviour (user agent, timeout, concurrency, retries). Changes
can be reloaded at runtime with F5 and written back with F2.

**Providers** are data, not a hard-wired list. Shipped are Esri World Imagery, BKG
TopPlusOpen, OpenStreetMap and a WMS template for the orthophotos of the state surveying
offices. Your own services are added as an entry — either as a tile template with the
placeholders `{z}` `{x}` `{y}` `{-y}` `{s}` `{key}` or as WMS, whose `BBOX` is formed from
the tile in EPSG:3857:

```ron
(
    id: "dop_nrw",
    name: "DOP Nordrhein-Westfalen",
    url: Wms(
        endpoint: "https://www.wms.nrw.de/geobasis/wms_nw_dop",
        layers: "nw_dop_rgb",
        version: "1.3.0",
        styles: "",
        extra: [("TRANSPARENT", "FALSE")],
    ),
    max_zoom: 20,
    tile_size: 512,
    format: Jpeg,
    attribution: "Geobasis NRW",
)
```

Availability and terms of use of each service must be checked before use; for bulk fetching,
put your own access keys into the configuration.

**Cache:** tiles end up under `<cache>/<provider>/<z>/<x>/<y>.<ext>`, with an in-memory cache
in front of it. Once loaded, the line can be edited offline (`L` toggles offline mode). Disk
space is capped; when the budget is full, the oldest tiles go first. The HUD shows hits,
loads, evictions and usage.

| Key | Function |
|---|---|
| `WASD` / arrows | Move the view point, `Page Up/Down` height |
| `O` | Overlay on/off |
| `P` | Switch provider |
| `[` `]` | Opacity |
| `,` `.` | Zoom level, `Z` back to target resolution |
| Numpad `4/6/8/2` | Image offset (with Shift in 5 m steps), `5` to reset |
| `L` | Offline mode |
| `C` / `R` | Clear cache / reset failed attempts |
| `F5` / `F2` | Load / save configuration |

## Scenarios

A scenario is a RON file of events — triggers plus actions:

```ron
(
    name: "Regionalbahn nach Musterstadt",
    player_train: 0,
    events: [
        (name: "abfahrt", trigger: Time(5.0),
         actions: [Announcement("RE 4711, Abfahrt frei.")]),
        (name: "regen", trigger: After(event: "abfahrt", delay: 60.0),
         actions: [SetRail(Wet), Message("Regen setzt ein.")]),
        (name: "ziel", trigger: TrainStopped(train: 0, edge: (2), s: 2600.0, radius: 50.0),
         actions: [Finish(success: true, reason: "Musterstadt erreicht")]),
    ],
)
```

Scored are timetable adherence, stopping accuracy, emergency brake applications, speed
limit violations and traction energy; the HUD shows messages and the score.

## Contributing

Rust stable, edition 2024. Before opening a pull request:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

- **Everything in English** — code, comments, documentation, commit messages (see [CLAUDE.md](CLAUDE.md)).
- **`sim-core` stays free of Bevy** and deterministic: fixed time step, seeded RNG, no wall clock.
  Simulation logic belongs there, not in the app.
- New behaviour comes with a headless test in the owning crate. Rulebook logic (PZB/LZB, brake) is
  table-driven — add a case, not a new test harness.
- Deliberate simplifications get a `ponytail:` comment naming the ceiling and the upgrade path.
- Pick up open points from [STATUS.md](STATUS.md); larger topics are outlined in [PLAN.md](PLAN.md).
  For anything sizeable, open an issue first so the direction is agreed before the work.

Licensed under MIT — contributions are accepted under the same licence.
