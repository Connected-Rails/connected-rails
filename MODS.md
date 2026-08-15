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
mods/<id>/mod.ron           manifest
         /vehicles/*.ron    VehicleSpec
         /lines/*.ron       LineSource — a line, or a module with `boundaries`
         /compositions/*.ron Composition — modules chained into one line
         /scenarios/*.ron   Scenario
         /timetable/*.ron   Timetable — referenced by a scenario for stop scoring
         /signals/*.ron     SignalType
         /signal_models/*.ron SignalModel — glTF parts on mount points, lamp bindings
         /scripts/*.lua     behaviour
         /assets/…          models, textures, sounds — as `mods://<id>/assets/…`
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
    description: "Reference mod: a vehicle with an AFB script, two signal types with a 3D signal model, and a line.",
    depends: [],
    enabled: true,
)
```

Only `id` and `name` are mandatory. `depends` lists mod ids that have to be loaded first;
`enabled: false` skips the mod without deleting it.

**The mod manager lives on the main menu** (start the simulator without arguments): every
installed mod with its version, whether it is switched on, which of its dependencies are
missing, and the loading warnings. ↑/↓ pick a mod, Enter switches it on or off — that writes
`enabled` back into its `mod.ron` (only that field; comments in the file survive) and takes
effect when the run starts. F9 opens the same list in the simulator; a toggle there only
takes effect after a restart, because reloading mid-run would mean rebuilding line, trains
and interlocking.

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
| `passenger_doors` | vehicle has passenger doors that follow the door control |
| `safety` | train protection fitted: `None` or `De(pzb: Some(Pzb90V20), lzb: true, sifa: Some(TimeTime), train_type: O)` |
| `doors` | door control the vehicle brings: `None` / `Tb0` / `Tav` / `UicWtb` |
| `hunting` | hunting −1 … 1, 0 = standard |
| `script` | optional behaviour hook `"<mod>:<name>"` |
| `model` | glTF file, levels of detail, moving parts — see below |

`safety` and `doors` are the **equipment** of the vehicle, so a train carries what its
vehicles carry — the leading vehicle determines the door control. Anything left out means
"not fitted": a coach without `safety` has no train protection, a loco without `doors` gives
the driver no door control. What the equipment achieves also depends on the line: the LZB
needs a conductor cable, the PZB needs track magnets.

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
| `brake_weight`, `max_force`, `max_cylinder`, `cylinder_to_reservoir` | braked weight [t], force at full pressure and standstill [N], cylinder pressure [bar], volume ratio (exhaustibility) — the two forces are those of the **loaded** vehicle, see `load_braking` |
| `load_braking` | load braking: `None`, `Weighing` (stepless weighing valve — throttles the cylinder pressure by `mass_empty`/`max_payload`, no figures of its own), or `Changeover(empty_share: 0.4, changeover_mass_t: 40.0)` for the empty/loaded lever with the anscribed braked weights |
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
        // or Fill — rack instead of engine speed. droop is the sag under load.
        governor: Speed(steps: 0, droop: 0.04),
        inertia: 60.0, response_time: 1.0,
    )),
    transmission: Some((
        circuits: [
            (kind: Converter, ratio: 3.93, stall_ratio: 2.4, coupling_nu: 0.85,
             absorption: 0.53, absorption_slope: 0.15,
             shift_up_kmh: 72.0, shift_primary_kmh: 25.0),
            (kind: Converter, ratio: 1.50, stall_ratio: 1.9, coupling_nu: 0.85,
             absorption: 0.53, absorption_slope: 0.15,
             shift_up_kmh: 0.0, shift_primary_kmh: 0.0),
        ],
        fill_steps: 0,        // 0 continuous, 1 fill/empty only, n partial filling stages
        fill_time: 1.2, drain_time: 0.7, hysteresis_kmh: 10.0,
        final_ratio: 1.0, wheel_diameter: 1.0, count: 1, efficiency: 0.95,
    )),
    hydrodynamic_brake: None,
)),
```

`absorption` λ is the pump wheel's torque coefficient at ν = 0: `M_pump = λ(ν)·ω²·fill`,
with `λ(ν) = absorption·(1 + absorption_slope·ν)`. Set λ so the circuit absorbs the engine's
rated torque at rated speed — `λ = M_rated / ω_rated²`; a slope of 0 nails a speed-governed
engine to one speed parabola for the whole converter range.

`shift_up_kmh` and `hysteresis_kmh` are the change points of the original, and
`shift_primary_kmh` is the primary influence: at the zero notch the change comes that many
km/h earlier than at full power. The last circuit ignores its change-up point.
`fill_time` and `drain_time` are separate because they are separate in the original — the
outgoing circuit lets go before the incoming one has taken hold, and that overlap is the
hole in the tractive effort at the change point. `drain_time: 0.0` takes the filling time.

Everything after `absorption` in a circuit, plus `drain_time` and `droop`, may be left out;
they default to 0, which is the behaviour of a transmission without any of them.

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
value today: `pantograph`, `door_left`, `door_right`, `gauge:speed`, `gauge:brake_pipe`,
`gauge:cylinder`, `gauge:main_reservoir`, `gauge:tractive_effort`, `switch:throttle`,
`switch:reverser`, `switch:direct_brake`, `lamp:main_switch`, `lamp:sanding`, and
`lamp:<indicator>` for every indicator of the fitted train protection — the names the HUD
prints: `lamp:pzb_1000hz`, `lamp:pzb_500hz`, `lamp:pzb_befehl`, `lamp:sifa`, `lamp:lzb_ue`,
`lamp:lzb_g`, `lamp:lzb_ende`, `lamp:lzb_b`, `lamp:lzb_v40`, `lamp:lzb_stoerung`, …
(a blinking indicator blinks at 1 Hz). Likewise `gauge:<indicator>` for the numeric ones —
`gauge:mfa_v_soll` and `gauge:mfa_v_ziel` are normalised by the vehicle's `v_max` (a second
pointer on the speedometer dial is exactly how the MFA shows the target), and
`gauge:mfa_zielentfernung` by 4000 m for an LED-bar style distance display. `wiper` sweeps
a windscreen wiper according to the wiper switch. `digit:<indicator>:<place>` turns a node
into one digit of a numeric display: its children are named `0` … `9` and the matching one
is shown (`place` 0 = ones; leading zeros stay blank, an absent indicator goes dark).
Everything else stays in its rest position until `sim-core` models the state (destination
displays, marker lights).

```bash
cargo run -p app -- --loco example:br101_afb --camera outside
```

### Interactive 3D cab

The `cab:` block inside `model:` makes the cab operable with the mouse (plan ch. 12).
It has two parts: the driver's **eye point** in model space (X right, Y above the rail
head, −Z ahead — where the cab camera sits) and the **controls**, each binding a glTF
node to a simulation input:

```ron
cab: Some((
    eye: (-0.55, 2.55, -6.5),
    controls: [
        (node: "cab_throttle", input: Throttle,
         motion: Rotate(axis: (1.0, 0.0, 0.0), degrees: 60.0)),
        (node: "cab_sifa", input: Sifa,
         motion: Translate(axis: (0.0, -1.0, 0.0), metres: 0.015)),
    ],
))
```

`input` is one of a closed list (the vehicle editor offers them in a dropdown):
`Throttle`, `Reverser`, `BrakeValve`, `DirectBrake`, `AfbTarget`, `Sifa`,
`PzbAcknowledge`, `PzbExempt`, `PzbOverride`, `LzbTakeover`, `LzbEnd`, `LzbTest`, `Horn`,
`Sanding`, `BrakeRelease`, `EngineStart`, `DoorReleaseLeft`, `DoorReleaseRight`,
`DoorClose`, `ParkingBrake`, `EpBrake`, `Afb`, `Battery`, `Pantograph`, `MainSwitch`,
`Compressor`, `TrainType`, `Wipers` (off – interval – slow – fast), and `Display(0)` …
`Display(7)` — softkeys for the [cab displays](#displays), read by the `display(ctx)`
script hook.

How a control answers the mouse follows from the input, nothing is configured:

- **Push buttons** (Sifa, PZB/LZB buttons, horn, sanding, …) are held while the mouse
  button is down and spring back on release.
- **Switches** (battery, pantograph, main switch, reverser, train type switch, …) cycle
  to their next position on click and step without wrap on the scroll wheel.
- **Levers and valves** (power controller, brake valves, AFB target) follow a drag along
  their on-screen direction of travel and step finely on the scroll wheel. The driver's
  brake valve runs fill – release – lap – service range – emergency over its travel.

`motion` is the same type the moving parts use and describes the node's travel between
input 0 and 1. The whole subtree of the node takes the mouse; the hovered control glows
and the HUD names it with its position. Everything also stays operable from the keyboard —
the two write the same inputs. The camera looks out of `eye` (F1), pans with the arrow
keys and looks around while the right mouse button is held.

### Displays

A screen in the cab is a texture the app renders onto a glTF node — the same idea as
TSW's render targets or MSFS's instrument webviews, only that the content is described
in the vehicle file or drawn by the vehicle's Lua script:

```ron
displays: [
    (name: "brake", node: "screen_brake", width: 192, height: 128, widgets: [
        Label(x: 8.0, y: 6.0, size: 14.0, text: "HL"),
        Value(x: 60.0, y: 6.0, size: 14.0, source: Quantity(BrakePipe),
              decimals: 2, unit: "bar"),
        Bar(x: 8.0, y: 30.0, w: 140.0, h: 10.0, source: Quantity(BrakePipe), max: 6.0),
    ]),
    (name: "mfa", node: "screen_mfa", width: 256, height: 160),
]
```

Content, in order of what is easiest:

1. **Widgets** — no code. `Label` (fixed text), `Value` (a number with `decimals`,
   `scale` and `unit`) and `Bar` (fills towards `max`). `source` is either
   `Quantity(<sound quantity>)` — everything the sound table can hear, including
   `Control(…)` positions — or `Indicator("<name>")`, a numeric indicator of the train
   protection (`mfa_v_soll`, `mfa_v_ziel`, `mfa_zielentfernung`, …). Coordinates are
   pixels from the top left; colors linear RGBA 0…1, default white.
2. **The `display(ctx)` script hook** — for real logic: pages, nested menus, EBuLa-like
   screens. The vehicle's behaviour script (the one that already has `update(ctx)`) adds
   a `display` function; it is called per screen with `ctx.display` set to the name and
   answers with a draw list. Returning `nil` hands the screen back to its widget list —
   so a script can drive one screen and leave the others declarative.
3. **An HTML page** — for screens with real layout. `html: Some("<mod>/displays/<file>.html")`
   on the display draws it from a single HTML file with `<style>` and `<script>`, the way
   MSFS instruments are written — only rendered in-engine (own DOM, flexbox layout,
   embedded ECMAScript), not by a browser: no extra process, layout and repaint run only
   when the DOM actually changed, unchanged screens cost nothing. When `html` is set it
   drives the screen alone; widgets and the Lua hook are ignored for it.

```lua
display = function(ctx)
    if ctx.display ~= "mfa" then return nil end
    return {
        { kind = "clear", color = {0.02, 0.03, 0.02} },
        { kind = "text", x = 128, y = 10, size = 28, align = "center",
          text = string.format("%.0f km/h", ctx.v_kmh) },
        { kind = "rect", x = 8, y = 50, w = 240, h = 2 },
        { kind = "line", x1 = 8, y1 = 120, x2 = 248, y2 = 120, width = 1 },
    }
end
```

Draw kinds: `clear` (background), `rect` (`filled` defaults to true), `line`, `text`
(`align` left/center/right). At most 512 commands per frame, text capped at 128
characters; a malformed entry is skipped and reported once in the mod log. `ctx`
carries the display name and size, the driving values `update(ctx)` gets (v_kmh,
brake_pipe, line_voltage, …), `value.<indicator>` / `lamp.<indicator>` for everything
the train protection shows, and `buttons[1]`…`buttons[8]` — the held state of the
`Display(0)`…`Display(7)` cab controls. Bind those to nodes next to the screen and a
click in the 3D cab walks the script's menu; edge detection is the script's job (keep
the previous state in a local).

The screen glows (emissive), so it stays readable in a dark cab. A display whose name
no script answers and whose widget list is empty stays dark.

#### HTML displays

The page is ordinary HTML with one `<style>` and one `<script>` block; the supported
subset is deliberately small and fully listed here. **HTML**: any tags (only their style
matters), `id`, `class`, `style`, text. **Live values without script**:
`data-bind="v_kmh"` replaces the element's text every tick, `data-format` shapes it
(printf subset `%d`, `%s`, `%.Nf`); `data-show="lamp.lzb_ue"` hides the element while
the value is 0/false. Field names: the driving values of the Lua ctx (`v_kmh`,
`brake_pipe`, `line_voltage`, …), `value.<indicator>` and `lamp.<indicator>`, `time`.

**CSS**: selectors `tag`, `.class`, `#id`, compounds and comma lists; properties
`display` (flex/block/none), `flex-direction`, `justify-content`, `align-items`,
`flex-grow`, `gap`, `width`/`height` (px/%), `padding`/`margin`, `position: absolute`
with `left/top/right/bottom`, `background-color`, `color`, `font-size`, `text-align`,
`border: <N>px solid <color>`, `visibility: hidden`, `opacity`. Colors as `#hex`,
`rgb()`/`rgba()` or the usual named handful. Text is measured in the app's monospaced
display font, so layout is deterministic.

**Script API** (the whole surface): `document.getElementById`, on elements
`textContent`, `getAttribute`/`setAttribute`, `classList.add/remove/toggle/contains`,
`style.setProperty`, `hidden`; the global `sim` with every field as a property plus
`sim.button(1..8)`; `onFrame(fn)` per tick and `onButton(fn(index, pressed))` on every
softkey edge — the same `Display(0…7)` cab controls the Lua hook reads, so clickable
3D softkeys drive the page. A handler that throws is disabled and reported once in the
mod log; the screen keeps its last good picture. Pages tick at ~30 Hz (button edges
immediately); `mods/example/displays/ebula.html` is a complete page to start from.

Which path for which screen: **widgets** for values, bars and labels (no code at all);
**Lua** when the logic already lives in the vehicle script and the drawing is simple;
**HTML** when the screen needs layout — headers, tables, pages, wrapped text. All three
end in the same renderer and the same texture; they differ only in how the picture is
described.

Modelling rules that matter for the simulation:

- **Length over buffers** is the RON value, not the model — but the buffers should be drawn
  1–2 cm compressed so that vehicles do not intersect in curves.
- **Origin** at the vehicle centre, on the top of the rails; the vehicle runs along −Z/+Z.
- **LOD0** is the close-up model; higher levels get coarser. What is not visible from 150 m
  away does not belong in LOD1.

### Sounds

The sound table sits in the vehicle file and is maintained in the vehicle editor. An entry
has three parts:

- **Trigger** — what starts the sound. `Loop` means *no* trigger: the sound runs
  continuously and only its volume and pitch are modulated. That is the normal case for
  driving noise.
- **Conditions** — state predicates. All of them have to hold, otherwise the sound stays
  silent. Brake squeal is a condition, not an event.
- **Dependencies** — curves with support points that map a quantity onto volume and
  playback speed.

```ron
sounds: [
    // A loop, modulated: no trigger, two curves.
    (
        name: "rolling",
        file: "synth:rolling",
        trigger: Loop,
        volume: Some((quantity: Speed, points: [(0.0, 0.0), (60.0, 0.55)])),
        pitch: Some((quantity: Speed, points: [(0.0, 0.7), (200.0, 1.7)])),
        positional: true,
    ),
    // A loop held by conditions: heard only while slow *and* braking.
    (
        name: "brake-squeal",
        file: "synth:squeal",
        trigger: Loop,
        conditions: [
            (quantity: Speed, min: 3.0, max: 25.0),
            (quantity: BrakeEffort, min: 10.0, max: 10000.0),
        ],
        volume: Some((quantity: Speed, points: [(3.0, 0.0), (8.0, 0.3), (25.0, 0.0)])),
        positional: true,
    ),
    // An event: the same table, only with a trigger instead of a loop.
    (
        name: "rail-joint",
        file: "synth:joint",
        trigger: Every(quantity: Distance, interval: 30.0),
        conditions: [(quantity: Speed, min: 3.0, max: 10000.0)],
        volume: Some((quantity: Speed, points: [(3.0, 0.12), (120.0, 0.35)])),
        positional: true,
    ),
]
```

`file` is a sample below the mods directory (`example/assets/whine.ogg`, the same path
scheme as a model) or one of the sources the app generates at start-up: `synth:rolling`,
`synth:traction`, `synth:air`, `synth:compressor`, `synth:horn`, `synth:buzzer`,
`synth:squeal`, `synth:joint`, `synth:contactor`.

`positional: true` places the sound on the vehicle: it is attenuated by distance,
Doppler-shifted — which is what makes another train audible as it passes — and muffled
by a lowpass while the camera sits in the cab, the cab wall. Sounds of the
driver's desk — buzzer, Sifa — set it to `false` so they stay at a constant place when the
camera goes outside, and they are never filtered: they are in the cab with the listener.

Triggers:

| Trigger | Fires |
|---|---|
| `Loop` | never — the sound runs and is modulated |
| `Rises(quantity: X, threshold: t)` | when `X` crosses `t` upwards |
| `Falls(quantity: X, threshold: t)` | when `X` crosses `t` downwards |
| `Every(quantity: X, interval: i)` | at every multiple of `i` — 30 m of `Distance` is a rail joint, 1 of `TapChangerStep` a contactor |

Quantities: `Speed` [km/h], `Distance` [m], `EngineRpm` [1/min], `TapChangerStep`,
`Circuit`, `TractiveEffort` [kN], `BrakeEffort` [kN], `BrakePipe` [bar],
`BrakeCylinder` [bar], `AirFlow` [bar/s], `Slip` [m/s], `Throttle`, `Pantograph`,
`MainSwitch`, `Compressor`, `Doors`, `Horn`, `Alert`, and `Control(<input>)` — the
position of a cab control, normalised 0 … 1 over its travel, for every input the
[3D cab](#interactive-3d-cab) can bind (`Control(Sifa)`, `Control(BrakeValve)`,
`Control(Battery)`, …). Curves interpolate linearly between their support points and hold
the last value beyond the ends.

`Control` is how a lever or switch gets its operating sound, and it fires no matter
whether mouse, keyboard, AFB or a script moved it: `Rises`/`Falls` around a threshold for
one edge, `Every` with the detent spacing for every notch passed — `Every(quantity:
Control(Battery), interval: 1.0)` clicks a two-position switch on both edges with one
entry, `Every(quantity: Control(Reverser), interval: 0.5)` on each of the three reverser
positions. The example vehicle carries a block of these. (The driver's brake valve usually
needs no click — its `AirFlow` hiss follows from the pressures by itself.)

A table stated here **replaces** the generated default completely, so it has to carry
everything the vehicle is to make. A vehicle without a table runs on the generated loops if
it is powered, and stays silent if it is hauled — a coach that is to roll audibly writes its
own entry. `mods/example/vehicles/br101_afb.ron` is a complete table to start from.

One sound that follows different quantities in different states is two entries with one
condition each — that is how the traction loop is split into an electric case (pitch over
speed, `EngineRpm` in 0…0) and a diesel case (pitch over `EngineRpm`, from 1 upwards).

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
Signals are evaluated in signalling order — `next_stop`/`next_slow` see the following signal's
final aspect, rule table included, from the same update.

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

### Signal models

A signal type maps situations to lamp images; a **signal model** gives them geometry.
Models are assemblies after the Zusi pattern: shared glTF parts — mast, screen,
indicator — chained by **mount points**, so a screen drawn once hangs on a mast here
and under a bridge there. `signal_models/*.ron`:

```ron
(
    parts: [
        (file: "example/assets/sig_mast.gltf"),                              // root
        (file: "example/assets/sig_schirm_ks.gltf", mount: Some((0, "mp_schirm"))),
        (file: "example/assets/sig_zs3.gltf", mount: Some((1, "mp_top"))),
    ],
    lamps: [
        (lamp: "red", part: 1, node: "lamp_red"),
        (lamp: "zs3_4", part: 2, node: "zs3_4"),
    ],
)
```

A part without `mount` stands at the device position; every other part hangs off a
named node of another part. `lamps` binds the free-form lamp-image strings of the
signal type to glTF nodes: the node is **visible while its string is in the current
lamp image** and hidden otherwise — a Zs3 digit is a lamp like any other, and a
script's `lamps` (the Zs1 below) lights them the same way.

Which model a signal wears: `model: Some("<mod>:<name>")` on the signal type is the
default; `model` on the signal placement in the line overrides it per signal. A
signal without either gets a placeholder mast whose light follows the aspect.

Model conventions: origin at the foot on rail-top height, +Y up, the face towards
the approaching driver looks along **+Z**. Mount points are empty nodes named
`mp_*`; lamp nodes conventionally `lamp_<image>` (their material carries the lit
look, e.g. an emissive factor — switching is pure visibility). The
**signal editor** (`trainsim-signal-editor`) assembles parts, binds lamps with
suggestions from these names, and lights any lamp image in its preview.

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

A `LineConductor` device marks a line conductor section — the cable, not what it transmits:

```ron
(kind: LineConductor, edge: 1, s: 0.0, payload: "(length:4000.0,cir_elke:false,end:false)"),
```

`length` is how far the section transmits, `cir_elke: true` marks a CIR-ELKE section (steeper
braking curve, 5 km/h speed steps, speed rises effective at the head of the train instead of
at its rear), `end: true` marks the last section of an LZB area, where the end procedure runs.
The last two may be left out.

The movement authority is not written into the line: the LZB centre builds it every step from
the block division and the state of the interlocking. The block division is a line datum of
its own — `BlockMarker` devices naming the section behind them:

```ron
(kind: BlockMarker, edge: 2, s: 0.0, payload: "(section:2)"),
```

An authority runs to the first boundary that is not clear: a block marker whose section is
occupied, or a main signal at stop. v-target is the most restrictive point ahead — a speed
step of the line counts as much as a stop, so a slow section needs no device of its own.
Whether the block mode ends up as full or partial block mode falls out of the line data too —
a line with block markers of its own divides the authority itself, a line without them leaves
the main signals as the only boundaries, and the signals stay binding together with their PZB
magnets.

### Modules and compositions

A big line is built from **modules**, after the Zusi 3 model: a module is an ordinary
`LineSource` whose open ends carry named `boundaries` — `Buffer` nodes at which another
module may attach:

```ron
boundaries: [(name: "nach_ost", node: 1)],
```

A **composition** (`compositions/*.ron`) chains modules into one line:

```ron
(
    name: "Gesamtstrecke",
    modules: ["example:modul_west", "example:modul_ost"],
)
```

**The connection comes from the georeference.** Every module is placed in real
coordinates; two boundaries that lie on the same spot (within a metre) snap together by
themselves. Building a module transition therefore means nothing more than starting your
module's edge at the agreed coordinates of the neighbour's boundary — no wiring, no
extra file. `connections: [(("<mod>:<module>", "<boundary>"), (…, …))]` states a pairing
explicitly for the rare case where positions alone cannot decide.

**UTM zones cannot shift a transition.** Module anchors are geodetic (`lat`/`lon`) and
compile into ECEF, one global frame for the whole world — there are no per-zone planar
coordinates anywhere in the line data. Modules whose source data came through different
UTM zones (say, a composition spanning the 12° E boundary between zones 32 and 33)
therefore meet to the millimetre; the metre-sized zone-seam displacement known from
Zusi's module system has no place to come from. A test pins this
(`compose::modules_from_different_utm_zones_meet_exactly`). UTM appears only where
elevation data is read (DGM tiles are delivered in a zone grid) — there the app takes
one source per zone: repeat `--dgm <dir> --epsg <code>` for each side of the boundary.

The composition is addressed like a line: `--line example:gesamtstrecke`. On loading,
every index space of a module (nodes, edges, devices, sections, signals, routes —
including the indices inside magnet, signal and block marker payloads) is shifted by the
module's offset.

**Signalling across the boundary** is composition data, because a module's `next` is a
module-local index:

```ron
signal_links: [(("example:modul_west", 0), ("example:modul_ost", 0))],
```

sets the first signal's `next` to the second — the last signal of one module then
announces the first signal of the next, in the same update like any other chain.

**Timetables and scenarios address positions module-locally.** A `module` field on the
timetable (or per stop), and on the scenario (or per event), makes every index in it
mean "of that module"; the runtime resolves them against the composition:

```ron
// timetable/*.ron — edge 0 *of Modul Ost*, wherever the composition puts it.
(
    number: "RB 42",
    module: Some("example:modul_ost"),
    stops: [(name: "Ostheim", edge: EdgeId(0), s: 1700.0, arrival: 240.0, departure: 300.0)],
)
```

A stop or event with its own `module:` overrides the file default — that is how one
timetable strings stops across many modules. Without any `module`, indices are those of
the (composed) line itself, as before.

**Several versions of one module** — other epochs, other equipment, a rebuilt station —
are simply several module files (`bahnhof_1975.ron`, `bahnhof_2020.ron`); a composition
picks one by name, so the same neighbouring modules combine with either. A scenario can
name its line or composition itself (`line: Some("example:gesamtstrecke")`), which is how
a timetable chains modules; `--line` on the command line wins.

`mods/example/lines/modul_west.ron`, `modul_ost.ron` and
`compositions/gesamtstrecke.ron` are a working pair to copy from;
`scenarios/modulfahrt.ron` plus `timetable/modulfahrt.ron` run across the seam with
module-local references (`cargo run -p app -- --scenario example:modulfahrt`).

`scenarios/*.ron` is a `Scenario` — triggers and actions, see the README section on scenarios.
Start one with `--scenario example:probefahrt`.

A scenario gets stop scoring by referencing a timetable: `timetable: Some("<mod>:<name>")`
points at a `timetable/*.ron` of the mod (train number, category, stops with position,
arrival and departure — `mods/example/timetable/probefahrt.ron` is a complete one). With it
the scoring counts position error and delay per stop on top of the scenario's own points;
without one, only the scenario points count. `kind` decides what the times mean:
`Scenario` (the default) counts seconds from the start of the run and the timetable runs
once; `Daily` reads them as seconds since midnight and wraps around every 24 h — the
looping all-day timetable, which an AI train follows indefinitely.

Where midnight lies — and where the sun stands — comes from the scenario's start clock:

```ron
start: (year: 2026, month: 8, day: 15, hour: 6, minute: 45, utc_offset: 2.0),
```

Date and local time at the start of the run; `utc_offset` is the local clock's offset
from UT in hours (Germany: 1 in winter, 2 in summer). It drives the sun and moon over
the georeferenced line and anchors `Daily` timetables; `Scenario` timetables and event
triggers stay relative to the start of the run. Without the field, a run begins at
midsummer noon.

### Line and scenario hooks `on_load(ctx)` / `on_frame(ctx)`

Both a `LineSource` and a `Scenario` may name a `script`. `on_load` runs once when the run
starts, `on_frame` once per frame. The rule stays the same as everywhere else: **the script
decides *when*, the RON says *what*.** An event with `trigger: Never` waits for the script:

```ron
(name: "stalled", trigger: Never, actions: [Announcement("Report your position."), Score(points: -5, reason: "Unscheduled stop")]),
```

```lua
function M.on_frame(ctx)
  if ctx.v_kmh < 1.0 and ctx.time - standing_since >= 60.0 and ctx.fired["stalled"] == nil then
    return { fire = { "stalled" } }         -- the event's actions run
  end
end
```

`ctx`: `dt`, `time`, `trains`, `player`, `v_kmh`, `edge`, `s`, `finished`, `bonus`, and
`fired` — a table of event name → firing time, so a hook does not have to remember which
events have gone off.

The answer may contain:

| field | effect |
|---|---|
| `fire = { "name", … }` | fires those scenario events; unknown names are logged once |
| `message = "…"`, `announcement = true` | a message to the player |
| `switch = { node = 3, position = "straight" \| "diverging" }` | throws a switch |

`fire` is the interesting one — through it a script reaches every action the scenario format
has (switch, route, weather, score, finish) without any of them being written in Lua. The
other two are there for a line that carries behaviour without a scenario.
`mods/example/scenarios/probefahrt.ron` plus `scripts/probefahrt.lua` is the worked example.

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
cargo run -p app -- --line example:beispielstrecke --scenario example:probefahrt
cargo run -p vehicle-editor -- mods/example/vehicles/br101_afb.ron
cargo run -p route-editor -- mods/example/lines/beispielstrecke.ron
```

Loading warnings and script errors go to the log with a `mod:` prefix.

## Distribution

Planned: `.trainsim` = a zip of the mod directory, unpacked to `<game>/mods/` by a mod manager.
For now, copy the directory into `mods/` by hand.

## Licensing your mod

Your mod is yours. RON data, assets and Lua scripts that use the documented interfaces are not
derivative works of the game — see the mod exception at the top of [LICENSE](LICENSE). Ship them
closed-source, sell them, pick any licence you like; no obligation to publish sources.

The EUPL only applies once you distribute the game itself, a modified copy of it, or native code
linked against its crates.
