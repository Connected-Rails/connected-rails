# Connected Rails — deutsche Übersetzung.
#
# Schlüssel auf -hint sind der Tooltip des gleichnamigen Feldes.
# Platzhalter wie { $file } füllt das Programm; sie müssen erhalten bleiben.

## Fenster und Menüs

window-simulator = Connected Rails
window-vehicle-editor = Connected Rails — Fahrzeugeditor
window-vehicle-editor-named = { $name } — Connected Rails Fahrzeugeditor
window-vehicle-editor-unsaved = • { $name } — Connected Rails Fahrzeugeditor
window-route-editor = Connected Rails — Streckeneditor
window-route-editor-named = { $name } — Connected Rails Streckeneditor
window-route-editor-unsaved = • { $name } — Connected Rails Streckeneditor

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
motion-glow = leuchten

## Fahrzeugeditor — Bremse

group-brake = Bremse
brk-valve = Steuerventil
brk-valve-hint = welches Steuerventil verbaut ist
brk-default-position = Bremsstellung (Grundstellung)
brk-default-position-hint = Stellung des Umstellhebels, wenn das Fahrzeug in Betrieb geht — gesetzt wird sie am Zug. G Güterzug · P Personenzug · R Schnellzug; R mit Magnetschienenbremse ergibt die Anschrift „R + Mg“
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
brk-mg-hint = Ausrüstung des Fahrzeugs. Sie wirkt in Bremsstellung R — zusammen ergibt das die Anschrift „R + Mg“
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
traction-steam = Dampf
drv-mode-electric = Elektrisch
drv-mode-diesel = Diesel
drv-mode-steam = Dampf
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
trm-power-control = Leistungssteuerung
trm-power-control-hint = Woher die Teillast kommt: aus der Füllung des Kreislaufs (Voith) oder aus der Motordrehzahl, bei schlicht gefülltem Kreislauf (Mekydro)
trm-power-control-filling = Füllung
trm-power-control-engine-speed = Motordrehzahl
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
trm-shunting-ratio = Rangiergang
trm-shunting-ratio-hint = Achsübersetzung der Rangierstufe eines Zweigang-Wendegetriebes (V 60, V 90); 0 = keine. Umschaltbar nur im Stillstand
gbx-gears = Gangübersetzungen
gbx-gears-hint = Motor : Getriebeabtrieb, erster Gang zuerst
gbx-clutch-torque = Kupplungsmoment
gbx-clutch-torque-hint = N·m, die der Belag hält, bevor er rutscht — damit fährt das Fahrzeug an
gbx-clutch-time = Kupplungszeit
gbx-clutch-time-hint = s für den vollen Weg der Kupplung
gbx-shift-time = Schaltzeit
gbx-shift-time-hint = s für Auskuppeln, Gang, Einkuppeln — die Lücke in der Zugkraft
gbx-shift-up = Hochschaltdrehzahl
gbx-shift-down = Rückschaltdrehzahl
gbx-shift-up-hint = 1/min, bei denen hochgeschaltet wird
gbx-shift-down-hint = 1/min, unter denen wieder zurückgeschaltet wird
hst-max-force = Zugkraftgrenze
hst-max-force-hint = N, die das Druckbegrenzungsventil zulässt — der flache Teil der Kennlinie
hst-response-time = Verstellzeit
hst-response-time-hint = s für den vollen Schwenkweg der Pumpe
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
    synth:rolling-low, synth:rolling-mid, synth:rolling-high, synth:traction-low,
    synth:traction-mid, synth:traction-high, synth:air, synth:compressor,
    synth:horn, synth:buzzer, synth:squeal, synth:joint, synth:contactor
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

# Die Vorschau spielt einen Eintrag über das eigene Ausgabegerät des Editors und
# lässt die Größen, von denen er abhängt, durchfahren.
snd-preview = Vorschau
snd-preview-hint = spielt diesen Eintrag; die Regler darunter fahren die Größen durch, von denen er abhängt
snd-preview-stop = Stopp
snd-preview-no-device = kein Audio-Ausgabegerät
snd-preview-level = Lautstärke { $volume } · Geschwindigkeit { $pitch }
snd-preview-failed = Nicht abspielbar: { $error }
snd-preview-not-scrubbable = folgt einem Führerraumbedienteil — nur im Fahrbetrieb hörbar

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
cab-input-road-gear = Gangschalter (Rangier-/Streckengang)
cab-input-train-type = Zugartschalter
cab-input-wipers = Wischerschalter
cab-input-headlights = Spitzensignal
cab-input-cab-light = Führerraumleuchte
cab-input-instrument-light = Instrumentenbeleuchtung
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

window-signal-editor = Connected Rails — Signaleditor
window-signal-editor-named = { $name } — Connected Rails Signaleditor
window-signal-editor-unsaved = • { $name } — Connected Rails Signaleditor
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
action-show-terrain = Gelände anzeigen (T)
action-perspective-view = 3D-Ansicht (F4)

# Viewport bar — icon buttons above the map, tooltips only.
view-top-down = Draufsicht (F4)
view-perspective = 3D-Ansicht (F4)
gizmo-move = Verschiebegriffe (W)
gizmo-rotate = Drehgriff (E)
view-imagery = Luftbild (T)
view-terrain = Gelände (T)
camera-speed = Kamerageschwindigkeit der 3D-Ansicht

help-pan = WASD/Pfeile oder mittlere Maustaste schwenken · Mausrad oder Bild↑/Bild↓ Höhe
help-fly = 3D-Ansicht (F4): rechte Maustaste halten zum Umsehen, WASD fliegt, Q/E runter/hoch, Umschalt schneller · Alt+links umkreist · F rückt die Auswahl ins Bild
help-gizmo = W Verschiebegriffe, E Drehgriff · Pfeil ziehen verschiebt die Auswahl längs des Gleises, quer dazu oder nach oben
help-opacity = [ ] Deckkraft · , . Zoomstufe · Z automatisch
help-offset = Ziffernblock 4/6/8/2 Bildversatz, 5 zurücksetzen
help-draw = Gleis zeichnen: Punkte klicken · Enter schließt ab · Esc bricht ab
help-map = Mittlere Maustaste zieht, Mausrad zoomt · WASD schwenkt · links Werkzeug wählen und klicken
help-terrain = T zeigt das Gelände des Moduls, so wie es die Fahrt baut, anstelle des Luftbilds

