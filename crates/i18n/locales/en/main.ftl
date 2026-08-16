# Connected Rails — English source strings.
#
# Keys ending in -hint are the tooltip of the field of the same name.
# Placeholders such as { $file } are filled in by the program; keep them.

## Windows and menus

window-simulator = Connected Rails
window-vehicle-editor = Connected Rails — Vehicle editor
window-vehicle-editor-named = { $name } — Connected Rails Vehicle editor
window-vehicle-editor-unsaved = • { $name } — Connected Rails Vehicle editor
window-route-editor = Connected Rails — Route editor
window-route-editor-named = { $name } — Connected Rails Route editor
window-route-editor-unsaved = • { $name } — Connected Rails Route editor

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
snd-factors = Volume factors
snd-factors-hint = each curve is multiplied into the volume — a second quantity scaling an entry whose volume already follows a first one, like the track roughness on the rolling noise
action-add-factor = Add factor
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
disp-widgets = { $count } widgets — edited in the vehicle file

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

## Route editor

action-new-line = New line
action-open-line = Open line…
action-delete = Delete
action-load-imagery = Load imagery configuration (F5)
action-save-imagery = Save imagery configuration (F2)
overlay-toggle = On/off (O)
overlay-next-provider = Next provider (P)
overlay-offline = Offline mode (L)
overlay-clear-cache = Clear cache (C)
overlay-retry = Reset failed attempts (R)
action-show-terrain = Show terrain (T)
help-pan = WASD/arrows or middle mouse drag pan · wheel or PgUp/PgDn height
help-opacity = [ ] opacity · , . zoom level · Z automatic
help-offset = Numpad 4/6/8/2 image offset, 5 reset
help-draw = Draw track: click points · Enter finishes · Esc cancels
help-map = Middle mouse button drags, wheel zooms · WASD pans · pick a tool on the left and click
help-terrain = T shows the module's terrain, as the run builds it, in place of the aerial imagery

