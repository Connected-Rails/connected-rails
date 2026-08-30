# Implementation status against PLAN.md

As of 2026-08-29 · `cargo test --workspace`: **954 tests green** · clippy and fmt clean.

**This project is mod-first.** See [MODS.md](MODS.md) for how to create trains, signals and lines.

## Milestones

| M | Contents | Status |
|---|---|---|
| **M0** | Workspace, `world-coords` (ECEF f64 + floating origin) | **done** — acceptance test "300 km without jitter/jump" green |
| **M1** | `track-model`, procedural track rendering, streaming | **done** — graph, clothoids, `eval`, switches (incl. trailing moves), track meshes; terrain tiles stream in and out around camera and trains |
| **M2** | Longitudinal dynamics + brake, electric loco + coaches, basic cab | **done** — coasting against Davis, emergency braking distance, starting on a gradient, coupler slack as tests; brake and drive down to control valve, motor and torque converter; basic sounds (rolling, traction, air, compressor, horn, buzzer) |
| **M3** | Sifa + PZB 90, signals, editor v1 | **done** — Sifa (time-time, time-distance, RZM) and every intermittent build from the Indusi I 54 to the PZB 90 V2.0 complete with standard-case tests; H/V + Ks signal logic present, and **signal models render the lamp images**: modular glTF assemblies on mount points (Zusi pattern), lamp nodes switched by the current lamp image, placeholder mast with an aspect light for signals without a model; the **route editor** edits the line over the aerial imagery (editor v3 — a TSC-style toolbox with the track tools: lay track standing-end/running-end with arc-to-point clicks, Ctrl-straights, snapping onto open ends and turnouts from a press on the track (facing/trailing by drag direction, split and wired with the throw time in the panel), split/join/offset/crossover/gradient tools, device placement and per-device fields, the signal/section/route tables of the interlocking as forms, support-point dragging, rule checking, module boundaries with a ghost neighbour, delete with index remapping, undo/redo, save/open with discard guards); the **vehicle editor** edits base data, drive/brake/equipment as a block diagram (see the 2026-08-17 entry), glTF model, LOD, moving parts, the 3D cab (eye point + interactive controls), the cab displays and the sound table; the **signal editor** assembles signal models (parts, mount points, lamp bindings, lamp test) |
| **M4** | Interlocking, AI trains, timetable | **done** — routes with locking/release, automatic block, AI stops at signals and platforms |
| **M5** | LZB 80 + AFB, MFA, tap-changer loco | **done** — LZB with guidance, braking curve, end and failure procedures, with and without PZB, full/partial block mode and CIR-ELKE; BR 110 present; **AFB** as vehicle equipment (`VehicleSpec::afb`): holds the dial speed with traction, dynamic brake and — where that does not suffice — the air brake, and under LZB guidance runs down the braking curve because the LZB's v-soll caps the dial; MFA values and lamps ship as indicators — HUD text, `gauge:`/`lamp:` instruments and render-to-texture displays in the 3D cab (see M6) |
| **M6** | Interactive 3D cab, start-up procedure, audio, weather/night | **done** — interactive 3D cab: per-vehicle cab data (eye point + controls binding glTF nodes to a closed input registry incl. wipers, lights and display softkeys), mouse picking with drag/click/scroll gestures per control kind, hover glow, HUD readout, operating clicks via `Control(…)` sound quantities; instruments: gauges/lamps of the safety systems (`gauge:`/`lamp:` indicators, MFA pointers), `digit:` seven-segment counters, and **displays rendered to texture** (declarative widget lists in RON, a Lua `display(ctx)` hook with nested menus and clickable softkeys, or an HTML/CSS/JS page per screen — parsed, flex-laid-out and scripted in-engine by the `html-display` crate, no browser embedded); edited in the vehicle editor with viewport preview; start-up chain operable via keyboard and mouse; **weather** (plan 14.1): `sim_core::weather` holds it as physical quantities — cover and cloud base, precipitation kind and rate [mm/h], wind speed and bearing, sight, temperature and a thunder rate — moved between thirteen named presets over a five-minute transition by the `SetWeather` scenario action, with the surface water and the lying snow integrated in the fixed step and the rail condition falling out of them (the first rain on a dry rail is greasy before it is merely wet). Nothing of it is replicated: between two scenario actions the weather is a pure function of the scenario clock, lightning included, so every client stands in the same rain and sees the same flash. Rendered as **clouds in two tiers, switched by a graphics setting** (`world_render::clouds`): both write a 2048 × 1024 equirectangular panorama through an offscreen camera and show it on a dome in the transparent phase, filtered cubically — a camera on the ground never enters a cloud, so a direction is all a cloud has to be a function of. The panorama is **amortised over sixteen frames** on a 4 × 4 Bayer slot, which is what pays for 0.18° a texel at fewer texels a frame than the 768 × 384 panorama it replaces, and **accumulated over about a second** — two buffers swapping roles each frame, every march blended into its texel, with the ray sent through a new point of the texel, started a new way into its first step and aimed along a new line of the light cone each turn, so the blend converges on a filtered edge and a noise-free body instead of freezing one sample's raster into the sky; the history is read where the deck has drifted from over the turn, so a moving cloud is followed rather than smeared. Volumetric is a Nubis-style raymarch (gradient-Perlin-Worley shape at 128³ carved by a Worley detail volume — wisps at the base, billows above — sampled anisotropically so a deck billows upwards instead of extruding one horizontal slice, Beer attenuation with a powder term for front-lit views only, a dual-lobe phase for the silver lining, 96 steps along the ray and four multiple-scattering octaves, an ambient read from Bevy's own atmosphere cubemap with the sun diffused two-stream through the body of the cloud so a closed deck is a grey sky rather than a black slab, and an analytic aerial perspective that fades a far cloud into that same sky); the fallback reads the same field on three slices and walks the self-shadow across that height field, a dozen fetches against several hundred, so a weak machine loses the billows rather than the sharpness. The deck both **drifts** (at 2.5 × the reported ten-metre wind, which is roughly what blows above the friction of the ground) and **evolves** — the march walks the unused part of the shape volume's vertical axis over time, so clouds grow and dissolve instead of only sliding past, and the cover breathes with it; both are functions of the scenario clock and cost nothing over the network, **haze in the atmosphere itself** (a Koschmieder extinction as an extra `ScatteringMedium` term, so fog is blue at dusk and bright around the sun, plus an analytic near-field falloff below 8 km of sight, which the planetary look-up tables cannot resolve), **wet and snowed-on surfaces** (`weather.wgsl`, shared by the terrain and by an extension swapped over every mod material as it spawns: Lagarde's albedo darkening and roughness, procedural ripple normals where the drops land, snow by world normal with a ragged edge, and the dapple of the clouds on the ground), **rain and snow around the camera** (one draw call, thinned by the intensity in the shader, added rather than blended, leaning into the wind of the weather plus the train's own rush of air, and off in a tunnel because the track type says where one is), **lightning and thunder** (a strike read off the clock, lighting the cloud deck and the ground, with a `Thunder` sound quantity delayed by `distance / 343 m/s` and rolling longer the further it struck) **rain on the cab glass** (`world_render::windscreen`: the panes a vehicle names in its `cab:` block get their own material — a film that thickens with the weather, drops in a cell grid that crawl down the glass at a stand and are pushed up it by the airflow above about 15 km/h, and the strip the wiper leaves clear, sampled from the same sweep curve the blade is drawn with) and **ground mist** as a Bevy fog volume with the sun's shafts through it (a graphics setting of its own). `--weather <preset>` places one for a screenshot; `mods/example/scenarios/regenfahrt.ron` shows a run into rain and fog; terrain from the DGM; **day/night cycle with a physically based sky** (`world_render::sky`): Bevy's implementation of Hillaire's scalable sky-and-atmosphere technique (transmittance, multiple-scattering, sky-view and aerial-perspective LUTs — Rayleigh and Mie scattering, so the blue noon, the red sunset and the haze over a distant valley all fall out of one model), the sun's disk drawn into it by the atmosphere itself, a moon disk half a degree wide shaded from the real sun direction (phase, terminator and earthshine out of the almanac), and the 8 900 naked-eye stars of the HYG catalogue as point sprites in J2000 equatorial coordinates plus a procedural Milky Way — turned into the local sky by the observer's latitude and the sidereal time, and extincted by air mass near the horizon; the scenario's start clock (date + time) and the georeferenced location are the whole input; **seasons** (ch. 14 "seasons v2"): the same start date colours ground textures and placeholder vegetation — meadows turn through October, ground, gravel and foliage go under snow from November to March — and a mod may add optional `autumn_model`/`winter_model` variants to its track objects, falling back to the year-round model where it ships none; **night lighting**: signal lamps glow (HDR + bloom on the main camera, emissive lenses), headlight cones at both train ends follow the light switch, the direction of travel and the darkness, red tail lamps (Zg 101) mark the opposite end, **mods' `_NIGHT` nodes** (lit windows, glowing signs) switch at dusk in every model, cab light on its own switch (`CabControl::Headlights`/`CabLight`, keys 9/0) and **instrument backlighting on its own dimmer** (`CabControl::InstrumentLight`, keys `,`/`.`) — a part on the new `Motion::Emissive`, which scales the emissive colour of the mod's own material by the dimmer instead of switching the node, so the dials come up out of the dark continuously (content per vehicle; the example BR 101 carries a backlit panel); **terrain texturing and vegetation** (ch. 14): texture splatting — per-vertex weights from slope and track distance blend three generated ground textures (grass/rock/gravel) in a `StandardMaterial` extension — and vegetation as **line content**: every tree its own `LineSource::trees` entry (3D objects from mods' `objects/*.ron`, placeholder for the unnamed), spawned as children of their terrain tile so they stream with it and batch into instanced draws; woods are baked into single trees by the editor, so each one stays individually editable; no recorded samples (the sources are generated — content, not code) |
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
  roughness, how much its surroundings ring (`reverb`, 0 = open line, 1 = tunnel),
  superstructure speed limit and an LZB flag — assigned per edge as a step
  profile over `s`, so one edge changes its type section by section, with the reserved
  name `"default"` returning to the built-in type. The mod runtime resolves the names
  after compile (like signal types) and merges `max_speed` into the one speed profile AI,
  LZB, HUD and scoring already read; the app **builds the track the type describes**
  (`world_render::track`): the ballast bed as the RL 853 trapezoid (4.0 m over the
  sleeper underside, sides 1:1), skinned per section (texture via `mods://` — the
  example mod ships CC0 photographs of ballast and sleepers, ambientCG and Poly
  Haven, tiled through a repeat sampler set on the image once it has arrived — else
  the type color), the sleepers as real prisms — concrete B 70/B 90 taper, timber 26 × 16 —
  at the type's spacing, merged into chunk meshes culled at 400 m, and the two rails
  extruded from the real rolled section of the type's profile (49E1, 54E3, 60E1 at
  1435 mm gauge, 1:40 inclined), so what the editor and the run show are the DB
  superstructure forms: B 90 and B 70 on the main lines, wooden sleepers on the jointed
  branch lines, Feste Fahrbahn where the type says slab. Feeds `roughness` into the sound
  table as the `Roughness` quantity.
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
  quasi-continuous, or a transmission whose part load comes from the engine speed with the
  circuit simply full (a Mekydro, whose gears sit behind the one converter); a two-range
  gearbox behind it (shunting/road gear of a V 60 or V 90), changed at a stand from the
  cab. Diesel-mechanical is its own path — friction clutch, gears by engine speed, the
  hole each change tears in the effort, and an engine that can be stalled (Köf, railbus) —
  and so is the hydrostatic drive of a small modern shunter, stepless behind its relief
  valve and its limiting-load control. Engine and pump find their working point against each other every time
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
  One sound is normally **several entries**: a single loop stretched over a whole speed range
  by its playback rate drags its formants along and arrives at the top as a toy train, so
  rolling noise and traction are three **layers** each, crossfaded by overlapping
  `Curve::window`s. Each layer only ever plays between 0.85 and 1.25 of its own pitch, and
  neighbours share a flank so the sum stays flat through the handover. With the window
  occupying the volume curve, how loud the sound is at all moves into `factors`.
  **The mixer is kira's** (`app::audio`), not Bevy's — Bevy's audio is a set of sinks on one
  bus with no filter graph, no sends and no effects, which was enough for "a loop whose
  volume follows the speed" and nothing beyond it:
  every vehicle gets its own **spatial mixer track**, so distance attenuation and stereo
  placement come out of its position and all of its sounds share one filter; **that filter is
  the cab wall and the air in one**, its cutoff falling with distance (air absorbs treble long
  before bass) and dropping to 800 Hz while the camera sits inside; **Doppler** is computed
  from the sim's own velocities and multiplied into the playback rate, which is what makes a
  wayside camera hear a train *pass* rather than approach and stop; **reverb** is a send track
  whose level follows the new `TrackType::reverb` under the player — 0 on the open line, 1 in
  a tunnel, in between for a station hall; and a **compressor on the main track** gives a
  dozen entries at their own volumes shared head-room. Volumes and rates are tweened, never
  set hard. The desk sounds (`positional: false`) go on a plain cab track: no distance, no
  wall — they are in the cab with the listener.
  A vehicle without a table of its own runs on the generated default (`sound::default_table`):
  three rolling bands, three traction bands over speed and three more over engine speed, air,
  compressor, horn, buzzer, rail joints and tap changer contactors. Their samples are
  **generated** (`sim_core::synth`, 44.1 kHz, four-second loops whose tail is crossfaded over
  their head so the seam does not click, each band normalised to the same RMS so a crossfade
  does not step in level), so the repository carries no samples — a mod's own files take the
  same path, only the `file` of the entry changes.
  The table is edited in the vehicle editor (Sounds section), where a **▶ per entry plays it
  through the editor's own output device** and puts up a slider for every quantity it depends
  on, so a crossfade can be dragged through by hand; documented in [MODS.md](MODS.md).
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
  response distance, timetable stops, operates Sifa and PZB itself. A driver can also be
  given a **shunt job** instead of, or after, its timetable (see *Shunting* below).
