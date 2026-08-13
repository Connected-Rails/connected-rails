# TrainSim-DE — English source strings.
#
# Keys ending in -hint are the tooltip of the field of the same name.
# Placeholders such as { $file } are filled in by the program; keep them.

## Windows and menus

window-simulator = TrainSim-DE
window-vehicle-editor = TrainSim-DE — Vehicle editor
window-route-editor = TrainSim-DE — Route editor

menu-file = File
menu-view = View
menu-help = Help
menu-overlay = Overlay
menu-language = Language

action-new = New
action-open = Open…
action-save = Save
action-save-as = Save as…
action-quit = Quit
action-suggest = Suggest

filter-vehicle-ron = Vehicle (RON)
filter-line-ron = Line (RON)
filter-model-gltf = Model (glTF)

## Shared

common-on = on
common-off = off
common-none = —

## Vehicle editor — menu and status bar

action-import-model = Import model…
action-import-gltf = Import glTF…
view-reference-body = Reference body (LÜP)
help-mouse = Right mouse button: rotate · Wheel: zoom
help-model-conventions = Model conventions: see MODS.md

status-new-vehicle = New vehicle
status-loading = { $file } loading…
status-loaded = { $file } loaded
status-written = { $file } written
status-error = { $file }: { $error }
status-nodes-read = { $count } nodes read
status-outside-mods = { $path } lies outside mods/ — copy the model into your mod first
status-unsaved = • unsaved
status-new-file = (new)

## Vehicle editor — base data

heading-vehicle = Vehicle
field-name = Name
group-base-data = Base data

veh-length = Length over buffers
veh-length-hint = m — official LÜP; draw the buffers 1–2 cm compressed
veh-gauge = Gauge
veh-gauge-hint = m — checked against the infrastructure
veh-vmax = v max
veh-vmax-hint = km/h — running gear limit
veh-mass = Mass
veh-mass-hint = kg — tare mass
veh-payload = Max payload
veh-payload-hint = kg — passenger coach about 5 t

group-running-gear = Running gear
veh-rotating-mass = Rotating mass
veh-rotating-mass-hint = share of the mass — E loco 0.15–0.25, coach 0.06–0.09
veh-axles = Axles
veh-axles-hint = information for consist lists
veh-axle-base = Axle base sum
veh-axle-base-hint = m — sum over all bogies, basis of the curve resistance
veh-tilt = Tilt angle
veh-tilt-hint = ° — 0 conventional, ~8 tilting
veh-hunting = Hunting
veh-hunting-hint = −1 none … 0 standard … 1 strong

group-resistance = Running resistance
res-rolling = Rolling resistance a
res-rolling-suggest-hint = about 2 ‰ of the weight
res-speed-term = Speed term b
res-speed-term-hint = N/(m/s)
res-air = Air resistance
res-cw-a = cw·A
res-davis-c-hint = quadratic Davis term c
res-curve = Curve resistance
res-curve-hint = factor on Röckl — 1 = as the axle base sum gives it
res-at-100 = Resistance at 100 km/h: { $newtons } N

## Vehicle editor — equipment

group-equipment = Equipment
opt-not-fitted = not fitted

eq-german-protection = German train protection
eq-german-protection-hint = Sifa, Indusi/PZB and LZB as fitted to the vehicle
eq-pzb = Indusi/PZB
eq-pzb-hint = build on board — without it the vehicle runs on the LZB alone
eq-sifa = Sifa
eq-sifa-hint = driver's safety device
sifa-time-time = time-time
sifa-time-distance = time-distance
eq-lzb = LZB 80/I 80
eq-lzb-on-board = on board
eq-lzb-hint = guides only on lines with a conductor cable
eq-passenger-doors = Passenger doors
eq-passenger-doors-hint = these doors follow the door control of the train
eq-doors = Door control
eq-doors-hint = what this cab commands — the leading vehicle decides for the train

group-behaviour = Behaviour
field-script-hint = Lua script <mod>:<name>

## Vehicle editor — model panel

heading-model = Model
model-none-loaded = No model loaded.
model-conventions =
    Levels of detail: node names ending in _LOD0, _LOD1, …
    Moving parts: prefixes door_, pant_, sw_, gauge_, lamp_, wheel_,
    or the Blender custom property ts_function.

