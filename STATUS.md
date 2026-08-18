# Implementation status against PLAN.md

As of 2026-08-17 · `cargo test --workspace`: **408 tests green** · clippy and fmt clean.

**This project is mod-first.** See [MODS.md](MODS.md) for how to create trains, signals and lines.

## Milestones

| M | Contents | Status |
|---|---|---|
| **M0** | Workspace, `world-coords` (ECEF f64 + floating origin) | **done** — acceptance test "300 km without jitter/jump" green |
| **M1** | `track-model`, procedural track rendering, streaming | **done** — graph, clothoids, `eval`, switches (incl. trailing moves), track meshes; terrain tiles stream in and out around camera and trains |
| **M2** | Longitudinal dynamics + brake, electric loco + coaches, basic cab | **done** — coasting against Davis, emergency braking distance, starting on a gradient, coupler slack as tests; brake and drive down to control valve, motor and torque converter; basic sounds (rolling, traction, air, compressor, horn, buzzer) |
| **M3** | Sifa + PZB 90, signals, editor v1 | **done** — Sifa (time-time, time-distance, RZM) and every intermittent build from the Indusi I 54 to the PZB 90 V2.0 complete with standard-case tests; H/V + Ks signal logic present, and **signal models render the lamp images**: modular glTF assemblies on mount points (Zusi pattern), lamp nodes switched by the current lamp image, placeholder mast with an aspect light for signals without a model; the **route editor** edits the line over the aerial imagery (editor v3 — arc-to-point track drawing, device placement and per-device fields, switch placement that splits the track and wires the turnout facing or trailing with its throw time in the panel, the signal/section/route tables of the interlocking as forms, support-point dragging, rule checking, module boundaries with a ghost neighbour, delete with index remapping, undo/redo, save/open with discard guards); the **vehicle editor** edits base data, drive/brake/equipment as a block diagram (see the 2026-08-17 entry), glTF model, LOD, moving parts, the 3D cab (eye point + interactive controls), the cab displays and the sound table; the **signal editor** assembles signal models (parts, mount points, lamp bindings, lamp test) |
| **M4** | Interlocking, AI trains, timetable | **done** — routes with locking/release, automatic block, AI stops at signals and platforms |
| **M5** | LZB 80 + AFB, MFA, tap-changer loco | **done** — LZB with guidance, braking curve, end and failure procedures, with and without PZB, full/partial block mode and CIR-ELKE; BR 110 present; **AFB** as vehicle equipment (`VehicleSpec::afb`): holds the dial speed with traction, dynamic brake and — where that does not suffice — the air brake, and under LZB guidance runs down the braking curve because the LZB's v-soll caps the dial; MFA values and lamps ship as indicators — HUD text, `gauge:`/`lamp:` instruments and render-to-texture displays in the 3D cab (see M6) |
| **M6** | Interactive 3D cab, start-up procedure, audio, weather/night | **done** — interactive 3D cab: per-vehicle cab data (eye point + controls binding glTF nodes to a closed input registry incl. wipers, lights and display softkeys), mouse picking with drag/click/scroll gestures per control kind, hover glow, HUD readout, operating clicks via `Control(…)` sound quantities; instruments: gauges/lamps of the safety systems (`gauge:`/`lamp:` indicators, MFA pointers), `digit:` seven-segment counters, and **displays rendered to texture** (declarative widget lists in RON, a Lua `display(ctx)` hook with nested menus and clickable softkeys, or an HTML/CSS/JS page per screen — parsed, flex-laid-out and scripted in-engine by the `html-display` crate, no browser embedded); edited in the vehicle editor with viewport preview; start-up chain operable via keyboard and mouse; **weather rendering**: `Sim::weather` (clear/rain/snow/fog) set by the `SetWeather` scenario action — overcast sky, dimmed sun, distance fog from the weather's visibility, rain/snow particle fields around the camera (their streaks slanted by the relative wind of the player's speed), a `Rain` sound quantity for the sound table, and the implied rail condition on every train (`mods/example/scenarios/regenfahrt.ron` shows it); terrain from the DGM; **day/night cycle**: scenario start clock (date + time), sun and moon computed from the georeferenced location, lighting/sky follow the sun's elevation; **seasons** (ch. 14 "seasons v2"): the same start date colours ground textures and placeholder vegetation — meadows turn through October, ground, gravel and foliage go under snow from November to March — and a mod may add optional `autumn_model`/`winter_model` variants to its track objects, falling back to the year-round model where it ships none; **night lighting**: signal lamps glow (HDR + bloom on the main camera, emissive lenses), headlight cones at both train ends follow the light switch, the direction of travel and the darkness, red tail lamps (Zg 101) mark the opposite end, **mods' `_NIGHT` nodes** (lit windows, glowing signs) switch at dusk in every model, cab light on its own switch (`CabControl::Headlights`/`CabLight`, keys 9/0) and **instrument backlighting on its own dimmer** (`CabControl::InstrumentLight`, keys `,`/`.`) — a part on the new `Motion::Emissive`, which scales the emissive colour of the mod's own material by the dimmer instead of switching the node, so the dials come up out of the dark continuously (content per vehicle; the example BR 101 carries a backlit panel); **terrain texturing and vegetation** (ch. 14): texture splatting — per-vertex weights from slope and track distance blend three generated ground textures (grass/rock/gravel) in a `StandardMaterial` extension — and vegetation as **line content**: every tree its own `LineSource::trees` entry (3D objects from mods' `objects/*.ron`, placeholder for the unnamed), spawned as children of their terrain tile so they stream with it and batch into instanced draws; woods are baked into single trees by the editor, so each one stays individually editable; no recorded samples (the sources are generated — content, not code) |
| **M7** | Pilot line from OSM/DGM, scenarios, scoring, save/load | **largely done** — scenario system, scoring, save/load and the OSM/DGM importer are in place; only a real pilot line is missing (data procurement) |
| **M8** | Mod runtime: declarative content plus Lua behaviour | **done** — loader with dependency order, vehicles/lines/compositions/scenarios/timetables/signal types/signal models/track types/track objects as RON, signal state machine as data, four Lua hooks (vehicle, signal aspect, line, scenario) with a sandbox, main menu with line, vehicle and scenario selection from the loaded mods (keyboard and mouse), mod manager on the same menu (a toggle applies on start) and under F9; reference mod under `mods/example` incl. a glTF model. Only distribution (`.crails` zip + installer) is still open |

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
  **Track types** (superstructure classes, `track_types/*.ron` in a mod): texture, color,
  roughness, superstructure speed limit and an LZB flag — assigned per edge as a step
  profile over `s`, so one edge changes its type section by section, with the reserved
  name `"default"` returning to the built-in type. The mod runtime resolves the names
  after compile (like signal types) and merges `max_speed` into the one speed profile AI,
  LZB, HUD and scoring already read; the app skins the ballast bed per section (texture
  via `mods://`, else the type color) and feeds `roughness` into the sound table as the
  `Roughness` quantity.
  **Track objects** (`objects/*.ron` in a mod): a 3D object plus the pose its author
  defined relative to the track — lateral offset, rotation about up, height. A line
  places instances at `(edge, s)`; each placement stores concrete values stamped from
  the object's defaults and editable per instance. The app spawns the glTF at the track
  pose (floating-origin safe like signal models, placeholder block for unknown names);
  the simulation reads none of it — objects are the line's furniture and follow edge
  deletion and splitting like devices.
- **Driving dynamics (ch. 6):** Davis, air resistance from cw·A, curve resistance after Röckl
  with its own factor, gradient resistance, couplers with slack and breaking force,
  Curtius/Kniffler adhesion with wheel slip/slide, sanding. Wheel slip protection in three
  kinds: wheel slip brake, traction cutback and electronic creep control — the last one holds
  the creep at the maximum of the adhesion curve and therefore gets more out of the rail than
  the other two.
