# Implementation status against PLAN.md

As of 2026-08-12 · `cargo test --workspace`: **189 tests green** · clippy and fmt clean.

**This project is mod-first.** See [MODS.md](MODS.md) for how to create trains, signals and lines.

## Milestones

| M | Contents | Status |
|---|---|---|
| **M0** | Workspace, `world-coords` (ECEF f64 + floating origin) | **done** — acceptance test "300 km without jitter/jump" green |
| **M1** | `track-model`, procedural track rendering, streaming | **done** — graph, clothoids, `eval`, switches (incl. trailing moves), track meshes; terrain tiles stream in and out around camera and trains |
| **M2** | Longitudinal dynamics + brake, electric loco + coaches, basic cab | **done except audio** — coasting against Davis, emergency braking distance, starting on a gradient, coupler slack as tests; **sound (ch. 13) missing** |
| **M3** | Sifa + PZB 90, signals, editor v1 | **partial** — Sifa (time-time, time-distance, RZM) and every intermittent build from the Indusi I 54 to the PZB 90 V2.0 complete with standard-case tests; H/V + Ks signal logic present, but without lamp aspect rendering; the **route editor** shows a line with an aerial imagery overlay but cannot edit it yet; the **vehicle editor** edits base data, glTF model, LOD and moving parts |
| **M4** | Interlocking, AI trains, timetable | **done** — routes with locking/release, automatic block, AI stops at signals and platforms |
| **M5** | LZB 80 + AFB, MFA, tap-changer loco | **partial** — LZB with guidance, braking curve, end and failure procedures, with and without PZB, full/partial block mode and CIR-ELKE; BR 110 present; **AFB missing**, MFA only as HUD text |
| **M6** | Interactive 3D cab, start-up procedure, audio, weather/night | **partial** — start-up chain simulated and operable via keyboard, weather changes through scenario actions, terrain from the DGM; no 3D cab, no audio, no vegetation/texturing |
| **M7** | Pilot line from OSM/DGM, scenarios, scoring, save/load | **largely done** — scenario system, scoring, save/load and the OSM/DGM importer are in place; only a real pilot line is missing (data procurement) |
| **M8** | Mod runtime: declarative content plus Lua behaviour | **usable** — loader with dependency order, vehicles/lines/scenarios/signal types as RON, signal state machine as data, two Lua hooks (vehicle, signal aspect) with a sandbox; reference mod under `mods/example` incl. a glTF model. **Missing:** mod manager UI, line/scenario hooks |

## What is in place

- **Coordinates (ch. 4):** ECEF f64, ENU frames, floating origin with rebase every 4 km incl.
  new ENU rotation, earth curvature correction of the tangent plane, UTM↔geodetic for the import.
- **Streaming (ch. 4.3):** the world is tiled in the UTM grid of the elevation data.
  `TerrainBuilder` keeps line and DGM resident and hands out single tiles by key; the app
  builds everything within 4 km of the camera **and of every train** on the
  `AsyncComputeTaskPool` and discards it again at 5 km (the gap is the hysteresis).
  The simulation is untouched by it — track graph and timetable stay resident, AI trains
  keep running in areas that carry no graphics.
- **Track model (ch. 5):** one segment type (`k0`, `dk`) for straight, curve and clothoid;
  switches with throw time, locking and trailing-move detection; trackside devices with RON payload.
- **Driving dynamics (ch. 6):** Davis, curve and gradient resistance, couplers with slack and
  breaking force, Curtius/Kniffler adhesion with wheel slip/slide, sanding, wheel slide protection.
- **Brake (ch. 7):** brake pipe as a node chain, KE control valve (three-pressure system,
  exhaustibility), brake positions G/P/R(+Mg), direct brake, blending with the electric brake,
  braked weight percentage.
- **Electrics (ch. 8):** start-up chain, pantograph travel time, main switch drop-out in
  neutral sections, three traction types (tap changer, converter, diesel).
- **Train protection (ch. 9):** trait abstraction + country package DE with all intermittent
  builds, LZB and three Sifa builds:
  - **Indusi/PZB** as one state machine plus a parameter set per build — **I 54**, **I 60**,
    **I 60M**, **I 60R**, **ÖBB PZB 60**, **PZB 90 V1.5** and **V2.0**. The older builds
    supervise the check speed only once the 1000 Hz time has run and hold the 500 Hz speed
    constant; the distance supervision (1250 m), the restrictive mode and the supervised
    override came with the I 60R and the PZB 90. V1.5 enters the restrictive mode only after
    a stop and holds 45 km/h there, V2.0 also after 15 s below 10 km/h and falls to 25 km/h.
    Train categories O/M/U with all check speeds, acknowledgement, exemption from 700 m,
    override 40 and the forced braking logic apply throughout.
  - **LZB 80/I 80** with and without PZB, full and partial block mode (in partial block mode
    the signals stay binding, so their magnets remain the fallback level), **CIR-ELKE**
    (steeper braking curve, 5 km/h speed steps, speed rises effective at the head of the
    train instead of at its rear), takeover, v-target/v-goal/distance to target, end and
    failure procedures.
  - **Function test** (Funktionsprüfung) of the PZB and the LZB: lamp test → internal test →
    acknowledgement, at a standstill only; the PZB holds the forced braking until it passes.
    Switching the battery on starts it.
  - **Sifa** in three builds: time-time, time-distance (30 s **or** 1250 m) and RZM
    (time-distance plus a minimum interval between operations).
