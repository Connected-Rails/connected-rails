# TrainSim-DE — deutsche Übersetzung.
#
# Schlüssel auf -hint sind der Tooltip des gleichnamigen Feldes.
# Platzhalter wie { $file } füllt das Programm; sie müssen erhalten bleiben.

## Fenster und Menüs

window-simulator = TrainSim-DE
window-vehicle-editor = TrainSim-DE — Fahrzeugeditor
window-vehicle-editor-named = { $name } — TrainSim-DE Fahrzeugeditor
window-vehicle-editor-unsaved = • { $name } — TrainSim-DE Fahrzeugeditor
window-route-editor = TrainSim-DE — Streckeneditor
window-route-editor-named = { $name } — TrainSim-DE Streckeneditor
window-route-editor-unsaved = • { $name } — TrainSim-DE Streckeneditor

menu-file = Datei
menu-edit = Bearbeiten
menu-view = Ansicht
menu-help = Hilfe
menu-overlay = Überlagerung
menu-language = Sprache

action-new = Neu
action-open = Öffnen…
action-save = Speichern
action-save-as = Speichern unter…
menu-recent = Zuletzt geöffnet
recent-missing = diese Datei gibt es nicht mehr
action-quit = Beenden
action-suggest = Vorschlag
action-undo = Rückgängig
action-redo = Wiederherstellen

filter-vehicle-ron = Fahrzeug (RON)
filter-line-ron = Strecke (RON)
filter-model-gltf = Modell (glTF)

## Gemeinsam

common-on = ein
common-off = aus
common-none = —

## Fahrzeugeditor — Menü und Statusleiste

action-import-model = Modell importieren…
action-import-gltf = glTF importieren…
view-reference-body = Referenzkörper (LÜP)
view-grid = Bodenraster (1 m)
help-mouse = Rechte Maustaste: drehen · Rad: zoomen
help-model-conventions = Modellkonventionen: siehe MODS.md
action-about = Über…
about-version = Version { $version }

status-new-vehicle = Neues Fahrzeug
status-loading = { $file } wird geladen…
status-loaded = { $file } geladen
status-written = { $file } geschrieben
status-error = { $file }: { $error }
status-nodes-read = { $count } Knoten gelesen
status-outside-mods = { $path } liegt außerhalb von mods/ — kopiere das Modell zuerst in deinen Mod
status-unsaved = • ungespeichert
status-new-file = (neu)
dialog-error-title = Fehler
confirm-comments-title = Kommentare gehen verloren
confirm-comments = { $file } enthält Kommentare. Der Editor schreibt die Datei neu aus den Daten — die Kommentare sind danach weg. Trotzdem speichern?
confirm-unsaved-title = Ungespeicherte Änderungen
confirm-unsaved = Änderungen an „{ $name }“ speichern?

## Fahrzeugeditor — Grunddaten

heading-vehicle = Fahrzeug
field-name = Name
group-base-data = Grunddaten

veh-length = Länge über Puffer
veh-length-hint = m — amtliche LÜP; Puffer 1–2 cm eingedrückt zeichnen
veh-gauge = Spurweite
veh-gauge-hint = m — wird gegen die Infrastruktur geprüft
veh-vmax = v max
veh-vmax-hint = km/h — Grenze des Laufwerks
veh-mass = Masse
veh-mass-hint = kg — Eigenmasse
veh-payload = Max. Zuladung
veh-payload-hint = kg — Reisezugwagen etwa 5 t

group-running-gear = Laufwerk
veh-rotating-mass = Rotierende Massen
veh-rotating-mass-hint = Anteil an der Masse — E-Lok 0,15–0,25, Wagen 0,06–0,09
veh-axles = Achsen
veh-axles-hint = Angabe für Zugbildungslisten
veh-adhesive = Anteil Reibungsmasse
veh-adhesive-hint = Anteil der Masse auf angetriebenen Achsen — Lok 1,0, Wagen 0,0. Begrenzt Zug- und Bremskraft über den Kraftschluss; bei 0 setzt das Fahrzeug nichts um.
veh-axle-base = Achsstandsumme
veh-axle-base-hint = m — Summe über alle Drehgestelle, Grundlage des Bogenwiderstands
veh-tilt = Neigewinkel
veh-tilt-hint = ° — 0 konventionell, ~8 Neigetechnik
veh-hunting = Sinuslauf
veh-hunting-hint = −1 keiner … 0 normal … 1 stark

group-coupler = Kupplung
cpl-type = Bauart
cpl-screw = Schraubenkupplung
cpl-centre = Mittelpufferkupplung
cpl-custom = eigene Werte
cpl-slack = Spiel
cpl-slack-hint = Gesamtspiel zwischen Zugvorrichtung und Puffern — Schraubenkupplung 0,06–0,10 m
cpl-draw = Zugvorrichtung
cpl-draw-hint = Steifigkeit der Zugvorrichtung
cpl-buffer = Puffer
cpl-buffer-hint = Steifigkeit der Puffer — steifer als die Zugvorrichtung
cpl-damping = Dämpfung
cpl-breaking = Bruchkraft
cpl-breaking-hint = Mindestbruchlast — Schraubenkupplung etwa 1 MN

group-resistance = Fahrwiderstand
res-rolling = Rollwiderstand a
res-rolling-suggest-hint = etwa 2 ‰ des Gewichts — ergäbe { $value } N
res-speed-term = Geschwindigkeitsglied b
res-air = Luftwiderstand
res-cw-a = cw·A
res-davis-c-hint = quadratisches Davis-Glied c
res-curve = Bogenwiderstand
res-curve-hint = Faktor auf Röckl — 1 = wie ihn die Achsstandsumme ergibt
res-at-100 = Widerstand bei 100 km/h: { $newtons } N
res-plot = Widerstand (km/h → N)

## Fahrzeugeditor — Ausrüstung

group-equipment = Ausrüstung
opt-not-fitted = nicht vorhanden

eq-german-protection = Deutsche Zugsicherung
eq-german-protection-hint = Sifa, Indusi/PZB und LZB, wie im Fahrzeug verbaut
eq-pzb = Indusi/PZB
eq-pzb-hint = Bauart an Bord — ohne sie fährt das Fahrzeug allein auf LZB
eq-sifa = Sifa
eq-sifa-hint = Sicherheitsfahrschaltung
sifa-time-time = Zeit-Zeit
sifa-time-distance = Zeit-Weg
eq-lzb = LZB 80/I 80
eq-lzb-on-board = an Bord
eq-lzb-hint = führt nur auf Strecken mit Linienleiter
eq-afb = AFB vorhanden
eq-afb-hint = Automatische Fahr- und Bremssteuerung: hält die im Führerstand eingestellte Zielgeschwindigkeit; unter LZB-Führung begrenzt deren v-Soll
eq-passenger-doors = Fahrgasttüren
eq-passenger-doors-hint = diese Türen folgen der Türsteuerung des Zuges
eq-doors = Türsteuerung
eq-doors-hint = was dieser Führerstand befiehlt — das führende Fahrzeug entscheidet für den Zug

group-behaviour = Verhalten
veh-script = Skript
veh-script-hint = Lua-Skript, das das Verhalten steuert — AFB, Schaltwerkslogik, Anlassvorgang
field-script-hint = <mod>:<name>