- **Shunting (ch. 11 "train formation"):** trains are made up and taken apart in the run.
  `Sim::uncouple` splits a consist at a coupler; the rear part becomes a train of its own
  with its own runtime and cab, and **the brake pipe parts there** — the part that keeps
  the driver keeps its air (the shunter closed its cock), the other part's hose is left
  hanging, so its pipe vents, its control valves apply and it stands. None of that is new
  physics: only the cocks are set, and `sim_core::brakes` does what it already did for a
  train that parts by accident. `Sim::couple` joins two consists that stand buffer to
  buffer, and refuses rather than surprising anyone: both at a stand and not closing faster
  than 0.3 m/s, the two ends within a metre of each other **along the track graph**
  (measured with `TrackPosition::distance_to`, so a turnout lying the other way puts them
  out of reach however near they are through the air) and pointing at each other rather
  than merely being close, and the coupling gear matching — `CouplerKind` is new on
  `CouplerSpec`, only like couples to like, and a `Bar` between two vehicles of one fixed
  unit is undone in the works, not on the ground. **Nothing is ever removed from
  `Sim::trains`, `runtime` or `controls`**: a consist that has been coupled away keeps its
  slot as an empty `stabled` train, which is what lets the AI drivers, `TrainSync.train`,
  `VehicleView.train` and the score keeper go on addressing trains by index. Every place
  that read `vehicles[0]` was audited and now tolerates an empty consist — `Train::head`
  is the new checked accessor, `head_position` answers "nowhere", and the step, the score,
  the scenario triggers, the AI, the HUD, the cameras, the origin rebase and the mod
  script hooks all hear it (pinned by a test that steps a `Sim` holding an empty stabled
  train). **The driver's side is a setpoint** (`CabInputs::shunt`), so it travels like
  every other lever: a client sends the order, the server applies it, and every peer works
  out the same consists from the same command and the same geometry. A refused order stays
  on the ground for ten seconds and is retried every step, so the peer that was twenty
  centimetres short couples a moment later instead of ending up with a different world.
  `Insert`/`Home` are the player's two buttons, and the shunter's answer — coupled,
  uncoupled, or which condition was not met — comes back on the HUD line the train
  protection interrupts on.
  **Setting back works at all now**: the reverser used to zero the traction in back gear
  (`throttle * reverser.max(0)`), so a train could only ever be driven forwards. The drive
  models compute the magnitude of the effort and the new `TractionState::back_gear` puts
  the sign on at the rail (`physics::transmit_traction`), with the adhesion, the slip and
  the wheel slip protection untouched in either gear.
- **Somewhere to shunt to (ch. 11 "v1 trains spawn/despawn at fiddle yards"):**
  `LineSource::yards` is line content next to the devices and objects — a `(edge, s,
  facing, length)` mark with a name. Two kinds: a **stabling road** is a siding on the
  modelled line, a **portal** is its edge, and **trains appear and disappear at portals and
  nowhere else** (the rule check refuses a portal whose track does not run out to a buffer
  stop or a module boundary). `Sim::place_at` puts a consist on a road — refusing one too
  short, one already occupied, and a mark the track behind runs out on — and
  `Sim::withdraw` takes a train off the line at a portal it is actually standing at; it
  keeps its slot and its vehicles, so the same unit comes back out later. Yards follow
  their edge through splits and deletions like devices, and a composition shifts them by
  the module's offset. The example line has a turnout at km 4.0, a stabling siding and a
  portal at each end; the Musterbahn has its two portals.
- **Shunt jobs (`ai-driver::shunt`):** a list of moves worked off in order — draw forward
  to a point, set back onto a road (measured from the *rear*, which is the end that leads a
  reversing move), couple to what stands there, uncouple at a coupler, finish at a stand. A
  target is a point on the graph or a road by name. The driver writes nothing but
  `CabInputs`, holds itself to the 25 km/h Rangiergeschwindigkeit and creeps the last few
  metres; a move ends when the buffers are met, whatever the mark said, and only at the end
  that leads — the rake left behind by an uncoupling sits against the other one. A target
  the line does not have, or a coupling that cannot be made, stops the train instead of
  running it on to nowhere.
- **Scenarios (ch. 11.4):** RON events with triggers (time, train position, stop, speed,
  signal aspect, emergency brake application, chaining with delay, `All`/`Any`)
  and actions (message, announcement, switch, route, weather, points, scenario end).
- **Operating days (ch. 11):** the second way a line is driven, beside the scenarios. An
  `OperatingDay` (`sim_core::day`, `days/*.ron` in a mod, plus a built-in one for the
  Musterbahn) is a whole day of **services** with wall-clock times that loop every 24 hours;
  the run picker lists every playable one under a heading of its own, and the run starts two
  minutes before the service departs. The rest of the plan runs around it: a service claims a
  train `LEAD` before it leaves and gives it back `TAIL` after its last arrival, and between
  two workings the unit is **stabled** — not driven, not drawn, and skipped by the occupancy
  detection, so it is genuinely off the line. The next service that needs the same stock takes
  that unit rather than a new one, which is what keeps a looping day at the train count of its
  busiest minute. Which services are out is a pure function of the clock and they claim in
  departure order, so a dedicated server and every client build the same train list at the same
  indices without a message about it; the AI drivers stay the server's. A service may name the
  **road it leaves its stock on** (`stable_at`, a `yards:` entry of the line): a stabling road
  holds the unit where it can be seen, occupying its siding like any other train and left with
  its brakes applied, and a portal swallows it off the module altogether. A plan that names
  none leaves its stock at the terminus, which is what it means when it says nothing. The AI
  **drives** the unit there — the working is given a shunt move on top of its timetable, worked
  once its last stop has been made, and its window stays open ten minutes instead of three to
  let it — and the placement at the end of the window is the backstop that keeps the whole
  thing a function of the clock.
- **Spawn points and standing stock (ch. 11, `sim_core::consist`):** where a train comes from
  is now a value of its own. A `Spawn` is either a place on the graph (`At`) or one of the
  line's roads by name (`Yard`), and naming a **portal** is what makes a train come out of, or
  disappear into, a piece of railway nobody built. On top of that both a scenario and an
  operating day may declare `consists:` — trains that stand on the line from the first minute,
  each naming its vehicles one by one head first, prepared or cold, driven to a timetable
  where they name one and left with the brakes applied where they do not. For a **scenario**
  that list closes a hole that had been open since the format existed: an event could name a
  train by index but nothing could put one there, so a scenario had exactly the one train the
  menu built. Its order is the order the indices run in, and `player_train` picks the player's.
  `mods/example/scenarios/rangierfahrt.ron` is a shunting scenario made of nothing else.
