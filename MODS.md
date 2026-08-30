# Modding guide

Connected Rails is built for mods: your own locomotives, your own signals, your own lines.
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
         /days/*.ron        OperatingDay — a whole day of services, looping every 24 h
         /signals/*.ron     SignalType
         /signal_models/*.ron SignalModel — glTF parts on mount points, lamp bindings
         /blocks/*.ron      block presets for the vehicle editor's palette
         /characters/*.ron  CharacterSpec — a person model: the walker's body, the passengers
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
    author: "Connected Rails",
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

### Tags

Scenery objects, signal types, signal models and track types each take a `tags` list — free
words the mod author picks, which the route editor's **content drawer** filters on:

```ron
tags: ["mast", "catenary", "epoch-4"],
```

The field is optional and defaults to empty; a mod without tags loses nothing but the filter.
Tags are **lower case, words joined by hyphens** — the signal editor normalises what is typed
(`Epoch 4` becomes `epoch-4`), and the drawer normalises again when it reads a hand-written
file, so `Mast` and `mast` are one tag and not two. There is no fixed vocabulary and no
registry: what a line builder searches for is what the mods around them happen to agree on.
Useful conventions are the epoch (`epoch-3`), the region or company, the kind of thing
(`main-signal`, `catenary`, `platform`) and the state of the model (`lod0`, `wip`).

A tag names the *entry*, not the placement: a mast tagged `epoch-4` stays that mast wherever a
line puts it. Nothing in the simulator reads tags — they exist for finding things in the
editor, and adding one can never change how a run behaves.

## Vehicles

`vehicles/*.ron` is a `VehicleSpec` — plain data. See `mods/example/vehicles/br101_afb.ron`:

| Field | Meaning |
|---|---|
| `name` | display name |
| `length` | length over buffers [m] |
| `mass_empty` | tare mass [kg] |
| `rotating_mass_factor` | allowance for rotating masses (0.05 coach … 0.25 powered vehicle) |
| `davis` | running resistance `R = a + b·v + c·v²` [N], `v` in m/s |
| `brake` | control valve, friction pairing, default brake position, braked weight [t], forces, pressures, reservoir volumes, additional brakes |
| `traction` | `Curve` / `TapChanger` / `Converter` / `Diesel` — or `None`; see below |
| `coupler` | `kind` plus slack, stiffnesses, damping, breaking force — see below |
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
| `safety` | train protection fitted: `None` or `De(pzb: Some(Pzb90V20), lzb: true, sifa: Some(TimeTime), train_type: O, gnt: false)` — `gnt: true` is the tilting speed supervision, which only does anything on a vehicle whose `tilt_angle_deg` is above zero |
| `afb` | AFB fitted: the built-in target speed controller drives the power controller toward the cab's AFB dial and brakes like the prototype — dynamic brake first, the air brake blends in where that does not suffice; under LZB guidance the LZB's v-soll caps the dial |
| `doors` | door control the vehicle brings: `None` / `Tb0` / `Tav` / `UicWtb` |
| `hunting` | hunting −1 … 1, 0 = standard |
| `script` | optional behaviour hook `"<mod>:<name>"` |
| `graph` | optional block diagram of drive, brake and equipment — when present it is authoritative over the fields it bakes, see below |
| `model` | glTF file, levels of detail, moving parts — see below |

`coupler.kind` is what a shunter can do with the vehicle, as against how the coupler
behaves once it is made. `Screw` is the European standard (screw coupling and side
buffers), `CenterBuffer` the automatic head of a multiple unit, and `Bar` a bar inside a
fixed unit. **Only like couples to like**, and a bar is undone in the works: a railcar
cannot be put in front of a rake of freight wagons, and a unit cannot be split between two
coaches that are barred together. Left out, a vehicle carries a screw coupling.

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
| `position` | `G` / `P` / `R`; an `R` on a valve without an R position falls back to `P` |
| `brake_weight`, `max_force`, `max_cylinder`, `cylinder_to_reservoir` | braked weight [t], force at full pressure and standstill [N], cylinder pressure [bar], volume ratio (exhaustibility) — the two forces are those of the **loaded** vehicle, see `load_braking` |
| `load_braking` | load braking: `None`, `Weighing` (stepless weighing valve — throttles the cylinder pressure by `mass_empty`/`max_payload`, no figures of its own), or `Changeover(empty_share: 0.4, changeover_mass_t: 40.0)` for the empty/loaded lever with the anscribed braked weights |
| `brake_weight_g`, `brake_weight_p`, `brake_weight_r` | the anscribed braked weight [t] of that position, where the vehicle carries one per position (`G 40 / P 60 / R 71`). Absent = `brake_weight` for all three. The **R** figure replaces the standard R force bonus; the **G** figure is a brake sheet datum only — G brakes with P's force and differs in the transition time, which is simulated separately |
| `apply_time_g`, `apply_time_p`, `release_time_g`, `release_time_p` | filling and release time of the brake cylinder [s], overriding the UIC figures (G 22/50 s, P and R 4 s). `_p` covers R as well: the changeover handle sets the timing between G and P, R differs in force |
| `has_mg`, `mg_force` | magnetic track brake and its force [N]; it applies in `position: R` only — the anscribed "R + Mg" is that pair |
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
    dynamic_brake: None,     // electric brake of a diesel-electric, see below
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

A **diesel-electric** locomotive (engine → generator → traction motors, Class 66/BR 232
style) has no hydraulic transmission; its rheostatic brake is `dynamic_brake` — the same
record the tap changer drive uses, optional and absent by default, so existing files are
untouched. A `regenerative: true` inside it is ignored: a diesel has no line to feed
back into.

### Block diagram

Drive, brake and equipment can also be declared as a **block diagram**: the vehicle is
a circuit of components, and the physics follows from what is wired to what. The diagram is edited on the vehicle editor's node canvas (chips at the
top left of the centre view switch between *3D model* and *Block diagram*; `--graph` on
the command line starts on the diagram) and stored in the vehicle file as the optional
`graph` field:

```ron
graph: Some((
    blocks: [
        (id: 0, kind: "diesel-engine", pos: (-320.0, 0.0),
         params: { "max_power": Number(1840000.0), "governor": Choice("speed") }),
        (id: 1, kind: "hydro-transmission", pos: (-80.0, 0.0)),
        (id: 2, kind: "wheelset", pos: (160.0, 0.0),
         params: { "axles": Number(4.0), "adhesive_mass_fraction": Number(1.0) }),
    ],
    wires: [
        (from: 0, from_port: "out", to: 1, to_port: "shaft"),
        (from: 1, from_port: "out", to: 2, to_port: "shaft"),
    ],
))
```

A block is `(id, kind, pos, params)`: `id` is a stable number the wires reference,
`kind` a palette id (built-in, or `<mod>:<preset>` — see the presets below), `pos` the
canvas position, `params` a map parameter id → value. Values are typed —
`Number(…)`, `Bool(…)`, `Choice("<kebab-case id>")`, `Text("…")`, `Curve([(x, y), …])`,
`List([…])` or `Circuits([…])`, the last one the circuits of a hydraulic transmission
in the same shape as `transmission.circuits` above. A missing parameter falls back to
the block's default.

**Ports have domains, and only like connects to like** — the editor colour- *and*
shape-codes the pins: mechanical (rotating shaft), force (at the wheel), electrical,
pneumatic, signal (control values 0 … 1), fuel, steam, water and heat. A drag between two
pins wires them; a right click on the canvas adds a block, a right click on a node removes
it.

**When a vehicle file carries a `graph`, the graph is authoritative.** Loading bakes it
and overwrites `traction`, `brake`, `safety`, `doors`, `passenger_doors`, `afb`,
`slip_protection`, `axles`, `adhesive_mass_fraction`, `script`, `signal`, `supply`,
`sand_rate`, `running_gear` and — where the diagram draws bogies — `axle_base_sum`; every other field —
mass, length, running resistance, coupler, sounds, model, … — stays hand-edited as
before. A vehicle **without** a graph keeps working unchanged; the editor synthesises a
diagram from the spec on open (`blocks::from_spec`) and writes it on save, so opening
and saving an old file upgrades it. While you wire, the editor lists the **bake
findings** (errors and warnings) below the palette — a click selects the offending
block.

The baker recognises the drive chains of all five traction models:

| Chain | Bakes to |
|---|---|
| `traction-curve` | `Curve` — tractive effort straight off the diagram |
| `pantograph` + `main-switch` + `transformer` + `tap-changer` (+ `series-motor`) (+ `dynamic-brake`) | `TapChanger` |
| … + `rheostat` / `chopper` + `series-parallel-switch` | `TapChanger` with `starter` — contactor drive |
| … + `traction-converter` + `traction-motor` or `async-motor` (+ `dynamic-brake`) | `Converter` |
| `diesel-engine` + `hydro-transmission` (+ `retarder`) | `Diesel`, diesel-hydraulic |
| `diesel-engine` + `mechanical-gearbox` | `Diesel` with `gearbox` — diesel-mechanical |
| `diesel-engine` + `hydrostatic-drive` | `Diesel` with `hydrostatic` |
| `diesel-engine` + `generator` + `load-regulator` (+ `rectifier`) + `series-motor` or `async-motor` | `Diesel` with `electric` — diesel-electric |
| `firebox` + `boiler` + `steam-cylinders` (+ `injector` + `tender`) | `Steam` |

Which motor block sits behind the generator decides the type of the diesel-electric
drive: `series-motor` is the classic DC machine, `async-motor` the modern inverter one.
A `cooling` block wired to the **heat** pin of a motor, a rheostat or the dynamic brake
gives that component a thermal model — without one it never gets hot.

One engine drives one path: a hydraulic transmission, a mechanical gearbox, a hydrostatic
drive or a generator. Wiring two of them warns and the transmission wins. A shunter's
two-range gearbox is not a block of its own — it is the `shunting_ratio` of the
transmission, and the cab's range selector (`CabControl::RoadGear`) changes it at a stand.

The brake blocks map onto the `BrakeSpec` fields above: the `control-valve` carries
valve type, default brake position, braked weight and load braking (the position is
only the setting the changeover handle starts in — what actually brakes is
`BrakeState::position`, set on the vehicle when the train is made up); the `brake-rigging` the
friction pairing and its maximum force; a `relay-valve` is the pre-controlled cylinder
(`pilot_controlled`; its `supplement` flag is the air supplement brake); the
`driver-brake-valve` carries the equalising device; reservoirs, pipe and compressor
their volumes and figures; `direct-brake`, `parking-brake`, `mg-brake` and
`wheel-slide-protection` the additional brakes and the slip protection. The
`brake-pipe` carries the **medium**: `air` or `vacuum` — a vacuum brake has no
auxiliary reservoir and no relay valve, and its compressor is an exhauster.

#### Logic blocks

The **Logic** group is the control wiring between the physical blocks: it computes, it
does not move anything. `bake` compiles that part of the diagram into a
[`SignalProgram`](crates/sim-core/src/signal.rs) — a flat list of operations in
evaluation order — which runs once per simulation step *before* the drive, so what it
commands takes hold in the same step the driver's lever moved.

A `value-in` reads something out of the vehicle, the blocks in between compute, and a
`signal-out` decides what the result takes hold of: power controller, brake demand,
sanding, blower, or one of four free values the cab displays can read. A cruise control
is three blocks — speed, set speed, `pid` — and a load-shedding relay is a
`rate-of-change` and a `value-switch`. Blocks that feed each other in a circle are
reported (`bake-signal-cycle`) and dropped rather than run.

The built-in palette:

| Kind | What it is | Key parameters |
|---|---|---|
| **Energy** | | |
| `battery` | on-board battery — the cab needs it | voltage, capacity [Ah] |
| `fuel-tank` | fuel supply of a diesel | capacity [l] |
| `pantograph` | collects power from the contact line — one block per supply system | supply system (AC 15/25 kV, DC 3/1.5 kV, third rail), rise time |
| `voltage-source` | stands in for the contact line where there is none | voltage |
| `diesel-engine` | diesel engine, optionally with the full engine map | force/power/v max hyperbola, ramp and cranking time; engine map: idle/rated/overspeed rpm, torque curve, speed or fill governor with notches and droop, inertia, rack time |
| **Drivetrain** | | |
| `hydro-transmission` | converters and couplings engaged by filling | circuits, power control (filling / engine speed), fill steps, fill/drain time, hysteresis, final drive, shunting gear, wheel diameter, count, efficiency |
| `mechanical-gearbox` | friction clutch and gears, no torque conversion (Köf, railbus) | gear ratios, final drive, wheel diameter, efficiency, clutch torque and travel time, change time, change-up and change-down speed |
| `hydrostatic-drive` | variable-displacement pump and hydraulic motor, stepless | effort limit of the relief valve, efficiency, swash plate travel time |
| `retarder` | hydrodynamic brake in the transmission | absorption, ratio, wheel diameter, force, power, fill time, fade-out |
| `generator` | main generator of a diesel-electric | electrical power, efficiency, maximum voltage and current |
| `traction-motor` | motor without data behind converter or generator | — |
| `series-motor` | series-wound motor behind the tap changer or the generator | count, resistance, machine constant, saturation and maximum current, voltage, field weakening steps, gear ratio, wheel diameter, efficiency |
| `async-motor` | three-phase induction motor (Kloss) | count, pole pairs, rated and pull-out torque, pull-out slip, rated and maximum frequency, gear ratio, wheel diameter, efficiency |
| `traction-curve` | the simplified model: effort straight off the diagram | force and brake curves [km/h → N], v max, ramp time |
| **Electric** | | |
| `main-switch` | main circuit breaker | — |
| `transformer` | main transformer | — |
| `rectifier` | generator alternating current into direct current | efficiency |
| `load-regulator` | holds the generator on the notch's power | travel time, blower share at idle |
| `tap-changer` | tap changer feeding series-wound motors | steps, step time, starting effort, power, v max |
| `rheostat` | starting resistors, cut out step by step | resistance steps [Ω], time per step |
| `series-parallel-switch` | regroups the motors as the speed rises | groupings (S→P, S→SP→P, S only, P only) |
| `chopper` | continuous voltage instead of resistor steps | response time |
| `traction-converter` | three-phase traction converter | starting effort, power, v max, ramp time, pull-out speed |
| `dynamic-brake` | electric brake of the drive it hangs on | force, power, fade-out speed, regenerative flag, ramp time |
| `cooling` | heat store and blower of what is wired to its **heat** pin | heat capacity, cooling, natural convection, derating and cut-out temperature, ambient |
| **Steam** | | |
| `boiler` | water, steam and pressure | water and steam space, working pressure, safety valves, heating surface, superheater |
| `firebox` | grate, damper and blower | grate area and capacity, burning rate, blower draught, shovelful |
| `steam-cylinders` | regulator and cutoff into tractive effort | count, bore, stroke, wheel diameter, longest cutoff, back pressure, efficiency, v max |
| `injector` | feed water into the boiler against its pressure | delivery [l/s] |
| `tender` | water and coal carried along | water [l], coal [kg] |
| **Brake** | | |
| `compressor` | air supply | delivery [l/min], type (compressor / exhauster) |
| `main-reservoir` | main air reservoir | volume [l] |
| `driver-brake-valve` | the driver's valve feeding the brake pipe | equalising device (Angleicher) |
| `brake-pipe` | this vehicle's stretch of the pipe | volume, leakage, medium (air / vacuum) |
| `angle-cock` | parts the brake pipe | end (front / rear) |
| `air-hose` | couples the pipe to the neighbouring vehicle | end (front / rear) |
| `emergency-valve` | vents the pipe from the compartment or the cab | — |
| `limiting-valve` | caps the cylinder pressure whatever asked | limit pressure [bar] |
| `double-check-valve` | passes the higher of two pressures | — |
| `retainer-valve` | holds a residual pressure through the release | position (off / slow / low / high) |
| `ep-brake` | electropneumatic brake — applies by wire | application and release rate, vents-the-pipe flag, steps |
| `control-valve` | control valve reading the pipe | valve type (K-GP … KE-L2d), position G/P/R, braked weight [t] (overall and per position), transition times per position, load braking (weighing / changeover) |
| `aux-reservoir` | auxiliary reservoir | volume [l] |
| `relay-valve` | pre-controlled cylinder fed from the main reservoir | supplement flag (air supplement brake) |
| `brake-cylinder` | the cylinder itself | maximum pressure, cylinder/reservoir volume ratio |
| `brake-rigging` | puts the cylinder force onto the wheel | friction pairing (block, disc, K, LL, Mg, custom curve), maximum force |
| `direct-brake` | direct (additional) brake of a traction unit | maximum cylinder pressure |
| `parking-brake` | parking brake | force, spring accumulator flag |
| `mg-brake` | magnetic track brake | force |
| `wheel-slide-protection` | slip protection | mode: slip brake / traction cutback / creep control |
| `sander` | sanding gear | sand rate [kg/min] |
| **Running gear** | | |
| `wheelset` | where force meets rail | axle count, adhesive mass fraction |
| `bogie` | groups the axles it carries | wheelbase |
| `axle` | one axle — drawn out to refine the wheelset | driven flag |
| **Control** | | |
| `cab` | the driver's desk: throttle, brake demand, direct brake, sanding, regulator, cutoff | — |
| `afb` | AFB in the throttle path | — |
| **Logic** | | |
| `value-in` | takes a reading out of the vehicle | reading (speed, brake pipe, motor current, temperature, …) |
| `constant` | a fixed number | value |
| `value-curve` | piecewise linear characteristic | table |
| `combine` | two values into one | operation (sum, difference, product, smaller, larger) |
| `clamp` | holds a value inside a range | lower and upper limit |
| `pid` | controls the actual value onto the set point | proportional, integral, derivative, output limits |
| `notch` | steps the output and limits its rate | notches (0 = continuous), rate |
| `rate-of-change` | how fast the input is changing | smoothing [s] |
| `value-switch` | picks one of two values, with hysteresis | threshold, hysteresis |
| `signal-out` | where the logic takes hold | sink (power controller, brake, sanding, blower, free value 1–4) |
| **Equipment** | | |
| `sifa` | driver's safety device | kind: time-time / time-distance / RZM |
| `pzb` | intermittent train protection | variant (I 54 … PZB 90 V2.0), train type O/M/U |
| `lzb` | continuous train protection | — |
| `gnt` | tilting speed supervision | — (needs `tilt_angle_deg` above zero on the vehicle) |
| `doors` | door control | system (TB0 / TAV / UIC-WTB), passenger doors flag |
| `script` | the Lua behaviour hook | script `"<mod>:<name>"` |

#### Block presets (`blocks/*.ron`)

A mod extends the palette with **presets**: a built-in block under a new name with new
parameter defaults — a data sheet, not new physics. A `mods/<id>/blocks/l620.ron`
holding the figures of a Voith L 620 reU2:

```ron
(
    id: "l620",
    name: "Voith L 620 reU2",
    description: "Two-circuit hydraulic transmission of the DB class 218: starting converter and travel converter, 2 700 kW input.",
    base: "hydro-transmission",
    params: {
        "final_ratio": Number(1.58),
        "wheel_diameter": Number(1.0),
        "fill_time": Number(1.2),
        "hysteresis_kmh": Number(8.0),
        "efficiency": Number(0.96),
        "circuits": Circuits([
            (kind: Converter, ratio: 3.93, stall_ratio: 2.4, coupling_nu: 0.85,
             absorption: 0.53, absorption_slope: 0.15,
             shift_up_kmh: 72.0, shift_primary_kmh: 20.0),
            (kind: Converter, ratio: 2.05, stall_ratio: 1.9, coupling_nu: 0.9,
             absorption: 0.5, absorption_slope: 0.1,
             shift_up_kmh: 0.0, shift_primary_kmh: 0.0),
        ]),
    },
)
```

`base` names the built-in block, `params` overrides its defaults with the same typed
values a graph uses. The preset appears in the editor palette under its `name`,
a vehicle's `graph` addresses it as `<mod>:<id>`, and it bakes exactly like
its base block with those defaults. An unknown `base` or a wrongly-typed parameter is
a loading warning, never a crash. Behaviour beyond the built-in physics still goes
through the `script` block — the Lua hook, as everywhere else.

### Model (glTF)

Models are glTF/GLB and use the format's own features. `mods/` is registered as an asset
source, so the path is stated **relative to the mods directory** — `<mod>/assets/<file>`,
loaded as `mods://<mod>/assets/<file>`. Everything else in a mod's `assets/` directory
(textures, sounds) is reachable the same way.

The example model `example/assets/br101.gltf` is generated by `tools/gen_br101.py` — a
procedural BR 101 built from scratch (no third-party assets in the model, licensed like
the project); re-run the script after editing it. The sounds the example vehicle's
sound table plays are the exception: recordings in `assets/sounds/` — CC0 cab clicks, and
driving noise of the real loco cut out of CC BY trainspotting videos and CC0 recordings —
credited in [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md) and rebuilt from their
sources by `tools/sounds/br101_sounds.py`. The track textures in
`example/assets/track/` are CC0 as well — a photographed Schotter, weathered concrete
and creosoted planks, credited in the same file.

```ron
model: Some((
    file: "example/assets/br101.gltf",
    lods: [(level: 0, distance: 150.0), (level: 1, distance: 400.0)],
    // Where a passenger may sit (see People): the floor point below the pelvis in
    // model space — the seat is 0.45 m above it, knees and feet in front of it — and
    // which way the seat faces (0 = ahead, 180 = backwards).
    seats: [(pos: (0.6, 1.25, -3.0)), (pos: (-0.6, 1.25, -3.0), yaw_deg: 180.0)],
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
| Custom property `ts_function` on the object | exported into glTF `extras` and beats the name. Optional: `ts_motion` (`rotate`/`translate`/`visibility`/`emissive`), `ts_axis` (`"0 0 1"`), `ts_amount` (degrees or metres) |

Custom properties are set in Blender under *Object Properties → Custom Properties*; the
glTF exporter writes them into `extras` if "Include → Custom Properties" is switched on.

The simulator spawns the model in place of the placeholder body, shows the level of detail
whose distance the vehicle is within, and moves the bound parts. Which functions have a
value today: `pantograph`, `door_left`, `door_right`, `gauge:speed`, `gauge:brake_pipe`,
`gauge:cylinder`, `gauge:main_reservoir`, `gauge:tractive_effort`, `switch:throttle`,
`switch:reverser`, `switch:direct_brake`, `switch:cab_light`, `switch:instrument_light`,
`lamp:main_switch`,
`lamp:sanding`, and
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

**Instrument backlighting** is one of those parts, and it has a motion of its own:
`motion: Emissive` does not move the node — it scales the **emissive colour of your
material** by the value, so the node glows along a dimmer instead of popping on at half
travel. Model the panel face a second time with an emissive material, put it behind the
dials that stand proud of it, and bind it to `switch:instrument_light`:

```ron
(node: "instrument_light", function: "switch:instrument_light", motion: Emissive),
```

`switch:instrument_light` is the instrument dimmer of the cab (keys `,` and `.`, or a
`CabControl::InstrumentLight` knob in the 3D cab); `switch:cab_light` is the cab lamp's
plain on/off switch (key 0) for a vehicle whose panel is simply lit or not. The colour in
your material is the fully-turned-up look, so how bright a cab glows stays yours — the
example loco carries `instrument_light` in `br101.gltf`. A node named `…_NIGHT` is the
third variant, for anything that follows dusk rather than a switch (see
[Lit windows at night](#track-objects)).

```bash
cargo run -p app -- --loco example:br101_afb --camera outside
```

### Interactive 3D cab

The `cab:` block inside `model:` makes the cab operable with the mouse (plan ch. 12).
It has three parts: the driver's **eye point** in model space (X right, Y above the rail
head, −Z ahead — where the cab camera sits), the **windscreens** the rain runs down, and
the **controls**, each binding a glTF node to a simulation input:

```ron
cab: Some((
    eye: (-0.55, 2.55, -6.5),
    windscreen: ["windscreen_front", "windscreen_rear"],
    controls: [
        (node: "cab_throttle", input: Throttle,
         motion: Rotate(axis: (1.0, 0.0, 0.0), degrees: 60.0)),
        (node: "cab_sifa", input: Sifa,
         motion: Translate(axis: (0.0, -1.0, 0.0), metres: 0.015)),
    ],
))
```

`windscreen` names the panes the weather works on (plan 14.1): rain gathers on them as a
film and as drops, the drops run down the glass at a stand and up it above about 15 km/h,
and the `Wipers` control clears a strip wherever its blade has just been. Three things a
pane wants:

- **A node of its own.** The runtime replaces the node's material, so a pane that is one
  primitive of the body mesh would take the rest of the body with it.
- **UVs across and up it**: `u` from the driver's left edge of the pane to his right, `v`
  from its bottom to its top. `u` is the frame the wiper sweeps in — a pane whose UVs run
  the other way has its wiper clearing the wrong side.
- **A material to look through.** The pane keeps whatever the model gave it and only has
  water added, so a nearly clear, blended, double-sided material is what a windscreen
  wants; `tools/gen_br101.py` writes one called `windscreen`.

`wiper` describes the blade on the *first* named pane, in the pane's own frame, and the
same numbers that pose the 3D blade (its `parts` entry): `pivot` in pane UV, `length` in
metres, `rest_degrees`/`sweep_degrees` from the pane's up axis towards +u. From mode and
clock the glass shader reconstructs the whole sweep analytically, so the cleared arc, the
bulge of pushed water at the blade's edge, and the drawn blade can never drift apart —
and the arc fills back in as fast as the rain can wet it.

Naming no pane is fine — then nothing is drawn on the glass, and the rest of the weather
is unaffected. A pane without `wiper` collects water that nothing clears.

`input` is one of a closed list (the vehicle editor offers them in a dropdown):
`Throttle`, `Reverser`, `BrakeValve`, `DirectBrake`, `AfbTarget`, `Sifa`,
`PzbAcknowledge`, `PzbExempt`, `PzbOverride`, `LzbTakeover`, `LzbEnd`, `LzbTest`, `Horn`,
`Sanding`, `BrakeRelease`, `EngineStart`, `DoorReleaseLeft`, `DoorReleaseRight`,
`DoorClose`, `ParkingBrake`, `EpBrake`, `Afb`, `Battery`, `Pantograph`, `MainSwitch`,
`Compressor`, `TrainType`, `Wipers` (off – interval – slow – fast), `Headlights`,
`CabLight`, `InstrumentLight` (the instrument dimmer, continuous), and `Display(0)` …
`Display(7)` — softkeys for the [cab displays](#displays), read by the `display(ctx)`
script hook.

How a control answers the mouse follows from the input, nothing is configured:

- **Push buttons** (Sifa, PZB/LZB buttons, horn, sanding, …) are held while the mouse
  button is down and spring back on release.
- **Switches** (battery, pantograph, main switch, reverser, train type switch, …) cycle
  to their next position on click and step without wrap on the scroll wheel.
- **Levers, valves and knobs** (power controller, brake valves, AFB target, instrument
  dimmer) follow a drag along
  their on-screen direction of travel and step finely on the scroll wheel. The driver's
  brake valve runs fill – release – lap – service range – emergency over its travel.

`motion` is the same type the moving parts use and describes the node's travel between
input 0 and 1. The whole subtree of the node takes the mouse; the hovered control glows
and the HUD names it with its position. Everything also stays operable from the keyboard —
the two write the same inputs. The camera looks out of `eye` (F1), pans with the arrow
keys and looks around while the right mouse button is held. F4 stands the driver up: the
eye then walks from `eye` through the vehicle's box (WASD, shift runs, space jumps) and
the mouse looks around by itself, so the eye point is the start of the walk, not a fixed
seat. `E` at the side of the vehicle steps out onto the ground and back in again — an
open passenger door (`doors` of the vehicle, ch. 9) or, on a traction unit, the cab door
of a vehicle carrying `cab` data. Outside, the ground and the walls are the meshes
themselves: what the walker stands on and what stops him comes out of a ray cast against
terrain, platforms and objects, so a platform needs no collision data of its own — only
a mesh at the height it is supposed to be walked at. The same ray carries him inside a
vehicle: an interior with modelled floors and stairs is walked as it is drawn, and a
model without one holds him on the floor its `eye` implies. Past the end of a vehicle he
walks on into the next one of the train.

### The character

The walker wears one of the mods' people (see *People* below): without a flag the first
character with the `Player` role, in registry order, and `--character` picks another one
— by name, or as a file from the same `mods://` paths as the vehicle models:

```
cargo run -p app -- --camera walk --character people:f01_lena
cargo run -p app -- --camera walk --character example/models/driver.glb
```

The model stands in its own origin — feet at Y = 0, looking along −Z, metres, Y up, which
is what the character pipeline gives. It is shown whenever the camera is not the
walker's own eye (F2, F3); in the first person it is hidden, because the eye sits inside
its head and its body would otherwise be in the way of the walker's ray casts. A model
with idle and walk clips (see *People* below) is animated: it stands with a little life
in it and walks when the walker walks, in the walk clip nearest his pace, the cycle sped
up with it; a model without clips simply stands.

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
  playback speed. `factors` is a list of further curves multiplied into the volume — a
  second quantity scaling an entry whose volume already follows a first one. The default
  rolling entry uses it for the track: `factors: [(quantity: Roughness, points: [(0.5,
  0.75), (2.0, 1.4)])]` — the `Roughness` quantity is the `roughness` of the track type
  under the vehicle (see Track types), so jointed superstructure is audibly louder than
  welded rail. `Rain` is the same pattern for the weather — it is how hard it falls
  rather than whether it does, so a drizzle is not a downpour with the volume turned
  down — and it is the hook for a rain-on-the-roof loop, as a condition or a volume
  curve. `Thunder` goes with it: 1.0 the moment the clap arrives (`distance / 343 m/s`
  after the flash) and rolling off after it, longer the further away the strike stood,
  which is what a triggered thunder entry hangs on.

```ron
sounds: [
    // A loop, modulated: no trigger, two curves.
    (
        name: "rolling",
        file: "synth:rolling-mid",
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
scheme as a model — WAV, OGG/Vorbis and FLAC are decoded) or one of the sources the app
generates at start-up: `synth:rolling-low`, `synth:rolling-mid`, `synth:rolling-high`,
`synth:traction-low`, `synth:traction-mid`, `synth:traction-high`, `synth:air`,
`synth:compressor`, `synth:horn`, `synth:buzzer`, `synth:squeal`, `synth:joint`,
`synth:contactor`. (`synth:rolling` and `synth:traction` without a band are the middle
band, for tables written before there were bands.)

`positional: true` places the sound on the vehicle: it is attenuated by distance,
stereo-placed, Doppler-shifted — which is what makes another train audible as it passes —
and low-passed, more with distance (air absorbs treble long before bass) and much more
while the camera sits in the cab, the cab wall. Sounds of the driver's desk — buzzer,
Sifa — set it to `false` so they stay at a constant place when the camera goes outside,
and they are never filtered: they are in the cab with the listener.

#### Layers

**One sound is normally several entries.** A single loop stretched over a whole speed range
by its playback rate drags its formants along with it and arrives at the top of the range as
a toy train. Three loops whose volume windows overlap each stay near their own pitch and hand
over to the next:

```ron
// Band 1 of 3: silent at a stand, full from 12 to 35 km/h, gone by 60.
(
    name: "rolling-low",
    file: "synth:rolling-low",
    trigger: Loop,
    volume: Some((quantity: Speed, points: [(0.0, 0.0), (12.0, 1.0), (35.0, 1.0), (60.0, 0.0)])),
    factors: [
        (quantity: Speed, points: [(0.0, 0.0), (60.0, 0.55)]),
        (quantity: Roughness, points: [(0.5, 0.75), (2.0, 1.4)]),
    ],
    pitch: Some((quantity: Speed, points: [(0.0, 0.85), (60.0, 1.25)])),
    positional: true,
),
// Band 2 fades in over exactly the range band 1 fades out over.
(
    name: "rolling-mid",
    file: "synth:rolling-mid",
    trigger: Loop,
    volume: Some((quantity: Speed, points: [(35.0, 0.0), (60.0, 1.0), (95.0, 1.0), (130.0, 0.0)])),
    // … same factors, pitch ramp over 35 … 130 km/h
),
```

Two rules make it work:

1. **Neighbours share a flank.** Layer A's fade-out (`35 … 60`) is layer B's fade-in, so the
   windows sum to 1 right through the handover — no dip, no doubling.
2. **The window occupies the volume curve**, so how *loud* the sound is at all goes into
   `factors`, which are multiplied in. Above, the first factor is the level over speed and
   the second the track roughness.

Keep each band's pitch ramp inside roughly 0.85 … 1.3. Swap the `synth:` names for six
recordings and nothing else about the table changes — `mods/example/vehicles/br101_afb.ron`
is the worked example, with loops cut out of recordings of the real loco (their sources in
[THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md), the cutting in
`tools/sounds/br101_sounds.py`). A sound with a fixed pitch — the 101's line-side
converters, which whine at 500 Hz and its harmonics whenever the main switch is closed,
louder under load — is its own entry on `MainSwitch` with a `TractiveEffort` factor rather
than part of a band.

#### Hearing it

The vehicle editor plays an entry: **▶ Preview** on its card starts it through the editor's
own output device and puts up a slider for every quantity the entry depends on. Moving the
speed slider walks the crossfade — auditioning `rolling-low` and then `rolling-mid` at the
same speed is what a level step between two layers sounds like. The preview is a plain
stereo bus: no distance, no cab wall, no reverb, because those belong to a place in the
world and the editor has none. Volume and playback speed are shown as numbers next to the
button, so a condition that mutes the entry is visible and not merely inaudible.

Triggers:

| Trigger | Fires |
|---|---|
| `Loop` | never — the sound runs and is modulated |
| `Rises(quantity: X, threshold: t)` | when `X` crosses `t` upwards |
| `Falls(quantity: X, threshold: t)` | when `X` crosses `t` downwards |
| `Every(quantity: X, interval: i)` | at every multiple of `i` — 30 m of `Distance` is a rail joint, 1 of `TapChangerStep` a contactor |

Quantities: `Speed` [km/h], `Distance` [m], `EngineRpm` [1/min], `TapChangerStep`,
`Circuit`, `TractiveEffort` [kN], `BrakeEffort` [kN], `DynamicBrake` [kN] (the electric
or hydrodynamic brake alone — what the converter is heard doing while the train slows),
`BrakePipe` [bar], `BrakeCylinder` [bar], `AirFlow` [bar/s], `Slip` [m/s], `Throttle`,
`Pantograph`, `MainSwitch`, `Compressor`, `Doors`, `Horn`, `Alert` (any train protection
or vigilance device demanding an operation), `VigilanceAlert` (the Sifa sounding),
`ProtectionAlert` (the PZB/LZB horn — an acknowledgement due, a supervision tripped, the
LZB being accepted or ended), and `Control(<input>)` — the
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

`mods/example/scripts/afb.lua` — a scripted AFB. `sim-core` brings its own
(`afb: true` in the vehicle file); the example vehicle leaves that flag off and
shows how a script replaces the built-in behaviour, here with the line speed as
an additional ceiling:

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
    tags: ["ks", "main-signal"],  // optional, for the content drawer's filter (see Tags)
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
    tags: ["ks", "mast", "zs3"],  // optional, for the content drawer's filter (see Tags)
)
```

A part without `mount` stands at the device position; every other part hangs off a
named node of another part. `lamps` binds the free-form lamp-image strings of the
signal type to glTF nodes: the node is **visible while its string is in the current
lamp image** and hidden otherwise — a Zs3 digit is a lamp like any other, and a
script's `lamps` (the Zs1 below) lights them the same way.

**Moving parts — semaphore signals.** `motions` binds a lamp-image string to a node
that *travels* instead of switching: while the string is in the lamp image the node
moves to full travel, without it back to rest, linearly over `seconds` — a quick
aspect change swings a semaphore arm through its real intermediate positions. The
strings name the moved elements, so an aspect that moves two arms lists two of them
(`signals/hv_form.ron` + `signal_models/form_hp.ron`):

```ron
motions: [
    (lamp: "fluegel1", node: "fluegel1",
     motion: Rotate(axis: (0.0, 0.0, 1.0), degrees: 45.0), seconds: 1.8),
    (lamp: "fluegel2", node: "fluegel2",
     motion: Rotate(axis: (0.0, 0.0, 1.0), degrees: 135.0), seconds: 1.8),
]
```

`motion` takes the same `Rotate`/`Translate`/`Visibility` as vehicle parts; one
binding per node. Rest pose (travel 0) is the stop position.

**Levels of detail.** An optional `lods` table switches nodes named
`<name>_LOD<level>` by camera distance, exactly like vehicles: coarsest last,
beyond the last distance the LOD nodes disappear; nodes without the suffix are
every level's furniture. Empty = the whole assembly at every distance.

```ron
lods: [(level: 0, distance: 200.0), (level: 1, distance: 2500.0)]
```

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

The **route editor** (`trainsim-route-editor`) edits a line over aerial imagery, with the
track tools of Train Simulator Classic's World Editor: lay track by pressing and dragging the
standing end (arc-to-point per click, a straight under `Ctrl`, snapping onto open ends,
optionally as clothoid – arc – clothoid with the rulebook's cant), start
a lay on a track's middle to make a turnout of it (drag along the track = facing, against it =
trailing — split and wired automatically), split, join (a stake-out calculator after Zusi's:
transitions – arc – compensating straight, or a double arc with an intermediate straight;
speed, radius, transitions and cant configurable), offset a parallel, build a crossover
between two parallel tracks, set gradient break points, and bend existing track by dragging
the round support-point handles of a selected edge. The
throw time of a turnout is edited on the tracks that meet at it. A *Checks* panel lists wiring
that compiles but fails on the line — a distant signal without its 1000 Hz magnet, a device
beyond its track, a boundary on a node that is no buffer.

The interlocking tables are edited there as well, so none of them has to be typed as RON:
a placed `Signal` device gets its **signal table entry** in the selection panel (kind, system,
the signal it announces, guarded sections, whether it needs a route, diverging speed, signal
type and 3D model) together with **the routes that start at it** — where each one ends and
what it locks, and *Find routes*, which runs out over the track and offers one route per leg
of every turnout ahead, each ending at the next signal on it. Routes already in the file stay
as they are, so finding again after a change adds what is new and touches nothing else.
The *Interlocking* panel holds the **sections** (a section is the set
of tracks that count as occupied together) and the **routes** (entry and exit signal, the
sections and the overlap they lock, the switch positions they require). *Derive path*
follows the track from the entry to the exit signal and fills the sections, the switch
positions and the overlap in by itself. The overlap comes out at the regular length of
the German rulebook for the speed the route ends at — 50 m up to 30 km/h, 100 m up to 60,
200 m up to 100, 300 m above that, and a diverging route counts with the entry signal's
diverging speed. Switch *regular length* off to walk out a length of your own; either way
the sections it reaches stay editable afterwards. Pointing at a section or a route draws
it on the map — its tracks in green, the overlap in orange, the flank protection in
violet — so the index lists can be checked against the line.

**Flank protection** (`flank` on a route) is what keeps a vehicle off the path where a
track joins it. Two kinds, both enforced by the interlocking: a **protecting turnout**
(`Switch(node, position)`) is set and locked with the route like one in the path, and a
**protecting signal** (`Signal(index)`) is held at stop for as long as the route is set —
no route can be cleared from it meanwhile, and a signal another route already runs from
cannot be taken as protection in the first place. The derivation fills both in wherever a
route trails a turnout, which is where the leg it does not use joins the path; a turnout
the route runs into facing needs none, because it already lies in the position that leads
a flank movement away.

A **track lock** (Gleissperre) is a signal, not a device of its own: give the signal table
entry `kind: TrackLock` and it has two states — stop is laid on, proceed is laid off. The
interlocking lays it off for a route running over it and holds it on where a route names it
as flank protection; no route ends at one, and none starts there. Everything visible is
yours: `mods/example/signals/gleissperre.ron` is the two-rule signal type, and a
`signal_models/*.ron` binds its `"sperre_auf"`/`"sperre_ab"` strings to the shoe — as a
`motions` entry it swings between the two positions over its travel time instead of
jumping. Without a model the app draws a plain shoe in the colour of the aspect.

Start a line with `cargo run -p app -- --line example:beispielstrecke`.

A `LineConductor` device marks a line conductor section — the cable, not what it transmits:

```ron
(kind: LineConductor, edge: 1, s: 0.0, payload: "(length:4000.0,cir_elke:false,end:false)"),
```

`length` is how far the section transmits, `cir_elke: true` marks a CIR-ELKE section (steeper
braking curve, 5 km/h speed steps, speed rises effective at the head of the train instead of
at its rear), `end: true` marks the last section of an LZB area, where the end procedure runs.
The last two may be left out.

A `Balise` device carries the data points of the **GNT**, the speed supervision of a tilting
unit. Inside a GNT area the on-board unit supervises a higher profile than the regular one —
but only while the tilting technology works:

```ron
(kind: Balise, edge: 1, s: 0.0, payload: "(profile_speed:160.0,length:2400.0)"),
(kind: Balise, edge: 1, s: 2400.0, payload: "(profile_speed:0.0,target_speed:0.0,target_distance:0.0,length:0.0,end:true)"),
```

`profile_speed` is the GNT speed from that data point [km/h] — `0` releases nothing and
leaves the regular profile in force. `target_speed` and `target_distance` name the point the
braking curve leads down to; leave `target_distance` at `0` and the profile speed applies over
the whole section. `length` is how far the data point is good for [m], measured to the rear of
the train, and `end: true` is the sign-off at the end of the GNT area. Everything but
`profile_speed` may be left out. A balise whose payload is not a GNT data point is ignored, so
the same device kind stays free for ETCS later.

The movement authority is not written into the line: the LZB centre builds it every step from
the block division and the state of the interlocking. The block division is a line datum of
its own — `BlockMarker` devices naming the section behind them:

```ron
(kind: BlockMarker, edge: 2, s: 0.0, payload: "(section:2)"),
```

A **platform** is a device too — the stop board of a timetable stop and the ground the
waiting passengers stand on (see *People*): it sits at the platform's start and runs on
for `length`, on the side the sign of its `lateral_offset` says (positive = left of
increasing arc length), and `height` is the platform surface above the railhead. A line
whose platforms are not modelled leaves the height out and the people stand on the
ground:

```ron
(kind: Platform, edge: 0, s: 1770.0, facing: Both, lateral_offset: 5.0,
 payload: "(name:\"Beispielstadt\",length:210.0,height:0.76)"),
```

An authority runs to the first boundary that is not clear: a block marker whose section is
occupied, or a main signal at stop. v-target is the most restrictive point ahead — a speed
step of the line counts as much as a stop, so a slow section needs no device of its own.
Whether the block mode ends up as full or partial block mode falls out of the line data too —
a line with block markers of its own divides the authority itself, a line without them leaves
the main signals as the only boundaries, and the signals stay binding together with their PZB
magnets.

### Track types

A **track type** (`track_types/*.ron`) describes the superstructure — what the track is
built like, not where it runs:

### Track types

A **track type** (`track_types/*.ron`) describes the superstructure — what the track is
built like, not where it runs:

```ron
(
    name: "Hauptbahn (B90)",
    texture: Some("example/assets/track/ballast.jpg"),  // ballast texture, tiled along the track
    normal_map: Some("example/assets/track/ballast_nor.jpg"),  // ballast normal map, same tiling
    color: (0.32, 0.30, 0.28),   // untextured fallback; the route editor tints sections its own way
    roughness: 1.0,              // scales the rolling noise; jointed track > 1, slab track < 1
    reverb: 0.0,                 // how much the surroundings ring: 0 = open line, 1 = tunnel
    max_speed: 250.0,            // superstructure limit [km/h], caps the line's speed profile
    lzb: false,                  // true: a line conductor belongs on this track (rule check)
    // The physical build, in real dimensions. Defaults are the DB Regeloberbau
    // (60E1 rail on concrete at 60 cm, 30 cm ballast).
    oberbau: (
        rail: R60,               // R49 = 49E1/S 49, R54 = 54E3/S 54, R60 = 60E1/UIC 60
        sleeper: Concrete,       // Concrete | Wood | Slab (Feste Fahrbahn, no ballast)
        sleeper_length: 2.6,     // across the track [m]; DB standard 2.6 m
        sleeper_width: 0.32,     // along the track [m] (B 90: 0.32, wood: 0.26)
        sleeper_height: 0.21,    // [m] (B 70/B 90: 0.21, wood: 0.16)
        sleeper_spacing: 0.60,   // between sleeper centres [m]; 0.60 = 1667 per km
        ballast_overhang: 0.70,  // shoulder beyond the sleeper end [m] each side
        ballast_depth: 0.30,     // ballast under the sleeper [m]
        sleeper_texture: Some("example/assets/track/sleeper-concrete.jpg"),
        sleeper_normal_map: Some("example/assets/track/sleeper-concrete_nor.jpg"),
    ),
    tags: ["main-line", "welded"],  // optional, for the content drawer's filter (see Tags)
)
```

The renderer builds what the `oberbau` says: the two rails are extruded from the real
rolled section (49E1/S 49, 54E3/S 54, 60E1/UIC 60 — 1435 mm gauge measured 14 mm under the
rail top, 1:40 inclined toward the gauge), the sleepers stand at the type's spacing and
shape, and the ballast bed follows the RL 853 cross-section — top width sleeper + twice the
shoulder, sides falling 1:1. A sleeper texture repeats 2.6 m along the sleeper, so a wood
plank set reads one plank per sleeper; with `sleeper: "slab"` the bed becomes a concrete
slab (Feste Fahrbahn) and the sleeper fields mean slab width and thickness instead. The
detailed sleepers are drawn only up to 400 m — beyond, the bed's texture carries the look.
A type that names no textures is skinned in its `color`; textures tile every 4 m.

A line assigns types per edge as steps over the arc length, so one edge changes its
superstructure section by section; the reserved name `"default"` returns to the built-in
default type:

```ron
track_type: [(0.0, "example:hauptbahn"), (3000.0, "example:altbau")],
```

`max_speed` merges into the speed profile every consumer already reads (AI, LZB, HUD,
scoring); `roughness` reaches the sound table as the `Roughness` quantity (the default
rolling entries carry a volume factor on it, see Sounds); `reverb` drives the reverb the
simulator mixes under everything the player hears — a tunnel type sets 1.0, a station hall
or a deep cutting sits around 0.3 … 0.6. Modelling the room on the track type rather than
on the terrain is the same trade `roughness` makes: a line says where its tunnels are by
assigning the type, and nothing has to trace geometry at run time. The route editor edits the
sections in the selection panel (a color chip per section, the map tints the track ribbon
to match) and its rule check flags names no installed mod has, and LZB types on a line
that places no line conductor.

### Electrification

What hangs over the track is a property of the **track**, not an assumption of the vehicle.
Every edge states its own wire, section by section, in the same shape as the other
profiles:

```ron
electrification: [(0.0, "ac-15kv"), (1200.0, "none"), (1260.0, "dc-1.5kv")],
```

The ids are `"ac-15kv"` (15 kV 16.7 Hz), `"ac-25kv"` (25 kV 50 Hz), `"dc-3kv"`,
`"dc-1.5kv"`, `"third-rail"` (750 V DC) and `"none"` for track under no wire at all. An
edge that says nothing carries no wire.

That example is a system boundary: sixty metres with nothing over them, because the wire
of two systems must not be bridged by a pantograph. A siding under no wire is the same
thing with one entry.

A vehicle states what it is **built for** — its `pantograph` blocks, one per system, and
the main switch closes only where the line carries one of them. The volts alone do not
decide: 25 kV is plenty of volts for a 15 kV locomotive and still the wrong system, and
the switch stays open. Running off the end of the wire drops it, exactly like a neutral
section.

The route editor edits the wire in the selection panel, on the track that is selected —
there is no line-wide switch. A line file written before the wire moved onto the track may
still carry a line-wide `electrification: "ac-15kv"`; it keeps applying where an edge says
nothing, and is kept on save, but nothing writes it any more.

### Track areas

Step profiles per property are right for a compiler and wrong for a person. A 40 km/h
restriction through a station means editing the speed steps of every track it touches, and
changing it again means finding them all a second time.

A **track area** is that job the way a builder thinks about it: mark the stretch, name it,
and set the properties on it.

```ron
areas: [
    (
        name: "Bahnhof Musterstadt",
        color: (0.95, 0.72, 0.25),
        width: 2.5,               // half-width of the stroke it is painted with [m]
        spans: [
            (edge: 3, from: 0.0,   to: 850.0),
            (edge: 4, from: 0.0,   to: 850.0),
            (edge: 7, from: 120.0, to: 640.0),
        ],
        speed: Some(40.0),
        track_type: Some("example:bahnhofsgleis"),
        // cant, grade and electrification are left unset: what the area does not
        // state, it does not touch.
    ),
]
```

A span is `[from, to)` along one track. What an area does not set it leaves alone — which
is what lets a speed restriction run across an electrification boundary without disturbing
the wire. Areas are laid over the tracks' own profiles **in file order**, so a later area
wins where two overlap: that is what "drawn on top" means on the map.

Nothing in the simulation knows about them. Loading bakes them down into the same step
profiles the tracks have always carried, so an area is an authoring convenience with no
run-time cost and no new concept downstream.

| Property | Sets |
|---|---|
| `speed` | permitted speed [km/h] |
| `cant` | cant [mm] |
| `grade` | longitudinal gradient [‰] |
| `track_type` | superstructure — rail section, sleepers, ballast, texture, roughness, reverb, speed limit, LZB flag |
| `electrification` | what hangs over it (see Electrification), or `"none"` |

In the route editor the areas are **painted**: pick **Mark area**, press on a track and
drag along it. The stroke follows that track until the button goes up — it never jumps to
a neighbouring track halfway through a station — and what it leaves behind is a **wide
coloured stroke over the rails**, half transparent so the track still reads underneath.
With an area selected the next stroke joins it, which is how one area comes to cover a
whole station, one track at a time. The stroke width is a property of the area, so a wide
marking stays wide when the file is opened again; the brush setting is the width new areas
get. The selected area keeps its colour and wears the editor accent as an outline. The
**Track areas** panel lists them, and the selection panel edits the properties — each a checkbox and a value, so "set" and "left alone" are the same two
states the file has. The rule check flags an area that covers nothing, sets nothing, lies
off its track or names a type no installed mod defines; the track panel says when an area
lies over the track being edited, so a value edited there that never reaches the line does
not go unnoticed.

Areas follow the track they are marked on: splitting a track splits a stretch that
straddles the cut, and deleting a track takes its stretches with it.

### Track objects

A **track object** (`objects/*.ron`) is a 3D object placed *relative to the track* —
catenary masts, kilometre boards, platform lamps. The object carries the pose its author
meant, so the editor's object tool drops it correctly with one click:

```ron
(
    name: "Mast",
    model: "example/assets/mast.gltf",  // glTF below mods/, like vehicle models
    lateral_offset: -3.5,  // m, positive = right of increasing arc length
    yaw_deg: 0.0,          // about up, clockwise from above; 0 = front along the track
    height: 0.0,           // m above the railhead
    tags: ["mast", "catenary", "epoch-4"],  // optional, for the content drawer (see Tags)
)
```

**Seasonal variants are optional.** An object may name an autumn or a winter glTF next to
its year-round one; each brings its own textures, and whatever is left out falls back to
`model`. A mast, a board or a lamp names neither and looks the same all year:

```ron
(
    name: "Birke",
    model: "example/assets/birke.gltf",
    autumn_model: Some("example/assets/birke_herbst.gltf"),
    winter_model: Some("example/assets/birke_winter.gltf"),
)
```

Which one is spawned follows the scenario's start date (see *Seasons* below): the winter
variant while snow lies, the autumn one while the leaves have turned, the year-round model
otherwise. An object that ships no variant is never treated as a mistake.

**Walkways in the model.** A platform, a footbridge or a station forecourt brings the
places its people walk along with it, as empty nodes in the glTF (in Blender: empties,
named in the outliner):

| Nodes | Meaning |
|---|---|
| `wp_<name>_0`, `wp_<name>_1`, … | the vertices of a **footpath** `<name>`, in walking order; people walk it up and down |
| `wa_<name>_0`, `wa_<name>_1`, … | the corners of a **walk area** `<name>` in ring order; people wander inside it, the rest stand |

Custom properties on the `_0` node (exported as glTF `extras`) size the crowd: `people`
(how many, default 4 on a path and 6 in an area), `width` (metres, a path's; default 2)
and `walking_share` (0 … 1, the share of an area's people that wander; default 0.5). The
positions are the model's own, so the way follows the object wherever a line places it,
and the people stand at the height the nodes are at — on the platform's surface, not on
the ground under it. `mods/example/assets/platform.gltf` (`tools/gen_platform.py`) is the
worked example: a 210 m platform with `wp_edge_*` along its length and `wa_middle_*` as
its waiting area, placed on the example line at the stop at km 1.98.

A line places instances under `objects:`; each placement stores concrete values (stamped
from the object's defaults, editable per instance), so the file stands on its own:

```ron
objects: [
    (object: "example:mast", edge: 0, s: 500.0, lateral_offset: -3.5),
],
```

The simulator spawns the glTF at the track pose plus offset, rotation and height,
floating-origin safe like the signal models; an unknown object name gets a placeholder
block and a warning. In the route editor the **object tool** (key 5) picks an object kind
and drops it on the nearest track; the selection panel edits position, lateral offset,
rotation and height, and the rule check flags placements outside their track or naming an
object no installed mod has. **Repeat in a row** stamps copies of the selected instance
along its track — spacing (default 65 m, the standard catenary span) and end position,
each copy carrying the instance's own offset and rotation and staying individually
editable, so a mast that collides with a tree is simply moved. Nothing in the simulation
reads objects — they are the line's furniture, and deleting or splitting tracks carries
them along like devices.

A placement with `snap_to_terrain: true` stands on the **terrain surface** instead of the
rail plane — `height` then measures from the ground. The strip beside the track is
blended toward rail height, so a snapped object next to the ballast still meets it. The
editor's selection panel has the checkbox, and both programs resolve the height the same
way: an object is placed by the terrain tile it stands on, so it streams in and out with
that tile and its feet meet the ground the tile actually has.

**Levels of detail** work as for vehicles and signals: nodes named `<name>_LOD0`,
`_LOD1`, … are shown by camera distance. A model without the suffix is one level, drawn
up to the cull distance. This matters most for trees: every tree of a wood is drawn as an
**instance** of its model's mesh parts — not as its own scene — so a thousand firs
sharing one glTF are a handful of draw calls, and a coarse `_LOD2` is what keeps a whole
hillside cheap. Keep a level to **one material** where you can: the renderer spawns one
entity per mesh part per level, so bark and leaves in one atlas is half the entities and
half the draw calls of two separate materials.

The bands are 80, 260 and 800 m, and an object is culled at 2.5 km (3 km for scenery
objects). An object may name **its own**, which is what vegetation does:

```ron
// Visible in full to 20 m, then coarser twice, gone at 700 m.
lod_distances: [20, 60, 120, 700],
```

The list is finest first and its **last entry is the cull distance**. A model with fewer
levels than the list has entries runs its last level on to the cull distance. Scale them
to how big the plant is: a level pays for its triangles only while the plant covers
enough pixels, so a forty metre fir hands over at a hundred metres and is drawn to two and
a half kilometres, while a two metre blackthorn hands over at twenty and is gone at seven
hundred — and a hedge of blackthorn drawn to two kilometres is tens of thousands of draw
calls for nothing.

A **crossed-quad impostor as the coarsest level wants a late hand-over**, not an early
one. Two quads at a right angle are the least that works — a single fixed billboard
vanishes the moment the camera looks along it — and the pair has a seam: whichever blade
is edge-on is drawn as a narrow strip through the other. Hand over where the plant is a
few dozen pixels tall and the strip is under a pixel; hand over early and the tree looks
sliced. `mods/trees` uses about eighteen times the plant's height.

**Lit windows at night.** A node whose name ends in **`_NIGHT`** is shown after dusk and
hidden by day — lit windows in a house, a glowing sign, the light pool under a platform
lamp. Nothing is declared in the RON: model a second window pane with an emissive
material, call it `fenster_NIGHT`, and it switches like a signal's lamp node. The
convention holds for every glTF the world is drawn from (scenery objects, trees, signal
parts, vehicles), and a model without such a node simply never lights up. It is a hard
switch at dusk, not a fade — the glow lives in your material, and it stays yours.

```
Haus.gltf
├── mauern           always there
├── fenster          the dark pane by day
└── fenster_NIGHT    the emissive one, shown after dusk
```

### Walkways

People need places to be: **footpaths** and **walk areas** are line content, geo-positioned
like the trees, with the height taken from the terrain:

```ron
walk_paths: [
    // A footbridge from the forecourt to the platform, walked up and down by four
    // people at a time; `height` lifts a way that is not on the ground.
    (name: "Zugang", points: [(lat: 52.0002, lon: 10.0210), (lat: 52.0004, lon: 10.0214)],
     width: 2.0, people: 4, height: 0.0, tags: ["station"]),
],
walk_areas: [
    // The forecourt: twelve people about, half of them wandering between spots
    // inside the polygon, the rest standing.
    (name: "Vorplatz", polygon: [(lat: 52.0000, lon: 10.0205), (lat: 52.0000, lon: 10.0212),
                                 (lat: 52.0003, lon: 10.0212), (lat: 52.0003, lon: 10.0205)],
     people: 12, walking_share: 0.5),
],
```

A path needs two points, an area three corners (the rule check says so), and both have
to lie inside the module envelope — a corner dragged outside is reported. The route
editor's **footpath** and **walk area** tools (the *People* group of the palette) draw
them with clicks — Enter or right-click finishes, Esc cancels — and edit them like the
envelope: drag a vertex, click a side of the selected way to add one, Delete removes the
held vertex or the way. The people
themselves are the app's (see *People*): which character walks where, and at what pace,
is decided from the line and the scenario clock, never stored. A platform that is a
model carries its own walkways (see *Track objects*), so a line only draws what no model
brings.

### Stabling roads and portals

A line that is only driven along needs nothing here; a line that is **shunted** needs
somewhere to shunt to. `yards:` names the places stock lives — a mark on the track like a
device, with the direction a standing train faces and the length of the road behind it:

```ron
yards: [
    (name: "Portal West", kind: Portal, edge: 0, s: 300.0, facing: Forward, length: 300.0),
    (name: "Portal Ost", kind: Portal, edge: 1, s: 300.0, facing: Backward, length: 300.0),
    (name: "Abstellgleis 1", kind: Stabling, edge: 2, s: 20.0, facing: Backward, length: 280.0),
],
```

Two kinds, and the difference is what may happen there:

* **`Stabling`** is a siding on the modelled line. A unit left on one stands where it can
  be seen and occupies its road like any other train — this is what an operating day puts
  its stock on between two workings.
* **`Portal`** is the *edge* of the modelled line: the fiddle yard past the last signal,
  the junction to the railway you did not build. **Trains appear and disappear at portals
  and nowhere else.** The rule check refuses a portal whose track does not run out to a
  buffer stop or a module boundary — a train appearing on a running road is not a train,
  it is a collision.

`name` is content, not translated: it is a place on this line, like a station name, and it
is what a shunt job addresses. `edge`/`s` is where the **head** of a standing train comes
to; `facing` is the way it looks (into the line at a portal, out of the siding on a
stabling road), so the body of the consist lies on the other side of the mark. `length` is
what fits on the road — `0` means "not stated", and then nothing is refused for being
long. Two roads of the same name is a finding, because a job that names it would always
get the first one.

Yards follow their track exactly as devices and objects do: splitting an edge carries them
onto the right half, deleting one takes its roads with it, and a composition shifts every
`edge` by the module's offset — so a module names its own roads and the composed line has
them all. The example line
(`mods/example/lines/beispielstrecke.ron`) has a turnout at km 4.0, a stabling siding off
its diverging leg and a portal at each end.

The simulation reads them as `sim_core::yard::Yard`: `Sim::place_at(train, "Abstellgleis 1")`
puts a consist on the road (refusing one that is too long, a road that is occupied, or a
mark the track behind runs out on), and `Sim::withdraw(train, "Portal Ost")` takes a train
off the line at a portal it is actually standing at. A withdrawn train keeps its slot and
its vehicles — it is out of service, not deleted — so the same unit comes back out later.

### Shunt jobs for the AI

A driver can be given a **shunt job** instead of, or after, a timetable — a list of moves
worked off in order. It is an `ai_driver::ShuntJob` and reads and writes as RON exactly
like a timetable does, but there is no `jobs/` directory yet: for now a job is handed to a
driver where the world is built, not loaded from a mod. The format below is what a loader
will read when it arrives, so a job written today keeps working.

```ron
(
    name: "Rangierfahrt Musterstadt",
    moves: [
        SetBack(Yard("Abstellgleis 1")),
        Couple,
        DrawUp(At(edge: EdgeId(0), s: 900.0)),
        Uncouple(0),
        Stand,
    ],
)
```

`DrawUp` runs forward until the **head** is at the target, `SetBack` reverses until the
**rear** is — that is what "set back onto a road" means, the road takes the rear first.
Either move ends early when the buffers are met, whatever the mark said, because that is
what the shunter's arm is for. A target is either a point on the graph (`At(edge:, s:)`,
with an optional `module:` resolved against a composition like a timetable stop) or a road
by name (`Yard("…")`). `Couple` joins the train to whatever it stands up against;
`Uncouple(n)` parts it behind vehicle `n`; `Stand` finishes.

The job is driven at the German shunting speed, 25 km/h, creeping over the last few metres.
A target the line does not have, or a coupling that cannot be made, stops the train instead
of running it on to nowhere.

### Vegetation

Trees are ordinary track objects — an `objects/*.ron` with a tree glTF is all a tree mod
is. A line stores **every tree as its own entry**, geo-positioned (no track reference,
height always from the terrain):

```ron
trees: [
    // Empty object = the app's built-in placeholder tree.
    (object: "trees:fichte_b", lat: 52.0006, lon: 10.004, yaw_deg: 0.0, scale: 1.3),
],
```

The **`trees` mod** ships the vegetation of Central Europe: twenty-eight species — spruce,
pine, silver fir, larch, Douglas fir, juniper; beech, oak, hornbeam, birch, alder, two
maples, ash, lime, aspen, Lombardy poplar, two willows, elm, horse chestnut, rowan, wild
cherry, black locust; hazel, hawthorn, elder, blackthorn — each in three individually
shaped variants (`_a`, `_b`, `_c`), four levels of detail, and summer, autumn and winter
models. It is generated, not modelled: `tools/trees/species.json` describes the species,
`tools/trees/build_trees.mjs` grows them with
[ez-tree](https://github.com/dgreenheck/ez-tree), and the leaves on their cards are
photographed leaves out of the CC0 libraries of [ambientCG](https://ambientcg.com) and
[Poly Haven](https://polyhaven.com), cut out and arranged by the pipeline. See
`tools/trees/README.md` to add one.

#### Stands

A wood is rarely one species. Any tree object may carry **`stand-…` tags** saying which
kinds of wood it grows in:

```ron
tags: ["laubbaum", "stand-laubwald", "stand-mischwald", "stand-allee"],
```

The route editor collects every `stand-…` tag the installed mods carry and offers them
above the single species in the tree and forest tools. Picking one has the forest brush
draw from all the species tagged with it, so a painted wood comes out mixed and no two
trees in it are the same shape. There is no stand file and no registry — a mod that adds
a species to an existing wood needs nothing but the tag the others already have, and one
that invents `stand-obstgarten` gets a new entry in the list for free.

The stands the `trees` mod brings: `mischwald`, `laubwald`, `nadelwald`, `bergwald`,
`auwald`, `bach`, `heide`, `pionier`, `bahndamm`, `boeschung`, `hecke`, `feld`, `allee`,
`park`, `stadt`.

There is no separate forest construct: a wood is many tree entries. That is deliberate —
whether a tree was hand-set, painted or imported, it is the same kind of row, so any tree
can be moved, rescaled or deleted on its own afterwards. Trees stream in and out with
their terrain tile and share meshes per species, so even a big wood renders as instanced
draws.

In the route editor the **tree tool** (key 6) plants one tree per click. The **forest
brush** (key 7) outlines an area — Enter or right-click **bakes** it into single trees
(one per `area per tree` m², species from the tool options, clear of the track strip).
**File ▸ Import forest…** reads an Overpass JSON extract (`landuse=forest` /
`natural=wood` ways, same download path as the track import) and bakes each polygon the
same way — an optional aid: whoever wants every tree hand-set simply never uses it, and
an imported wood is thinned out or cleared exactly like a painted one. For bulk edits the
**marking brush** (key 8) sweeps over the map and marks every tree and object under the
circle; Delete (or the panel button) removes them together in one undo step.

### Fields

The countryside a line runs through is mostly farmed, and a field is not a green
rectangle: it is winter wheat, and in April that is ankle-high and blue-green, in June
waist-high, in late July gold and about to be cut, and in August a stubble field. A line
stores the **outline and the crop**; everything else follows from the crop and the date:

```ron
fields: [
    (
        polygon: [(lat: 51.5901, lon: 8.1402), (lat: 51.5901, lon: 8.1436), …],
        crop: "winter-cereal",
        // What the register said, kept verbatim — nothing reads it, but it is
        // what says where a wrong crop came from.
        code: "115",
        label: "Winterweichweizen",
        // The direction the field was worked in, against grid east. Furrows,
        // tramlines and the combine's swath all run along it.
        direction_deg: 12.4,
        source: "NW",
        year: 2026,
        seed: 8123481956122354,
    ),
],
```

`crop` is one of thirteen groups — the resolution at which two fields genuinely look
different from a train window:

| id | | id | |
|---|---|---|---|
| `winter-cereal` | wheat, rye, barley, triticale | `grassland` | meadow and pasture |
| `summer-cereal` | the same sown in spring, plus oats | `vegetable` | beds, worked in strips |
| `maize` | grain and silage | `orchard` | fruit, nuts, Christmas trees |
| `rapeseed` | and the mustards, which flower like it | `vineyard` | vines |
| `sugar-beet` | and fodder beet | `fallow` | set-aside, flowering strips, margins |
| `potato` | | `other` | anything with no group of its own |
| `legume` | peas, beans, lupins, soya | | |

An id no table knows is drawn as bare ground and called out by the rule check, so a typo
is visible rather than fatal.

**Nothing about the appearance is stored.** The crop plus the scenario's date gives the
growth stage, the colour, the ground cover and the row contrast; the `seed` shifts the
field's own year by up to a week either way, so two neighbouring wheat fields are never
cut on the same afternoon. That is also what makes fields free over the network: every
client works the picture out from the same three numbers.

**The crop stands.** Beyond the painted surface, each field grows **plants** — real
low-poly crop models (Quaternius, CC0 — see `THIRD_PARTY_LICENSES.md`) where the camera is
close, painted cards under and between them, and none at all past about 400 m, where the
paint carries the field alone. What a plant looks like follows the same crop, day and seed
as the paint under it: bare ground grows nothing, a young stand is short and thin, stubble
keeps the odd straw tuft, deep winter takes the crop away entirely. The plants belong to
the program, not to a mod — the same rule as the ground textures — so a mod adds nothing
and changes nothing here; a field's look follows from its data alone.

#### Importing them

Every EU member state has to publish what its farmers declared (Art. 67(3) of Regulation
(EU) 2021/2116), and the German states do it as web services. **File ▸ Import fields…**
in the route editor asks them:

- **Cover** — the whole module inside its envelope, or the selected field on its own. The
  first is what a new module wants; the second is for re-fetching one parcel after
  correcting a crop mapping.
- **Cut at the boundary** — cut the fields at the envelope, so the neighbouring module
  owns the rest. Off keeps whole every field whose middle is inside.
- **Smallest field**, **Clear of the track** — below half a hectare a parcel is a margin
  strip, and nothing should lie on the formation the ground pulls up to rail height.
- **Fetch again** — ask the services rather than reading what was fetched before.

The import runs on a thread of its own: a bar, the state being asked, and a Stop that
means it. It ends in a **summary** — so many fields, so many hectares, this many of each
crop, these warnings — and nothing is written until **Add to the module** is pressed.
That is one undo step, so Ctrl+Z takes a whole import back out. A module import replaces
what an earlier import put there and leaves hand-drawn fields alone. Headless, the same
import is `cargo run -p content --bin import-module -- --line <file.ron>` — the dialog's
defaults, the summary on stderr, the module written back (see the Börde module below).

The **field tool** (landscape category) draws one by hand: clicks set the corners, Enter
or right-click closes it, and the crop comes from the tool options. That is the way to
fill a corner the register does not cover, and the only way in a fictional module.

#### A line to look at it on

`lines/boerde.ron` in the example mod is five kilometres across the Soester Börde with 134
real parcels beside it, its roads out of OpenStreetMap and the ground of the state's DGM1
under it — `example:boerdefahrt` drives it:

```bash
cargo run -p app -- --scenario example:boerdefahrt
cargo run -p app -- --scenario example:boerdefahrt --date 2026-04-25   # the same fields in April
```

The module is deliberately plain — two buffer stops, one curve, one main signal — because
everything worth looking at is beside the track. Drive it in the last week of July, which is
what the scenario is set to, and the winter wheat is gold with half of it already cut, the
maize is at full height and dark, and the beet is closed and darker still. Drive the same
five kilometres in April and it is a different place: nothing about the appearance is
stored, so the date decides all of it.

It is also the module the three imports are demonstrated on, end to end and headless:

```bash
cargo run -p content --bin import-module -- \
    --line mods/example/lines/boerde.ron \
    --dgm cache/dgm/nrw --fetch-dgm nrw
```

That asks the register for the fields again, Overpass for the roads, downloads the DGM1
sheets NRW publishes on its open data (GeoTIFF, one per square kilometre), cuts the
corridor's terrain tiles into `heights/boerde/` and **fits the track to the ground** —
start at 92.4 m NHN, down to 85 m in the dip at km 2, up to 103.5 m at the east end, the
grades rounded to 0.1 ‰ on half-kilometre nodes over the land's trend. The route editor
does the same three imports by dialog (File ▸ Import fields, File ▸ Import roads, the
Height data (DGM) panel); the tool is the same code without the window, which is what
makes the module reproducible.

Two things it shows that are easy to get wrong when building your own. The **track has to
sit at the height the ground is** — put the rails eight metres below the plain and the
line runs down a cutting for its whole length and you see banks instead of countryside.
Where the module ships no DGM the ground is the terrain's fallback height, and the track
has to match *that*; where it ships one, as this one does since the DGM import, the fit
above is what puts the rails on the land. And the import's **clearance** decides how
close the fields come: 15 m keeps them off the formation, and on a plain brings them up
to the lineside where they belong.

#### What each state publishes

Two levels, and which one a state offers decides how much the import can do. **GSA** is
the applied-for parcel with its crop code — what a passenger perceives as one field.
**LPIS** is the field block, arable or grassland and no finer; one block can hold half a
dozen crops.

| | | |
|---|---|---|
| GSA, with the crop | NW, NI (and HB, HH), BB (and BE), SN, TH | dl-de/by-2-0 or CC BY 4.0 |
| LPIS, blocks only | BY, HE, MV, SL, ST, SH | CC BY 4.0, dl-de/zero-2-0, or unstated |
| no register — OpenStreetMap instead | RP | ODbL 1.0 |

For an LPIS state the crop is **drawn rather than guessed**: the regional cropping
statistics give the share of each crop on arable land, and the draw is seeded by the
parcel's own id. The single field is then wrong about as often as the statistics say it
should be, and the landscape is right — the correct share of wheat, in fields of the
right size, in the right places. From a train window that is the whole of the effect.

Rhineland-Palatinate publishes no InVeKoS service at all — the register entry is empty and
the application portal is behind a login. There the import falls back to **OpenStreetMap**:
`landuse=farmland` and its neighbours out of Overpass, with the crop drawn from the
statistics the same way, or read off a `crop=*` tag where a mapper wrote one down. That
gives the shape of the countryside — which piece is farmed, which is meadow, which is
vineyard — at whatever coverage the mappers have managed, which is patchy. Note that
OpenStreetMap is **ODbL**, which is share-alike: a module built on it carries that on. The
same path is what a module outside Germany gets, where there is no register at all.

The better fallback for Rhineland-Palatinate would be the state's own **ATKIS Basis-DLM**
(dl-de/by-2-0 since June 2024, object class 41001 `AX_Landwirtschaft` with arable,
grassland, vineyard and orchard on it). It comes out of a web shop as a whole-state file
rather than a service, so it needs the same "point the editor at a file" path
Schleswig-Holstein's GeoPackage needs; neither is built yet.

#### Outside Germany

The approach is **national by nature**: the registers are national, so are their schemas
and their crop code lists. A module in Austria or the Netherlands finds no state under it
and takes the same OpenStreetMap fallback — the outline from `landuse=*`, the crop from a
`crop=*` tag or the statistics, the UTM zone from the longitude rather than from a state's
convention. The dialog says so before the import runs, and the import warns again in its
summary, because ODbL is share-alike and thinner than a register.

Doing better means reading that country's own IACS publication. They exist and they have
the same shape — Austria's AMA data is on data.gv.at under CC BY, with *Feldstücke* and
*Schläge* carrying a `SNAR_BEZEICHNUNG` land-use code — so adding one is a service entry, a
crop CSV and an attribution line, the same three things a German state needs. None is built:
what is built is the mechanism, and the fallback that keeps a foreign module from being
empty in the meantime.

#### Licences

The data is free, not unconditional. Most states ask for a source note; Schleswig-Holstein
asks for nothing. The import collects which states it drew on and shows the note verbatim,
ready to copy into the module's credits, and the line records what it was built against:

```ron
field_sources: [(land: "NW", year: 2026, fetched: 1787059200)],
```

Three states have **not stated an open licence** — Mecklenburg-Vorpommern writes "UrhG",
Saxony-Anhalt says nothing, Baden-Württemberg publishes no open download. The import
fetches them and marks the module: build and look with them, and get the answer in
writing before a module that uses them is released. Bavaria's *Feldstückskarte* is
CC BY-ND — no derivatives, which is exactly what turning a polygon into a mesh is — so
the import asks its LPIS service under CC BY instead and never touches that one.

#### Correcting a crop mapping

Which bucket *Kohlrübe* belongs in is a judgement call, and the answer changes with the
region. So the mapping is a CSV, not code:

```
# cache/fields/crops/nw.csv — code, render group, and documentation after it
602,potato,Kartoffeln,HF,16.63
603,sugar-beet,Zuckerrüben,HF,16.21
```

Drop a file into `cache/fields/crops/` named after the state (`nw.csv`, `by.csv`, …) and
it is read over the built-in table the next time the dialog opens; `groups.csv` and
`arable.csv` override the weights the draws use. Only the first two columns are read —
the rest is there so a human can see what a row is about.

### Roads

The streets beside the track are roads — `roads:` on the line, one entry per
carriageway:

```ron
roads: [
    (
        name: "Gabrechten",
        points: [(lat: 51.598, lon: 8.160), (lat: 51.599, lon: 8.161)],
        width: 7.5,
        surface: Asphalt,          // or Concrete
        center_line: Dashed,       // None, Dashed, DashedUrban, Solid
        edge_lines: true,
        bridge: false,             // flies over a dip
        tags: ["highway-primary"],
    ),
],
```

The `points` are the **centre line** OSM maps a street with; the carriageway is the
`width` either side of it, draped on the terrain when the tiles are built. `width`,
`surface`, `center_line`, `edge_lines` and `bridge` carry defaults, so a hand-written
entry can be as short as `points` alone.

A road with `bridge: true` **flies**: where the ground dips below the straight line
between the way's own ends — a valley, a river, a cutting — the carriageway holds that
line instead of following the hollow, and its ends are measured on the shaped ground
(the elevation data as the tile grid samples it), so the deck meets the draped road at
the abutments and both tiles at a seam cut the same chord. A bridge way in OSM spans
abutment to abutment, which is exactly what this wants; a crossing where the elevation
data is flat shows no dip and no bridge.

**Import.** File ▸ Import roads asks Overpass for every `highway=*` way inside the
module envelope — the same extract a hand-downloaded Overpass Turbo query returns, so a
file picked with the dialog works too. The OSM class decides what the road is made of,
and the mapper's own tags win where they say more: `surface=*` over the preset's
surface, `width=*`/`lanes=*` over its width, `oneway=yes` takes the centre line out (a
divided road is two one-way carriageways, and an Autobahn reads as two of these rather
than as one striped one), and any `bridge=*` but `no` flags the way as flying. The
dialog's two checkboxes opt the many-and-thin classes in:
**field tracks** (`highway=track` — what an agricultural module is stitched with) and
**access ways** (`service`, `living_street`, `pedestrian`). Nothing is written before
the summary's Commit, and Commit is one undo step. `import-module --tracks --narrow`
runs the same query and the same filters headless (without the flags it takes the
dialog's defaults, and it replaces the road list — the module is being rebuilt).

**The presets** are the German road system's widths, and the road tool stamps them:

| Preset | Width | Markings |
| --- | --- | --- |
| Autobahn, 2/3 lanes + shoulder (asphalt or concrete) | 11 / 15 m | edge lines only — one carriageway |
| Bundes-/Landstraße außerorts | 7.5 m | dashed centre, edge lines |
| Landstraße with overtaking ban | 7.0 m | solid centre, edge lines |
| Kreisstraße | 6.5 m | dashed centre, edge lines |
| Gemeindestraße | 5.5 m | dashed centre (innerorts), edge lines |
| Anliegerstraße | 4.5 m | edge lines only |
| Spielstraße | 3.0 m | none |
| Wirtschaftsweg (asphalt) | 3.5 m | none |
| Feldweg (concrete slabs) | 3.0 m | none |
| Fußweg | 2.0 m | none |

**The road tool** (landscape category) draws one by hand for the track the import did
not carry: clicks lay the centre line, Enter or a right click paves it, the preset combo
and the width decide what it is made of, and a click on a drawn road takes it. The
panel edits width, surface and markings of the selected road afterwards.

The look is **the program's, not the module's**: two surface scans (asphalt, concrete —
ambientCG, CC0, see `THIRD_PARTY_LICENSES.md`) and the markings drawn by the shader in
real metres, per the RMS (Richtlinien für die Markierung von Straßen): 12 cm strokes,
edge lines 25 cm off the kerb, the centre dash running 6 m on and 12 m off outside
built-up areas and 3 m on and 6 m off inside them (`Dashed` vs `DashedUrban`). The
surface texture tiles 4 m × 4 m on every carriageway, so the grain keeps its shape
whatever the road's width. A module carries no road bitmaps, and two clients of a
multiplayer run agree on what a road looks like without a byte crossing the network —
roads are static module content, a pure function of the line file and the elevation
data.

### Height data (DGM)

A module can carry its own ground, so it runs without `--dgm` on the command line:

```ron
heights: [(path: "example:heights/musterbahn", zone: 32)],
```

Behind that path lies one **ESRI ASCII grid per terrain tile**
(`<mod>/heights/<line>/x<kx>_y<ky>.asc`), cut out of the state survey office's delivery
by the route editor. A federal state's DGM1 is hundreds of gigabytes; the corridor of a
20 km module at 10 m spacing is a few megabytes.

The delivery can be XYZ, ESRI ASCII Grid or **GeoTIFF** — what NRW has delivered since it
retired its XYZ service: one single-band float tile per square kilometre, placed by the
`ModelPixelScale`/`ModelTiepoint` tags, NODATA from `GDAL_NODATA`, named after its
south-west corner like every state's sheets. NRW publishes every tile on its open data,
so the headless `import-module` can fetch exactly the sheets a corridor needs
(`--fetch-dgm nrw`, dl-de/by-2-0 — the module's header carries the source note) and cut
them without the editor.

In the editor's **Height data (DGM)** panel: choose the delivery directory, the UTM zone
(32 west, 33 east of 12° E) and the grid spacing, then

- **Import whole module** — every tile of the line's corridor, or
- **Import picked tiles** — only what the **DGM tiles** tool has picked. That tool draws
  the tile grid on the map: green already has heights, blue is picked, grey has none.
  Tile by tile is how a long line gets its ground without re-cutting what is already
  there.

Tiles the delivery holds no data for are skipped, so a module at the edge of a state's
data does not ship plates of zeros. The panel shows the coverage (`n of m corridor
tiles`). **Drop reference** removes the entry from the line but leaves the files.

At runtime the module's heights come **after** any `--dgm` given on the command line:
whoever has the original delivery keeps its finer grid, everyone else gets the module's
cut-out.

### Terrain brush

The ground comes from the DGM, and a line reshapes it with **brush strokes** — round
stamps that are stored as data, not baked into a heightfield:

```ron
terrain: [
    // Raise the ground by 6 m over a 120 m radius, fading to nothing at the edge.
    (lat: 52.0012, lon: 10.0031, radius: 120.0, edit: Raise(6.0)),
    // Pull it to an absolute height instead — a level forecourt.
    (lat: 52.0020, lon: 10.0044, radius: 80.0, edit: Level(143.5)),
],
```

Strokes apply in file order, so a later one paints over an earlier one exactly as it was
drawn, and each one stays pickable, re-dialled or deleted afterwards. The DGM is never
modified: re-importing better elevation data keeps the shaping. **The track is never
moved** — strokes work the ground *before* the cutting/embankment blend, so the strip
along the rails keeps rail height whatever is stamped across it.

In the route editor the **terrain brush** (key 0) stamps one stroke per click, with
radius and height change in the tool options. **Level to rail** takes its target height
from the nearest track — that is what levelling a station forecourt or a depot means,
and it needs no elevation data in the editor. On the map every stroke draws its true
footprint (warm = raising, cold = lowering, grey = levelling), so overlaps are visible;
the selection panel re-dials radius and amount, Delete removes a stroke.

**`T` shows the world itself** (View ▸ Show terrain): the editor draws with the same code
the run does — the DGM, the strokes, the cutting/embankment at the track, the ground
textures, the ballast bed, sleepers and rails of every track type, the line's **trees and scenery
objects**, and the **signal assemblies** on their mount points. A wood, a lineside hut or
a signal mast is judged where it is set instead of only in the run. Terrain and aerial
imagery lie in the same place, so only one of them is drawn at a time; a module that
brings height data starts on its world view, one without stays on the imagery. Whichever
is shown, the status bar reads out the ground height under the cursor.

Signals stand **at stop** there: the editor runs no interlocking, so a signal shows the
lamp image its type's first matching rule gives for an untouched situation — the picture
a line shows before the first route is set. A signal without a type stays dark, and one
without a model gets the run's placeholder mast.

A stroke smaller than the terrain grid spacing (4 m at the track, up to 32 m at the edge
of the corridor) barely shows — the grid cannot resolve it. There is no smoothing brush:
smoothing needs the neighbourhood, which a stamp does not have; a large, gentle `Level`
is the same thing by hand.

### Reference markers

Markers are the editor's drawing aids: a labelled point that says *where* something
belongs while the track is drawn by hand. Nothing in the simulation reads them, and
nothing is wired up — a marker on a level crossing is a note, not a level crossing.

```ron
markers: [
    (layer: "level-crossing", label: "Dorfstraße", lat: 52.0006, lon: 10.004),
    (layer: "kilometre-mark", label: "108.2", lat: 52.0021, lon: 10.007),
],
```

The `layer` is a free name, and everything sharing it is one layer. In the route
editor's **Reference markers** panel each layer has a checkbox (hide it on the map — it
is then unpickable too), its marker count, a button that jumps to it, and one that
deletes the whole layer. Retyping the layer in the selection panel moves a single marker
into another one. Hiding is session state; the markers themselves travel with the line,
so the next session still has them.

The **marker tool** (key 9) sets one per click into the layer named in the tool options.
**File ▸ Import reference markers…** reads an Overpass JSON extract and turns the tags it
knows into markers, each in the layer of its tag: `level-crossing`, `platform`,
`station`, `signal`, `switch`, `buffer-stop`, `kilometre-mark`, `bridge`, `tunnel`,
`power-tower`, `tower`. Ways become their midpoint; the label comes from `name`, `ref` or
`railway:position`. Everything else in the extract is ignored, and a layer that turns out
to be noise is deleted in one click.

### Modules and compositions

A big line is built from **modules**, after the Zusi 3 model: a module is an ordinary
`LineSource` whose open ends carry named `boundaries` — `Buffer` nodes at which another
module may attach:

```ron
boundaries: [(name: "nach_ost", node: 1)],
```

A module also states **where it sits and how far it reaches**:

```ron
anchor: Some((lat: 52.0, lon: 10.0146, height: 100.0)),
envelope: [
    (lat: 51.982034, lon: 9.985418),
    (lat: 51.982034, lon: 10.043782),
    (lat: 52.017966, lon: 10.043782),
    (lat: 52.017966, lon: 9.985418),
],
```

A module may also say what it portrays:

```ron
year: Some(1985),
fictional: false,
```

`year` is the state of the line a driver is meant to find, `fictional` marks a module that is
invented rather than a rebuild of a real place. Nothing in the simulation reads either yet —
they are what a module says about itself. Both are optional; a module that does not care
simply leaves them out.

The `anchor` is the module's place — the point the editor opens on, and the point the
envelope is built around when the module is created. The **envelope** is that boundary
itself, the *Hüllkurve* Zusi builds modules around: a closed polygon, in file order, that
says what this module covers. **Everything the module owns has to lie inside it**, so the ground between two modules belongs to exactly one of them and
neither shapes the other's. A rectangle is rarely the right shape — a line follows a
valley, and the boundary is agreed where it makes operational sense — so the polygon takes
as many corners as it needs, and it has to be **simple**: an envelope that crosses itself has
no inside, and the rule check says so (`check-envelope-crossed`).

The track, its turnouts and its lineside equipment are bounded as well, but with ten metres of
tolerance: a boundary is exactly where a rail meets its neighbour's, so the last metre of track
sits *on* the polygon, where a strict test would refuse the one click a module transition is
made of.

Dragging a corner inwards afterwards does not delete anything — the editor's rule check
reports what the envelope no longer covers (`check-outside-envelope`), so the choice between
moving the boundary and deleting the trees stays yours.

An **empty `envelope` bounds nothing**, which is what every line written before envelopes
reads as, and what a composition of several modules produces. A module that has none gets
one from *Module → Envelope → Reset*.

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

The route editor's *Module* panel places boundaries (select a track, add one at an open
end), holds the anchor and the envelope, and loads the neighbour module as a grey **ghost**: its track is drawn read-only and
its boundary circles are snap targets, so drawing clicks near them land exactly on the
agreed coordinates.

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

**A scenario says what stands on the line.** Its `consists:` list is the same one an
operating day has (see *Operating days* below), and its order is the order the train
indices run in: `player_train` picks which of them the player drives, and every event's
`train:` addresses the same list. `mods/example/scenarios/rangierfahrt.ron` is a complete
one — a light engine standing in the siding and three machines to collect from the platform
— and shows both spawn point shapes:

```ron
consists: [
    (number: "Lokzug 77401", vehicles: [(vehicle: "example:br101_afb", count: 3)],
     at: At(edge: EdgeId(0), s: 3800.0, dir: 1)),
    (number: "Lz 77400", vehicles: [(vehicle: "example:br101_afb", count: 1)],
     at: Yard("Abstellgleis 1")),
],
player_train: 1,
```

Leave `consists:` out and the old shape still holds: the run builds the player's train from
the vehicle picked in the menu, and the scenario brings no traffic of its own.

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

### Operating days (`days/*.ron`)

A scenario is one prepared run. An **operating day** is the other way a line is used: the
whole timetable of a day, looping every 24 hours, out of which the player picks one
service and drives it. `mods/example/days/beispieltag.ron` is a complete one; start it
without the menu with

```
cargo run -p app -- --line example:beispielstrecke --day example:beispieltag --service 0
```

The file names the line it belongs to, the date the day plays on by default, and its
services:

```ron
(
    name: "Beispieltag",
    line: Some("example:beispielstrecke"),
    date: (year: 2026, month: 8, day: 15),
    utc_offset: 2.0,
    weather: Dynamic,                       // or Fixed(Rain)
    services: [
        (
            number: "RB 30001",
            category: "RB",
            description: "Frühzug nach Beispielstadt",
            vehicle: Some("example:br101_afb"),   // None = what the player picked
            cars: 3,
            origin: At(edge: EdgeId(0), s: 200.0, dir: 1),   // or Yard("Portal Ost")
            stable_at: Some("Abstellgleis 1"),    // where its stock goes afterwards
            stable_way: DrawUp,                   // or SetBack, the default
            playable: true,                       // false = AI only, for the traffic
            stops: [ /* ScheduledStop, times in seconds since local midnight */ ],
        ),
    ],
)
```

A service's `stops` are the same `ScheduledStop`s a `timetable/*.ron` holds, and they are
read as a `Daily` timetable: seconds since local midnight, wrapping at 24 h. A service that
leaves at 23:50 and arrives at 00:12 needs nothing said about days — write 85 800 and 720.

`origin` is a **spawn point**, and it has two shapes:

```ron
origin: At(edge: EdgeId(0), s: 200.0, dir: 1),   // a place on the line
origin: Yard("Portal Ost"),                      // a road of it, by name
```

`At` is where the head of the train stands and which way it faces (`dir: 1` = towards
rising `s`); that is also what decides which way round the consist is put on the track.
`Yard` names one of the line's `yards:` — a stabling road, or a **portal**. A service
whose stock comes out of a portal is a working that started on a piece of railway nobody
has built: it appears there, runs in, and is on the line like any other train.

**The other services run too.** As each one's hour comes the simulator puts its train on
the line and the AI drives it; when it is over the unit is put away and the next service
that needs the same stock takes it rather than a new one. Over a looping day that keeps the
train count at the size of the busiest minute. Which services are out is a pure function of
the clock, so a dedicated server and every client put the same trains on the line without
sending anything about it; only the driving is the server's.

`stable_at` names where a working leaves its stock — one of the line's `yards:` (see
*Shunting*). A **stabling road** holds the unit in its siding, standing on the track with
its brakes applied where everyone can see it; a **portal** is the edge of the module and
swallows it altogether, which is how a working that carries on over unbuilt railway leaves
the scene. Leave the field out and the unit simply goes out of service where it terminated,
which is right for a plan whose workings are paired — the unit that arrives at ten past
forms the one that leaves at half past.

The unit is **driven** there: a service with a road is given a shunt move on top of its
timetable, worked once the last stop has been made, and `stable_way` says which way it runs
(`SetBack`, the default, or `DrawUp` for a road that lies ahead of where the working ends).
Its window stays open ten minutes instead of three to give it time. Whatever the driver
managed, the unit is **placed** on the road when that window closes — that placement is the
backstop, and it is what keeps the dispatching a function of the clock, because a driven
move is not one and a client and the server would otherwise end up with different trains on
the line. Getting `stable_way` wrong therefore costs the look of the move, not the plan.

**Stock that is simply there.** Both a day and a scenario may declare `consists:` — trains
that stand on the line from the first minute, whatever the plan does later:

```ron
consists: [
    (
        number: "Wagengruppe 4",
        vehicles: [(vehicle: "example:br101_afb", count: 1)],
        at: At(edge: EdgeId(1), s: 550.0, dir: -1),
        prepared: true,                  // false = a cold engine to wake up
        timetable: Some("example:probefahrt"),   // None = it just stands there
    ),
],
```

A consist names its vehicles one by one, head first — "a locomotive and n coaches" is only
the commonest case of it, and a rake of vans behind a shunter is a train too. `at:` is the
same spawn point as a service's `origin`. With a `timetable` the AI drives it to that
timetable; without one it stands where it was put with its brakes applied, which is what
stock in a siding does all day.

A consist may also carry a **shunt job of its own**, with or without a timetable:

```ron
shunt: Some((
    name: "62701 ins Bw",
    moves: [DrawUp(Yard("Portal Ost")), Stand],
)),
```

The moves are worked in order: `DrawUp(target)` and `SetBack(target)` drive one end of the
train onto a place on the line or a road by name, `Couple` couples to whatever the train
stands up against, `Uncouple(n)` parts it behind vehicle `n`, and `Stand` finishes at a
stand. With no timetable beside it the consist is a pilot and nothing else — a movement
that exists to move stock about. With one, the job is worked after the last stop has been
made, which is what a service that puts its own unit away comes to.

### Shunting: signals and routes

German practice draws a hard line between a **Zugfahrt** and a **Rangierfahrt**, and
almost everything about how a movement is signalled hangs off which side of it the
movement is on (Ril 408 / 301):

|  | Zugfahrt | Rangierfahrt |
|---|---|---|
| Signalled by | the main signals (Hp 1 / Hp 2) | **Sh 1** and nothing else |
| Track ahead | proved clear before the signal clears | may be **occupied** |
| Speed | the line speed | 25 km/h, on sight |
| 2000 Hz magnet | live at a signal at stop | switched off by Sh 1 |

**A movement changes kind by passing a signal.** Under Sh 1 it becomes a shunting
movement; under a main proceed aspect it becomes a train. That is exactly how a shunt
draws up to the starting signal, is given a train route, and leaves as a train — nothing
switches a mode, the signal does it.

A **Sperrsignal** is a signal of kind `Shunting`. It shows Sh 0 / Sh 1 and no main aspect
at all, and Sh 0 is "Halt! Fahrverbot" — it stops a train movement as dead as a shunting
one. `mods/example/signals/sperrsignal.ron` is the signal type; the aspect rules read
`shunting: Some(true)`, which the interlocking sets while a shunting route is locked there:

```ron
(kind: Signal, edge: 2, s: 20.0, facing: Backward, payload: "(signal:Some(2))"),
…
(kind: Shunting, system: Ks, device: 6, requires_route: true, signal_type: Some("example:sperrsignal")),
```

A **Rangierstraße** is a route with `kind: Shunt`. It locks the points and clears Sh 1 at
its entry signal, it leaves the main signal at stop, and — unlike a Zugstraße — it may be
set into an occupied track, which is what collecting a rake that is standing there needs.
It has no overlap: a shunting movement is driven on sight, so there is nothing to run past.

```ron
routes: [
    (entry: 2, exit: 0, kind: Shunt, switches: [(1, Diverging)], sections: [0]),
],
```

The route **belongs to the movement that ran over it**: passing the signal under Sh 1
makes it that movement's, and it is given back when *that* train has cleared it. A second
shunt running past the same signal takes nothing away from the first, and a route over a
road that was occupied to begin with is still released — the track clear detection records
which trains are on a section, not merely that something is. A route set for a movement that
never came is given back after five minutes, the Zeitverschluss of a Rangierfahrstraße. The
route editor writes the kind in the route form (*Kind*: Train route / Shunting route).

**The signalman answers by himself.** A movement standing in front of a Sperrsignal is
given the first free shunting route out of it, which is how a unit gets out of a siding
without anybody scripting it. Train routes are not set this way: which route a train takes
is a decision, not a reflex.

**The player may change trains.** In a timetable run the driver can walk out of the door —
the AI takes the working on from the stop that is actually next — walk along the platform
into another train, and take that one over at its desk (`Tab`, or whatever it is rebound
to). What they take is what they are scored against from then on. A train with nothing in it
that pulls, one that is out of service, or one somebody else is driving is refused with a
reason on screen. A **scenario** is not part of this: its events, its scoring and its ending
all name one train, so that is the one it is driven in.

**The player sets the date and the weather.** Picking a service in the run list opens one
more step: the day it runs on — the plan's `date` to begin with, dialled with ← / → — and
the weather, either *dynamic* (the day makes its own out of the date and the plan's name:
fronts move in, the rain stops and the sun comes back, all as a function of the clock, so
a whole 24-hour day has weather in it rather than one weather) or *set by hand*, where the
player names one of the thirteen presets and it is placed at the start and held. A mod
picks the default with `weather: Dynamic` or `weather: Fixed(Snow)`; a scenario is not
asked, because it brings its own sky.

A day whose `line` names a line other than the one picked is not offered — its stops are
indices into that line's track graph. Leave `line` out and it is offered everywhere, which
is what the built-in Musterbahn day does.

#### Seasons

The same date paints the **season**, and no mod has to do anything for it: the generated
ground textures and the built-in placeholder trees turn through October and go under snow
from November to March. On top of that a mod may ship **seasonal variants of its objects**
(`autumn_model` / `winter_model`, see *Track objects*) — each with its own textures inside
the glTF, each optional. A tree mod that ships only a summer birch is complete; one that
adds a winter birch gets it spawned while the snow lies. The season is fixed at load, so a
run that drives from October into November keeps the world it started in, and the route
editor always builds in summer — which season a run shows is the scenario's date, not the
module's.

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

## People

A mod may ship **characters** (`characters/*.ron`): the models the walker wears and the
passengers are made of. The shipped `people` mod carries twenty-four of them, generated
out of MakeHuman 2 by `tools/characters/` (its README explains the pipeline and how to
add faces of your own); a mod of yours adds people the same way:

```ron
(
    name: "Lena",                        // display name — content, not translated
    model: "people/assets/f01_lena.glb", // glTF below mods/, like every other model
    gender: Female,                      // Female | Male | Unspecified
    roles: [Player, Passenger],          // what the app may pick it for; default both
    height: 1.68,                        // m, informational
    tags: ["young", "casual"],
)
```

The file is all the app needs; everything else is convention in the model, so a
character needs no per-file tables:

- **Origin and axes:** metres, Y up, the origin on the ground between the feet, the face
  towards −Z — the frame the walker and the vehicles use.
- **Levels of detail:** the skinned mesh as nodes `char_LOD0` … `char_LOD3` (the `_LOD<n>`
  convention every model shares), finest first, all on one skeleton so a level switches
  without a pose change. The generated people carry about 30 000 / 6 000 / 1 600 / 500
  triangles; the app hands over at 30, 80 and 200 m and culls a person at 500 m (300 m
  for somebody aboard a train).
- **Clips,** in families told apart by name, as many of each as the model likes:
  `idle`, `idle2`, `idle3`, … (looping stands with a little life in them — a standing
  person plays one of them, drawn by its seed), `sit`, `sit2`, … (feet on the floor, the
  seat about 0.45 m up; a passenger plays one), `walk_<cm/s>` (one gait cycle in place,
  named after the pace it was recorded at — `walk_120` covers 1.2 m/s at its own speed;
  a plain `walk` counts as 1.5 m/s — a walking person plays the one nearest its pace,
  sped up or down to cover the ground, so a stroll and a hurry are different clips
  rather than one clip at two speeds) and `stand`, `stand2`, … (held frames, what a
  model without idles falls back to). Whatever is missing is not used — the app falls
  back to the nearest family it finds, and a character with no clips stands in its rest
  pose. The shipped people carry four walks, four idles and three seated clips each,
  retargeted from motion capture recordings (`tools/characters/README.md`) that are
  licensed CC BY — a mod that copies one of these files takes the attribution in
  `THIRD_PARTY_LICENSES.md` with it.
- **Textures:** one opaque atlas for skin and clothes, one cut-out atlas (alpha mask) for
  hair, brows, lashes and eyes; the app builds the mip chain the first time a character
  is shown.

**Where people appear.** Passengers are placed by the app, not by the line file — they
are cosmetic and derived, so nothing about them is stored or sent:

- **On platforms:** every `Platform` device gets a waiting crowd along its length, one
  person per six metres or so (at least one, at most sixty), 2.3–2.9 m beside the track
  on the platform's side, standing on the platform's height (or on the ground where none
  is given), facing the track give or take — a few look along the platform — in a mix
  of the model's idle clips with staggered starts so no two move in step. About a
  third of them walk instead, up and down a 1.2 m wide lane 3.8 m from the track, a
  shoulder's width behind the crowd. Which person stands where is decided by a hash of the line's name and the
  device's index, so every client of a multiplayer run — and every restart — shows the
  same crowd.
- **In seats:** a vehicle whose model block lists `seats` (see *Model (glTF)*) has about
  two thirds of them taken, decided by a hash of the train, the vehicle and the seat, with
  the `sit` clip. A vehicle that lists none carries nobody.
- **On walkways:** a line may draw **footpaths** and **walk areas** (see *Walkways* under
  *Lines and scenarios*), and a model may carry them (see *Track objects*): people walk a
  footpath round and round without stopping, and wander a walk area between spots
  inside it while the rest of its people stand about. Nobody walks through anybody: on
  a footpath everyone keeps to the right (0.35 m and more from the middle, so two
  meeting pass a shoulder's width apart), walks at the one pace the way has (so nobody
  overtakes) and turns round at each end in a half circle from one lane onto the other
  — an oval nobody on it ever has to cross; a wanderer's spots and ways keep clear of
  the people standing in the area and of the spots the other wanderers stop at — two
  wanderers may still cross while both walk.
- **The walker:** see *The character* above. What another player's walker looks like is
  not sent over the wire yet; remote walkers are not drawn at all.

Where a person is at any moment is a **pure function of the line, a seed and the scenario
clock** — nothing about the crowd is stored or sent, every client computes the same
people in the same places, and the crowd stands still while the run is paused. People
stream with the terrain tiles like trees and scenery objects, so a long line with many
stations costs nothing where nobody is looking.

The glTF files of the `people` mod are **Git LFS** objects, like every binary asset below
`mods/` (`.gitattributes` lists the extensions): a checkout needs `git lfs install` once,
otherwise the files are pointers — the mod manager then warns `… is a Git LFS pointer`
for each model instead of loading nothing.

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

Planned: `.crails` = a zip of the mod directory, unpacked to `<game>/mods/` by a mod manager.
For now, copy the directory into `mods/` by hand.

## Licensing your mod

Your mod is yours. RON data, assets and Lua scripts that use the documented interfaces are not
derivative works of the game — see the mod exception at the top of [LICENSE](LICENSE). Ship them
closed-source, sell them, pick any licence you like; no obligation to publish sources.

The EUPL only applies once you distribute the game itself, a modified copy of it, or native code
linked against its crates.