group-lods = Levels of detail
action-read-node-names = Read from node names
lod-show-hint = show in the viewport

group-parts = Moving parts
action-take-suggestions = Take over all suggestions
part-function-hint = function
group-nodes = Nodes in the file
node-bind-hint = bind as a moving part

motion-visible = visible
motion-rotate = rotate
motion-move = move

## Vehicle editor — brake

group-brake = Brake
brk-valve = Control valve
brk-valve-hint = which control valve is fitted
brk-position = Brake position
brk-position-hint = G freight · P passenger · R rapid · R+Mg with magnetic track brake
brk-friction = Friction pairing
brk-friction-hint = how the friction coefficient runs over speed
brk-weight = Braked weight
brk-weight-hint = t — from the vehicle's anscriptions
brk-force = Brake force
brk-force-hint = N at full cylinder pressure and standstill
brk-force-suggest-hint = from the braked weight
brk-cylinder = Cylinder pressure
brk-cylinder-hint = bar at a full application
brk-cyl-reservoir = Cylinder / reservoir
brk-cyl-reservoir-hint = volume ratio — decides how quickly the brake exhausts itself

group-additional-brakes = Additional brakes
label-force = Force
brk-mg = Magnetic track brake
brk-direct = Direct (additional) brake
brk-direct-cylinder-hint = bar; 0 = same as the automatic brake
brk-parking = Parking brake
brk-spring = Spring-applied (Federspeicher)
brk-spring-hint = held off by air — applies by itself when the main reservoir runs empty
brk-pilot = Pre-controlled cylinder
brk-pilot-hint = relay valve fed from the main reservoir: fills faster, cannot exhaust
brk-supplement = Air supplement brake
brk-supplement-hint = fills up whatever the dynamic brake falls short of
brk-angleicher = Equalising device (Angleicher)
brk-angleicher-hint = makes up brake pipe leakage in lap position; without a memory

group-air = Air
air-aux = Auxiliary reservoir
air-aux-hint = l
air-pipe = Brake pipe
air-pipe-hint = l — this vehicle's share
air-main = Main reservoir
air-main-hint = l — 0 = none
air-compressor = Compressor
air-compressor-hint = l/min of free air — 0 = none
air-leakage = Leakage
air-leakage-hint = l/min of free air
brk-slip = Wheel slip protection
brk-slip-hint = how the vehicle answers a spinning or sliding wheelset

friction-block = Cast iron block
friction-disc = Disc
friction-k = K block
friction-ll = LL block
friction-magnetic = Magnetic rail
friction-custom = Own characteristic

slip-none = none
slip-brake = wheel slip brake
slip-cutback = traction cutback
slip-creep = creep control

## Vehicle editor — drive

group-drive = Drive
drive-unpowered-note = Unpowered vehicle.
drv-type = Drive type
drv-type-hint = curve: effort straight off the diagram · tap changer: series-wound motors on notches · converter: three-phase drive · diesel: engine map and transmission
traction-none = unpowered
traction-curve = effort curve
traction-tap = tap changer
traction-converter = converter
traction-diesel = diesel
curve-note = Tractive effort straight off the diagram — no motor, no gearbox.

drv-vmax = v max
drv-vmax-hint = km/h
drv-ramp = Rise time
drv-ramp-hint = s from 0 to full effort
drv-start-force = Starting effort
drv-start-force-hint = N
drv-start-force-diesel = Starting effort
drv-start-force-diesel-hint = N — without an engine map
drv-power = Power
drv-power-hint = W at the wheel
drv-pullout = Pull-out speed
drv-pullout-hint = km/h — above it the effort falls with 1/v²; 0 = no limit
drv-brake-force = Brake force
drv-brake-force-hint = N
drv-brake-power = Brake power
drv-brake-power-hint = W
drv-brake-fade = Brake fade-out
drv-brake-fade-hint = km/h
drv-fade = Fade-out
drv-fade-hint = km/h
drv-crank-time = Cranking time
drv-crank-time-hint = s
drv-wheel-diameter = Wheel diameter
drv-wheel-diameter-hint = m
drv-regenerative = Regenerative
drv-regenerative-hint = feeds back into the contact line — dead without line voltage