- **Interlocking (ch. 10):** signal aspects, distant signalling, automatic block, routes,
  signal-dependent magnet activation.
- **AI (ch. 11):** look-ahead across the track graph, braking curve with reaction and
  response distance, timetable stops, operates Sifa and PZB itself.
- **Scenarios (ch. 11.4):** RON events with triggers (time, train position, stop, speed,
  signal aspect, emergency brake application, chaining with delay, `All`/`Any`)
  and actions (message, announcement, switch, route, weather, points, scenario end).
- **Scoring (ch. 11):** timetable adherence, stopping accuracy, emergency brake applications,
  speed limit violations and traction energy → itemised score.
- **Alignment (ch. 15):** design elements are reconstructed from the point sequence —
  section separation, radius averaging over the whole curve (Kåsa), rounding to standard radii
  with tolerance, transition curves and cant per the rulebook (`c = 11.8·v²/R` minus
  cant deficiency, capped, ramp 1:10·v). Direction is estimated via best-fit lines in a sliding
  window, not from neighbouring differences — otherwise point noise completely masks the
  curvature. Acceptance: an alignment designed to the rulebook is recovered with the exact
  radius, correct cant and < 6 m positional deviation, even with ±2 m noise on the
  support points.
- **Import (ch. 15):** Overpass JSON → way chain → alignment → `LineSource`. From OSM come
  geometry, `maxspeed` and `name`; DGM tiles (XYZ or ESRI ASCII Grid) from a single file or
  an entire directory supply the gradient profile. Tiles are loaded lazily (sheet boundaries
  from the file name) and kept in an LRU, so even a federal state's DGM1 is usable.
  CLI: `import-line`.
- **Terrain (ch. 14):** 512 m tiles only within the line corridor, grid spacing by distance
  from the track (4 m to 32 m instead of 1 m), skirts against LOD cracks, cutting/embankment
  at the track, view distance limit per LOD level in the app, built while driving (see
  streaming above).
- **Aerial imagery overlay (ch. 15):** dedicated `imagery` crate with Web Mercator tile maths,
  provider configuration (tile template or WMS, placeholders, keys, zoom limits,
  attribution) and a two-level cache (memory + disk, budget with eviction of the oldest
  tiles, maximum age, offline mode). Fetching and decoding run on worker threads, the editor
  places the tiles georeferenced beneath the track ribbon.
  All of it controllable through a RON file, reloadable at runtime.
- **Editors (ch. 15):** two separate programs with a desktop UI (menu bar, docked panels,
  native file dialogs): `route-editor` shows a line with an aerial imagery overlay and can
  load another one at runtime; `vehicle-editor` edits the vehicle base data (LÜP, gauge,
  v max, mass, rotating mass, axles, axle base sum, rolling and air resistance, tilt angle,
  hunting, payload), imports glTF models, reads their levels of detail from the node names
  and binds moving parts — either through name prefixes, through the Blender custom
  property `ts_function`, or by hand from the node list. The viewport shows one level at a
  time against a reference body of the length over buffers.
- **Vehicle models in the app (ch. 15.3):** a vehicle with a model gets its glTF instead of
  the placeholder body; the level of detail follows the camera distance, and the bound parts
  follow the simulation (pantograph, gauges, switches, lamps). `--camera outside` starts on
  the external camera.
- **Mod runtime (ch. 19):** `mods/<id>/` with manifest, vehicles, lines, scenarios, signal types
  and scripts; everything addressed as `"<mod>:<file>"`, loaded in dependency order, broken files
  are warnings instead of crashes. **Data and behaviour are separate:** the vehicle description
  stays RON, only behaviour is Lua. Signals are a declarative state machine (situation → aspect +
  lamp image) with an optional script hook for what a table cannot express. Lua runs sandboxed
  (`table`/`string`/`math` only) via `mlua`; a failing script is switched off, not fatal.
  `mods/` is an asset source, so models, textures and sounds of a mod are addressed as
  `mods://<mod>/assets/…`. The app takes `--line <mod>:<name>` and `--loco <mod>:<name>`.
