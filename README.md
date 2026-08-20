# Connected Rails

[![CI](https://github.com/Connected-Rails/connected-rails/actions/workflows/ci.yml/badge.svg)](https://github.com/Connected-Rails/connected-rails/actions/workflows/ci.yml)

A **mod-first** German train simulator built on Bevy — implementation of [PLAN.md](PLAN.md).
Current state and open points: [STATUS.md](STATUS.md).

This project is designed from the ground up for modding — your own locomotives, your own
signals, your own lines. See [Mods](#mods) for the guide. The main menu offers a clickable
interface to choose your line, vehicle and scenario from the loaded mods, to switch mods on
and off, and to change [settings](#settings) that are kept between runs.

## Build and run

```bash
cargo test --workspace     # all acceptance tests (headless, no GPU)
cargo run -p app           # start the simulator (main menu: drive, mods, settings, quit)
cargo run -p app -- --frames 120   # rendering smoke test (CI)
cargo run -p app -- --screenshot screenshots/hud.png   # capture an image and exit
cargo run -p app -- --screenshot shot.png --overlays   # …with the F5 and F6 overlays open
cargo run -p app -- --screenshot shot.png --hud reduced # …with the display on one of its three steps
cargo run -p app -- --menu --screenshot screenshots/menu.png   # …of the menu instead
cargo run -p app -- --screenshot night.png --time 22:30 --date 2026-01-15  # …at another hour of another day

cargo run -p app -- --line example:beispielstrecke --loco example:br101_afb   # from a mod
cargo run -p app -- --line example:beispielstrecke --scenario example:probefahrt
cargo run -p app -- --loco example:br101_afb --camera outside   # look at the vehicle model
cargo run -p app -- --loco example:br101_afb --camera walk      # start on foot (F4) instead of on the seat
cargo run -p app -- --camera walk --character example/models/driver.glb   # with a body (seen from F2/F3)

cargo run -p app -- --dedicated 27015                    # dedicated server, no window
cargo run -p app -- --connect 127.0.0.1:27015            # join one
```

Without arguments the simulator opens on a title screen: wordmark over the backdrop, and four
verbs — **Drive**, **Mods**, **Settings**, **Quit**. Drive walks line → vehicle → scenario in
three steps, shown as a numbered rail across the top with what was picked under each. Beside
the list a detail pane reads the highlighted entry out of the loaded content: length, permitted
speed and signals of a line; mass, running-gear limit, drive and brake of a vehicle; start time,
timetable and events of a scenario. `↑`/`↓` or the mouse select, `Enter` or a left click
confirms, `←`/`→` dial a setting, `Esc` goes one step back and leaves at the title screen; `F9`
opens the mod manager in-game. Any run flag (`--line`, `--loco`, `--scenario`, `--frames`,
`--screenshot`, …) skips the menu entirely, so the invocations above stay non-interactive —
`--menu` puts it back in front, optionally on a named page (`--menu settings`, also `root`,
`line`, `loco`, `scenario`, `mods`), which is the only way to photograph the menu itself.

The picture behind the menu lives in `crates/app/images/` and is compiled into the binary. The
one checked in today is a **placeholder that is not ours to distribute** — see the README there.

For a faster edit-compile-run loop, add `--features dev` to any of the four binaries
(`app`, `route-editor`, `vehicle-editor`, `signal-editor`). It links Bevy as a shared library, which cuts the
relink after a code change. The first build with the flag recompiles Bevy, and the resulting
binary needs the Bevy DLL next to it — so use it for development only, never for a release.
Builds also use the toolchain's own `rust-lld` linker on Windows (see `.cargo/config.toml`), dependencies compile at `opt-level = 3` while the workspace itself stays at `1`, and `--release` adds thin LTO with a single codegen unit.

Train protection and door control are **vehicle equipment**, not command line options: the
`safety` and `doors` fields of a `VehicleSpec` state which Indusi/PZB build, which Sifa and
which door control a vehicle carries (see [Mods](#vehicles)). Whether the equipment can do
anything also depends on the line — the LZB needs a conductor cable, the PZB needs magnets.
Switching the battery off and on again (`1`) restarts the function test of every system on
board.

`--screenshot` is available in the editors as well; `--frames N` sets after how many frames
the capture happens (60 frames ≈ 1 s of simulation time).

## Multiplayer

The same binary is the dedicated server. `--dedicated <port>` (or `<address>:<port>`) builds
the world, opens a UDP socket and runs the simulation without a window, a renderer or a sound
card; `--connect <host:port>` joins one. Without either flag nothing of it runs — single
player never opens a socket.

Both sides have to build the same world, so start them with the same `--line`/`--scenario`;
a fingerprint over line name and consists is exchanged on joining and complained about in the
log when it differs. On joining, a client asks for the train its own scenario put it in and
gets it while it is still free — otherwise the first train nobody has taken. A train a player
has taken over is no longer driven by the AI, and goes back to it when that player leaves.

What travels is the driver's levers and, ten times a second, the position of each train on
the track — `(edge, s, dir, v, a)`, about 17 bytes, not a transform. Every peer runs the same
deterministic simulation on those levers; the positions only correct the drift, and they do it
through the speed rather than by setting anything, so nothing ever jumps. Trains further than
3 km away are corrected once a second, past 20 km not at all. The HUD line **Server** shows
the connection, the train, the round trip time and the correction still pending — in normal
running it stays under a handful of centimetres. See [PLAN.md](PLAN.md) ch. 20 for the why.

Still open: choosing the server from the menu (today it is the command line), a lobby listing
the free trains, and authentication beyond netcode's shared key.

## Settings

The **Settings** section of the main menu writes a TOML file into the operating system's
settings directory for the current user (`%LOCALAPPDATA%\dev.vanlueck.connected-rails\settings.toml`
on Windows, `~/.config/dev.vanlueck.connected-rails/settings.toml` on Linux). It is Bevy's own
`bevy::settings`, so the file is plain text and can be edited by hand; an unknown or malformed
key falls back to the built-in default instead of taking the program down.

Every setting applies the moment it is changed — none of them waits for a restart or for
the next run. View distance moves the streamer's load radius while tiles are in the air,
bloom is added to and taken off the live camera, and the rest is re-read where it is used.

`Esc` during a run raises the **pause overlay** — the world stands still under it — with
**Resume**, **Settings**, **Back to the main menu** and **Quit**. Its settings page is the
same one, minus the language (not a driving decision) and the reset (too blunt to have under
the cursor while a train is standing on a gradient); everything on it takes effect while you
watch. `Esc` on the overlay resumes; going back to the main menu ends the run and takes the
built world down with it, so the next one starts from an empty world.

| Section | Setting | Effect |
|---|---|---|
| `[graphics]` | `view_distance` | How far terrain is built and drawn [m], 1000 … 12000. The biggest single cost. |
| | `shadows` | Shadow maps of the sun. |
| | `bloom` | Glow around lamps and signals after dark. |
| | `shadow_quality` | Edge length of the sun's shadow map: `Low` 1024, `Medium` 2048, `High` 4096 texels. |
| | `mist` | Ground mist as a volume, with the sun's shafts through it. |
| | `mist_quality` | Steps of the raymarch through it: `Low` 16, `Medium` 32, `High` 64. |
| | `texture_quality` | Size and filtering of the generated ground textures: `Low` 128², `Medium` 256², `High` 512². |
| | `anti_aliasing` | How the edges are smoothed: `Off`, `Fxaa`, `Smaa` or `Msaa`. |
| | `aa_quality` | How hard that works: `Low`, `Medium` or `High` — 2×/4×/8× for MSAA, the preset for the other two. |
| | `window` | `Windowed`, `Borderless` over the whole monitor, or exclusive `Fullscreen`. |
| | `vsync` | Caps the frame rate at the monitor's. |
| | `max_fps` | Frames a second the simulator holds itself to, 30 … 240; the top step (250) is no cap at all. |
| `[audio]` | `master` | Linear master volume, 0 … 1. |
| `[gameplay]` | `language` | `en`, `de`, or empty for the system's. |
| | `hud` | How much of the display is drawn: `Full`, `Reduced` or `Off` (`F7` walks the three). |
| | `look_speed` | Factor on the mouse look speed, 0.2 … 3.0. |
| `[controls]` | `binds` | The bindings, one line per rebound row — `throttle-up KeyW DPadUp` for a button, `lever-brake-valve RightTrigger2` for a lever, `-` for nothing. Only what differs from the default is written, so a new default reaches everyone who never touched that row. |

`TRAINSIM_LANG` stays the outermost override: where it is set, the stored `language` is
ignored, so scripted and CI runs are not steered by whatever was last picked in the menu.

## Mods

Everything is meant to be moddable: your own locomotives, your own signals, your own lines.
A mod is a directory below `mods/`; `mods/example/` is the reference to copy from.

```
mods/<id>/mod.ron           id, name, version, author, depends, enabled
         /vehicles/*.ron    locomotives and coaches
         /lines/*.ron       track, equipment, signals, electrification, track areas — a line, or a module with boundaries
         /compositions/*.ron modules chained into one line (georeferenced, auto-snapping)
         /scenarios/*.ron   triggers and actions
         /timetable/*.ron   timetables (stop scoring, referenced by a scenario)
         /signals/*.ron     signal types (aspect table + optional script)
         /signal_models/*.ron signal models: glTF parts on mount points, lamp bindings
         /blocks/*.ron      block presets for the vehicle editor's palette
         /track_types/*.ron superstructure classes: texture, speed limit, roughness, reverb, LZB flag
         /objects/*.ron     track objects: a 3D model plus its pose relative to the track
         /displays/*.html   cab displays as an HTML/CSS/JS page
         /scripts/*.lua     behaviour
         /assets/…          models, textures, sounds — as `mods://<id>/assets/…`
```

Everything is addressed as `"<mod>:<file stem>"`, e.g. `example:br101_afb`, so two mods may use
the same file names. Nothing is fatal: a broken file is a warning, everything else still loads.
Mods are loaded in dependency order (`depends`), alphabetically within that.

Lines are built from **modules** (Zusi-style): a module declares named `boundaries` at its
open ends, and a composition chains modules into one line — boundaries that lie at the
same geo position connect automatically. Several versions of a module (other epochs) are
simply several files; the composition picks one. See [MODS.md](MODS.md#modules-and-compositions).

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
here an AFB variant that replaces the built-in one (`mods/example/scripts/afb.lua`):

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
alignment from the OSM points. Heights can also be pulled in later from inside the route
editor: the DGM tile tool shows the elevation tile grid and imports either the picked tiles
or the whole corridor.

## Workspace

| Crate | Contents |
|---|---|
| `i18n` | Translations of everything the user reads (Fluent `.ftl`, English and German) |
| `world-coords` | ECEF f64 world coordinates, floating origin, geodesy (plan ch. 4) |
| `track-model` | Track geometry (straight/curve/clothoid), topology, switches, lineside equipment (ch. 5) |
| `sim-core` | Driving dynamics (adhesion axle by axle), air and vacuum brake, electrics, steam, train protection, interlocking, timetable, scenario and scoring — **without Bevy**, deterministic (ch. 6–11) |
| `content` | Vehicle database, line source format (RON) + compiler, scenarios, OSM/DGM importer (ch. 15) |
| `mod-runtime` | Mod discovery, declarative content, Lua behaviour hooks (ch. 19) |
| `html-display` | HTML/CSS/JS cab displays: parser, layout, script engine — in-engine, no browser (ch. 12) |
| `ai-driver` | AI train driver, look-ahead (ch. 11) |
| `imagery` | Aerial imagery tiles: providers, Web Mercator maths, cache, fetching (ch. 15) |
| `world-render` | Rendering shared by app and route editor: terrain tiles and splatting, vegetation, track objects, floating-origin anchoring |
| `app` | Bevy app: rendering, cameras, input, HUD (ch. 12), sound on kira's mixer — spatial tracks, distance and cab-wall filtering, Doppler, reverb (ch. 13); multiplayer and the dedicated server on lightyear (ch. 20); text in Fira Sans and Fira Mono (`fonts/`, SIL OFL 1.1) |
| `editor-ui` | Shared look and feel of the desktop editors: colors, typography (Inter), spacing, form widgets |
| `route-editor` | Route editor: top-down map over aerial imagery or a flown 3D view — track, equipment, objects, vegetation, terrain (ch. 15) |
| `vehicle-editor` | Vehicle editor: base data, block diagram (drive, brake, equipment), glTF import, LOD, moving parts (ch. 15) |
| `signal-editor` | Signal editor: modular signal models — glTF parts on mount points, lamp bindings (ch. 15) |

`sim-core` is a pure Rust library with a fixed time step (200 Hz). The Bevy app ticks it and
mirrors the state into ECS components — simulation logic does not belong there.

## The display

The HUD says what a driver could read off the desk without leaning forward, plus the two
things no desk shows: what the run is supposed to do, and what the line ahead is about to
ask for.

Everything on it is either **hardware** or **overlay**, and the two never look alike.
Hardware is the instrument panel at the bottom and the lamp housing beside it: a lighter
surface with a lit top edge and a shadow under it, round instruments with needles that
turn. Overlay is the run and the systems at the top: type on the world under one wash
across the width of the screen, with no frame at all.

| Zone | What stands there |
|---|---|
| Bottom centre | **The desk** — the speedometer with the line's permitted speed marked on its rim and the supervised speed over it, the Doppelmanometer carrying brake pipe (pale needle) and main reservoir (red needle) as in the cab, the brake cylinder on its own gauge, and beside them the levers: power controller, brake valve, effort, reverser, AFB, distance run |
| Bottom left | **Train protection** — the round lamps of a German desk, 1000 Hz, 500 Hz, Befehl, the train category and Sifa, with the LZB row under them and the MFA's v-soll, v-target and target distance while the LZB is guiding. Glass and legend light together in the lamp's own colour |
| Bottom right | **Look-ahead** — signed the way the line signs it: the triangle of an Lf 7 board with the speed on it, or the disc of Hp 0, and how far off it is |
| Top left | **The run** — clock, punctuality, service, and the timetable as a route ribbon: the stops in order down a rail, the wedge marking where the train stands with the distance to the next stop beside it, the next stop the only line set large. The score sits under a rule at the foot |
| Top right | **Systems** — ten annunciators of the desk (battery, pantograph, main switch, compressor, spring brake, sanding, doors, lights, wheel slip, hot motors) and three rows the drive labels itself: wire and motor current on an electric, engine and fill on a diesel, boiler, water glass and fire on a steam locomotive |
| Top centre | Scenario messages, and over the desk the banner that says the train protection has taken over |

**The timetable is a route ribbon, not a list of fields.** A "next stop / platform /
departure" block says where the train goes next; a ribbon says where it *is*. The rail
carries the stops in order — the one behind dimmed, the next one large, the two after it
between the two — and the wedge sits between the stop behind and the stop ahead with the
remaining distance beside it. The rows have fixed roles, so their weight is built once
rather than switched every frame.

**Punctuality is worked out, not printed.** The delay a train left the last stop with is
carried, and on top of that it is late by however long the scheduled arrival at the next
stop has been and gone. A train that has not reached a stop yet is never *early*: without
that rule every run would open by announcing itself seven minutes ahead of a stop it has
not moved towards.

**The graphics are drawn, not fetched** (`crates/app/src/glyphs.rs`): dial faces, needles,
rim markers, the Lf 7 board, the Hp 0 disc and the ten pictograms are a few lines of
geometry each, rasterised by a small signed-distance rasteriser when the run starts. There
is no asset directory, no icon set, and no third-party licence to carry — and a pantograph
that should read better at 20 px is a coordinate in that file rather than a new download.
The speedometer's scale comes from the vehicle's maximum speed and is drawn once, so the
figures on the face stay put while the line's limit changes.

Nothing that does not apply is drawn: the AFB row exists on a vehicle fitted with one, the
LZB lamps where an LZB is, the look-ahead when something is actually coming. `F5` opens the
keyboard as a sheet — with a legend of what the ten annunciators mean — and `F6` the
diagnostics: terrain, air detail, axles, temperatures, signals and the network, which is
where everything a driver has no use for lives.

**`F7` walks three steps** — full, reduced, off — and rounds back. The reduced step keeps
what the train is *driven* by (the desk and the protection lamps) and everything that
interrupts (the banner, scenario messages), and drops what it is *planned* by: the run, the
systems and the look-ahead. It is the step for driving by the cab's own instruments without
giving up the train protection; off is the one for a photograph. The step is a setting like
any other (`[gameplay] hud` — `Full`, `Reduced`, `Off`), so it survives the run, and
`--hud <step>` sets it for a screenshot without writing the settings file.

## Key bindings

Every one of them can be changed: **Settings → Key bindings** opens a page with one row per
control, showing the key on the left of the value column and the controller input on the
right. `Enter` on a row takes the next key or controller button pressed, `Backspace` takes
the binding away, `Esc` leaves it as it was, and one key only ever works one control —
binding it somewhere else takes it off whoever had it. The page is reachable from the pause
overlay as well, so a binding can be changed with the train standing on the line, and the
key sheet (`F5`) behind it is rewritten as soon as it is. The choice is kept in the settings
file under `[controls]`.

A controller is a first-class input: any connected pad answers to the same bindings. Out of
the box the D-pad is the power controller, the triggers are the brakes, `A` is the horn, `B`
the Sifa and `Y` the PZB acknowledge — the buttons are named by the letters Xbox pads print
on them rather than by Bevy's compass points.

**Levers on an axis.** The last group of the page is the three controls that have a
*position* rather than a direction — power controller, driver's brake valve, direct brake.
A key can only nudge one; a stick or a trigger holds it. `Enter` on such a row takes the
next axis moved past half travel, and from then on that axis drives the lever absolutely:
the stick or trigger *is* where the lever stands, and the keys for it are no longer read.
The power controller runs the full −1 … 1, so a stick pushed down is the electric brake and
a trigger gives the positive half; the brake valve maps 0 … 1 onto the full 1.5 bar of pipe
drop. Lap, fill and emergency stay on their keys — an axis has no detent for them, and
emergency latches until a key leaves it. Nothing is bound here out of the box: a bound lever
writes its control every frame, which would otherwise hold the brake valve at Release for
everyone who has a pad plugged in and never touches it.

Looking around and walking are the one deliberate exception: the right stick looks and the
left stick walks, always, and neither is bindable. Those are not levers of the desk.

The table below is what everything ships with.

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
| `Y` | Wipers: off → interval → slow → fast (cycles) |
| `U` | Train type switch (Zugartschalter): O → M → U, at standstill |
| `^` | Range selector of a two-range gearbox: shunting gear ↔ road gear, takes at a stand |
| `H` | Horn |
| `1`–`4` | Battery / pantograph / main switch / compressor |
| `5` | Start the diesel engine |
| `6` / `7` / `8` | AFB on/off / dial down / dial up (in 10 km/h steps) |
| `9` / `0` | Headlights / cab light |
| `,` / `.` | Instrument backlighting dimmer down / up |
| `Esc` | Pause: resume, settings, back to the main menu, quit — the world stands still under the overlay |
| `F1`–`F4` | Camera: driver's seat / external / lineside / first person |
| `F5` / `F6` | Keyboard sheet / diagnostics overlay |
| `F7` | Display: full → reduced → off, and round again |
| `F9` | Mod manager (↑/↓ select, `Enter` toggles; in-game it applies on the next restart, on the main menu it applies on start, rows are clickable) |
| Arrow keys | View direction, `Numpad +/-` camera distance |
| `WASD` / `Shift` | First person (`F4`): walk (1.5 m/s) and run (5 m/s) through the train and over the ground. The walker falls where the ground drops away, climbs what is no higher than a step, is stopped by what stands at chest height and walks on through the train from vehicle to vehicle. The mouse looks around on its own, the cursor is caught on the crosshair and the driving keys rest until `F1` puts the driver back on the seat |
| `E` | First person: through the door — out beside the train, and back in at any vehicle standing next to you. A passenger door has to be open for it; a traction unit's own cab door opens itself, both only at a stand |
| Mouse | Left button operates the controls of the 3D cab (click, drag, wheel), right button looks around |

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

The ground is textured by **splatting**: per-vertex weights from slope and track distance blend
grass, rock and gravel. Trees are line content — every tree its own entry, spawned as a child
of its terrain tile, so vegetation streams and batches with the ground it stands on. Terrain,
splatting, vegetation and track objects live in `world-render` and therefore look the same in
the simulator and in the route editor.

The app shows the terrain automatically (flat without DGM):

```bash
cargo run -p app -- --dgm ./dgm1_niedersachsen --epsg 25832
```

For a line across the 12° UTM zone boundary, repeat the pair — one elevation source per
zone; the n-th `--epsg` belongs to the n-th `--dgm`:

```bash
cargo run -p app -- --dgm ./dgm1_west --epsg 25832 --dgm ./dgm1_ost --epsg 25833
```

## Editors

There are **three separate programs**, because the jobs have nothing to do with each
other: a route is geodata, a vehicle is a model with a data sheet, a signal model is
an assembly of shared parts.

| Program | Purpose |
|---|---|
| `cargo run -p route-editor` | line: track, equipment, switches, marked track areas, objects, vegetation, terrain, aerial imagery overlay |
| `cargo run -p vehicle-editor` | vehicle: base data, block diagram (drive, brake, equipment), glTF model, LOD, moving parts, 3D cab, displays, sounds |
| `cargo run -p signal-editor` | signal model: glTF parts on mount points, lamp bindings, lamp test |

All are desktop applications, not game screens: menu bar, docked panels, the operating
system's own file dialogs. `--frames N` and `--screenshot file.png` work in all of them.

The route editor draws the module under the simulator's own sky. Its **Time of day**
section sets the date, the clock, the time zone and the cloud cover, and a slider runs a
whole day past in one drag — which is how you find out that the platform lies in the
shadow of its own canopy all morning. Latitude and longitude are not edited there: they
are the module's anchor, the same pair a run reads, so both programs put the sun over the
same hillside. Underneath, the panel reads out where the sun and the moon actually stand.

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
cargo run -p vehicle-editor -- mods/example/vehicles/br101_afb.ron --graph   # open on the block diagram
```

The left panel holds the vehicle's base data, the right one the model, the middle shows the
3D viewport with the track and a reference body of the length over buffers — so it is
immediately visible whether the model matches the LÜP. Right mouse button rotates, the
wheel zooms. Chips at the top left of the centre switch it between the **3D model** and the
**block diagram**; the `--graph` flag starts on the diagram.

**Base data** (everything that is declaration, not script):

| Field | Meaning |
|---|---|
| Length over buffers | the official LÜP — spacing of the following vehicle. Draw the buffers 1–2 cm compressed in the model so they do not intersect in curves |
| Gauge | checked against the infrastructure, and used for the curve resistance |
| v max | highest permitted *running* speed, independent of the traction characteristic |
| Mass | tare mass; payload separately |
| Rotating mass | allowance for rotating parts of running gear and drive — acts on the inertia, not on the weight. Diesel-hydraulic 10–15 %, diesel-electric and electric loco 15–25 %, freight wagon 8–10 %, coach 6–9 % |
| Axle base sum | sum over all bogies (two bogies of 2.5 m → 5.0 m), **not** the vehicle length — the larger the value, the higher the curve resistance |
| Rolling resistance | bearing friction and rolling of the wheel; "Suggest" derives a standard value from the mass |
| Air resistance | cw·A [m²]; `F = ½·ρ·cw·A·v²`. Without it the quadratic Davis term applies |
| Curve resistance | factor on Röckl — 1 = as the axle base sum gives it; lower it for radial steering bogies |
| Tilt angle | 0 for conventional vehicles, ~8° for German tilting units |
| Hunting | −1 no hunting, 0 standard (tuned for bogie vehicles), up to 1 more — raise it slightly for single-axle running gear |
| Max payload | e.g. about 5 t for a passenger coach, per the anscriptions for freight |

**Drive, brake, equipment and behaviour are the block diagram** — a blueprint-style node
editor: the vehicle is a circuit of components, and the physics follows from what is wired
to what. The palette on the left (searchable, grouped by category) carries every physical
component as a block — pantograph, transformer, tap changer, starting resistors, chopper
and series/parallel switch, traction converter, series-wound and induction motors, diesel
engine with hydraulic transmission and retarder, with a mechanical gearbox and its clutch,
with a hydrostatic drive or with generator and load regulator,
boiler, firebox, cylinders, injector and tender of a steam locomotive, the complete air
**or vacuum** brake from compressor to brake rigging including EP brake, angle cocks,
limiting and retaining valve, cooling systems, wheelset with its bogies and axles, cab, AFB, the
logic blocks (reading, characteristic, PID, notching, rate of change, switch, output),
Sifa, PZB, LZB, doors and the Lua script hook — plus the **presets installed mods bring**
(`blocks/*.ron`, e.g. a Voith L 620 as a preset of the hydraulic transmission). A block is
dragged out of the palette onto the canvas and lands where the pointer lets go; a click on
it appends it below the diagram instead. A right click on the canvas adds a
block, a right click on a node removes it, a drag from pin to pin wires them; pins are
colour- **and** shape-coded by domain (shaft, force, electrical, pneumatic, signal, fuel,
steam, water, heat), and only like connects to like. Clicking a node puts its data sheet
below the palette —
control valve and friction pairing, engine map, converter circuits, motor data; axle count
and adhesive mass sit on the wheelset block — together with the live **bake findings**:
the diagram is stored in the vehicle file (`graph`) and baked into the runtime fields
(`traction`, `brake`, `safety`, `doors`, …) on save and on load, and every error or
warning of that bake is listed, a click selecting the offending block. A vehicle file
without a diagram opens with one synthesised from its spec. The palette reference, the
wire rules and the preset format: [MODS.md](MODS.md#block-diagram).

**Models are glTF**, and the glTF's own features are used. Levels of detail and moving parts
are found in the file; the binding is stored in the vehicle RON, so **nothing has to be
prepared in Blender** — but a prepared file needs no clicking:

| In Blender | Result |
|---|---|
| Object name `body_LOD0`, `body_LOD1`, … | "Read from node names" fills the LOD table; the distances stay editable |
| Object name `door_left`, `pant_front`, `sw_throttle`, `gauge_speed`, `lamp_left`, `wheel_1` | suggested function plus a sensible motion |
| Custom property `ts_function` (plus `ts_motion` = `rotate`/`translate`/`visibility`/`emissive`, `ts_axis` = `"0 0 1"`, `ts_amount`) | exported into glTF `extras` and beats the name |
| Node name ending in `_NIGHT` (lit windows, glowing signs) | switched on at dusk in every model, no binding needed |

The simulator uses the same data: a vehicle with a model gets its glTF instead of the
placeholder body, the level of detail follows the camera distance, and the bound parts follow
the simulation state (pantograph, gauges, switches, lamps). Models live in the mod and are
addressed as `mods://<mod>/assets/<file>` — the same string in the editor and in the game.

**Cab, displays and sounds** are edited in the same program. The cab panel sets the eye point
and binds glTF nodes to cab controls — each with the gesture that suits it, so a lever is
dragged, a button pressed and a rotary switch stepped with the wheel. Instruments are bound the
same way: `gauge:` pointers, `lamp:` indicators and `digit:` seven-segment counters. A screen is
a **display** rendered to texture, written either as a declarative widget list in RON, as a Lua
`display(ctx)` hook with menus and softkeys, or as an HTML/CSS/JS page under `displays/` —
parsed, laid out and scripted in-engine by `html-display`, with no browser embedded. The sound
table maps quantities (speed, traction, air, roughness, rain, control clicks) to the mod's own
samples, normally as **crossfaded layers** — three loops over overlapping speed windows rather
than one stretched by its playback rate. A **▶ per entry** plays it through the editor's own
output device with a slider for every quantity it depends on, so the crossfade can be dragged
through by hand instead of guessed at from a sparkline.

Details: [MODS.md](MODS.md).

### Route editor

```bash
cargo run -p route-editor                              # example line
cargo run -p route-editor -- line.ron --imagery my_imagery.ron
```

The line is edited from above, over the aerial imagery. The tool palette sits on the left, the
selected element's fields on the right; the middle mouse button drags the map, the wheel zooms.
Every edit goes through undo/redo (Ctrl+Z, Ctrl+Y or Ctrl+Shift+Z), the rule check flags what
the compiler will reject, and saving guards against discarding unsaved work.

The palette is grouped by what the work is about — the track itself, what is mounted along
it, and the landscape it runs through — and every tool carries a drawn icon beside its name.
A bar sits above the viewport with the controls that belong to looking rather than to the
document: view angle, gizmo mode, aerial imagery on or off, and the camera speed of the 3D
view. The terrain is always drawn; the imagery is a layer draped over its shape.

**Content drawer** (`Ctrl`+`Space`, or the button at the left of the status bar): a panel
that comes up from the bottom edge with everything the installed mods brought — scenery
objects, signal types, signal models and track types, each with the mod it came from and a
filter over the lot. It is the one place that answers whether the editor found a newly
installed mod at all; the tool pickers only ever show one kind. Picking an object from it
arms the object tool with that object.

Everything that has a model carries a **rendered preview** of it. The editor renders each
one once, off to the side on its own render layer, and reads the picture back off the
render target into an ordinary image — a target only holds its contents while an active
camera points at it. One at a time, and only for what the drawer is actually drawing, so a
catalogue nobody opens costs nothing. Track types show their colour instead, which is what
a track type is on the map. `F4` opens the same document into a **3D view** and back. It is the same orbit at a different
angle — the map is the case that looks straight down — and it is flown the way an Unreal
viewport is: hold the right mouse button to look and fly with `WASD` (`Q`/`E` down and up,
`Shift` faster, the wheel sets the camera speed), `Alt`+left orbits the view point, the middle
button pans, `F` frames the selection. Selecting is a question about pixels in both views:
whatever is under the cursor, near or far.

The selection carries a **transform gizmo**, `W` for the arrows and `E` for the rotation ring
(in the 3D view, where those letters are free). Its axes are the fields the item actually has,
not world X/Y/Z: dragging the red arrow slides a signal *along* the track (`s`), the green one
across it (`lateral_offset`) and the blue one up (`height`) — so the saved file still reads
like a placement. Trees, markers and terrain strokes are free of the track and get east/north
instead. There is no scale handle, because nothing in the file format has a scale.

**A module starts as a place, not as a blank sheet.** *File → New module* (`Ctrl+N`) asks for
a name and the module's anchor — latitude and longitude as fields, and beneath them a small
OpenStreetMap map with a place search: type "Göttingen Bahnhof", pick the hit, click the exact
spot, and the coordinates fill themselves in. Around that anchor the new module gets its first
**envelope**, a square of the module size the dialog asks for (4 km by default), whose corners
are dragged into shape afterwards (see the *Envelope* tool below, and MODS.md → Modules). The anchor decides which elevation tiles, which aerial
imagery and which neighbours the module will meet, so typing it blind is the one thing worth
a dialog.

The palette is grouped, and the number keys count down it — the key and the button always
agree, because both read the same list.

| Tool | What a click does |
|---|---|
| **Track** | |
| `1` Select | Pick a track, device or object and edit its fields; `Delete` removes it |
| `2` Draw track | Every click appends the arc that leaves the alignment tangentially and hits the point — G1-continuous by construction. `Enter` or right-click finishes, `Esc` cancels |
| `3` Place switch | Splits the track and draws the branch; facing or trailing and the throw time follow in the panel |
| `4` Mark area | Press on a track and drag along it: the tool paints a wide coloured stroke over the rails, and that stretch is the area. With an area selected the next stroke joins it. A marked area carries speed, cant, gradient, track type and electrification — set the stretch once instead of editing a step profile per property per track |
| **Lineside equipment** | |
| `5` Place device | Puts the chosen device kind (signal, magnet, LZB, platform, …) on the clicked track |
| `6` Place object | Drops a mod's 3D object at its predefined offset and rotation |
| `7` Place marker | A reference marker in a named layer — a drawing aid, nothing in the simulation reads it |
| **Landscape** | |
| `8` / `9` Tree / forest | One tree per click, or an outlined area baked into single trees — each one stays editable |
| `0` Marking brush | Sweep to mark trees and objects in bulk and delete them together |
| Terrain brush | One stroke per click: raise, lower or level. The track keeps its height, cutting and embankment are laid over it afterwards |
| DGM tiles | Shows the elevation tile grid and picks single tiles for the height import; without a pick the whole corridor is imported |
| **Module** | |
| Envelope | Reshapes the module boundary: drag a corner, a click on a side adds one there, `Delete` removes the selected one. Everything the module owns has to lie inside it — the landscape strictly, the track up to the boundary itself |

The **scene is rendered into the whole window** and the panels are drawn on top of it —
`bevy_egui` hangs its context on the same camera, so a camera viewport of its own would
squeeze the UI into that rect as well. The camera is therefore shifted sideways instead, by
exactly what the panels cover: the pivot sits in the middle of what is *visible*, and the
imagery tiles are fetched around that point rather than around a spot behind the side panel.

`T` swaps the aerial imagery for the module's terrain, built exactly as the run builds it —
so track types, objects and vegetation can be judged against the ground they sit on. The
interlocking (signals, sections, routes), the **track areas** and the module boundaries are
forms of their own, with a ghost of the neighbouring module at the boundary. Every painted
area lies on the map in its own colour, all the time — a marking that only shows while it
is selected is a marking nobody trusts.

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
| `WASD` / arrows | Move the view point on the map, `Page Up/Down` height |
| `F4` | Switch between the top-down map and the 3D view |
| Right mouse + `WASD` `Q` `E` | Look and fly (3D view), `Shift` faster |
| `Alt` + left mouse | Orbit the view point (3D view) |
| `F` | Frame the selection |
| `W` / `E` | Move or rotate handles of the gizmo (3D view) |
| `1`–`0` | Pick a tool (see the table above) |
| `Ctrl`+`Space` | Content drawer: everything the installed mods bring |
| `O` | Aerial imagery on/off — it drapes over the terrain, which is always drawn |
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
    start: (year: 2026, month: 8, day: 15, hour: 6, minute: 45, utc_offset: 2.0),
    player_train: 0,
    events: [
        (name: "abfahrt", trigger: Time(5.0),
         actions: [Announcement("RE 4711, Abfahrt frei.")]),
        (name: "regen", trigger: After(event: "abfahrt", delay: 60.0),
         actions: [SetWeather(Rain), Message("Regen setzt ein.")]),
        (name: "ziel", trigger: TrainStopped(train: 0, edge: EdgeId(2), s: 2600.0, radius: 50.0),
         actions: [Finish(success: true, reason: "Musterstadt erreicht")]),
    ],
)
```

Scored are timetable adherence, stopping accuracy, emergency brake applications, speed
limit violations and traction energy; the HUD shows messages and the score. A scenario
gets its timetable by reference (`timetable: Some("<mod>:<name>")`, a `timetable/*.ron`
in the mod) — without one, only the scenario's own points count. A timetable is either
`kind: Scenario` (times from the start of the run, runs once) or `kind: Daily` (times as
seconds since midnight, wrapping around every 24 h). `start:` sets date and local time
of the run (default: midsummer noon) — it anchors `Daily` timetables, puts the sun
and moon where they belong for the georeferenced line, and paints the season: meadows
turn through October, ground and trees go under snow from November to March. The sky
those two stand in is a scattering model, not a gradient: Rayleigh and Mie through the
look-up tables of Hillaire's technique, so noon is blue, sunset is red and a valley
twenty kilometres off lies in haze, all of it out of the sun's elevation alone. Behind
them stand the real stars — the naked-eye HYG catalogue, held in equatorial coordinates
and turned by the latitude and the sidereal time, so the pole star sits at the latitude's
altitude and everything rises four minutes earlier each night. The moon is a disk half a
degree wide, lit from where the sun really is, which is where its phase comes from.
`--time 22:30` and `--date 2026-01-15` move the clock of a run for a screenshot. A mod's
track object may bring its own `autumn_model`/`winter_model` glTF — optional, and
whatever it leaves out keeps the year-round model.
`SetWeather(Clear | Cloudy | Overcast | Fog | Drizzle | Rain | Storm | Thunderstorm |
Sleet | Snow | Blizzard | Hail | Frost)` moves the weather there over five minutes — a
front, not a switch. Every preset is a set of physical numbers (cover, cloud base,
precipitation rate, wind, sight, temperature, thunder), and everything downstream reads
those rather than the name: volumetric clouds and their shadows, the haze in the
atmosphere, the rain and snow around the camera, and the water and snow that gather on
the ground and decide what the wheels find on the rail. A scenario can also start in a
weather (`weather: Rain` beside its `start`), and `--weather snow` places one for a
screenshot; in a normal run the same flag lets the front *move in* over five minutes —
a first drizzle, single drops on the glass, the rail greasy before wet. `--wipers 2`
starts with them running. `SetRail(Dry | Wet | Slippery)` still
sets the rail by hand — leaves and sanded rail have no weather to come from — and holds
until the weather next changes. From the driver's seat the rain is on the glass too: a
vehicle names its panes in `cab: (windscreen: [...])`, and the wiper clears the strip its
blade has just crossed.

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
licence, see the mod exception in [LICENSE](LICENSE). Material from other projects that is
checked into the repository is listed in [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).

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
[The release workflow](.github/workflows/release.yml) builds the simulator, the route
editor and the vehicle editor for Linux, Windows and macOS (Intel and Apple Silicon) —
the signal editor is built from source for now — packs each together with
`mods/` and the licence, and attaches the archives to a GitHub release whose notes are
generated from the merged pull requests.
