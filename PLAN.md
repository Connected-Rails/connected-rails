# Development plan: German train simulator on Bevy (working title "TrainSim-DE")

This plan is written as a complete set of working instructions for an executing AI / developer team.
Reference feature scope: MaSzyna EU07 (https://github.com/MaSzyna-EU07/maszyna), transferred to Germany
with German train protection systems (PZB, LZB, Sifa, …) and a country abstraction layer.

---

**IMPORTANT: This project is mod-first. Trains, signals, and lines are meant to be extended, not
hard-coded.** See **chapter 19: Mod runtime** below.

---

## 0. Vision

A simulator with the simulation depth of MaSzyna (not an arcade game):

- Physically correct driving dynamics of whole trains (longitudinal dynamics, couplers, buffers).
- Full air brake simulation (brake pipe as a pipe model, not just a "braking force slider").
- Electrical equipment as a circuit simulation (pantograph, main switch, traction motors/converters).
- German train protection: Sifa, PZB 90, LZB 80/CE, later ETCS — behind a country-neutral abstraction.
- German signalling system (Ks, H/V, Hl) with interlocking logic (routes).
- AI trains with timetables, event/scenario system.
- Walkable, fully operable 3D cab.
- Large worlds (100+ km lines), georeferenced (ETRS89/UTM), without f32 precision artefacts,
  robust across UTM zone boundaries.

Non-goals (v1): multiplayer (only architecturally prepared), passenger simulation, dynamic freight logistics.

---

## 1. Feature inventory from MaSzyna (target coverage)

Every feature gets a chapter in the plan below. Checklist of what MaSzyna can do and we must cover:

| MaSzyna feature | Adoption | Chapter |
|---|---|---|
| Longitudinal dynamics of whole trains (coupler/buffer springs, coupler breakage) | yes | 6 |
| Adhesion model, wheel slip/slide, sanding | yes | 6 |
| Air brake (driver's brake valve, control valves, brake pipe/main reservoir pressures, brake positions G/P/R(+Mg)) | yes | 7 |
| Electric traction (power controller, starting resistors / tap changer / converter, field weakening) | yes (German vehicles: tap changer BR 110/140, converter BR 101/185/423, diesel BR 218/BR 648) | 8 |
| Overhead line with voltage/pantograph interaction, main switch, train line | yes | 8 |
| Train protection (there SHP/CA/Radiostop) | replaced by Sifa/PZB/LZB + abstraction | 9 |
| Country-specific signals (there PL) | replaced by Ks/H/V/Hl + interlocking | 10 |
| Train radio (there Radio-Zew) | GSM-R-style train radio, emergency call | 10.5 |
| AI driver, timetables, train formation | yes | 11 |
| Event/scenario system (triggers, setting switches, announcements) | yes | 11.4 |
| Interactive 3D cab (all switches clickable, instruments) | yes | 12 |
| Interior/exterior cameras, free camera | yes | 12.4 |
| 3D sound (running noise, announcements, positional) | yes | 13 |
| Weather, day/night, seasons | yes | 14 |
| Line editor / content pipeline (there .scn/.e3d) | own format + importer | 15 |
| Physics console/debug tools | yes (egui overlays) | 16.3 |

---

## 2. Tech stack

- **Language:** Rust (stable), edition 2024.
- **Engine:** Bevy, latest stable version at project start (>= 0.15). Rendering, ECS, asset system,
  audio (`bevy_audio` suffices for now; `kira` if needed).
- **UI:** `bevy_egui` for debug/editor UI; cab instruments as 3D meshes + shaders, not as 2D UI.
- **Geodesy:** `proj4rs` or `geodesy` (pure Rust) for ETRS89/UTM ↔ ECEF/ENU conversion in the importer.
- **Serialisation:** `serde` + RON for hand-editable configs, binary format (`bincode`/own chunk format)
  for compiled line data.
- **Physics:** own implementation (longitudinal dynamics/pneumatics are domain-specific — Rapier and the like
  do not help here, at most for collisions/ragdoll matters later).
- **Repo structure:** Cargo workspace, one crate per domain (see 3.1).

---

## 3. Architecture

### 3.1 Workspace layout

```
train-sim/
├─ crates/
│  ├─ sim-core/          # fixed-timestep simulation, NO Bevy dependency (testable, headless)
│  │   ├─ physics/       #   longitudinal dynamics, adhesion
│  │   ├─ brakes/        #   pneumatics
│  │   ├─ electric/      #   traction, on-board power
│  │   ├─ safety/        #   train protection abstraction + country implementations
│  │   └─ interlock/     #   signals, routes, block logic
│  ├─ world-coords/      # f64 world coordinates, origin shifting, geo conversion
│  ├─ track-model/       # track geometry, topology, line data format
│  ├─ content/           # asset formats, importer (incl. geo reprojection), vehicle definitions
│  ├─ ai-driver/         # AI train driver, timetable
│  ├─ app/               # Bevy app: rendering, camera, input, audio, UI; binds sim-core to ECS
│  ├─ route-editor/      # route editor (own Bevy app, desktop UI)
│  └─ vehicle-editor/    # vehicle editor (own Bevy app, glTF import)
└─ assets/
```

**Core rule:** `sim-core` is a pure Rust lib with a fixed time step (recommended 100–200 Hz physics,
pneumatics with substeps if needed), deterministic, without Bevy. The Bevy app ticks it and mirrors state
into ECS components for rendering/audio/UI. That keeps everything headless-testable and leaves multiplayer
open for later.

### 3.2 Data flow per frame

1. Input → cab controls (set values).
2. `sim-core::step(dt)` in fixed steps (accumulator): electrics → traction/brake → longitudinal dynamics →
   position on the track graph → train protection → interlocking/signals → AI.
3. Sync to ECS: vehicle poses (f64 → rendered relative to the origin), instrument values, sound triggers.
4. Rendering/audio/UI.

---

## 4. Coordinate system & large worlds (critical, nail down first)

### 4.1 Problem statement

- Germany lies in UTM zones 32N and 33N (boundary at 12° east, straight through eastern Germany).
  Lines (e.g. Berlin–Hanover) cross the zone boundary. UTM coordinates from two zones cannot be joined;
  at the seam you get a jump plus direction/scale errors.
- f32 (Bevy `Transform`) has only ~3 cm resolution at UTM eastings (~500,000 m) → jitter.

### 4.2 Solution (binding)

**Internal world coordinates: ECEF in f64.** A global, Cartesian, projection-free system —
no zones, no seams, no distortion problem, all of Germany (or Europe) in one frame.

- All simulation (track geometry, vehicle positions) computes in `DVec3` (f64) in the ECEF frame
  (ETRS89 ellipsoid). At an earth radius of ~6.4e6 m, f64 has a resolution < 1 µm — more than enough.
- **Rendering: floating origin.** A `RenderOrigin` resource holds position (DVec3) + rotation
  (local ENU frame at the origin point, so that "up" stays +Y). On sync, every pose is written as
  `(f64 pose − origin)` into an f32 `Transform`. The origin jumps anew (to the camera position) as soon
  as the camera is > ~4 km away from it; on the jump the ENU rotation is redetermined as well → earth
  curvature is automatically correct (a distant line lies "below the horizon") without us ever modelling
  it explicitly.
- **UTM only in the importer:** source data (DB geodata, OSM, DGM in UTM32/33) is reprojected to ECEF
  during the line import via `geodesy`/`proj4rs`. At runtime, UTM does not exist.
  Zone boundaries are thus entirely an importer problem: the importer takes each file's CRS
  (EPSG code from the metadata) and converts — done.
- Heights: DGM heights (DHHN2016) are brought to ellipsoidal height via a geoid offset (approximately
  constant per line, ~46–50 m in Germany; v1: constant offset per line in the line header).

The `world-coords` crate provides: `EcefPos(DVec3)`, `RenderOrigin`, sync system, `geo::to_ecef(lat/lon/h)`,
`enu_frame_at(pos)`. Acceptance test: two points 300 km apart, camera travels back and forth, no jitter,
no visible jumps on origin rebase.

### 4.3 Streaming

- World in tiles (e.g. 2×2 km, key = quantised ECEF/ENU coordinate of the line).
- Async loading via Bevy assets, load radius around the camera + around all active trains (AI trains keep
  simulating even when unloaded — track graph + timetable data are always resident, only
  graphics/detail collision stream).

---

## 5. Track & line data model (`track-model`)

- **Topology:** graph of `TrackNode` (switches, buffer stops, connections) and `TrackEdge`.
- **Geometry per edge:** segment list of straight / circular arc / clothoid (transition curve) + cant ramp.
  Stored in the tile's local ENU frame, resolved to ECEF at runtime.
  API: `eval(edge, s) -> (EcefPos, tangent, cant)` — everything runs on arc length `s`.
- **Train position:** chain of `(EdgeId, s, direction)` per wheelset/bogie — vehicles are track-bound,
  no free-body 3D physics for driving.
- **Switches:** state (position left/right, trailed), throw time, locking by the interlocking.
- **Lineside equipment as trackside objects** with position `(EdgeId, s)`: signals, PZB magnets,
  LZB loop cable sections, balises (ETCS-ready), speed boards (Lf), gradient changes, platforms,
  stop boards, block boundaries. Country-neutral as `TracksideDevice { kind: DeviceKind, payload: ron::Value }` —
  the country plugins (ch. 9) interpret their own device types.
- Speed and gradient profile as a step function over `s` per line.

---

## 6. Driving physics (`sim-core::physics`)

Fixed timestep, per train:

- **Vehicle** = rigid body on the track with mass (empty/loaded), rotating masses (surcharge factor),
  length, running resistance (Davis formula a+bv+cv², vehicle database parameters), curve and gradient
  resistance from the track geometry.
- **Train consist:** vehicles connected via coupling elements: spring-damper with slack (screw coupling:
  draw spring + buffer separately, "stretching the train" on departure is noticeable; centre buffer coupler
  stiffer). Coupler breakage on overload (configurable). Integration: semi-implicit Euler, substepping when stiff.
- **Adhesion:** adhesion limit µ(v, rail condition [dry/wet/leaves]), wheel slip/slide per
  traction unit (v1: per vehicle, not per wheelset — add a `// ponytail` comment), sanding raises µ.
  Wheel slide/slip protection as a vehicle feature.
- **Acceptance tests (headless):** coasting trial against the Davis target curve, starting on a gradient,
  braking distance tables (see 7) against literature values (Minden values) ±5 %.

---

## 7. Brake system (`sim-core::brakes`)

MaSzyna level, i.e. real pneumatics:

- **Brake pipe** as a 1D pipe model along the train: one volume node per vehicle,
  throttle connections between vehicles → pressure wave travel time, propagation speed,
  a long freight train brakes later at the rear. (v1: a node model suffices; no PDE solver.)
- **Driver's brake valve** (positions fill/running/lap/service brake steps/emergency brake),
  time-dependent filling/venting, equalising.
- **Control valve** per vehicle (KE valve behaviour): three-pressure system, brake positions **G/P/R**,
  load braking (switchable automatically/manually), release surge, exhaustibility via auxiliary reservoir volume.
- **Further:** direct brake on the traction unit, electric (dynamic) brake with blending,
  Mg brake (with R+Mg and emergency braking), spring-applied/hand brake, passenger emergency brake,
  main reservoir + compressor (pressure switch), brake test procedure (full/simplified) as a scenario feature.
- **Output:** block/disc braking force → into longitudinal dynamics; brake cylinder/brake pipe/main reservoir
  gauge values for the cab.
- **Acceptance test:** braked weight percentage calculation of an example train, emergency braking distance
  from 100 km/h against the target value.

---

## 8. Electrics & traction (`sim-core::electric`)

Component-based on-board power simulation (directed circuit graph, no SPICE):

- **High voltage:** pantograph (raise/lower with travel time, contact with the overhead line only where a
  contact wire exists; neutral sections = de-energised sections as trackside objects), main switch,
  high-voltage transformer, 15 kV 16.7 Hz (Germany) — the voltage level is a country/line parameter.
- **Traction chains** (the vehicle database picks the type):
  1. **Transformer + tap changer** (older electric locos BR 110/140/141): step switch with switching time,
     traction motor characteristics.
  2. **Converter/three-phase** (BR 101/185/423/ICE): tractive/braking effort setpoint via an AFB-capable lever,
     tractive effort map over v, efficiency.
  3. **Diesel** (BR 218 hydraulic, BR 648 mechanical/hydraulic): engine map, gearbox/torque converter.
- **Auxiliaries:** battery, train line (heating), compressor, fans — as consumers with
  states, relevant for the start-up procedure.
- **Start-up** as a complete procedure: battery on → raise pantograph → main switch on → air compressor →
  test train protection. Checklist events for the tutorial system.

---

## 9. Train protection (`sim-core::safety`) — the centrepiece

### 9.1 Country abstraction

```rust
// Country-neutral interface. Every system is a state machine with defined inputs/outputs.
pub trait TrainProtectionSystem {
    fn update(&mut self, dt: f64, train: &TrainState, cab: &CabInputs,
              events: &[TracksideEvent]) -> ProtectionOutput;
    fn indicators(&self) -> &[Indicator];      // indicator lamps/displays for the cab
    fn isolate(&mut self, isolated: bool);      // isolating switch
}

pub struct TracksideEvent { pub device: DeviceKind, pub payload: ron::Value, pub s_offset: f64 }
pub enum ProtectionAction { None, ForcedServiceBrake, EmergencyBrake, TractionCutOff }
pub struct ProtectionOutput { pub action: ProtectionAction, pub speed_limit: Option<f64>, /* displays … */ }
```

- A vehicle carries a list `Vec<Box<dyn TrainProtectionSystem>>` (from the vehicle database).
- Trackside devices (ch. 5) generate `TracksideEvent`s when an antenna-carrying vehicle passes over them.
- A **country package** = a bundle of (train protection implementations + signalling system definition +
  trackside device types + rulebook parameters), registered as a Rust module/plugin. DE is the first;
  the Polish (SHP/CA) or Austrian systems would be later packages against the same API.

### 9.2 Sifa (implement first — the simplest state machine)

- Time-time Sifa (German standard): pedal/button; after 30 s without operation → indicator lamp,
  +2.5 s → buzzer, +2.5 s → forced braking (emergency brake), release only after a pedal change.
  Parameters per vehicle.
- Can be switched off (isolating switch, logged).

### 9.3 PZB 90 (complete, the heart of German operation)

- **Lineside:** 500 Hz, 1000 Hz, 2000 Hz track magnets as trackside devices; activation is
  signal-dependent (1000 Hz active at Vr0/Vr2, 2000 Hz at Hp0 — coupling to the signalling system, ch. 10).
- **On-board, complete PZB 90 logic:**
  - Train categories O/M/U with all check speeds and braking curves (165/125/105 → 85/70/55 etc.).
  - 1000 Hz influence: acknowledge within 4 s, 1000 Hz supervision for 1250 m, braking curve,
    exemption after 700 m (release button), 1000 Hz indicator lamp.
  - 500 Hz influence: immediate supervision (65/50/40 falling to 45/35/25), 250 m, no exemption.
  - Restrictive supervision (45/25 km/h) after a stop or v < 10 km/h for > 15 s within supervision.
  - 2000 Hz → forced braking; override 40 button (passing a signal at danger with written order, ≤ 40 km/h,
    override indicator lamp).
  - Forced braking logic: emergency brake to a standstill, release via acknowledge + conditions.
  - All indicator lamps (85/70/55, 1000 Hz, 500 Hz, override) + acknowledge/release/override buttons as CabInputs.
- **Acceptance:** test scenarios per standard case (table of PZB 90 supervision cases), headless as unit tests.

### 9.4 LZB 80/CE

- **Lineside:** loop cable sections (area identifier, block division) as trackside section objects;
  the LZB centre as part of the interlocking simulation issues the movement authority (distance to target,
  target speed) from the route/block state.
- **On-board:** entry check/takeover (takeover button), guidance via target/goal speed and
  distance to target (MFA displays: v-target, v-goal, distance to target, Ü/G/EL/ENDE/V40/B indicator lamps),
  braking curve supervision with forced braking, end procedure (LZB end → handover to PZB), failure procedure.
  CIR-ELKE mode as a parameter (shorter blocks, higher limits — purely a data setting).
- Under LZB guidance: PZB magnets suppressed (correct LZB↔PZB interaction).
- **AFB** (a separate vehicle feature, not a train protection system): target speed controller, uses the
  LZB setpoints.

### 9.5 Further German systems (after LZB, order as needed)

- **ETCS** L1 Limited Supervision/L2 (balises are already provided for as a trackside kind; DMI display;
  scope v2 — the architecture just must not preclude it).
- **ZBS** (Berlin S-Bahn), **GNT/tilting technology**: v2+, same trait API.

### 9.5a Door control (implemented)

`sim-core::doors` — TB0, TAV and UIC-WTB, chosen per train (`Train::doors`). Common to all
three: no traction while a door is not closed and locked, and an unlocked door above
5 km/h applies the emergency brake. TB0 needs the driver's close button, TAV closes by
itself, UIC-WTB is TAV over the train bus (inauguration after a consist change, one bus
cycle per vehicle). Vehicles take part when `VehicleSpec::passenger_doors` is set.

### 9.6 Train radio

- GSM-R-style: channel selection, registration by train number, emergency call (triggers emergency braking
  on AI trains in the vicinity), announcements from the dispatcher (scenario script). v1: UI + emergency call,
  no speech simulation.

---

## 10. Signalling system & interlocking (`sim-core::interlock`)

### 10.1 Signalling system abstraction

- A signal = a state machine with aspects (`SignalAspect`), the definition data-driven per country package:
  aspects, lamp images (for rendering), linking rules (the distant signal shows x when the main signal shows y),
  associated train protection effects (which magnet is active when).
- **German package v1:** the Ks system (Ks1/Ks2, Zs3/Zs3v indicators, mast signs, Zs1, marker light) **and**
  the H/V system (Hp0/1/2, Vr0/1/2, Sh1) — both are widespread. Hl (eastern network) v2. Lf/Ne/Zs boards as
  passive trackside objects.
- **Example data definition:** `signals_de.ron` describes aspects & rules; the renderer maps aspect →
  lamps on the signal mesh.

### 10.2 Interlocking logic

- **Routes:** defined as a path from the start to the destination signal via switch positions; locking
  (switches fixed), flank protection (v1: only switch locking, no complete flank protection graph —
  `// ponytail`), overlap, release by the train movement (axle counter sections = track clear detection).
- **Block safety** on the line: automatic block (the signal behind the train goes to danger, clears when the
  block is vacated).
- **Operation:** v1 fully automatic from the timetable (train routing: a route is requested when a train
  approaches per the timetable). A manual interlocking UI (player as dispatcher) = v2, the architecture
  (request → locking → signal) already supports it.
- The LZB centre (9.4) reads the same block/route state.

---

## 11. AI & timetable (`ai-driver`)

- **Timetable data model:** trains with train number, category, vehicle configuration, arrival/departure per
  operating point, track assignment. RON/CSV import.
- **AI train driver** (drives the same vehicle simulation as the player — no cheat physics):
  speed controller with look-ahead braking curve planning based on the line profile + signal aspects
  (the AI "sees" signals/temporary speed restrictions ahead via the track graph), stopping at the platform
  (stop board position), departure per timetable + departure signal, operation of Sifa/PZB (acknowledging),
  incident = simply come to a stand + radio message (v1).
- **Train formation:** shunting v2; v1 trains spawn/despawn at fiddle yards (portals at the edge of the line).
- **Scenario/event system:** triggers (time, train position, state) → actions (signal/switch, announcement,
  weather change, scoring, message). RON-based, equivalent to MaSzyna's event system.
- **Scoring:** timetable adherence, stopping accuracy, prohibited forced brake applications, energy
  consumption → log + score.

---

## 12. Cab, input, cameras (`app`)

- **3D cab:** vehicle model with named interaction nodes (`lever_fbv`, `btn_pzb_wachsam`, …).
  A mapping file per vehicle connects node ↔ sim input (axis/button/switch with detents).
  Mouse interaction via raycast (click/drag), alongside a complete keyboard layout and gamepad/RailDriver
  (v2: RailDriver HID).
- **Instruments:** needles as rotating sub-meshes (brake pipe/main reservoir/cylinder gauges, speedometer),
  MFA/EBuLa/displays as render-to-texture (egui-in-world or custom shaders). Indicator lamps = emissive toggle.
- **Walkable consist:** v2. v1: cab + exterior cameras.
- **Cameras:** cab (head freely pannable), exterior orbit, free-flying, lineside/pass-by camera.
- **Access to everything via keyboard** (the MaSzyna principle): fully operable without a mouse.

## 13. Audio

- Positional 3D sound: traction/diesel engine (RPM-dependent loops with crossfade), tap changer,
  compressor, brake hiss (coupled to valve events of the pneumatics!), block brake squeal,
  rail joints depending on speed, curve squeal, switch rumble (tied to track geometry events),
  horn, Sifa buzzer, PZB forced braking, announcements.
- Sound definitions in the vehicle file (event → sample + curves). Interior/exterior filter (lowpass in the cab).

## 14. Environment & rendering

- Terrain from the DGM (importer, ch. 15), texture splatting; vegetation/buildings as streamed instances.
- Overhead line procedurally from the line data (masts, catenary as mesh/shader curves).
- Day/night (sun position by date/location — the position is georeferenced after all), weather
  (rain/snow/fog, affects adhesion ch. 6 and visibility), seasons v2.
- Night lighting: signals, platforms, cab instrument lighting, headlights/marker lights.

## 15. Content pipeline & editor

- **Vehicle definition:** RON file (mass, Davis, brake equipment, traction type + maps,
  train protection equipment, sounds, cab mapping) + glTF models. Equivalent to MaSzyna's .fiz/.mmd, but readable.
- **Line source format:** editable RON (track graph, geometry, trackside devices, routes,
  scenery placements) → the compiler builds binary tiles (ch. 4.3).
- **Importer:** OSM track data (rough alignment) + DGM (terrain height) as the starting point of a line;
  CRS reprojection happens here (ch. 4.2). No MaSzyna .scn importer (effort > benefit, different country).
### 15.1 Two editors, not one

Building a line and building a vehicle share nothing: one is geodata, the other a model
with a data sheet. So there are **two programs**, each its own Bevy app:

- **`route-editor`:** track laying (clothoid tool), placing trackside devices (with rule
  checking: "1000 Hz magnet missing at the distant signal"), route definition, scenery,
  timetable editor, aerial imagery overlay as the template. Comes early (M3), otherwise
  there is no content.
- **`vehicle-editor`:** base data, glTF import, levels of detail, moving parts, brake and
  traction characteristics, later cab layout and sounds.

Both are **desktop applications, not game screens**: menu bar, docked panels, the operating
system's own file dialogs (`rfd`), status bar. UI with `bevy_egui` — a real native toolkit
(WinUI/GTK/Qt) would buy platform-native widgets at the price of losing the 3D viewport in
the same window, which is exactly what both editors are about.

### 15.2 Vehicle base data

Everything the running gear and the driving dynamics need is declaration, and each field is
one the modder can find in a data sheet: length over buffers (LÜP), gauge, v max, mass,
rotating mass allowance, number of axles, **axle base sum** (the sum over all bogies — this
is what forces the axles in a curve, not the vehicle length), rolling resistance, air
resistance cw·A, tilt angle, hunting factor, maximum payload.

### 15.3 Models: glTF and its own features

Models are glTF/GLB. Levels of detail and moving parts are found in the file; **the binding
is stored in the vehicle RON**, so a model needs no preparation in Blender — but a prepared
file needs no clicking either:

- **Levels of detail:** node names ending in `_LOD0`, `_LOD1`, … (in Blender that is just
  the object name — no add-on, unlike the `MSFT_lod` extension, which neither Blender nor
  Bevy supports out of the box). The distances are data, not model.
- **Moving parts:** name prefixes (`door_`, `pant_`, `sw_`, `gauge_`, `lamp_`, `wheel_`) as
  the zero-configuration fallback, and Blender custom properties (`ts_function`,
  `ts_motion`, `ts_axis`, `ts_amount`) — the exporter writes them into glTF `extras`, and
  they beat the name.
- **Motion** is `Visibility`, `Rotate` or `Translate` with an axis and an amount; `function`
  is a free-form string, exactly like the lamp images of a signal.

`mods/` is registered as a Bevy asset source, so a model is addressed as
`mods://<mod>/assets/<file>` — the same string in the editor and in the simulator. The app
spawns the scene in place of the placeholder body, shows the level of detail matching the
camera distance and drives the bound parts from the simulation state.

## 16. Cross-cutting concerns

1. **Determinism:** sim-core with a fixed timestep + own seeded RNG → replays, regression tests, MP option.
2. **Save/load:** the complete sim-core state serialisable (serde) — from the start, retrofitting is hell.
3. **Debug tools:** egui overlays: brake pressure diagram along the train, adhesion, train protection state
   machine, track graph view, signal/route state, fast-forward/pause/single step.
4. **Localisation:** UI strings external (fluent), German first, English carried along.
5. **Modding:** all content from `assets/` + RON — vehicles/lines/country packages without recompiling
   (country package logic in Rust stays compile-time; data is free).

---

## 17. Milestones (each with an acceptance criterion)

| M | Contents | Acceptance |
|---|---|---|
| **M0** | Workspace, `world-coords` (ECEF f64 + floating origin), 300 km test scene, camera | No jitter/jump on a 300 km distance flight |
| **M1** | `track-model` (graph, clothoids, eval), procedural track rendering, vehicle runs at constant speed on a test oval, streaming skeleton | Run over a switch, tile streaming without stutter |
| **M2** | Longitudinal dynamics + brake (ch. 6+7), one electric loco (converter type) + 5 coaches, basic sounds, basic cab (keyboard) | Headless braking distance tests green; starting/braking feels correct |
| **M3** | Sifa + PZB 90 complete, H/V + Ks signals static, **editor v1** (tracks + devices) | All PZB test cases green as unit tests; test run with 1000/500/2000 Hz cases |
| **M4** | Interlocking (routes, automatic block), AI trains + timetable, signal dynamics | Player + 3 AI trains on a 20 km line, conflict-free per timetable |
| **M5** | LZB 80 + AFB, MFA displays, tap-changer loco (BR 110) as a 2nd traction type | LZB guidance incl. end/failure procedures; transition LZB→PZB |
| **M6** | 3D cab fully interactive (mouse), start-up procedure, full audio, weather + night | "Cold loco" through to a line run entirely by mouse in the cab |
| **M7** | Pilot line (~30 km real, imported from OSM/DGM, across no less than one UTM zone boundary if feasible), scenario system, scoring, save/load | A playable 45-minute scenario with scoring |
| v2+ | ETCS, ZBS, GNT, detailed diesel models, shunting, dispatcher mode, multiplayer, RailDriver | — |

Rationale for the order: coordinates first (foundation, cannot be retrofitted), physics before train protection
(forced braking needs a real brake), editor before content, LZB after the interlocking (needs block data).

## 18. Test strategy

- `sim-core` headless: unit tests per state machine (PZB cases table-driven!), physics against
  literature values, determinism test (2 runs, same seed, identical state hash).
- Scenario regression tests: recorded input replays, target end state.
- CI: `cargo test` + clippy + fmt; rendering only as a smoke test (app starts, 100 frames).

## 19. Mod runtime (extensibility)

**Core principle:** trains, signals and lines are meant to be extended by players, not hard-coded
in the engine. Everything the simulator shows should be replaceable from a mod.

### 19.1 Data and behaviour are separate

The bulk of a locomotive is *declaration*, not script: model paths, wheel arrangement, masses,
tractive effort curve, brake valve parameters, sounds, cab layout. All of that is RON, validated
while loading and editable without programming.

Scripts only cover *real behaviour* — things a table cannot express:

| Declarative (RON) | Script (Lua) |
|---|---|
| masses, lengths, Davis coefficients | tap changer logic, AFB |
| brake equipment, braked weight, brake position | start-up procedure, fault behaviour |
| tractive effort/power/v_max per traction type | choice of a signal aspect beyond the table |
| track geometry, gradient, cant, speed | line events depending on state |
| signal aspect table (situation → aspect + lamps) | substitute signal, timers, dispatcher decisions |

That keeps roughly 80 % of every mod declarative: validatable, diffable, safe, and free of a
Lua interpreter in the hot path.

### 19.2 Lua as the scripting language

- **Why Lua:** light-weight, easy to sandbox, well-worn path (Roblox, Garry's Mod,
  Cities: Skylines, FS/MSFS).
- **Implementation:** `mlua` (Lua 5.4, statically linked, no external Lua binary).
- **Sandbox:** `table`, `string`, `math` — no `io`, no `os`, no `require`, no filesystem.
  A script sees a context table of numbers and booleans and answers with a table of overrides;
  it never holds a handle on the simulation. The trust boundary is a single place: the value
  check when the answer is applied (finite, clamped to the valid range).

### 19.3 Mod structure

```
mods/
└─ <id>/
   ├─ mod.ron          # id, name, version, author, description, depends, enabled
   ├─ vehicles/*.ron   # VehicleSpec — declaration, optionally naming a script
   ├─ lines/*.ron      # LineSource — track, equipment, signals
   ├─ scenarios/*.ron  # Scenario — triggers and actions
   ├─ signals/*.ron    # SignalType — aspect table, optional script hook
   ├─ scripts/*.lua    # behaviour
   └─ assets/…         # models, textures, sounds
```

Everything is addressed as `"<mod>:<file stem>"`, e.g. `example:br101_afb`, so two mods may
use the same file names.

### 19.4 Loading and fallback

1. **Discovery:** `mods/` is scanned at startup, `mod.ron` read (`enabled: false` skips a mod).
2. **Load order:** dependencies first (`depends`), alphabetical within that. A missing or
   circular dependency is a warning, the mod is loaded last anyway.
3. **Never crash:** a broken file is a warning, everything else still loads. A script that
   raises an error is disabled after the first error — the run continues. A signal type
   without a matching rule shows stop.

### 19.5 Signals: declarative state machine plus optional hook

The interlocking stays country-neutral. It computes only the *situation* of a signal — guarded
sections clear, route locked, diverging route, aspect of the following signal — and the signal
type maps that to an aspect. The first matching rule wins:

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

`lamps` are free-form strings — the presentation of the mod decides what they look like.
Anything the table cannot express gets `script: Some("<mod>:<name>")`; the hook runs after the
table, sees its result, and may override it:

```lua
-- Zs1 (substitute signal) after three minutes at stop — needs memory, so it cannot be a rule.
local M = {}
local DELAY, at_stop_since = 180.0, {}

function M.aspect(ctx)   -- ctx: signal, time, clear, route, diverging, next_stop, next_slow,
  if ctx.main ~= "stop" then      --      main, distant, speed (the table's result)
    at_stop_since[ctx.signal] = nil
    return nil                    -- nil = keep what the table decided
  end
  local since = at_stop_since[ctx.signal]
  if since == nil then at_stop_since[ctx.signal] = ctx.time return nil end
  if ctx.time - since >= DELAY then
    return { main = "substitute", speed = 40.0, lamps = { "red", "zs1" } }
  end
end

return M
```

A line points at a signal type by name; the mod runtime resolves it:

```ron
signals: [(kind: Main, system: Ks, device: 1, guarded: [0], signal_type: Some("example:ks_main"))],
```

### 19.6 Vehicles: declaration plus behaviour hook

`VehicleSpec` is unchanged data — the only addition is `script`. The hook is called once per
frame for the train whose leading vehicle names it and writes cab controls:

```lua
-- AFB: holds the speed set in the cab; the line speed always wins.
local M = {}

function M.update(ctx)   -- ctx: dt, time, v_kmh, speed_limit_kmh, mass_t, throttle, reverser,
  if not ctx.afb or ctx.reverser == 0 then   --   direct_brake, sanding, afb, afb_target,
    return nil                               --   brake_pipe, notch, line_voltage,
  end                                        --   tractive_effort
  local target = math.min(ctx.afb_target, ctx.speed_limit_kmh)
  local notch = (target - ctx.v_kmh) / 10.0
  return { throttle = math.max(-1.0, math.min(1.0, notch)) }   -- also: direct_brake, sanding
end

return M
```

### 19.7 Integration points

- **`mod-runtime` crate:** discovery, manifests, registry, Lua state, all four hooks.
- **`sim-core`:** knows the signal type as *data* (`interlock::SignalType`, `Situation`) and the
  script name on `VehicleSpec` — but no Lua. That keeps the core deterministic and serialisable.
- **`app`:** loads `mods/` at startup, `--line`/`--loco`/`--scenario <mod>:<name>` select
  content from a mod, one system per frame runs the hooks after `Sim::advance`, and F9 opens
  the mod manager (enable/disable, dependency check, loading warnings).
- Still open: `.trainsim` = zip with an installer.

### 19.7a Line and scenario hooks

A `LineSource` and a `Scenario` may name a `script` with `on_load(ctx)` and `on_frame(ctx)`.
The division of labour is the same as everywhere else — the script decides *when*, the RON
says *what*. An event with `trigger: Never` waits for the script to fire it by name:

```lua
function M.on_frame(ctx)   -- ctx: dt, time, trains, player, v_kmh, edge, s, finished,
  if ctx.time - standing_since >= 60.0 then   --   bonus, fired (event name → time)
    return { fire = { "stalled" } }           -- the event's actions run, straight out of RON
  end
end
```

Besides `fire` the answer may carry `message`/`announcement` and
`switch = { node, position }` — those are for a line that carries behaviour without a
scenario to fire events in.

### 19.8 Distribution

A `.trainsim` is a zip of the mod directory; the mod manager unpacks it to `<game>/mods/`.
---

## 20. Risks

| Risk | Countermeasure |
|---|---|
| The pneumatics model becomes numerically unstable | Substepping, implicit node solution, reference tests early |
| Bevy breaking changes | sim-core Bevy-free; keep the app layer thin; pin the Bevy version per milestone |
| Content bottleneck (building lines is expensive) | Editor early (M3), OSM/DGM import, **mods allow community lines** |
| Train protection rulebook implemented incorrectly | Derive test cases from Richtlinie 483 (PZB/LZB operating rules), table-driven |
| f64 ECEF errors creep into f32 paths | `EcefPos` as a newtype, clippy lint/review: no `as f32` outside the origin sync |
| Lua mods become the bottleneck | Pre-compile to bytecode, limit per-frame callback frequency, profile hotspots |
| Mods can't access sim state → not interesting | Lua API must support creative use cases; start with 3 concrete examples |