- **Brake (ch. 7):** brake pipe as a node chain, one control valve **per vehicle** — the whole
  train is simulated wagon by wagon, not as one braking force. Control valve types K-GP, KE-GP,
  KE-GPR, KE-Tm, KE-L2a and KE-L2d as presets over their observable behaviour: graduated or
  single release, R position, second cylinder pressure stage by speed or by a full application,
  release button of a loco valve. Friction pairings for cast iron, disc, K and LL blocks and
  the magnetic track brake, each as a family of two curves interpolated over the axle load,
  plus an own characteristic as a table. Load braking (Lastabbremsung) in both builds: the
  stepless weighing valve, which throttles the cylinder pressure so that the braked weight
  percentage stays put however full the vehicle is, and the empty/loaded changeover lever,
  which moves the rigging at the changeover mass — braked weight and brake sheet follow the
  load. Equalising device
  (deliberately without a memory), pre-controlled cylinder through a relay valve, electrically
  transmitted (ep) brake, air supplement brake behind the dynamic brake, direct brake on every
  powered vehicle, spring-applied parking brake, magnetic track brake. Air is accounted for:
  reservoir volumes, main reservoir with a pressure-switched compressor, leakage and a running
  consumption figure in normal litres.
- **Electrics and drive (ch. 8):** start-up chain, pantograph travel time, main switch drop-out
  in neutral sections, diesel engine cranking. Four drive models, each optionally with the
  detailed data of the data sheet: the simplified tractive effort curve; series-wound motors
  behind a tap changer, computed from the machine equations with saturation, current limit and
  field weakening; three-phase drive with a pull-out range and a regenerative brake that dies
  in a neutral section; diesel-hydraulic with an engine map, a speed governor with droop or a
  fill governor, and a transmission of torque converters and fluid couplings that are engaged
  by filling them — separate filling and emptying times, so the outgoing circuit lets go
  before the incoming one takes hold and the change point tears its hole in the tractive
  effort; change points with hysteresis and primary influence, so the change speed depends on
  the notch and not on speed alone; filling from on/off through partial stages to
  quasi-continuous. Engine and pump find their working point against each other every time
  step. Dynamic brakes come from the drive model: rheostatic, regenerative or hydrodynamic
  (retarder).
- **Sound (ch. 13):** a **declarative sound table in the vehicle file**, after the model
  Zusi uses. An entry has three parts: a **trigger** (what starts it — `Loop` means *no*
  trigger, the continuously modulated case), **conditions** (state predicates that mute it
  or release it) and **dependencies** (curves with support points that map a quantity onto
  volume and playback speed). The mapping quantity → volume/pitch is therefore data, not
  code: a rolling noise and a contactor click are the same mechanism, the click with a
  trigger and no loop, the rolling noise the other way round. `factors` are further curves
  multiplied into the volume — the rolling entry carries one on the `Roughness` quantity,
  the roughness of the track type under the vehicle, so jointed superstructure is audibly
  louder than welded rail.
  **`sim-core` hands out no sound events.** It exports a named set of state quantities
  (`sound::SoundState`: speed, distance, engine speed, tap changer notch, converter circuit,
  tractive and brake effort, pipe and cylinder, air flow, slip, power controller, pantograph,
  main switch, compressor, doors, horn, protection alert); edge detection on them *is* the
  trigger, and it runs in the app. A tap changer notch is a number whose crossing fires the
  contactor, brake squeal a condition (speed window ∧ brake force) on a loop. Rail joints,
  the one noise that does not follow from the vehicle state, come out of a distance interval.
  Entries marked `positional` are placed on the vehicle — distance attenuation and Doppler,
  so **other trains are audible**; the buzzer and the rest of the desk stay unplaced.
  While the camera sits in the cab, placed sounds pass a one-pole lowpass — the **cab
  wall**; Bevy's audio has no filter graph, so the filter sits in the audio decoder
  (`audio::Exterior`), its cutoff steered by the camera mode over an atomic. The outside
  cameras hear the full spectrum, the desk sounds are never filtered.
  A vehicle without a table of its own runs on the generated default (`sound::default_table`):
  rolling, traction split into an electric and a diesel entry, air, compressor, horn, buzzer,
  rail joints and tap changer contactors. Their samples are **generated at startup**, a few
  oscillators and a noise generator written into a WAV buffer, so the repository carries no
  samples — a mod's own files take the same path, only the `file` of the entry changes.
  The table is edited in the vehicle editor (Sounds section) and documented in
  [MODS.md](MODS.md). Volumes fade rather than jump.
- **Train protection (ch. 9):** trait abstraction + country package DE with all intermittent
  builds, LZB and three Sifa builds:
  - **Indusi/PZB** as one state machine plus a parameter set per build — **I 54**, **I 60**,
    **I 60M**, **I 60R**, **ÖBB PZB 60**, **PZB 90 V1.5** and **V2.0**. **Every build carries
    the check speeds of its own time**: the Indusi builds supervise 95/75/60 km/h after
    20/26/34 s, the I 54 the 95/90/80 km/h it was set to by vehicle maximum speed, and only
    the PZB 90 harmonised the figures down to 85/70/55 km/h — the same loco therefore runs
    10 km/h faster past a 1000 Hz magnet before it was rebuilt. The older builds supervise the
    check speed only once the 1000 Hz time has run and hold the 500 Hz speed constant; the
    I 60R, being computer-controlled, already supervises a falling curve, but onto the I 60
    check speed. The distance supervision (1250 m), the restrictive mode and the supervised
    override came with the I 60R and the PZB 90. V1.5 enters the restrictive mode only after
    a stop and holds 45 km/h there, V2.0 also after 15 s below 10 km/h and falls to 25 km/h.
    Train categories O/M/U, acknowledgement, exemption from 700 m, override 40 and the forced
    braking logic apply throughout.
  - **LZB 80/I 80** with and without PZB, **CIR-ELKE** (steeper braking curve, 5 km/h speed
    steps, speed rises effective at the head of the train instead of at its rear), takeover,
    v-target/v-goal/distance to target, end and failure procedures.
    The **movement authority comes from an LZB centre** that builds it afresh every step out
    of the line data and the state of the interlocking: it runs to the first block boundary
    that is not clear — a block marker whose section is occupied, or a main signal at stop —
    so a signal going to stop ahead of the train shortens it at once. v-target is the most
    restrictive point in the next 12 km, a speed restriction of the line as much as a stop.
    The **block division is a line datum**, the `BlockMarker` devices of the route; full and
    partial block mode are what falls out of it, and with them whether the signals stay
    binding and their magnets remain the fallback level.
    The **braking curve is the train's own**: it follows the braked weight percentage (BRH),
    the brake position (BRA) through the build-up time of the brake, and the initial braking
    speed, above 150 km/h with the falling deceleration of the brake tables. An ICE at
    180 BRH therefore runs a curve a freight train at 65 BRH in G never gets. The curve only
    supervises — the braking itself stays physical, so a train braked too weakly for its
    movement authority simply cannot hold it.
  - **Function test** (Funktionsprüfung) of the PZB and the LZB: lamp test → internal test →
    acknowledgement, at a standstill only; the PZB holds the forced braking until it passes.
    Switching the battery on starts it.
  - **Sifa** in three builds: time-time, time-distance (30 s **or** 1250 m) and RZM
    (time-distance plus a minimum interval between operations).
  - **AFB** (ch. 9.4 — a vehicle feature, not a train protection system): target speed
    controller, fitted per vehicle (`VehicleSpec::afb`). It drives the power controller
    and brakes as the prototype does: the dynamic brake is preferential, and the air
    brake blends in through the driver's brake valve once the dynamic brake does not
    suffice — immediately on a train whose drive has none. A brake application by the
    driver overrides it (traction cut, and the AFB never releases what the driver
    applied). Under LZB guidance the LZB's v-soll caps the dial, so the train runs down
    the braking curve by itself; forced braking still wins, exactly as it does against
    the driver's levers. The example script `afb.lua` replaces it for the modded BR 101,
    which leaves the flag off.
