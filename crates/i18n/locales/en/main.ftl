# TrainSim-DE — English source strings.
#
# Keys ending in -hint are the tooltip of the field of the same name.
# Placeholders such as { $file } are filled in by the program; keep them.

## Windows and menus

window-simulator = TrainSim-DE
window-vehicle-editor = TrainSim-DE — Vehicle editor
window-vehicle-editor-named = { $name } — TrainSim-DE Vehicle editor
window-vehicle-editor-unsaved = • { $name } — TrainSim-DE Vehicle editor
window-route-editor = TrainSim-DE — Route editor

menu-file = File
menu-edit = Edit
menu-view = View
menu-help = Help
menu-overlay = Overlay
menu-language = Language

action-new = New
action-open = Open…
action-save = Save
action-save-as = Save as…
menu-recent = Recent
recent-missing = this file is no longer there
action-quit = Quit
action-suggest = Suggest
action-undo = Undo
action-redo = Redo

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
view-grid = Ground grid (1 m)
help-mouse = Right mouse button: rotate · Wheel: zoom
help-model-conventions = Model conventions: see MODS.md
action-about = About…
about-version = Version { $version }

status-new-vehicle = New vehicle
status-loading = { $file } loading…
status-loaded = { $file } loaded
status-written = { $file } written
status-error = { $file }: { $error }
status-nodes-read = { $count } nodes read
status-outside-mods = { $path } lies outside mods/ — copy the model into your mod first
status-unsaved = • unsaved
status-new-file = (new)
dialog-error-title = Error
confirm-comments-title = Comments will be lost
confirm-comments = { $file } contains comments. The editor rewrites the file from the data — the comments will be gone. Save anyway?
confirm-unsaved-title = Unsaved changes
confirm-unsaved = Save changes to “{ $name }”?

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
veh-adhesive = Adhesive mass share
veh-adhesive-hint = share of the mass on driven axles — loco 1.0, coach 0.0. Limits tractive and braking effort through adhesion; at 0 the vehicle transmits nothing.
veh-axle-base = Axle base sum
veh-axle-base-hint = m — sum over all bogies, basis of the curve resistance
veh-tilt = Tilt angle
veh-tilt-hint = ° — 0 conventional, ~8 tilting
veh-hunting = Hunting
veh-hunting-hint = −1 none … 0 standard … 1 strong

group-coupler = Coupler
cpl-type = Type
cpl-screw = Screw coupler
cpl-centre = Centre buffer coupler
cpl-custom = own values
cpl-slack = Slack
cpl-slack-hint = total slack between draw gear and buffers — screw coupler 0.06–0.10 m
cpl-draw = Draw gear
cpl-draw-hint = stiffness of the draw gear
cpl-buffer = Buffers
cpl-buffer-hint = stiffness of the buffers — stiffer than the draw gear
cpl-damping = Damping
cpl-breaking = Breaking force
cpl-breaking-hint = minimum breaking load — screw coupler about 1 MN

group-resistance = Running resistance
res-rolling = Rolling resistance a
res-rolling-suggest-hint = about 2 ‰ of the weight — would give { $value } N
res-speed-term = Speed term b
res-air = Air resistance
res-cw-a = cw·A
res-davis-c-hint = quadratic Davis term c
res-curve = Curve resistance
res-curve-hint = factor on Röckl — 1 = as the axle base sum gives it
res-at-100 = Resistance at 100 km/h: { $newtons } N
res-plot = Resistance (km/h → N)

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
veh-script = Script
veh-script-hint = Lua script that drives the behaviour — AFB, tap changer logic, start-up procedure
field-script-hint = <mod>:<name>

## Vehicle editor — model panel

heading-model = Model
model-none-loaded = No model loaded.
model-conventions =
    Levels of detail: node names ending in _LOD0, _LOD1, …
    Moving parts: prefixes door_, pant_, sw_, gauge_, lamp_, wheel_,
    or the Blender custom property ts_function.