- **Zugfahrt and Rangierfahrt (ch. 10/11, Ril 408 / 301):** which of the two a train is
  making is a value on it (`shunt::Movement`), and almost everything about how it is
  signalled hangs off it. A **train movement** reads the main aspects, is only let onto
  proved track and runs at the line speed; a **shunting movement** is let past by **Sh 1**
  and by nothing else — Hp 1 says nothing to it, and a main signal that carries no shunting
  signal stops it dead — may be let into an occupied track, and is held to 25 km/h on sight
  throughout the look-ahead. A **Sperrsignal** (`SignalKind::Shunting`) shows Sh 0 / Sh 1
  and no main aspect at all, and its Sh 0 is "Halt! Fahrverbot" for every movement. A
  **Rangierstraße** (`RouteKind::Shunt`) locks the points and clears Sh 1 while leaving the
  main signal at stop; it may be set into an occupied section, has no overlap, and is
  released by **the movement it belongs to**: passing the signal under Sh 1 makes the route
  that movement's (`Route::owner`), and it is given back when *that* train has cleared its
  sections — which the track clear detection can now answer, because a section records which
  trains are on it and not merely that something is. That is what lets a second shunt run
  past the same signal without taking the first one's route away, and what lets a route over
  a road that was occupied to begin with be released at all. A route set for a movement that
  never came is given back after `SHUNT_HOLD` (five minutes) — the Zeitverschluss of a
  Rangierfahrstraße, without which the points under it would stay locked for the rest of the
  day. The 2000 Hz magnet
  of a signal showing Sh 1 is switched off — otherwise every shunting movement past a signal
  at stop would be tripped, which is what the real interlocking does for the same reason.
  **A movement changes kind by passing a signal**: under Sh 1 it becomes a shunt, under a
  main proceed aspect a train, so a shunt drawing up to the starting signal and being given
  a train route leaves as a train with nothing switching a mode. A movement standing in
  front of a Sperrsignal is given the first free shunting route out of it by the interlocking
  itself — a pure function of the world inside the fixed step, so every peer sets the same
  route without a message. Shunt jobs are content: a scenario's or a day's consist may carry
  one (`ConsistSource::shunt`) with or without a timetable beside it, so a pilot that exists
  only to move stock about is a plain entry in a file.
- **Changing trains (ch. 11, `app::crew`):** the player is responsible for one train at a time
  or for none, and that — not `PlayerTrain`, which only says where they are standing — is what
  decides who is on the levers. The AI drives every train it has a driver for **except** that
  one, and the cab keys move the levers of that one only, so the two can never be on a lever
  together. **Getting out hands the working over**: the AI takes it on from the stop that is
  actually next rather than from the first, and a train with no working at all is secured
  where it stands instead of being driven away. **Walking into another train** boards it — a
  door within reach is searched across every train on the line now, not only the player's —
  and being aboard makes them a passenger; **the take-over key** (`Tab`) at the desk makes them
  its driver, and gives the train back. Every refusal is said out loud: not from the platform,
  not a train that is out of service, not a rake with nothing that pulls, not somebody else's,
  and in a scenario not any train but the scenario's own. The **run follows the driver** — the
  scoring is re-pointed at the working actually taken. Over the network the take-over is a
  wish (`net::TakeOver`) the server grants by answering with the `Welcome` that already exists
  for joining, and refuses by naming the train the client already had; the dispatcher is told
  which trains are somebody's so a unit is never stabled out from under its driver.
- **Date and weather of a run (ch. 11, 14.1):** picking a service opens one more step of the run
  picker. The **date** is the plan's own to begin with and is dialled a day at a time (a proper
  civil calendar, so month and year ends and leap days roll over) — it decides the season and
  where the sun stands at the service's hour. The **weather** is either `Fixed(preset)`, placed
  at the start with the wet or snowed-on ground it implies and held, or `Dynamic`: a day that
  makes its own weather out of `(seed, clock)`. Two octaves of value noise give a severity, and
  that severity is read off a ladder of presets — clear, cloudy, overcast, and on into what
  falls out of it — with the month choosing the ladder, so the front that rains in June snows in
  January. Nothing of it is state: no seed to replicate, no keyframes to sync, and the seed
  itself comes out of the content (the plan's name and the date) rather than a clock reading at
  start-up, so the same service on the same date brings the same sky on every machine. A
  scenario's `SetWeather` takes the sky over from the generator and switches it off.
- **Scoring (ch. 11):** timetable adherence, stopping accuracy, emergency brake applications,
  speed limit violations and traction energy → itemised score. A timetable is either
  `Scenario` (times from the start of the run, runs once) or `Daily` (seconds since
  midnight, wrapping around every 24 h — delay, departure and the AI's stop list wrap
  with it). A service of an operating day is a `Daily` timetable, so the HUD's ribbon and the
  delay read wall clock throughout.
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
- **Fields from the agricultural registers (field plan, complete):** the countryside beside
  the line is farmed, and a field is a crop rather than a green rectangle. A line stores the
  outline, the crop, the working direction and a seed (`route::FieldSource`); what a field
  *looks like* on a given day comes out of `fields::phenology` — a table of key dates per
  crop giving ground cover, stand height, colour and row contrast, with the seed shifting a
  field's own year by up to a week so no two neighbours are cut on the same afternoon. That
  makes the appearance free over the network (every client derives it from the same three
  numbers) and free to change: the date lives in the material, so dragging the editor's date
  slider turns every field in the world without a mesh being rebuilt.
  **The data** is the InVeKoS publication every EU member state owes under Art. 67(3) of
  Regulation (EU) 2021/2116. The new `fields` crate holds the sixteen state services with
  their levels and licences (`land.rs`, the boundaries baked from the BKG's VG2500), a WFS
  GeoJSON client that always carries a bounding box and quarters it when a service answers
  with more than a request can hold (`wfs.rs`), a two-stage crop taxonomy — the state's own
  code where it publishes one, its InVeKoS group otherwise, the regional cropping statistics
  where it publishes neither, drawn deterministically from the parcel's id (`crops.rs`,
  `stats.rs`, 222 North Rhine-Westphalian codes read off the service itself and shipped as a
  CSV a builder can correct), a Greiner-Hormann clipper with Douglas-Peucker thinning, ear
  clipping and rotating calipers for the working direction (`geometry.rs`), and a tile cache
  that makes an import reproducible and offline (`cache.rs`). Six states publish the parcel
  with its crop (NW, NI/HB/HH, BB/BE, SN, TH — Saxony as points, joined onto its reference
  parcels); six publish the field block alone; Rhineland-Palatinate publishes nothing at all
  and falls back to OpenStreetMap's `landuse=farmland` through Overpass (`osm.rs`, ODbL,
  which the attribution records); three states have stated no licence, which the import
  flags rather than hides. **A module outside Germany** finds no state under it and takes
  the same fallback — `FieldFeature::land` is an `Option`, the UTM zone comes from the
  longitude instead of a state's convention, and the crop is drawn from the general
  statistics row. Measured on the Marchfeld east of Vienna: 27 ways, 23 fields, 443 ha,
  credited to OpenStreetMap under ODbL.
  **The editor** has a threaded import with a progress bar, the state being asked, a Stop
  that means it, and a summary that has to be confirmed before anything is written — one
  undo step for a whole import. Scope is the module envelope or the selected field. A field
  tool draws one by hand. **The renderer** cuts each field to the terrain tiles, drapes it on
  the tile's own height grid, punches out the track's formation, and draws it with one
  material per crop — furrows at the crop's own drill spacing, the sprayer's tramlines every
  24 m, both fading out as they fall below a pixel. **Driveable**: `lines/boerde.ron` and
  `example:boerdefahrt` are five kilometres across the Soester Börde with 135 real parcels
  beside the track, which is what caught the one bug the editor could not — a module whose
  rails sit below the terrain's fallback height runs down a cutting for its whole length,
  and from a cab that is banks rather than countryside. Invisible from a top-down editor
  view; obvious at two and a half metres.