status-ready = Ready
status-position = { $lat }°, { $lon }°   height { $height } m
status-ground-height = ground { $height } m
status-terrain-flat = No height data yet — the ground is flat; import a DGM under Height data
status-cache-cleared = Cache cleared
status-retry-reset = Failed attempts reset
status-saved = { $file } saved
status-save-failed = Saving failed: { $error }
status-not-readable = { $file } not readable
status-not-compiling = { $file } does not compile
status-compile-error = Line does not compile: { $error }
status-no-track-hit = No track near the click — devices sit on a track
status-split-at-end = Too close to the track end — click at least 1 m inside
status-split-failed = Switch not placed — the line does not compile
status-ghost-loaded = Ghost module { $file }: { $boundaries } boundaries
status-route-derived = Path found: { $sections } sections, { $overlap } in the overlap, { $switches } switches
status-routes-found = { $added } routes added, { $known } already there
status-no-route-path = No path from the entry to the exit signal — check the direction the signals act in
status-no-objects = No objects installed — a mod ships them as objects/*.ron
status-config-unreadable = { $file } not readable ({ $error }) — default active
status-config-created = { $file } created
status-config-not-writable = { $file } not writable: { $error }

heading-line = Line
line-name = Name
line-counts = { $edges } tracks · { $devices } devices

heading-tools = Tools
tool-select = Select
tool-select-hint = Click a device or a track to inspect it · Delete removes it (1)
tool-draw = Draw track
tool-draw-hint = Clicks set points: the first starts the track, every further one appends a tangent arc · Enter or right-click finishes · Esc cancels (2)
tool-device = Place device
tool-device-hint = Click a track to put the chosen device kind on it (3)
tool-switch = Place switch
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
terrain-count = { $count } strokes on this line
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
sel-none = Nothing selected — the Select tool picks devices and tracks.
sel-edge-summary = Track { $index }: { $length } m, { $segments } segments
sel-edge-devices = { $devices } devices on this track
sel-edge-handles = Drag the round handles on the map to bend the track.
sel-edge-fixed = Transition curves — this track's support points are not editable.
sel-track-type = Track type
sel-track-type-none = Default type over the whole track.
sel-track-type-from = From this distance along the track
sel-track-type-hint = Each row: from position s onwards this type applies — texture, roughness, superstructure speed limit come from track_types/*.ron of a mod
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
status-heights-need-mod = Save the line inside a mod first — height data lives next to it, in the mod
status-heights-no-source = No DGM delivery chosen yet
status-heights-imported = { $tiles } height tiles written, { $empty } skipped without data — the module carries its ground now

heading-markers = Reference markers

heading-module = Module
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
ghost-boundaries = { $count } boundaries as snap targets
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
check-lzb-no-conductor = Track { $edge }: LZB track type, but the line places no line conductor
heading-imagery = Aerial imagery
img-enabled = Show overlay
img-provider = Provider
img-opacity = Opacity
img-zoom = Zoom
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

hud-speed = v = { $speed } km/h   max { $limit } km/h   distance { $distance } m   { $time }
hud-brakes = BP { $pipe } bar   C { $cylinder } bar   AR { $auxiliary } bar   MR { $main } bar   Direct { $direct } bar   Air { $air } Nl
hud-traction = Power controller { $throttle }   Tractive effort { $tractive } kN   Braking effort { $braking } kN   Brake { $valve }
hud-afb = AFB { $state }   target { $target } km/h
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
hud-control = { $name }: { $value } %
hud-keys-drive = W/S power controller  A/D brake  E emergency  Q lap  Z fill  C/V direct brake  Y wiper  6 AFB  7/8 AFB target
hud-keys-safety = Space Sifa  PgDn acknowledge  End free  Del override  N/M/B LZB  U train category  1–4 preparation  F1–F3 camera  F9 mods
hud-keys-lights = 9 headlights  0 cab light  ,/. instrument dimmer

## Main menu
#
# The navigation column on the left, then the pages it opens. A -hint key is the
# second, dimmer line under the entry of the same name.

menu-tagline = German railway simulation
menu-nav-title = Main menu
menu-drive = Drive
menu-drive-hint = Line, vehicle and scenario
menu-mods = Mods
menu-mods-hint = Switch installed content on and off
menu-settings = Settings
menu-settings-hint = Picture, sound and controls
menu-quit = Quit
menu-quit-hint = Leave the simulator
menu-step = Step { $step } of { $total }
menu-select-line = Select line
menu-select-loco = Select vehicle
menu-select-scenario = Select scenario
# The built-in content the simulator falls back on when nothing is picked. The chip on
# the row says so, so the name itself no longer has to.
menu-chip-builtin = Built in
menu-chip-composition = Composition
menu-line-builtin = Example line
menu-loco-builtin = BR 101
menu-scenario-none = No scenario — free run
menu-free-run = No timetable and no scoring: the line, the vehicle, and wherever you take it.
# The key hints in the footer bar: one chip per key.
menu-hint-select = select
menu-hint-confirm = confirm
menu-hint-toggle = on/off
menu-hint-start = start run
menu-hint-change = change
menu-hint-next = next value
menu-hint-back = back
menu-hint-section = section
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
menu-fact-line = Line
menu-fact-events = Events
menu-fact-km = { $value } km
menu-fact-m = { $value } m
menu-fact-t = { $value } t
menu-fact-kmh = { $value } km/h

## Settings
#
# One page of the main menu, stored as TOML in the operating system's settings
# directory. A -hint key is the description under the setting's name.

set-graphics = Graphics
set-audio = Audio
set-gameplay = Gameplay
set-stored = Kept between runs in the settings file of your user account.
# Badge on the rows that are baked into the scene when a run starts.
set-restart-badge = next run
set-view-distance = View distance
set-view-distance-hint = How far terrain is built and drawn — the biggest single cost.
set-shadows = Shadows
set-shadows-hint = Shadow maps of the sun.
set-bloom = Bloom
set-bloom-hint = Makes lamps and signals glow after dark.
set-fullscreen = Fullscreen
set-fullscreen-hint = Borderless, on the monitor the window is on.
set-vsync = Vertical sync
set-vsync-hint = Caps the frame rate at the monitor's, against tearing.
set-volume = Master volume
set-volume-hint = Loudness of everything the simulator plays.
set-language = Language
set-language-hint = Applies right away, to the menu as well as to the cab.
set-language-system = System
set-hud = HUD
set-hud-hint = The readout of speed, brakes and train protection while driving.
set-look-speed = Look sensitivity
set-look-speed-hint = How far the view turns while the right mouse button is held.
set-reset = Reset to defaults
set-reset-hint = Puts every setting on this page back to how it shipped.
# Units of the values on the right of a settings row.
set-metres = { $value } m
set-percent = { $value } %
set-factor = { $value } ×

## Mod manager

mods-title = Mods
mods-none = No mods installed — put a mod directory into mods/.
mods-missing-depends = requires: { $depends } (missing or switched off)
mods-content = Content: { $vehicles } vehicles, { $lines } lines, { $compositions } compositions, { $scenarios } scenarios, { $timetables } timetables, { $signals } signal types, { $scripts } scripts
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
