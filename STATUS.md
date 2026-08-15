# Implementation status against PLAN.md

As of 2026-08-15 · `cargo test --workspace`: **332 tests green** · clippy and fmt clean.

**This project is mod-first.** See [MODS.md](MODS.md) for how to create trains, signals and lines.

## Milestones

| M | Contents | Status |
|---|---|---|
| **M0** | Workspace, `world-coords` (ECEF f64 + floating origin) | **done** — acceptance test "300 km without jitter/jump" green |
| **M1** | `track-model`, procedural track rendering, streaming | **done** — graph, clothoids, `eval`, switches (incl. trailing moves), track meshes; terrain tiles stream in and out around camera and trains |
| **M2** | Longitudinal dynamics + brake, electric loco + coaches, basic cab | **done** — coasting against Davis, emergency braking distance, starting on a gradient, coupler slack as tests; brake and drive down to control valve, motor and torque converter; basic sounds (rolling, traction, air, compressor, horn, buzzer) |
| **M3** | Sifa + PZB 90, signals, editor v1 | **partial** — Sifa (time-time, time-distance, RZM) and every intermittent build from the Indusi I 54 to the PZB 90 V2.0 complete with standard-case tests; H/V + Ks signal logic present, and **signal models render the lamp images**: modular glTF assemblies on mount points (Zusi pattern), lamp nodes switched by the current lamp image, placeholder mast with an aspect light for signals without a model; the **route editor** shows a line with an aerial imagery overlay but cannot edit it yet; the **vehicle editor** edits base data, glTF model, LOD, moving parts, the 3D cab (eye point + interactive controls), the cab displays and the sound table; the **signal editor** assembles signal models (parts, mount points, lamp bindings, lamp test) |
| **M4** | Interlocking, AI trains, timetable | **done** — routes with locking/release, automatic block, AI stops at signals and platforms |
| **M5** | LZB 80 + AFB, MFA, tap-changer loco | **done** — LZB with guidance, braking curve, end and failure procedures, with and without PZB, full/partial block mode and CIR-ELKE; BR 110 present; **AFB** as vehicle equipment (`VehicleSpec::afb`): holds the dial speed with traction, dynamic brake and — where that does not suffice — the air brake, and under LZB guidance runs down the braking curve because the LZB's v-soll caps the dial; MFA values and lamps ship as indicators — HUD text, `gauge:`/`lamp:` instruments and render-to-texture displays in the 3D cab (see M6) |
| **M6** | Interactive 3D cab, start-up procedure, audio, weather/night | **partial** — interactive 3D cab: per-vehicle cab data (eye point + controls binding glTF nodes to a closed input registry incl. wipers and display softkeys), mouse picking with drag/click/scroll gestures per control kind, hover glow, HUD readout, operating clicks via `Control(…)` sound quantities; instruments: gauges/lamps of the safety systems (`gauge:`/`lamp:` indicators, MFA pointers), `digit:` seven-segment counters, and **displays rendered to texture** (declarative widget lists in RON, a Lua `display(ctx)` hook with nested menus and clickable softkeys, or an HTML/CSS/JS page per screen — parsed, flex-laid-out and scripted in-engine by the `html-display` crate, no browser embedded); edited in the vehicle editor with viewport preview; start-up chain operable via keyboard and mouse; weather changes through scenario actions, terrain from the DGM; **day/night cycle**: scenario start clock (date + time), sun and moon computed from the georeferenced location, lighting/sky follow the sun's elevation; **night lighting**: signal lamps glow (HDR + bloom on the main camera, emissive lenses), the leading vehicle's headlight cone follows the darkness; no recorded samples (the sources are generated), no weather **rendering**, no vegetation/texturing |
| **M7** | Pilot line from OSM/DGM, scenarios, scoring, save/load | **largely done** — scenario system, scoring, save/load and the OSM/DGM importer are in place; only a real pilot line is missing (data procurement) |
| **M8** | Mod runtime: declarative content plus Lua behaviour | **done** — loader with dependency order, vehicles/lines/compositions/scenarios/timetables/signal types/signal models as RON, signal state machine as data, four Lua hooks (vehicle, signal aspect, line, scenario) with a sandbox, mod manager on the main menu (a toggle applies on start) and under F9; reference mod under `mods/example` incl. a glTF model. Only distribution (`.trainsim` zip + installer) is still open |

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
  trigger and no loop, the rolling noise the other way round.
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
  signal-dependent magnet activation.
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
  at the track, view distance limit per LOD level in the app, built while driving (see
  streaming above). **One elevation source per UTM zone**: `--dgm`/`--epsg` may be
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
  native file dialogs): `route-editor` shows a line with an aerial imagery overlay and can
  load another one at runtime; `vehicle-editor` edits the vehicle base data (LÜP, gauge,
  v max, mass, rotating mass, axles, axle base sum, rolling and air resistance, tilt angle,
  hunting, payload, curve resistance factor), the complete brake equipment (control valve,
  friction pairing, brake position, load braking, forces and pressures, additional brakes,
  reservoir volumes,
  compressor, leakage, wheel slip protection), the drive with all its detailed data (motor,
  engine map, converter circuits with change points and hysteresis, retarder) and the
  **sound table** — one card per entry with its trigger, its conditions and its dependency
  curves, each curve a sparkline over its support points that opens the shared modal
  curve editor (draggable points plus an exact-value table). A hydraulic
  transmission is fitted rather than entered: the drive panel plots the tractive effort curve
  the parameters actually produce, and a suggestion turns five data sheet figures into a
  starting set to fit from. It also imports glTF
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
  scenarios, timetables, signal types
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
  `trigger: Never` waits for the script. **The mod manager lives on the main menu**
  (installed mods with version, on/off state, missing dependencies and the loading
  warnings); switching writes `enabled` back into `mod.ron` (that one field only) and takes
  effect when the run starts, because the world is built only on leaving the menu. F9 opens
  the same list in the simulator, where a toggle needs a restart. Any run flag on the
  command line (`--line`, `--frames`, …) skips the menu, so CLI and CI invocations stay
  non-interactive.