- **Door control (ch. 9.5a):** **TB0**, **TAV** and **UIC-WTB**, taken from the leading
  vehicle's equipment (`VehicleSpec::doors`, like `VehicleSpec::safety` for the train
  protection). Release only at a
  standstill and per side, traction interlock until every door is closed and locked, and an
  unlocked door above 5 km/h applies the emergency brake. TB0 needs the driver's close
  button, TAV closes by itself after the boarding time, UIC-WTB is TAV over the train bus:
  after a consist change the bus is inaugurated first, and a command needs one bus cycle per
  vehicle to reach the rear.
- **Interlocking (ch. 10):** signal aspects, distant signalling, automatic block, routes,
  signal-dependent magnet activation. **Flank protection** (Ril 819) is what a route
  carries against movements from the side: `Route::flank` lists **protecting turnouts**
  (`FlankGuard::Switch`), which are commanded and locked exactly like the ones in the
  path, and **protecting signals** (`FlankGuard::Signal`), which are held at stop for as
  long as the route is set — a counter per signal, so two routes may lean on the same
  one. The hold works both ways: a held signal shows stop and reports itself as not
  clear to a mod's rule table, and no route can be cleared from it; conversely a signal
  another route already runs from cannot be taken as protection, so the request fails
  instead of clearing two conflicting moves.
  A **track lock** (Gleissperre) is a signal variant, `SignalKind::TrackLock`: two states,
  stop = laid on, and everything else falls out of what a signal already has — the
  interlocking lays it off for a route over it and holds it on as flank protection, the
  aspect comes from a mod's signal type and the look from its signal model, where a
  `motions` binding swings the shoe between its two positions. No route ends at one and
  none starts there; without a model the app draws the shoe itself in the aspect colour
  rather than a mast. `mods/example/signals/gleissperre.ron` is the two-rule table.
- **AI (ch. 11):** look-ahead across the track graph, braking curve with reaction and
  response distance, timetable stops, operates Sifa and PZB itself.
- **Scenarios (ch. 11.4):** RON events with triggers (time, train position, stop, speed,
  signal aspect, emergency brake application, chaining with delay, `All`/`Any`)
  and actions (message, announcement, switch, route, weather, points, scenario end).
- **Scoring (ch. 11):** timetable adherence, stopping accuracy, emergency brake applications,
  speed limit violations and traction energy → itemised score. A timetable is either
  `Scenario` (times from the start of the run, runs once) or `Daily` (seconds since
  midnight, wrapping around every 24 h — delay, departure and the AI's stop list wrap
  with it).
- **Alignment (ch. 15):** design elements are reconstructed from the point sequence —
  section separation, radius averaging over the whole curve (Kåsa), rounding to standard radii
  with tolerance, transition curves and cant per the rulebook (`c = 11.8·v²/R` minus
  cant deficiency, capped, ramp 1:10·v). Direction is estimated via best-fit lines in a sliding
  window, not from neighbouring differences — otherwise point noise completely masks the
  curvature. Acceptance: an alignment designed to the rulebook is recovered with the exact
  radius, correct cant and < 6 m positional deviation, even with ±2 m noise on the
  support points.
- **Line modules (ch. 15, after the Zusi 3 model):** a module is a `LineSource` with
  named `boundaries` — `Buffer` nodes at the open ends. A `Composition`
  (`compositions/*.ron` in a mod) chains modules into one line: every index space is
  shifted by the module's offset (including the indices inside magnet, signal and block
  marker payloads), and boundaries that lie at the same geo position are fused into one
  joint automatically — the georeference *is* the connection, so a module transition is
  nothing more than starting an edge at the neighbour's agreed coordinates. Explicit
  `connections` cover the rest. Several versions of a module (epochs, rebuilds) are
  several files; the composition picks one by name. A composition is addressed like a
  line (`--line example:gesamtstrecke`), and a scenario can name its line or composition
  itself (`Scenario::line`) — the way a timetable chains modules. `signal_links` on the
  composition set a signal's `next` across the boundary, so the last signal of one
  module announces the first of the next within the same update. Timetables and
  scenarios address positions **module-locally**: a `module` on the file (or per stop /
  per event) makes every index mean "of that module", and the mod runtime resolves them
  against the composition's offsets — no offset arithmetic in content files. **UTM
  zones cannot shift a transition:** module anchors are geodetic and the world is ECEF, so modules
  whose data came through different zones meet to the millimetre — the zone-seam
  displacement known from Zusi has no source here (pinned by a test at the 32/33
  boundary).
- **Import (ch. 15):** Overpass JSON → way chain → alignment → `LineSource`. From OSM come
  geometry, `maxspeed` and `name`; DGM tiles (XYZ or ESRI ASCII Grid) from a single file or
  an entire directory supply the gradient profile. Tiles are loaded lazily (sheet boundaries
  from the file name) and kept in an LRU, so even a federal state's DGM1 is usable.
  CLI: `import-line`.
- **Terrain (ch. 14):** 512 m tiles only within the line corridor, grid spacing by distance
  from the track (4 m to 32 m instead of 1 m), skirts against LOD cracks, cutting/embankment
  at the track — the ground there is the **formation**, `rail_offset` (40 cm) below the top
  of rail, so the ballast bed lies on it instead of inside it — view distance limit per LOD
  level in the app, built while driving (see streaming above). **Texturing/vegetation:** every tile carries per-vertex splat
  weights (gravel on the strip the track flattens, rock on steep ground, grass
  elsewhere) and the line's trees — every tree an own `trees:` entry, its foot
  on the tile's height grid. Woods come out of the editor's forest brush and
  forest import, which **bake** polygons into single trees
  (`terrain::fill_polygon`: deterministic, one per `area_per_tree` m², clear of
  the track strip) — one primitive, so any tree of a wood is moved or deleted
  like a hand-set one. Trees are 3D objects from mods (`objects/*.ron`; empty
  name = generated placeholder tree). The app blends three generated ground
  textures by the weights in a `StandardMaterial` extension shader and spawns
  the trees as children of the tile — shared assets per species, so they render
  as instanced draws and stream with the tile. Track objects can opt into
  `snap_to_terrain`: the base moves from the rail plane onto the terrain
  surface (`TerrainBuilder::surface_height`). **One elevation source per UTM zone**: `--dgm`/`--epsg` may be
  repeated, and a line across the 12° zone boundary takes each height from the first
  source that has one — the tile grid stays in the first zone, which is only a
  partitioning and continues past the boundary without a seam.
- **Aerial imagery overlay (ch. 15):** dedicated `imagery` crate with Web Mercator tile maths,
  provider configuration (tile template or WMS, placeholders, keys, zoom limits,
  attribution) and a two-level cache (memory + disk, budget with eviction of the oldest
  tiles, maximum age, offline mode). Fetching and decoding run on worker threads, the editor
  places the tiles georeferenced beneath the track ribbon.
  All of it controllable through a RON file, reloadable at runtime.