## Fahrzeugeditor — Modellbereich

heading-model = Modell
model-none-loaded = Kein Modell geladen.
model-conventions =
    Detailstufen: Knotennamen, die auf _LOD0, _LOD1, … enden
    Bewegte Teile: Präfixe door_, pant_, sw_, gauge_, lamp_, wheel_,
    oder die Blender-Eigenschaft ts_function.

group-lods = Detailstufen
action-read-node-names = Aus Knotennamen lesen
action-read-node-names-hint = übernimmt { $count } aus den Knotennamen erkannte Stufen
action-read-node-names-same = die Stufen entsprechen bereits den Knotennamen
lod-show-hint = im Ansichtsfenster zeigen
lod-distance-hint = bis zu dieser Entfernung wird diese Stufe gezeichnet

group-parts = Bewegte Teile
action-take-suggestions = Alle Vorschläge übernehmen
action-take-suggestions-hint = bindet { $count } noch nicht gebundene Knoten
action-take-suggestions-none = alle vorgeschlagenen Knoten sind bereits gebunden
part-function-placeholder = Funktion
part-function-hint = Was der Knoten darstellt. Bekannte Formen: door_<name> · pantograph · switch:<name> · gauge:<Name oder Melder> · lamp:<Name oder Melder> · digit:<Melder>:<Stelle> · wiper · wheel — eigene Namen sind erlaubt, die App bildet ab, was sie kennt.
part-amount-hint = voller Ausschlag der Bewegung — vom Funktionswert 0 bis 1
group-nodes = Knoten in der Datei
node-bind-hint = als bewegtes Teil binden
part-node-missing-hint = diesen Knoten gibt es im Modell nicht — die Bindung läuft ins Leere
node-filter-hint = Knoten filtern
node-count = { $total } Knoten
node-count-filtered = { $shown } von { $total } Knoten

motion-visible = sichtbar
motion-rotate = drehen
motion-move = verschieben

## Fahrzeugeditor — Bremse

group-brake = Bremse
brk-valve = Steuerventil
brk-valve-hint = welches Steuerventil verbaut ist
brk-position = Bremsstellung
brk-position-hint = G Güterzug · P Personenzug · R Schnellzug · R+Mg mit Magnetschienenbremse
brk-friction = Reibpaarung
brk-friction-hint = wie der Reibwert über der Geschwindigkeit verläuft
brk-friction-points = Reibwert (km/h → µ)
brk-friction-plot = Reibwert über der Geschwindigkeit
brk-weight = Bremsgewicht
brk-weight-hint = t — aus der Anschrift des Fahrzeugs, beladenes Fahrzeug
brk-load = Lastabbremsung
brk-load-hint = wie viel vom Bremsgewicht bleibt, wenn das Fahrzeug leer läuft
brk-load-empty = Bremsgewicht leer
brk-load-empty-hint = Anteil am Bremsgewicht des beladenen Fahrzeugs in Stellung „Leer“
brk-load-mass = Umstellmasse
brk-load-mass-hint = t Gesamtmasse — darüber bremst der Wagen in Stellung „Beladen“
brk-force = Bremskraft
brk-force-hint = N bei vollem Zylinderdruck und Stillstand
brk-force-suggest-hint = aus dem Bremsgewicht — ergäbe { $value } N
brk-cylinder = Zylinderdruck
brk-cylinder-hint = bar bei Vollbremsung
brk-cyl-reservoir = Zylinder / Vorratsbehälter
brk-cyl-reservoir-hint = Volumenverhältnis — bestimmt, wie schnell sich die Bremse erschöpft
brk-percentage = Bremshundertstel des leeren Fahrzeugs: { $percent } %

group-additional-brakes = Zusatzbremsen
label-force = Kraft
brk-mg = Magnetschienenbremse
brk-direct = Zusatzbremse (direkte Bremse)
brk-direct-cylinder-hint = bar; 0 = wie die selbsttätige Bremse
brk-parking = Feststellbremse
brk-spring = Federspeicherbremse
brk-spring-hint = wird von Luft gelöst gehalten — legt sich selbst an, wenn der Hauptluftbehälter leerläuft
brk-pilot = Vorgesteuerter Bremszylinder
brk-pilot-hint = Relaisventil aus dem Hauptluftbehälter gespeist: füllt schneller, kann nicht entlüften
brk-supplement = Luftergänzungsbremse
brk-supplement-hint = ergänzt, was die elektrische Bremse nicht aufbringt
brk-angleicher = Angleicher
brk-angleicher-hint = gleicht Undichtigkeiten der Hauptluftleitung in Abschlussstellung aus; ohne Gedächtnis

group-air = Luft
air-aux = Vorratsluftbehälter
air-pipe = Hauptluftleitung
air-pipe-hint = l — Anteil dieses Fahrzeugs
air-main = Hauptluftbehälter
air-main-hint = l — 0 = keiner
air-compressor = Kompressor
air-compressor-hint = l/min Ansaugluft — 0 = keiner
air-leakage = Undichtigkeit
air-leakage-hint = l/min Ansaugluft
brk-slip = Gleitschutz
brk-slip-hint = wie das Fahrzeug auf einen schleudernden oder gleitenden Radsatz antwortet

friction-block = Graugussklotz
friction-disc = Scheibe
friction-k = K-Sohle
friction-ll = LL-Sohle
friction-magnetic = Magnetschiene
friction-custom = Eigene Kennlinie

load-none = keine
load-weighing = Wiegeventil (stufenlos)
load-changeover = Umstellung Leer/Beladen

slip-none = keiner
slip-brake = Schleuderbremse
slip-cutback = Zugkraftrücknahme
slip-creep = Schlupfregelung

## Fahrzeugeditor — Antrieb

group-drive = Antrieb
drive-unpowered-note = Antriebsloses Fahrzeug.
drv-type = Antriebsart
drv-type-hint = Kennlinie: Zugkraft direkt aus dem Diagramm · Schaltwerk: Reihenschlussmotoren mit Fahrstufen · Umrichter: Drehstromantrieb · Diesel: Motorkennfeld und Getriebe
traction-none = antriebslos
traction-curve = Zugkraftkennlinie
traction-tap = Schaltwerk
traction-converter = Umrichter
traction-diesel = Diesel
curve-note = Zugkraft direkt aus dem Diagramm — kein Motor, kein Getriebe.