group-lods = Levels of detail
action-read-node-names = Read from node names
action-read-node-names-hint = takes over { $count } levels found in the node names
action-read-node-names-same = the levels already match the node names
lod-show-hint = show in the viewport
lod-distance-hint = up to this distance this level is drawn

group-parts = Moving parts
action-take-suggestions = Take over all suggestions
action-take-suggestions-hint = binds { $count } nodes that are not bound yet
action-take-suggestions-none = every suggested node is bound already
part-function-placeholder = function
part-function-hint = What the node represents. Known forms: door_<name> · pantograph · switch:<name> · gauge:<name> · lamp:<name> · wheel — own names are allowed, the app maps the ones it knows.
part-amount-hint = full travel of the movement — from function value 0 to 1
group-nodes = Nodes in the file
node-bind-hint = bind as a moving part
part-node-missing-hint = this node is not in the model — the binding points at nothing
node-filter-hint = Filter nodes
node-count = { $total } nodes
node-count-filtered = { $shown } of { $total } nodes

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
brk-friction-points = Friction coefficient (km/h → µ)
brk-friction-plot = Friction over speed
brk-weight = Braked weight
brk-weight-hint = t — from the vehicle's anscriptions, loaded vehicle
brk-load = Load braking
brk-load-hint = how much of the braked weight is left when the vehicle runs empty
brk-load-empty = Braked weight empty
brk-load-empty-hint = share of the loaded braked weight in the empty position
brk-load-mass = Changeover mass
brk-load-mass-hint = t total mass — above it the wagon brakes in the loaded position
brk-force = Brake force
brk-force-hint = N at full cylinder pressure and standstill
brk-force-suggest-hint = from the braked weight — would give { $value } N
brk-cylinder = Cylinder pressure
brk-cylinder-hint = bar at a full application
brk-cyl-reservoir = Cylinder / reservoir
brk-cyl-reservoir-hint = volume ratio — decides how quickly the brake exhausts itself
brk-percentage = Braked weight percentage of the empty vehicle: { $percent } %

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

load-none = none
load-weighing = weighing valve (stepless)
load-changeover = empty/loaded changeover

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

drv-force-plot = Tractive effort (km/h → N)
drv-vmax = Traction v max
drv-vmax-hint = end of the tractive effort curve — above it the drive gives nothing
drv-ramp = Rise time
drv-ramp-hint = s from 0 to full effort
drv-start-force = Starting effort
drv-start-force-diesel = Starting effort
drv-start-force-diesel-hint = N — without an engine map
drv-power = Power
drv-power-hint = W at the wheel
drv-pullout = Pull-out speed
drv-pullout-hint = km/h — above it the effort falls with 1/v²; 0 = no limit
drv-brake-force = Dynamic brake force
drv-brake-force-hint = what the electric brake contributes — the air brake adds to it separately
drv-brake-power = Dynamic brake power
drv-brake-power-hint = limit of regeneration or of the braking resistors
drv-brake-fade = Brake fade-out
drv-brake-fade-hint = below this the electric brake fades out and the air brake takes over
drv-fade = Fade-out
drv-fade-hint = below this the electric brake fades out and the air brake takes over
drv-crank-time = Cranking time
drv-wheel-diameter = Wheel diameter
drv-regenerative = Regenerative
drv-regenerative-hint = feeds back into the contact line — dead without line voltage

table-tractive-effort = Tractive effort (km/h → N)
table-dynamic-brake = Dynamic brake (km/h → N)
table-torque = Full load torque (1/min → N·m)
action-add-point = + point

tap-steps = Notches
tap-steps-hint = of the tap changer
tap-step-time = Time per notch

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
eng-governor = Governor
eng-governor-hint = speed-governed: main line diesels · fill-governed: shunters and railcars with mechanical injection pumps
gov-speed = speed-governed
gov-speed-hint = the power controller sets the engine speed, the governor holds it
gov-fill = fill-governed
gov-fill-hint = the power controller is the fuel rack, the speed follows the load
gov-notches = Notches
gov-notches-hint = 0 = continuous
gov-droop = Droop
gov-droop-hint = share of the rated speed the set speed sags by at full rack — 0 = isochronous, original 0.03…0.05

