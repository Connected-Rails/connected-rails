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
draw-active = Zeichnen: { $segments } Segmente — Enter oder Rechtsklick schließt ab, Esc bricht ab

heading-selection = Auswahl
sel-none = Nichts ausgewählt — das Auswahlwerkzeug greift Geräte und Gleise.
sel-edge-summary = Gleis { $index }: { $length } m, { $segments } Segmente
sel-edge-devices = { $devices } Geräte auf diesem Gleis
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