drv-force-plot = Zugkraft (km/h → N)
drv-vmax = v max Antrieb
drv-vmax-hint = Ende der Zugkraftkennlinie — darüber gibt der Antrieb nichts mehr ab
drv-ramp = Anstiegszeit
drv-ramp-hint = s von 0 bis zur vollen Zugkraft
drv-start-force = Anfahrzugkraft
drv-start-force-diesel = Anfahrzugkraft
drv-start-force-diesel-hint = N — ohne Motorkennfeld
drv-power = Leistung
drv-power-hint = W am Rad
drv-pullout = Kippgeschwindigkeit
drv-pullout-hint = km/h — darüber fällt die Zugkraft mit 1/v²; 0 = keine Grenze
drv-brake-force = E-Bremskraft
drv-brake-force-hint = was die elektrische Bremse aufbringt — die Druckluftbremse kommt getrennt dazu
drv-brake-power = E-Bremsleistung
drv-brake-power-hint = Grenze der Rückspeisung bzw. der Bremswiderstände
drv-brake-fade = Ausblendung
drv-brake-fade-hint = darunter blendet die elektrische Bremse aus; die Druckluftbremse übernimmt
drv-fade = Ausblendung
drv-fade-hint = darunter blendet die elektrische Bremse aus; die Druckluftbremse übernimmt
drv-crank-time = Anlassdauer
drv-wheel-diameter = Raddurchmesser
drv-regenerative = Rückspeisefähig
drv-regenerative-hint = speist in den Fahrdraht zurück — ohne Fahrdrahtspannung wirkungslos

table-tractive-effort = Zugkraft (km/h → N)
table-dynamic-brake = Elektrische Bremse (km/h → N)
table-torque = Volllastmoment (1/min → N·m)
action-add-point = + Punkt
action-close = Schließen

# The shared curve editor: every (x, y) table opens it from its sparkline.
curve-empty = noch keine Punkte
curve-open-hint = Klick öffnet den Kurveneditor.
curve-editor-help = Punkte ziehen · Doppelklick fügt einen Punkt hinzu · Rechtsklick entfernt ihn

tap-steps = Fahrstufen
tap-steps-hint = des Schaltwerks
tap-step-time = Zeit je Fahrstufe

section-series-motor = Daten des Reihenschlussmotors
section-rheostatic-brake = Widerstandsbremse
section-engine-map = Motorkennfeld
section-transmission = Strömungsgetriebe
section-retarder = Hydrodynamische Bremse

mot-count = Motoren
mot-count-hint = Anzahl im Fahrzeug
mot-resistance = Widerstand
mot-resistance-hint = Ω — Anker und Feld zusammen
mot-machine-constant = Maschinenkonstante
mot-machine-constant-hint = V·s/A — Flussverkettung je Ampere, ungesättigt
mot-saturation = Sättigungsstrom
mot-max-current = Höchststrom
mot-max-current-hint = A — das Stromgrenzrelais
mot-max-voltage = Höchstspannung
mot-max-voltage-hint = V auf der obersten Fahrstufe
mot-gear-ratio = Übersetzung
mot-gear-ratio-hint = Motor : Radsatz
mot-efficiency = Wirkungsgrad
mot-efficiency-hint = Motor und Getriebe
mot-field-steps = Feldschwächstufen (1 = volles Feld)
action-add-stage = + Stufe

eng-idle = Leerlauf
eng-idle-hint = 1/min
eng-rated = Nenndrehzahl
eng-rated-hint = 1/min
eng-overspeed = Abregeldrehzahl
eng-overspeed-hint = 1/min
eng-inertia = Trägheitsmoment
eng-inertia-hint = kg·m² einschl. Schwungrad
eng-rack-time = Reglerlaufzeit
eng-rack-time-hint = s vom Leerlauf bis Volllast
eng-governor = Regler
eng-governor-hint = drehzahlgeregelt: Hauptstreckendiesel · füllungsgeregelt: Rangierloks und Triebwagen mit mechanischer Einspritzpumpe
gov-speed = drehzahlgeregelt
gov-speed-hint = der Fahrschalter stellt die Drehzahl, der Regler hält sie
gov-fill = füllungsgeregelt
gov-fill-hint = der Fahrschalter ist die Einspritzmenge, die Drehzahl folgt der Last
gov-notches = Stufen
gov-notches-hint = 0 = stufenlos
gov-droop = Ungleichförmigkeitsgrad
gov-droop-hint = Anteil der Nenndrehzahl, um den die Solldrehzahl bei voller Füllung absinkt — 0 = isochron, Vorbild 0,03…0,05

trm-suggest-hint = Startsatz aus Anfahrzugkraft, Höchstgeschwindigkeit, Nenndrehzahl, Nennmoment und Raddurchmesser — hier beginnt das Fitten gegen den Plot
trm-fill-steps = Füllstufen
trm-fill-steps-hint = 0 = stufenlos, 1 = nur füllen/entleeren, höher = Teilfüllung wie im Original
trm-fill-time = Füllzeit
trm-fill-time-hint = s zum Füllen eines Kreislaufs
trm-drain-time = Entleerzeit
trm-drain-time-hint = s zum Entleeren eines Kreislaufs; 0 = wie die Füllzeit
trm-hysteresis = Schalthysterese
trm-hysteresis-hint = km/h unter dem Schaltpunkt, bei denen zurückgeschaltet wird
trm-final-ratio = Achsgetriebe
trm-final-ratio-hint = Abtrieb : Radsatz
trm-count = Getriebe
trm-count-hint = Anzahl im Fahrzeug
trm-efficiency = Wirkungsgrad
trm-efficiency-hint = Getriebe hinter dem Kreislauf

group-circuits = Kreisläufe
circuit-converter = Wandler
circuit-coupling = Kupplung
cir-ratio = Übersetzung
cir-ratio-hint = Turbine : Abtrieb
cir-stall = Anfahrwandlung
cir-stall-hint = µ bei ν = 0
cir-coupling-point = Kupplungspunkt
cir-coupling-point-hint = ν, bei dem µ den Wert 1 erreicht hat
cir-absorption = Leistungsaufnahme λ
cir-absorption-hint = N·m/(rad/s)² bei ν = 0 — das Nennmoment der Pumpe bei Nenndrehzahl
cir-absorption-slope = λ-Verlauf
cir-absorption-slope-hint = λ(ν) = λ·(1 + Verlauf·ν) — 0 nagelt den Motor im ganzen Wandlerbereich auf eine Drehzahlparabel fest
cir-shift-up = Schaltpunkt
cir-shift-up-hint = km/h — der letzte Kreislauf übergeht ihn
cir-shift-primary = Primärbeeinflussung
cir-shift-primary-hint = km/h, um die der Schaltpunkt in der Nullstellung tiefer liegt — 0 = der Schaltpunkt hängt allein an der Geschwindigkeit
action-add-circuit = + Kreislauf

ret-absorption = Leistungsaufnahme λ
ret-absorption-hint = N·m/(rad/s)² bei voller Füllung
ret-ratio = Übersetzung
ret-ratio-hint = Rotor : Radsatz
ret-brake-force = Bremskraft
ret-brake-force-hint = N — mechanische Grenze
ret-brake-power = Bremsleistung
ret-brake-power-hint = W — was der Kühler abführen kann
ret-fill-time = Füllzeit

## Fahrzeugeditor — Sounds
##
## Die Soundtabelle des Fahrzeugs: ein Eintrag je Klang, jeder mit Auslöser,
## Bedingungen und Abhängigkeitskurven. Eine Größe ist ein Zustandswert der
## Simulation, dem der Klang folgen kann.

group-sounds = Sounds
snd-default-table = Keine eigene Tabelle — das Fahrzeug fährt auf den erzeugten Schleifen.
action-add-sound = Sound hinzufügen
action-add-sound-hint = ein Eintrag: Auslöser, Bedingungen, Abhängigkeiten
snd-name-placeholder = Name des Eintrags
snd-file-placeholder = <mod>/assets/<datei>.ogg oder synth:<name>
snd-file-hint =
    Sample unterhalb des mods-Verzeichnisses oder eine erzeugte Quelle:
    synth:rolling, synth:traction, synth:air, synth:compressor, synth:horn,
    synth:buzzer, synth:joint, synth:contactor
