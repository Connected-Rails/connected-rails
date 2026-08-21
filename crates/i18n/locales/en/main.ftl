# Connected Rails — English source strings.
#
# Keys ending in -hint are the tooltip of the field of the same name.
# Placeholders such as { $file } are filled in by the program; keep them.

## Windows and menus

window-simulator = Connected Rails
window-vehicle-editor = Connected Rails — Vehicle editor
window-vehicle-editor-named = { $name } — Connected Rails Vehicle editor
window-vehicle-editor-unsaved = • { $name } — Connected Rails Vehicle editor
window-route-editor = Connected Rails — Module editor
window-route-editor-named = { $name } — Connected Rails Module editor
window-route-editor-unsaved = • { $name } — Connected Rails Module editor

menu-file = File
menu-edit = Edit
menu-view = View
menu-help = Help
menu-overlay = Imagery
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
filter-line-ron = Module (RON)
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
eq-afb = AFB fitted
eq-afb-hint = automatic driving/braking control: holds the target speed set in the cab; under LZB guidance the LZB's v-soll caps it
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
part-function-hint = What the node represents. Known forms: door_<name> · pantograph · switch:<name> · gauge:<name or indicator> · lamp:<name or indicator> · digit:<indicator>:<place> · wiper · wheel — own names are allowed, the app maps the ones it knows.
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
motion-glow = glow

## Vehicle editor — brake

group-brake = Brake
brk-valve = Control valve
brk-valve-hint = which control valve is fitted
brk-default-position = Default brake position
brk-default-position-hint = Position the changeover handle stands in when the vehicle goes into service — the train is what sets it. G freight · P passenger · R rapid; R plus a magnetic track brake is the anscribed "R + Mg"
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
brk-mg-hint = Fitted equipment. It applies in brake position R — the anscribed "R + Mg" is that pair
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
traction-steam = steam
drv-mode-electric = Electric
drv-mode-diesel = Diesel
drv-mode-steam = Steam
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
action-close = Close

# The shared curve editor: every (x, y) table opens it from its sparkline.
curve-empty = no points yet
curve-open-hint = Click opens the curve editor.
curve-editor-help = Drag points · double-click adds a point · right-click removes it

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
trm-power-control = Power control
trm-power-control-hint = What sets the part load: the filling of the circuit (Voith) or the engine speed, with the circuit simply full (Mekydro)
trm-power-control-filling = Filling
trm-power-control-engine-speed = Engine speed
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
trm-shunting-ratio = Shunting gear
trm-shunting-ratio-hint = Final drive of the shunting range of a two-range gearbox (V 60, V 90); 0 = none. Changed at a stand only
gbx-gears = Gear ratios
gbx-gears-hint = engine : gearbox output, first gear first
gbx-clutch-torque = Clutch torque
gbx-clutch-torque-hint = N·m the lining holds before it slips — what the vehicle can get away with
gbx-clutch-time = Clutch travel time
gbx-clutch-time-hint = s for the clutch over its full travel
gbx-shift-time = Change time
gbx-shift-time-hint = s of clutch out, gear, clutch in — the hole in the tractive effort
gbx-shift-up = Change-up speed
gbx-shift-down = Change-down speed
gbx-shift-up-hint = 1/min at which the driver changes up
gbx-shift-down-hint = 1/min below which he changes down again
hst-max-force = Tractive effort limit
hst-max-force-hint = N the pressure relief valve allows — the flat part of the curve
hst-response-time = Swash plate travel time
hst-response-time-hint = s over the full travel of the pump
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
    Sample below the mods directory, or a generated source: synth:rolling-low,
    synth:rolling-mid, synth:rolling-high, synth:traction-low, synth:traction-mid,
    synth:traction-high, synth:air, synth:compressor, synth:horn, synth:buzzer,
    synth:squeal, synth:joint, synth:contactor
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
snd-factors = Volume factors
snd-factors-hint = each curve is multiplied into the volume — a second quantity scaling an entry whose volume already follows a first one, like the track roughness on the rolling noise
action-add-factor = Add factor
snd-pitch = Playback speed
snd-curve-follows = follows a quantity
snd-curve-follows-hint = without a curve the sound plays at its own volume and pitch

# The preview plays one entry through the editor's own output device and lets the
# author scrub the quantities it depends on.
snd-preview = Preview
snd-preview-hint = plays this entry; the sliders below scrub the quantities it depends on
snd-preview-stop = Stop
snd-preview-no-device = no audio output device
snd-preview-level = volume { $volume } · speed { $pitch }
snd-preview-failed = Cannot play: { $error }
snd-preview-not-scrubbable = follows a cab control — only audible while driving

snd-quantity-speed = Speed [km/h]
snd-quantity-distance = Distance travelled [m]
snd-quantity-engine-rpm = Engine speed [1/min]
snd-quantity-tap-changer-step = Tap changer notch
snd-quantity-circuit = Converter circuit
snd-quantity-tractive-effort = Tractive effort [kN]
snd-quantity-brake-effort = Brake force [kN]
snd-quantity-brake-pipe = Brake pipe [bar]
snd-quantity-brake-cylinder = Brake cylinder [bar]
snd-quantity-main-reservoir = Main reservoir [bar]
snd-quantity-air-flow = Air flow [bar/s]
snd-quantity-slip = Slip speed [m/s]
snd-quantity-throttle = Power controller
snd-quantity-pantograph = Pantograph
snd-quantity-main-switch = Main switch
snd-quantity-compressor = Compressor
snd-quantity-doors = Doors
snd-quantity-horn = Horn
snd-quantity-alert = Train protection alert
snd-quantity-roughness = Track roughness
snd-quantity-rain = Rain
snd-quantity-thunder = Thunder

## Vehicle editor — cab
##
## Interactive 3D cab: each control binds a glTF node to a simulation input;
## the input decides whether it acts as a push button, a switch or a lever.
## The cab-input-* names double as control labels in the simulator HUD.

group-cab = Cab
cab-none = No cab yet — the model gets an eye point and mouse-operable controls.
action-add-cab = Add cab
action-add-cab-hint = eye point plus mouse-operable controls
action-add-control = Add control
action-add-control-hint = binds a glTF node to a simulation input
cab-eye = Eye point
cab-eye-hint = m in model space: X right, Y above the rail head, −Z ahead
cab-control-node = Node
cab-control-input = Input
cab-control-test = Test
cab-control-test-hint = moves the node in the preview; not saved

cab-input-throttle = Power controller
cab-input-reverser = Reverser
cab-input-brake-valve = Driver's brake valve
cab-input-direct-brake = Direct brake
cab-input-afb-target = AFB target speed
cab-input-sifa = Sifa
cab-input-pzb-acknowledge = PZB acknowledge
cab-input-pzb-exempt = PZB free
cab-input-pzb-override = PZB override
cab-input-lzb-takeover = LZB takeover
cab-input-lzb-end = LZB end
cab-input-lzb-test = LZB test button
cab-input-horn = Horn
cab-input-sanding = Sanding
cab-input-brake-release = Loco brake release
cab-input-engine-start = Engine starter
cab-input-door-release-left = Door release left
cab-input-door-release-right = Door release right
cab-input-door-close = Close doors
cab-input-parking-brake = Parking brake
cab-input-ep-brake = EP brake
cab-input-afb = AFB
cab-input-battery = Battery
cab-input-pantograph = Pantograph
cab-input-main-switch = Main switch
cab-input-compressor = Compressor
cab-input-road-gear = Range selector (shunting / road gear)
cab-input-train-type = Train type switch
cab-input-wipers = Wiper switch
cab-input-headlights = Headlights
cab-input-cab-light = Cab light
cab-input-instrument-light = Instrument backlighting
cab-input-display-1 = Display button 1
cab-input-display-2 = Display button 2
cab-input-display-3 = Display button 3
cab-input-display-4 = Display button 4
cab-input-display-5 = Display button 5
cab-input-display-6 = Display button 6
cab-input-display-7 = Display button 7
cab-input-display-8 = Display button 8

## Vehicle editor — displays
##
## Screens in the cab, rendered to texture: a name the script hook answers to,
## the glTF node that shows the texture, and its resolution. Content comes
## from the widget list in the file or from the vehicle script's display(ctx).

group-displays = Displays
action-add-display = Add display
action-add-display-hint = a screen rendered to texture on a glTF node — widgets or the display(ctx) script hook draw it
disp-name = Name
disp-name-hint = what the script hook is asked for (ctx.display)
disp-node = Node
disp-size = Resolution
disp-size-hint = px — width × height of the rendered texture
disp-html = HTML file
disp-html-hint = path below mods/ — the screen is drawn from this HTML/CSS/JS page instead of widgets or the script hook

## Signal editor
##
## A signal model is an assembly after the Zusi pattern: shared glTF parts
## (mast, screen, indicator) chained by mount points — empty nodes named
## mp_* — plus the binding of the signal type's lamp-image strings to nodes.

window-signal-editor = Connected Rails — Signal editor
window-signal-editor-named = { $name } — Connected Rails Signal editor
window-signal-editor-unsaved = • { $name } — Connected Rails Signal editor
heading-signal-model = Signal model
filter-signal-model-ron = Signal model (RON)
status-new-signal-model = New signal model
group-signal-parts = Parts
group-signal-lamps = Lamps
group-signal-motions = Motions
group-signal-test = Lamp test
action-add-part = Add part…
action-add-lamp = Add lamp
action-add-motion = Add motion
action-lamps-off = All off
sig-seconds-hint = travel time of the full swing [s] — 0 switches instantly
sig-mount = Mount
sig-mount-root = At the signal position
sig-mount-node = Mount point
sig-lamp = Lamp image
sig-node = Node
sig-test-empty = Bind lamps first — the test then lights them without the simulator.
help-signal-conventions = Origin at the foot, +Y up, front towards the driver = +Z · mount points are empty nodes “mp_…”

## Module editor

action-new-line = New module

# --- New module dialog -----------------------------------------------------
new-module-title = New module
new-module-name = Name
new-module-name-placeholder = e.g. Göttingen – Northeim
new-module-lat = Latitude
new-module-lon = Longitude
new-module-size = Initial size
new-module-size-hint = Edge length of the square envelope the module starts with; its corners are dragged into shape afterwards.
new-module-year = Depicted year
new-module-year-hint = The year the module portrays — the state of the line a driver is meant to find.
new-module-kind = Rebuild
new-module-kind-real = Real
new-module-kind-fictional = Fictional
new-module-search-placeholder = Search for a place, station or address
action-search = Search
new-module-searching = Searching…
new-module-no-hits = Nothing found under that name.
new-module-search-failed = Search failed: { $error }
new-module-map-hint = Click to set the anchor, drag to move the map, scroll to zoom.
action-create-module = Create module
new-module-needs-name = The module needs a name.
status-module-created = Module “{ $name }” created — the envelope is the square around the anchor.
action-open-line = Open module…
action-delete = Delete
action-load-imagery = Load imagery configuration (F5)
action-save-imagery = Save imagery configuration (F2)
overlay-toggle = On/off (O)
overlay-next-provider = Next provider (P)
overlay-offline = Offline mode (L)
overlay-clear-cache = Clear cache (C)
overlay-retry = Reset failed attempts (R)