- **Editors (ch. 15):** three separate programs with a desktop UI (menu bar, docked panels,
  native file dialogs): `route-editor` edits a line over the aerial imagery overlay
  (editor v3): a **track drawing tool** that appends one
  tangent-continuous arc or straight per click (arc-to-point, G1 by construction), a
  **device tool** that drops any `DeviceKind` onto the nearest track, and a selection
  panel with the device's fields (kind, position, facing, lateral offset, RON payload).
  A **switch tool** clicks a point on a track and draws the branch: on finish the edge
  is split at the cut (`LineSource::split_edge` — devices, profiles, sections, switch
  legs and followers all follow), the joint becomes the turnout node and the branch its
  diverging leg, tangential by construction. The tool places **facing or trailing**
  turnouts: trailing reverses the drawing heading and makes the far half of the split
  the root, so a train over the clicked track trails the points instead of facing them.
  The **throw time** is edited on the selected track, which names the node and says
  whether that track is its root, straight or diverging leg — the map has no node
  picking, and every leg of a switch is a track.
  The **interlocking tables are forms**, not RON text: a placed `Signal` device carries
  its `SignalSource` entry in the selection panel (kind, system, the signal it announces,
  guarded sections, `requires_route`, diverging speed, signal type and model override),
  **the routes that start at it** — the list a signal carries in the Zusi editor: where
  each one ends and what it locks, a jump into its fields, and **Find routes**
  (`LineSource::routes_from`), which runs out over the track and offers one route per leg
  of every turnout ahead, each ending at the next signal on it (existing routes are left
  alone, distant signals start none).
  An **Interlocking section** holds the occupancy sections (tracks added from the
  selection) and the routes (entry/exit signal, sections, overlap, required switch
  positions, flank protection, diverging flag). **Derive path** fills a route in from the
  geometry:
  `LineSource::route_between` runs a breadth-first search over `(edge, direction)`
  from the entry signal to the exit signal — the entry signal's own edge stays out of
  the sections (that is where the waiting train stands), every turnout on the way
  reports the position its leg needs, and a path over a diverging leg marks the route
  as one. The **overlap** is walked on behind the exit signal and comes out at the
  regular length of the rulebook for the speed the route ends at (`regular_overlap`:
  50 m to 30 km/h, 100 m to 60, 200 m to 100, 300 m above — a diverging route counts
  with the entry signal's Zs3 speed); the panel switches that off for a length of its
  own, the sections it reaches become the overlap and the turnouts inside it are locked
  with the route. **Flank protection** comes out of the same walk (see the interlocking
  above): every turnout the route trails contributes the guard that covers the leg it
  does not use — a signal to be held at stop, or a turnout to be laid away — and the
  panel edits them as chips, offering only signals that can actually hold a movement.
  The row under the mouse is **drawn on the map**: the tracks of its sections
  in green, the overlap in orange, its switches and its two signals as circles and its
  flank protection in violet, so an index list can be checked against the line
  instead of read. Deleting an entry remaps
  what pointed at it —
  `remove_signal` clears `next` links, drops routes over that signal and rewrites the
  magnet payloads that named it; `remove_section` moves guarded lists, route sections
  and overlaps, and empties the block marker payload of a marker that loses its section
  rather than letting it mark the next one. **Support points are draggable**: the
  selected edge shows a handle per segment boundary, a drag refits the whole chain
  arc-to-point through the untouched points (edges with transition curves offer no
  handles — a refit would flatten them). A **rule check** (`LineSource::check`, run on
  every rebuild) lists wiring that compiles but fails on the line — distant signal
  without linked 1000 Hz magnet, main without 2000 Hz, distant without `next`, device
  beyond its track, broken magnet/block-marker payloads, boundary on a non-buffer —
  each finding with a jump-to button. The **module panel** places `boundaries` on the
  open ends of the selected track and loads a neighbour module as a grey **ghost**:
  read-only track plus its boundaries as snap targets, so drawing clicks land exactly
  on the agreed coordinates. **Track types are edited per section** in the selection
  panel — `(s, type)` rows with a color chip each, the map tints the ribbon per section
  in the same palette, and the type combo lists every installed mod's types. An
  **object tool** (key 5) drops any installed mod's 3D object (`objects/*.ron`) onto the
  nearest track at the object's own default offset and rotation; the selection panel
  edits position, lateral offset, rotation and height per placed instance, and
  **Repeat in a row** stamps copies along the track (spacing, default 65 m; end
  position) — the Zusi editor function "insert one every x metres", each copy an
  ordinary instance that can be moved or deleted on its own. **Vegetation tools**:
  a tree tool (key 6) plants single trees free of the track, a forest brush
  (key 7) outlines a polygon and bakes it into single trees (species and density
  in the tool options), and **File ▸ Import forest** reads an Overpass extract's
  `landuse=forest`/`natural=wood` ways and bakes them the same way — an optional
  aid next to hand placement, and every baked tree stays individually editable
  and deletable. A **marking brush** (key 8) sweeps over the map, marks trees
  and objects in bulk and deletes them together in one undo step. Objects offer
  **snap to terrain** (base on the terrain surface instead of the rail plane).
  **Height data travels with the module** (`LineSource::heights`): the DGM panel
  cuts the state survey office's delivery down to one ESRI ASCII grid per
  terrain tile (`<mod>/heights/<line>/x<kx>_y<ky>.asc`, `HeightTile::sample` +
  `to_asc`, grid spacing of the module's choosing, default 10 m) — either the
  whole corridor or the tiles the **DGM tiles** tool picked on the map, which
  draws the tile grid with its coverage (green has heights, blue is picked).
  Tiles the delivery has no data for are skipped. The app loads them behind any
  `--dgm` source, so a module runs self-contained while whoever holds the
  original delivery keeps its finer grid.
  A **terrain brush** (key 0) shapes the ground itself: every click stamps a
  round stroke into `LineSource::terrain` — `Raise(±m)` on top of the DGM, or
  `Level(height)`, which takes its target from the nearest rail. Strokes are
  data, not a baked heightfield (pickable, re-dialled, deleted; the DGM stays
  untouched, so better elevation data can be re-imported without losing the
  shaping), they apply in file order, fade out with a smoothstep at their
  radius, and are prefiltered per tile in `TerrainEdits::in_rect`. They act on
  the ground **before** the cutting/embankment blend, so no stroke can lift the
  track out of its alignment (pinned by a test). The map draws each stroke's
  true footprint — warm raising, cold lowering, grey levelling.
  **The editor shows the world it builds** (`terrain.rs`, `signals.rs`): `T`
  switches the map for the run's own picture — the same `TerrainBuilder`, mesh,
  splat material and ground textures, the **track as ballast bed and rails**
  skinned per track type, the line's **trees and scenery objects** as the mods'
  glTF at the placement's own pose (placeholder trees for unnamed ones, objects
  that ask for it on the terrain surface), and the **signal assemblies** on
  their mount points. The shared `world-render` crate is that code, used by
  both programs, so a stroke, a wood, a signal box or a signal mast is judged
  where it is set instead of only in the run. Tiles are
  built on the task pool around the view point (radius from the view height,
  capped at 64 tiles); the standing builder takes an edit over without
  re-indexing the DGM, and the old tile stays until its replacement arrives.
  Terrain and aerial imagery are the same ground layer, so only one of them is
  drawn: `T` (View ▸ Show terrain) switches, and a module that brings height
  data starts on its terrain. The status bar reads out the **ground height
  under the cursor** — brush strokes and cutting/embankment included —
  whichever layer is shown. The hill shading comes from a directional light
  from the north-west at 35°; the editing aids (device markers, gizmos) stay
  unlit above the world. Trees hang on their terrain tile as in the run, so
  they come and go with it. **Signals stand at stop**: the editor runs no
  interlocking, so the lamp image is the one its type's rule table gives for an
  untouched situation — what a line shows before the first route is set — and
  the finest LOD is drawn whatever the view height, where the run would long
  have switched the model off.
  **Reference markers** (`LineSource::markers`) are the drawing aids for a
  hand-built line: a labelled point in a freely named layer, set one per click
  with the marker tool (key 9) or imported from an Overpass extract
  (**File ▸ Import reference markers**), which sorts the tags it knows into
  layers of their own (level crossings, platforms, stations, signals, switches,
  buffer stops, kilometre marks, bridges, tunnels, towers; a way becomes its
  midpoint). The marker panel lists the layers with their count and hides,
  centres on or deletes them layer by layer — a hidden layer is unpickable too.
  Nothing in the simulation reads a marker: it says where something belongs,
  it is not the thing.
  Deleting an edge or device **remaps every index in the file** — devices, signals,
  `next` links, routes, sections and switch legs follow, and an edge that continued
  from a removed one is re-anchored geographically first (tested in `content::route`).
  Undo/redo snapshots one step per interaction; New/Open/Save/Quit and the window's
  close button go through the discard and comment-loss guards of the vehicle editor.
  The panel follows the editor design system (sticky header with the line name, jump
  bar, collapsible sections in editing order); the imagery template is edited in
  place (provider, opacity, zoom mode, offset, offline) instead of via letter keys
  alone; the map pans with the middle mouse button and zooms with the wheel (tools on
  1/2/3/4); device payloads come from one-click RON templates serialised from the
  `sim-core` types;
  `vehicle-editor` edits the vehicle base data (LÜP, gauge,
  v max, mass, rotating mass, axle base sum, rolling and air resistance, tilt angle,
  hunting, payload, curve resistance factor), the complete brake equipment and the drive
  **as a block diagram** (see the 2026-08-17 entry below — control valve, friction
  pairing, load braking, additional brakes, air data and slip protection, engine map,
  converter circuits, motor data as block parameters) and the
  **sound table** — one card per entry with its trigger, its conditions and its dependency
  curves, each curve a sparkline over its support points that opens the shared modal
  curve editor (draggable points plus an exact-value table). It also imports glTF
  models, reads their levels of detail from the node names
  and binds moving parts — either through name prefixes, through the Blender custom
  property `ts_function`, or by hand from the node list. The viewport shows one level at a
  time against a reference body of the length over buffers.
- **Localisation:** every string the user reads goes through the `i18n` crate
  (Fluent `.ftl`, English source plus German), including the simulator HUD, both editors
  and the scoring report. Language from `TRAINSIM_LANG` or the operating system,
  switchable at runtime under View → Language; a test fails on a key that only one
  language has. Crowdin config in `crowdin.yml`. Text out of the mods (scenario
  messages, station names) is content, not code, and is not translated.
- **Vehicle models in the app (ch. 15.3):** a vehicle with a model gets its glTF instead of
  the placeholder body; the level of detail follows the camera distance, and the bound parts
  follow the simulation (pantograph, gauges, switches, lamps). `--camera outside` starts on
  the external camera.
- **Signal models (ch. 15.3, after the Zusi assembly pattern):** a `SignalModel`
  (`signal_models/*.ron` in a mod) is a list of glTF **parts chained by mount points**
  (empty nodes `mp_*`), so masts, screens and indicators are shared files, plus a binding
  table lamp-image string → glTF node. The app spawns the assembly at the device pose
  (plumb mast, front towards the approaching driver, floating-origin safe) and shows a
  lamp node exactly while its string is in the signal's current lamp image — Zs3 digits
  and the script-lit Zs1 included. **Semaphore signals** come out of the same strings:
  `motions` bindings make a node *travel* (rotate/translate over a travel time) while
  its string is in the lamp image — the example line's Form signal swings its arms
  through the real intermediate positions. Optional **`lods`** switch `_LOD<n>` nodes
  by camera distance, like vehicles. The model comes from the signal type's `model`
  default or a per-placement override; a signal without one gets a placeholder mast
  whose light follows the aspect, so every line shows its signals. The **signal editor**
  (`signal-editor`, third desktop program on `editor-ui`) assembles the parts, offers
  mount points and lamp nodes from the loaded files (`mp_*`/`lamp_*` conventions,
  suggestions included), guards against mount cycles, edits motions and LOD distances,
  and lights any lamp image in its preview — arms swing there exactly as in the run.
- **Mod runtime (ch. 19):** `mods/<id>/` with manifest, vehicles, lines, compositions,
  scenarios, timetables, signal types, track types, track objects
  and scripts; everything addressed as `"<mod>:<file>"`, loaded in dependency order, broken files
  are warnings instead of crashes. **Data and behaviour are separate:** the vehicle description
  stays RON, only behaviour is Lua. Signals are a declarative state machine (situation → aspect +
  lamp image) with an optional script hook for what a table cannot express; the rules are
  evaluated in signalling order, so a signal announces its follower's final aspect from the
  same update. Lua runs sandboxed
  (`table`/`string`/`math` only) via `mlua`; a failing script is switched off, not fatal.
  `mods/` is an asset source, so models, textures and sounds of a mod are addressed as
  `mods://<mod>/assets/…`. The app takes `--line`, `--loco` and `--scenario <mod>:<name>`.
  A scenario references its timetable (`timetable: Some("<mod>:<name>")`) and gets stop
  scoring with it; without one only the scenario points count.
  Lines and scenarios have their own hooks (`on_load`, `on_frame`): the script decides *when*
  an event fires, the actions of that event stay declarative RON — an event with
  `trigger: Never` waits for the script. **The main menu picks line, vehicle and scenario**
  from the loaded mods — three list pages, each opening with the built-in default, so a
  run starts even with nothing installed; lines and compositions share one list, since
  `resolve_line` takes either name. **The mod manager lives on the same menu** (installed
  mods with version, on/off state, missing dependencies and the loading warnings);
  switching writes `enabled` back into `mod.ron` (that one field only) and takes effect
  when the run starts, because the world is built only on leaving the menu. F9 opens the
  same list in the simulator, where a toggle needs a restart. **The settings section**
  sits next to it: view distance, shadows, bloom, fullscreen, vertical sync, master
  volume, language, HUD and look sensitivity, kept between runs as TOML in the operating
  system's settings directory (Bevy's own `bevy::settings`, `crates/app/src/settings.rs`).
  **Every one of them applies the moment it is changed** — `apply_scene` moves the
  streamer's load radius and the view distance while tiles are in the air and adds or
  removes `Bloom` on the live camera, `apply_window` carries fullscreen and vertical sync
  onto the window, and language, volume, HUD and look sensitivity are re-read where they
  are used. Nothing waits for a restart; a setting that needs one is an excuse. **Esc
  during a run raises the same menu as an overlay** (`GameState::Paused`, `spawn_pause`):
  no camera of its own — the cab's draws the UI — no wallpaper, a thinner scrim so the
  world stays recognisable, and Resume / Settings / Quit. Every driving system is gated on
  `Driving`, so the pause freezes simulation, clock and camera by itself. The overlay's
  settings page is the front end's minus the language and the reset. Going back to the
  title screen is **not** offered there: the world `setup` builds carries no despawn
  marker, so tearing it down again is its own piece of work.
  Any run flag on the command line (`--line`, `--frames`, …) skips the menu, so CLI and CI
  invocations stay non-interactive, and a flag beats the menu's choice where both are set;
  `--menu` puts the menu back in front, which is the only way to photograph it.