snd-trigger = Auslöser
snd-trigger-hint = was den Sound startet — ohne Auslöser läuft er als Schleife und wird nur moduliert
snd-trigger-loop = keiner (Schleife)
snd-trigger-rises = steigt über
snd-trigger-falls = fällt unter
snd-trigger-every = je Intervall von
snd-quantity = Größe
snd-threshold = Schwelle
snd-interval = Intervall
snd-interval-hint = löst bei jedem Vielfachen aus — 30 m Weg sind ein Schienenstoß, 1 Fahrstufe ein Schütz
snd-positional = Im Raum platziert
snd-positional-hint = mit Entfernung gedämpft und dopplerverschoben; aus heißt, der Führerstand hört ihn an fester Stelle
snd-conditions = Bedingungen
snd-conditions-hint = der Sound ist nur hörbar, solange jede Größe in ihrem Fenster liegt
snd-min = Untere Grenze
snd-max = Obere Grenze
action-add-condition = Bedingung hinzufügen
snd-volume = Lautstärke
snd-factors = Lautstärkefaktoren
snd-factors-hint = jede Kennlinie wird in die Lautstärke multipliziert — eine zweite Größe skaliert einen Eintrag, dessen Lautstärke schon einer ersten folgt, wie die Gleisrauigkeit das Rollgeräusch
action-add-factor = Faktor hinzufügen
snd-pitch = Abspielgeschwindigkeit
snd-curve-follows = folgt einer Größe
snd-curve-follows-hint = ohne Kennlinie läuft der Sound in seiner eigenen Lautstärke und Tonhöhe

snd-quantity-speed = Geschwindigkeit [km/h]
snd-quantity-distance = Zurückgelegter Weg [m]
snd-quantity-engine-rpm = Motordrehzahl [1/min]
snd-quantity-tap-changer-step = Fahrstufe Schaltwerk
snd-quantity-circuit = Wandlerkreis
snd-quantity-tractive-effort = Zugkraft [kN]
snd-quantity-brake-effort = Bremskraft [kN]
snd-quantity-brake-pipe = Hauptluftleitung [bar]
snd-quantity-brake-cylinder = Bremszylinder [bar]
snd-quantity-main-reservoir = Hauptluftbehälter [bar]
snd-quantity-air-flow = Luftstrom [bar/s]
snd-quantity-slip = Schlupfgeschwindigkeit [m/s]
snd-quantity-throttle = Fahrschalter
snd-quantity-pantograph = Stromabnehmer
snd-quantity-main-switch = Hauptschalter
snd-quantity-compressor = Kompressor
snd-quantity-doors = Türen
snd-quantity-alert = Zugsicherung meldet
snd-quantity-horn = Signalhorn
snd-quantity-roughness = Gleisrauigkeit
snd-quantity-rain = Regen

## Vehicle editor — cab
##
## Interactive 3D cab: each control binds a glTF node to a simulation input;
## the input decides whether it acts as a push button, a switch or a lever.
## The cab-input-* names double as control labels in the simulator HUD.

group-cab = Führerstand
cab-none = Noch kein Führerstand — das Modell bekommt Augpunkt und mausbedienbare Bedienelemente.
action-add-cab = Führerstand anlegen
action-add-cab-hint = Augpunkt plus mausbedienbare Bedienelemente
action-add-control = Bedienelement hinzufügen
action-add-control-hint = bindet einen glTF-Node an einen Simulationseingang
cab-eye = Augpunkt
cab-eye-hint = m im Modellraum: X rechts, Y über Schienenoberkante, −Z voraus
cab-control-node = Node
cab-control-input = Eingang
cab-control-test = Test
cab-control-test-hint = bewegt den Node in der Vorschau; wird nicht gespeichert

cab-input-throttle = Fahrschalter
cab-input-reverser = Richtungsschalter
cab-input-brake-valve = Führerbremsventil
cab-input-direct-brake = Zusatzbremse
cab-input-afb-target = AFB-Sollgeschwindigkeit
cab-input-sifa = Sifa
cab-input-pzb-acknowledge = PZB Wachsam
cab-input-pzb-exempt = PZB Frei
cab-input-pzb-override = PZB Befehl
cab-input-lzb-takeover = LZB Übernahme
cab-input-lzb-end = LZB Ende
cab-input-lzb-test = LZB Prüftaste
cab-input-horn = Makrofon
cab-input-sanding = Sanden
cab-input-brake-release = Lösetaste Lokbremse
cab-input-engine-start = Anlasser
cab-input-door-release-left = Türfreigabe links
cab-input-door-release-right = Türfreigabe rechts
cab-input-door-close = Türen schließen
cab-input-parking-brake = Feststellbremse
cab-input-ep-brake = ep-Bremse
cab-input-afb = AFB
cab-input-battery = Batterie
cab-input-pantograph = Stromabnehmer
cab-input-main-switch = Hauptschalter
cab-input-compressor = Luftpresser
cab-input-train-type = Zugartschalter
cab-input-wipers = Wischerschalter
cab-input-headlights = Spitzensignal
cab-input-cab-light = Führerraumleuchte
cab-input-display-1 = Display-Taste 1
cab-input-display-2 = Display-Taste 2
cab-input-display-3 = Display-Taste 3
cab-input-display-4 = Display-Taste 4
cab-input-display-5 = Display-Taste 5
cab-input-display-6 = Display-Taste 6
cab-input-display-7 = Display-Taste 7
cab-input-display-8 = Display-Taste 8

## Vehicle editor — displays
##
## Screens in the cab, rendered to texture: a name the script hook answers to,
## the glTF node that shows the texture, and its resolution. Content comes
## from the widget list in the file or from the vehicle script's display(ctx).

group-displays = Displays
action-add-display = Display hinzufügen
action-add-display-hint = ein in eine Textur gerenderter Bildschirm auf einem glTF-Node — Widgets oder der display(ctx)-Skript-Hook zeichnen ihn
disp-name = Name
disp-name-hint = wonach der Skript-Hook gefragt wird (ctx.display)
disp-node = Node
disp-size = Auflösung
disp-size-hint = px — Breite × Höhe der gerenderten Textur
disp-html = HTML-Datei
disp-html-hint = Pfad unterhalb von mods/ — der Bildschirm wird aus dieser HTML/CSS/JS-Seite gezeichnet statt aus Widgets oder dem Skript-Hook
disp-widgets = { $count } Widgets — werden in der Fahrzeugdatei gepflegt

## Signaleditor
##
## Ein Signalmodell ist eine Baugruppe nach dem Zusi-Muster: geteilte
## glTF-Bauteile (Mast, Schirm, Anzeiger), verkettet über Montagepunkte —
## leere Knoten namens mp_* — plus die Bindung der Lampenbild-Strings des
## Signaltyps an Knoten.