status-ready = Bereit
status-position = { $lat }°, { $lon }°   Höhe { $height } m
status-ground-height = Boden { $height } m
status-terrain-flat = Noch keine Höhendaten — der Boden ist flach; unter Höhendaten ein DGM importieren
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
line-power = Elektrifizierung
line-power-hint = Was über dieser Strecke hängt, wo ein Gleis nichts sagt

heading-tools = Werkzeuge
tool-select = Auswählen
tool-select-hint = Klick auf Gerät oder Gleis wählt es aus · Entf löscht es (1)
tool-draw = Gleis zeichnen
tool-draw-hint = Klicks setzen Punkte: der erste beginnt das Gleis, jeder weitere hängt einen tangentialen Bogen an · Enter oder Rechtsklick schließt ab · Esc bricht ab (2)
tool-device = Gerät platzieren
tool-device-hint = Klick auf ein Gleis setzt die gewählte Geräteart dorthin (3)
tool-switch = Weiche setzen
tool-area = Bereich markieren
tool-area-hint = Einen Strich am Gleis entlang ziehen und dem Abschnitt Eigenschaften geben
tool-area-drag = Auf einem Gleis drücken und daran entlangziehen. Der Strich folgt diesem Gleis, bis die Taste losgelassen wird.
tool-area-joins = Kommt zu { $name } dazu.
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
heading-areas = Gleisbereiche
sel-none = Nichts ausgewählt — das Auswahlwerkzeug greift Geräte und Gleise.
sel-edge-summary = Gleis { $index }: { $length } m, { $segments } Segmente
sel-edge-devices = { $devices } Geräte auf diesem Gleis
sel-edge-handles = Die runden Griffe auf der Karte verschieben — so biegt sich das Gleis.
sel-edge-covered = Über diesem Gleis liegen markierte Bereiche: { $areas }. Wo sie eine Eigenschaft setzen, gewinnen sie.
sel-edge-fixed = Übergangsbögen — die Stützpunkte dieses Gleises sind nicht editierbar.
sel-track-type = Gleisart
sel-track-type-none = Standard-Gleisart auf dem ganzen Gleis.
sel-track-type-from = Ab dieser Position auf dem Gleis
sel-track-type-hint = Jede Zeile: ab Position s gilt diese Gleisart — Textur, Rauigkeit und Oberbau-Geschwindigkeit kommen aus track_types/*.ron eines Mods
sel-power = Elektrifizierung
sel-power-default = Folgt der Strecke: { $system }
sel-power-from = Ab dieser Bogenlänge
sel-power-hint = Ein eigener Abschnitt — die Lücke an einer Systemtrennstelle oder ein Gleis ohne Fahrdraht
sel-area-summary = { $spans } Abschnitte, { $length } m Gleis
sel-area-sets-nothing = Setzt noch nichts — bisher nur eine Markierung
sel-area-properties = Eigenschaften
sel-area-spans = Abschnitte
sel-area-no-spans = Kein Abschnitt — auf der Karte einen markieren
sel-area-span-track = Gleis { $index }
sel-area-span-from = Ab dieser Bogenlänge
sel-area-span-to = Bis zu dieser Bogenlänge
sel-area-list-empty = Keine markierten Bereiche
sel-area-list-covers = { $length } m
area-name = Name
area-color = Farbe
area-width = Strichbreite
area-speed = Zulässige Geschwindigkeit
area-cant = Überhöhung
area-grade = Neigung
area-track-type = Gleisbauart
area-unset = nicht gesetzt
area-unnamed = Unbenannter Bereich
area-set-hint = Angehakt setzt der Bereich die Eigenschaft; ohne Haken lässt er liegen, was darunter steht
area-default-name = Bereich { $index }
action-add-area = Neuen Bereich malen
action-add-area-hint = Auf einem Gleis drücken und daran entlangziehen
action-mark-more = Weiteren Strich ziehen
action-mark-more-hint = Der nächste Strich kommt zu diesem Bereich dazu, statt einen neuen zu öffnen
status-area-too-short = Zu kurz — am Gleis entlangziehen
power-none = Ohne Fahrdraht
action-add-power-section = Elektrifizierungsabschnitt hinzufügen
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
check-area-off-track = Gleisbereich { $area }: ein Abschnitt liegt auf keinem Gleis oder hinter dessen Ende
check-area-no-effect = Gleisbereich { $area }: deckt nichts ab oder setzt nichts — er erreicht die Strecke nicht
check-area-track-type = Gleisbereich { $area }: nennt eine Gleisbauart, die kein installiertes Mod kennt
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
hud-network = Server { $state }   Zug { $train }   Latenz { $latency } ms   Korrektur { $correction } cm
hud-network-joined = verbunden
hud-network-connecting = verbinde …
hud-brakes = HL { $pipe } bar   C { $cylinder } bar   R { $auxiliary } bar   HB { $main } bar   Zusatz { $direct } bar   Luft { $air } Nl
hud-traction = Fahrschalter { $throttle }   Zugkraft { $tractive } kN   Bremskraft { $braking } kN   Bremse { $valve }
hud-afb = AFB { $state }   Ziel { $target } km/h
hud-electrics = Batterie { $battery }   Bügel { $pantograph } %   Hauptschalter { $switch }   Fahrdraht { $voltage } V   Federspeicher { $parking }
hud-tap = Fahrstufe { $step }/{ $steps }   Motorstrom { $current } A   Feld { $field } %   E-Bremse { $force } kN
hud-diesel = Motor { $rpm } 1/min   Füllung { $fill } %   Wandler { $circuit }   ν { $nu }   Retarder { $retarder } %
hud-dynamic = E-Bremse { $force } kN
hud-axles = Achsen { $slipping }/{ $axles } { $state }, schlimmste { $worst } m/s
hud-axles-spinning = schleudern
hud-axles-sliding = gleiten
hud-generator = Generator { $voltage } V   { $current } A   Leistungsregler { $regulator } %
hud-boiler = Kessel { $pressure } bar   Wasserstand { $glass } %   Feuer { $fire } %   Rost { $coal } kg
hud-tender = Tender { $water } l Wasser   { $coal } kg Kohle   Abblasen { $blowing }
hud-temperature = Motoren { $motor } °C   Widerstände { $resistor } °C
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
hud-keys-walk = WASD Gehen  Umschalt Laufen  Maus Umsehen  E Ein-/Aussteigen  Linksklick Bedienen  F1 zurück zum Sitz
hud-keys-drive = W/S Fahrschalter  A/D Bremse  E Schnellbremsung  Q Abschluss  Z Füllen  C/V Zusatzbremse  Y Wischer  6 AFB  7/8 AFB-Ziel
hud-keys-safety = Leertaste Sifa  Bild↓ Wachsam  Ende Frei  Entf Befehl  N/M/B LZB  U Zugart  1–4 Aufrüsten  F1–F4 Kamera  F9 Mods
hud-keys-lights = 9 Spitzensignal  0 Führerraumlicht  ,/. Instrumentenlicht

## Hauptmenü
#
# Die Navigationsspalte links, danach die Seiten, die sie öffnet. Ein -hint-Schlüssel
# ist die zweite, blassere Zeile unter dem gleichnamigen Eintrag.

menu-tagline = Deutsche Eisenbahnsimulation
menu-drive = Fahren
menu-drive-hint = Strecke, Fahrzeug und Szenario
menu-mods = Mods
menu-mods-hint = Inhalte ein- und ausschalten
menu-settings = Einstellungen
menu-settings-hint = Bild, Ton und Steuerung
menu-quit = Beenden
menu-quit-hint = Den Simulator verlassen
# Die Esc-Überlagerung über einer stehenden Fahrt.
menu-paused = Pause
menu-resume = Fortsetzen
menu-resume-hint = Weiter, wo der Zug steht
menu-step = Schritt { $step } von { $total }
menu-select-line = Strecke auswählen
menu-select-line-hint = Wo die Fahrt stattfindet.
menu-select-loco = Fahrzeug auswählen
menu-select-loco-hint = Was an der Spitze des Zuges läuft.
menu-select-scenario = Szenario auswählen
menu-select-scenario-hint = Ein Fahrplan mit Aufgabe — oder freie Hand.
# Die eingebauten Inhalte, auf die der Simulator zurückfällt, wenn nichts gewählt wird.
# Das Fähnchen an der Zeile sagt das, der Name selbst muss es nicht mehr.
menu-chip-builtin = Integriert
menu-chip-composition = Komposition
menu-line-builtin = Beispielstrecke
menu-loco-builtin = BR 101
menu-scenario-none = Kein Szenario — freie Fahrt
menu-free-run = Kein Fahrplan und keine Wertung: die Strecke, das Fahrzeug, und wohin Sie damit fahren.
# Die Tastenhinweise in der Fußleiste: ein Fähnchen je Taste.
menu-hint-select = auswählen
menu-hint-confirm = bestätigen
menu-hint-toggle = ein/aus
menu-hint-start = Fahrt starten
menu-hint-change = ändern
menu-hint-next = nächster Wert
menu-hint-back = zurück
menu-hint-open = öffnen
menu-hint-resume = fortsetzen
# Die Schaltfläche am Fuß der Detailspalte — dasselbe, was Enter tut.
menu-action-next = Weiter
menu-action-start = Fahrt starten
# Die zweite Zeile einer Zeile — Werte aus dem Inhalt selbst.
menu-meta-line = { $length } km · { $signals } Signale
menu-meta-vehicle = { $mass } t · { $speed } km/h
# Die Detailspalte neben der Liste.
menu-fact-length = Länge
menu-fact-signals = Signale
menu-fact-scenery = Streckenobjekte
menu-fact-drive = Antrieb
menu-fact-brake = Bremse
menu-fact-start = Beginn
menu-fact-timetable = Fahrplan
menu-fact-line = Strecke
menu-fact-events = Ereignisse
menu-fact-km = { $value } km
menu-fact-m = { $value } m
menu-fact-t = { $value } t
menu-fact-kmh = { $value } km/h

## Einstellungen
#
# Eine Seite des Hauptmenüs, als TOML im Einstellungsverzeichnis des Betriebssystems
# abgelegt. Ein -hint-Schlüssel ist die Beschreibung unter dem Namen der Einstellung.

set-graphics = Grafik
set-audio = Ton
set-gameplay = Spiel
set-stored = Bleibt zwischen zwei Starts in der Einstellungsdatei des Benutzerkontos erhalten.
set-view-distance = Sichtweite
set-view-distance-hint = Wie weit Gelände gebaut und gezeichnet wird — der größte einzelne Posten.
set-shadows = Schatten
set-shadows-hint = Schattenkarten der Sonne.
set-bloom = Lichtschein
set-bloom-hint = Lässt Lampen und Signale nach Einbruch der Dunkelheit leuchten.
set-fullscreen = Vollbild
set-fullscreen-hint = Randlos, auf dem Bildschirm, auf dem das Fenster steht.
set-vsync = Vertikale Synchronisation
set-vsync-hint = Deckelt die Bildrate auf die des Bildschirms, gegen Bildrisse.
set-volume = Gesamtlautstärke
set-volume-hint = Lautstärke von allem, was der Simulator abspielt.
set-language = Sprache
set-language-hint = Wirkt sofort, im Menü wie im Führerstand.
set-language-system = System
set-hud = HUD
set-hud-hint = Die Anzeige von Geschwindigkeit, Bremsen und Zugsicherung während der Fahrt.
set-look-speed = Umsehgeschwindigkeit
set-look-speed-hint = Wie weit sich der Blick dreht, während die rechte Maustaste gehalten wird.
set-reset = Auf Standard zurücksetzen
set-reset-hint = Setzt jede Einstellung dieser Seite auf den Auslieferungszustand.
# Einheiten der Werte am rechten Rand einer Einstellungszeile.
set-metres = { $value } m
set-percent = { $value } %
set-factor = { $value } ×

## Mod-Verwaltung

mods-title = Mods
mods-none = Keine Mods installiert — ein Mod-Verzeichnis nach mods/ legen.
mods-missing-depends = benötigt: { $depends } (fehlt oder ist abgeschaltet)
mods-content = Inhalte: { $vehicles } Fahrzeuge, { $lines } Strecken, { $compositions } Kompositionen, { $scenarios } Szenarien, { $timetables } Fahrpläne, { $signals } Signaltypen, { $scripts } Skripte
mods-log = Warnungen:
mods-restart = Änderung wirkt nach einem Neustart.
mods-keys = ↑/↓ auswählen   Enter ein/aus   F9 schließen

## Wertung

score-summary = Wertung: { $total } von { $base }
score-stop-missed = Haltepunkt { $stop } um { $metres } m verfehlt
score-timetable = { $stop } { $minutes } min gegenüber dem Fahrplan
score-forced-brakes = { $count } Zwangsbremsung(en)
score-overspeed = { $seconds } s zu schnell (max. { $excess } km/h)
score-energy = { $energy } kWh Traktionsenergie
score-scenario = Szenariowertung

## Fahrzeugeditor: Bausteindiagramm — Ansichten und Leinwand

view-model = 3D-Modell
view-blocks = Bausteindiagramm
graph-palette = Bausteine
graph-search = Bausteine suchen…
graph-inspector = Eigenschaften
graph-issues = Befunde
graph-no-selection = Einen Baustein auf der Leinwand auswählen, um seine Werte zu bearbeiten.
graph-no-params = Dieser Baustein hat keine eigenen Werte.
graph-add-block = Baustein hinzufügen
graph-remove-block = Baustein entfernen
graph-domain-mismatch = Nur Anschlüsse gleicher Farbe lassen sich verbinden.
graph-circuit-add = Kreislauf hinzufügen
graph-circuit-remove = Kreislauf entfernen

## Fahrzeugeditor: Bausteindiagramm — Anschlussarten (Leitungsfarben)

domain-mech = Welle (Drehmoment)
domain-force = Kraft
domain-elec = Elektrisch
domain-air = Druckluft
domain-signal = Steuersignal
domain-fuel = Kraftstoff
domain-steam = Dampf
domain-water = Speisewasser
domain-heat = Wärme

## Fahrzeugeditor: Bausteindiagramm — Anschlussbeschriftungen

port-shaft = Welle
port-elec = Elektrisch
port-air = Luft
port-brake-pipe = Hauptluftleitung
port-force = Kraft
port-ctrl = Steuerung
port-throttle = Fahrschalter
port-brake-demand = Führerbremsventil
port-direct = Zusatzbremse
port-sanding = Sanden
port-slip = Schlupf
port-pilot = Vorsteuerung
port-supply = Versorgung
port-aux = Vorratsbehälter
port-fuel = Kraftstoff
port-steam = Dampf
port-water = Wasser
port-heat = Wärme
port-value = Wert
port-value-a = A
port-value-b = B
port-value-actual = Istwert
port-value-target = Sollwert
port-value-control = Steuerwert
port-excitation = Erregung
port-inlet-a = Eingang A
port-inlet-b = Eingang B
port-axles = Achsen
port-body = Wagenkasten
port-regulator = Regler
port-cutoff = Füllung

## Fahrzeugeditor: Bausteindiagramm — Palettengruppen

blkcat-energy = Energie
blkcat-drivetrain = Antriebsstrang
blkcat-electric = Elektrik
blkcat-brake = Bremse
blkcat-running-gear = Laufwerk
blkcat-control = Steuerung
blkcat-equipment = Ausrüstung
blkcat-steam = Dampf
blkcat-logic = Logik

## Fahrzeugeditor: Bausteindiagramm — Bausteinnamen und Tooltips

blk-battery = Batterie
blk-battery-hint = Fahrzeugbatterie: Steuerstrom für Stromabnehmer, Motorstart und Beleuchtung
blk-fuel-tank = Kraftstoffbehälter
blk-fuel-tank-hint = Dieselvorrat des Motors
blk-pantograph = Stromabnehmer
blk-pantograph-hint = Nimmt den Strom von der Fahrleitung ab (15 kV 16,7 Hz)
blk-diesel-engine = Dieselmotor
blk-diesel-engine-hint = Kraftquelle: Drehmomentkennfeld, Regler und Anlassen — der Kopf jeder Dieselkette
blk-hydro-transmission = Strömungsgetriebe
blk-hydro-transmission-hint = Wandler und Kupplungen, durch Füllen und Entleeren geschaltet — das Voith-Prinzip
blk-mechanical-gearbox = Mechanisches Getriebe
blk-mechanical-gearbox-hint = Reibkupplung und Gänge — keine Drehmomentwandlung, also fährt die Kupplung schleifend an, und der Motor lässt sich abwürgen
blk-hydrostatic-drive = Hydrostatischer Antrieb
blk-hydrostatic-drive-hint = Verstellpumpe und Hydromotor: stufenlos, nichts zu schalten, das Druckbegrenzungsventil deckelt die Zugkraft
blk-retarder = Hydrodynamische Bremse
blk-retarder-hint = Retarder im Getriebe: verschleißfreies Bremsen, stark bei Fahrt, wirkungslos im Stand
blk-generator = Hauptgenerator
blk-generator-hint = Dieselelektrische Kette: der Motor treibt ihn, die Fahrmotoren nehmen seine Leistung — die Leistung steht am Dieselmotor
blk-traction-motor = Fahrmotor
blk-traction-motor-hint = Fahrmotor hinter Stromrichter oder Generator; die Kennwerte der Kette stehen an jenen Bausteinen
blk-series-motor = Reihenschlussmotor
blk-series-motor-hint = Der klassische Gleichstrommotor (BR 110/140) nach seinen Maschinengleichungen: sättigender Fluss, Feldschwächung
blk-main-switch = Hauptschalter
blk-main-switch-hint = Verbindet das Fahrzeug mit der Versorgung; schließt nur mit gehobenem Stromabnehmer und anliegender Fahrdrahtspannung
blk-transformer = Haupttransformator
blk-transformer-hint = Setzt die Fahrdrahtspannung für Schaltwerk, Stromrichter und Hilfsbetriebe herab
blk-tap-changer = Schaltwerk
blk-tap-changer-hint = Stufe um Stufe entlang der Trafowicklung — die Steuerung der klassischen Wechselstromlok
blk-traction-converter = Traktionsstromrichter
blk-traction-converter-hint = Drehstromantrieb: Zugkraft entlang der Kraft-/Leistungshyperbel, darüber die Kippgrenze
blk-dynamic-brake = Dynamische Bremse
blk-dynamic-brake-hint = Motoren als Generatoren: in die Bremswiderstände, oder zurück ins Netz, wenn rückspeisend
blk-traction-curve = Zugkraftkurve
blk-traction-curve-hint = Vereinfachter Antrieb direkt aus dem Diagramm des Datenblatts — weiß nichts von Motoren oder Getrieben
blk-compressor = Kompressor
blk-compressor-hint = Füllt den Hauptluftbehälter zwischen Ein- und Ausschaltdruck
blk-main-reservoir = Hauptluftbehälter
blk-main-reservoir-hint = Luftvorrat des Triebfahrzeugs: Zusatzbremse, Relaisventil und Federspeicher bedienen sich hier
blk-driver-brake-valve = Führerbremsventil
blk-driver-brake-valve-hint = Stellt den Hauptluftleitungsdruck ein: Lösen, Abschluss, Betriebsbremsung, Schnellbremsung
blk-brake-pipe = Hauptluftleitung
blk-brake-pipe-hint = Die zuglange Steuerleitung mit 5 bar: ein Druckabfall legt die Bremse an — ausfallsicher
blk-control-valve = Steuerventil
blk-control-valve-hint = Vergleicht Hauptluftleitung und Referenzdruck und füllt danach den Zylinder
blk-aux-reservoir = Vorratsbehälter
blk-aux-reservoir-hint = Vorrat je Fahrzeug, aus der Hauptluftleitung gefüllt; speist den Bremszylinder
blk-relay-valve = Relaisventil
blk-relay-valve-hint = Vorsteuerung: bildet den Steuerdruck mit Hauptluftbehälterluft nach — schnell, unerschöpfbar, und der Weg der EP-Bremsung
blk-brake-cylinder = Bremszylinder
blk-brake-cylinder-hint = Druck wird Kolbenkraft
blk-brake-rigging = Bremsgestänge
blk-brake-rigging-hint = Übersetzung und Reibpaarung: Zylinderkraft wird Verzögerung am Rad
blk-direct-brake = Zusatzbremse
blk-direct-brake-hint = Lokbremse direkt aus dem Hauptluftbehälter
blk-parking-brake = Feststellbremse
blk-parking-brake-hint = Federspeicher oder Handbremse — hält ohne Luft
blk-mg-brake = Magnetschienenbremse
blk-mg-brake-hint = Presst auf den Schienenkopf, unabhängig vom Kraftschluss der Räder; wirkt in Stellung R bei einer Schnellbremsung
blk-wheel-slide-protection = Gleitschutz
blk-wheel-slide-protection-hint = Überwacht den Schlupf und antwortet mit Schleuderbremse, Zugkraftrücknahme oder Schlupfregelung
blk-sander = Sandstreuer
blk-sander-hint = Sand vor den Treibrädern erhöht den Kraftschluss
blk-wheelset = Radsatz
blk-wheelset-hint = Wo Zug- und Bremskraft die Schiene erreichen: Achszahl und Reibungsmasse
blk-cab = Führerstand
blk-cab-hint = Die Bedienelemente: Fahrschalter, Führerbremsventil, Zusatzbremse, Sanden
blk-afb = AFB
blk-afb-hint = Automatische Fahr- und Bremssteuerung zwischen Fahrschalter und Antrieb
blk-sifa = Sifa
blk-sifa-hint = Sicherheitsfahrschaltung
blk-pzb = PZB
blk-pzb-hint = Indusi/PZB-Zugbeeinflussung, mit der Anfangsstellung des Zugartschalters
blk-lzb = LZB
blk-lzb-hint = Linienzugbeeinflussung auf Strecken mit Linienleiter
blk-doors = Türsteuerung
blk-doors-hint = Türsystem des Fahrzeugs, und ob seine Türen der Freigabe des Zuges folgen
blk-script = Lua-Skript
blk-script-hint = Verhaltens-Hook eines Mods: Schaltwerkslogik, AFB, Aufrüstvorgang
blk-voltage-source = Spannungsquelle
blk-voltage-source-hint = Ersatz für die Fahrleitung, wo keine da ist — Prüfstand oder Stromschiene
blk-rectifier = Gleichrichter
blk-rectifier-hint = Macht aus dem Drehstrom des Generators den Gleichstrom, den die Reihenschlussmotoren brauchen
blk-load-regulator = Leistungsregler
blk-load-regulator-hint = Hält den Generator über die Erregung auf der Leistung, die die Fahrstufe verlangt — das Herz eines dieselelektrischen Antriebs
blk-async-motor = Asynchronmotor
blk-async-motor-hint = Drehstrommotor nach der Kloß'schen Gleichung: Zugkraft-, Leistungs- und Kippbereich ergeben sich aus der Maschine selbst
blk-rheostat = Anfahrwiderstände
blk-rheostat-hint = Werden stufenweise ausgeschaltet, während der Zug beschleunigt; was von der Spannung übrig bleibt, wird Wärme
blk-series-parallel-switch = Reihen-Parallel-Schaltwerk
blk-series-parallel-switch-hint = Gruppiert die Motoren mit steigender Geschwindigkeit um — jede Umgruppierung ist eine Stufe in der Zugkraftkurve
blk-chopper = Chopper
blk-chopper-hint = Stellt die Motorspannung stufenlos statt in Stufen ein und verheizt dabei nichts
blk-cooling = Kühlung
blk-cooling-hint = Wärmespeicher und Lüfter dessen, was daran hängt — ein heißgelaufenes Paket nimmt nichts mehr auf
blk-boiler = Kessel
blk-boiler-hint = Wasser, Dampf und Druck: was das Feuer hineingibt und die Zylinder herausnehmen
blk-firebox = Feuerbüchse
blk-firebox-hint = Rost, Aschkastenklappe und Bläser — der Zug entscheidet, wie stark das Feuer brennt
blk-steam-cylinders = Zylinder
blk-steam-cylinders-hint = Regler und Füllung werden zu Zugkraft; die Expansion ist der Grund, warum Zurücknehmen sich lohnt
blk-injector = Injektor
blk-injector-hint = Drückt Speisewasser gegen den Kesseldruck hinein und kostet dabei Druck
blk-tender = Tender
blk-tender-hint = Mitgeführtes Wasser und Kohle — sind sie alle, ist die Fahrt zu Ende
blk-angle-cock = Absperrhahn
blk-angle-cock-hint = Trennt die Hauptluftleitung. Mitten im Zug geschlossen bleibt alles dahinter ungebremst; am Zugende offen füllt sich die Leitung nie
blk-air-hose = Bremsschlauch
blk-air-hose-hint = Kuppelt die Hauptluftleitung ans Nachbarfahrzeug
blk-emergency-valve = Notbremsventil
blk-emergency-valve-hint = Entlüftet die Hauptluftleitung aus dem Fahrgastraum oder dem Führerstand
blk-limiting-valve = Druckbegrenzer
blk-limiting-valve-hint = Deckelt den Bremszylinderdruck, egal wer ihn anfordert — das hält den Lokführer davon ab, Flachstellen zu fahren
blk-double-check-valve = Wechselventil
blk-double-check-valve-hint = Lässt den höheren von zwei Drücken durch — so teilen sich selbsttätige, direkte und EP-Bremse einen Zylinder
blk-retainer-valve = Rückhalteventil
blk-retainer-valve-hint = Hält beim Lösen einen Restdruck im Zylinder, während der Zug nachfüllt — von Hand gestellt, Wagen für Wagen
blk-ep-brake = EP-Bremse
blk-ep-brake-hint = Die Bremsstellung läuft über Draht: der ganze Zug bremst im selben Moment, statt auf die Druckwelle zu warten
blk-bogie = Drehgestell
blk-bogie-hint = Fasst die Achsen zusammen, die es trägt
blk-axle = Achse
blk-axle-hint = Eine Achse des Laufwerks; einzeln gezeichnet ergeben sich Achszahl und Treibachsanteil aus den Bausteinen
blk-value-in = Messwert
blk-value-in-hint = Holt einen Wert aus dem Fahrzeug in die Logik
blk-constant = Konstante
blk-constant-hint = Eine feste Zahl
blk-value-curve = Kennlinie
blk-value-curve-hint = Stückweise lineare Tabelle: ein Wert hinein, ein anderer heraus
blk-combine = Verknüpfung
blk-combine-hint = Zwei Werte zu einem — Summe, Differenz, Produkt, kleinerer oder größerer
blk-clamp = Begrenzer
blk-clamp-hint = Hält einen Wert in einem Bereich
blk-pid = PID-Regler
blk-pid-hint = Regelt den Istwert auf den Sollwert; daraus baut man eine Geschwindigkeitsregelung
blk-notch = Stufung
blk-notch-hint = Führt den Ausgang mit begrenzter Geschwindigkeit zum Eingang und landet nur auf seinen Stufen
blk-rate-of-change = Änderungsrate
blk-rate-of-change-hint = Wie schnell sich der Eingang ändert, geglättet
blk-value-switch = Umschalter
blk-value-switch-hint = Wählt über einen Steuerwert einen von zwei Werten, mit Hysterese gegen Flattern
blk-signal-out = Ausgang
blk-signal-out-hint = Wo die Logik zugreift: Fahrschalter, Bremse, Sanden, Lüfter oder ein freier Wert für die Anzeigen

## Fahrzeugeditor: Bausteindiagramm — neue Parameter

eng-map = Motorkennfeld
eng-map-hint = mit Kennfeld entscheidet die Drehmomentbilanz; ohne folgt die Zugkraft der Hyperbel
eng-torque-curve = Volllastdrehmoment (1/min → N·m)
eng-notches = Fahrstufen
eng-notches-hint = des Fahrschalters; 0 = stufenlos
eng-droop = Statik (Droop)
eng-droop-hint = Anteil der Nenndrehzahl, um den die Solldrehzahl zwischen Leerlauf und voller Füllung absinkt
eng-governor-speed = Drehzahlgeregelt
eng-governor-fill = Füllungsgeregelt
drv-brake-curve = Dynamische Bremse (km/h → N)
drv-fuel-capacity = Tankinhalt
cir-kind = Bauart
cir-kind-hint = ein Wandler übersetzt das Drehmoment, eine Kupplung überträgt es eins zu eins
cir-kind-converter = Drehmomentwandler
cir-kind-coupling = Strömungskupplung
brk-compressor-delivery = Förderleistung
brk-compressor-delivery-hint = l/min freie Luft
brk-main-volume = Volumen
brk-pipe-volume = Leitungsanteil
brk-pipe-volume-hint = Anteil dieses Fahrzeugs am Volumen der Hauptluftleitung
brk-leakage = Leckrate
brk-leakage-hint = l/min freie Luft, die die Leitung verliert
brk-aux-volume = Volumen
brk-direct-cylinder = Zylinderdruck
brk-mg-force = Kraft
brk-mg-force-hint = N auf den Schienenkopf
brk-load-none = Keine
brk-load-weighing = Wiegeventil
brk-load-changeover = Umstellung Leer/Beladen
brk-friction-block = Graugussklötze
brk-friction-disc = Scheibe
brk-friction-composite-k = Komposit K
brk-friction-composite-ll = Komposit LL
brk-friction-magnetic = Magnetisch
brk-friction-custom = Eigene Kurve
brk-slip-slip-brake = Schleuderbremse
brk-slip-traction-cutback = Zugkraftrücknahme
brk-slip-creep-control = Schlupfregelung
eq-train-type = Zugart
eq-train-type-hint = Anfangsstellung des Zugartschalters
eq-sifa-time-time = Zeit-Zeit
eq-sifa-time-distance = Zeit-Weg
eq-sifa-rzm = Reaktionszeitmessung (RZM)
eq-doors-none = Keine
bat-voltage = Spannung
bat-capacity = Kapazität
pan-system = Stromsystem
pan-system-third-rail = Stromschiene
pan-rise-time = Hubzeit
src-voltage = Spannung
gen-power = Elektrische Leistung
gen-power-hint = liefert der Generator in der höchsten Fahrstufe
gen-efficiency = Wirkungsgrad
gen-max-voltage = Höchstspannung
gen-max-current = Höchststrom
rec-efficiency = Wirkungsgrad
reg-time = Stellzeit
reg-time-hint = braucht der Regler für seinen ganzen Bereich
reg-blower-idle = Lüfter im Leerlauf
reg-blower-idle-hint = Anteil der Kühlung, der schon mit laufendem Motor arbeitet
mot-pole-pairs = Polpaare
mot-rated-torque = Nenndrehmoment
mot-rated-torque-hint = je Motor
mot-pullout-ratio = Kippmoment
mot-pullout-ratio-hint = als Vielfaches des Nenndrehmoments
mot-pullout-slip = Kippschlupf
mot-rated-freq = Eckfrequenz
mot-rated-freq-hint = darüber hat der Stromrichter keine Spannung mehr übrig und das Feld wird geschwächt
mot-max-freq = Höchstfrequenz
rhe-steps = Widerstandsstufen
rhe-steps-hint = Ω je Schützstellung, stärkste zuerst, endend bei 0
rhe-step-time = Zeit je Stufe
spg-groups = Gruppierungen
spg-groups-s-p = Reihe → Parallel
spg-groups-s-sp-p = Reihe → Reihen-Parallel → Parallel
spg-groups-s-only = Nur Reihe
spg-groups-p-only = Nur Parallel
chp-time = Ansprechzeit
cool-capacity = Wärmekapazität
cool-rate = Kühlleistung
cool-rate-hint = W je Kelvin über Umgebung, mit laufendem Lüfter
cool-natural = Eigenkonvektion
cool-natural-hint = Anteil der Kühlung, der ohne Lüfter bleibt
cool-warn = Leistungsrücknahme ab
cool-max = Abschalttemperatur
cool-ambient = Umgebungstemperatur
stm-water-space = Wasserraum
stm-steam-space = Dampfraum
stm-working-pressure = Kesseldruck
stm-safety-valve = Sicherheitsventile
stm-safety-valve-hint = Druck, bei dem sie abblasen
stm-heating-surface = Heizfläche
stm-superheater = Überhitzer
stm-superheater-hint = trockener Dampf durch die Expansion — etwa ein Fünftel des Verbrauchs wert
stm-grate-area = Rostfläche
stm-grate-capacity = Rostfüllung
stm-burn-rate = Abbrand
stm-burn-rate-hint = je Quadratmeter Rost bei vollem Zug
stm-blower = Bläserzug
stm-blower-hint = Anteil des Zugs, den der Bläser allein erzeugt
stm-shovel = Schaufel
stm-cylinders = Zylinderzahl
stm-bore = Bohrung
stm-stroke = Hub
stm-max-cutoff = Größte Füllung
stm-back-pressure = Gegendruck
stm-efficiency = Mechanischer Wirkungsgrad
stm-injector-rate = Förderleistung
stm-tender-water = Wasser
stm-tender-coal = Kohle
brk-pump-kind = Bauart
brk-pump-kind-compressor = Kompressor
brk-pump-kind-exhauster = Luftsauger
brk-medium = Medium
brk-medium-air = Druckluft
brk-medium-vacuum = Vakuum
brk-cock-end = Ende
brk-cock-end-front = Vorn
brk-cock-end-rear = Hinten
brk-limit = Begrenzungsdruck
brk-retainer = Stellung
brk-retainer-off = Direktes Auslassen
brk-retainer-slow = Langsames direktes Lösen
brk-retainer-low = Niedriger Restdruck
brk-retainer-high = Hoher Restdruck
brk-ep-apply = Anlegegeschwindigkeit
brk-ep-release = Lösegeschwindigkeit
brk-ep-vents-pipe = Entlüftet die Hauptluftleitung
brk-ep-vents-pipe-hint = damit folgt die Druckluftbremse als Rückfallebene; ohne sie löst ein Drahtbruch den ganzen Zug
brk-ep-steps = Stufen
brk-ep-steps-hint = der EP-Bremsung; 0 = stufenlos
brk-sand-rate = Sandmenge
veh-wheelbase = Achsstand
veh-axle-driven = Angetrieben
sig-source = Messwert
sig-value = Wert
sig-curve = Kennlinie
sig-combine = Rechenart
sig-combine-add = Summe
sig-combine-sub = Differenz
sig-combine-mul = Produkt
sig-combine-min = Kleinerer
sig-combine-max = Größerer
sig-min = Untere Grenze
sig-max = Obere Grenze
sig-kp = Proportional
sig-ki = Integral
sig-kd = Differential
sig-steps = Stufen
sig-steps-hint = 0 = stufenlos
sig-rate = Stellgeschwindigkeit
sig-rate-hint = voller Bereich je Sekunde
sig-smoothing = Glättung
sig-threshold = Schwelle
sig-hysteresis = Hysterese
sig-sink = Ausgang
sig-in-throttle = Fahrschalter
sig-in-brake = Bremsanforderung
sig-in-direct = Direkte Bremse
sig-in-speed = Geschwindigkeit (m/s)
sig-in-speed-kmh = Geschwindigkeit (km/h)
sig-in-target-speed = Sollgeschwindigkeit
sig-in-cylinder = Bremszylinder
sig-in-pipe = Hauptluftleitung
sig-in-main-res = Hauptluftbehälter
sig-in-current = Motorstrom
sig-in-rpm = Motordrehzahl
sig-in-temp = Temperatur
sig-in-effort = Zugkraft
sig-in-reverser = Richtungsschalter
sig-in-sanding = Sanden
sig-source-throttle = Fahrschalter
sig-source-brake = Bremsanforderung
sig-source-direct = Direkte Bremse
sig-source-speed = Geschwindigkeit (m/s)
sig-source-speed-kmh = Geschwindigkeit (km/h)
sig-source-target-speed = Sollgeschwindigkeit
sig-source-cylinder = Bremszylinder
sig-source-pipe = Hauptluftleitung
sig-source-main-res = Hauptluftbehälter
sig-source-current = Motorstrom
sig-source-rpm = Motordrehzahl
sig-source-temp = Temperatur
sig-source-effort = Zugkraft
sig-source-reverser = Richtungsschalter
sig-source-sanding = Sanden
sig-out-throttle = Fahrschalter
sig-out-brake = Bremsanforderung
sig-out-sanding = Sanden
sig-out-blower = Lüfter
sig-out-aux = Freier Wert
sig-sink-throttle = Fahrschalter
sig-sink-brake = Bremsanforderung
sig-sink-sanding = Sanden
sig-sink-blower = Lüfter
sig-sink-aux0 = Freier Wert 1
sig-sink-aux1 = Freier Wert 2
sig-sink-aux2 = Freier Wert 3
sig-sink-aux3 = Freier Wert 4
grp-series = Reihenschaltung
grp-series-parallel = Reihen-Parallel-Schaltung
grp-parallel = Parallelschaltung

## Fahrzeugeditor: Bausteindiagramm — Befunde des Bakings

bake-unknown-block = Unbekannter Bausteintyp — der Mod, der ihn definiert, ist nicht installiert
bake-duplicate-block = Dieser Baustein darf je Fahrzeug nur einmal vorkommen
bake-bad-wire = Eine Leitung verbindet Anschlüsse, die nicht zueinander passen
bake-unconnected = Mit nichts verbunden
bake-missing-wire = Eine erwartete Verbindung fehlt
bake-multiple-drives = Mehr als ein Antrieb — ein Fahrzeug nimmt eine Antriebskette
bake-brake-needs-drive = Eine dynamische Bremse braucht einen Antrieb, mit dem sie arbeitet
bake-no-pantograph = Ein elektrischer Antrieb erwartet einen Stromabnehmer
bake-two-drive-paths = Mehr als ein Antriebsstrang am selben Motor — es gilt zuerst das Strömungsgetriebe, dann das Schaltgetriebe
bake-gearbox-needs-map = Ein mechanisches Getriebe braucht das Motorkennfeld
bake-transmission-needs-map = Ein Strömungsgetriebe braucht das Motorkennfeld
bake-hydro-and-generator = Strömungsgetriebe und Generator am selben Motor — das Getriebe gewinnt
bake-brake-needs-generator = Eine dieselelektrische Bremse braucht die Generatorkette
bake-series-motor-unused = Ein Reihenschlussmotor wirkt nur hinter einem Schaltwerk
bake-no-control-valve = Kein Steuerventil — das Fahrzeug kann nicht bremsen
bake-no-brake-cylinder = Kein Bremszylinder — das Fahrzeug kann nicht bremsen
bake-no-brake-rigging = Kein Bremsgestänge — die Zylinderkraft erreicht kein Rad
bake-no-brake-pipe = Keine Hauptluftleitung — die Zugbremse hat keine Steuerleitung
bake-no-aux-reservoir = Kein Vorratsbehälter — es gilt der Vorgabewert von 100 l
bake-needs-main-reservoir = Braucht einen Hauptluftbehälter als Luftversorgung
bake-mg-needs-r = Die Magnetschienenbremse wirkt nur in Stellung R — das Steuerventil hat keine R-Stellung
bake-no-wheelset = Kein Radsatz — nichts trägt die Kräfte auf die Schiene
bake-no-motor = Ein Generator ohne Fahrmotoren treibt nichts an
bake-no-load-regulator = Ein dieselelektrischer Antrieb ohne Leistungsregler hält keine Leistung
bake-no-boiler = Kein Kessel — die Zylinder haben nichts, womit sie arbeiten könnten
bake-no-firebox = Keine Feuerbüchse — nichts heizt den Kessel
bake-no-tender = Kein Tender — die Lok führt weder Wasser noch Kohle mit
bake-no-injector = Kein Injektor — der Kessel lässt sich nicht speisen
bake-starter-needs-motor = Anfahrschaltung wirkt nur mit Motordaten
bake-axle-count-mismatch = Radsatz und Einzelachsen sind sich über die Achszahl nicht einig
bake-no-driven-axle = Ein Triebfahrzeug ohne Treibachse trägt seine Zugkraft nirgendwohin
bake-axles-per-bogie = Die Achsen gehen nicht gleichmäßig auf die Drehgestelle auf
bake-vacuum-no-relay = Eine Vakuumbremse hat kein Relaisventil zur Vorsteuerung
bake-vacuum-needs-exhauster = Eine Vakuumbremse braucht einen Luftsauger, keinen Kompressor
bake-signal-cycle = Diese Logikbausteine speisen sich im Kreis und lassen sich nicht auswerten
bake-signal-out-open = Dieser Ausgang ist an nichts angeschlossen
bake-signal-no-output = Die Logikbausteine rechnen etwas aus, das nirgendwohin geht

## Fahrzeugeditor: Bausteindiagramm — Kommentarrahmen und Leinwand-Tastatur

graph-group = Kommentarrahmen
graph-group-default = Kommentar
graph-group-name = Titel
graph-group-color = Farbe
graph-group-remove = Kommentarrahmen entfernen
