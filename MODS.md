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
| `brake` | brake type, brake position, braked weight [t], forces, pressures |
| `traction` | `TapChanger` / `Converter` / `Diesel` with force, power, v_max — or `None` |
| `coupler` | slack, stiffnesses, damping, breaking force |
| `adhesive_mass_fraction` | share of the mass on driven axles (loco 1.0, coach 0.0) |
| `slip_control` | wheel slip / wheel slide protection fitted |
| `gauge` | track gauge [m], standard gauge 1.435 |
| `v_max` | highest permitted running speed [km/h] — the running gear limit |
| `axles` | number of axles; information only |
| `axle_base_sum` | total axle base [m], sum over all bogies — basis of the curve resistance |
| `cw_a` | air resistance cw·A [m²]; replaces the quadratic Davis term |
| `max_payload` | maximum payload [kg] |
| `tilt_angle_deg` | maximum tilt angle [°], 0 without tilting technology |
| `hunting` | hunting −1 … 1, 0 = standard |
| `script` | optional behaviour hook `"<mod>:<name>"` |
| `model` | glTF file, levels of detail, moving parts — see below |

Start it with `cargo run -p app -- --loco example:br101_afb`, edit it with
`cargo run -p vehicle-editor -- mods/example/vehicles/br101_afb.ron`.

### Model (glTF)

Models are glTF/GLB and use the format's own features. `mods/` is registered as an asset
source, so the path is stated **relative to the mods directory** — `<mod>/assets/<file>`,
loaded as `mods://<mod>/assets/<file>`. Everything else in a mod's `assets/` directory
(textures, sounds) is reachable the same way.

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