- **Cross-cutting (ch. 16):** fixed time step, seeded RNG, state hash with determinism test,
  full serialisation for save/load.
- **Block diagram (2026-08-17, `sim_core::blocks` + vehicle editor):** drive, brake and
  equipment of a vehicle as a **node graph of connected blocks**. A palette of 37 built-in
  blocks — every physical component of the simulation, from pantograph, transformer,
  tap changer, traction converter and motors through diesel engine, hydraulic
  transmission and retarder and the complete air brake down to wheelset, cab, AFB, Sifa,
  PZB, LZB, doors and the Lua script hook — wired over colour- and shape-coded port
  domains (shaft, force, electrical, pneumatic, signal, fuel; only like connects to
  like). The diagram is stored in the vehicle file (`VehicleSpec::graph`, optional) and
  **baked** on load and save (`blocks::bake`): the graph is authoritative for
  `traction`, `brake`, `safety`, `doors`, `passenger_doors`, `afb`, `slip_protection`,
  `axles`, `adhesive_mass_fraction` and `script`; every other field stays hand-edited,
  and a vehicle without a graph is untouched — the editor synthesises its diagram from
  the spec on open (`blocks::from_spec`) and writes it on save. The baker recognises the
  drive chains of all four traction models, diesel-electric included:
  `TractionSpec::Diesel` gained an optional, serde-defaulted `dynamic_brake` — the
  rheostatic brake of a Class 66/BR 232-style loco (a `regenerative` flag is ignored on
  a diesel, no line to feed into). Mods extend the palette with **presets**
  (`mods/<id>/blocks/*.ron`: a built-in `base` plus overridden parameter defaults,
  addressed as `<mod>:<id>`; an unknown base or a wrongly-typed parameter warns instead
  of crashing — `mods/example/blocks/voith-l620.ron`, a Voith L 620 reU2 on the
  hydraulic transmission, is the worked example). In the editor the centre toggles
  between 3D model and diagram (chips top left; `--graph` starts on the diagram); the
  former Brake/Drive/Equipment/Behaviour forms are replaced by the palette (searchable,
  grouped by category), per-block properties and **live bake findings** (a click selects
  the offending block), and axle count and adhesive mass moved onto the wheelset block.