- **Cross-cutting (ch. 16):** fixed time step, seeded RNG, state hash with determinism test,
  full serialisation for save/load.

## Deliberately deferred

Every simplification is marked with a `ponytail:` comment at the code site, with an upgrade path:

- **Brake pipe model** is a node/diffusion model, not a pressure wave.
- **Slip per vehicle**, not per wheelset.
- **Flank protection** is only switch locking.
- **LZB braking curve** with a fixed deceleration instead of train-specific brake assessment
  (0.6 m/s², 0.85 m/s² under CIR-ELKE).
- **LZB block modes** are reduced to "signals binding yes/no". The block division of the
  movement authority itself follows once the LZB centre in the interlocking generates block
  markers of its own.
- **Sifa RZM** is modelled as time-distance plus a minimum interval between operations, so
  beating the pedal continuously does not satisfy the device. Where the build differs in the
  detail, `SifaParams` is the place to correct it.
- **The parameter sets of the older Indusi builds** (I 54 … ÖBB PZB 60) carry the check
  speeds of the train categories over from the PZB 90 rulebook, which is where they came
  from historically; what differs per build is *which* supervision an influence starts, and
  that is modelled in full. The tables sit in `PzbVariant::spec` as `const` values.
- **Geoid undulation** as a constant offset per line.
- **Device payload** as RON *text* instead of `ron::Value` (Value loses unit enum variants).
- **No CRS framework:** UTM 32/33 directly as a Snyder series in `world-coords::geo` instead of
  `proj4rs`/`geodesy` — for Germany that is exactly two projections. Should Gauss-Krüger or
  neighbouring countries be added, `proj4rs` steps in behind the same signature.
- **The importer chains a single strand**, no routing across switches; station throats will
  need a real graph search later.
- **DGM sheet boundaries from the file name** (state convention: `…_389_5711_…`); if the name
  does not match, the tile is read once to determine its extent.
  Packed deliveries (`.gz`, `.zip`) must be unpacked beforehand.
- **One terrain builder behind a mutex:** the DGM cache inside it is shared state, so tile
  builds run one after another even though they sit on the task pool. One source per
  worker if a single tile at a time turns out to be too slow.
- **The track ribbon is not streamed** — one mesh per edge at startup. A whole 100 km line
  costs a few hundred thousand vertices there; only when the ballast bed gets sleepers and
  a texture does the same tile logic have to be applied to it.
- **No Bevy `AssetLoader` for tiles** (plan 4.3 suggests one): terrain is computed, not
  loaded, and the task pool does that without a detour through an asset path.
- **Transition curve length and cant come from the rulebook**, not from the data: neither can be
  recovered from a noisy point sequence (the section boundary is uncertain by more than a
  hundred metres). Where the source deviates from this, the reconstruction deviates by a few
  metres — the same order of magnitude as the positional accuracy of OSM itself.
- **No routing across switches** when chaining, and station areas with multiple tracks are not
  separated.
- **Mod hooks run once per frame**, not once per simulation step — a Lua call every 5 ms per
  train for behaviour that reacts in tenths of a second would be pure overhead. A hook that
  genuinely has to see every step moves into `Sim::step`.
- **A modded signal announces one step late** if the signal ahead of it is modded as well: the
  rule table sees the following signal's built-in aspect from the same update. At 200 Hz the
  difference is 5 ms; evaluating the rules in signalling order would remove it.

## Sensible next steps

1. **Route editor with tools**: so far it only displays. Next up: drawing alignments,
   placing switches and signals, positioning platforms — the aerial image behind it is the
   template for that.
2. **Signal models**: the lamp images of a modded signal are strings without geometry —
   signals are still drawn as plain devices. The same path as for vehicles (glTF plus a
   binding table) is missing.
3. **Import a real pilot line** (Overpass extract + DGM1 from a state surveying office).
4. **Switch catalogue**: standard switches (EW 190-1:9 … EW 1200-1:18.5) as a data table with
   radius, branch length and diverging speed; OSM only supplies a `railway=switch` node
   without any geometry.
5. **Carry over OSM equipment**: signals, platforms, stopping points and level crossings —
   then the import directly yields an equipped line instead of a bare strand.
6. **Evaluate better sources** than OSM: the EU's RINF infrastructure register
   (speeds, gradients, train protection, partly minimum radii) and DB's open geodata.
7. **Texturing/vegetation** — the terrain is single-coloured; splatting and instancing are missing.
8. **Audio (ch. 13)** — `sim-core` already emits the events.
9. **3D cab (M6)** — the biggest chunk.