table-tractive-effort = Tractive effort (km/h → N)
table-dynamic-brake = Dynamic brake (km/h → N)
table-torque = Full load torque (1/min → N·m)
action-add-point = + point

tap-steps = Notches
tap-steps-hint = of the tap changer
tap-step-time = Time per notch
tap-step-time-hint = s

section-series-motor = Series-wound motor data
section-rheostatic-brake = Rheostatic brake
section-engine-map = Engine map
section-transmission = Hydraulic transmission
section-retarder = Hydrodynamic brake

mot-count = Motors
mot-count-hint = number in the vehicle
mot-resistance = Resistance
mot-resistance-hint = Ω — armature and field together
mot-machine-constant = Machine constant
mot-machine-constant-hint = V·s/A — flux linkage per ampere, unsaturated
mot-saturation = Saturation current
mot-saturation-hint = A
mot-max-current = Max current
mot-max-current-hint = A — the current limit relay
mot-max-voltage = Max voltage
mot-max-voltage-hint = V at the top notch
mot-gear-ratio = Gear ratio
mot-gear-ratio-hint = motor : wheelset
mot-efficiency = Efficiency
mot-efficiency-hint = motor and gearing
mot-field-steps = Field weakening stages (1 = full field)
action-add-stage = + stage

eng-idle = Idle
eng-idle-hint = 1/min
eng-rated = Rated speed
eng-rated-hint = 1/min
eng-overspeed = Overspeed
eng-overspeed-hint = 1/min
eng-inertia = Inertia
eng-inertia-hint = kg·m² incl. flywheel
eng-rack-time = Rack travel time
eng-rack-time-hint = s from idle to full load
gov-speed = speed-governed
gov-speed-hint = the power controller sets the engine speed, the governor holds it
gov-fill = fill-governed
gov-fill-hint = the power controller is the fuel rack, the speed follows the load
gov-notches = Notches
gov-notches-hint = 0 = continuous

trm-fill-steps = Filling steps
trm-fill-steps-hint = 0 = continuous, 1 = fill/empty only, higher = partial filling to the original
trm-fill-time = Filling time
trm-fill-time-hint = s to fill or empty a circuit
trm-hysteresis = Change hysteresis
trm-hysteresis-hint = km/h below the change-up point at which it changes back
trm-final-ratio = Final drive
trm-final-ratio-hint = output : wheelset
trm-count = Transmissions
trm-count-hint = number in the vehicle
trm-efficiency = Efficiency
trm-efficiency-hint = gearing behind the circuit

group-circuits = Circuits
circuit-converter = converter
circuit-coupling = coupling
cir-ratio = Ratio
cir-ratio-hint = turbine : output
cir-stall = Stall torque ratio
cir-stall-hint = µ at ν = 0
cir-coupling-point = Coupling point
cir-coupling-point-hint = ν at which µ has reached 1
cir-absorption = Absorption λ
cir-absorption-hint = N·m/(rad/s)² — the pump's rated torque at rated speed
cir-shift-up = Change-up point
cir-shift-up-hint = km/h — the last circuit ignores it
action-add-circuit = + circuit

ret-absorption = Absorption λ
ret-absorption-hint = N·m/(rad/s)² at full filling
ret-ratio = Ratio
ret-ratio-hint = rotor : wheelset
ret-brake-force = Brake force
ret-brake-force-hint = N — mechanical limit
ret-brake-power = Brake power
ret-brake-power-hint = W — what the cooler can carry off
ret-fill-time = Filling time
ret-fill-time-hint = s

## Route editor

action-open-line = Open line…
action-load-imagery = Load imagery configuration (F5)
action-save-imagery = Save imagery configuration (F2)
overlay-toggle = On/off (O)
overlay-next-provider = Next provider (P)
overlay-offline = Offline mode (L)
overlay-clear-cache = Clear cache (C)
overlay-retry = Reset failed attempts (R)
help-pan = WASD/arrows pan · PgUp/PgDn height
help-opacity = [ ] opacity · , . zoom level · Z automatic
help-offset = Numpad 4/6/8/2 image offset, 5 reset