# Viewport bar — icon buttons above the viewport, tooltips only.
view-imagery = Aerial imagery on the ground
gizmo-move = Move handles (W)
gizmo-rotate = Rotate handle (E)
camera-speed = Camera speed
camera-speed-hint = Camera speed — { $speed } m/s. Right mouse and the wheel turn the same dial.
camera-speed-scalar = Speed multiplier
camera-speed-value = { $speed } m/s

help-fly = Hold right mouse to look, WASD to fly, Q/E down/up, Shift slower · right mouse + wheel sets the camera speed · Alt+left orbits · middle mouse pans · wheel zooms · F frames the selection
help-gizmo = W move handles, E rotate handle · drag an arrow to move the selection along the track, across it or upwards
help-opacity = [ ] opacity · , . zoom level · Z automatic
help-offset = Numpad 4/6/8/2 image offset, 5 reset
help-draw = Draw track: click points · Enter finishes · Esc cancels

status-ready = Ready
status-perf = { $fps } fps · { $entities } entities · { $tiles } tiles (+{ $pending })
status-perf-hint = Frames per second, entities in the scene, terrain tiles in the scene and being built. What to watch while flying: the tile count should settle, the frame rate should not.
status-position = { $lat }°, { $lon }°   height { $height } m
status-ground-height = ground { $height } m
status-terrain-flat = No height data yet — the ground is flat; import a DGM under Height data
status-cache-cleared = Cache cleared
status-retry-reset = Failed attempts reset
status-saved = { $file } saved
status-save-failed = Saving failed: { $error }
status-not-readable = { $file } not readable
status-not-compiling = { $file } does not compile
status-compile-error = Module does not compile: { $error }
status-no-track-hit = No track near the click — devices sit on a track
status-split-at-end = Too close to the track end — click at least 1 m inside
status-split-failed = Switch not placed — the module does not compile
status-ghost-loaded = Ghost module { $file }: { $boundaries } boundaries
status-route-derived = Path found: { $sections } sections, { $overlap } in the overlap, { $switches } switches
status-routes-found = { $added } routes added, { $known } already there
status-no-route-path = No path from the entry to the exit signal — check the direction the signals act in
status-no-objects = No objects installed — a mod ships them as objects/*.ron
status-config-unreadable = { $file } not readable ({ $error }) — default active
status-config-created = { $file } created
status-config-not-writable = { $file } not writable: { $error }

heading-line = Module
line-name = Name
line-counts = Tracks: { $edges } · Devices: { $devices }

heading-tools = Tools
tool-group-track = Track
tool-group-equipment = Lineside equipment
tool-group-landscape = Landscape
tool-select = Select
tool-select-hint = Click a device or a track to inspect it · Delete removes it (1)
tool-draw = Draw track
tool-draw-hint = Clicks set points: the first starts the track, every further one appends a tangent arc · Enter or right-click finishes · Esc cancels (2)
tool-device = Place device
tool-device-hint = Click a track to put the chosen device kind on it (3)
tool-device-pick = none — pick one in the content drawer
tool-switch = Place switch
tool-area = Mark area
tool-area-hint = Paint a stroke along a track and give the stretch properties
tool-area-drag = Press on a track and drag along it. The stroke follows that track until the button goes up.
tool-area-joins = It joins { $name }.
tool-switch-hint = Click a track to split it there, then click the branch like drawing a track · Enter or right-click finishes, Esc cancels (4)
tool-object = Place object
tool-object-hint = Click a track to drop the chosen 3D object at its predefined offset and rotation (5)
tool-tree = Place tree
tool-tree-hint = Every click plants one tree of the chosen species — anywhere, free of the track (6)
tool-forest = Forest brush
tool-forest-hint = Clicks outline an area; Enter or right-click fills it with single trees — each one stays individually editable and deletable · Esc cancels (7)
tool-brush = Marking brush
tool-brush-hint = Hold the left button and sweep to mark trees and objects in bulk; Delete removes them together, Esc clears the marking (8)
tool-marker = Place marker
tool-marker-hint = Every click sets a reference marker in the named layer — a drawing aid, not equipment: nothing in the simulation reads it (9)
marker-layer = Layer
marker-layer-hint = Everything sharing this name is one layer, and layers are hidden and deleted as a whole
marker-label = Label
marker-label-hint = Free text next to the marker — a kilometre, a road name, a note to self
tool-terrain = Terrain brush
tool-terrain-hint = Every click stamps one round stroke into the elevation data — raise, lower or level. The track keeps its height: strokes work the ground, the cutting and embankment are laid over them afterwards (0)
terrain-radius = Radius
terrain-radius-hint = Reach of the stroke; it fades to nothing at the edge, so overlapping strokes blend without a crease
terrain-amount = Height change
terrain-amount-hint = Metres the centre rises (+) or drops (−)
terrain-target = Target height
terrain-target-hint = Ellipsoidal height the stroke pulls the ground to
terrain-mode = Mode
terrain-raise = Raise/lower
terrain-raise-hint = Adds the height change on top of the elevation data
terrain-level = Level to rail
terrain-level-hint = Pulls the ground to the height of the nearest rail — a station forecourt, a depot, a level yard
terrain-count = Strokes in this module: { $count }
sel-terrain-summary = Terrain stroke { $index }
tool-tile = DGM tiles
tool-tile-hint = Shows the terrain tile grid and picks single tiles by clicking — green already has heights, blue is picked. Without a pick the import covers the whole corridor
switch-orientation = Turnout
switch-facing = Facing
switch-facing-hint = The branch leaves in the running direction of the clicked track — a train coming over that track faces the fork
switch-trailing = Trailing
switch-trailing-hint = The branch runs back along the clicked track — the fork lies behind a train coming over it, and the far half of the split becomes the root
draw-active = Drawing: { $segments } segments — Enter or right-click finishes, Esc cancels
draw-branch = Branch track: { $segments } segments — Enter or right-click wires the switch, Esc cancels
forest-active = Forest area: { $corners } corners — Enter or right-click closes it, Esc cancels

heading-selection = Selection
heading-areas = Track areas
sel-none = Nothing selected — the Select tool picks devices and tracks.
sel-edge-summary = Track { $index }: { $length } m, { $segments } segments
sel-edge-devices = { $devices } devices on this track
sel-edge-handles = Drag the round handles on the map to bend the track.
sel-edge-covered = Marked areas lie over this track: { $areas }. Where they set a property, it wins.
sel-edge-fixed = Transition curves — this track's support points are not editable.
sel-track-type = Track type
sel-track-type-none = Default type over the whole track.
sel-track-type-from = From this distance along the track
sel-track-type-hint = Each row: from position s onwards this type applies — texture, roughness, superstructure speed limit come from track_types/*.ron of a mod
sel-power = Electrification
sel-power-default = Nothing said here: { $system }
sel-power-from = From this arc length onwards
sel-power-hint = A section of its own — a gap under a system boundary, or a siding under no wire
sel-area-summary = { $spans } stretches, { $length } m of track
sel-area-sets-nothing = Sets nothing yet — it is a marking and no more
sel-area-properties = Properties
sel-area-spans = Stretches
sel-area-no-spans = No stretch — mark one on the map
sel-area-span-track = Track { $index }
sel-area-span-from = From this arc length
sel-area-span-to = To this arc length
sel-area-list-empty = No marked areas
sel-area-list-covers = { $length } m
area-name = Name
area-color = Colour
area-width = Stroke width
area-speed = Permitted speed
area-cant = Cant
area-grade = Gradient
area-track-type = Track type
area-unset = not set
area-unnamed = Unnamed area
area-set-hint = Tick to let this area set the property; unticked it leaves what lies underneath alone
area-default-name = Area { $index }
action-add-area = Paint a new area
action-add-area-hint = Press on a track and drag along it
action-mark-more = Paint another stroke
action-mark-more-hint = The next stroke joins this area instead of opening a new one
status-area-too-short = Too short — drag along the track
power-none = No wire
action-add-power-section = Add electrification section
track-type-default = (Standard)
action-add-type-section = Add type section
sel-switch = Switch
sel-switch-node = Node { $node } ({ $leg }), throw time
sel-switch-hint = How long the point machine takes from one position to the other; a route holds the switch locked for that time
switch-leg-root = root
switch-leg-straight = straight leg
switch-leg-diverging = diverging leg
sel-device-summary = Device { $index } on track { $edge }
dev-kind = Device kind
dev-s = Position
dev-s-hint = Distance from the start of the track
dev-facing = Facing
dev-facing-hint = Running direction in which the device acts
dev-lateral = Lateral offset
dev-lateral-hint = Offset to the right of the track axis; signal masts commonly stand at 3.5 m
dev-payload = Payload
dev-payload-hint = Country-specific data as RON text — e.g. (frequency:Hz1000,signal:Some(0)) for a magnet, (name:"Musterstadt",length:210.0) for a platform
facing-forward = Forward
facing-backward = Backward
facing-both = Both
sel-signal = Signal table
sel-signal-hint = Only a device with an entry here is a signal to the interlocking — the entry carries kind, system, what it announces and what it guards
action-add-signal = Create signal entry
action-delete-signal = Delete signal entry
action-delete-signal-hint = The device stays; routes on this signal and links to it go with the entry
signal-label = { $index }: { $kind } (device { $device })
sig-kind = Kind
sig-kind-main = Main signal
sig-kind-distant = Distant signal
sig-kind-combined = Combination signal (Ks)
sig-kind-shunting = Shunting signal
sig-kind-track-lock = Track lock
sig-system = System
sig-system-hint = Signalling system of the screen — H/V, Ks or Hl
sig-next = Announces
sig-next-hint = The signal whose aspect this one announces; a distant signal needs it
sig-requires-route = Needs a route
sig-requires-route-hint = Stays at stop until a route is set — station signals do, block signals do not
sig-diverging-speed = Diverging speed
sig-diverging-speed-hint = Speed of the diverging route (Zs3); without it the signal shows no speed indicator
sig-type = Signal type
sig-type-hint = "<mod>:<name>" from signal_types/*.ron — the aspect then comes from that rule table
sig-model = 3D model
sig-model-hint = "<mod>:<name>" below signal_models/ — overrides the signal type's own model
sig-guarded = Guards sections
sig-routes = Routes from here
sig-routes-none = No routes start at this signal — a signal that needs one stays at stop until it has it.
sig-route-row = → { $exit } · { $sections } sections, { $switches } switches
action-edit-route = Edit
action-find-routes = Find routes
action-find-routes-hint = Runs out over the track and offers a route for every leg of every turnout ahead, each ending at the next signal on it. Routes already in the file are left as they are.
sel-object-summary = Object { $index } on track { $edge }
obj-kind = Object
obj-kind-hint = 3D object from a mod (objects/*.ron) — it brings its own default offset and rotation
obj-s = Position
obj-s-hint = Distance from the start of the track
obj-lateral = Lateral offset
obj-lateral-hint = Positive = right of the running direction of increasing position
obj-yaw = Rotation
obj-yaw-hint = About the up axis, clockwise from above; 0° = front along the track
obj-height = Height
obj-height-hint = Above the railhead — above the terrain instead while "Snap to terrain" is on
obj-snap = Snap to terrain
obj-snap-hint = Puts the object's base on the terrain surface instead of the rail plane; resolved in the app, which has the elevation data
check-object-off-edge = Object { $object } sits outside its track
check-unknown-object = Object { $object }: names an object no installed mod has
check-flank-guard = Route { $route }: flank protection names a node that is no turnout, or a signal that is gone
obj-repeat = Repeat
obj-repeat-interval = Spacing
obj-repeat-interval-hint = A copy every this many metres; catenary masts commonly stand 65 m apart
obj-repeat-until = Up to position
obj-repeat-until-hint = End of the row, measured along the track; stops at the track end either way
action-repeat-object = Repeat in a row
obj-repeat-hint = Stamps { $count } copies with this object's offset, rotation and height — each one stays individually editable
obj-repeat-empty = Nothing fits: the spacing runs past the end position or the track end.
sel-tree-summary = Tree { $index }
veg-species = Species
veg-species-hint = 3D object from a mod (objects/*.ron); the placeholder is the app's built-in tree
veg-placeholder = (Placeholder tree)
tree-yaw = Rotation
tree-yaw-hint = About the up axis, clockwise from above
tree-scale = Scale
tree-scale-hint = Uniform factor on the object's own size
forest-area = Area per tree
forest-area-hint = One baked tree per this many m² — smaller is denser
brush-radius = Brush radius
brush-radius-hint = Everything whose position falls inside the circle gets marked while sweeping
brush-marked = { $count } marked
action-delete-marked = Delete marked
action-clear-marked = Clear marking
status-forest-points = A forest needs at least three corners
status-forest-baked = { $count } trees baked — every one stays individually editable
action-import-forest = Import forest…
filter-overpass-json = Overpass extract (JSON)
status-forest-imported = { $count } trees baked from { $areas } forest areas — each one stays individually editable
status-forest-import-empty = { $file }: no landuse=forest or natural=wood ways found
action-import-markers = Import reference markers…
action-delete-layer = Delete layer
status-markers-imported = { $count } reference markers in { $layers } layers — hide or delete them by layer
status-marker-import-empty = { $file }: no taggable features for reference markers found
marker-none = No reference markers yet — the marker tool sets them, File ▸ Import reference markers brings them out of OSM
marker-total = { $count } markers in total
sel-marker-summary = Marker { $index }
action-center = Center view
action-payload-template = Insert template
action-reset = Reset
kind-signal = Signal
kind-magnet = PZB magnet
kind-line-conductor = Line conductor (LZB)
kind-balise = Balise
kind-speed-board = Speed board
kind-platform = Platform
kind-stop-board = Stop board
kind-block-marker = Block marker
kind-neutral-section = Neutral section
heading-interlock = Interlocking
il-sections = Sections
il-sections-none = No sections yet — a section is the set of tracks that count as occupied together.
il-sections-hint = Adds a section from the selected track; block markers, guarded lists and routes address it by its index
il-section-row = Section { $index }
action-add-section = Add section
action-add-track = + track
il-add-track-hint = Select a track first — a section is made of tracks.
il-routes = Routes
il-routes-none = No routes yet — without one a signal that needs a route never clears.
il-routes-need-signals = Two signal entries are needed before a route can run from one to the other.
il-routes-hint = Adds a route between the first two signals; entry, exit, sections and switches are set below
route-entry = Entry signal
route-exit = Exit signal
route-diverging = Diverging route
route-diverging-hint = The route runs over the diverging leg — the entry signal shows the slow aspect
route-sections = Sections
route-overlap = Overlap
route-overlap-length = Overlap length
route-overlap-length-hint = How far behind the exit signal the derivation walks on. The sections it reaches become the overlap; turnouts inside it are locked with the route.
overlap-by-rule = regular length
overlap-by-rule-hint = The regular overlap of the German rulebook, by the speed the route ends at: 50 m up to 30 km/h, 100 m up to 60, 200 m up to 100, 300 m above that. A diverging route counts with the entry signal's diverging speed. Switch it off to set a length of your own.
route-switches = Switches
route-flank = Flank protection
route-flank-hint = What keeps a vehicle off the path where a track joins it: a turnout laid so it leads such a movement away, or a signal held at stop while the route is set
flank-switch = W{ $node } { $position }
flank-signal = { $signal } at stop
flank-signal-hint = Held at stop while the route is set; no route can be cleared from it in the meantime
flank-add-switch = + turnout
flank-add-signal = + signal
route-switch-hint = Click to flip the required position
switch-straight = straight
switch-diverging = diverging
action-add-route = Add route
action-derive-route = Derive path
action-derive-route-hint = Follows the track from the entry to the exit signal and fills in the sections it runs through, the position every turnout on the way needs, and the overlap behind the exit signal.
action-delete-route = Delete route
heading-heights = Height data (DGM)
dgm-source = Delivery
dgm-source-hint = Directory of the state survey office's DGM the module's own tiles are cut from
dgm-zone = UTM zone
dgm-zone-hint = 32 west, 33 east of 12° E — it is part of the delivery's EPSG code
dgm-cell = Grid spacing
dgm-cell-hint = Spacing the module's tiles are written at. 10 m is well below what the terrain builds at the track and a fraction of the delivery's size; 1 m ships the original resolution
dgm-coverage = { $have } of { $total } corridor tiles have height data
dgm-picked = { $count } tiles picked
action-choose-dgm = Choose delivery…
action-import-heights-all = Import whole module
action-import-heights-picked = Import picked tiles
action-clear-picked = Clear pick
action-drop-heights = Drop reference
status-heights-need-mod = Save the module inside a mod first — height data lives next to it, in the mod
status-heights-no-source = No DGM delivery chosen yet
status-heights-imported = { $tiles } height tiles written, { $empty } skipped without data — the module carries its ground now

heading-markers = Reference markers

heading-module = Module boundaries
module-boundaries = Boundaries
boundary-none = No boundaries yet — select a track and add one at an open end.
boundary-node = node { $node }
boundary-select-edge = Select a track to add a boundary at one of its open ends.
boundary-taken = This node already carries a boundary.
boundary-needs-buffer = Boundaries sit on open ends (buffer nodes) only.
action-add-boundary-start = Boundary at track start
action-add-boundary-end = Boundary at track end
module-ghost = Neighbour module
module-ghost-hint = Another module drawn as a grey ghost. Its boundary circles are snap targets — clicks near them land exactly on the agreed coordinates.
action-load-ghost = Load ghost…
action-clear-ghost = Clear
ghost-boundaries = Boundaries as snap targets: { $count }
# --- Time of day -----------------------------------------------------------
# The sky over the module. Latitude and longitude are the module's anchor, so
# only the date and the clock are edited here.
heading-sky = Time of day
sky-date = Date
sky-date-hint = Day, month and year. The date decides how high the sun climbs and which way it rises, and it is what the simulator reads out of the scenario's start time.
sky-time = Time
sky-time-hint = Local clock. Drag the slider below to run a whole day past.
sky-zone = Time zone
sky-zone-hint = How far the local clock runs ahead of UT. Germany: 1 in winter, 2 in summer.
sky-overcast = Cloud cover
sky-overcast-hint = 0 is a clear sky, 1 a closed deck: the sun is dimmed, its shadows go, and the stars are gone.
sky-weather = Weather
sky-weather-hint = The named weather this module is shown in. Picking one writes its cover, sight and precipitation; the fields below edit them further.
sky-visibility = Visibility
sky-visibility-hint = How far a dark object stays visible against the horizon. The atmosphere carries it as a scattering term, so the haze takes the colour of the hour.
weather-custom = Custom
weather-clear = Clear
weather-cloudy = Cloudy
weather-overcast = Overcast
weather-fog = Fog
weather-drizzle = Drizzle
weather-rain = Rain
weather-storm = Storm
weather-thunderstorm = Thunderstorm
weather-sleet = Sleet
weather-snow = Snow
weather-blizzard = Blizzard
weather-hail = Hail
weather-frost = Frost
sky-scrub = Run the day past
sky-sun-at = Sun { $elevation }° above the horizon, { $azimuth }° from north
sky-moon-at = Moon { $elevation }° above the horizon, { $phase } % lit
sky-place = From the module's anchor: { $lat }°, { $lon }°

# --- Calendar --------------------------------------------------------------
# The date button of the status bar and the month it opens. Weeks run Monday
# to Sunday; cal-weekday-1 is therefore Monday, cal-weekday-7 Sunday.
cal-date = { $year }-{ $month }-{ $day }
cal-weekday-1 = Mo
cal-weekday-2 = Tu
cal-weekday-3 = We
cal-weekday-4 = Th
cal-weekday-5 = Fr
cal-weekday-6 = Sa
cal-weekday-7 = Su
cal-month-1 = January
cal-month-2 = February
cal-month-3 = March
cal-month-4 = April
cal-month-5 = May
cal-month-6 = June
cal-month-7 = July
cal-month-8 = August
cal-month-9 = September
cal-month-10 = October
cal-month-11 = November
cal-month-12 = December

heading-checks = Checks
check-ok = No findings.
check-device-off-edge = Device { $device } sits outside its track
check-magnet-payload = Device { $device }: magnet payload invalid or names a missing signal
check-blockmarker-payload = Device { $device }: block marker payload invalid or names a missing section
check-distant-no-1000hz = Signal { $signal }: no 1000 Hz magnet is linked to the distant signal
check-main-no-2000hz = Signal { $signal }: no 2000 Hz magnet is linked to the main signal
check-distant-no-next = Signal { $signal }: distant signal announces nothing (next missing)
check-signal-device = Signal { $signal }: its device is missing or not a signal device
check-boundary-invalid = Boundary { $boundary }: node is missing or not a buffer
check-unknown-track-type = Track { $edge }: names a track type no installed mod has
check-area-off-track = Marked area { $area }: a stretch lies on no track, or past its end
check-area-no-effect = Marked area { $area }: covers nothing, or sets nothing — it does not reach the module
check-area-track-type = Marked area { $area }: names a track type no installed mod defines
check-lzb-no-conductor = Track { $edge }: LZB track type, but the module places no line conductor
check-outside-envelope = Outside the envelope: trees { $trees }, terrain strokes { $terrain }, markers { $markers } — move the boundary or delete them
check-envelope-crossed = The envelope crosses itself — a boundary like that has no inside; pull the corner back
status-outside-envelope-track = Past the module envelope — a track may reach the boundary, not cross it
heading-imagery = Aerial imagery
img-enabled = Show overlay
img-provider = Provider
img-opacity = Opacity
img-zoom = Zoom
img-radius = Load radius
img-radius-hint = How far around the camera tiles are fetched. Every metre more is more tiles to download and hold on the graphics card — the count below says how many
img-offset = Offset
img-offset-hint = East/north — aerial imagery is often metres off the map
img-offline = Offline mode
img-offline-hint = Serve tiles from the cache only — nothing is fetched
zoom-fixed = fixed level
zoom-auto = automatic
zoom-current = level { $level }
tiles-summary = { $shown } shown, { $pending } in flight
heading-cache = Cache
cache-summary = { $hits } hits ({ $disk } from disk), { $stored } stored, { $evicted } evicted
cache-size = { $megabytes } MB in { $directory }
group-errors = Errors

## Simulator HUD
#
# The display over the running game. It is laid out in four zones — the run (top left),
# the systems (top right), the driving strip (bottom centre), the train protection
# (bottom left) and the look-ahead (bottom right) — plus two overlays on F5 and F6.
# Labels are short on purpose: they stand beside a figure in a narrow column, and a
# label that wraps moves the figure.

# The run, top left.
hud-timetable = Timetable
hud-platform = Pl. { $platform }
hud-late = +{ $minutes } min
hud-early = −{ $minutes } min
hud-on-time = on time
hud-free-run = Free run
hud-score = Score
hud-scenario-passed = passed
hud-scenario-failed = failed

# The systems, top right. The annunciators are the abbreviations of a driver's desk and
# are set in capitals in the interface — keep them to a few letters.
hud-systems = Systems
hud-chip-battery = Batt
hud-chip-pantograph = Panto
hud-chip-main-switch = Main
hud-chip-compressor = Comp
hud-chip-parking = Park
hud-chip-sanding = Sand
hud-chip-doors = Doors
hud-chip-lights = Lights
hud-chip-slip = Slip
hud-chip-hot = Hot
# What the drive says about itself — one label per row, whichever three apply.
hud-catenary = Contact line
hud-motor-current = Motor current
hud-notch = Notch
hud-engine = Engine
hud-fill = Fill
hud-converter = Converter
hud-generator = Generator
hud-boiler = Boiler
hud-water-glass = Water glass
hud-fire = Fire
hud-dynamic-brake = Dynamic brake

# The driving strip, bottom centre.
hud-unit-kmh = km/h
hud-permitted = max { $speed }
hud-supervised = sup { $speed }
hud-levers = Levers
hud-power = Power
hud-brake = Brake
hud-effort = Effort
hud-air-pipe = HL { $value }
hud-air-reservoir = HB { $value }
hud-air-cylinder = C { $value }
hud-reverser = Reverser
hud-afb = AFB
hud-odometer = Distance
hud-forward = Forward
hud-reverse = Reverse
hud-neutral = Neutral
hud-valve-release = Release
hud-valve-lap = Lap
hud-valve-fill = Fill
hud-valve-service = Service { $drop }
hud-valve-emergency = Emergency

# The train protection, bottom left. The lamp legends themselves (1000 Hz, 500 Hz,
# Befehl, Ü, G, Ende) are the markings of a German cab and stay as they are, the way a
# type designation does.
hud-protection = Train protection
hud-v-permitted = v permitted
hud-v-target = v target
hud-target-distance = target
hud-pzb-restrictive = Restrictive supervision
hud-self-test = Function test: { $phase }

# The look-ahead, bottom right.
hud-ahead = Ahead
hud-stop-in = Stop in { $distance }
hud-in = in { $distance }

# The banner over the driving strip, and the message column at the top.
hud-alert-emergency = EMERGENCY BRAKE
hud-alert-forced = FORCED BRAKE APPLICATION
hud-alert-cut-off = TRACTION CUT OFF
hud-alert-blocked = ROUTE NOT SET
hud-control = { $name }: { $value } %

# The key help, F5.
hud-help = Keyboard
hud-help-close = F5 closes · F6 shows the diagnostics
hud-help-annunciators = What the annunciators mean
hud-help-driving = Driving
hud-help-brakes = Brakes
hud-help-safety = Train protection
hud-help-vehicle = Vehicle
hud-help-view = View
hud-key-throttle = Power controller
hud-key-throttle-off = Power controller to zero
hud-key-reverser = Forward · neutral · reverse
hud-key-range = Range selector
hud-key-afb = AFB on and off
hud-key-afb-target = AFB target speed
hud-key-brake = Brake · release
hud-key-lap = Lap
hud-key-fill = Fill position
hud-key-emergency = Emergency brake
hud-key-direct = Direct brake
hud-key-release = Release the loco brake
hud-key-parking = Parking brake
hud-key-ep = EP brake
hud-key-sand = Sanding
hud-key-sifa = Sifa
hud-key-acknowledge = Acknowledge
hud-key-free = Free
hud-key-override = Override (Befehl)
hud-key-lzb = LZB take over · end · test
hud-key-train-type = Train category
hud-key-horn = Horn
hud-key-prepare = Battery · pantograph · main switch · compressor
hud-key-starter = Engine starter
hud-key-lamps = Headlights · cab light
hud-key-dimmer = Instrument dimmer
hud-key-wipers = Wipers
hud-key-doors = Release left · right · close
hud-key-cameras = Cab · outside · lineside · walk
hud-key-look = Look around
hud-key-zoom = Camera distance
hud-key-walk = Walk (F4)
hud-key-hud = Display: full · reduced · off
hud-key-overlays = This sheet · diagnostics
hud-key-mods = Mod manager
hud-key-pause = Pause

# The diagnostics, F6. Machine output — it may be as dense as it likes.
hud-diagnostics = Diagnostics
hud-diag-frame = Frame    { $fps } fps, { $millis } ms, { $entities } entities
hud-diag-terrain = Terrain  { $tiles } tiles (+{ $pending }), { $triangles } tri, { $megabytes } MB, view { $view } m
hud-diag-air = Air      AR { $auxiliary } bar   direct { $direct } bar   { $air } Nl used
hud-diag-axles = Axles    { $slipping }/{ $axles } slipping, worst { $worst } m/s
hud-diag-temperature = Heat     motors { $motor } °C   resistors { $resistor } °C
hud-diag-signals = Signals  { $aspects }
hud-diag-network = Network  { $state }, { $latency } ms, correction { $correction } cm
hud-network-joined = connected
hud-network-connecting = connecting …

## Main menu
#
# The navigation column on the left, then the pages it opens. A -hint key is the
# second, dimmer line under the entry of the same name.

menu-tagline = German railway simulation
menu-drive = Drive
menu-drive-hint = Module, vehicle and scenario
menu-mods = Mods
menu-mods-hint = Switch installed content on and off
menu-settings = Settings
menu-settings-hint = Picture, sound and controls
menu-quit = Quit
menu-quit-hint = Leave the simulator
# The Esc overlay over a run that is standing still.
menu-paused = Paused
menu-resume = Resume
menu-resume-hint = Carry on where the train stands
menu-title = Back to the main menu
menu-title-hint = Ends the run and takes down the world — unsaved progress in the scenario is lost.
menu-step = Step { $step } of { $total }
menu-select-line = Select module
menu-select-line-hint = Where the run takes place.
menu-select-loco = Select vehicle
menu-select-loco-hint = What is at the head of the train.
menu-select-scenario = Select scenario
menu-select-scenario-hint = A timetable and a task, or free rein.
# The built-in content the simulator falls back on when nothing is picked. The chip on
# the row says so, so the name itself no longer has to.
menu-chip-builtin = Built in
menu-chip-composition = Composition
menu-line-builtin = Example module
menu-loco-builtin = BR 101
menu-scenario-none = No scenario — free run
menu-free-run = No timetable and no scoring: the module, the vehicle, and wherever you take it.
# The key hints in the footer bar: one chip per key.
menu-hint-select = select
menu-hint-confirm = confirm
menu-hint-toggle = on/off
menu-hint-start = start run
menu-hint-change = change
menu-hint-next = next value
menu-hint-back = back
menu-hint-open = open
menu-hint-resume = resume
# The button at the foot of the detail pane — the same thing Enter does.
menu-action-next = Continue
menu-action-start = Start run
# The second line of a row — figures read off the content itself.
menu-meta-line = { $length } km · { $signals } signals
menu-meta-vehicle = { $mass } t · { $speed } km/h
# The detail pane beside the list.
menu-fact-length = Length
menu-fact-signals = Signals
menu-fact-scenery = Scenery objects
menu-fact-drive = Drive
menu-fact-brake = Brake
menu-fact-start = Start
menu-fact-timetable = Timetable
menu-fact-line = Module
menu-fact-events = Events
menu-fact-km = { $value } km
menu-fact-m = { $value } m
menu-fact-t = { $value } t
menu-fact-kmh = { $value } km/h

## Settings
#
# One page of the main menu, stored as TOML in the operating system's settings
# directory. A -hint key is the description under the setting's name.

set-input = Input
set-graphics = Graphics
set-audio = Audio
set-gameplay = Gameplay
set-stored = Kept between runs in the settings file of your user account.
set-view-distance = View distance
set-view-distance-hint = How far terrain is built and drawn — the biggest single cost.
set-shadows = Shadows
set-shadows-hint = Shadow maps of the sun.
set-bloom = Bloom
set-bloom-hint = Makes lamps and signals glow after dark.
set-mist = Ground mist
set-mist-hint = Draws fog as a volume, with the sun's shafts through it. Costs about half a millisecond a frame in foggy weather, nothing in clear.
set-aa = Anti-aliasing
set-aa-hint = Which technique smooths the edges. FXAA is one cheap pass, SMAA a sharper one, MSAA resolves the geometry itself and costs the most.
set-aa-off = Off
set-aa-fxaa = FXAA
set-aa-smaa = SMAA
set-aa-msaa = MSAA
set-aa-quality = Anti-aliasing quality
set-aa-quality-hint = How hard the chosen technique works — the sample count for MSAA, the preset for the other two.
set-aa-2x = 2 ×
set-aa-4x = 4 ×
set-aa-8x = 8 ×
set-texture-quality = Texture quality
set-texture-quality-hint = Size and filtering of the generated ground textures. Applies to the terrain already on screen.
set-shadow-quality = Shadow quality
set-shadow-quality-hint = Edge length of the sun's shadow map: 1024, 2048 or 4096 texels. Four times the texels a step, so it is the setting to lower first.
set-mist-quality = Mist quality
set-mist-quality-hint = Steps of the raymarch through the ground mist — what decides whether a light shaft is a shaft or a staircase.
set-window = Window
set-window-hint = Windowed, borderless over the whole monitor, or exclusive fullscreen.
set-window-windowed = Window
set-window-borderless = Borderless
set-window-fullscreen = Fullscreen
set-max-fps = Frame cap
set-max-fps-hint = Holds the simulator to this many frames a second. The top step is no cap at all; vertical sync is the monitor’s rate, which is a different question.
set-fps = { $value } fps
set-fps-unlimited = Unlimited
set-quality-low = Low
set-quality-medium = Medium
set-quality-high = High
# Shown in place of a quality step for something that is switched off.
set-quality-none = —
set-vsync = Vertical sync
set-vsync-hint = Caps the frame rate at the monitor's, against tearing.
set-volume = Master volume
set-volume-hint = Loudness of everything the simulator plays.
set-language = Language
set-language-hint = Applies right away, to the menu as well as to the cab.
set-language-system = System
set-hud = HUD
set-hud-hint = Full, or reduced to the instruments and the train protection — F7 walks the three steps while driving.
set-hud-full = Full
set-hud-reduced = Reduced
set-hud-off = Off
set-look-speed = Look sensitivity
set-look-speed-hint = How far the view turns while the right mouse button is held.
set-controls = Key bindings
set-controls-hint = Which key and which controller button work each lever of the desk.
set-reset = Reset to defaults
set-reset-hint = Puts every setting on this page back to how it shipped.
# Units of the values on the right of a settings row.
set-metres = { $value } m
set-percent = { $value } %
set-factor = { $value } ×

## Key bindings

# The page behind the key bindings row of the settings page: one row per action, the key
# on the left of the value column and the controller button on the right. The names are
# the levers themselves, so they read the same here as they do in the key help (F5).

ctl-title = Key bindings
ctl-caption = Enter takes the next key or controller button pressed — on a lever row, the next stick or trigger moved. Backspace takes the binding away.
# What a row waiting for its new key says in place of its bindings.
ctl-press = press a key …
# Nothing is bound to this half of the row.
ctl-unbound = —
ctl-hint-rebind = rebind
ctl-hint-clear = clear
ctl-reset = Reset all bindings
ctl-reset-hint = Puts every key and controller button back to how it shipped.

ctl-group-driving = Driving
ctl-group-brakes = Brakes
ctl-group-safety = Train protection
ctl-group-vehicle = Vehicle
ctl-group-view = View and overlays
ctl-group-walk = On foot

ctl-throttle-up = Power up
ctl-throttle-down = Power down
ctl-throttle-off = Power to zero
ctl-reverser-forward = Reverser forward
ctl-reverser-neutral = Reverser neutral
ctl-reverser-back = Reverser reverse
ctl-road-gear = Range selector
ctl-afb = Cruise control on/off
ctl-afb-down = Target speed down
ctl-afb-up = Target speed up

ctl-brake-apply = Brake valve apply
ctl-brake-release = Brake valve release
ctl-brake-lap = Brake valve lap
ctl-brake-fill = Brake valve fill
ctl-brake-emergency = Emergency brake
ctl-direct-brake-apply = Direct brake apply
ctl-direct-brake-release = Direct brake release
ctl-loco-brake-release = Release the loco brake
ctl-parking-brake = Parking brake
ctl-ep-brake = Pre-controlled brake
ctl-sanding = Sanding

ctl-sifa = Sifa acknowledge
ctl-pzb-acknowledge = PZB acknowledge
ctl-pzb-free = PZB free
ctl-pzb-override = PZB override
ctl-lzb-takeover = LZB takeover
ctl-lzb-end = LZB end
ctl-lzb-test = LZB test
ctl-train-type = Train type switch
ctl-horn = Horn

ctl-battery = Battery
ctl-pantograph = Pantograph
ctl-main-switch = Main switch
ctl-compressor = Compressor
ctl-engine-start = Engine starter
ctl-headlights = Headlights
ctl-cab-light = Cab light
ctl-instrument-light-up = Instrument light brighter
ctl-instrument-light-down = Instrument light dimmer
ctl-wipers = Wipers
ctl-door-left = Release doors left
ctl-door-right = Release doors right
ctl-door-close = Close doors

ctl-view-cab = Cab view
ctl-view-outside = Outside view
ctl-view-wayside = Lineside view
ctl-view-walk = Stand up and walk
ctl-look-left = Look left
ctl-look-right = Look right
ctl-look-up = Look up
ctl-look-down = Look down
ctl-zoom-in = Move camera closer
ctl-zoom-out = Move camera away
ctl-help-overlay = Key help
ctl-diagnostics = Diagnostics
ctl-hud-mode = HUD step
ctl-mod-manager = Mod manager
ctl-pause = Pause menu

ctl-walk-forward = Walk forward
ctl-walk-back = Walk back
ctl-walk-left = Walk left
ctl-walk-right = Walk right
ctl-walk-run = Run
ctl-walk-door = Through the door

# The levers that have a position rather than a direction: only a stick or a trigger can
# hold one, so their rows take an axis and their key column stays empty.
ctl-group-levers = Levers on an axis
ctl-lever-hint = Bind a stick or a trigger and it holds this lever where you put it; the keys for it stop being read.
ctl-lever-throttle = Power controller
ctl-lever-brake-valve = Driver's brake valve
ctl-lever-direct-brake = Direct brake
# What a lever row waiting for its axis says in place of its binding.
ctl-move = move an axis …

## Mod manager

mods-title = Mods
mods-none = No mods installed — put a mod directory into mods/.
mods-missing-depends = requires: { $depends } (missing or switched off)
mods-content = Content: { $vehicles } vehicles, { $lines } modules, { $compositions } compositions, { $scenarios } scenarios, { $timetables } timetables, { $signals } signal types, { $scripts } scripts
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

## Vehicle editor: block diagram — views and canvas

view-model = 3D model
view-blocks = Block diagram
graph-palette = Blocks
graph-search = Search blocks…
graph-inspector = Properties
graph-issues = Findings
graph-no-selection = Select a block on the canvas to edit its values.
graph-no-params = This block has no values of its own.
graph-add-block = Add block
graph-remove-block = Remove block
graph-domain-mismatch = Only ports of the same colour can be connected.
graph-circuit-add = Add circuit
graph-circuit-remove = Remove circuit

## Vehicle editor: block diagram — port domains (wire colours)

domain-mech = Shaft (torque)
domain-force = Force
domain-elec = Electrical
domain-air = Compressed air
domain-signal = Control signal
domain-fuel = Fuel
domain-steam = Steam
domain-water = Feed water
domain-heat = Heat

## Vehicle editor: block diagram — pin labels

port-shaft = Shaft
port-elec = Electrical
port-air = Air
port-brake-pipe = Brake pipe
port-force = Force
port-ctrl = Control
port-throttle = Throttle
port-brake-demand = Brake valve
port-direct = Direct brake
port-sanding = Sanding
port-slip = Slip
port-pilot = Pilot
port-supply = Supply
port-aux = Auxiliary reservoir
port-fuel = Fuel
port-steam = Steam
port-water = Water
port-heat = Heat
port-value = Value
port-value-a = A
port-value-b = B
port-value-actual = Actual value
port-value-target = Set point
port-value-control = Control value
port-excitation = Excitation
port-inlet-a = Inlet A
port-inlet-b = Inlet B
port-axles = Axles
port-body = Body
port-regulator = Regulator
port-cutoff = Cutoff

## Vehicle editor: block diagram — palette categories

blkcat-energy = Energy
blkcat-drivetrain = Drivetrain
blkcat-electric = Electrics
blkcat-brake = Brake
blkcat-running-gear = Running gear
blkcat-control = Control
blkcat-equipment = Equipment
blkcat-steam = Steam
blkcat-logic = Logic

## Vehicle editor: block diagram — block names and tooltips

blk-battery = Battery
blk-battery-hint = Vehicle battery: control power for pantograph, engine start and lighting
blk-fuel-tank = Fuel tank
blk-fuel-tank-hint = Diesel supply of the engine
blk-pantograph = Pantograph
blk-pantograph-hint = Collects power from the contact line (15 kV 16.7 Hz)
blk-diesel-engine = Diesel engine
blk-diesel-engine-hint = Prime mover: torque map, governor and cranking — the head of every diesel chain
blk-hydro-transmission = Hydraulic transmission
blk-hydro-transmission-hint = Torque converters and couplings, engaged by filling and emptying — the Voith principle
blk-mechanical-gearbox = Mechanical gearbox
blk-mechanical-gearbox-hint = Friction clutch and gears — no torque conversion, so getting away is the clutch slipping and the engine can be stalled
blk-hydrostatic-drive = Hydrostatic drive
blk-hydrostatic-drive-hint = Variable-displacement pump and hydraulic motor: stepless, nothing to change, the relief valve caps the effort
blk-retarder = Hydrodynamic brake
blk-retarder-hint = Retarder in the transmission: wear-free braking, strong at speed, useless at a stand
blk-generator = Main generator
blk-generator-hint = Diesel-electric chain: the engine turns it, the traction motors take its power — set the power on the diesel engine
blk-traction-motor = Traction motor
blk-traction-motor-hint = Traction motor behind converter or generator; the chain's figures sit on those blocks
blk-series-motor = Series-wound motor
blk-series-motor-hint = The classic DC motor (BR 110/140) by its machine equations: saturating flux, field weakening
blk-main-switch = Main switch
blk-main-switch-hint = Connects the vehicle to the supply; closes only with the pantograph up and line voltage present
blk-transformer = Main transformer
blk-transformer-hint = Steps the line voltage down for tap changer, converter and auxiliaries
blk-tap-changer = Tap changer
blk-tap-changer-hint = Notch by notch along the transformer winding — the control of the classic AC locomotive
blk-traction-converter = Traction converter
blk-traction-converter-hint = Three-phase drive: tractive effort along the force/power hyperbola, pull-out limit above
blk-dynamic-brake = Dynamic brake
blk-dynamic-brake-hint = Motors as generators: into the braking resistors, or back into the line when regenerative
blk-traction-curve = Tractive effort curve
blk-traction-curve-hint = Simplified drive straight off the data sheet's diagram — knows nothing of motors or gearboxes
blk-compressor = Compressor
blk-compressor-hint = Charges the main reservoir between cut-in and cut-out pressure
blk-main-reservoir = Main reservoir
blk-main-reservoir-hint = Air store of the traction unit: direct brake, relay valve and spring parking brake take from it
blk-driver-brake-valve = Driver's brake valve
blk-driver-brake-valve-hint = Sets the brake pipe pressure: release, lap, service, emergency
blk-brake-pipe = Brake pipe
blk-brake-pipe-hint = The train-long control line at 5 bar: a pressure drop applies the brake — fail-safe
blk-control-valve = Control valve
blk-control-valve-hint = Compares brake pipe and reference pressure and fills the cylinder accordingly
blk-aux-reservoir = Auxiliary reservoir
blk-aux-reservoir-hint = Per-vehicle store charged from the brake pipe; supplies the brake cylinder
blk-relay-valve = Relay valve
blk-relay-valve-hint = Pre-control: reproduces the pilot pressure with main reservoir air — fast, inexhaustible, and the path of the EP application
blk-brake-cylinder = Brake cylinder
blk-brake-cylinder-hint = Pressure becomes piston force
blk-brake-rigging = Brake rigging
blk-brake-rigging-hint = Leverage and friction pairing: cylinder force becomes retardation at the wheel
blk-direct-brake = Direct brake
blk-direct-brake-hint = Loco-only additional brake fed straight from the main reservoir
blk-parking-brake = Parking brake
blk-parking-brake-hint = Spring-applied or hand brake — holds without air
blk-mg-brake = Magnetic track brake
blk-mg-brake-hint = Presses onto the rail head, independent of wheel adhesion; applies in position R on a rapid braking
blk-wheel-slide-protection = Wheel slide protection
blk-wheel-slide-protection-hint = Watches the slip and answers with slip brake, cutback or creep control
blk-sander = Sander
blk-sander-hint = Sand before the driven wheels raises the adhesion
blk-wheelset = Wheelset
blk-wheelset-hint = Where traction and brake force meet the rail: axle count and adhesive mass
blk-cab = Cab
blk-cab-hint = The driver's controls: throttle, brake valve, direct brake, sanding
blk-afb = AFB
blk-afb-hint = Automatic driving/braking control between the throttle and the drive
blk-sifa = Sifa
blk-sifa-hint = Driver's safety device
blk-pzb = PZB
blk-pzb-hint = Indusi/PZB train protection, with the initial position of the train type switch
blk-lzb = LZB
blk-lzb-hint = Continuous train control on lines with a conductor cable
blk-doors = Door control
blk-doors-hint = Door system of the vehicle, and whether its doors follow the train's release
blk-script = Lua script
blk-script-hint = Behaviour hook of a mod: tap changer logic, AFB, start-up procedure
blk-voltage-source = Voltage source
blk-voltage-source-hint = Stands in for the contact line where there is none — a test rig, or a vehicle on a third rail
blk-rectifier = Rectifier
blk-rectifier-hint = Turns the generator's alternating current into the direct current the DC motors need
blk-load-regulator = Load regulator
blk-load-regulator-hint = Holds the generator on the power the notch asks for by adjusting its excitation — the heart of a diesel-electric drive
blk-async-motor = Induction motor
blk-async-motor-hint = Three-phase motor by Kloss's equation: constant effort, constant power and the pull-out limit come out of the machine itself
blk-rheostat = Starting resistors
blk-rheostat-hint = Cut out step by step as the train gains speed; what is left of the voltage goes into heat
blk-series-parallel-switch = Series/parallel switch
blk-series-parallel-switch-hint = Regroups the motors as the speed rises — each regrouping is a step in the tractive effort curve
blk-chopper = Chopper
blk-chopper-hint = Sets the motor voltage continuously instead of in steps, and burns nothing doing it
blk-cooling = Cooling system
blk-cooling-hint = Heat store and blower of whatever is wired to it — a bank that has run hot cannot take any more
blk-boiler = Boiler
blk-boiler-hint = Water, steam and pressure: what the fire puts in and the cylinders take out
blk-firebox = Firebox
blk-firebox-hint = Grate, damper and blower — the draught decides how hard the fire burns
blk-steam-cylinders = Cylinders
blk-steam-cylinders-hint = Regulator and cutoff into tractive effort; the expansion is why winding back pays
blk-injector = Injector
blk-injector-hint = Puts feed water into the boiler against its own pressure, and costs pressure doing it
blk-tender = Tender
blk-tender-hint = Water and coal carried along — when they run out, so does the journey
blk-angle-cock = Angle cock
blk-angle-cock-hint = Parts the brake pipe. Closed mid-train it leaves everything behind it unbraked; open at the end the train will not charge
blk-air-hose = Brake hose
blk-air-hose-hint = Couples the brake pipe to the neighbouring vehicle
blk-emergency-valve = Emergency valve
blk-emergency-valve-hint = Vents the brake pipe from the passenger compartment or the cab
blk-limiting-valve = Limiting valve
blk-limiting-valve-hint = Caps the cylinder pressure whatever asked for it — what stops a driver flatting the wheels
blk-double-check-valve = Double check valve
blk-double-check-valve-hint = Passes the higher of two pressures — how automatic, direct and EP brake share one cylinder
blk-retainer-valve = Retaining valve
blk-retainer-valve-hint = Holds a residual cylinder pressure while the train releases and recharges — set by hand, one wagon at a time
blk-ep-brake = EP brake
blk-ep-brake-hint = The application travels by wire: the whole train applies in the same moment instead of waiting for the pressure wave
blk-bogie = Bogie
blk-bogie-hint = Groups the axles it carries
blk-axle = Axle
blk-axle-hint = One axle of the running gear; drawn out, the axle count and the driven share follow from the blocks
blk-value-in = Reading
blk-value-in-hint = Takes a value out of the vehicle and into the logic
blk-constant = Constant
blk-constant-hint = A fixed number
blk-value-curve = Characteristic
blk-value-curve-hint = Piecewise linear table: one value in, another out
blk-combine = Combination
blk-combine-hint = Two values into one — sum, difference, product, smaller or larger
blk-clamp = Limiter
blk-clamp-hint = Holds a value inside a range
blk-pid = PID controller
blk-pid-hint = Controls the actual value onto the set point; this is what a cruise control is built out of
blk-notch = Notching
blk-notch-hint = Steps the output towards the input at a limited rate and lands only on its notches
blk-rate-of-change = Rate of change
blk-rate-of-change-hint = How fast the input is changing, smoothed
blk-value-switch = Switch
blk-value-switch-hint = Picks one of two values by a control value, with hysteresis so it does not chatter
blk-signal-out = Output
blk-signal-out-hint = Where the logic takes hold: power controller, brake, sanding, blower or a free value for the displays

## Vehicle editor: block diagram — new parameters

eng-map = Engine map
eng-map-hint = with the map the torque balance decides; without it the effort follows the hyperbola
eng-torque-curve = Full load torque (1/min → N·m)
eng-notches = Notches
eng-notches-hint = of the power controller; 0 = continuous
eng-droop = Droop
eng-droop-hint = share of the rated speed the set speed sags by between no load and full rack
eng-governor-speed = Speed-governed
eng-governor-fill = Fill-governed
drv-brake-curve = Dynamic brake (km/h → N)
drv-fuel-capacity = Tank capacity
cir-kind = Type
cir-kind-hint = a converter multiplies the torque, a coupling transmits it one to one
cir-kind-converter = Torque converter
cir-kind-coupling = Fluid coupling
brk-compressor-delivery = Delivery
brk-compressor-delivery-hint = l/min of free air
brk-main-volume = Volume
brk-pipe-volume = Pipe share
brk-pipe-volume-hint = this vehicle's share of the brake pipe volume
brk-leakage = Leakage
brk-leakage-hint = l/min of free air lost from the pipe
brk-aux-volume = Volume
brk-direct-cylinder = Cylinder pressure
brk-mg-force = Force
brk-mg-force-hint = N on the rail head
brk-load-none = None
brk-load-weighing = Weighing valve
brk-load-changeover = Empty/loaded changeover
brk-friction-block = Cast iron blocks
brk-friction-disc = Disc
brk-friction-composite-k = Composite K
brk-friction-composite-ll = Composite LL
brk-friction-magnetic = Magnetic
brk-friction-custom = Own curve
brk-slip-slip-brake = Wheel slip brake
brk-slip-traction-cutback = Traction cutback
brk-slip-creep-control = Creep control
eq-train-type = Train type
eq-train-type-hint = initial position of the train type switch (Zugartschalter)
eq-sifa-time-time = Time-time
eq-sifa-time-distance = Time-distance
eq-sifa-rzm = Reaction time measurement (RZM)
eq-doors-none = None
bat-voltage = Voltage
bat-capacity = Capacity
pan-system = Supply system
pan-system-third-rail = Third rail
pan-rise-time = Rise time
src-voltage = Voltage
gen-power = Electrical power
gen-power-hint = the generator delivers this at the full notch
gen-efficiency = Efficiency
gen-max-voltage = Maximum voltage
gen-max-current = Maximum current
rec-efficiency = Efficiency
reg-time = Travel time
reg-time-hint = the regulator needs this for its full range
reg-blower-idle = Blower at idle
reg-blower-idle-hint = share of the cooling that runs with the engine alone
mot-pole-pairs = Pole pairs
mot-rated-torque = Rated torque
mot-rated-torque-hint = per motor
mot-pullout-ratio = Pull-out torque
mot-pullout-ratio-hint = as a multiple of the rated torque
mot-pullout-slip = Pull-out slip
mot-rated-freq = Rated frequency
mot-rated-freq-hint = above it the converter has no voltage left and the field weakens
mot-max-freq = Maximum frequency
rhe-steps = Resistance steps
rhe-steps-hint = Ω per contactor position, strongest first, ending at 0
rhe-step-time = Time per step
spg-groups = Groupings
spg-groups-s-p = Series → parallel
spg-groups-s-sp-p = Series → series-parallel → parallel
spg-groups-s-only = Series only
spg-groups-p-only = Parallel only
chp-time = Response time
cool-capacity = Heat capacity
cool-rate = Cooling
cool-rate-hint = W per kelvin above ambient, blower running
cool-natural = Natural convection
cool-natural-hint = share of the cooling left with the blower off
cool-warn = Derating from
cool-max = Cut-out temperature
cool-ambient = Ambient temperature
stm-water-space = Water space
stm-steam-space = Steam space
stm-working-pressure = Working pressure
stm-safety-valve = Safety valves
stm-safety-valve-hint = pressure at which they lift
stm-heating-surface = Heating surface
stm-superheater = Superheater
stm-superheater-hint = dry steam through the expansion — worth about a fifth of the consumption
stm-grate-area = Grate area
stm-grate-capacity = Grate capacity
stm-burn-rate = Burning rate
stm-burn-rate-hint = per square metre of grate at full draught
stm-blower = Blower draught
stm-blower-hint = share of the draught the blower alone can make
stm-shovel = Shovelful
stm-cylinders = Cylinders
stm-bore = Bore
stm-stroke = Stroke
stm-max-cutoff = Longest cutoff
stm-back-pressure = Back pressure
stm-efficiency = Mechanical efficiency
stm-injector-rate = Delivery
stm-tender-water = Water
stm-tender-coal = Coal
brk-pump-kind = Type
brk-pump-kind-compressor = Compressor
brk-pump-kind-exhauster = Exhauster
brk-medium = Medium
brk-medium-air = Compressed air
brk-medium-vacuum = Vacuum
brk-cock-end = End
brk-cock-end-front = Front
brk-cock-end-rear = Rear
brk-limit = Limit pressure
brk-retainer = Position
brk-retainer-off = Direct exhaust
brk-retainer-slow = Slow direct release
brk-retainer-low = Low pressure retained
brk-retainer-high = High pressure retained
brk-ep-apply = Application rate
brk-ep-release = Release rate
brk-ep-vents-pipe = Vents the brake pipe
brk-ep-vents-pipe-hint = with it the pneumatic brake follows as a back-up; without it a wire failure releases the train
brk-ep-steps = Steps
brk-ep-steps-hint = of the EP application; 0 = continuous
brk-sand-rate = Sand rate
veh-wheelbase = Wheelbase
veh-axle-driven = Driven
sig-source = Reading
sig-value = Value
sig-curve = Characteristic
sig-combine = Operation
sig-combine-add = Sum
sig-combine-sub = Difference
sig-combine-mul = Product
sig-combine-min = Smaller
sig-combine-max = Larger
sig-min = Lower limit
sig-max = Upper limit
sig-kp = Proportional
sig-ki = Integral
sig-kd = Derivative
sig-steps = Notches
sig-steps-hint = 0 = continuous
sig-rate = Rate
sig-rate-hint = full range per second
sig-smoothing = Smoothing
sig-threshold = Threshold
sig-hysteresis = Hysteresis
sig-sink = Output
sig-in-throttle = Power controller
sig-in-brake = Brake demand
sig-in-direct = Direct brake
sig-in-speed = Speed (m/s)
sig-in-speed-kmh = Speed (km/h)
sig-in-target-speed = Set speed
sig-in-cylinder = Brake cylinder
sig-in-pipe = Brake pipe
sig-in-main-res = Main reservoir
sig-in-current = Motor current
sig-in-rpm = Engine speed
sig-in-temp = Temperature
sig-in-effort = Tractive effort
sig-in-reverser = Reverser
sig-in-sanding = Sanding
sig-source-throttle = Power controller
sig-source-brake = Brake demand
sig-source-direct = Direct brake
sig-source-speed = Speed (m/s)
sig-source-speed-kmh = Speed (km/h)
sig-source-target-speed = Set speed
sig-source-cylinder = Brake cylinder
sig-source-pipe = Brake pipe
sig-source-main-res = Main reservoir
sig-source-current = Motor current
sig-source-rpm = Engine speed
sig-source-temp = Temperature
sig-source-effort = Tractive effort
sig-source-reverser = Reverser
sig-source-sanding = Sanding
sig-out-throttle = Power controller
sig-out-brake = Brake demand
sig-out-sanding = Sanding
sig-out-blower = Blower
sig-out-aux = Free value
sig-sink-throttle = Power controller
sig-sink-brake = Brake demand
sig-sink-sanding = Sanding
sig-sink-blower = Blower
sig-sink-aux0 = Free value 1
sig-sink-aux1 = Free value 2
sig-sink-aux2 = Free value 3
sig-sink-aux3 = Free value 4
grp-series = Series
grp-series-parallel = Series-parallel
grp-parallel = Parallel

## Vehicle editor: block diagram — baking findings

bake-unknown-block = Unknown block type — the mod that defines it is not installed
bake-duplicate-block = This block may only appear once per vehicle
bake-bad-wire = A wire joins ports that do not fit each other
bake-unconnected = Not connected to anything
bake-missing-wire = An expected connection is missing
bake-multiple-drives = More than one drive — a vehicle takes one drive chain
bake-brake-needs-drive = A dynamic brake needs a drive to work with
bake-no-pantograph = An electric drive expects a pantograph
bake-two-drive-paths = More than one drive path on the same engine — the hydraulic transmission wins, then the gearbox
bake-gearbox-needs-map = A mechanical gearbox needs the engine map
bake-transmission-needs-map = A hydraulic transmission needs the engine map
bake-hydro-and-generator = Hydraulic transmission and generator on the same engine — the transmission wins
bake-brake-needs-generator = A diesel dynamic brake needs the generator chain
bake-series-motor-unused = A series-wound motor only works behind a tap changer
bake-no-control-valve = No control valve — the vehicle cannot brake
bake-no-brake-cylinder = No brake cylinder — the vehicle cannot brake
bake-no-brake-rigging = No brake rigging — the cylinder force reaches no wheel
bake-no-brake-pipe = No brake pipe — the train brake has no control line
bake-no-aux-reservoir = No auxiliary reservoir — the default of 100 l is used
bake-needs-main-reservoir = Needs a main reservoir as its air supply
bake-mg-needs-r = The magnetic track brake applies in position R only — the control valve has no R position
bake-no-wheelset = No wheelset — nothing carries the forces to the rail
bake-no-motor = A generator without traction motors drives nothing
bake-no-load-regulator = A diesel-electric drive without a load regulator holds no power
bake-no-boiler = No boiler — the cylinders have nothing to work with
bake-no-firebox = No firebox — nothing heats the boiler
bake-no-tender = No tender — the locomotive carries no water and no coal
bake-no-injector = No injector — the boiler cannot be fed
bake-starter-needs-motor = Starting equipment only works with motor data
bake-axle-count-mismatch = The wheelset and the single axles do not agree on the axle count
bake-no-driven-axle = A powered vehicle with no driven axle carries its traction nowhere
bake-axles-per-bogie = The axles do not divide evenly between the bogies
bake-vacuum-no-relay = A vacuum brake has no relay valve to pre-control it
bake-vacuum-needs-exhauster = A vacuum brake needs an exhauster, not a compressor
bake-signal-cycle = These logic blocks feed each other in a circle and cannot be evaluated
bake-signal-out-open = This output is not connected to anything
bake-signal-no-output = The logic blocks compute something that goes nowhere

## Vehicle editor: block diagram — comment frames and canvas shortcuts

graph-group = Comment frame
graph-group-default = Comment
graph-group-name = Title
graph-group-color = Colour
graph-group-remove = Remove comment frame

## Content drawer

tag-add = Add
tag-add-placeholder = New tag
tag-remove-hint = Removes the tag
group-tags = Tags
tags-hint = Free-form, for finding the entry again in the content drawer — lower case, words joined by hyphens. Enter adds one.
drawer-title = Content
drawer-filter-placeholder = Filter by name or key
drawer-count-filtered = { $shown } of { $total }
drawer-empty = Nothing here — no installed mod brings this kind.
drawer-source-all = All mods
drawer-system-all = All systems
drawer-tag-all = All tags
drawer-empty-filtered = Nothing matches the filter.
drawer-reset-filters = Reset filters
drawer-objects = Scenery objects
drawer-signal-types = Signal types
drawer-signal-models = Signal models
drawer-track-types = Track types
action-content-drawer = Content drawer (Ctrl+Space)

## Vehicle editor: new data panel sections and templates

group-metadata = Description
group-key-figures = Key figures
group-checks = Checks
menu-new-from-template = New from template
status-new-from-template = Started from { $name }

## Vehicle editor: braked weight and transition times per brake position

brk-weight-g = Braked weight G
brk-weight-g-hint = t in the G position — from the anscription, loaded vehicle; 0 = the same as the braked weight above
brk-weight-p = Braked weight P
brk-weight-p-hint = t in the P position — from the anscription, loaded vehicle; 0 = the same as the braked weight above
brk-weight-r = Braked weight R
brk-weight-r-hint = t in the R position — the anscribed "R + Mg" figure where a magnetic track brake is fitted; 0 = the same as the braked weight above
brk-apply-time-g = Application time G
brk-apply-time-g-hint = s the brake cylinder takes to 95 % in the G position; 0 = the UIC figure of 22 s
brk-apply-time-p = Application time P/R
brk-apply-time-p-hint = s the brake cylinder takes to 95 % in the P and R positions; 0 = the UIC figure of 4 s
brk-release-time-g = Release time G
brk-release-time-g-hint = s the brake cylinder takes to empty in the G position; 0 = the UIC figure of 50 s
brk-release-time-p = Release time P/R
brk-release-time-p-hint = s the brake cylinder takes to empty in the P and R positions; 0 = the UIC figure of 17 s

## Vehicle editor: part function registry

partfn-pantograph = Pantograph
partfn-door-left = Door, left
partfn-door-right = Door, right
partfn-wiper = Windscreen wiper
partfn-gauge-speed = Speedometer
partfn-gauge-brake-pipe = Brake pipe gauge
partfn-gauge-cylinder = Brake cylinder gauge
partfn-gauge-main-reservoir = Main reservoir gauge
partfn-gauge-tractive-effort = Tractive effort gauge
partfn-switch-throttle = Power controller position
partfn-switch-reverser = Reverser position
partfn-switch-direct-brake = Direct brake position
partfn-switch-cab-light = Cab light
partfn-switch-instrument-light = Instrument backlighting
partfn-lamp-main-switch = Main switch lamp
partfn-lamp-sanding = Sanding lamp
partfn-prefix-gauge = Pointer of an indicator
partfn-prefix-lamp = Indicator lamp
partfn-prefix-digit = Digit of a numeric display
partfn-unknown = The simulator knows no function of this name — the part stays at rest
partfn-unknown-indicator = No train protection system publishes an indicator of this name
partfn-digit-needs-place = A digit needs its decimal place: digit:indicator:place

## Vehicle editor: new from template

tpl-group-powered = Powered vehicles
tpl-group-hauled = Hauled vehicles
tpl-name = { $base } (copy)
tpl-br101-hint = Three-phase drive with a converter and a regenerative brake, disc brake in position R, PZB 90 and LZB on board — the modern main line loco.
tpl-br110-hint = Transformer with a tap changer on series-wound motors, no electric brake, block brake in position P, Indusi and Sifa only.
tpl-br218-hint = Diesel-hydraulic: speed-governed engine and two torque converters, block brake in position P, PZB 90 and Sifa.
tpl-br232-hint = Diesel-electric: engine, generator and load regulator on six DC motors, block brake in position P, PZB 90 and Sifa.
tpl-br52-hint = Steam: fire, boiler and cylinders as one loop, block brake in position G, no train protection.
tpl-railcar-hint = Diesel railcar with two engines, hydrodynamic brake in the transmission, disc and magnetic track brake, PZB 90 and automatic doors.
tpl-coach-hint = Hauled passenger coach without a drive, KE-GPR disc brake in position P, doors but no train protection of its own.
tpl-eaos-hint = Open freight wagon without a drive, block brake in position G with an empty/loaded changeover, no train protection.
tpl-eaos-k-hint = The same wagon with a K control valve: graduated application, but single-release — it has to be recharged before it brakes properly again.

## Vehicle editor: display widgets
##
## The code-free content of a screen: labels, values and bars placed in texture
## pixels and drawn in list order, so a later widget covers an earlier one. The
## preview shows the layout, not the readings — those are simulation state.

disp-widget-list = Widgets
disp-widgets-empty = no widgets — the screen stays black unless the vehicle script draws it
disp-widget-count = { $count } widgets
disp-html-overrides = the HTML page draws this screen on its own — a widget list kept beside it never reaches the texture
disp-preview-note = layout preview, the values are placeholders — drag a widget to place it
action-add-widget = Add widget
action-add-widget-hint = one element of the code-free screen content — a label, a value or a bar
action-widget-up = one place earlier — an earlier widget is covered by the later ones
action-widget-down = one place later — a later widget is drawn on top
disp-widget-kind = Kind
disp-widget-kind-hint = switching keeps the position, the colour and the source
disp-widget-label = Label
disp-widget-value = Value
disp-widget-bar = Bar
disp-widget-untitled = (no text)
disp-widget-pos = Position
disp-widget-pos-hint = px from the top left corner of the texture
disp-widget-text = Text
disp-widget-size = Text size
disp-widget-size-hint = px — the height of the glyphs on the texture
disp-widget-box = Bar size
disp-widget-box-hint = px — width × height of the full bar
disp-widget-source = Source
disp-widget-source-hint = the quantity the value or the bar follows
disp-source-indicator = Indicator …
disp-widget-indicator = Indicator
disp-widget-indicator-hint = named readout of the train protection (mfa_v_soll, mfa_zielentfernung); 0 while it is absent
disp-widget-indicator-placeholder = mfa_v_soll
disp-widget-decimals = Decimals
disp-widget-unit = Unit
disp-widget-unit-hint = written after the number with a space; empty leaves it off
disp-widget-scale = Scale
disp-widget-scale-hint = the value is multiplied by this before it is formatted — 3.6 turns m/s into km/h
disp-widget-max = Full scale
disp-widget-max-hint = the value at which the bar is full
disp-widget-color = Colour
disp-widget-color-hint = linear RGBA of the text or the bar

## Vehicle editor: metadata, variants and loads

meta-class = Class
meta-class-hint = Class or type designation as it is anscribed: BR 101, Bmz 236
meta-manufacturer = Manufacturer
meta-manufacturer-hint = Who built the vehicle — Adtranz, Siemens, MBB
meta-year = Year built
meta-year-hint = Year the vehicle was delivered; leave it unset where the file does not say
meta-year-unset = not stated
meta-epoch = Era
meta-epoch-hint = Free text, and free to span two: V, IV–VI
meta-country = Country
meta-country-hint = Where the vehicle runs. The list holds the countries the simulator has signals and train protection for; anything else takes its ISO 3166-1 alpha-2 code in the field beside it
meta-country-unset = not stated
meta-country-other = code
meta-operator = Operator
meta-operator-hint = The railway the vehicle runs for: DB Fernverkehr, ÖBB
meta-author = Author
meta-author-hint = Who built this file, not who built the vehicle
meta-thumbnail = Preview image
meta-thumbnail-hint = Picture the vehicle browser lists the vehicle with. Like the model it has to lie below mods/
meta-thumbnail-placeholder = mod/assets/…
meta-thumbnail-pick = Pick a picture below mods/
meta-description = Description
meta-description-placeholder = What the vehicle browser says about this vehicle
filter-image = Image

var-heading = Variants
var-empty = No variants — the vehicle runs in one appearance, without a running number
action-add-variant = Add variant
action-add-variant-hint = a livery, a number series, an era — appearance only, never physics
var-name-placeholder = Name of the variant
var-model = Model
var-model-hint = glTF file this variant is drawn with; empty = the vehicle's own model
var-model-placeholder = empty = base model
var-model-none = No model — neither the variant nor the vehicle states one
var-model-effective = Drawn as { $file }
var-epoch = Era
var-epoch-hint = Empty = the era of the vehicle
var-numbers = Running numbers
var-numbers-hint = One per line. The consist builder draws one from them, decided by the scenario's seed — so every player sees the same number on the same vehicle
var-numbers-placeholder = 101 001-6
var-description = Description
var-description-placeholder = Empty = the description of the vehicle

load-heading = Loads
load-empty = No loads — the vehicle runs empty
action-add-load = Add load
action-add-load-hint = goods, their mass, and the model node that shows them
load-name-placeholder = Name of the goods
load-mass = Mass
load-mass-hint = kg of goods, on top of the tare mass
load-node = Model node
load-node-hint = glTF node shown while this load is carried — a coal heap, a stack of containers. Empty = the load cannot be seen
load-node-placeholder = empty = invisible
load-total = Total mass { $mass } t
load-capped = Heavier than the maximum payload — the vehicle carries { $max } t of it

## Vehicle editor: key figures and the tractive effort diagram

key-mass = Mass
key-mass-hint = Tare mass, and behind the arrow the mass with the full payload
key-axle-load = Axle load
key-axle-load-hint = Total mass over the number of axles. Line class D carries 22.5 t, class C 20 t, class B 18 t
key-axle-load-warn = Axle load { $load } t is over the { $limit } t of line class D — the vehicle is restricted to lines that carry it
key-brake-percentage = Braked weight percentage
key-brake-percentage-hint = Braked weight over mass. The figure a brake sheet is written in, and the first place a mistyped braked weight shows up
key-brake-percentage-g = … in position G
key-brake-percentage-p = … in position P
key-brake-percentage-r = … in position R
key-adhesive-mass = Adhesive mass
key-adhesive-mass-hint = Mass on the driven axles — what the tractive effort has to be carried by
key-adhesion-limit = Adhesion limit
key-adhesion-limit-hint = Tractive effort the dry rail carries at standstill, after Curtius/Kniffler. Above it the wheels spin
key-starting-effort = Starting tractive effort
key-starting-effort-hint = Tractive effort at standstill, of the strongest drive mode
key-power-weight = Power-to-weight ratio
key-power-weight-hint = Highest power at the wheel over the mass — how briskly the vehicle accelerates, whatever its size
key-balancing-speed = Balancing speed
key-balancing-speed-hint = Where the tractive effort has fallen to the running resistance: what the vehicle runs at on the level, on its own
key-above-v-max = above v max
key-slip-warn = Starting tractive effort { $force } kN is over the { $limit } kN the adhesive mass carries — the vehicle spins its wheels on every start
plot-tractive-effort = Tractive effort (km/h → N)
plot-resistance = Running resistance
plot-dynamic-brake = Dynamic brake
plot-adhesion-limit = Adhesion limit

## Vehicle editor: part function picker

part-function-pick = Pick a function the simulator reads

## Vehicle editor: check report
## Findings of the file-wide check next to the data panel. Every one of them names
## what to do, not only what is wrong.

check-length = Length over buffers is 0 — state it in metres, otherwise the next vehicle of the consist stands inside this one
check-mass = Tare mass is 0 — state it in kilogrammes; the vehicle has no inertia and no weight on the rail without it
check-gauge = Track gauge is 0 — state it in metres (1.435 for standard gauge), otherwise the vehicle fits no line
check-axles = No axle count — the brake runs on the reference axle load instead of this vehicle's, and no consist list adds up. State the number of axles
check-rotating-mass = Rotating mass allowance { $value } lies outside 0 … 0.5 — a coach carries about 0.05, a powered vehicle about 0.25
check-load-over-payload = Load "{ $load }" weighs { $mass } t, more than the maximum payload of { $max } t — the vehicle carries only the maximum. Raise the payload or lighten the load
check-doors = Passenger doors, but no door control — a powered vehicle releases its own doors, so nothing would ever open them. Pick a door system
check-no-brake-weight = No braked weight — the vehicle is dragged along unbraked. State the anscribed figure in tonnes
check-no-brake-force = No brake force — the cylinder pushes nothing against the wheel. State the force of the fully applied brake in newtons
check-brake-percentage = Braked weight percentage comes out at { $value } % — check the braked weight of { $weight } t against the tare mass; a European brake sheet stays between 30 and 250 %
check-load-braking = Fully loaded the vehicle only reaches { $value } % braked weight for { $payload } t of payload — the braked weight does not follow the load. Fit a weighing valve, or an empty/loaded changeover
check-drive-no-adhesion = The vehicle has a drive, but no driven axle carries its force to the rail — it will not move. State the share of the mass on driven axles, or mark axles as driven in the diagram
check-adhesion-no-drive = Weight on driven axles, but no drive chain — the figure does nothing. Set the adhesive mass share to 0, or add a drive
check-no-v-max = No top speed stated — speedometer and AFB fall back to 160 km/h. State the running gear limit
check-drive-over-v-max = The drive pulls to { $drive } km/h, past the running gear limit of { $vehicle } km/h — nothing stops the vehicle there. Raise the top speed, or cap the drive
check-tractive-effort = Starting tractive effort of { $force } kN is above what a dry rail holds ({ $limit } kN) — the vehicle spins its wheels instead of starting. Lower the force, or put more weight on the driven axles
check-model-no-file = The model names no glTF file — state the file below mods/, or take the model out
check-part-node = Moving part on "{ $node }": the model has no node of that name — the part never moves. Correct the name, or take the part out
check-part-function = Moving part on "{ $node }": { $reason }. The part stays where it is; pick a function the simulator evaluates, unless a mod of your own reads this one
check-control-node = Cab control on "{ $node }": the model has no node of that name — the control cannot be operated. Correct the name
check-display-node = Display "{ $name }" on "{ $node }": the model has no node of that name — the screen stays dark. Correct the name
check-load-node = Load "{ $load }" shows "{ $node }": the model has no node of that name — the load stays invisible. Correct the name, or leave the node empty
check-node-twice = Node "{ $node }" is bound more than once — only the first binding acts, the others are dropped. Give each part a node of its own
check-lod-duplicate = Level of detail { $level } is listed twice — one view distance per level
check-lod-order = The view distance of level { $level } is not greater than that of the level before it — list the levels coarsest last, with rising distances
check-lod-no-nodes = Level of detail { $level } is listed, but no node is named _LOD{ $level } — the level draws nothing. Take it out, or rename the nodes
check-lod-missing = The model carries _LOD nodes, but the vehicle lists no levels — every level draws at once, on top of the others. Take the levels over from the file

## GNT — speed supervision for tilting technology

blk-gnt = GNT
blk-gnt-hint = Speed supervision for tilting technology — releases the higher curve speeds of a tilting unit inside a GNT area. Needs a tilt angle above zero on the vehicle
bake-gnt-without-tilt = GNT on a vehicle without tilting technology — set a tilt angle, otherwise the equipment is dropped

## Main menu: the vehicle's data sheet in the detail pane

menu-fact-variant = Variant
menu-fact-class = Class
menu-fact-manufacturer = Manufacturer
menu-fact-build-year = Built
menu-fact-epoch = Era
menu-fact-operator = Operator
menu-fact-country = Country
menu-fact-author = Author

## Vehicle editor: check section header

group-checks-errors = Checks — { $errors } errors
group-checks-warnings = Checks — { $warnings } warnings
group-checks-both = Checks — { $errors } errors, { $warnings } warnings

# --- Module envelope -------------------------------------------------------
tool-group-module = Module
tool-envelope = Envelope
tool-envelope-hint = Reshape the module boundary: drag a corner, click a side to add one, Delete removes the selected corner
module-envelope = Envelope
envelope-points = Corners: { $count }
envelope-anchor-lat = Anchor latitude
envelope-anchor-lon = Anchor longitude
envelope-min-points = A polygon needs three corners.
action-edit-envelope = Edit
action-reset-envelope = Reset
action-reset-envelope-hint = Puts a square envelope back around the anchor — a module without one gets its first here.
sel-envelope-summary = Envelope corner { $index } of { $count }
status-envelope-none = This module has no envelope yet — reset it under Module boundaries.
status-envelope-point-added = Corner added — drag it where it belongs.
status-envelope-no-hit = Nothing hit: drag a corner, or click a side to add one.
status-outside-envelope = Outside the module envelope — that ground belongs to the neighbouring module.
status-forest-baked-clipped = { $count } trees baked, { $dropped } dropped outside the envelope
status-forest-imported-clipped = { $count } trees from { $areas } areas, { $dropped } dropped outside the envelope
action-cancel = Cancel
