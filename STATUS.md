# Implementation status against PLAN.md

As of 2026-08-12 · `cargo test --workspace`: **141 tests green** · clippy and fmt clean.

## Milestones

| M | Contents | Status |
|---|---|---|
| **M0** | Workspace, `world-coords` (ECEF f64 + floating origin) | **done** — acceptance test "300 km without jitter/jump" green |
| **M1** | `track-model`, procedural track rendering, streaming | **partial** — graph, clothoids, `eval`, switches (incl. trailing moves), track meshes done; **tile streaming (plan 4.3) missing** |
| **M2** | Longitudinal dynamics + brake, electric loco + coaches, basic cab | **done except audio** — coasting against Davis, emergency braking distance, starting on a gradient, coupler slack as tests; **sound (ch. 13) missing** |
| **M3** | Sifa + PZB 90, signals, editor v1 | **partial** — Sifa and PZB 90 complete with standard-case tests; H/V + Ks signal logic present, but without lamp aspect rendering; the **editor** exists as a top-down view with aerial imagery overlay, but cannot edit anything yet |
| **M4** | Interlocking, AI trains, timetable | **done** — routes with locking/release, automatic block, AI stops at signals and platforms |
| **M5** | LZB 80 + AFB, MFA, tap-changer loco | **partial** — LZB with guidance, braking curve, end and failure procedures; BR 110 present; **AFB missing**, MFA only as HUD text |
| **M6** | Interactive 3D cab, start-up procedure, audio, weather/night | **partial** — start-up chain simulated and operable via keyboard, weather changes through scenario actions, terrain from the DGM; no 3D cab, no audio, no vegetation/texturing |
| **M7** | Pilot line from OSM/DGM, scenarios, scoring, save/load | **largely done** — scenario system, scoring, save/load and the OSM/DGM importer are in place; only a real pilot line is missing (data procurement) |

## What is in place

- **Coordinates (ch. 4):** ECEF f64, ENU frames, floating origin with rebase every 4 km incl.
  new ENU rotation, earth curvature correction of the tangent plane, UTM↔geodetic for the import.
- **Track model (ch. 5):** one segment type (`k0`, `dk`) for straight, curve and clothoid;
  switches with throw time, locking and trailing-move detection; trackside devices with RON payload.
- **Driving dynamics (ch. 6):** Davis, curve and gradient resistance, couplers with slack and
  breaking force, Curtius/Kniffler adhesion with wheel slip/slide, sanding, wheel slide protection.
- **Brake (ch. 7):** brake pipe as a node chain, KE control valve (three-pressure system,
  exhaustibility), brake positions G/P/R(+Mg), direct brake, blending with the electric brake,
  braked weight percentage.
- **Electrics (ch. 8):** start-up chain, pantograph travel time, main switch drop-out in
  neutral sections, three traction types (tap changer, converter, diesel).
- **Train protection (ch. 9):** trait abstraction + country package DE with Sifa, complete
  PZB 90 (O/M/U, 1000/500/2000 Hz, exemption, restrictive supervision, override 40) and
  LZB 80 (takeover, v-target/v-goal/distance to target, end and failure procedures).
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
  at the track, view distance limit per LOD level in the app.
- **Aerial imagery overlay (ch. 15):** dedicated `imagery` crate with Web Mercator tile maths,
  provider configuration (tile template or WMS, placeholders, keys, zoom limits,
  attribution) and a two-level cache (memory + disk, budget with eviction of the oldest
  tiles, maximum age, offline mode). Fetching and decoding run on worker threads, the editor
  places the tiles georeferenced beneath the track ribbon.
  All of it controllable through a RON file, reloadable at runtime.
- **Cross-cutting (ch. 16):** fixed time step, seeded RNG, state hash with determinism test,
  full serialisation for save/load.

## Deliberately deferred

Every simplification is marked with a `ponytail:` comment at the code site, with an upgrade path:

- **Brake pipe model** is a node/diffusion model, not a pressure wave.
- **Slip per vehicle**, not per wheelset.
- **Flank protection** is only switch locking.
- **LZB braking curve** with a fixed deceleration instead of train-specific brake assessment.
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
- **Terrain tiles are built at startup**, not at runtime — for 100 km lines, asynchronous
  loading needs to be retrofitted.
- **Transition curve length and cant come from the rulebook**, not from the data: neither can be
  recovered from a noisy point sequence (the section boundary is uncertain by more than a
  hundred metres). Where the source deviates from this, the reconstruction deviates by a few
  metres — the same order of magnitude as the positional accuracy of OSM itself.
- **No routing across switches** when chaining, and station areas with multiple tracks are not
  separated.

## Sensible next steps

1. **Editor with tools**: so far it only displays. Next up: drawing alignments, placing
   switches and signals, positioning platforms — the aerial image behind it is the
   template for that.
2. **Import a real pilot line** (Overpass extract + DGM1 from a state surveying office).
3. **Switch catalogue**: standard switches (EW 190-1:9 … EW 1200-1:18.5) as a data table with
   radius, branch length and diverging speed; OSM only supplies a `railway=switch` node
   without any geometry.
4. **Carry over OSM equipment**: signals, platforms, stopping points and level crossings —
   then the import directly yields an equipped line instead of a bare strand.
5. **Evaluate better sources** than OSM: the EU's RINF infrastructure register
   (speeds, gradients, train protection, partly minimum radii) and DB's open geodata.
6. **Terrain streaming (ch. 4.3)** — tiles are currently built at startup; at 100 km they
   must be loaded and discarded at runtime. The tile structure already supports this,
   only the asynchronous generation is missing.
7. **Texturing/vegetation** — the terrain is single-coloured; splatting and instancing are missing.
8. **Audio (ch. 13)** — `sim-core` already emits the events.
9. **3D cab (M6)** — the biggest chunk.