- **Cross-cutting (ch. 16):** fixed time step, seeded RNG, state hash with determinism test,
  full serialisation for save/load.

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
- **Flank protection** is only switch locking.
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
- **The main menu and the mod manager are keyboard-driven text panels** on the existing
  Bevy UI — no mouse, no `egui` in the simulator. A clickable menu comes when it grows
  real content (line, vehicle and scenario selection); the state machine behind it
  (`GameState::Menu`/`Driving`) is already the one it will hang off. Toggling a mod
  mid-run still needs going through a restart — reloading would mean rebuilding line,
  trains and interlocking.
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
  astronomy), so the season falls out of the date. Seasonal *appearance* (vegetation,
  snow) remains content/texturing work (ch. 14 "seasons v2").

## Sensible next steps

1. **Route editor with tools**: so far it only displays. Next up: drawing alignments,
   placing switches and signals, positioning platforms — the aerial image behind it is the
   template for that. Module tooling belongs in the same pass: placing boundaries,
   showing a neighbour module's edge as a ghost so the builder hits its coordinates.
2. **Import a real pilot line** (Overpass extract + DGM1 from a state surveying office).
3. **Switch catalogue**: standard switches (EW 190-1:9 … EW 1200-1:18.5) as a data table with
   radius, branch length and diverging speed; OSM only supplies a `railway=switch` node
   without any geometry.
4. **Carry over OSM equipment**: signals, platforms, stopping points and level crossings —
   then the import directly yields an equipped line instead of a bare strand.
5. **Evaluate better sources** than OSM: the EU's RINF infrastructure register
   (speeds, gradients, train protection, partly minimum radii) and DB's open geodata.
6. **Texturing/vegetation** — the terrain is single-coloured; splatting and instancing are missing.
7. **Recorded samples for the sound table** — the mechanism is in place and positional; what
   is missing is the audio itself. Rail joints out of the track instead of out of a distance
   interval belong in the same pass.
8. **Weather rendering (M6)** — rain/fog affecting visibility is still missing. Night
   lighting is in: signal lamps glow through HDR + bloom, the player's leading vehicle
   carries a headlight cone that follows the darkness. Still open there: a proper
   Spitzensignal that follows the direction of travel and a cab light switch (needs a
   light control in `sim-core`); instrument backlighting is content work per vehicle.