status-ready = Ready
status-position = { $lat }°, { $lon }°   height { $height } m
status-cache-cleared = Cache cleared
status-retry-reset = Failed attempts reset
status-saved = { $file } saved
status-save-failed = Saving failed: { $error }
status-not-readable = { $file } not readable
status-not-compiling = { $file } does not compile
status-config-unreadable = { $file } not readable ({ $error }) — default active
status-config-created = { $file } created
status-config-not-writable = { $file } not writable: { $error }

heading-line = Line
line-summary = { $name } · { $edges } edges
heading-imagery = Aerial imagery
img-provider = Provider
img-status = Status
img-opacity = Opacity
img-zoom = Zoom
img-tiles = Tiles
img-offset = Offset
img-mode = Mode
zoom-fixed = fixed
zoom-resolution = { $metres } m/px
tiles-summary = { $shown } shown, { $pending } in flight
mode-offline = offline
mode-online = online
heading-cache = Cache
cache-summary = { $hits } hits ({ $disk } from disk), { $stored } stored, { $evicted } evicted
cache-size = { $megabytes } MB in { $directory }
group-errors = Errors

## Simulator HUD

hud-speed = v = { $speed } km/h   max { $limit } km/h   distance { $distance } m   t = { $time } s
hud-brakes = BP { $pipe } bar   C { $cylinder } bar   AR { $auxiliary } bar   MR { $main } bar   Direct { $direct } bar   Air { $air } Nl
hud-traction = Power controller { $throttle }   Tractive effort { $tractive } kN   Braking effort { $braking } kN   Brake { $valve }
hud-electrics = Battery { $battery }   Pantograph { $pantograph } %   Main switch { $switch }   Contact line { $voltage } V   Spring brake { $parking }
hud-tap = Notch { $step }/{ $steps }   Motor current { $current } A   Field { $field } %   Dynamic brake { $force } kN
hud-diesel = Engine { $rpm } 1/min   Fill { $fill } %   Circuit { $circuit }   ν { $nu }   Retarder { $retarder } %
hud-dynamic = Dynamic brake { $force } kN
hud-protection = Train protection: { $action }   Supervision { $limit }   Lamps: { $lamps }
hud-pzb = { $variant }   Train category { $category }{ $note }
hud-pzb-restrictive = {"   "}restrictive
hud-pzb-selftest = {"   "}Function test: { $phase }
hud-lzb-selftest = LZB function test: { $phase } (B = acknowledge)
hud-lzb = LZB { $mode } { $block }{ $cirelke }: v permitted { $permitted }   v target { $target }   target distance { $distance } m
hud-signals = Signals: { $aspects }
hud-terrain = Terrain: { $tiles } tiles loaded (+{ $pending } in progress), { $triangles } triangles, { $megabytes } MB, view { $view } m
hud-scenario = { $number } — { $name }
hud-scenario-passed = Scenario passed
hud-scenario-failed = Scenario failed
hud-outcome = { $result }: { $reason }
hud-score = Score { $total } | Forced brake applications { $forced } | { $energy } kWh
hud-keys-drive = W/S power controller  A/D brake  E emergency  Q lap  Z fill  C/V direct brake
hud-keys-safety = Space Sifa  PgDn acknowledge  End free  Del override  N/M/B LZB  U train category  1–4 preparation  F1–F3 camera  F9 mods

## Mod manager

mods-title = Mods
mods-none = No mods installed — put a mod directory into mods/.
mods-missing-depends = requires: { $depends } (missing or switched off)
mods-content = Content: { $vehicles } vehicles, { $lines } lines, { $scenarios } scenarios, { $signals } signal types, { $scripts } scripts
mods-log = Warnings:
mods-restart = Change takes effect after a restart.
mods-keys = ↑/↓ select   Enter on/off   F9 close

## Scoring

score-summary = Score: { $total } of { $base }
score-stop-missed = Stopping point { $stop } missed by { $metres } m
score-timetable = { $stop } { $minutes } min against the timetable
score-forced-brakes = { $count } forced brake application(s)
score-overspeed = { $seconds } s too fast (max. { $excess } km/h)
score-energy = { $energy } kWh traction energy
score-scenario = Scenario score