window-signal-editor = TrainSim-DE — Signaleditor
window-signal-editor-named = { $name } — TrainSim-DE Signaleditor
window-signal-editor-unsaved = • { $name } — TrainSim-DE Signaleditor
heading-signal-model = Signalmodell
filter-signal-model-ron = Signalmodell (RON)
status-new-signal-model = Neues Signalmodell
group-signal-parts = Bauteile
group-signal-lamps = Lampen
group-signal-motions = Bewegungen
group-signal-test = Lampentest
action-add-part = Bauteil hinzufügen…
action-add-lamp = Lampe hinzufügen
action-add-motion = Bewegung hinzufügen
action-lamps-off = Alle aus
sig-seconds-hint = Stellzeit des vollen Wegs [s] — 0 schaltet sofort
sig-mount = Montage
sig-mount-root = Am Signalstandort
sig-mount-node = Montagepunkt
sig-lamp = Lampenbild
sig-node = Knoten
sig-test-empty = Erst Lampen binden — der Test schaltet sie dann ohne Simulator.
help-signal-conventions = Ursprung am Fuß, +Y oben, Front zum Triebfahrzeugführer = +Z · Montagepunkte sind leere Knoten „mp_…“

## Streckeneditor

action-new-line = Neue Strecke
action-open-line = Strecke öffnen…
action-delete = Löschen
action-load-imagery = Luftbild-Konfiguration laden (F5)
action-save-imagery = Luftbild-Konfiguration speichern (F2)
overlay-toggle = Ein/aus (O)
overlay-next-provider = Nächster Anbieter (P)
overlay-offline = Offline-Modus (L)
overlay-clear-cache = Cache leeren (C)
overlay-retry = Fehlversuche zurücksetzen (R)
help-pan = WASD/Pfeile oder mittlere Maustaste schwenken · Mausrad oder Bild↑/Bild↓ Höhe
help-opacity = [ ] Deckkraft · , . Zoomstufe · Z automatisch
help-offset = Ziffernblock 4/6/8/2 Bildversatz, 5 zurücksetzen
help-draw = Gleis zeichnen: Punkte klicken · Enter schließt ab · Esc bricht ab
help-map = Mittlere Maustaste zieht, Mausrad zoomt · WASD schwenkt · links Werkzeug wählen und klicken