- **Palette completed (2026-08-18):** the block system now covers every component the
  simulation has a model for — **69 built-in blocks** in nine groups, over nine port
  domains (shaft, force, electrical, pneumatic, signal, fuel, steam, water, heat). What
  came with it, physics first:
  - **Diesel-electric drive** (`DieselElectric`): generator, rectifier and a **load
    regulator** that holds the engine on the power the notch asks for by adjusting the
    excitation; the motors take whatever voltage and current that works out to, so the
    curve is constant power with the current limit taking over at the bottom and the
    voltage limit at the top. DC (`SeriesMotor`) or AC (`AsyncMotor`) behind it. The
    `BR 232` is the worked example — 353 kN and 2.2 MW off the works plate.
  - **Induction motor** (`AsyncMotor`, Kloss's equation): the three ranges of a modern
    tractive effort curve now come out of the machine instead of out of the `v_pullout`
    fudge — constant effort, constant power in the field-weakening range, and the
    pull-out torque bending it down with 1/v² at the top.
  - **Contactor drive** (`Starter`): starting resistors cut out step by step,
    series/series-parallel/parallel regrouping, or a chopper in place of the lot. Every
    regrouping is a step in the tractive effort curve, and the current limit relay is
    why the last few notches feel like nothing at all.
  - **Thermal model** (`Thermal`): one lumped mass with a cooling term on motors,
    starting resistors and the braking resistors, wired up with the `cooling` block. A
    rheostatic brake that is held long enough fades out, and a loco that has been
    slogging derates.
  - **Steam locomotive** (`sim_core::steam`, `TractionSpec::Steam`): coal on the grate,
    water in the boiler, steam above it, and the exhaust closing the loop back onto the
    draught. Mean effective pressure with expansion (so winding the cutoff back pays),
    safety valves, injectors that cost pressure, priming, low water. The `BR 52` is the
    worked example.
  - **Brake system**: vacuum brake as a second medium (`BrakeMedium`, same handle
    positions, its own numbers, exhauster instead of compressor), **EP brake**
    (`EpBrake`) with or without venting the pipe, **angle cocks and hoses** per vehicle
    end — a cock closed mid-train leaves everything behind it unbraked, one left open at
    the end will not let the train charge — limiting valve, retaining valve and the
    double check valve that was already implicit in `applied_cylinder`.
  - **Signal graph** (`sim_core::signal`): the control wiring between the physical
    blocks, compiled out of the Logic group by `bake` into a flat list of operations in
    topological order and evaluated once per step before the drive. Reading, constant,
    characteristic, combination, limiter, PID with anti-windup, notching, rate of change,
    switch with hysteresis, and an output that takes hold of the power controller, the
    brake, sanding, the blower or one of four free values for the cab displays. A cycle
    is reported and dropped rather than run.
  - **Running gear**: `bogie` and `axle` blocks refine the `wheelset` — drawn out, the
    axle count, the driven share and the axle base sum follow from the blocks, and so does
    the layout the physics runs on (see the per-axle entry below).
  - **On-board electrical system** (`PowerSupply`): the supply system the vehicle is
    built for (15/25 kV AC, 3/1.5 kV DC, third rail) — the main switch will not close on
    one it is not, a shoe needs no rise time — a **battery with a charge state** that the
    standing load drains and a running machine charges (a flat one cranks nothing and
    switches everything off), and a `voltage-source` that stands in for the contact line
    on a test rig.
  - **Emergency valve** (`emergency-valve`, `CabInputs::emergency_valve`): vents the pipe
    and the driver's valve cannot make it up. With it, the **brake pipe ends** became
    data: a vehicle whose pipe stops short of an end cannot pass the brake through.
  - **Sand rate**: `VehicleSpec::sand_rate`, the sander block's figure, scales the
    adhesion bonus — more sand helps, but not past about twice the reference rate.

  The HUD gained the generator, boiler, tender and temperature lines; both locales carry
  every new key.
- **Electrification is a line datum (2026-08-18, `track_model::power` + `sim_core`):**
  what hangs over the track is stated by the line, not assumed by the vehicle.
  `PowerSystem` (15/25 kV AC, 3/1.5 kV DC, third rail) lives in `track-model` so that both
  sides name the same thing; `TrackNetwork::default_electrification` is the line's own
  wire and `TrackEdge::electrification` overrides it section by section, so a line states
  it once and names only its exceptions — a gap at a system boundary, an unelectrified
  branch. `Sim::step_train` reads it at the pantograph into `TractionState::line_system`,
  and the main switch closes only where `PowerSupply::accepts` says the vehicle was built
  for it: **the volts alone no longer decide**, because 25 kV is plenty of volts for a
  15 kV loco and still the wrong system. Multi-system vehicles carry a list of systems —
  in the diagram, one `pantograph` block per system, as the real ones carry one head per
  system. The route editor edits the line's wire in its properties and per-track sections
  in the selection panel; lines saved before any of this read as the German main line, so
  what ran on them keeps running.
- **Per-axle running gear (2026-08-18, `sim_core::physics`):** the adhesion is worked out
  axle by axle. `AxleSpec` gives every axle its share of the weight and says whether it is
  driven; `AxleState` carries its own slip, tractive effort and brake force. A vehicle
  that states nothing gets the even layout its axle count and adhesive mass imply
  (`AxleSpec::layout`, exact for any fraction), so nothing calibrated against the lumped
  model moved — all 36 test binaries passed the refactor unchanged.

  What it buys is the thing a lumped model cannot have: **the leading axle runs on a
  dirtier rail than the ones behind it** (`physics::rail_cleaning` — the first axle wipes
  it as it goes, recovering over three or four axles, normalised against the vehicle's own
  load distribution so the total adhesive force is unchanged). So the leading axle spins
  first and the rest keep pulling, the wheel slide protection releases the sliding axle
  alone, and the wheel slip brake takes down the one wheelset that is spinning instead of
  the whole drive. `Vehicle::slip` stays the worst axle, which is what the HUD, the sound
  table and the scoring read; the HUD gained a running-gear line that appears only when an
  axle actually has something to say.

- **Track areas (2026-08-18, `content::route` + route editor):** a stretch of track
  marked by hand that carries properties — speed, cant, gradient, track type and
  electrification — instead of editing a step profile per property per track.
  `TrackAreaSource` has a name, a colour and a list of `AreaSpan`s (`[from, to)` on one
  track); every property is an `Option`, and what an area does not state it does not
  touch, so a speed restriction laid across an electrification boundary leaves the wire
  alone. `compile` lays them over the tracks' own profiles in file order (a later one
  wins) and bakes them down into the same step profiles as before — **nothing in the
  simulation knows they exist**, and there is no run-time cost.

  In the editor they are **painted**: **Mark area** is a brush — press on a track, drag
  along it, release. The stroke is projected onto the track it started on, so it follows
  that track even where the cursor wanders off it and never jumps to a neighbour halfway
  through a station; with an area selected the next stroke joins it, which is how one area
  comes to cover a whole station one track at a time. What it leaves behind is a **wide
  coloured quad over the rails** (`spawn_areas`, half transparent, above the track ribbon
  and a good deal wider than it), drawn all the time and not only while selected. The
  stroke width belongs to the area, so it survives a save; the brush setting is what new
  areas get. The live stroke under the cursor is a gizmo band, the selected area an accent
  outline around its stroke. A panel lists them, and a property panel of
  checkbox-plus-value pairs reads exactly like the file. The rule check flags an area that covers nothing, sets nothing, lies off its
  track or names an unknown type; the track panel says when an area lies over the track
  being edited. Areas follow their track through a split and a deletion.

## Deliberately deferred

Every simplification is marked with a `ponytail:` comment at the code site, with an upgrade path:

- **Brake pipe model** is a node/diffusion model, not a pressure wave.
- **Slip per vehicle**, not per wheelset.
- **Friction coefficients as a family of two closed curves per pairing** instead of Karwatzki's
  full pressure-dependent formula: one curve for a light vehicle (5 t per axle) and one for a
  loaded one (20 t), interpolated linearly over the axle load and held beyond the two. The block
  force per block decides the shape and is in no vehicle data sheet — the axle load stands in for
  it and is in every one. Only the shape follows the load; the level stays with the braked
  weight, which already carries the friction level of the vehicle. Where measurements exist,
  `BrakeKind::Custom` takes them as a table.