trm-suggest-hint = a starting set out of starting effort, top speed, rated speed, rated torque and wheel diameter — the fit against the plot starts here
trm-fill-steps = Filling steps
trm-fill-steps-hint = 0 = continuous, 1 = fill/empty only, higher = partial filling to the original
trm-fill-time = Filling time
trm-fill-time-hint = s to fill a circuit
trm-drain-time = Emptying time
trm-drain-time-hint = s to empty a circuit; 0 = same as the filling time
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
cir-absorption-hint = N·m/(rad/s)² at ν = 0 — the pump's rated torque at rated speed
cir-absorption-slope = λ trend
cir-absorption-slope-hint = λ(ν) = λ·(1 + trend·ν) — 0 nails the engine to one speed parabola over the whole converter range
cir-shift-up = Change-up point
cir-shift-up-hint = km/h — the last circuit ignores it
cir-shift-primary = Primary influence
cir-shift-primary-hint = km/h the change point sits lower at the zero notch — 0 = the change depends on speed alone
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

## Vehicle editor — sounds
##
## The sound table of the vehicle: one entry per sound, each with a trigger,
## conditions and dependency curves. A quantity is a state value of the
## simulation the sound can follow.

group-sounds = Sounds
snd-default-table = No table of its own — the vehicle runs on the generated loops.
action-add-sound = Add sound
action-add-sound-hint = one entry: trigger, conditions, dependencies
snd-name-placeholder = Name of the entry
snd-file-placeholder = <mod>/assets/<file>.ogg or synth:<name>
snd-file-hint =
    Sample below the mods directory, or a generated source: synth:rolling,
    synth:traction, synth:air, synth:compressor, synth:horn, synth:buzzer,
    synth:joint, synth:contactor
snd-trigger = Trigger
snd-trigger-hint = what starts the sound — without one it loops and is only modulated
snd-trigger-loop = none (loop)
snd-trigger-rises = rises above
snd-trigger-falls = falls below
snd-trigger-every = every interval of
snd-quantity = Quantity
snd-threshold = Threshold
snd-interval = Interval
snd-interval-hint = fires at every multiple — 30 m of distance is a rail joint, 1 notch a contactor
snd-positional = Placed in the world
snd-positional-hint = attenuated by distance and Doppler-shifted; off means the cab hears it at a constant place
snd-conditions = Conditions
snd-conditions-hint = the sound is only heard while every quantity lies inside its window
snd-min = Lower bound
snd-max = Upper bound
action-add-condition = Add condition
snd-volume = Volume
snd-pitch = Playback speed
snd-curve-follows = follows a quantity
snd-curve-follows-hint = without a curve the sound plays at its own volume and pitch

snd-quantity-speed = Speed [km/h]
snd-quantity-distance = Distance travelled [m]
snd-quantity-engine-rpm = Engine speed [1/min]
snd-quantity-tap-changer-step = Tap changer notch
snd-quantity-circuit = Converter circuit
snd-quantity-tractive-effort = Tractive effort [kN]
snd-quantity-brake-effort = Brake force [kN]
snd-quantity-brake-pipe = Brake pipe [bar]
snd-quantity-brake-cylinder = Brake cylinder [bar]
snd-quantity-air-flow = Air flow [bar/s]
snd-quantity-slip = Slip speed [m/s]
snd-quantity-throttle = Power controller
snd-quantity-pantograph = Pantograph
snd-quantity-main-switch = Main switch
snd-quantity-compressor = Compressor
snd-quantity-doors = Doors
snd-quantity-horn = Horn
snd-quantity-alert = Train protection alert

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
