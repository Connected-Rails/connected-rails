# TrainSim-DE

[![CI](https://github.com/vanlueckn/open-train-simulator/actions/workflows/ci.yml/badge.svg)](https://github.com/vanlueckn/open-train-simulator/actions/workflows/ci.yml)

A **mod-first** German train simulator built on Bevy — implementation of [PLAN.md](PLAN.md).
Current state and open points: [STATUS.md](STATUS.md).

This project is designed from the ground up for modding — your own locomotives, your own
signals, your own lines. See [Mods](#mods) for the guide.

## Build and run

```bash
cargo test --workspace     # all acceptance tests (headless, no GPU)
cargo run -p app           # start the simulator
cargo run -p app -- --frames 120   # rendering smoke test (CI)
cargo run -p app -- --screenshot screenshots/hud.png   # capture an image and exit

cargo run -p app -- --line example:beispielstrecke --loco example:br101_afb   # from a mod
cargo run -p app -- --line example:beispielstrecke --scenario example:probefahrt
cargo run -p app -- --loco example:br101_afb --camera outside   # look at the vehicle model
```

For a faster edit-compile-run loop, add `--features dev` to any of the three binaries
(`app`, `route-editor`, `vehicle-editor`). It links Bevy as a shared library, which cuts the
relink after a code change. The first build with the flag recompiles Bevy, and the resulting
binary needs the Bevy DLL next to it — so use it for development only, never for a release.
Builds also use the toolchain's own `rust-lld` linker on Windows (see `.cargo/config.toml`).

Train protection and door control are **vehicle equipment**, not command line options: the
`safety` and `doors` fields of a `VehicleSpec` state which Indusi/PZB build, which Sifa and
which door control a vehicle carries (see [Mods](#vehicles)). Whether the equipment can do
anything also depends on the line — the LZB needs a conductor cable, the PZB needs magnets.
Switching the battery off and on again (`1`) restarts the function test of every system on
board.

`--screenshot` is available in both editors as well; `--frames N` sets after how many frames
the capture happens (60 frames ≈ 1 s of simulation time).

## Mods

Everything is meant to be moddable: your own locomotives, your own signals, your own lines.
A mod is a directory below `mods/`; `mods/example/` is the reference to copy from.

```
mods/<id>/mod.ron          id, name, version, author, depends, enabled
         /vehicles/*.ron   locomotives and coaches
         /lines/*.ron      track, equipment, signals
         /scenarios/*.ron  triggers and actions
         /signals/*.ron    signal types (aspect table + optional script)
         /scripts/*.lua    behaviour
         /assets/…         models, textures, sounds — as `mods://<id>/assets/…`
```

Everything is addressed as `"<mod>:<file stem>"`, e.g. `example:br101_afb`, so two mods may use
the same file names. Nothing is fatal: a broken file is a warning, everything else still loads.
Mods are loaded in dependency order (`depends`), alphabetically within that.

### Data and behaviour are separate

The bulk of a locomotive is **declaration**, not script — masses, running resistance, brake
equipment, tractive effort curve. That is RON, validated on load and editable without
programming. **Lua only covers real behaviour:** tap changer logic, AFB, the choice of a signal
aspect. That keeps roughly 80 % of every mod declarative, checkable and safe.

The Lua sandbox has `table`, `string` and `math` — no `io`, no `os`, no `require`, no
filesystem. A script sees a context table of numbers and booleans and answers with a table of
overrides; it never gets a handle on the simulation. A script that raises an error is switched
off, and the run continues.

### Signals: state machine as data, script only where needed

The interlocking supplies the *situation* of a signal — guarded sections clear, route locked,
diverging route, aspect of the following signal. The signal type maps that to an aspect; the
first matching rule wins (`mods/example/signals/ks_main.ron`):

```ron
(
    system: Ks,
    rules: [
        (when: (clear: Some(false)), show: (main: Some(Stop)), lamps: ["red"]),
        (when: (diverging: Some(true)),
         show: (main: Some(ProceedSlow), distant: Some(ExpectStop), speed: Some(40.0)),
         lamps: ["yellow", "zs3_4"]),
        (when: (next_stop: Some(true)),
         show: (main: Some(Proceed), distant: Some(ExpectStop)), lamps: ["yellow"]),
        (when: (), show: (main: Some(Proceed), distant: Some(ExpectProceed)), lamps: ["green"]),
    ],
    script: None,
)
```

`lamps` are free-form strings — your own presentation decides what they look like. A line points
at the type by name: `signal_type: Some("example:ks_main")`.

What a table cannot express — anything with memory or a timer — goes into `script`. The hook
runs after the table, sees its result in `ctx.main` and returns `nil` to keep it
(`mods/example/scripts/zs1.lua` gives Zs1 after three minutes at stop):

```lua
-- ctx: signal, time, clear, route, diverging, next_stop, next_slow, main, distant, speed
function M.aspect(ctx)
  if ctx.time - since >= 180.0 then
    return { main = "substitute", speed = 40.0, lamps = { "red", "zs1" } }
  end
end
```

### Vehicles: declaration plus behaviour hook

A `vehicles/*.ron` is the plain vehicle description; `script` is the only addition. The hook is
called once per frame for the train whose leading vehicle names it and writes cab controls —
here the AFB that is otherwise still missing (`mods/example/scripts/afb.lua`):

```lua
-- ctx: dt, time, v_kmh, speed_limit_kmh, mass_t, throttle, reverser, afb, afb_target, …
function M.update(ctx)
  if not ctx.afb or ctx.reverser == 0 then
    return nil
  end
  local target = math.min(ctx.afb_target, ctx.speed_limit_kmh)
  local notch = (target - ctx.v_kmh) / 10.0
  return { throttle = math.max(-1.0, math.min(1.0, notch)) }   -- also: direct_brake, sanding
end
```

Full field reference, sandbox rules and packaging: [MODS.md](MODS.md); background and state:
[PLAN.md ch. 19](PLAN.md), [STATUS.md](STATUS.md).

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
| `i18n` | Translations of everything the user reads (Fluent `.ftl`, English and German) |
| `world-coords` | ECEF f64 world coordinates, floating origin, geodesy (plan ch. 4) |
| `track-model` | Track geometry (straight/curve/clothoid), topology, switches, lineside equipment (ch. 5) |
| `sim-core` | Driving dynamics, air brake, electrics, train protection, interlocking, timetable, scenario and scoring — **without Bevy**, deterministic (ch. 6–11) |
| `content` | Vehicle database, line source format (RON) + compiler, scenarios, OSM/DGM importer (ch. 15) |
| `mod-runtime` | Mod discovery, declarative content, Lua behaviour hooks (ch. 19) |
| `ai-driver` | AI train driver, look-ahead (ch. 11) |
| `imagery` | Aerial imagery tiles: providers, Web Mercator maths, cache, fetching (ch. 15) |
| `app` | Bevy app: rendering, cameras, input, HUD (ch. 12), sound (ch. 13) |
| `editor-ui` | Shared look and feel of the desktop editors: colors, typography (Inter), spacing, form widgets |
| `route-editor` | Route editor: top-down view with aerial imagery overlay (ch. 15) |
| `vehicle-editor` | Vehicle editor: base data, glTF import, LOD, moving parts (ch. 15) |

`sim-core` is a pure Rust library with a fixed time step (200 Hz). The Bevy app ticks it and
mirrors the state into ECS components — simulation logic does not belong there.

## Key bindings

| Key | Function |
|---|---|
| `W` / `S` | Power controller up/down (negative = electric brake), `X` = zero |
| `R` / `F` / `T` | Reverser forward / reverse / neutral |
| `A` / `D` | Driver's brake valve release / brake |
| `Q` / `E` / `Z` | Lap / emergency brake / fill |
| `C` / `V` | Direct (additional) brake apply / release |
| `L` | Release button of the loco brake |
| `P` / `O` | Parking brake / pre-controlled (ep) brake on-off |
| `G` | Sanding |
| `J` / `K` / `I` | Door release left / right, close the doors |
| `Space` | Sifa (driver's safety device) |
| `Page Down` / `End` / `Delete` | PZB acknowledge / release / override |
| `N` / `M` / `B` | LZB takeover / end / function test |
| `U` | Train type switch (Zugartschalter): O → M → U, at standstill |
| `H` | Horn |
| `1`–`4` | Battery / pantograph / main switch / air compressor |
| `5` | Start the diesel engine |
| `F1`–`F3` | Camera: cab / external / lineside |
| `F9` | Mod manager: switch mods on and off (↑/↓ select, `Enter` toggles) |
| Arrow keys | View direction, `Numpad +/-` camera distance |

## Example line

`content::musterbahn()` — 7 km: 3 km straight (160 km/h), 1 km curve R = 1200 m with cant
ramp (130 km/h), 3 km at 8 ‰ gradient. Block signal at km 2.0 with distant signal,
1000/500/2000 Hz magnets, and over the last 4 km an LZB loop cable with block markers of its
own, so the LZB area runs in full block mode.

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

## Editors

There are **two separate programs**, because the two jobs have nothing to do with each
other: a route is geodata, a vehicle is a model with a data sheet.

| Program | Purpose |
|---|---|
| `cargo run -p route-editor` | line: track, equipment, aerial imagery overlay |
| `cargo run -p vehicle-editor` | vehicle: base data, glTF model, LOD, moving parts |

Both are desktop applications, not game screens: menu bar, docked panels, the operating
system's own file dialogs. `--frames N` and `--screenshot file.png` work in both.

## Language

Simulator and editors speak **English and German**. The language comes from the operating
system; `TRAINSIM_LANG=en` (or `de`) overrides it, and both editors switch it at runtime
under View → Language.

The strings live in `crates/i18n/locales/<lang>/main.ftl` ([Fluent][fluent]) and are
translated on Crowdin (`crowdin.yml`). A new language is a new directory next to `en`
plus one line in `i18n::LANGUAGES` — the source language is English.

[fluent]: https://projectfluent.org/

### Vehicle editor

```bash
cargo run -p vehicle-editor                                   # new vehicle
cargo run -p vehicle-editor -- mods/example/vehicles/br101_afb.ron
```

The left panel holds the vehicle's base data, the right one the model, the middle shows the
3D viewport with the track and a reference body of the length over buffers — so it is
immediately visible whether the model matches the LÜP. Right mouse button rotates, the
wheel zooms.

**Base data** (everything that is declaration, not script):

| Field | Meaning |
|---|---|
| Length over buffers | the official LÜP — spacing of the following vehicle. Draw the buffers 1–2 cm compressed in the model so they do not intersect in curves |
| Gauge | checked against the infrastructure, and used for the curve resistance |
| v max | highest permitted *running* speed, independent of the traction characteristic |
| Mass | tare mass; payload separately |
| Rotating mass | allowance for rotating parts of running gear and drive — acts on the inertia, not on the weight. Diesel-hydraulic 10–15 %, diesel-electric and electric loco 15–25 %, freight wagon 8–10 %, coach 6–9 % |
| Axles | information for consist lists and brake sheets |
| Axle base sum | sum over all bogies (two bogies of 2.5 m → 5.0 m), **not** the vehicle length — the larger the value, the higher the curve resistance |
| Rolling resistance | bearing friction and rolling of the wheel; "Suggest" derives a standard value from the mass |
| Air resistance | cw·A [m²]; `F = ½·ρ·cw·A·v²`. Without it the quadratic Davis term applies |
| Curve resistance | factor on Röckl — 1 = as the axle base sum gives it; lower it for radial steering bogies |
| Tilt angle | 0 for conventional vehicles, ~8° for German tilting units |
| Hunting | −1 no hunting, 0 standard (tuned for bogie vehicles), up to 1 more — raise it slightly for single-axle running gear |
| Max payload | e.g. about 5 t for a passenger coach, per the anscriptions for freight |

**Brake** — the panel below the base data. Control valve (`K-GP`, `KE-GP`, `KE-GPR`, `KE-Tm`,
`KE-L2a`, `KE-L2d`), brake position (G/P/R/R+Mg), friction pairing (cast iron block, disc,
K block, LL block, magnetic rail, or an own characteristic as a table), braked weight and
the force that follows from it, cylinder pressure and the cylinder/reservoir volume ratio.
Below that the additional brakes — magnetic track brake, direct brake, parking brake with or
without a spring accumulator, pre-controlled cylinder, air supplement brake, equalising
device — and the air data: auxiliary reservoir, brake pipe, main reservoir, compressor
delivery and leakage, plus the wheel slip protection (none, wheel slip brake, traction
cutback, creep control).

**Drive** — pick the model, then fill in the data sheet:

| Model | What is asked for |
|---|---|
| Tractive effort curve | the simplified model: a table km/h → N, optionally a second one for the dynamic brake |
| Tap changer (series-wound) | notches, time per notch, starting effort, power — and optionally the motor data (resistance, machine constant, saturation and maximum current, voltage, field weakening stages, gear ratio, wheel diameter), plus a rheostatic brake |
| Converter (three-phase) | starting effort, power, pull-out speed (above it the effort falls with 1/v²), brake force and power, fade-out speed, regenerative yes/no |
| Diesel | engine map (idle/rated/overspeed, full load torque over engine speed, speed- or fill-governed with droop, inertia, rack travel time), hydraulic transmission (circuits as converter or coupling with ratio, stall torque ratio, coupling point, absorption and its trend over ν, change-up point and primary influence; filling steps, filling and emptying time, change hysteresis, final drive, number of transmissions), hydrodynamic brake |

The detailed data is optional throughout: a `Diesel` without an engine map runs on the plain
tractive effort hyperbola, and the motor or gearbox can be added later without changing the
vehicle's type.

A hydraulic transmission cannot be computed back out of a given tractive effort curve, so it
is fitted: the plot under the drive panel draws the curve the parameters actually produce —
change point and all — and **Suggest** puts a usable starting set there, out of the starting
effort, the top speed, the rated speed, the rated torque and the wheel diameter.

**Models are glTF**, and the glTF's own features are used. Levels of detail and moving parts
are found in the file; the binding is stored in the vehicle RON, so **nothing has to be
prepared in Blender** — but a prepared file needs no clicking:

| In Blender | Result |
|---|---|
| Object name `body_LOD0`, `body_LOD1`, … | "Read from node names" fills the LOD table; the distances stay editable |
| Object name `door_left`, `pant_front`, `sw_throttle`, `gauge_speed`, `lamp_left`, `wheel_1` | suggested function plus a sensible motion |
| Custom property `ts_function` (plus `ts_motion` = `rotate`/`translate`/`visibility`, `ts_axis` = `"0 0 1"`, `ts_amount`) | exported into glTF `extras` and beats the name |

The simulator uses the same data: a vehicle with a model gets its glTF instead of the
placeholder body, the level of detail follows the camera distance, and the bound parts follow
the simulation state (pantograph, gauges, switches, lamps). Models live in the mod and are
addressed as `mods://<mod>/assets/<file>` — the same string in the editor and in the game.

Details: [MODS.md](MODS.md).

### Route editor with aerial imagery overlay

```bash
cargo run -p route-editor                              # example line
cargo run -p route-editor -- line.ron --imagery my_imagery.ron
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

Licensed under the EUPL v. 1.2 — contributions are accepted under the same licence. Mods are
exempt: RON data, assets and Lua scripts are not derivative works and may be sold under any
licence, see the mod exception in [LICENSE](LICENSE).

## Releases

`main` is the only long-lived branch. Work happens on short-lived `feat/…` or `fix/…`
branches (or forks) and lands via pull request; [CI](.github/workflows/ci.yml) runs
fmt, clippy and the test suite on Linux, Windows and macOS.

A release is a tag. Bump `workspace.package.version` in [Cargo.toml](Cargo.toml), then:

```bash
git tag v0.2.0 && git push origin v0.2.0        # release
git tag v0.2.0-rc.1 && git push origin v0.2.0-rc.1   # prerelease
```

Any tag containing a `-` is published as a prerelease — the version part must still
match `Cargo.toml`, otherwise the workflow stops before anything is published.
[The release workflow](.github/workflows/release.yml) builds the simulator and both
editors for Linux, Windows and macOS (Intel and Apple Silicon), packs each together with
`mods/` and the licence, and attaches the archives to a GitHub release whose notes are
generated from the merged pull requests.