- **Control valve types are presets** over the observable behaviour, not a rebuild of the
  valve's internal chambers; every one of the parameters can be overridden per vehicle.
- **The empty/loaded lever always stands right**: its position follows the mass, whether the
  wagon changes over by hand or by a weighing valve of its own. A lever forgotten in the
  loaded position — the classic way to flat a wheel — needs a state of its own, and someone
  to set it while shunting.
- **Torque converters** with a linear µ(ν) up to the coupling point and a linear λ(ν), not a
  measured characteristic field — four numbers per circuit instead of two curves. The field
  cannot be read back out of a tractive effort curve anyway, so the numbers are fitted against
  the plot in the vehicle editor; the data sheets state the ends of both lines.
- **The LZB braking curve** is derived from the train, but with a straight line through the
  deceleration steps instead of the steps themselves: proportional to the braked weight
  percentage, falling off linearly above 150 km/h. The real steps of the LZB brake tables sit
  in DB Netz specifications that are handed out only on proven legitimate interest, so no
  simulator carries them in an open file. `Lzb80::deceleration` is the single place a table
  would replace. **CIR-ELKE II** — gradients up to 40 ‰ and kinked supervision with three
  speed-dependent deceleration values — is not modelled; where those three values coincide
  (a 140 km/h EMU on a CE-II line), the CIR-ELKE I curve covers the case.
- **The LZB centre reports one target**, the point ahead whose braking curve cuts deepest,
  picked with the reference deceleration rather than the train's own. Two targets whose curves
  cross inside the authority would need the vehicle to be given both — which is as much as the
  MFA has room for anyway.
- **Sifa RZM** is modelled as time-distance plus a minimum interval between operations, so
  beating the pedal continuously does not satisfy the device. Where the build differs in the
  detail, `SifaParams` is the place to correct it.
- **The I 54 is modelled in its state from 1959**, whose three check speeds followed the
  vehicle's maximum speed rather than the train category; the train type carries them here,
  because the two axes line up closely enough. The earlier rulebook, with only 95 and 75, is
  not modelled. **The ÖBB PZB 60** runs on the contemporary German Indusi set — no figures of
  its own are published. The tables sit in `PzbVariant::spec` as `const` values.
- **Rail joints come out of a distance interval** (`Trigger::Every` over `Distance`), not
  out of the track. In Zusi the route builder places them, with an editor function that
  inserts one every x metres — this is that function at run time. A `DeviceKind` on the edge
  replaces the interval as soon as joints are to sit where the track says they do; that is
  the one sound trigger which genuinely has to come from the line rather than from the
  vehicle state.
- **The generated sources are synthetic**, so the default table sounds like a simulator of
  1998. That is the sources, not the architecture: Zusi's sound designers solve a transition
  with several loops cross-faded by volume and pitch curve — the ICE 3 carries one file per
  semitone rather than pitching one loop across the range — and the table format here does
  exactly that, several entries with overlapping curves. What is missing is somebody's
  recordings, and those go in a mod, not in this repository.
- **The cab wall is one lowpass with one cutoff** for every vehicle; a per-vehicle
  insulation value moves into `VehicleSpec` when someone records real cabs and can hear
  the difference.
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
- **A turnout is placed from the track, never from the node** — the switch tool splits an
  edge, and the throw time is edited on the tracks that meet at the joint. Node picking on
  the map (and with it crossings, double slips and a switch drawn between two existing
  tracks) is a selection kind of its own.
- **A signal's routes end at the next signal** (`routes_from`) — that is what a route is,
  but it means the editor offers no route *over* an intermediate signal, and no shunting
  route that ends at a point rather than at a signal. Both are entered by hand: entry and
  exit are pickers over the whole signal table, and Derive path takes any pair.
- **Flank protection is derived only where a route trails a turnout** — that is where the
  leg it does not use joins the path. A facing turnout guards the route itself by lying
  in the position the route needs. Beyond the fork the search takes the first signal
  covering that leg, or the first turnout that can be laid away; where the leg opens into
  another turnout at its *root*, both legs beyond it would need protection and the search
  reports none — a station throat the builder answers for.
- **The derived path is the shortest one** (`route_between`, breadth-first with one visit
  per `(edge, direction)`), and its overlap runs straight on wherever the track forks —
  an overlap is the plain continuation of the route, not a second diverging move. A
  builder who wants the long way round, or another leg, edits the fields; the search
  fills them in, it does not own them.
- **The regular overlap is four steps** (`regular_overlap`: 50/100/200/300 m by the speed
  the route ends at), as the literature on German signalling states them — Ril 819
  "Durchrutschwege bemessen" is no more a public document than the LZB brake tables. A
  full table would replace that one function; until then the editor's own length is
  what a line with better knowledge uses.
- **Support-point dragging refits arcs only**: edges carrying transition curves show no
  handles, because the arc-to-point refit would flatten the clothoids. Re-fitting
  transitions around a moved point belongs to an alignment-aware pass (ch. 15 already
  has the reconstruction).
- **The track ribbon is not streamed** — one mesh per edge at startup. A whole 100 km line
  costs a few hundred thousand vertices there; only when the ballast bed gets sleepers and
  a texture does the same tile logic have to be applied to it.
- **Ground textures are generated noise**, not photographs — the same policy as the
  sound sources (content, not code). Authored textures go into a mod once terrain
  texturing becomes moddable content.
- **One entity per tree**; a per-instance buffer replaces it if someone wants real
  forest density. **Forest bakes cap at 10 000 trees per polygon** — every baked tree
  is a file row and part of every undo snapshot, so importing a whole state forest
  needs a compacter representation, not a bigger cap. **Forest import reads closed
  ways only** — multipolygon relations (forests with clearings) come in as their
  outer ways or not at all; the relation assembly joins the importer once a real
  line needs it. **The marking brush marks trees and scenery objects**, not devices —
  a swept-away magnet would silently break signal wiring.
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
- **The main menu is built from plain Bevy UI nodes** — no `egui` in the simulator. A
  **title screen** (wordmark over the backdrop and four verbs set large) and behind it a
  **full-width flow**: the three steps of picking a run stand in a numbered rail across the
  top with what was picked under each, the list sits left, and a detail pane on the right
  shows what the highlighted row actually is — length, permitted speed and signal count of
  a line, mass, running-gear limit, drive and brake of a vehicle, start time, timetable and
  event count of a scenario, all read off the same data the simulation runs on. There is
  deliberately **no navigation rail down the side**: a rail plus a content pane is the shape
  of a web dashboard and reads as one whatever it is coloured; the step rail is the
  breadcrumb, and Esc is the way home. Keyboard and mouse drive the same selection index:
  ↑/↓ or hover selects, Enter or a left click confirms, ←/→ dial a setting, Esc goes one
  step back. Every page is the same list of rows (leading slot, label, provenance chip,
  second line, control), so a new page is a `match` arm and nothing else. The rows are
  rebuilt whenever a fingerprint of what they show changes, rather than patched in place —
  a row is four nodes deep and differently shaped per page, and the menu is idle the rest
  of the time. A settings row draws its value as a pill, a filled track or a pair of
  chevrons, so nothing that can be changed looks like plain text.
- **The menu is monochrome, with traffic red as the only saturated colour.** Selection and
  focus are bone white on stepped grey surfaces; RAL 3020 appears twice, as the mark above
  the wordmark and on the button that starts something, and amber is kept for the two
  warnings (a setting that waits for the next run, a mod missing a dependency). A picture
  sits behind everything under a left-to-right wash that thins where nothing is written —
  compiled into the binary (`crates/app/images/`), and today a **placeholder that is not
  ours to distribute**.
