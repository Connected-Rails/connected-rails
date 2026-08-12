# Modding guide

TrainSim-DE is built for mods: your own locomotives, your own signals, your own lines.
`mods/example/` is a working reference — copying it is the fastest way to start.

## Data and behaviour are separate

The bulk of a locomotive is **declaration**, not script: masses, running resistance, brake
equipment, tractive effort curve, later model paths, sounds and cab layout. All of that is RON —
validated while loading, diffable, editable without programming.

**Lua only covers real behaviour**, the things a table cannot express: tap changer logic, AFB,
start-up procedures, the choice of a signal aspect that depends on time or memory. That keeps
roughly 80 % of every mod declarative, checkable and safe.

## Structure of a mod

```
mods/<id>/mod.ron          manifest
         /vehicles/*.ron   VehicleSpec
         /lines/*.ron      LineSource
         /scenarios/*.ron  Scenario
         /signals/*.ron    SignalType
         /scripts/*.lua    behaviour
         /assets/…         models, textures, sounds — as `mods://<id>/assets/…`
```

Everything is addressed as `"<mod>:<file stem>"` — `mods/example/vehicles/br101_afb.ron`
becomes `example:br101_afb`. Two mods may therefore use the same file names.

`mod.ron`:

```ron
(
    id: "example",
    name: "Example Mod",
    version: "0.1.0",
    author: "TrainSim-DE",
    description: "Reference mod: a vehicle with an AFB script, two signal types and a line.",
    depends: [],
    enabled: true,
)
```

Only `id` and `name` are mandatory. `depends` lists mod ids that have to be loaded first;
`enabled: false` skips the mod without deleting it.

**Nothing is fatal.** A broken file produces a warning, everything else still loads. A script
that raises an error is switched off after the first error and the run continues. A signal type
without a matching rule shows stop.

## Vehicles

`vehicles/*.ron` is a `VehicleSpec` — plain data. See `mods/example/vehicles/br101_afb.ron`:

| Field | Meaning |
|---|---|
| `name` | display name |
| `length` | length over buffers [m] |
| `mass_empty` | tare mass [kg] |
| `rotating_mass_factor` | allowance for rotating masses (0.05 coach … 0.25 powered vehicle) |
| `davis` | running resistance `R = a + b·v + c·v²` [N], `v` in m/s |
| `brake` | control valve, friction pairing, brake position, braked weight [t], forces, pressures, reservoir volumes, additional brakes |
| `traction` | `Curve` / `TapChanger` / `Converter` / `Diesel` — or `None`; see below |
| `coupler` | slack, stiffnesses, damping, breaking force |
| `adhesive_mass_fraction` | share of the mass on driven axles (loco 1.0, coach 0.0) |
| `slip_protection` | `None` / `SlipBrake` / `TractionCutback` / `CreepControl` |
| `gauge` | track gauge [m], standard gauge 1.435 |
| `v_max` | highest permitted running speed [km/h] — the running gear limit |
| `axles` | number of axles; information only |
| `axle_base_sum` | total axle base [m], sum over all bogies — basis of the curve resistance |
| `cw_a` | air resistance cw·A [m²]; replaces the quadratic Davis term |
| `curve_resistance_factor` | factor on the Röckl curve resistance, 1 = standard |
| `max_payload` | maximum payload [kg] |
| `tilt_angle_deg` | maximum tilt angle [°], 0 without tilting technology |
| `hunting` | hunting −1 … 1, 0 = standard |
| `script` | optional behaviour hook `"<mod>:<name>"` |
| `model` | glTF file, levels of detail, moving parts — see below |

Start it with `cargo run -p app -- --loco example:br101_afb`, edit it with
`cargo run -p vehicle-editor -- mods/example/vehicles/br101_afb.ron`.

### Brake

Every vehicle brakes for itself — the brake pipe runs as a chain of nodes along the train, so
the rear of a long freight train applies seconds after the front.

| Field | Meaning |
|---|---|
| `valve` | control valve: `KGp` (single-release), `KeGp`, `KeGpr` (with R position), `KeTm`, `KeL2a` (second cylinder pressure stage by speed), `KeL2d` (by a full application) |
| `valve_params` | overrides the type's preset field by field: `graduated_release`, `rapid_position`, `high_stage`, `high_stage_trigger`, `loco`, `response_drop`, `full_service_drop` |
| `kind` | friction pairing: `Block` (cast iron), `Disc`, `CompositeK`, `CompositeLl`, `Magnetic`, or `Custom([(km/h, µ), …])` for your own measurements |
| `position` | `G` / `P` / `R` / `RMg`; an `R` on a valve without an R position falls back to `P` |
| `brake_weight`, `max_force`, `max_cylinder`, `cylinder_to_reservoir` | braked weight [t], force at full pressure and standstill [N], cylinder pressure [bar], volume ratio (exhaustibility) |
| `has_mg`, `mg_force` | magnetic track brake and its force [N] |
| `has_direct`, `direct_max_cylinder` | direct (additional) brake of a traction unit |
| `parking_force`, `spring_parking` | parking brake [N]; with `spring_parking` it is held off by air and applies by itself when the main reservoir runs empty |
| `pilot_controlled` | pre-controlled cylinder: fed from the main reservoir through a relay valve, fills faster, cannot exhaust — and it is what the electrically transmitted (ep) brake acts on |
| `supplement_brake` | air supplement brake: fills up whatever the dynamic brake falls short of |
| `angleicher` | equalising device: makes up brake pipe leakage in lap position. Deliberately **without a memory** — leaving the lap position throws the set point away |
| `aux_volume`, `pipe_volume`, `main_volume` | reservoir volumes [l]; `main_volume` 0 = no main reservoir |
| `compressor_delivery`, `leakage` | compressor delivery and leakage [l/min of free air] |

### Traction

The detailed data is optional everywhere: without it the variant runs on the tractive effort
hyperbola from `max_force`/`max_power`.

```ron
// The simplified model: tractive effort straight off the diagram.
traction: Some(Curve(
    force: [(0.0, 200000.0), (50.0, 120000.0), (150.0, 40000.0)],
    v_max: 160.0,
    brake: [],           // dynamic brake over speed, empty = none
    ramp_time: 2.0,
)),

// Series-wound drive behind a tap changer: the characteristic follows from the
// machine equations, not from a table.
traction: Some(TapChanger(
    steps: 28, max_force: 275000.0, max_power: 3620000.0, v_max: 150.0, step_time: 0.8,
    motor: Some((
        count: 4, resistance: 0.05, flux_constant: 0.0289,
        saturation_current: 600.0, max_current: 1600.0, max_voltage: 1000.0,
        field_steps: [1.0, 0.85, 0.7],
        gear_ratio: 2.17, wheel_diameter: 1.25, efficiency: 0.95,
    )),
    dynamic_brake: None,          // rheostatic brake, if the loco has one
)),

// Three-phase drive. Above `v_pullout` the effort falls with 1/v² instead of 1/v.
traction: Some(Converter(
    max_force: 300000.0, max_power: 6400000.0, v_max: 220.0,
    brake_force: 150000.0, brake_power: 2600000.0, ramp_time: 2.5,
    v_pullout: 150.0, regenerative: true, brake_fade_kmh: 10.0,
)),

// Diesel-hydraulic: engine map plus converters that are engaged by filling them.
traction: Some(Diesel(
    max_force: 235000.0, max_power: 1840000.0, v_max: 140.0,
    ramp_time: 4.0, start_time: 8.0,
    engine: Some((
        idle_rpm: 600.0, rated_rpm: 1500.0, max_rpm: 1650.0,
        torque_curve: [(600.0, 9000.0), (1000.0, 13500.0), (1500.0, 13115.0)],
        governor: Speed(steps: 0),   // or Fill — rack instead of engine speed
        inertia: 60.0, response_time: 1.0,
    )),
    transmission: Some((
        circuits: [
            (kind: Converter, ratio: 3.93, stall_ratio: 2.4, coupling_nu: 0.85,
             absorption: 0.53, shift_up_kmh: 72.0),
            (kind: Converter, ratio: 1.50, stall_ratio: 1.9, coupling_nu: 0.85,
             absorption: 0.53, shift_up_kmh: 0.0),
        ],
        fill_steps: 0,        // 0 continuous, 1 fill/empty only, n partial filling stages
        fill_time: 1.2, hysteresis_kmh: 10.0,
        final_ratio: 1.0, wheel_diameter: 1.0, count: 1, efficiency: 0.95,
    )),
    hydrodynamic_brake: None,
)),
```

`absorption` λ is the pump wheel's torque coefficient: `M_pump = λ·ω²·fill`. Set it so the
circuit absorbs the engine's rated torque at rated speed — `λ = M_rated / ω_rated²`.
`shift_up_kmh` and `hysteresis_kmh` are the change points of the original; the last circuit
ignores its change-up point.

### Model (glTF)

Models are glTF/GLB and use the format's own features. `mods/` is registered as an asset
source, so the path is stated **relative to the mods directory** — `<mod>/assets/<file>`,
loaded as `mods://<mod>/assets/<file>`. Everything else in a mod's `assets/` directory
(textures, sounds) is reachable the same way.

The example model `example/assets/br101.gltf` is generated by `tools/gen_br101.py` — a
procedural BR 101 built from scratch (no third-party assets, licensed like the project);
re-run the script after editing it.

```ron
model: Some((
    file: "example/assets/br101.gltf",
    lods: [(level: 0, distance: 150.0), (level: 1, distance: 400.0)],
    parts: [
        (node: "door_left", function: "door_left",
         motion: Translate(axis: (0.0, 0.0, 1.0), metres: 0.8)),
        (node: "pantograph_rear", function: "pantograph",
         motion: Rotate(axis: (1.0, 0.0, 0.0), degrees: 45.0)),
        (node: "gauge_speed", function: "gauge:speed",
         motion: Rotate(axis: (0.0, 0.0, 1.0), degrees: -270.0)),
        (node: "lamp_left", function: "lamp:left", motion: Visibility),
    ],
))
```

`function` is a free-form string, exactly like the lamp images of a signal: the app maps
the names it knows, mods may invent their own. `motion` describes how the node moves as the
function goes from 0 to 1 — `Visibility`, `Rotate` or `Translate`.

**Nothing has to be prepared in Blender.** The vehicle editor lists every node of the file
and you bind it with one click; the binding lands in the RON, the model stays untouched.
Two conventions are recognised on top of that, so that a prepared file needs no clicking:

| In Blender | Effect |
|---|---|
| Object name `body_LOD0`, `body_LOD1`, … | "Read from node names" fills the LOD table with default distances (150 / 400 / 1000 / 4000 m) |
| Object name with the prefix `door_`, `pant_`, `sw_`, `gauge_`, `lamp_`, `wheel_`/`axle_` | suggested function plus a sensible motion |
| Custom property `ts_function` on the object | exported into glTF `extras` and beats the name. Optional: `ts_motion` (`rotate`/`translate`/`visibility`), `ts_axis` (`"0 0 1"`), `ts_amount` (degrees or metres) |

Custom properties are set in Blender under *Object Properties → Custom Properties*; the
glTF exporter writes them into `extras` if "Include → Custom Properties" is switched on.

The simulator spawns the model in place of the placeholder body, shows the level of detail
whose distance the vehicle is within, and moves the bound parts. Which functions have a
value today: `pantograph`, `gauge:speed`, `gauge:brake_pipe`, `gauge:cylinder`,
`gauge:main_reservoir`, `gauge:tractive_effort`, `switch:throttle`, `switch:reverser`,
`switch:direct_brake`, `lamp:main_switch`, `lamp:sanding`. Everything else stays in its rest
position until `sim-core` models the state (doors, destination displays, marker lights).

```bash
cargo run -p app -- --loco example:br101_afb --camera outside
```

Modelling rules that matter for the simulation:

- **Length over buffers** is the RON value, not the model — but the buffers should be drawn
  1–2 cm compressed so that vehicles do not intersect in curves.
- **Origin** at the vehicle centre, on the top of the rails; the vehicle runs along −Z/+Z.
- **LOD0** is the close-up model; higher levels get coarser. What is not visible from 150 m
  away does not belong in LOD1.

### Behaviour hook `update(ctx)`

Called once per frame for the train whose leading vehicle names a script. `ctx` is read-only;
the returned table is applied to the cab controls, `nil` leaves the driver in charge.

| `ctx` | |
|---|---|
| `dt`, `time` | frame time and simulation time [s] |
| `v_kmh`, `speed_limit_kmh` | speed and permitted line speed [km/h] |
| `mass_t` | train mass [t] |
| `throttle`, `reverser`, `direct_brake`, `sanding` | current cab controls |
| `afb`, `afb_target` | AFB switched on, target speed [km/h] |
| `brake_pipe` | brake pipe pressure at the leading vehicle [bar] |
| `notch`, `line_voltage`, `tractive_effort` | traction state |

| return | |
|---|---|
| `throttle` | power controller −1 … +1 (negative = dynamic brake) |
| `direct_brake` | direct brake 0 … 1 |
| `sanding` | boolean |

Values are checked when applied: non-finite numbers are ignored, the rest is clamped.

`mods/example/scripts/afb.lua` — the AFB that `sim-core` does not implement:

```lua
local M = {}
local BAND = 10.0   -- proportional band [km/h]

function M.update(ctx)
  if not ctx.afb or ctx.reverser == 0 then
    return nil
  end
  local target = math.min(ctx.afb_target, ctx.speed_limit_kmh)
  local notch = (target - ctx.v_kmh) / BAND
  if notch > 1.0 then notch = 1.0 end
  if notch < -1.0 then notch = -1.0 end
  return { throttle = notch }
end

return M
```

## Signals

The interlocking stays country-neutral: it computes the *situation* of a signal, the signal type
maps it to an aspect. The first matching rule wins, an empty `when: ()` matches everything.

| `when` | |
|---|---|
| `clear` | all guarded sections are clear |
| `route` | a route is locked at this signal |
| `diverging` | the locked route leads over a diverging path |
| `next_stop` | the following main signal shows stop |
| `next_slow` | the following main signal shows slow speed |

| `show` | |
|---|---|
| `main` | `Stop`, `Proceed`, `ProceedSlow`, `Substitute`, `DarkLight` |
| `distant` | `ExpectStop`, `ExpectProceed`, `ExpectSlow` |
| `speed` | Zs3 speed indicator [km/h] |

`lamps` is a list of free-form strings for the presentation — your own assets decide what they
look like. `mods/example/signals/ks_main.ron`:

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

A line references the type by name:

```ron
signals: [(kind: Main, system: Ks, device: 1, guarded: [0], signal_type: Some("example:ks_main"))],
```

### Aspect hook `aspect(ctx)`

Runs after the rule table and sees its result. `nil` keeps that result.

| `ctx` | |
|---|---|
| `signal`, `time` | signal index and simulation time [s] |
| `clear`, `route`, `diverging`, `next_stop`, `next_slow` | the situation, as above |
| `main`, `distant`, `speed` | what the table decided (`"stop"`, `"proceed"`, `"proceed_slow"`, `"substitute"`, `"dark"`; `"expect_stop"`, `"expect_proceed"`, `"expect_slow"`) |

The return table takes the same `main`, `distant`, `speed` and `lamps`.

`mods/example/scripts/zs1.lua` — the substitute signal, a case that needs memory:

```lua
local M = {}
local DELAY = 180.0
local at_stop_since = {}

function M.aspect(ctx)
  if ctx.main ~= "stop" then
    at_stop_since[ctx.signal] = nil
    return nil
  end
  local since = at_stop_since[ctx.signal]
  if since == nil then
    at_stop_since[ctx.signal] = ctx.time
    return nil
  end
  if ctx.time - since >= DELAY then
    return { main = "substitute", speed = 40.0, lamps = { "red", "zs1" } }
  end
  return nil
end

return M
```

## Lines and scenarios

`lines/*.ron` is a `LineSource` — nodes, edges with geometry (straight/curve/clothoid), gradient,
cant, permitted speed, trackside equipment, sections, signals and routes. The format is the same
one the OSM/DGM importer writes, so `cargo run -p content --bin import-line` produces a file you
can drop into a mod unchanged. `mods/example/lines/beispielstrecke.ron` is a short hand-written
example.

Start it with `cargo run -p app -- --line example:beispielstrecke`.

A `LineConductor` device carries the LZB telegram as its payload:

```ron
(kind: LineConductor, edge: 2, s: 0.0, payload: "(permitted_speed:160.0,target_speed:0.0,\
target_distance:3000.0,length:3000.0,block_mode:Full,cir_elke:false)"),
```

`block_mode` is `Full` (LZB block markers instead of signals) or `Partial` (the LZB is laid
over the signal block division, the signals stay binding and their PZB magnets keep working).
`cir_elke: true` marks a CIR-ELKE section: steeper braking curve, 5 km/h speed steps, and
speed rises that take effect at the head of the train instead of at its rear. Both fields
may be left out — the defaults are `Full` and `false`.

`scenarios/*.ron` is a `Scenario` — triggers and actions, see the README section on scenarios.
Line and scenario hooks (`on_load`, `on_frame`) do not exist yet.

## Sandbox

Scripts get `table`, `string` and `math` — no `io`, no `os`, no `require`, no filesystem, no
network. A script sees a context table of numbers and booleans and answers with a table; it
never holds a handle on the simulation. State between calls lives in the script's own locals,
as in `zs1.lua` above.

Hooks are called **once per frame**, not once per simulation step (200 Hz). Keep them cheap
anyway — no long loops, cache what you can.

## Testing

```bash
cargo test -p mod-runtime                     # loads mods/, parses everything, runs the hooks
cargo run -p app -- --line example:beispielstrecke --loco example:br101_afb --frames 120
cargo run -p vehicle-editor -- mods/example/vehicles/br101_afb.ron
cargo run -p route-editor -- mods/example/lines/beispielstrecke.ron
```

Loading warnings and script errors go to the log with a `mod:` prefix.

## Distribution

Planned: `.trainsim` = a zip of the mod directory, unpacked to `<game>/mods/` by a mod manager.
For now, copy the directory into `mods/` by hand.