- **Terrain (ch. 14):** 512 m tiles only within the line corridor, grid spacing by distance
  from the track (4 m to 32 m instead of 1 m), skirts against LOD cracks, cutting/embankment
  at the track — the ground there is the **formation**, `rail_offset` (40 cm) below the top
  of rail, so the ballast bed lies on it instead of inside it — view distance limit per LOD
  level in the app, built while driving (see streaming above). The formation is sized like
  the real thing (2026-08-29): a single track's Planum half-width of **4 m** (the ~2.6 m
  ballast body plus shoulder), the embankment or cutting running from its edge to the
  natural ground within **12 m** — roughly 1:2 at the heights a main line dam has — and the
  gravel texture full on the formation, fading out by **7 m** so the slopes stay grass.
  The distance queries measure against the centreline **segments** rather than the 25 m
  samples, so the blend zone is where the line is, not where its samples happen to fall.
  **Edges without a formation** (`route::EdgeSource::formation`, default on): track the
  builder laid on their own constructions — bridges, platforms, self-shaped ground — gets
  bare rails, no ballast bed mesh, no embankment, no gravel; the terrain leaves it alone.
  Set per edge in the lay panel (for the piece about to be laid) or in the selection panel
  (for one lying there). **Texturing/vegetation:** every tile carries per-vertex splat
  weights (gravel on the strip the track flattens, rock on steep ground, grass
  elsewhere) and the line's trees — every tree an own `trees:` entry, its foot
  on the tile's height grid. Woods come out of the editor's forest brush and
  forest import, which **bake** polygons into single trees
  (`terrain::fill_polygon`: deterministic, one per `area_per_tree` m², clear of
  the track strip) — one primitive, so any tree of a wood is moved or deleted
  like a hand-set one. Trees are 3D objects from mods (`objects/*.ron`; empty
  name = generated placeholder tree), and `mods/trees` is the one that fills
  them (see the 2026-08-29 entry below). The app blends three generated ground
  textures by the weights in a `StandardMaterial` extension shader and spawns
  the trees **and the scenery objects** as children of the tile
  (`content::terrain::Scenery`, `world_render::scatter`): a tree is one entity
  per mesh part of its model, sharing the part's mesh and material handles —
  read once out of the loaded `Gltf`, never a scene instance — so Bevy batches
  a wood into instanced draws, `_LOD` nodes become `VisibilityRange` bands,
  and a tile's wood appears in one go once its models have loaded. Track
  objects can opt into `snap_to_terrain`: the base moves from the rail plane
  onto the tile's own height grid. The `TerrainBuilder` is shared read-only
  across the workers (`Arc`, no lock — the DGM sheets carry their own short
  one, reads happen outside it), and an edited line is a new builder sharing
  the sheets (`with_line`); the F6 panel reads frame time and entity count. **One elevation source per UTM zone**: `--dgm`/`--epsg` may be
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
  (editor v3), its tools in a **toolbox** on the left edge after Train Simulator
  Classic's World Editor — categories in the top box (track, lineside equipment,
  vegetation, terrain, module), the active category's tools in the middle box, the
  active tool's own switches (radius snap, easements, terrain snap) in the bottom
  box, the number keys counting down the box that is up — and the form panel docked
  on the **right** edge, its *Tool* section carrying the active tool's remaining
  options. The **lay tool** works standing-end/running-end: the press sets the start —
  on an open end it continues that track, on a track's middle it starts a turnout
  branch, on open ground it starts fresh — and the drag until release sets the heading
  (for a branch it decides **facing or trailing**: along the track or against it).
  Every further click appends one tangent-continuous arc or straight (arc-to-point,
  G1 by construction), a **straight while Ctrl is held**, and the running end **snaps
  onto open ends**, closing the gap with two tangent arcs (biarc) so the join is
  tangential at both sides; the status bar reads out the piece's **length and radius**.
  What the piece is laid as comes from the *Tool* section — track type (the content
  drawer's track-type cards arm it too), speed, gradient, electrification, **parallel
  tracks** at a spacing, an optional **snap onto the standard radius series** of
  the alignment rulebook, an optional **terrain snap** (sampled ground heights become
  the piece's grade profile, a free start drops onto the surface, an end joined onto
  other track keeps that track's height), and **easements with cant**: a curve then goes down as
  clothoid – arc – clothoid with the rulebook's cant for the piece's speed
  (`CantRules` — equilibrium minus deficiency, capped, ramp 1:10·v), the cant band
  written as the same 10 m steps the importer writes (`ramp_cant`), signed by the
  curve direction so the roll tips into the curve. Continuing from an open end
  starts the entry transition from that end's own curvature. On finish a branch splits its base edge
  (`LineSource::split_edge` — devices, profiles, sections, switch legs and followers
  all follow) and wires the joint into the turnout, the branch its
  diverging leg; trailing makes the far half of the split
  the root, so a train over the clicked track trails the points instead of facing them.
  Beside the lay tool sit the tools that work on laid track: **split** (one click, two
  tracks on a joint), **join** (weld two open ends on one spot into one node —
  `LineSource::merge_nodes`, every node index remapped — or **stake the connection
  out after Zusi's Absteckrechner** (`route-editor/src/stake.rs`): the simplest case
  is transition – arc – transition plus one compensating straight at the start or
  the end (with the radius on automatic a bisection grows it until exactly one
  remains; a fixed radius keeps straights at both ends or refuses as too big), a
  parallel offset or reversing heading becomes a double arc around an intermediate
  straight of at least the configured length (seeded from the two-circle tangent
  construction, polished with a 3×3 Newton so the transitions land the chain on the
  far pose), and a curved end feeds its curvature into the boundary transition —
  compound-curve ground — and can carry no straight of its own. Design speed,
  radius, transition curves and their length, cant and the least intermediate
  straight are the staking parameters in the Tool section; the refusals are worded
  like the original's — not plausible, radius too big, double arc impossible), **offset** (the parallel at the set spacing, on the side of the
  click, exact for straights and arcs), **crossover** (cuts the clicked track and the
  parallel one and wires the S of two turnout-radius arcs between them, both switches
  included, whichever way the second track runs), and **gradient** (a click puts a
  break point on the track; the selection panel edits the `(s, ‰)` steps and reads out
  the climb over the edge — the height the run integrates from the grade profile).
  A **device tool** drops any `DeviceKind` onto the nearest track, and a selection
  panel carries the device's fields (kind, position, facing, lateral offset, RON payload).
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
  **object tool** drops any installed mod's 3D object (`objects/*.ron`) onto the
  nearest track at the object's own default offset and rotation; the selection panel
  edits position, lateral offset, rotation and height per placed instance, and
  **Repeat in a row** stamps copies along the track (spacing, default 65 m; end
  position) — the Zusi editor function "insert one every x metres", each copy an
  ordinary instance that can be moved or deleted on its own. **Vegetation tools**:
  a tree tool plants single trees free of the track, a forest brush
  outlines a polygon and bakes it into single trees (species — a single one or
  a **stand**, which mixes everything the mods tag `stand-…` — and density in
  the tool options), and **File ▸ Import forest** reads an Overpass extract's
  `landuse=forest`/`natural=wood` ways and bakes them the same way — an optional
  aid next to hand placement, and every baked tree stays individually editable
  and deletable. A **marking brush** sweeps over the map, marks trees
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
  The **terrain tools** shape the ground itself, one per gesture in the
  toolbox's terrain category: raise and lower stamp round strokes into
  `LineSource::terrain` (`Raise(±m)` on top of the DGM, the amount and radius
  shared tool options), **flatten** levels to the ground height under the
  click (`Level(height)` from the built tiles — the World Editor's plateau),
  and **level to rail** takes its target from the nearest rail. Strokes are
  data, not a baked heightfield (pickable, re-dialled, deleted; the DGM stays
  untouched, so better elevation data can be re-imported without losing the
  shaping), they apply in file order, fade out with a smoothstep at their
  radius, and are prefiltered per tile in `TerrainEdits::in_rect`. They act on
  the ground **before** the cutting/embankment blend, so no stroke can lift the
  track out of its alignment (pinned by a test). The map draws each stroke's
  true footprint — warm raising, cold lowering, grey levelling.
  **The editor shows the world it builds** (`terrain.rs`, `signals.rs`): `T`
  switches the map for the run's own picture — the same `TerrainBuilder`, mesh,
  splat material and ground textures, the **track as ballast bed, sleepers
  and rails on their real sections** skinned per track type, the line's
  **trees and scenery objects** as the mods'
  glTF at the placement's own pose (placeholder trees for unnamed ones, objects
  that ask for it on the terrain surface), and the **signal assemblies** on
  their mount points. The shared `world-render` crate is that code, used by
  both programs, so a stroke, a wood, a signal box or a signal mast is judged
  where it is set instead of only in the run. Tiles are
  built on the task pool around the view point (3 km radius with a 25 %
  unload hysteresis, capped at 64 tiles); an edit is **diffed** against the
  last state (`main.rs::diff`) into what it reached — a stroke the ground of
  the tiles under its disc, a moved tree or object only the trees and objects
  of its tile, which are placed onto the standing ground again
  (`TerrainBuilder::rescatter`) without a rebuild; only the track asks for
  everything. The old tile stays until its replacement arrives, and the status
  bar reads out frame rate, entities and tiles.
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
  with the marker tool or imported from an Overpass extract
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
  alone; the map pans with the middle mouse button and zooms with the wheel (the
  number keys count down the toolbox's active category); device payloads come from
  one-click RON templates serialised from the
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
- **Trees of Central Europe (2026-08-29, `mods/trees`, `tools/trees/`):** the
  vegetation is generated, not modelled. `tools/trees/species.json` describes
  twenty-eight species — spruce, pine, silver fir, larch, Douglas fir, juniper;
  beech, oak, hornbeam, birch, alder, sycamore and Norway maple, ash, lime,
  aspen, Lombardy poplar, white and goat willow, elm, horse chestnut, rowan,
  wild cherry, black locust; hazel, hawthorn, elder, blackthorn — with their
  heights, crown widths, branch parameters, bark and leaves, and
  `build_trees.mjs` grows them with [ez-tree](https://github.com/dgreenheck/ez-tree)
  (MIT; cloned at a pinned commit into `~/.cache`, never vendored). Out come
  three differently seeded individuals per species (`_a`, `_b`, `_c`), each with
  **four levels of detail** and **three seasons**: 237 models, 84 objects.

  *Variance* is three things at once: three shapes per species, the stands that
  mix the species, and the yaw and scale every baked tree already carried.

  *The meshes* are cut for a wood, not for a hero shot. Averages are 5 500
  triangles at `crown_LOD0`, 1 200 at `LOD1`, 270 at `LOD2` and **4** at
  `LOD3` — two crossed quads carrying a picture of the tree, rasterised from
  its own `LOD0` geometry against the very atlas the near levels sample, so the
  silhouette and the colour agree across the switch. The leaf cards' normals
  come out of the **canopy**, not out of the card: ez-tree points a card's
  normal where it happened to put the card, so two neighbours can face opposite
  ways, and under a low sun one is blown out while the other is black — a crown
  two hundred metres off then reads as a mosaic of hard bright and dark blocks
  rather than as foliage. Taken from the vertex's direction out of the middle of
  the canopy instead, biased upwards for the sky light, the crown shades like
  the rounded mass it is. `LOD1` and `LOD2` drop
  every branch below a share of the trunk radius and leave the canopy where it
  was, which is what ez-tree's own coarsest level cannot do (it keeps a ring of
  three segments per branch — eleven hundred triangles of twig before a leaf).
  Bark, two foliage cards and the impostor share **one 1024 × 512 atlas**, so a
  level is one primitive and one material: four entities per tree instead of
  eight, and one instanced draw per level instead of two. The foliage card is a
  *spray* of fifteen to twenty leaves rather than one leaf — ten times fewer
  quads for the same canopy — and the arrangement is botanical, which is what
  tells an ash from an oak in a stand. The **leaves on it are photographs** out
  of two CC0 libraries: ambientCG's `LeafSet` sheets of loose leaves (an oak
  takes `LeafSet016` in summer and `LeafSet012` turned, a beech `LeafSet024` and
  `LeafSet015`, an ash the whole compound sprays of `LeafSet002`) and Poly
  Haven's fir and pine twig atlases for the conifers.
  `tools/trees/fetch_foliage.mjs` pulls them into the cache, `lib/leaves.mjs`
  flood-fills the opacity mask, cuts each leaf out and stamps it — *the
  arrangement stays the pipeline's own*, which is the half of a card that
  carries the species. Nothing of either library is redistributed; what ships is
  the atlas composed from them, which CC0 allows without condition
  (`THIRD_PARTY_LICENSES.md`). The **bark is still painted** per species: no
  generic scan gives a birch its lenticels or a Scots pine its orange plates.

  *The bands* are the object's own (`TrackObject::lod_distances`, new), scaled
  to the plant's height: a thirty-eight metre spruce hands over at 95 m and is
  drawn to 2.5 km, a two metre blackthorn hands over at 20 m and is gone at
  700 m. The bands are **generous** — 4×, 14× and 40× the plant's height, which
  on a 1440-line screen at sixty degrees is hand-overs at roughly 340, 100 and
  35 pixels, or 120 m, 420 m and 1.2 km for a thirty metre beech. An earlier set
  at less than half of that had a wood two hundred metres off reading as
  streaks; it passed review because the review window was 1200 lines and the
  same level looks finer there. `LOD3` starts **late**, at forty times the
  height: it is two
  quads crossed at a right angle, and whichever blade the camera looks along
  edge-on is drawn as a narrow strip *through* the other, which at four hundred
  pixels reads as a slice through the crown and under forty is invisible. One
  quad is not the alternative — a fixed billboard vanishes the moment the
  camera looks along it, and on a railway the angle to a tree sweeps through
  every value as the train passes it. So `LOD2` carries the middle distance on
  real geometry and costs its 270 triangles there. one normal **per blade**, the direction that blade faces. A flat
  quad has one normal — a normal per *corner* lights its left half differently
  from its right and splits every tree down the middle, which was the seam.
  Straight up removes the seam but also removes front and back, so the billboard
  never darkens with the sun behind it and a backlit wood glows at dawn. No
  upward component either: `doubleSided` negates the normal on the far side, and
  a tilt up comes back as a tilt down. Levels that start beyond
  300 m carry `NotShadowCaster` — a crossed quad's shadow is a cross, and the
  sun's own visibility pass over half a million tree entities is not free.

  *A canopy is mostly holes*, and three things had to be put right before a wood
  held together at a distance (all of them the renderer's, not the generator's):
  the **mip chain keeps the coverage of the alpha** (`with_mipmaps`) — box
  filtering halves it at every level, so by the fourth or fifth almost no texel
  reached the cutoff and the foliage evaporated while the opaque trunks stayed;
  the tree materials are switched to **alpha-to-coverage** once loaded, so a
  leaf's edge is resolved by the sample mask instead of stepping from texel to
  texel (Bevy falls back to the mask without MSAA); and the levels **crossfade**
  over ±8 % of their hand-over distance instead of switching in one frame.

  *The impostor's shading is measured against the level it replaces.* Rendering
  the same wood once as `LOD0` and once forced to `LOD3`, and comparing the mean
  luminance of the tree pixels against the sky behind them, is what says whether
  the two match — and whether the billboard reacts to the sun at all, which a
  reading of the shader had claimed but not shown. It does: backlit it comes out
  at 0.510 of the sky against the meshed tree's 0.511, front-lit 0.597 against
  0.598, and the directional swing between the two is a shade stronger than the
  geometry's. The constants that produced that were the second try; the first
  was a quarter too dark, which is a visible step at the hand-over that no
  amount of reading the shader would have found.

  *Culled by the tile.* The terrain streams to the view distance — four to seven
  kilometres — while no tree is drawn past two and a half, so more than half the
  resident tiles carry trees that cannot appear; each is still four entities,
  each looked at once a frame for the camera and once more per shadow cascade
  only to fail its own `VisibilityRange`. Every tile's trees therefore hang
  under one `Wood` entity whose visibility is switched by distance, and both
  `check_visibility` and the light's own pass give up on an entity with a false
  `InheritedVisibility` before they touch its bounds. Measured on the 60 000
  tree wood: **42 of 68 woods switched off**, so about three fifths of the tree
  entities never reach a bounds test. The test is distance and not the frustum
  on purpose — a tile behind the camera still throws its shadow into the picture
  under a low sun, and the shadow pass reads the same inherited visibility.

  *Measured* with `tools/trees/bench_forest.mjs`, which writes a throw-away mod
  with a given number of trees over an 8 km line in a 2 km wide corridor and
  runs it (`--frames 400`, debug build, vsync off, this machine):

  | trees | fps | frame | entities |
  | ----- | --- | ----- | -------- |
  | 0 | 252 | 4.0 ms | 1 482 |
  | 5 000 | 211 | 4.8 ms | 19 782 |
  | 20 000 | 177 | 5.7 ms | 74 738 |
  | 60 000 | 135 | 7.7 ms | 221 994 |
  | 150 000 | 63 | 15.9 ms | 552 390 |

  The cost is **linear in entities, flat in triangles** — the levels hold the
  geometry budget wherever the density goes, and what is left is Bevy's
  per-entity visibility pass at about 28 ns each. The shadow cutoff above was
  worth 19 % at 60 000 trees and 21 % at 150 000. 60 000 trees is one per
  250 m² of the whole corridor, which is a forested line; 150 000 is one per
  100 m², which is denser than anything a line file would hold.

  *Seasons* come free of the geometry: summer, autumn and an evergreen's winter
  share one `.bin` and differ only in the sheet of leaves; a bare deciduous
  crown is the one that needs its own. `--date 2026-10-18` and `--date
  2026-01-20` show them.

  The demo line and the example mod's three lines are planted with them, mixed
  out of stands: mixed wood, spruce stand, embankment scrub, an avenue of
  Lombardy poplars.

- **Toolbox boxes and the right-hand panel (2026-08-24, route editor):** the editor's
  frame now matches the World Editor's. The toolbox became three card-framed boxes of
  paired icon columns — the categories, the active category's tools, and the active
  tool's own switches: the lay tool's radius snap and easements, the join tool's
  easements, the object tool's snap to terrain, which moved out of the form rows into
  the strip (an object placed with the toggle on starts with `snap_to_terrain` set).
  The form panel docked over to the **right** edge and holds its width — a size cap
  keeps one wide row from ratcheting the panel wider for the rest of the session
  (egui persists the width content grew a panel to), the counts joined the heading's
  line, and the drawer button beside the object picker became an icon. **The section
  interiors followed**: interlocking sections and routes became one card each
  (identifier and delete in the header, fields and chips in the body — the route's
  delete no longer sits amid the chip "×"s), the marker layers and the area list
  are grids instead of per-row `horizontal`s so their counts and buttons sit in
  columns, the DGM source path truncates with the full path on hover, and the
  route editor learned the vehicle editor's `--window WxH` flag.
  **Placement grew a preview** (`tools::placement_preview`): the object and tree
  tools carry the model as a ghost at the cursor — spawned once per picked model,
  moved every frame, hidden off-snap — standing exactly where the click would put
  it (`scatter_objects`' own pose maths); the device tool draws its snap point
  and track direction. A **double click** on a selection jumps the panel to its
  properties (own detector in `tool_input` — the map is no egui widget), the
  gradient tool draws **slope chevrons** (a V every 60 m pointing uphill on every
  graded stretch), and the editor **remembers language, window size and panel
  width** between runs (`settings.rs` after the vehicle editor's; the language
  menu used to throw the choice away at the next start).
  **Orientation and view controls followed** (`viewport_bar`, `view.rs`): a drawn
  **compass** whose needle shows where north lies — the click faces north — and a
  **top-down toggle** that tips the camera vertical for track work over the
  imagery (`Focus::toggle_top_down`), both also under View; the **properties
  panel folds away** (`EditorState::panel_hidden`, viewport-bar button and View
  entry; any jump into the panel unfolds it), and the File/Edit menus finally
  **show their accelerators** (`Button::shortcut_text`).
  **The map followed the same original** (`tools.rs::draw_gizmos`): while the track
  category is up, every edge wears the World Editor's spline line (blue, drawn
  first so highlights and selection paint over it), every node a square — grey
  weld at joints and switches, red at loose ends, replacing the lay/join-only
  grey circles; the cursor-near and join-first-pick states stayed. The selected
  edge grew the red/blue direction arrows out of its start and end, which say
  which way its metre figures run.
- **Track laying after the World Editor (2026-08-23, route editor):** the toolbox strip
  and the track-tool set described under *Editors* above — standing-end/running-end
  laying with Ctrl-straights, end snapping and drag-decided facing/trailing turnouts,
  plus split, join (weld/biarc), offset, crossover and gradient tools, the lay options
  panel for the next piece, and the content drawer arming the lay tool with a track
  type. The switch tool went with it: a turnout is a lay that starts on a track.
  **The rest of the editor followed the toolbox**: vegetation and terrain are
  categories of their own (raise, lower, flatten, level-to-rail and the DGM tile
  picker as separate tools with drawn icons, the stroke preview in the colour the
  stroke will wear), and the form panel is contextual — the fixed Tool and
  Selection sections plus only the active category's own (`ui.rs::
  category_sections`, pinned by a test), so the eleven-section scroll became a
  panel as short as the work in hand. A UX pass the same day: the **select tool
  leads every box** (`1` everywhere, the category is held in the state so taking
  it does not switch the panel away), the module category wears a jigsaw icon of
  its own instead of doubling the envelope's, the "pick one in the drawer" notes
  became **buttons that open the content drawer on the right category**, and a
  **findings badge** on the status bar carries the rule check's count into every
  category — the click opens the checks where they live.
  **Easements and cant followed the same day**: the lay option turns a clicked curve
  into clothoid – arc – clothoid with the rulebook cant of the piece's speed, fitted
  so the chain still ends on the click (and, with the radius snap, on the standard
  series). Fixing that surfaced a sign bug in the importer: cant was written unsigned,
  so every right-hand curve rolled *outward* — `CantRules` now signs the cant by the
  curve direction and takes ramp lengths from the magnitude, pinned by tests on both
  the importer and the editor. **The stake-out calculator closed the set**: the join
  tool now connects two open ends the way Zusi's Absteckrechner does (see the join
  tool under *Editors*), with its parameters — design speed, radius or automatic,
  transitions, cant, least intermediate straight — as the tool's options.
  **The select tool grew the World Editor's multi-selection**: `Ctrl`+click gathers
  devices, objects, trees and markers into the marked set (a second `Ctrl`+click takes
  one out), a press on empty ground dragged open is the **circle selection** over the
  same kinds, and `Delete` removes the whole set as one undo step — the marking
  brush's machinery (`Mark`, `delete_marked`), taught two more kinds and given the
  select tool as a second front end.
- **Vehicle editor completed (2026-08-19):** the gaps against Zusi 3's vehicle editor are
  closed, and the data behind them is read by the simulator rather than sitting in the file.
  - **Metadata, variants and loads** (`VehicleSpec::meta`/`variants`/`loads`): class,
    manufacturer, year built, era, country, operator, author, description and a preview
    image below `mods/`; variants that override livery, era and running numbers (drawn
    from a seeded list, so a consist looks the same on every client) but never the
    physics; loads with their own mass and the glTF node that shows them. The main menu's
    vehicle browser lists all of it and dials the variant with ←/→; `models.rs` shows the
    carried load's node and loads the variant's own model file.
  - **Braked weight per brake position** (`brake_weight_g/_p/_r`) and the transition times
    as vehicle data instead of the UIC constants. `Train::brake_percentage` now reports the
    position each vehicle's changeover handle actually stands in. **G brakes with P's
    force**: its lower anscription is the standardised consequence of the 22 s transition,
    which the model simulates separately — scaling the force as well would count it twice.
  - **Key figures and the tractive effort diagram**: braked weight percentage (empty,
    laden, per position), axle load with the 22.5 t warning, adhesive mass and the adhesion
    limit against the stated starting effort, power-to-weight ratio, balancing speed; a
    multi-series plot of tractive effort per drive mode, dynamic brake, running resistance
    and the adhesion limit (`editor_ui::multi_plot`).
  - **Check report** (`vehicle-editor::validate`): errors, warnings and notes over the whole
    file — missing figures, contradicting speeds, bindings that point at no node, LOD order,
    a node bound twice — with the counts in the section header so a collapsed section cannot
    hide them. All nine reference vehicles are silent.
  - **Part function registry** (`sim_core::cab::PART_FUNCTIONS`): one source of truth for
    what the simulator reads, offered as a picker in the parts list and checked in the
    report. A name it does not know stays legal (`MODS.md` documents the field as free
    text) and is reported as a note, not an error.
  - **Display widgets** are edited in the editor, with a to-scale preview the widgets can be
    dragged around in, instead of by hand in the RON file.
  - **New from template**: the nine reference vehicles of `content::vehicles` as starting
    points, each with a tooltip naming drive, brake and train protection.
  - **GNT** (`sim_core::safety::de::gnt`): the tilting speed supervision, which is what makes
    `tilt_angle_deg` mean something. GNT data points as `Balise` payloads (no new device
    kind), profile release only with a working tilt system, braking curve to the target,
    forced braking, and the return run onto the regular profile when the tilt system fails.
    Under LZB guidance it stands down; the PZB magnets stay effective underneath it. Its
    lamps are in the HUD and bindable as cab parts. No train data entry and no function test
    yet — both are marked in the module.
  - **Not done, deliberately:** ETCS and ZBS (PLAN 9.5 calls them v2+, and a half-built
    supervision is worse than none), and a second country package.
  - **Multiplayer limitation:** variant and load live on `Vehicle` in `sim-core`, so they are
    deterministic and travel with the consist — but nothing produces a consist on the server
    yet (it builds from `Selection::default()`, which is already true of the chosen
    locomotive). They are therefore part of `world::fingerprint`, so a client that picked a
    different livery is refused at join instead of silently seeing another train than
    everyone else. The fix is a server-owned consist in the scenario, not replication.
- **Localisation:** every string the user reads goes through the `i18n` crate
  (Fluent `.ftl`, English source plus German), including the simulator HUD, both editors
  and the scoring report. Language from `TRAINSIM_LANG` or the operating system,
  switchable at runtime under View → Language; a test fails on a key that only one
  language has. Crowdin config in `crowdin.yml`. Text out of the mods (scenario
  messages, station names) is content, not code, and is not translated.
- **Vehicle models in the app (ch. 15.3):** a vehicle with a model gets its glTF instead of
  the placeholder body; the level of detail follows the camera distance, and the bound parts
  follow the simulation (pantograph, gauges, switches, lamps). `--camera outside` starts on
  the external camera, `--camera walk` on foot. The built-in reference BR 101 wears the
  example mod's dress — `content::vehicles::br101()` embeds `br101_afb.ron` and takes its
  model, cab, displays and sound table — so the loco without the AFB script has the same
  3D cab as the one with it; the vehicle editor's templates strip that dress again.
- **First person (ch. 12.4):** F4 stands the driver up out of the seat — a character
  controller that falls. He walks at 1.5 m/s, runs at 5 m/s with shift, looks around
  with the mouse on its own (the cursor is caught on a crosshair, so the cab controls
  keep taking clicks), climbs what is no higher than a step (platform edge, stair), is
  stopped by what stands at chest height and falls where the ground drops away. Space
  jumps — half a metre of air, off whatever he is standing on and only off something he
  is standing on, so the key is no second jump in mid-air. Ground and walls come out of
  two ray casts against the meshes that are drawn anyway, one down and one ahead, so
  nothing in the world needs collision data of its own — terrain, platforms, objects and
  a modelled interior all carry him as they are drawn; a vehicle without an interior
  holds him on the floor its `eye` implies. Past the end of a vehicle he walks on into
  the next one of the train, and `E` takes him through a door: out of an open passenger
  door or a traction unit's cab door at a stand, and back in at any vehicle beside him.
  He wears one of the mods' people, shown from the outside cameras (see *People* below;
  `--character` picks another). The driving keys rest while he is off the seat; F1 puts
  him back.
- **People (ch. 12):** a mod ships characters (`characters/*.ron`), and the `people` mod
  carries twenty-four of them generated out of MakeHuman 2 by `tools/characters/` (their
  glTF files are Git LFS objects, like every binary asset below `mods/`) — four
  levels of detail on one skeleton (about 30 000 / 6 000 / 1 600 / 500 triangles — the
  garments Loop-subdivided before the finest one, so a bust is round and not a facet), an
  opaque and a cut-out atlas each, and **motion capture clips** retargeted onto each
  person's own skeleton (`tools/characters/mocap.py`, from the CC BY collections 100STYLE
  and ACCAD — see `THIRD_PARTY_LICENSES.md`): four walks named after the pace they were
  walked at on these legs (`walk_<cm/s>` — a neutral walk, the actor's own, and two of
  the age band's styles: phones and pockets for the young, folded arms and a proud stride
  for the middle-aged, hands behind the back and a bowed head for the old), four idles of
  ten to twenty seconds (`idle`, `idle2`, …) and three seated clips (`sit`, `sit2`, …:
  the upper body of an idle over the chair pose, so passengers fold their arms or look
  at their phones). Every `Platform` device gets a **waiting crowd**
  (`content::people`): one person per six metres or so, at a random spot along the
  platform and 2.3–3.5 m beside the track on the platform's side, facing the track give
  or take, each in one of its model's idles drawn by the seed (`Pose::Idle(variant)`)
  with staggered starts, placed like the scenery objects — track pose, tile bucket, on
  the platform's height or on the ground — so they stream with the terrain tiles. The renderer (`world_render::people`) finishes
  each instance once its scene is there: the `_LOD<n>` nodes become visibility bands
  (hand-over at 30, 80 and 200 m, culled at 500 m, 300 m aboard a train), the clips one
  cached animation graph per glTF, and the atlases get the mip chain the pipeline does not
  ship. A vehicle whose model lists `seats` has about two thirds of them **taken**,
  decided by a hash of train, vehicle and seat, in one of the model's seated clips. The
  **walker** wears the first `Player` character (`--character people:f01_lena` or a file
  picks another) and is animated off his own pace — his idle at a stand, a walk on the
  move, cross-faded, the clip picked for the pace (`world_render::people::gait`: the one
  that covers the ground within 0.8–1.25× of its own speed, kept while it stays within
  0.7–1.4×, the nearest sped up or slowed when none fits) and the cycle sped up with the
  pace. Nothing of it is replicated: the crowd is a function
  of the line's name and the device index, the seats of the train's indices, so every
  client shows the same people; what another player's walker looks like is not sent, and
  remote walkers are not drawn.
- **Walking people (ch. 12):** people walk as well as stand, and where is content:
  a line draws **footpaths** (`walk_paths`, walked up and down by a few people at a time)
  and **walk areas** (`walk_areas`, a polygon some of whose people wander between spots
  inside it) — geo-positioned like the trees, height from the terrain; a scenery
  model carries its own as empty glTF nodes `wp_<name>_<i>` / `wa_<name>_<i>` with
  `people`/`width`/`walking_share` in the extras (`mods/example/assets/platform.gltf`,
  generated by `tools/gen_platform.py`, is a 210 m platform with a path along its edge
  and a waiting area, placed on the example line at the stop); and a third of a
  `Platform` device's crowd walks a lane behind the waiting crowd. Nobody walks through
  anybody by construction rather than by simulation: right-hand traffic and one pace per
  footpath, an oval with half-circle turns at its ends instead of stops, wanderers'
  spots and legs kept clear of the standers — a stepped avoidance would diverge between clients whose tiles stream at
  different times. The motion is a **pure function of
  the scenario clock**, a seed and the way (`content::people::stroll_pose`: laps of an
  oval along a path, eight seeded waypoints inside an area,
  1.0–1.6 m/s on a path, 0.8–1.3 m/s in an area), so nothing is stored or sent, every
  client sees the same people, and a paused run freezes them; `world_render::people`
  moves them every frame and cross-fades the walk picked for the agent's pace and
  variant (rate = pace / the clip's own pace) and the agent's idle on state changes only. The **route editor** draws both in a *People* group of the
  palette (`--tool walk-path` / `walk-area`): clicks add vertices, Enter finishes, corners
  drag, a side click inserts, Delete removes; the rule check reports too few points and
  vertices outside the envelope.
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
  are used. Anti-aliasing is two rows of its own — the technique (off, FXAA, SMAA, MSAA)
  and how hard it works (2×/4×/8× for MSAA, the preset for the other two) — swapped on the
  live camera as three different components, MSAA off wherever a post pass does the job.
  Shadows, mist and ground textures carry a quality of their own on the same Low/Medium/
  High scale: the sun's shadow map at 1024/2048/4096 texels, the raymarch through the mist
  at 16/32/64 steps, and the generated ground textures at 128²/256²/512² with 1×/4×/16×
  anisotropy — the last of them written back into the handles the terrain material already
  holds, so it reaches the tiles standing on screen and not only the ones built after it.
  The window is a three-way choice (windowed, borderless, exclusive fullscreen; the
  exclusive one names the primary monitor, because a window that does not exist yet is on
  none and Bevy panics rather than guessing), and a frame cap holds the program to a rate
  of its own — a slot-based sleep in `Last` that keeps its rhythm while it is met and
  starts afresh when it is missed, with the slider's top step meaning no cap at all.
  Nothing waits for a restart; a setting that needs one is an excuse. **Esc
  during a run raises the same menu as an overlay** (`GameState::Paused`, `spawn_pause`):
  no camera of its own — the cab's draws the UI — no wallpaper, a thinner scrim so the
  world stays recognisable, and Resume / Settings / Back to the main menu / Quit. Every
  driving system is gated on `Driving`, so the pause freezes simulation, clock and camera
  by itself. **Resuming does not build a second run:** it enters `Driving` again, and the
  chain behind `OnEnter(Driving)` is what builds one — `RunBuilt` is the resource that
  says a world already stands and holds the chain back until `tear_down_run` drops it, so
  the Esc that resumes gives the run back rather than putting a second world, a second
  camera and a second simulation on top of it. The overlay's settings page is the front
  end's minus the language and the reset. **Going back to the title screen tears the built
  world down** (`tear_down_run`): the run carries no despawn marker, so what is dropped is
  decided by a snapshot of the entities that existed *before* `setup` — everything newer
  goes, except resources (which are entities of their own in Bevy 0.19), observers, and
  what a plugin put up once at startup and marked `world_render::Persistent` (the cloud
  dome, the mist volume). The mixer's tracks are dropped with it, so the loops of the run
  stop, and walker and camera state — both of which point into the world that has just
  gone — go back to default.
  Any run flag on the command line (`--line`, `--frames`, …) skips the menu, so CLI and CI
  invocations stay non-interactive, and a flag beats the menu's choice where both are set;
  `--menu` puts the menu back in front, which is the only way to photograph it.
- **Key bindings are the player's** (`crates/app/src/bindings.rs`, page `--menu controls`):
  Bevy brings the raw devices and nothing above them, so a thin layer sits in between — a
  table of some sixty actions with their default key and controller button, a `Binds`
  resource subscripted by `Action as usize`, and one `SystemParam` that every driving
  system asks for in place of `ButtonInput<KeyCode>`. Settings → Key bindings lists them by
  group; `Enter` on a row takes the next key **or controller button** pressed, `Backspace`
  clears it, `Esc` cancels, and binding a key takes it off whoever had it, so one key only
  ever works one lever. Kept in the settings file under `[controls]` as one line per
  rebound row and nothing else, so a changed default still reaches everyone who never
  touched that row. A controller answers to the same bindings on every connected pad. **The
  three controls that have a position rather than a direction** — power controller,
  driver's brake valve, direct brake — are a group of their own (`Lever`), bound to a stick
  axis or an analogue trigger instead of to a key: `Enter` on such a row takes the next axis
  moved past half travel, and the axis then drives the lever absolutely, after and over the
  keys. Nothing is bound there out of the box, because a bound lever writes its control
  every frame and would hold the brake valve at Release for anyone with a pad plugged in;
  lap, fill and emergency stay on their keys, emergency latching until one leaves it. The
  two sticks that look and walk are the exception in the other direction — always on, never
  bindable, because they are not levers of the desk. The
  key sheet on **F5** no longer spells its caps out — each line names the actions it stands
  for and is rewritten the moment a binding changes, so a rebinding made in the pause
  overlay reaches the sheet standing behind it.
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
  of crashing). In the editor the centre toggles
  between 3D model and diagram (chips top left; `--graph` starts on the diagram); the
  former Brake/Drive/Equipment/Behaviour forms are replaced by the palette (searchable,
  grouped by category), per-block properties and **live bake findings** (a click selects
  the offending block), and axle count and adhesive mass moved onto the wheelset block.
- **Palette completed (2026-08-18):** the block system now covers every component the
  simulation has a model for — **72 built-in blocks** in nine groups, over nine port
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

- **Multiplayer over dedicated servers (ch. 20):** on
  [lightyear](https://github.com/cBournhonesque/lightyear) 0.29 (netcode over UDP), in the shape
  SimRail uses — the same binary is the server (`--dedicated <port>`, no window, no renderer, no
  sound card) and the client (`--connect <host:port>`); without either flag no socket is opened
  and single player is untouched. Client and server build the same world through the shared
  `app/src/world.rs` and exchange a fingerprint over line name and consists on joining. A client
  asks for the train its own scenario put it in and keeps it while it is free; a train a player
  took over stops being driven by the AI and goes back to it when that player leaves.
  What travels: the driver's levers (`CabInputs`) as events on a reliable channel — so the
  setpoint is replicated, never the result — and ten times a second the position of each train
  on the track, `(edge, s, dir, v, a)`, about 17 bytes and not a transform; the pose with its
  cant in curves is rebuilt from the spline on every client, and only the leading vehicle is
  sent because the rest of the consist follows from the couplers. A correction is **never**
  applied as a position: what arrives becomes a distance and a speed still owed, the speed taken
  over at 0.3 m/s² across every vehicle equally (so no coupler notices), the distance as a
  moment of running a fraction of a percent fast or slow — a train is only ever *placed* when it
  is more than 50 m out or the client has never seen it before, which is a resync, not a snap.
  Received states are extrapolated over the measured one-way latency, capped at half a second,
  which on a train is worth centimetres. Interest management: full 10 Hz within 3 km of a client,
  1 Hz out to 20 km, nothing beyond. The server's simulation clock rides along in every packet,
  so a client that joined late or stalled takes it over and resyncs. Measured against a
  dedicated server on the example line: after the join resync the leading train tracks the
  server to within ±0.3 m at 100 km/h with speeds matching to 0.01 m/s. New in `sim-core`:
  `Vehicle::a`, `Train::nudge`, `Train::place_head_at`; in `track-model`:
  `TrackPosition::distance_to`, which measures a correction along the track instead of through
  the air. The HUD carries a **Server** line (connection, train, round trip time, pending
  correction).

## Deliberately deferred

Every simplification is marked with a `ponytail:` comment at the code site, with an upgrade path:

- **A field has no holes and no standing crop.** `fields::geometry::clip` keeps outer rings
  only, so a pond or a copse in the middle of a field is drawn over rather than cut out; a
  polygon type with holes would have to run all the way through the mesh builder, and a hole
  in a German field block is rare enough to wait for that. And the surface lies on the
  ground: a maize field in August really does stand two and a half metres above the track,
  but the crop's height is colour and row contrast, not geometry. Standing crop wants its own
  pass — a shell over the surface with sides, or instanced cards.
- **The cropping statistics are national, not per district.** A state that publishes only
  field blocks has its crop drawn from `crops/arable.csv`, which is North Rhine-Westphalia's
  own area shares standing in for the country. The mechanism is per region already — the
  first column is the key, and a `cache/fields/crops/arable.csv` with a row per state
  overrides it — but the district-level figures the plan asks for (Destatis *Bodennutzung*,
  BKG's `vg2500_krs` for the boundaries) are not shipped. Until they are, a line through the
  Uckermark grows North Rhine-Westphalia's rotation, which is the right shape and the wrong
  region.
- **No foreign register is read.** Every EU member state owes the same publication, and the
  neighbours' have the same shape — Austria's AMA data is on data.gv.at under CC BY, with
  *Feldstücke* and *Schläge* carrying a land-use code. Adding one is a `Land::service`
  entry, a crop CSV and an attribution line, which is what a German state costs; the
  machinery does not care which country it is asking. What stands in the way is that
  `fields::land` is built around the sixteen German states, so a country needs a rung above
  `Land` before the second one is worth adding. Until then a module abroad gets the
  OpenStreetMap fallback, which is thinner and share-alike, and is told so.
- **Two states need a file the import cannot fetch.** Schleswig-Holstein publishes its field
  blocks as a yearly GeoPackage and no service, and Rhineland-Palatinate's best source is the
  ATKIS Basis-DLM out of a web shop. Both are open (dl-de/zero-2-0 and dl-de/by-2-0) and
  neither can be asked for a bounding box, so both want a "point the editor at the file you
  downloaded" path that does not exist. Until it does, Schleswig-Holstein reports what it
  needs and Rhineland-Palatinate makes do with OpenStreetMap.
- **No landscape elements.** Hedges, tree rows, field margins and copses are published in
  the same services (`Landschaftselemente`, `invekos:LandscapeFeature`) and are the thing
  that turns a patchwork of fields into a countryside. They are not imported: a hedge is a
  mesh along a line, which is a vegetation feature rather than a field one, and it belongs
  with the tree rows the forest brush cannot draw either.
- **Soil is one brown everywhere.** `world_render::farmland::SOIL` is a single colour under
  every crop, so a loess field in the Börde and a sandy one on the Geest are the same bare
  ground. The soil map (BÜK200, also open) would fix it, at the cost of a second import and
  a second attribution.
- **The service endpoints are a table in code.** `fields::land::Land::service` lists sixteen
  states' URLs, layer names and licences. They move about once a year, and a wrong one is a
  bug report rather than a setting — but it does mean a state that changes its endpoint needs
  a release. The BLE keeps the register that says when they move
  (<https://gdi.bmleh.de/geodaten/geodaten-aus-dem-invekos-eu-agrarfoerderung>); looking
  there belongs in the release routine, twice a year.
- **A shunt move that does not finish is placed instead.** A working with a road to go to is
  given the move on top of its timetable and ten minutes of window to drive it, and whatever
  the driver managed the unit is *placed* on the road when that window closes. That backstop
  is deliberate — the dispatching has to stay a pure function of the clock, and a driven move
  is not one — but it means a plan whose road is unreachable, or whose `stable_way` runs the
  wrong way, looks like a teleport rather than an error. A driver who has given up is not
  reported anywhere either; `ShuntPhase` knows, and nothing asks it.
- **Shunting has no editor and no file of its own yet.** A line's stabling roads and
  portals are written by hand in the `yards:` list, the way the interlocking tables were
  before they became forms — the route editor's rule check finds a bad one and jumps to its
  track, but there is no yard tool that drops one on the map. A **shunt job** is an
  `ai_driver::ShuntJob` that serialises to RON like a timetable, but the mod runtime loads
  no `jobs/` directory: a job reaches a driver where the world is built (`world::build`),
  not out of a file. Both are one `read_ron` call and one panel away, and neither changes
  the format.
- **A coupler kind belongs to the vehicle, not to its two ends.** `CouplerSpec::kind` is one
  value per vehicle, so a driving trailer with a bar at its inner end and a screw coupling
  at its outer one cannot be stated. The rules work around it — a bar only refuses where
  *both* neighbours carry one, which is what a bar inside a fixed unit means — but a vehicle
  with genuinely different gear at its two ends needs the field split in two.
- **`Sim::couple` takes the first consist it finds** within reach of either end, in train
  index order. Two vehicles up against both ends of the same train at once is a real
  situation (a loco between two rakes) and the order is then arbitrary; naming the end, or
  the train, is what `Sim::couple_to` is for.
- **Nothing offers the player the next working.** Their train is theirs for as long as they
  stay in it, and they may hand it over and take another one at any platform (`app::crew`) —
  but the plan never says "your service ends here, the 17:42 leaves from platform 2". Which
  working is due next is one `OperatingDay::active` call away; what it needs is somewhere on
  screen to say it, and that is a decision about the HUD rather than about the model.
- **An operating day is offered only on its own line.** A plan whose `line` names another module
  is left out of the picker, because its stops are indices into that line's track graph; a plan
  without a `line` is offered everywhere and is the mod author's own risk. Started against the
  wrong line anyway (`--day` with a mismatched `--line`), a service whose origin is off the end
  of the graph is a warning and a start at the beginning of the line — not a crash. What is not
  checked is the *stops*: one on an edge that does not exist is simply never reached, and the
  loader should say so instead of the AI driving past it forever.
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
  exactly that, several entries with overlapping curves. The example BR 101 has its
  recordings since 2026-08-29: loops cut out of CC BY trainspotting videos and CC0
  recordings (`tools/sounds/br101_sounds.py`, sources in THIRD_PARTY_LICENSES.md) — the
  line-side converters' whine while standing, the GTO converter at the start, the electric
  brake, the Makrofon, a buzzer each for the Sifa and the PZB/LZB, rolling noise in three
  bands, air and compressor (the electric brake and the Makrofon from two CC BY-SA
  recordings, which those files stay). The example line got the **distant signal at km 1.0**
  that its 1000 Hz magnet had been missing: `WhenRestrictive` asks the linked signal for its
  distant indication, and the main signal it hung on carries none, so the magnet had never
  been live and the classic acknowledgement case could not happen at all
  (`crates/content/tests/pzb_alert.rs` drives it). The sound table tells the Sifa from the
  train protection since the same day: `VigilanceAlert` and `ProtectionAlert` next to the
  combined `Alert`, and `DynamicBrake` for the electric brake's own force. The default
  table of every other vehicle stays generated, and so do the 101's brake squeal and rail
  joints. What no free recording covers is the loco under power at speed from inside, so
  the upper two traction and brake bands are resampled; no free recording of the PZB horn
  or the Sifa buzzer of a 101 exists either.
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
- **A turnout is placed from the track, never from the node** — laying from a track's
  middle splits the edge, and the throw time is edited on the tracks that meet at the
  joint. Node picking on the map (and with it crossings and double slips) is a
  selection kind of its own.
- **The stake-out calculator turns at most ~200° per arc** and seeds the double
  arc's automatic radius from the gap (a third of the distance, 300–5000 m) — a
  reversing loop connection stays possible, a whip-around from a mis-click does
  not, and a radius the seed cannot reach is entered as a fixed one. Two curved
  ends have no single-arc answer (that would take a true compound curve of several
  radii) and go to the double arc.
- **Eased pieces come from a Newton fit, plain pieces from closed form**: the lay
  tool's clothoid–arc–clothoid is fitted numerically onto the clicked point
  (curvature and arc length, ramps from the cant rulebook), and falls back to the
  bare arc where the click leaves no room for the ramps, under 50 m radius, or the
  iteration finds nothing — the status readout says which one is under the cursor.
  Joins (biarc), crossovers and turnout branches stay bare arcs on purpose: their
  prototypes carry no transition curves either. An eased edge offers no draggable
  support points (the arc-to-point refit would flatten its clothoids), which is why
  the option is off by default.
- **An offset clothoid is approximated**: the parallel of a transition curve is no
  clothoid, so `offset_edge` maps its end curvatures and scales its length by the
  midpoint — centimetres at track spacing, exact for the straights and arcs the
  drawing tools produce.
- **A crossover is a single connection** (two turnouts), not the double crossover the
  World Editor builds from a second click — the second diagonal is the same tool used
  once more the other way round.
- **The join weld keeps the geometry as clicked**: ends within a metre share a node
  from then on, but their coordinates stay their own — the weld is topological, the
  metre is the builder's to close (the lay tool's end snapping already lands exactly).
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
- **The track ribbon is not streamed** — the meshes are built per edge at startup. The
  bed is a real trapezoid now and the sleepers are chunk meshes with a 400 m cull band,
  but a 100 km line still builds everything at startup (a few hundred thousand vertices of
  rails and bed alone); the terrain's tile streaming has to reach it before 100 km lines
  with hundreds of thousands of sleepers stay cheap.
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
  step back. A long page scrolls to either: the keyboard is followed by `scroll_into_view`
  when the selection moves, and the wheel writes the offset itself (`scroll_menu`) — Bevy's
  UI keeps a scroll offset on the node but moves it for nobody. Following the selection
  every frame rather than only when it moves is what would make the wheel unusable, so it
  is a courtesy and not an invariant. Every page is the same list of rows (leading slot, label, provenance chip,
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
- **The in-game display is a laid-out HUD with drawn instruments** (`crates/app/src/hud.rs`,
  `glyphs.rs`). Everything on it is either **hardware** — the instrument panel at the
  bottom and the lamp housing beside it, a lighter surface with a lit top edge and a shadow
  under it — or **overlay**: type on the world under one wash across the width of the
  screen, no frame at all. Bottom centre is the **desk**: a round speedometer whose scale
  is drawn for the vehicle's maximum speed, with the line's permitted speed as an amber
  marker on the rim and the supervised speed as a red one over it; the **Doppelmanometer**
  carrying brake pipe and main reservoir on one face, pale needle and red needle as in the
  cab; the brake cylinder on its own shorter scale; and beside them the levers (power
  controller, brake valve, effort, reverser, AFB, distance). Bottom left the **train
  protection** as round lamps — 1000 Hz amber, 500 Hz and Befehl red, the train category,
  Sifa, and the LZB row with the MFA's three figures — where glass and legend light
  together in the lamp's own colour. Bottom right the **look-ahead**, signed the way the
  line signs it: the triangle of an Lf 7 board with the speed on it, or the disc of Hp 0.
  Top left the **run**: clock, a punctuality chip, the service, and the timetable as a
  **route ribbon** — the stops in order down a rail, the one behind dimmed, the next one
  the only line set large, and a wedge between them marking where the train stands with
  the distance to the next stop beside it. Punctuality carries the delay the train left
  the last stop with and adds however long the next scheduled arrival has been and gone; a
  train that has not reached a stop yet is never early, which is what stopped every run
  from opening seven minutes ahead of a stop it had not moved towards. Top right ten
  **annunciators** of the desk as pictograms plus three rows the drive labels itself, and
  the top centre free for scenario messages and the banner that says the protection has
  taken the train over.
- **The HUD's graphics are drawn rather than fetched** (`crates/app/src/glyphs.rs`): a
  small signed-distance rasteriser (segment, circle, rounded box, triangle; coverage over
  one texel is the anti-aliasing) and about thirty lines of geometry produce the dial
  faces, the needles, the rim markers, the Lf 7 board, the Hp 0 disc and the ten
  pictograms into `Image`s when the run starts. Same rule as the generated ground
  textures and the synthesised sounds of the default sound table: no asset directory, no
  icon set, no third-party licence to carry (the example BR 101's recorded sounds are
  the exception, credited in THIRD_PARTY_LICENSES.md) — and a pantograph that should read
  better at 20 px is a coordinate in that file.
  Everything comes out white on transparent and is tinted where it is used, so one drawing
  serves a lamp that is lit and one that is dark.
- **The display has three steps on F7** (`settings::HudMode`): full, reduced, off, and
  round again. Reduced keeps what the train is driven by — the desk and the protection
  lamps — plus anything that interrupts (the banner, scenario messages), and drops what it
  is planned by: the run, the systems and the look-ahead. It is a setting like any other,
  so the step survives the run; `--hud <step>` sets it for a screenshot through a resource
  of its own rather than through the setting — the settings file is written on exit whether
  anything changed or not, so an override put into the setting would be left behind in the
  player's preferences. A
  settings file from before this was three steps carries `hud = true`, which the loader
  cannot read into the enum and therefore leaves at `Full`.
- **The display is refilled in place, never respawned.** A figure carries `Readout`, a bar
  `Gauge`, a pointer `Needle` (a rotation of the whole square, so the spindle is the middle
  of the instrument by construction), a lamp `Lamp`, an annunciator `Chip` and a
  collapsible part `Block`; one loop per kind fills them from a `Frame` that reads the
  simulation once. The eight queries live in one `#[derive(SystemParam)]` bundle — a Bevy
  system takes sixteen parameters at most, and one system is what lets them share a single
  look-ahead scan. Nothing that does not apply is drawn: a block with nothing to say
  collapses instead of printing zeroes. What a driver has no use for — terrain, air detail,
  axles, temperatures, signals, network — moved to a diagnostics overlay on **F6**, and the
  key bindings to a sheet on **F5** that also says what the ten annunciators mean;
  `--overlays` opens both for a screenshot. Palette and the two faces are shared with the
  menu through `crates/app/src/theme.rs`.
- **The console on F8 (plan 16.3)** is the simulator's one command line: a panel on the
  bottom edge built from the same plain UI nodes as the rest of the game — no egui — with
  the commands in one static table (`weather`, `time`, `fly`, `help`, `clear`), `Tab`
  completion over commands and their arguments, and a history over `↑`/`↓`. Everything it
  prints goes through the i18n crate; commands and arguments stay English — a `preset` is
  one of `sim_core::weather`'s names in lower case, the same word the `--weather` flag
  takes, and a leading `/` (`/fly`) is tolerated spelling. `fly` toggles the free dev
  camera — a `CameraMode::Fly` of its own in `ui.rs`, detached from train and walker: it
  starts wherever the view was, flies where it looks (W/A/S/D along the view — the
  walker's keys, so rebindings carry over — Space up, Ctrl down, Shift five times as
  fast, right-drag to look), and leaves by the same F1–F4. Purely local, like every
  camera: the view is the one thing a client owns outright, so there is no wire shape for
  it.
  **Multiplayer**: the world is the server's, so a client's `weather` posts
  a wish (`net::WeatherRequest` → `WeatherWish`); the server applies it to its timeline
  and answers *every* client with a `WeatherSet` anchored to the moment it applied, so all
  peers run the same five-minute transition. `time` moves the run's clock, which the
  dispatcher, the scenario and the sun all hang off — a clock jump has no setpoint shape,
  so on a client it is refused rather than half-shipped. While the console is open it holds
  the keyboard: the driving keys, the walker, the takeover key and Esc all rest until it
  lets go.
- **A composed line runs one script** — the composition's, or the single module script
  found; further module scripts are dropped with a note. Running every module's hook
  side by side needs a script list in the runtime, nothing in the format.
- **Boundary snapping is a constant** (`compose::SNAP_DISTANCE`, 1 m) — module edges are
  placed at agreed coordinates. A per-composition tolerance steps in when real survey
  data needs one.
- **The simulation clock counts seconds since the start of the run**; the wall clock
  comes from the scenario's `start` (date, local time, UTC offset — default midsummer
  noon) via `Sim::clock()`. It anchors `Daily` timetables (`Timetable::delay`/
  `next_occurrence` take the start-of-day offset) and drives the whole sky: sun
  and moon positions from date, time and the georeferenced location
  (`world_coords::sun`, low-precision astronomy — 0.1° for the sun, a few degrees
  for the moon), and with them the atmosphere, the star sphere's rotation and the
  moon's phase (`world_render::sky`), so the season falls out of the date as well.
  Nothing of the sky is replicated: it is a pure function of the scenario clock,
  which is already the same on every machine, and of the line. **The route editor
  draws the same sky over the same module** — its Time-of-day panel sets date,
  clock, time zone and cloud cover, and a slider runs a whole day past; latitude
  and longitude are the module's anchor, exactly as in a run, so the sun comes over
  the same hillside in both programs. Two ceilings the sky carries: the camera's
  exposure is fixed at Bevy's default, so the sun's illuminance and the stars'
  luminance are lifted or lowered into it by constants rather than by an EV curve
  (`world_render::sky` names them and says what the correct version is), and a sun
  below the horizon still lights vertical faces from underneath, because Bevy hands
  the atmosphere the directional light's own colour and anything taken off the
  light would take the twilight sky with it. **The seasonal appearance hangs off
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
5. **Recorded samples for the sound table** — the example BR 101 has them (line-side
   converters, start, electric brake, Makrofon, buzzers, rolling in three bands, air,
   compressor); what is missing is a recording of the loco under power at speed, the real
   PZB and Sifa buzzers of a 101, brake squeal and rail joints, and the other reference
   vehicles. Rail joints out of the track instead of out of a distance interval belong in
   the same pass.
6. **Weather and night rendering are done** (M6 and plan 14.1, see above) — thirteen
   weathers as physical quantities, volumetric clouds with their shadows, haze in the
   atmosphere itself, wet and snowed-on surfaces, lightning and thunder, and a ground mist
   with the sun's shafts in it; headlights follow switch and direction of travel, red tail
   lamps (Zg 101) mark the rear end, the cab light has its switch and the instrument
   backlighting its dimmer, and the sound table hears `Rain` and `Thunder`; the cab's
   windscreens carry the rain themselves — a film, drops that run up the glass with speed,
   and the strip the wiper leaves clear behind its blade — off the panes a vehicle names in
   `cab: (windscreen: [...])`. What is left is per-vehicle content: a mod's own emissive
   panel, and the real lenses a modelled loco wants instead of the placeholder body's
   lamps.