- **The simulator draws prose in Fira Sans and machine output in Fira Mono**
  (`crates/app/fonts`, SIL OFL 1.1, compiled in) — the same family, so names and figures
  read as one typeface. Mono is not a preference: the HUD, the cab displays and the mod
  panel are laid out in columns, so it goes into the asset slot the empty `TextFont` handle
  points at and they get it without naming a font anywhere. That slot also replaces Bevy's
  built-in ASCII subset of the same face, which left every umlaut and arrow as a box. The
  menu asks for the two Fira Sans faces by handle on top of that, for everything that is a
  sentence rather than a measurement.
- **A composed line runs one script** — the composition's, or the single module script
  found; further module scripts are dropped with a note. Running every module's hook
  side by side needs a script list in the runtime, nothing in the format.
- **Boundary snapping is a constant** (`compose::SNAP_DISTANCE`, 1 m) — module edges are
  placed at agreed coordinates. A per-composition tolerance steps in when real survey
  data needs one.
- **The simulation clock counts seconds since the start of the run**; the wall clock
  comes from the scenario's `start` (date, local time, UTC offset — default midsummer
  noon) via `Sim::clock()`. It anchors `Daily` timetables (`Timetable::delay`/
  `next_occurrence` take the start-of-day offset) and drives sun and moon: positions
  from date, time and the georeferenced location (`world_coords::sun`, low-precision
  astronomy), so the season falls out of the date. **The seasonal appearance hangs off
  the same date** (`world_render::Season`, ch. 14 "seasons v2"): the generated ground
  textures and the placeholder trees are built in the colours of the start day — meadows
  turn through October, ground, gravel and foliage go under snow from November to March,
  a rock face keeps showing through. A mod may follow along without having to:
  `autumn_model`/`winter_model` on a track object name a glTF with its own textures for
  that season, each optional, missing ones falling back to the year-round `model` — a
  mast names neither, a birch may name both. Two ceilings: the curve is a central
  European lowland year (a line in the Alps or south of the equator wants a per-line
  climate entry, not a finer curve), and the season is **baked in at load** (a run from
  October into November keeps the ground it started on, and a variant either shows or it
  does not — cross-fading two glTFs would draw every tree twice). The route editor
  builds in summer; which season a run shows is the scenario's date.
- **Night furniture is a node name**: a glTF node ending in `_NIGHT`
  (`world_render::NIGHT_SUFFIX`) is shown below half daylight and hidden above it —
  lit windows, glowing signs, the light pool under a lamp, in every model the world is
  drawn from. Like the `_LOD<level>` convention it needs no entry in any RON, and the
  lit look stays in the mod's own emissive material. Two ceilings: it is a hard switch
  at dusk, not a fade (fading would mean patching every loaded glTF material per frame),
  and there is one threshold for the whole world — a shop that closes earlier than the
  street lamp wants a per-node schedule, which is a scenario question, not a render one.
- **A glowing part dims its whole material** (`Motion::Emissive`): every mesh below the
  node gets a clone of its material and its emissive is scaled as a whole, so a panel
  that is to dim in zones needs one node per zone. The value is linear in the dimmer, not
  the gamma-corrected curve a real rheostat gives — a per-vehicle curve belongs in the
  `Motion` entry once a vehicle's plot disagrees.

## Sensible next steps

1. **The track is drawn in the editor, not imported.** OSM's way chain is traced from aerial
   imagery and carries neither design elements nor a usable vertical alignment; over the
   imagery overlay the drawing tool gets closer with less effort. `import-line` stays as a
   starting strand and as a reference, but the pilot line comes out of the editor. What the
   import owes the module is the **surroundings** (2) and the elevation data the
   terrain is built from (the DGM panel cuts it into the module).
2. **OSM as scenery, layer by layer and opt-in.** "File ▸ Import forest…" is the pattern:
   pick an Overpass extract, get ordinary primitives that stay individually editable, use it
   or hand-place everything yourself. To be added the same way, in order of effort:
   - **Point features onto `TreeSource`** (object at lat/lon with yaw and scale — the
     primitive exists, only the name says "tree"): single trees, power towers, wind turbines,
     towers and steeples, lamps. Cheapest layer, no format change.
   - **Areas and bands onto a fourth splat channel** — `terrain::splat_weights` already
     carries `[grass, rock, gravel, 1.0]`, the fourth component is free. Water
     (`natural=water`, `waterway=riverbank`), roads (`highway=*` buffered by their width),
     farmland and meadow paint into it instead of becoming meshes: one texture in the
     extension shader, no water renderer, no road ribbon.
   - **Buildings** (`building=*`, `building:levels`): footprint extruded into a block model —
     the one layer that genuinely needs new geometry, and the biggest visual gain.
   - **Reference markers are in place** (see the editor above) — level crossings, platforms,
     stations, kilometre marks and the rest come out of an Overpass extract into layers that
     hide and delete as a whole. What they still lack is DB InfraGO's kilometrage line and its
     operational points (see 4); those come from CSV, not from Overpass, so the import needs a
     second reader.
3. **Switch catalogue**: standard switches (EW 190-1:9 … EW 1200-1:18.5) as a data table with
   radius, branch length and diverging speed; OSM only supplies a `railway=switch` node
   without any geometry.
4. **Better sources than OSM — evaluated on 2026-08-16; verdict: attributes yes, geometry no.**
   - **DB InfraGO "Infrastrukturdaten"** (Mobilithek/GovData, CC BY 4.0, data status May 2026,
     27 MB of CSV plus a GeoPackage): `Streckennetz` gives, per directional track and km range,
     the VzG speed (`Geschwindigkeit`, `"120 km/h"`; some 3 % of the rows say `SKVerb`,
     `kein VZG erforderlich` or nothing and need a fallback), electrification, number of tracks
     and the geometry as WKT in EPSG 4326/25832/31467; `Betriebsstellen` (DS100 code, Bf/Hp/
     Abzw/…), `Bahnübergänge`, railway bridges and tunnels come km-referenced with coordinates.
     The geometry is **generalised** — measured over 4 000 rows the vertex spacing is 50 m in the
     median and 205 m at the 90th percentile — so it is coarser than OSM and no basis for the
     alignment reconstruction. The value is in the attributes, keyed by (Streckennummer, km).
   - **RINF / ERA knowledge graph** (SPARQL at `https://rinf.data.era.europa.eu/api/sparql`,
     CC BY 4.0): the German coverage is complete — of 37 012 tracks 36 840 carry
     `maximumPermittedSpeed`, 33 153 a `gradientProfile` (entries `-2.8(+108.21)`: per mille
     plus VzG kilometre, breakpoints every few hundred metres), 35 847 a
     `minimumHorizontalRadius`; on top of that `etcs`, `contactLineSystem`,
     `trainDetectionSystem`, `cantDeficiency` and 17 797 operational points with `uopid`
     (DS100). **No coordinates on the German operational points and no track geometry** — RINF
     too is attributes over (line, km). The endpoint is slow and answers GET with 500: queries
     have to be POSTed (`Content-Type: application/sparql-query`) and scoped by country.
   - **Ruled out**: DB's ISR data service (€1 846 a year, and only for access-authorised
     railway undertakings) and Trassenfinder (no open licence).
   - **Consequence**: neither replaces the drawing tool, both hang off the **kilometrage**.
     Once a module knows its line number and km range, the speed profile comes from DB InfraGO
     and the gradient profile from RINF onto the drawn edges — the DGM measures the terrain,
     not the top of rail, and is wrong on every bridge, embankment and tunnel. DB's line
     geometry, coarse as it is, carries the kilometre marks and so makes the best reference
     layer while drawing (see 2).
5. **Recorded samples for the sound table** — the mechanism is in place and positional; what
   is missing is the audio itself. Rail joints out of the track instead of out of a distance
   interval belong in the same pass.
6. **Weather and night rendering are done** (M6, see above) — rain/fog/snow affect
   visibility, sky and rail; headlights follow switch and direction of travel, red tail
   lamps (Zg 101) mark the rear end, the cab light has its switch and the instrument
   backlighting its dimmer, the precipitation streaks lean into the relative wind, and the
   sound table hears a `Rain` quantity. What is left is per-vehicle content: a mod's own
   emissive panel, and the real lenses a modelled loco wants instead of the placeholder
   body's lamps.