status-ready = Bereit
status-position = { $lat }°, { $lon }°   Höhe { $height } m
status-cache-cleared = Cache geleert
status-retry-reset = Fehlversuche zurückgesetzt
status-saved = { $file } gespeichert
status-save-failed = Speichern fehlgeschlagen: { $error }
status-not-readable = { $file } nicht lesbar
status-not-compiling = { $file } lässt sich nicht übersetzen
status-compile-error = Strecke lässt sich nicht übersetzen: { $error }
status-no-track-hit = Kein Gleis nahe dem Klick — Geräte sitzen auf einem Gleis
status-split-at-end = Zu nah am Gleisende — mindestens 1 m innerhalb klicken
status-split-failed = Weiche nicht gesetzt — die Strecke kompiliert nicht
status-ghost-loaded = Geistermodul { $file }: { $boundaries } Grenzen
status-route-derived = Weg gefunden: { $sections } Abschnitte, { $overlap } im Durchrutschweg, { $switches } Weichen
status-routes-found = { $added } Fahrstraßen angelegt, { $known } schon vorhanden
status-no-route-path = Kein Weg vom Start- zum Zielsignal — die Wirkrichtung der Signale prüfen
status-no-objects = Keine Objekte installiert — ein Mod liefert sie als objects/*.ron
status-config-unreadable = { $file } nicht lesbar ({ $error }) — Vorgabe aktiv
status-config-created = { $file } angelegt
status-config-not-writable = { $file } nicht beschreibbar: { $error }

heading-line = Strecke
line-name = Name
line-counts = { $edges } Gleise · { $devices } Geräte

heading-tools = Werkzeuge
tool-select = Auswählen
tool-select-hint = Klick auf Gerät oder Gleis wählt es aus · Entf löscht es (1)
tool-draw = Gleis zeichnen
tool-draw-hint = Klicks setzen Punkte: der erste beginnt das Gleis, jeder weitere hängt einen tangentialen Bogen an · Enter oder Rechtsklick schließt ab · Esc bricht ab (2)
tool-device = Gerät platzieren
tool-device-hint = Klick auf ein Gleis setzt die gewählte Geräteart dorthin (3)
tool-switch = Weiche setzen
tool-switch-hint = Klick auf ein Gleis teilt es dort, danach den Zweig klicken wie beim Gleiszeichnen · Enter oder Rechtsklick schließt ab, Esc bricht ab (4)
tool-object = Objekt platzieren
tool-object-hint = Klick auf ein Gleis setzt das gewählte 3D-Objekt in seinem vordefinierten Abstand und seiner Rotation dorthin (5)
tool-tree = Baum setzen
tool-tree-hint = Jeder Klick pflanzt einen Baum der gewählten Art — frei in der Fläche, ohne Gleisbezug (6)
tool-forest = Wald-Pinsel
tool-forest-hint = Klicks umreißen eine Fläche; Enter oder Rechtsklick füllt sie mit Einzelbäumen — jeder bleibt einzeln editier- und löschbar · Esc bricht ab (7)
tool-brush = Markier-Pinsel
tool-brush-hint = Linke Taste halten und überstreichen markiert Bäume und Objekte in der Fläche; Entf löscht sie gemeinsam, Esc leert die Markierung (8)
tool-marker = Marker setzen
tool-marker-hint = Jeder Klick setzt einen Referenzmarker in den genannten Layer — eine Zeichenhilfe, keine Ausstattung: die Simulation liest ihn nicht (9)
marker-layer = Layer
marker-layer-hint = Alles mit demselben Namen ist ein Layer, und Layer werden als Ganzes ausgeblendet und gelöscht
marker-label = Beschriftung
marker-label-hint = Freier Text am Marker — ein Kilometer, ein Straßenname, eine Notiz
tool-terrain = Gelände-Pinsel
tool-terrain-hint = Jeder Klick stempelt einen runden Strich ins Höhenmodell — anheben, absenken oder planieren. Das Gleis behält seine Höhe: die Striche formen den Boden, Einschnitt und Damm legen sich danach darüber (0)
terrain-radius = Radius
terrain-radius-hint = Reichweite des Strichs; er läuft am Rand auf null aus, überlappende Striche gehen also ohne Kante ineinander über
terrain-amount = Höhenänderung
terrain-amount-hint = Meter, um die die Mitte steigt (+) oder fällt (−)
terrain-target = Zielhöhe
terrain-target-hint = Ellipsoidische Höhe, auf die der Strich den Boden zieht
terrain-mode = Modus
terrain-raise = Anheben/Absenken
terrain-raise-hint = Rechnet die Höhenänderung auf das Höhenmodell auf
terrain-level = Auf Schienenhöhe
terrain-level-hint = Zieht den Boden auf die Höhe der nächsten Schiene — Bahnhofsvorfeld, Betriebsgelände, ebene Fläche
terrain-count = { $count } Striche auf dieser Strecke
sel-terrain-summary = Geländestrich { $index }
tool-tile = DGM-Kacheln
tool-tile-hint = Zeigt das Geländekachel-Raster und wählt einzelne Kacheln per Klick — grün hat schon Höhen, blau ist gewählt. Ohne Auswahl importiert der Import den ganzen Korridor
switch-orientation = Weichenlage
switch-facing = Spitz befahren
switch-facing-hint = Der Zweig geht in Laufrichtung des geklickten Gleises ab — ein Zug von dort fährt spitz auf die Weiche zu
switch-trailing = Stumpf befahren
switch-trailing-hint = Der Zweig läuft am geklickten Gleis zurück — ein Zug von dort befährt die Weiche stumpf, und die andere Hälfte der Teilung wird zur Wurzel
draw-active = Zeichnen: { $segments } Segmente — Enter oder Rechtsklick schließt ab, Esc bricht ab
draw-branch = Zweiggleis: { $segments } Segmente — Enter oder Rechtsklick verdrahtet die Weiche, Esc bricht ab
forest-active = Waldfläche: { $corners } Eckpunkte — Enter oder Rechtsklick schließt sie, Esc bricht ab

heading-selection = Auswahl
sel-none = Nichts ausgewählt — das Auswahlwerkzeug greift Geräte und Gleise.
sel-edge-summary = Gleis { $index }: { $length } m, { $segments } Segmente
sel-edge-devices = { $devices } Geräte auf diesem Gleis
sel-edge-handles = Die runden Griffe auf der Karte verschieben — so biegt sich das Gleis.
sel-edge-fixed = Übergangsbögen — die Stützpunkte dieses Gleises sind nicht editierbar.
sel-track-type = Gleisart
sel-track-type-none = Standard-Gleisart auf dem ganzen Gleis.
sel-track-type-from = Ab dieser Position auf dem Gleis
sel-track-type-hint = Jede Zeile: ab Position s gilt diese Gleisart — Textur, Rauigkeit und Oberbau-Geschwindigkeit kommen aus track_types/*.ron eines Mods
track-type-default = (Standard)
action-add-type-section = Gleisart-Abschnitt hinzufügen
sel-switch = Weiche
sel-switch-node = Knoten { $node } ({ $leg }), Umlaufzeit
sel-switch-hint = Wie lange der Antrieb von einer Lage in die andere braucht; eine Fahrstraße hält die Weiche so lange verschlossen
switch-leg-root = Wurzel
switch-leg-straight = Stammgleis
switch-leg-diverging = Zweiggleis
sel-device-summary = Gerät { $index } auf Gleis { $edge }
dev-kind = Geräteart
dev-s = Position
dev-s-hint = Abstand vom Gleisanfang
dev-facing = Wirkrichtung
dev-facing-hint = Fahrtrichtung, in der das Gerät wirkt
dev-lateral = Seitlicher Versatz
dev-lateral-hint = Versatz rechts der Gleisachse; Signalmasten stehen üblicherweise bei 3,5 m
dev-payload = Nutzdaten
dev-payload-hint = Länderspezifische Daten als RON-Text — z. B. (frequency:Hz1000,signal:Some(0)) für einen Magneten, (name:"Musterstadt",length:210.0) für einen Bahnsteig
facing-forward = Vorwärts
facing-backward = Rückwärts
facing-both = Beide
sel-signal = Signaltabelle
sel-signal-hint = Erst ein Gerät mit Eintrag hier ist für das Stellwerk ein Signal — der Eintrag trägt Art, System, was es ankündigt und was es deckt
action-add-signal = Signaleintrag anlegen
action-delete-signal = Signaleintrag löschen
action-delete-signal-hint = Das Gerät bleibt; Fahrstraßen über dieses Signal und Verweise darauf gehen mit dem Eintrag
signal-label = { $index }: { $kind } (Gerät { $device })
sig-kind = Art
sig-kind-main = Hauptsignal
sig-kind-distant = Vorsignal
sig-kind-combined = Kombinationssignal (Ks)
sig-kind-shunting = Rangiersignal
sig-kind-track-lock = Gleissperre
sig-system = System
sig-system-hint = Signalsystem des Schirms — H/V, Ks oder Hl
sig-next = Kündigt an
sig-next-hint = Das Signal, dessen Begriff dieses ankündigt; ein Vorsignal braucht es
sig-requires-route = Braucht Fahrstraße
sig-requires-route-hint = Bleibt in Halt, bis eine Fahrstraße eingestellt ist — Bahnhofssignale ja, Blocksignale nein
sig-diverging-speed = Ablenkgeschwindigkeit
sig-diverging-speed-hint = Geschwindigkeit im ablenkenden Strang (Zs3); ohne sie zeigt das Signal keinen Geschwindigkeitsanzeiger
sig-type = Signaltyp
sig-type-hint = "<mod>:<name>" aus signal_types/*.ron — der Begriff kommt dann aus dieser Regeltabelle
sig-model = 3D-Modell
sig-model-hint = "<mod>:<name>" unter signal_models/ — schlägt das Modell des Signaltyps
sig-guarded = Deckt Abschnitte
sig-routes = Fahrstraßen von hier
sig-routes-none = An diesem Signal beginnt keine Fahrstraße — ein Signal, das eine braucht, bleibt ohne sie in Halt.
sig-route-row = → { $exit } · { $sections } Abschnitte, { $switches } Weichen
action-edit-route = Bearbeiten
action-find-routes = Fahrstraßen suchen
action-find-routes-hint = Läuft über das Gleis hinaus und bietet für jeden Strang jeder Weiche voraus eine Fahrstraße an, die jeweils am nächsten Signal endet. Schon vorhandene Fahrstraßen bleiben, wie sie sind.
sel-object-summary = Objekt { $index } auf Gleis { $edge }
obj-kind = Objekt
obj-kind-hint = 3D-Objekt aus einem Mod (objects/*.ron) — es bringt seinen eigenen Standard-Abstand und die Standard-Rotation mit
obj-s = Position
obj-s-hint = Abstand vom Gleisanfang
obj-lateral = Seitlicher Abstand
obj-lateral-hint = Positiv = rechts der Laufrichtung steigender Position
obj-yaw = Rotation
obj-yaw-hint = Um die Hochachse, im Uhrzeigersinn von oben; 0° = Front in Gleisrichtung
obj-height = Höhe
obj-height-hint = Über der Schienenoberkante — bei „Auf Gelände setzen" stattdessen über dem Gelände
obj-snap = Auf Gelände setzen
obj-snap-hint = Setzt den Fußpunkt des Objekts auf die Geländeoberfläche statt auf die Schienenebene; aufgelöst in der App, die die Höhendaten hat
check-object-off-edge = Objekt { $object } liegt außerhalb seines Gleises
check-unknown-object = Objekt { $object }: nennt ein Objekt, das kein installierter Mod hat
check-flank-guard = Fahrstraße { $route }: Flankenschutz nennt einen Knoten, der keine Weiche ist, oder ein Signal, das es nicht mehr gibt
obj-repeat = Wiederholung
obj-repeat-interval = Abstand
obj-repeat-interval-hint = Alle so viele Meter eine Kopie; Oberleitungsmasten stehen üblicherweise 65 m auseinander
obj-repeat-until = Bis Position
obj-repeat-until-hint = Ende der Reihe, gemessen entlang des Gleises; am Gleisende ist ohnehin Schluss
action-repeat-object = In Reihe wiederholen
obj-repeat-hint = Setzt { $count } Kopien mit Abstand, Rotation und Höhe dieses Objekts — jede bleibt einzeln editierbar
obj-repeat-empty = Nichts passt: der Abstand läuft über die Endposition oder das Gleisende hinaus.
sel-tree-summary = Baum { $index }
veg-species = Art
veg-species-hint = 3D-Objekt aus einem Mod (objects/*.ron); der Platzhalter ist der eingebaute Baum der App
veg-placeholder = (Platzhalter-Baum)
tree-yaw = Rotation
tree-yaw-hint = Um die Hochachse, im Uhrzeigersinn von oben
tree-scale = Skalierung
tree-scale-hint = Einheitlicher Faktor auf die Eigengröße des Objekts
forest-area = Fläche je Baum
forest-area-hint = Ein gebackener Baum je so viele m² — kleiner ist dichter
brush-radius = Pinselradius
brush-radius-hint = Alles, dessen Position beim Überstreichen im Kreis liegt, wird markiert
brush-marked = { $count } markiert
action-delete-marked = Markierte löschen
action-clear-marked = Markierung leeren
status-forest-points = Ein Wald braucht mindestens drei Eckpunkte
status-forest-baked = { $count } Bäume gesetzt — jeder bleibt einzeln editierbar
action-import-forest = Wald importieren…
filter-overpass-json = Overpass-Auszug (JSON)
status-forest-imported = { $count } Bäume aus { $areas } Waldflächen gesetzt — jeder bleibt einzeln editierbar
status-forest-import-empty = { $file }: keine Wege mit landuse=forest oder natural=wood gefunden
action-import-markers = Referenzmarker importieren…
action-delete-layer = Layer löschen
status-markers-imported = { $count } Referenzmarker in { $layers } Layern — layerweise ausblendbar und löschbar
status-marker-import-empty = { $file }: keine auswertbaren Objekte für Referenzmarker gefunden
marker-none = Noch keine Referenzmarker — das Marker-Werkzeug setzt sie, Datei ▸ Referenzmarker importieren holt sie aus OSM
marker-total = { $count } Marker insgesamt
sel-marker-summary = Marker { $index }
action-center = Ansicht zentrieren
action-payload-template = Vorlage einsetzen
action-reset = Zurücksetzen
kind-signal = Signal
kind-magnet = PZB-Magnet
kind-line-conductor = Linienleiter (LZB)
kind-balise = Balise
kind-speed-board = Geschwindigkeitstafel
kind-platform = Bahnsteig
kind-stop-board = Haltetafel
kind-block-marker = Blockstelle
kind-neutral-section = Schutzstrecke
heading-interlock = Stellwerk
il-sections = Abschnitte
il-sections-none = Noch keine Abschnitte — ein Abschnitt ist die Menge Gleise, die zusammen als besetzt gelten.
il-sections-hint = Legt einen Abschnitt aus dem gewählten Gleis an; Blockkennzeichen, Deckungslisten und Fahrstraßen sprechen ihn über seinen Index an
il-section-row = Abschnitt { $index }
action-add-section = Abschnitt anlegen
action-add-track = + Gleis
il-add-track-hint = Erst ein Gleis auswählen — ein Abschnitt besteht aus Gleisen.
il-routes = Fahrstraßen
il-routes-none = Noch keine Fahrstraßen — ohne eine bleibt ein Signal, das eine braucht, in Halt.
il-routes-need-signals = Zwei Signaleinträge werden gebraucht, damit eine Fahrstraße von einem zum anderen laufen kann.
il-routes-hint = Legt eine Fahrstraße zwischen den ersten beiden Signalen an; Start, Ziel, Abschnitte und Weichen werden darunter gesetzt
route-entry = Startsignal
route-exit = Zielsignal
route-diverging = Ablenkende Fahrstraße
route-diverging-hint = Die Fahrstraße läuft über das Zweiggleis — das Startsignal zeigt den langsamen Begriff
route-sections = Abschnitte
route-overlap = Durchrutschweg
route-overlap-length = Länge des D-Wegs
route-overlap-length-hint = Wie weit die Ableitung hinter dem Zielsignal weiterläuft. Die erreichten Abschnitte werden zum Durchrutschweg; Weichen darin verschließt die Fahrstraße mit.
overlap-by-rule = Regellänge
overlap-by-rule-hint = Der Regel-Durchrutschweg nach deutschem Regelwerk, nach der Geschwindigkeit am Ende der Fahrstraße: 50 m bis 30 km/h, 100 m bis 60, 200 m bis 100, darüber 300 m. Eine ablenkende Fahrstraße zählt mit der Ablenkgeschwindigkeit des Startsignals. Ausschalten, um eine eigene Länge zu setzen.
route-switches = Weichen
route-flank = Flankenschutz
route-flank-hint = Was ein Fahrzeug dort vom Fahrweg fernhält, wo ein Gleis einmündet: eine Weiche in abweisender Lage oder ein Signal, das in Halt gehalten wird, solange die Fahrstraße steht
flank-switch = W{ $node } { $position }
flank-signal = { $signal } in Halt
flank-signal-hint = Bleibt in Halt, solange die Fahrstraße steht; von ihm lässt sich währenddessen keine Fahrstraße einstellen
flank-add-switch = + Weiche
flank-add-signal = + Signal
route-switch-hint = Klick kippt die verlangte Lage
switch-straight = Grundstellung
switch-diverging = abzweigend
action-add-route = Fahrstraße anlegen
action-derive-route = Weg ableiten
action-derive-route-hint = Verfolgt das Gleis vom Start- zum Zielsignal und trägt die durchfahrenen Abschnitte, die verlangte Lage jeder Weiche auf dem Weg und den Durchrutschweg hinter dem Zielsignal ein.
action-delete-route = Fahrstraße löschen
heading-heights = Höhendaten (DGM)
dgm-source = Datenlieferung
dgm-source-hint = Verzeichnis des DGM der Landesvermessung, aus dem die Kacheln des Moduls geschnitten werden
dgm-zone = UTM-Zone
dgm-zone-hint = 32 westlich, 33 östlich von 12° O — sie steckt im EPSG-Code der Lieferung
dgm-cell = Rasterweite
dgm-cell-hint = Weite, mit der die Kacheln des Moduls geschrieben werden. 10 m liegt deutlich unter dem, was das Gelände am Gleis baut, und ist ein Bruchteil der Liefergröße; 1 m nimmt die Originalauflösung mit
dgm-coverage = { $have } von { $total } Korridorkacheln haben Höhendaten
dgm-picked = { $count } Kacheln gewählt
action-choose-dgm = Lieferung wählen…
action-import-heights-all = Ganzes Modul importieren
action-import-heights-picked = Gewählte Kacheln importieren
action-clear-picked = Auswahl leeren
action-drop-heights = Verweis entfernen
status-heights-need-mod = Die Strecke zuerst in einem Mod speichern — die Höhendaten liegen daneben, im Mod
status-heights-no-source = Noch keine DGM-Lieferung gewählt
status-heights-imported = { $tiles } Höhenkacheln geschrieben, { $empty } ohne Daten übersprungen — das Modul bringt sein Gelände jetzt mit

heading-markers = Referenzmarker

heading-module = Modul
module-boundaries = Grenzen
boundary-none = Noch keine Grenzen — ein Gleis auswählen und an einem offenen Ende eine anlegen.
boundary-node = Knoten { $node }
boundary-select-edge = Ein Gleis auswählen, um an einem offenen Ende eine Grenze anzulegen.
boundary-taken = Dieser Knoten trägt bereits eine Grenze.
boundary-needs-buffer = Grenzen sitzen nur auf offenen Enden (Prellbock-Knoten).
action-add-boundary-start = Grenze am Gleisanfang
action-add-boundary-end = Grenze am Gleisende
module-ghost = Nachbarmodul
module-ghost-hint = Ein weiteres Modul, grau als Geist gezeichnet. Seine Grenzkreise sind Fangpunkte — Klicks in ihrer Nähe landen exakt auf den vereinbarten Koordinaten.
action-load-ghost = Geist laden…
action-clear-ghost = Entfernen
ghost-boundaries = { $count } Grenzen als Fangpunkte
heading-checks = Prüfung
check-ok = Keine Befunde.
check-device-off-edge = Gerät { $device } liegt außerhalb seines Gleises
check-magnet-payload = Gerät { $device }: Magnet-Payload ungültig oder nennt ein fehlendes Signal
check-blockmarker-payload = Gerät { $device }: Blockkennzeichen-Payload ungültig oder nennt einen fehlenden Abschnitt
check-distant-no-1000hz = Signal { $signal }: kein 1000-Hz-Magnet mit dem Vorsignal verknüpft
check-main-no-2000hz = Signal { $signal }: kein 2000-Hz-Magnet mit dem Hauptsignal verknüpft
check-distant-no-next = Signal { $signal }: Vorsignal kündigt nichts an (next fehlt)
check-signal-device = Signal { $signal }: Gerät fehlt oder ist kein Signalgerät
check-boundary-invalid = Grenze { $boundary }: Knoten fehlt oder ist kein Prellbock
check-unknown-track-type = Gleis { $edge }: nennt eine Gleisart, die kein installierter Mod hat
check-lzb-no-conductor = Gleis { $edge }: LZB-Gleisart, aber die Strecke verlegt keinen Linienleiter
heading-imagery = Luftbilder
img-enabled = Overlay anzeigen
img-provider = Anbieter
img-opacity = Deckkraft
img-zoom = Zoom
img-offset = Versatz
img-offset-hint = Ost/Nord — Luftbilder liegen oft Meter neben der Karte
img-offline = Offline-Modus
img-offline-hint = Kacheln kommen nur aus dem Cache — nichts wird geladen
zoom-fixed = feste Stufe
zoom-auto = automatisch
zoom-current = Stufe { $level }
tiles-summary = { $shown } gezeigt, { $pending } unterwegs
heading-cache = Cache
cache-summary = { $hits } Treffer ({ $disk } von der Platte), { $stored } abgelegt, { $evicted } verworfen
cache-size = { $megabytes } MB in { $directory }
group-errors = Fehler

## Simulator-Anzeige

hud-speed = v = { $speed } km/h   zul. { $limit } km/h   Weg { $distance } m   { $time }
hud-brakes = HL { $pipe } bar   C { $cylinder } bar   R { $auxiliary } bar   HB { $main } bar   Zusatz { $direct } bar   Luft { $air } Nl
hud-traction = Fahrschalter { $throttle }   Zugkraft { $tractive } kN   Bremskraft { $braking } kN   Bremse { $valve }
hud-afb = AFB { $state }   Ziel { $target } km/h
hud-electrics = Batterie { $battery }   Bügel { $pantograph } %   Hauptschalter { $switch }   Fahrdraht { $voltage } V   Federspeicher { $parking }
hud-tap = Fahrstufe { $step }/{ $steps }   Motorstrom { $current } A   Feld { $field } %   E-Bremse { $force } kN
hud-diesel = Motor { $rpm } 1/min   Füllung { $fill } %   Wandler { $circuit }   ν { $nu }   Retarder { $retarder } %
hud-dynamic = E-Bremse { $force } kN
hud-protection = Zugsicherung: { $action }   Überwachung { $limit }   LM: { $lamps }
hud-pzb = { $variant }   Zugart { $category }{ $note }
hud-pzb-restrictive = {"   "}restriktiv
hud-pzb-selftest = {"   "}Funktionsprüfung: { $phase }
hud-lzb-selftest = LZB Funktionsprüfung: { $phase } (B = quittieren)
hud-lzb = LZB { $mode } { $block }{ $cirelke }: v-Soll { $permitted }   v-Ziel { $target }   Zielentfernung { $distance } m
hud-signals = Signale: { $aspects }
hud-terrain = Gelände: { $tiles } Kacheln geladen (+{ $pending } in Arbeit), { $triangles } Dreiecke, { $megabytes } MB, Sichtweite { $view } m
hud-scenario = { $number } — { $name }
hud-scenario-passed = Szenario bestanden
hud-scenario-failed = Szenario gescheitert
hud-outcome = { $result }: { $reason }
hud-score = Wertung { $total } | Zwangsbremsungen { $forced } | { $energy } kWh
hud-control = { $name }: { $value } %
hud-keys-drive = W/S Fahrschalter  A/D Bremse  E Schnellbremsung  Q Abschluss  Z Füllen  C/V Zusatzbremse  Y Wischer  9/0 Licht  6 AFB  7/8 AFB-Ziel
hud-keys-safety = Leertaste Sifa  Bild↓ Wachsam  Ende Frei  Entf Befehl  N/M/B LZB  U Zugart  1–4 Aufrüsten  F1–F3 Kamera  F9 Mods

## Hauptmenü

menu-start = Fahrt starten
menu-mods = Mods
menu-quit = Beenden
menu-keys = ↑/↓ auswählen   Enter bestätigen

## Mod-Verwaltung

mods-title = Mods
mods-none = Keine Mods installiert — ein Mod-Verzeichnis nach mods/ legen.
mods-missing-depends = benötigt: { $depends } (fehlt oder ist abgeschaltet)
mods-content = Inhalte: { $vehicles } Fahrzeuge, { $lines } Strecken, { $compositions } Kompositionen, { $scenarios } Szenarien, { $timetables } Fahrpläne, { $signals } Signaltypen, { $scripts } Skripte
mods-log = Warnungen:
mods-restart = Änderung wirkt nach einem Neustart.
mods-keys = ↑/↓ auswählen   Enter ein/aus   F9 schließen
mods-keys-menu = ↑/↓ auswählen   Enter ein/aus   Esc zurück

## Wertung

score-summary = Wertung: { $total } von { $base }
score-stop-missed = Haltepunkt { $stop } um { $metres } m verfehlt
score-timetable = { $stop } { $minutes } min gegenüber dem Fahrplan
score-forced-brakes = { $count } Zwangsbremsung(en)
score-overspeed = { $seconds } s zu schnell (max. { $excess } km/h)
score-energy = { $energy } kWh Traktionsenergie
score-scenario = Szenariowertung
