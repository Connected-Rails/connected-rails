# Connected Rails — deutsche Übersetzung.
#
# Schlüssel auf -hint sind der Tooltip des gleichnamigen Feldes.
# Platzhalter wie { $file } füllt das Programm; sie müssen erhalten bleiben.

## Fenster und Menüs

window-simulator = Connected Rails
window-vehicle-editor = Connected Rails — Fahrzeugeditor
window-vehicle-editor-named = { $name } — Connected Rails Fahrzeugeditor
window-vehicle-editor-unsaved = • { $name } — Connected Rails Fahrzeugeditor
window-route-editor = Connected Rails — Moduleditor
window-route-editor-named = { $name } — Connected Rails Moduleditor
window-route-editor-unsaved = • { $name } — Connected Rails Moduleditor

menu-file = Datei
menu-edit = Bearbeiten
menu-view = Ansicht
menu-help = Hilfe
menu-overlay = Luftbild
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
filter-line-ron = Modul (RON)
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
snd-quantity-vigilance-alert = Sifa-Summer
snd-quantity-protection-alert = Zugbeeinflussung (PZB/LZB) hupt
snd-quantity-dynamic-brake = E-Bremskraft [kN]
snd-quantity-horn = Signalhorn
snd-quantity-roughness = Gleisrauigkeit
snd-quantity-rain = Regen
snd-quantity-thunder = Donner

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

## Moduleditor

action-new-line = Neues Modul

# --- Neues Modul -----------------------------------------------------------
new-module-title = Neues Modul
new-module-name = Name
new-module-name-placeholder = z. B. Göttingen – Northeim
new-module-lat = Breite
new-module-lon = Länge
new-module-size = Anfangsgröße
new-module-size-hint = Kantenlänge der quadratischen Hüllkurve, mit der das Modul startet; ihre Ecken werden danach in Form gezogen.
new-module-year = Umsetzungsjahr
new-module-year-hint = Das Jahr, das das Modul zeigt — der Zustand der Strecke, den ein Fahrer vorfinden soll.
new-module-kind = Nachbau
new-module-kind-real = Real
new-module-kind-fictional = Fiktiv
new-module-search-placeholder = Ort, Bahnhof oder Adresse suchen
action-search = Suchen
new-module-searching = Suche läuft…
new-module-no-hits = Unter diesem Namen nichts gefunden.
new-module-search-failed = Suche fehlgeschlagen: { $error }
new-module-map-hint = Zum Setzen des Ankers klicken, zum Verschieben der Karte ziehen, mit dem Mausrad zoomen.
action-create-module = Modul anlegen
new-module-needs-name = Das Modul braucht einen Namen.
status-module-created = Modul „{ $name }“ angelegt — die Hüllkurve ist das Quadrat um den Anker.
action-open-line = Modul öffnen…
action-delete = Löschen
action-load-imagery = Luftbild-Konfiguration laden (F5)
action-save-imagery = Luftbild-Konfiguration speichern (F2)
overlay-toggle = Ein/aus (O)
overlay-next-provider = Nächster Anbieter (P)
overlay-offline = Offline-Modus (L)
overlay-clear-cache = Cache leeren (C)
overlay-retry = Fehlversuche zurücksetzen (R)

# Viewport bar — icon buttons above the viewport, tooltips only.
view-imagery = Luftbild auf dem Gelände
gizmo-move = Verschiebegriffe (W)
gizmo-rotate = Drehgriff (E)
camera-speed = Kamerageschwindigkeit
camera-speed-hint = Kamerageschwindigkeit — { $speed } m/s. Rechte Maustaste und Mausrad drehen dasselbe Rad.
camera-speed-scalar = Geschwindigkeitsfaktor
camera-speed-value = { $speed } m/s

help-fly = Rechte Maustaste halten zum Umsehen, WASD fliegt, Q/E runter/hoch, Umschalt langsamer · rechte Maustaste + Mausrad stellt die Kamerageschwindigkeit · Alt+links umkreist · mittlere Maustaste schwenkt · Mausrad zoomt · F rückt die Auswahl ins Bild
help-gizmo = W Verschiebegriffe, E Drehgriff · Pfeil ziehen verschiebt die Auswahl längs des Gleises, quer dazu oder nach oben
help-opacity = [ ] Deckkraft · , . Zoomstufe · Z automatisch
help-offset = Ziffernblock 4/6/8/2 Bildversatz, 5 zurücksetzen
help-draw = Gleis verlegen: Drücken und Ziehen setzt Start und Richtung, jeder Klick ein Stück · Strg gerade · Enter oder Rechtsklick schließt ab · Esc bricht ab

status-ready = Bereit
status-perf = { $fps } fps · { $entities } Entitäten · { $tiles } Kacheln (+{ $pending })
status-perf-hint = Bilder pro Sekunde, Entitäten in der Szene, Geländekacheln in der Szene und im Bau. Beim Fliegen im Blick behalten: die Kachelzahl soll sich einpendeln, die Bildrate nicht einbrechen.
status-position = { $lat }°, { $lon }°   Höhe { $height } m
status-ground-height = Boden { $height } m
status-terrain-flat = Noch keine Höhendaten — der Boden ist flach; unter Höhendaten ein DGM importieren
status-cache-cleared = Cache geleert
status-retry-reset = Fehlversuche zurückgesetzt
status-saved = { $file } gespeichert
status-save-failed = Speichern fehlgeschlagen: { $error }
status-not-readable = { $file } nicht lesbar
status-not-compiling = { $file } lässt sich nicht übersetzen
status-compile-error = Modul lässt sich nicht übersetzen: { $error }
status-no-track-hit = Kein Gleis nahe dem Klick — Geräte sitzen auf einem Gleis
status-split-at-end = Zu nah am Gleisende — mindestens 1 m innerhalb klicken
status-split-failed = Weiche nicht gesetzt — das Modul kompiliert nicht
status-ghost-loaded = Geistermodul { $file }: { $boundaries } Grenzen
status-route-derived = Weg gefunden: { $sections } Abschnitte, { $overlap } im Durchrutschweg, { $switches } Weichen
status-routes-found = { $added } Fahrstraßen angelegt, { $known } schon vorhanden
status-no-route-path = Kein Weg vom Start- zum Zielsignal — die Wirkrichtung der Signale prüfen
status-no-objects = Keine Objekte installiert — ein Mod liefert sie als objects/*.ron
status-config-unreadable = { $file } nicht lesbar ({ $error }) — Vorgabe aktiv
status-config-created = { $file } angelegt
status-config-not-writable = { $file } nicht beschreibbar: { $error }

heading-line = Modul
line-name = Name
line-counts = Gleise: { $edges } · Geräte: { $devices }

heading-tools = Werkzeug
tool-group-track = Gleis
tool-group-equipment = Streckenausrüstung
tool-group-vegetation = Vegetation
tool-group-terrain = Gelände
tool-select = Auswählen
tool-select-hint = Klick wählt Einzelnes aus · Strg+Klick sammelt eine Mehrfachauswahl, ein zweiter Strg+Klick nimmt es wieder heraus · auf freiem Boden drücken und ziehen wählt im Kreis · Entf löscht die Auswahl
tool-draw = Gleis verlegen
tool-draw-hint = Drücken setzt das stehende Ende, das Ziehen seine Richtung — auf einem offenen Ende führt es das Gleis fort, auf der Gleismitte beginnt der Zweig einer Weiche. Jeder Klick hängt einen tangentialen Bogen an, mit gehaltenem Strg eine Gerade; das laufende Ende rastet auf offenen Enden ein. Enter oder Rechtsklick schließt ab · Esc bricht ab
tool-split = Gleis trennen
tool-split-hint = Klick auf ein Gleis schneidet es dort durch — zwei Gleise an einem Stoß, jedes für sich wähl- und löschbar
tool-join = Enden verbinden
tool-join-hint = Erst ein offenes Ende klicken, dann das andere. Enden am selben Punkt werden verschweißt; Enden mit Abstand steckt der Rechner ab wie Zusis Absteckrechner: Übergangsbogen – Bogen – Übergangsbogen plus eine Ausgleichsgerade, oder ein Doppelbogen mit Zwischengerade, wo kein einzelner Bogen hinreicht
tool-offset = Parallelgleis
tool-offset-hint = Klick auf ein Gleis legt seine Parallele im eingestellten Gleisabstand — auf der Seite des Klicks
tool-crossover = Gleisverbindung
tool-crossover-hint = Erst das Gleis klicken, das sie verlässt, dann das parallele, das sie erreicht — beide werden geschnitten und zu den zwei Weichen einer Gleisverbindung verdrahtet
tool-gradient = Neigung
tool-gradient-hint = Klick auf ein Gleis setzt dort einen Neigungswechsel; das Auswahlpanel stellt die Promille zwischen den Punkten ein
tool-device = Gerät platzieren
tool-device-hint = Klick auf ein Gleis setzt die gewählte Geräteart dorthin
tool-area = Bereich markieren
tool-area-hint = Einen Strich am Gleis entlang ziehen und dem Abschnitt Eigenschaften geben
tool-area-drag = Auf einem Gleis drücken und daran entlangziehen. Der Strich folgt diesem Gleis, bis die Taste losgelassen wird.
tool-area-joins = Kommt zu { $name } dazu.
tool-object = Objekt platzieren
tool-object-hint = Klick auf ein Gleis setzt das gewählte 3D-Objekt in seinem vordefinierten Abstand und seiner Rotation dorthin
tool-tree = Baum setzen
tool-tree-hint = Jeder Klick pflanzt einen Baum der gewählten Art — frei in der Fläche, ohne Gleisbezug
tool-forest = Wald-Pinsel
tool-forest-hint = Klicks umreißen eine Fläche; Enter oder Rechtsklick füllt sie mit Einzelbäumen — jeder bleibt einzeln editier- und löschbar · Esc bricht ab
tool-brush = Markier-Pinsel
tool-brush-hint = Linke Taste halten und überstreichen markiert Bäume und Objekte in der Fläche; Entf löscht sie gemeinsam, Esc leert die Markierung
tool-marker = Marker setzen
tool-marker-hint = Jeder Klick setzt einen Referenzmarker in den genannten Layer — eine Zeichenhilfe, keine Ausstattung: die Simulation liest ihn nicht
marker-layer = Layer
marker-layer-hint = Alles mit demselben Namen ist ein Layer, und Layer werden als Ganzes ausgeblendet und gelöscht
marker-label = Beschriftung
marker-label-hint = Freier Text am Marker — ein Kilometer, ein Straßenname, eine Notiz
tool-terrain-raise = Boden anheben
tool-terrain-raise-hint = Jeder Klick stempelt einen runden Strich, der den Boden um den eingestellten Betrag hebt. Das Gleis behält seine Höhe: die Striche formen den Boden, Einschnitt und Damm legen sich danach darüber
tool-terrain-lower = Boden absenken
tool-terrain-lower-hint = Derselbe Strich nach unten — eine Senke, ein Teichbett, eine Grube
tool-terrain-level = Planieren
tool-terrain-level-hint = Zieht den Boden im Kreis auf die Höhe, die er unter dem Klick hat — die Plateau-Geste des World Editors
tool-terrain-rail = Auf Schienenhöhe
tool-terrain-rail-hint = Zieht den Boden auf die Höhe der nächsten Schiene — Bahnhofsvorfeld, Betriebsgelände, ebene Fläche
status-no-ground-height = Unter dem Klick liegt noch keine Bodenkachel — warten, bis die Kacheln gebaut sind, oder näher an der Strecke klicken
terrain-radius = Radius
terrain-radius-hint = Reichweite des Strichs; er läuft am Rand auf null aus, überlappende Striche gehen also ohne Kante ineinander über
terrain-amount = Höhenänderung
terrain-amount-hint = Meter, um die die Mitte wandert — ob nach oben oder unten, bestimmt das Werkzeug in der Hand
terrain-target = Zielhöhe
terrain-target-hint = Ellipsoidische Höhe, auf die der Strich den Boden zieht
terrain-count = Striche in diesem Modul: { $count }
sel-terrain-summary = Geländestrich { $index }
tool-tile = DGM-Kacheln
tool-tile-hint = Zeigt das Geländekachel-Raster und wählt einzelne Kacheln per Klick — grün hat schon Höhen, blau ist gewählt. Ohne Auswahl importiert der Import den ganzen Korridor
# --- Gleis verlegen --------------------------------------------------------
# Die Eigenschaften des nächsten Stücks — das Panel des World Editors für das
# Stück, das gleich gelegt wird; liegendes Gleis berührt es nie.
lay-type = Gleisart
lay-type-hint = Als was das nächste Stück gebaut wird — Textur, Rauigkeit und Oberbau-Geschwindigkeit kommen aus track_types/*.ron eines Mods; die Inhaltsschublade wählt sie ebenfalls
lay-speed = Zulässige Geschwindigkeit
lay-speed-hint = Geschwindigkeitsprofil, mit dem das nächste Stück beginnt; 0 lässt den Standard der Strecke stehen
lay-grade = Neigung
lay-grade-hint = Das nächste Stück steigt (+) oder fällt (−) mit diesem Wert; das Neigungswerkzeug formt sie danach um
lay-power = Elektrifizierung
lay-power-unset = (wie die Strecke)
lay-power-hint = Was über dem nächsten Stück hängt; ohne Angabe gilt der Standard der Strecke
lay-parallel = Gleise je Zug
lay-parallel-hint = Legt so viele Parallelgleise auf einmal, rechts neben dem gezeichneten — ein Bahnhofsfeld in einer Geste
lay-spacing = Gleisabstand
lay-spacing-hint = Achsabstand paralleler Gleise; 4 m ist der deutsche Hauptbahn-Standard
lay-snap-radius = Auf Regelradien einrasten
lay-snap-radius-hint = Rundet einen gezeichneten Bogen auf die Radienreihe der Trassierung, wie der World Editor sein laufendes Ende einrasten lässt
lay-easements = Übergangsbögen & Überhöhung
lay-easements-hint = Legt Bögen als Klothoide – Bogen – Klothoide mit der Regelwerks-Überhöhung für die Geschwindigkeit des Stücks (Rampe 1:10v, Deckel 160 mm) statt als nackte Kreisbögen. Ein Gleis mit Übergangsbögen hat danach keine ziehbaren Stützpunkte mehr
lay-snap-terrain = Auf Gelände legen
lay-snap-terrain-hint = Das gelegte Stück folgt dem Boden: abgetastete Geländehöhen werden sein Steigungsprofil, ein freier Anfang fällt auf die Oberfläche. Ein an bestehendes Gleis angeschlossenes Ende behält dessen Höhe
lay-turnout-radius = Weichenradius
lay-turnout-radius-hint = Radius der zwei Weichenbögen, aus denen eine Gleisverbindung gebaut wird; 190 m ist die verbreitete deutsche EW-190-Geometrie
# --- Absteckrechner ---------------------------------------------------------
# Die Parameter des Verbinden-Werkzeugs, nach Zusis Absteckrechner: woraus die
# Verbindung zweier offener Enden gebaut werden darf.
stake-speed = Trassierungsgeschwindigkeit
stake-speed-hint = Übergangsbogenlängen und Überhöhung folgen dieser Geschwindigkeit; 0 nimmt die der Verlege-Optionen
stake-radius = Radius
stake-radius-hint = Fester Bogenradius; 0 lässt den Rechner ihn wachsen, bis genau eine Ausgleichsgerade übrig bleibt — Zusis Automatik
stake-easements = Übergangsbögen
stake-easements-hint = Eine Klothoide an jedem Krümmungswechsel; abgeschaltet besteht die Kette aus nackten Geraden und Bögen
stake-easement-length = Übergangsbogenlänge
stake-easement-length-hint = Feste Länge je Übergangsbogen; 0 nimmt die Regelwerks-Rampe für die Trassierungsgeschwindigkeit
stake-cant = Überhöhung
stake-cant-hint = Die Regelwerks-Überhöhung unter den Bögen, gerampt über die Übergangsbögen
stake-min-straight = Zwischengerade
stake-min-straight-hint = Kürzeste Gerade, die ein Doppelbogen zwischen seinen beiden Bögen behält
status-stake-not-plausible = Zwischen diesen Enden passt keine Verbindung — nicht plausibel; ein Ende verschieben oder drehen
status-stake-radius-too-big = Der gewählte Radius ist zu groß für den Abstand der Enden
status-stake-arc-too-short = Der Bogen zwischen den Übergangsbögen wird zu kurz — kleinerer Radius oder kürzere Übergangsbögen
status-stake-double-impossible = Auch ein Doppelbogen passt nicht — Enden verschieben oder Radius und Zwischengerade ändern
join-first-set = Erstes Ende gewählt — jetzt das Ende klicken, mit dem es verbunden wird.
crossover-first-set = Erstes Gleis geschnitten — jetzt das parallele Gleis klicken, das die Verbindung erreicht.
draw-aiming = Ziehen bestimmt die Richtung — loslassen, dann Punkte klicken
draw-aim-facing = Spitze Weiche: der Zweig geht mit dem Gleis ab — loslassen legt fest, Ziehen nach hinten macht sie stumpf
draw-aim-trailing = Stumpfe Weiche: der Zweig läuft am Gleis zurück — loslassen legt fest, Ziehen nach vorn macht sie spitz
draw-readout-arc = { $length } m · R { $radius } m
draw-readout-arc-canted = { $length } m · R { $radius } m · u { $cant } mm
draw-readout-straight = { $length } m · gerade
draw-active = Verlegen: { $segments } Stücke — Enter oder Rechtsklick schließt ab, Esc bricht ab
draw-branch = Zweiggleis: { $segments } Stücke — Enter oder Rechtsklick verdrahtet die Weiche, Esc bricht ab
status-no-open-end = Kein offenes Gleisende am Klick — das Verbinden-Werkzeug schweißt Stumpfgleise
status-join-same-end = Beide Klicks trafen dasselbe Ende — zwei verschiedene wählen
status-crossover-same-track = Beide Klicks trafen dasselbe Gleis — die Gleisverbindung braucht das parallele
status-crossover-not-parallel = Das zweite Gleis läuft dort nicht parallel — die Weichenbögen landen daneben
forest-active = Waldfläche: { $corners } Eckpunkte — Enter oder Rechtsklick schließt sie, Esc bricht ab

heading-selection = Auswahl
heading-areas = Gleisbereiche
sel-none = Nichts ausgewählt — das Auswahlwerkzeug greift alles, was auf der Karte steht, in jeder Kategorie.
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
sel-power-default = Ohne Angabe: { $system }
sel-power-from = Ab dieser Bogenlänge
sel-power-hint = Ein eigener Abschnitt — die Lücke an einer Systemtrennstelle oder ein Gleis ohne Fahrdraht
sel-grade = Neigung
sel-grade-none = Eben über das ganze Gleis.
sel-grade-from = Ab dieser Position auf dem Gleis
sel-grade-hint = Ein eigener Neigungswechsel — ab seiner Position steigt oder fällt das Gleis mit dem neuen Wert
sel-grade-climb = Höhengewinn über dieses Gleis: { $climb } m
action-add-grade-section = Neigungswechsel hinzufügen
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
check-yard-off-edge = Gleis { $yard } liegt außerhalb seines Gleises
check-portal-inside = Portal { $yard } liegt nicht am Rand der Strecke — das Gleis dahinter muss auf einen Prellbock oder eine Modulgrenze auslaufen
check-yard-name-twice = Gleis { $yard } trägt einen Namen, den ein anderes schon hat
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
action-browse-drawer = In der Schublade wählen…
status-issues = Befunde: { $count }
status-issues-hint = Die Befunde der Prüfung — der Klick öffnet sie unter der Modul-Kategorie
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
view-north = Nach Norden ausrichten
view-north-hint = Nach Norden ausrichten — Kamerakurs { $degrees }°
view-top-down = Draufsicht
view-top-down-hint = Senkrecht nach unten für Gleisarbeit über dem Luftbild; erneut klicken für die freie Ansicht
view-panel = Eigenschaften-Panel
il-route-row = Fahrstraße { $index }
route-entry = Startsignal
route-exit = Zielsignal
route-kind = Art
route-kind-train = Zugstraße
route-kind-shunt = Rangierstraße
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
status-heights-need-mod = Das Modul zuerst in einem Mod speichern — die Höhendaten liegen daneben, im Mod
status-heights-no-source = Noch keine DGM-Lieferung gewählt
status-heights-imported = { $tiles } Höhenkacheln geschrieben, { $empty } ohne Daten übersprungen — das Modul bringt sein Gelände jetzt mit

heading-markers = Referenzmarker

heading-module = Modulgrenzen
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
ghost-boundaries = Grenzen als Fangpunkte: { $count }
# --- Time of day -----------------------------------------------------------
# The sky over the module. Latitude and longitude are the module's anchor, so
# only the date and the clock are edited here.
heading-sky = Tageszeit
sky-date = Datum
sky-date-hint = Tag, Monat und Jahr. Das Datum entscheidet, wie hoch die Sonne steigt und wo sie aufgeht — im Simulator kommt es aus der Startzeit des Szenarios.
sky-time = Uhrzeit
sky-time-hint = Ortszeit. Mit dem Regler darunter lässt sich ein ganzer Tag durchfahren.
sky-zone = Zeitzone
sky-zone-hint = Wie weit die Ortszeit der Weltzeit vorausläuft. Deutschland: 1 im Winter, 2 im Sommer.
sky-overcast = Bewölkung
sky-overcast-hint = 0 ist ein klarer Himmel, 1 eine geschlossene Decke: die Sonne wird gedämpft, ihre Schatten verschwinden, die Sterne sind weg.
sky-weather = Wetter
sky-weather-hint = Das benannte Wetter, in dem dieses Modul gezeigt wird. Eine Auswahl schreibt Bewölkung, Sicht und Niederschlag; die Felder darunter ändern sie weiter.
sky-visibility = Sichtweite
sky-visibility-hint = Wie weit ein dunkler Gegenstand vor dem Horizont sichtbar bleibt. Die Atmosphäre trägt sie als Streuungsterm, der Dunst nimmt also die Farbe der Stunde an.
weather-custom = Eigen
weather-clear = Klar
weather-cloudy = Heiter
weather-overcast = Bedeckt
weather-fog = Nebel
weather-drizzle = Nieselregen
weather-rain = Regen
weather-storm = Sturm
weather-thunderstorm = Gewitter
weather-sleet = Schneeregen
weather-snow = Schnee
weather-blizzard = Schneesturm
weather-hail = Hagel
weather-frost = Frost
sky-scrub = Tag durchfahren
sky-sun-at = Sonne { $elevation }° über dem Horizont, { $azimuth }° aus Nord
sky-moon-at = Mond { $elevation }° über dem Horizont, { $phase } % beleuchtet
sky-place = Aus dem Modulanker: { $lat }°, { $lon }°

# --- Calendar --------------------------------------------------------------
# The date button of the status bar and the month it opens. Weeks run Monday
# to Sunday; cal-weekday-1 is therefore Monday, cal-weekday-7 Sunday.
cal-date = { $day }.{ $month }.{ $year }
cal-weekday-1 = Mo
cal-weekday-2 = Di
cal-weekday-3 = Mi
cal-weekday-4 = Do
cal-weekday-5 = Fr
cal-weekday-6 = Sa
cal-weekday-7 = So
cal-month-1 = Januar
cal-month-2 = Februar
cal-month-3 = März
cal-month-4 = April
cal-month-5 = Mai
cal-month-6 = Juni
cal-month-7 = Juli
cal-month-8 = August
cal-month-9 = September
cal-month-10 = Oktober
cal-month-11 = November
cal-month-12 = Dezember

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
check-area-no-effect = Gleisbereich { $area }: deckt nichts ab oder setzt nichts — er erreicht das Modul nicht
check-area-track-type = Gleisbereich { $area }: nennt eine Gleisbauart, die kein installiertes Mod kennt
check-lzb-no-conductor = Gleis { $edge }: LZB-Gleisart, aber das Modul verlegt keinen Linienleiter
check-outside-envelope = Außerhalb der Hüllkurve: Bäume { $trees }, Geländestriche { $terrain }, Marker { $markers }, Wegpunkte { $walkways }, Feldecken { $fields } — Grenze verlegen oder löschen
check-walk-path-short = Fußweg { $path } hat weniger als zwei Punkte — Weg zeichnen oder löschen
check-walk-area-small = Gehfläche { $area } hat weniger als drei Ecken — Fläche umreißen oder löschen
check-envelope-crossed = Die Hüllkurve überschneidet sich — so eine Grenze hat kein Innen; die Ecke zurückziehen
status-outside-envelope-track = Hinter der Hüllkurve — ein Gleis darf die Grenze erreichen, nicht überschreiten
heading-imagery = Luftbilder
img-enabled = Overlay anzeigen
img-provider = Anbieter
img-opacity = Deckkraft
img-zoom = Zoom
img-radius = Laderadius
img-radius-hint = Wie weit um die Kamera herum Kacheln geholt werden. Jeder Meter mehr sind mehr Kacheln zum Laden und im Grafikspeicher — die Anzahl darunter sagt wie viele
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

## Simulator-HUD
#
# Die Anzeige über dem laufenden Spiel. Sie ist in Zonen aufgeteilt — die Fahrt (oben
# links), die Systeme (oben rechts), das Fahrpult (unten Mitte), die Zugsicherung (unten
# links) und die Vorschau (unten rechts) — dazu zwei Überlagerungen auf F5 und F6.
# Die Beschriftungen sind bewusst kurz: sie stehen neben einem Wert in einer schmalen
# Spalte, und eine Beschriftung, die umbricht, verschiebt den Wert.

# Die Fahrt, oben links.
hud-timetable = Fahrplan
hud-platform = Gl. { $platform }
hud-late = +{ $minutes } min
hud-early = −{ $minutes } min
hud-on-time = pünktlich
hud-free-run = Freie Fahrt
# Welche Fahrt der Spieler macht (Ril 301): eine Zugfahrt führt eine Zugnummer und
# wird von den Hauptsignalen signalisiert, eine Rangierfahrt führt keine und wird
# allein durch Sh 1 zugelassen.
movement-train = Zugfahrt
movement-shunt = Rangierfahrt
hud-score = Wertung
hud-scenario-passed = bestanden
hud-scenario-failed = gescheitert

# Die Systeme, oben rechts. Die Melder sind die Kürzel des Führerpults und stehen in der
# Oberfläche in Versalien — bitte bei wenigen Buchstaben bleiben.
hud-systems = Systeme
hud-chip-battery = Batt
hud-chip-pantograph = Bügel
hud-chip-main-switch = HS
hud-chip-compressor = Presser
hud-chip-parking = Feder
hud-chip-sanding = Sand
hud-chip-doors = Türen
hud-chip-lights = Licht
hud-chip-slip = Schleudern
hud-chip-hot = Heiß
# Was der Antrieb über sich sagt — eine Beschriftung je Zeile, die drei, die zutreffen.
hud-catenary = Fahrdraht
hud-motor-current = Motorstrom
hud-notch = Fahrstufe
hud-engine = Motor
hud-fill = Füllung
hud-converter = Wandler
hud-generator = Generator
hud-boiler = Kessel
hud-water-glass = Wasserstand
hud-fire = Feuer
hud-dynamic-brake = E-Bremse

# Das Fahrpult, unten Mitte.
hud-unit-kmh = km/h
hud-permitted = zul. { $speed }
hud-supervised = üw. { $speed }
hud-levers = Steuerung
hud-power = Fahrschalter
hud-brake = Bremse
hud-effort = Kraft
hud-air-pipe = HL { $value }
hud-air-reservoir = HB { $value }
hud-air-cylinder = C { $value }
hud-reverser = Richtung
hud-afb = AFB
hud-odometer = Weg
hud-forward = Vorwärts
hud-reverse = Rückwärts
hud-neutral = Neutral
hud-valve-release = Fahrt
hud-valve-lap = Abschluss
hud-valve-fill = Füllstoß
hud-valve-service = Betriebsbremsung { $drop }
hud-valve-emergency = Schnellbremsung

# Die Zugsicherung, unten links. Die Lampenaufschriften selbst (1000 Hz, 500 Hz, Befehl,
# Ü, G, Ende) sind die Beschriftung eines deutschen Führerpults und bleiben, wie sie
# sind — wie eine Baureihenbezeichnung.
hud-protection = Zugsicherung
hud-v-permitted = v-Soll
hud-v-target = v-Ziel
hud-target-distance = Zielweg
hud-pzb-restrictive = Restriktive Überwachung
hud-self-test = Funktionsprüfung: { $phase }

# Die Vorschau, unten rechts.
hud-ahead = Voraus
hud-stop-in = Halt in { $distance }
hud-in = in { $distance }

# Das Band über dem Fahrpult und die Meldespalte oben.
hud-alert-emergency = ZWANGSBREMSUNG
hud-alert-forced = ZWANGSBREMSUNG (BETRIEBSBREMSE)
hud-alert-cut-off = ABSCHALTUNG DER ZUGKRAFT
hud-alert-blocked = FAHRWEG NICHT EINGESTELLT
hud-control = { $name }: { $value } %

# Was der Rangierer am Boden meldet, auf derselben Zeile, auf der auch die
# Zugbeeinflussung dazwischenredet (Plan Kap. 11).
hud-shunt-coupled = GEKUPPELT
hud-shunt-uncoupled = ABGEKUPPELT
hud-shunt-waiting = RANGIERER WARTET — { $reason }
hud-shunt-refused = NICHT MÖGLICH — { $reason }

# Die Tastenhilfe, F5.
hud-help = Tastatur
hud-help-close = F5 schließt · F6 zeigt die Diagnose
hud-help-annunciators = Was die Melder bedeuten
hud-help-driving = Fahren
hud-help-brakes = Bremsen
hud-help-safety = Zugsicherung
hud-help-vehicle = Fahrzeug
hud-help-view = Sicht
hud-key-throttle = Fahrschalter
hud-key-throttle-off = Fahrschalter auf null
hud-key-reverser = Vorwärts · neutral · rückwärts
hud-key-range = Wendegetriebe
hud-key-afb = AFB ein und aus
hud-key-afb-target = AFB-Sollgeschwindigkeit
hud-key-brake = Bremsen · lösen
hud-key-lap = Abschluss
hud-key-fill = Füllstoß
hud-key-emergency = Schnellbremsung
hud-key-direct = Zusatzbremse
hud-key-release = Lokbremse lösen
hud-key-parking = Feststellbremse
hud-key-ep = EP-Bremse
hud-key-sand = Sanden
hud-key-sifa = Sifa
hud-key-acknowledge = Wachsam
hud-key-free = Frei
hud-key-override = Befehl
hud-key-lzb = LZB Übernahme · Ende · Prüfung
hud-key-train-type = Zugart
hud-key-horn = Signalhorn
hud-key-prepare = Batterie · Bügel · Hauptschalter · Presser
hud-key-starter = Anlasser
hud-key-lamps = Spitzensignal · Führerraumlicht
hud-key-dimmer = Instrumentenlicht
hud-key-wipers = Scheibenwischer
hud-key-doors = Freigabe links · rechts · schließen
hud-key-shunt = Kuppeln · abkuppeln
hud-key-take-over = Übernehmen · abgeben
hud-key-cameras = Führerstand · außen · Strecke · gehen
hud-key-look = Umsehen
hud-key-zoom = Kameraabstand
hud-key-walk = Gehen (F4)
hud-key-jump = Springen
hud-key-hud = Anzeige: voll · reduziert · aus
hud-key-overlays = Diese Übersicht · Diagnose
hud-key-mods = Mod-Verwaltung
hud-key-console = Konsole
hud-key-pause = Pause

# Die Diagnose, F6. Maschinenausgabe — sie darf so dicht sein, wie sie will.
hud-diagnostics = Diagnose
hud-diag-frame = Bild     { $fps } fps, { $millis } ms, { $entities } Entitäten
hud-diag-terrain = Gelände  { $tiles } Kacheln (+{ $pending }), { $triangles } Dreiecke, { $megabytes } MB, Sicht { $view } m
hud-diag-air = Luft     R { $auxiliary } bar   Zusatz { $direct } bar   { $air } Nl verbraucht
hud-diag-axles = Achsen   { $slipping }/{ $axles } schleudern, schlimmste { $worst } m/s
hud-diag-temperature = Wärme    Motoren { $motor } °C   Widerstände { $resistor } °C
hud-diag-signals = Signale  { $aspects }
hud-diag-network = Netz     { $state }, { $latency } ms, Korrektur { $correction } cm
hud-network-joined = verbunden
hud-network-connecting = verbinde …

## Hauptmenü
#
# Die Navigationsspalte links, danach die Seiten, die sie öffnet. Ein -hint-Schlüssel
# ist die zweite, blassere Zeile unter dem gleichnamigen Eintrag.

menu-tagline = Deutsche Eisenbahnsimulation
menu-drive = Fahren
menu-drive-hint = Modul, Fahrzeug und Fahrt
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
menu-title = Zurück zum Hauptmenü
menu-title-hint = Beendet die Fahrt und baut die Welt ab — nicht gewerteter Fortschritt im Szenario geht verloren.
menu-step = Schritt { $step } von { $total }
menu-select-line = Modul auswählen
menu-select-line-hint = Wo die Fahrt stattfindet.
menu-select-loco = Fahrzeug auswählen
menu-select-loco-hint = Was an der Spitze des Zuges läuft.
menu-select-run = Fahrt auswählen
menu-select-run-hint = Ein Szenario, eine Leistung aus dem Fahrplan des Tages — oder freie Hand.
# Die eingebauten Inhalte, auf die der Simulator zurückfällt, wenn nichts gewählt wird.
# Das Fähnchen an der Zeile sagt das, der Name selbst muss es nicht mehr.
menu-chip-builtin = Integriert
menu-chip-composition = Komposition
menu-line-builtin = Beispielmodul
menu-loco-builtin = BR 101
menu-scenario-none = Kein Szenario — freie Fahrt
menu-free-run = Kein Fahrplan und keine Wertung: das Modul, das Fahrzeug, und wohin Sie damit fahren.
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
menu-fact-scenery = Szenerieobjekte
menu-fact-drive = Antrieb
menu-fact-brake = Bremse
menu-fact-start = Beginn
menu-fact-timetable = Fahrplan
menu-fact-line = Modul
menu-fact-events = Ereignisse
menu-fact-km = { $value } km
menu-fact-m = { $value } m
menu-fact-t = { $value } t
menu-fact-kmh = { $value } km/h

## Fahrplanfahrten
#
# Neben den Szenarien lässt sich eine Strecke aus ihrem Betriebstag fahren: der
# ganze Fahrplan eines Tages, der alle 24 Stunden von vorn beginnt und aus dem
# der Spieler eine Leistung übernimmt. Ein Szenario bringt seine Stunde und
# seinen Himmel mit; eine Leistung liegt jedes Mal zur selben Stunde auf
# derselben Strecke — deshalb werden Datum und Wetter hier eingestellt, auf dem
# Schritt zwischen Auswahl und Start.

# Die Überschrift über den Leistungen eines Betriebstags in der Fahrtauswahl.
menu-day-heading = Fahrplan · { $name }
# Eine Leistung darin: wo sie beginnt und wo sie endet.
menu-service = { $from } – { $to }
menu-run-setup = Fahrt einrichten
menu-run-setup-hint = An welchem Tag sie fährt, und was das Wetter darüber macht.
menu-fact-train = Zug
menu-fact-departure = Abfahrt
menu-fact-arrival = Ankunft
menu-fact-stops = Halte
run-date = Datum
run-date-hint = Entscheidet über Jahreszeit und Sonnenstand.
run-weather = Wetter
run-weather-hint = Vom Tag selbst gemacht — oder vorgegeben und gehalten.
run-weather-dynamic = Dynamisch
run-weather-fixed = Selbst gewählt
run-preset = Welches Wetter
run-preset-hint = Gilt von der ersten bis zur letzten Minute.

## Zugwechsel
#
# Ein Triebfahrzeugführer ist für einen Zug zuständig — oder für keinen. Wer
# aussteigt, übergibt die Leistung an die KI; wer in einen anderen Zug geht und
# sich an dessen Pult setzt, übernimmt diesen. Jede Ablehnung wird angezeigt und
# nicht verschluckt.

crew-took-over = { $train } übernommen.
crew-handed-over = { $train } übergeben — die KI fährt.
crew-secured = { $train } gesichert abgestellt.
crew-not-aboard = Nicht vom Bahnsteig aus: erst in den Führerstand.
crew-out-of-service = Dieser Zug ist außer Dienst.
crew-nothing-to-drive = In diesem Zug zieht nichts.
crew-scenario-train = Ein Szenario wird im eigenen Zug gefahren.
crew-another-driver = Diesen Zug fährt jemand anderes.

## Einstellungen
#
# Eine Seite des Hauptmenüs, als TOML im Einstellungsverzeichnis des Betriebssystems
# abgelegt. Ein -hint-Schlüssel ist die Beschreibung unter dem Namen der Einstellung.

set-input = Eingabe
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
set-volumetric-clouds = Volumetrische Wolken
set-volumetric-clouds-hint = Berechnet die Wolkendecke als Volumen — Quellungen, ein durchleuchtetes Inneres und ein Silberrand, wo die Sonne dahintersteht. Aus zeichnet dieselben Wolken als eine beleuchtete Schicht, genauso scharf und etwa ein Zwanzigstel so teuer.
set-mist = Bodennebel
set-mist-hint = Zeichnet Nebel als Volumen, mit den Strahlen der Sonne darin. Kostet bei Nebelwetter etwa eine halbe Millisekunde pro Bild, bei klarer Sicht nichts.
set-aa = Kantenglättung
set-aa-hint = Welches Verfahren die Kanten glättet. FXAA ist ein billiger Durchgang, SMAA ein schärferer, MSAA löst die Geometrie selbst auf und kostet am meisten.
set-aa-off = Aus
set-aa-fxaa = FXAA
set-aa-smaa = SMAA
set-aa-msaa = MSAA
set-aa-quality = Stufe der Kantenglättung
set-aa-quality-hint = Wie hart das gewählte Verfahren arbeitet — die Zahl der Abtastungen bei MSAA, die Voreinstellung bei den beiden anderen.
set-aa-2x = 2 ×
set-aa-4x = 4 ×
set-aa-8x = 8 ×
set-texture-quality = Texturqualität
set-texture-quality-hint = Größe und Filterung der erzeugten Bodentexturen. Gilt auch für das Gelände, das schon zu sehen ist.
set-shadow-quality = Schattenqualität
set-shadow-quality-hint = Kantenlänge der Schattenkarte der Sonne: 1024, 2048 oder 4096 Texel. Eine Stufe kostet die vierfache Zahl an Texeln — die Einstellung, die man zuerst senkt.
set-mist-quality = Nebelqualität
set-mist-quality-hint = Schritte des Raymarch durch den Bodennebel — davon hängt ab, ob ein Sonnenstrahl ein Strahl ist oder eine Treppe.
set-window = Fenster
set-window-hint = Fenster, randlos über den ganzen Monitor oder exklusives Vollbild.
set-window-windowed = Fenster
set-window-borderless = Randlos
set-window-fullscreen = Vollbild
set-max-fps = Bildratenbegrenzung
set-max-fps-hint = Hält den Simulator auf so viele Bilder je Sekunde. Die oberste Stufe ist keine Begrenzung; die vertikale Synchronisation gibt die Rate des Monitors vor, das ist eine andere Frage.
set-fps = { $value } fps
set-fps-unlimited = Unbegrenzt
set-quality-low = Niedrig
set-quality-medium = Mittel
set-quality-high = Hoch
# Shown in place of a quality step for something that is switched off.
set-quality-none = —
set-vsync = Vertikale Synchronisation
set-vsync-hint = Deckelt die Bildrate auf die des Bildschirms, gegen Bildrisse.
set-volume = Gesamtlautstärke
set-volume-hint = Lautstärke von allem, was der Simulator abspielt.
set-language = Sprache
set-language-hint = Wirkt sofort, im Menü wie im Führerstand.
set-language-system = System
set-hud = HUD
set-hud-hint = Voll, oder reduziert auf die Instrumente und die Zugsicherung — F7 geht während der Fahrt durch die drei Stufen.
set-hud-full = Voll
set-hud-reduced = Reduziert
set-hud-off = Aus
set-look-speed = Umsehgeschwindigkeit
set-look-speed-hint = Wie weit sich der Blick dreht, während die rechte Maustaste gehalten wird.
set-controls = Tastenbelegung
set-controls-hint = Welche Taste und welche Controller-Taste jeden Hebel des Führerstands bedienen.
set-reset = Auf Standard zurücksetzen
set-reset-hint = Setzt jede Einstellung dieser Seite auf den Auslieferungszustand.
# Einheiten der Werte am rechten Rand einer Einstellungszeile.
set-metres = { $value } m
set-percent = { $value } %
set-factor = { $value } ×

## Tastenbelegung

# Die Seite hinter der Zeile „Tastenbelegung“ der Einstellungen: eine Zeile je Aktion, die
# Taste links in der Wertespalte und die Controller-Taste rechts. Die Namen sind die Hebel
# selbst, damit sie sich hier wie in der Tastenhilfe (F5) lesen.

ctl-title = Tastenbelegung
ctl-caption = Enter übernimmt die nächste gedrückte Taste oder Controller-Taste — in einer Hebelzeile den nächsten bewegten Stick oder Trigger. Rücktaste löscht die Belegung.
# Was eine Zeile anzeigt, während sie auf ihre neue Taste wartet.
ctl-press = Taste drücken …
# Diese Hälfte der Zeile ist nicht belegt.
ctl-unbound = —
ctl-hint-rebind = neu belegen
ctl-hint-clear = löschen
ctl-reset = Alle Belegungen zurücksetzen
ctl-reset-hint = Setzt jede Taste und Controller-Taste auf den Auslieferungszustand.

ctl-group-driving = Fahren
ctl-group-brakes = Bremsen
ctl-group-safety = Zugbeeinflussung
ctl-group-vehicle = Fahrzeug
ctl-group-shunting = Rangieren
ctl-group-view = Sicht und Einblendungen
ctl-group-walk = Zu Fuß

ctl-throttle-up = Fahrstufe auf
ctl-throttle-down = Fahrstufe ab
ctl-throttle-off = Fahrstufe auf null
ctl-reverser-forward = Richtungswender vorwärts
ctl-reverser-neutral = Richtungswender neutral
ctl-reverser-back = Richtungswender rückwärts
ctl-road-gear = Getriebestufe
ctl-afb = AFB ein/aus
ctl-afb-down = Sollgeschwindigkeit ab
ctl-afb-up = Sollgeschwindigkeit auf

ctl-brake-apply = Führerbremsventil bremsen
ctl-brake-release = Führerbremsventil lösen
ctl-brake-lap = Führerbremsventil abschließen
ctl-brake-fill = Führerbremsventil füllen
ctl-brake-emergency = Schnellbremsung
ctl-direct-brake-apply = Zusatzbremse bremsen
ctl-direct-brake-release = Zusatzbremse lösen
ctl-loco-brake-release = Lokbremse lösen
ctl-parking-brake = Feststellbremse
ctl-ep-brake = Vorgesteuerte Bremse
ctl-sanding = Sanden

ctl-sifa = Sifa quittieren
ctl-pzb-acknowledge = PZB Wachsam
ctl-pzb-free = PZB Frei
ctl-pzb-override = PZB Befehl
ctl-lzb-takeover = LZB Übernahme
ctl-lzb-end = LZB Ende
ctl-lzb-test = LZB Störschalter
ctl-train-type = Zugartschalter
ctl-horn = Signalpfeife

ctl-battery = Batterie
ctl-pantograph = Stromabnehmer
ctl-main-switch = Hauptschalter
ctl-compressor = Luftpresser
ctl-engine-start = Motoranlasser
ctl-headlights = Spitzensignal
ctl-cab-light = Führerraumbeleuchtung
ctl-instrument-light-up = Instrumentenlicht heller
ctl-instrument-light-down = Instrumentenlicht dunkler
ctl-wipers = Scheibenwischer
ctl-door-left = Türfreigabe links
ctl-door-right = Türfreigabe rechts
ctl-door-close = Türen schließen
ctl-couple = An das kuppeln, was davor steht
ctl-uncouple = Hinter dem besetzten Fahrzeug abkuppeln
ctl-take-over = Zug übernehmen
ctl-view-cab = Führerstandssicht
ctl-view-outside = Außensicht
ctl-view-wayside = Streckensicht
ctl-view-walk = Aufstehen und gehen
ctl-look-left = Blick nach links
ctl-look-right = Blick nach rechts
ctl-look-up = Blick nach oben
ctl-look-down = Blick nach unten
ctl-zoom-in = Kamera heran
ctl-zoom-out = Kamera weg
ctl-help-overlay = Tastenhilfe
ctl-diagnostics = Diagnose
ctl-hud-mode = HUD-Stufe
ctl-console = Konsole
ctl-mod-manager = Mod-Verwaltung
ctl-pause = Pausenmenü

ctl-walk-forward = Vorwärts gehen
ctl-walk-back = Rückwärts gehen
ctl-walk-left = Nach links gehen
ctl-walk-right = Nach rechts gehen
ctl-walk-run = Laufen
ctl-walk-door = Durch die Tür
ctl-walk-jump = Springen

# Die Hebel, die eine Stellung statt einer Richtung haben: nur ein Stick oder ein Trigger
# kann eine halten, deshalb nehmen ihre Zeilen eine Achse und die Tastenspalte bleibt leer.
ctl-group-levers = Hebel auf einer Achse
ctl-lever-hint = Stick oder Trigger belegen, und der Hebel bleibt, wo er hingeschoben wird; die Tasten dafür werden dann nicht mehr gelesen.
ctl-lever-throttle = Fahrschalter
ctl-lever-brake-valve = Führerbremsventil
ctl-lever-direct-brake = Zusatzbremse
# Was eine Hebelzeile anzeigt, während sie auf ihre Achse wartet.
ctl-move = Achse bewegen …

## Mod-Verwaltung

mods-title = Mods
mods-none = Keine Mods installiert — ein Mod-Verzeichnis nach mods/ legen.
mods-missing-depends = benötigt: { $depends } (fehlt oder ist abgeschaltet)
mods-content = Inhalte: { $vehicles } Fahrzeuge, { $lines } Module, { $compositions } Kompositionen, { $scenarios } Szenarien, { $timetables } Fahrpläne, { $signals } Signaltypen, { $scripts } Skripte
mods-log = Warnungen:
mods-restart = Änderung wirkt nach einem Neustart.
mods-keys = ↑/↓ auswählen   Enter ein/aus   F9 schließen

## Konsole

console-hint = Enter ausführen · Tab vervollständigen · ↑/↓ Verlauf · Esc schließen
console-unknown = Unbekannter Befehl: { $name } — help listet sie auf
console-unknown-weather = Unbekanntes Wetter: { $name }
console-weather-list = Vorgaben: { $list }
console-weather-now = { $weather } · { $rate } mm/h · Wind { $wind } m/s · { $temp } °C
console-weather-set = Das Wetter schlägt um auf { $weather } — es zieht über fünf Minuten auf
console-weather-asked = Der Server wird um { $weather } gebeten
console-time-now = { $time }
console-time-set = Die Uhr springt auf { $time } — die Züge behalten ihren Zustand, der Plan zieht nach
console-time-mp = Die Uhr lässt sich nur im Einzelspieler bewegen
console-usage-weather = weather [preset]
console-usage-time = time [HH:MM[:SS]]
console-usage-help = help [command]
console-usage-clear = clear
console-help-weather = das Wetter ändern — ohne Name zeigt es das aktuelle
console-help-time = die Uhr auf eine Tageszeit vorstellen — ohne Zeit zeigt sie die aktuelle
console-help-help = die Befehle auflisten
console-help-clear = den Log leeren

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

## Inhalte-Schublade

tag-add = Hinzufügen
tag-add-placeholder = Neuer Tag
tag-remove-hint = Entfernt den Tag
group-tags = Tags
tags-hint = Frei wählbar, zum Wiederfinden in der Inhalte-Schublade — klein geschrieben, Wörter mit Bindestrich. Enter fügt einen hinzu.
drawer-title = Inhalte
drawer-filter-placeholder = Nach Name oder Schlüssel filtern
drawer-count-filtered = { $shown } von { $total }
drawer-empty = Nichts vorhanden — kein installierter Mod bringt diese Art.
drawer-source-all = Alle Mods
drawer-system-all = Alle Systeme
drawer-tag-all = Alle Tags
drawer-empty-filtered = Der Filter trifft nichts.
drawer-reset-filters = Filter zurücksetzen
drawer-objects = Szenerieobjekte
drawer-signal-types = Signaltypen
drawer-signal-models = Signalmodelle
drawer-track-types = Gleisarten
action-content-drawer = Inhalte-Schublade (Strg+Leertaste)

## Fahrzeugeditor: neue Abschnitte der Datenspalte und Vorlagen

group-metadata = Beschreibung
group-key-figures = Kennzahlen
group-checks = Prüfungen
menu-new-from-template = Neu aus Vorlage
status-new-from-template = Ausgehend von { $name }

## Fahrzeugeditor: Bremsgewicht und Übergangszeiten je Bremsstellung

brk-weight-g = Bremsgewicht G
brk-weight-g-hint = t in Stellung G — aus der Anschrift, beladenes Fahrzeug; 0 = wie das Bremsgewicht darüber
brk-weight-p = Bremsgewicht P
brk-weight-p-hint = t in Stellung P — aus der Anschrift, beladenes Fahrzeug; 0 = wie das Bremsgewicht darüber
brk-weight-r = Bremsgewicht R
brk-weight-r-hint = t in Stellung R — bei Magnetschienenbremse der angeschriebene Wert „R + Mg“; 0 = wie das Bremsgewicht darüber
brk-apply-time-g = Ansprechzeit G
brk-apply-time-g-hint = s, in denen der Bremszylinder in Stellung G auf 95 % kommt; 0 = UIC-Richtwert 22 s
brk-apply-time-p = Ansprechzeit P/R
brk-apply-time-p-hint = s, in denen der Bremszylinder in den Stellungen P und R auf 95 % kommt; 0 = UIC-Richtwert 4 s
brk-release-time-g = Lösezeit G
brk-release-time-g-hint = s, in denen sich der Bremszylinder in Stellung G leert; 0 = UIC-Richtwert 50 s
brk-release-time-p = Lösezeit P/R
brk-release-time-p-hint = s, in denen sich der Bremszylinder in den Stellungen P und R leert; 0 = UIC-Richtwert 17 s

## Fahrzeugeditor: Verzeichnis der Teilfunktionen

partfn-pantograph = Stromabnehmer
partfn-door-left = Tür links
partfn-door-right = Tür rechts
partfn-wiper = Scheibenwischer
partfn-gauge-speed = Geschwindigkeitsmesser
partfn-gauge-brake-pipe = Manometer Hauptluftleitung
partfn-gauge-cylinder = Manometer Bremszylinder
partfn-gauge-main-reservoir = Manometer Hauptluftbehälter
partfn-gauge-tractive-effort = Zugkraftmesser
partfn-switch-throttle = Stellung des Fahrschalters
partfn-switch-reverser = Stellung des Richtungsschalters
partfn-switch-direct-brake = Stellung der Zusatzbremse
partfn-switch-cab-light = Führerstandsbeleuchtung
partfn-switch-instrument-light = Instrumentenbeleuchtung
partfn-lamp-main-switch = Leuchtmelder Hauptschalter
partfn-lamp-sanding = Leuchtmelder Sanden
partfn-prefix-gauge = Zeiger eines Melders
partfn-prefix-lamp = Leuchtmelder
partfn-prefix-digit = Ziffer einer Zifferanzeige
partfn-unknown = Der Simulator kennt keine Funktion dieses Namens — das Teil bleibt in Ruhelage
partfn-unknown-indicator = Kein Zugsicherungssystem veröffentlicht einen Melder dieses Namens
partfn-digit-needs-place = Eine Ziffer braucht ihre Dezimalstelle: digit:Melder:Stelle

## Fahrzeugeditor: Neu aus Vorlage

tpl-group-powered = Triebfahrzeuge
tpl-group-hauled = Wagen
tpl-name = { $base } (Kopie)
tpl-br101-hint = Drehstromantrieb mit Umrichter und Nutzbremse, Scheibenbremse in Stellung R, PZB 90 und LZB an Bord — die moderne Streckenlok.
tpl-br110-hint = Trafo mit Schaltwerk auf Reihenschlussmotoren, keine E-Bremse, Klotzbremse in Stellung P, nur Indusi und Sifa.
tpl-br218-hint = Dieselhydraulisch: drehzahlgeregelter Motor und zwei Wandler, Klotzbremse in Stellung P, PZB 90 und Sifa.
tpl-br232-hint = Dieselelektrisch: Motor, Generator und Leistungsregler auf sechs Gleichstrommotoren, Klotzbremse in Stellung P, PZB 90 und Sifa.
tpl-br52-hint = Dampf: Feuer, Kessel und Zylinder als ein Kreislauf, Klotzbremse in Stellung G, keine Zugsicherung.
tpl-railcar-hint = Dieseltriebwagen mit zwei Motoren, hydrodynamischer Bremse im Getriebe, Scheiben- und Magnetschienenbremse, PZB 90 und Türautomatik.
tpl-coach-hint = Reisezugwagen ohne Antrieb, KE-GPR-Scheibenbremse in Stellung P, Türen, aber keine eigene Zugsicherung.
tpl-eaos-hint = Offener Güterwagen ohne Antrieb, Klotzbremse in Stellung G mit Umstellung leer/beladen, keine Zugsicherung.
tpl-eaos-k-hint = Derselbe Wagen mit K-Ventil: abstufbar bremsend, aber einlösig — vor dem nächsten kräftigen Bremsen muss er erst wieder aufgefüllt werden.

## Fahrzeugeditor: Display-Widgets
##
## Der code-freie Inhalt eines Bildschirms: Beschriftungen, Werte und Balken in
## Texturpixeln, in Listenreihenfolge gezeichnet — ein späteres Widget deckt ein
## früheres ab. Die Vorschau zeigt das Layout, nicht die Anzeigewerte; die sind
## Simulationszustand.

disp-widget-list = Widgets
disp-widgets-empty = keine Widgets — der Bildschirm bleibt schwarz, solange ihn nicht das Fahrzeugskript zeichnet
disp-widget-count = { $count } Widgets
disp-html-overrides = die HTML-Seite zeichnet diesen Bildschirm allein — eine daneben gepflegte Widget-Liste erreicht die Textur nie
disp-preview-note = Layout-Vorschau, die Werte sind Platzhalter — ein Widget mit der Maus ziehen setzt es
action-add-widget = Widget hinzufügen
action-add-widget-hint = ein Element des code-freien Bildschirminhalts — Beschriftung, Wert oder Balken
action-widget-up = einen Platz nach vorn — ein früheres Widget wird von den späteren überdeckt
action-widget-down = einen Platz nach hinten — ein späteres Widget liegt obenauf
disp-widget-kind = Art
disp-widget-kind-hint = der Wechsel behält Position, Farbe und Quelle
disp-widget-label = Beschriftung
disp-widget-value = Wert
disp-widget-bar = Balken
disp-widget-untitled = (ohne Text)
disp-widget-pos = Position
disp-widget-pos-hint = px von der linken oberen Ecke der Textur
disp-widget-text = Text
disp-widget-size = Schriftgröße
disp-widget-size-hint = px — Höhe der Zeichen auf der Textur
disp-widget-box = Balkengröße
disp-widget-box-hint = px — Breite × Höhe des vollen Balkens
disp-widget-source = Quelle
disp-widget-source-hint = die Größe, der Wert oder Balken folgen
disp-source-indicator = Anzeiger …
disp-widget-indicator = Anzeiger
disp-widget-indicator-hint = benannter Anzeigewert der Zugbeeinflussung (mfa_v_soll, mfa_zielentfernung); 0, solange er fehlt
disp-widget-indicator-placeholder = mfa_v_soll
disp-widget-decimals = Nachkommastellen
disp-widget-unit = Einheit
disp-widget-unit-hint = wird mit Leerzeichen hinter die Zahl geschrieben; leer lässt sie weg
disp-widget-scale = Skalierung
disp-widget-scale-hint = der Wert wird vor der Formatierung damit multipliziert — 3,6 macht aus m/s km/h
disp-widget-max = Vollausschlag
disp-widget-max-hint = der Wert, bei dem der Balken voll ist
disp-widget-color = Farbe
disp-widget-color-hint = lineares RGBA von Text oder Balken

## Fahrzeugeditor: Metadaten, Varianten und Ladegut

meta-class = Baureihe
meta-class-hint = Baureihe oder Gattung, wie sie angeschrieben ist: BR 101, Bmz 236
meta-manufacturer = Hersteller
meta-manufacturer-hint = Wer das Fahrzeug gebaut hat — Adtranz, Siemens, MBB
meta-year = Baujahr
meta-year-hint = Jahr der Ablieferung; wo die Quelle nichts sagt, bleibt das Feld leer
meta-year-unset = keine Angabe
meta-epoch = Epoche
meta-epoch-hint = Freier Text, und er darf zwei umfassen: V, IV–VI
meta-country = Land
meta-country-hint = Wo das Fahrzeug fährt. Die Liste führt die Länder, für die der Simulator Signale und Zugsicherung hat; alles andere kommt als ISO-3166-1-alpha-2-Code in das Feld daneben
meta-country-unset = keine Angabe
meta-country-other = Code
meta-operator = Betreiber
meta-operator-hint = Die Bahn, für die das Fahrzeug fährt: DB Fernverkehr, ÖBB
meta-author = Autor
meta-author-hint = Wer diese Datei gebaut hat, nicht wer das Fahrzeug gebaut hat
meta-thumbnail = Vorschaubild
meta-thumbnail-hint = Bild, mit dem der Fahrzeugbrowser das Fahrzeug listet. Es muss wie das Modell unterhalb von mods/ liegen
meta-thumbnail-placeholder = mod/assets/…
meta-thumbnail-pick = Bild unterhalb von mods/ auswählen
meta-description = Beschreibung
meta-description-placeholder = Was der Fahrzeugbrowser zu diesem Fahrzeug sagt
filter-image = Bild

var-heading = Varianten
var-empty = Keine Varianten — das Fahrzeug fährt in einer Ausführung, ohne Betriebsnummer
action-add-variant = Variante hinzufügen
action-add-variant-hint = eine Lackierung, eine Nummernreihe, eine Epoche — nur Aussehen, nie Physik
var-name-placeholder = Name der Variante
var-model = Modell
var-model-hint = glTF-Datei, mit der diese Variante gezeichnet wird; leer = das Modell des Fahrzeugs
var-model-placeholder = leer = Basismodell
var-model-none = Kein Modell — weder die Variante noch das Fahrzeug nennt eines
var-model-effective = Gezeichnet als { $file }
var-epoch = Epoche
var-epoch-hint = Leer = die Epoche des Fahrzeugs
var-numbers = Betriebsnummern
var-numbers-hint = Eine je Zeile. Der Zugbildner zieht eine davon, bestimmt durch den Startwert des Szenarios — so sieht jeder Spieler dieselbe Nummer am selben Fahrzeug
var-numbers-placeholder = 101 001-6
var-description = Beschreibung
var-description-placeholder = Leer = die Beschreibung des Fahrzeugs

load-heading = Ladegut
load-empty = Kein Ladegut — das Fahrzeug fährt leer
action-add-load = Ladegut hinzufügen
action-add-load-hint = Gut, seine Masse und der Modellknoten, der es zeigt
load-name-placeholder = Name des Guts
load-mass = Masse
load-mass-hint = kg Ladegut, zusätzlich zur Eigenmasse
load-node = Modellknoten
load-node-hint = glTF-Knoten, der bei dieser Ladung sichtbar wird — ein Kohlehaufen, ein Containerstapel. Leer = die Ladung ist nicht zu sehen
load-node-placeholder = leer = unsichtbar
load-total = Gesamtmasse { $mass } t
load-capped = Schwerer als die zulässige Zuladung — das Fahrzeug trägt davon { $max } t

## Vehicle editor: key figures and the tractive effort diagram

key-mass = Masse
key-mass-hint = Eigenmasse, hinter dem Pfeil die Masse mit der vollen Zuladung
key-axle-load = Radsatzlast
key-axle-load-hint = Gesamtmasse durch die Zahl der Radsätze. Streckenklasse D trägt 22,5 t, Klasse C 20 t, Klasse B 18 t
key-axle-load-warn = Radsatzlast { $load } t liegt über den { $limit } t der Streckenklasse D — das Fahrzeug ist auf Strecken beschränkt, die sie tragen
key-brake-percentage = Bremshundertstel
key-brake-percentage-hint = Bremsgewicht bezogen auf die Masse. Die Zahl, in der ein Bremszettel geschrieben ist, und die erste Stelle, an der ein vertipptes Bremsgewicht auffällt
key-brake-percentage-g = … in Stellung G
key-brake-percentage-p = … in Stellung P
key-brake-percentage-r = … in Stellung R
key-adhesive-mass = Reibungsmasse
key-adhesive-mass-hint = Masse auf den angetriebenen Radsätzen — davon muss die Zugkraft getragen werden
key-adhesion-limit = Haftungsgrenze
key-adhesion-limit-hint = Zugkraft, die die trockene Schiene im Stand trägt, nach Curtius/Kniffler. Darüber schleudern die Räder
key-starting-effort = Anfahrzugkraft
key-starting-effort-hint = Zugkraft im Stand, in der stärksten Antriebsart
key-power-weight = Leistungsgewicht
key-power-weight-hint = Höchste Leistung am Rad bezogen auf die Masse — wie zügig das Fahrzeug beschleunigt, unabhängig von seiner Größe
key-balancing-speed = Beharrungsgeschwindigkeit
key-balancing-speed-hint = Wo die Zugkraft auf den Fahrwiderstand gefallen ist: was das Fahrzeug in der Ebene aus sich heraus fährt
key-above-v-max = über v max
key-slip-warn = Anfahrzugkraft { $force } kN liegt über den { $limit } kN, die die Reibungsmasse trägt — das Fahrzeug schleudert bei jedem Anfahren
plot-tractive-effort = Zugkraft (km/h → N)
plot-resistance = Fahrwiderstand
plot-dynamic-brake = Dynamische Bremse
plot-adhesion-limit = Haftungsgrenze

## Fahrzeugeditor: Auswahl der Teilfunktion

part-function-pick = Eine Funktion wählen, die der Simulator ausliest

## Vehicle editor: check report
## Findings of the file-wide check next to the data panel. Every one of them names
## what to do, not only what is wrong.

check-length = Länge über Puffer ist 0 — trag sie in Metern ein, sonst steht das nächste Fahrzeug des Zuges in diesem drin
check-mass = Eigenmasse ist 0 — trag sie in Kilogramm ein; ohne sie hat das Fahrzeug weder Trägheit noch Gewicht auf der Schiene
check-gauge = Spurweite ist 0 — trag sie in Metern ein (1,435 für Normalspur), sonst passt das Fahrzeug auf keine Strecke
check-axles = Keine Achszahl — die Bremse rechnet mit der Referenzachslast statt mit der dieses Fahrzeugs, und keine Zugliste stimmt. Trag die Zahl der Achsen ein
check-rotating-mass = Zuschlag für rotierende Massen { $value } liegt außerhalb von 0 … 0,5 — ein Wagen hat etwa 0,05, ein Triebfahrzeug etwa 0,25
check-load-over-payload = Ladung „{ $load }“ wiegt { $mass } t und damit mehr als die zulässige Zuladung von { $max } t — das Fahrzeug trägt nur die Zuladung. Erhöhe sie, oder mach die Ladung leichter
check-doors = Fahrgasttüren, aber keine Türsteuerung — ein Triebfahrzeug gibt seine Türen selbst frei, so öffnet sie nie jemand. Wähle eine Türsteuerung
check-no-brake-weight = Kein Bremsgewicht — das Fahrzeug wird ungebremst mitgeschleppt. Trag den angeschriebenen Wert in Tonnen ein
check-no-brake-force = Keine Bremskraft — der Zylinder drückt nichts ans Rad. Trag die Kraft der voll angelegten Bremse in Newton ein
check-brake-percentage = Die Bremshundertstel ergeben { $value } % — prüfe das Bremsgewicht von { $weight } t gegen die Eigenmasse; ein europäischer Bremszettel bleibt zwischen 30 und 250 %
check-load-braking = Voll beladen erreicht das Fahrzeug bei { $payload } t Zuladung nur noch { $value } % Bremshundertstel — das Bremsgewicht folgt der Last nicht. Bau ein Wiegeventil ein, oder eine Umstellvorrichtung Leer/Beladen
check-drive-no-adhesion = Das Fahrzeug hat einen Antrieb, aber keine Treibachse bringt seine Kraft auf die Schiene — es fährt nicht an. Trag den Anteil der Masse auf Treibachsen ein, oder markiere Achsen im Schaltbild als angetrieben
check-adhesion-no-drive = Gewicht auf Treibachsen, aber keine Antriebskette — der Wert bewirkt nichts. Setz den Reibungsmassenanteil auf 0, oder ergänze einen Antrieb
check-no-v-max = Keine Höchstgeschwindigkeit angegeben — Tacho und AFB fallen auf 160 km/h zurück. Trag die Grenze des Laufwerks ein
check-drive-over-v-max = Der Antrieb zieht bis { $drive } km/h und damit über die Laufwerksgrenze von { $vehicle } km/h hinaus — dort bremst das Fahrzeug nichts ein. Erhöhe die Höchstgeschwindigkeit, oder begrenze den Antrieb
check-tractive-effort = Die Anfahrzugkraft von { $force } kN liegt über dem, was eine trockene Schiene hält ({ $limit } kN) — das Fahrzeug schleudert, statt anzufahren. Verringere die Kraft, oder bring mehr Gewicht auf die Treibachsen
check-model-no-file = Das Modell nennt keine glTF-Datei — trag die Datei unterhalb von mods/ ein, oder nimm das Modell heraus
check-part-node = Bewegtes Teil an „{ $node }“: Das Modell hat keinen Knoten dieses Namens — das Teil bewegt sich nie. Korrigiere den Namen, oder nimm das Teil heraus
check-part-function = Bewegtes Teil an „{ $node }“: { $reason }. Das Teil bleibt stehen; wähle eine Funktion, die der Simulator auswertet — es sei denn, ein eigener Mod liest diese hier
check-control-node = Führerstandsbedienung an „{ $node }“: Das Modell hat keinen Knoten dieses Namens — die Bedienung lässt sich nicht anfassen. Korrigiere den Namen
check-display-node = Anzeige „{ $name }“ an „{ $node }“: Das Modell hat keinen Knoten dieses Namens — der Bildschirm bleibt dunkel. Korrigiere den Namen
check-load-node = Ladung „{ $load }“ zeigt „{ $node }“: Das Modell hat keinen Knoten dieses Namens — die Ladung bleibt unsichtbar. Korrigiere den Namen, oder lass den Knoten leer
check-node-twice = Knoten „{ $node }“ ist mehrfach belegt — nur die erste Bindung wirkt, die übrigen fallen weg. Gib jedem Teil einen eigenen Knoten
check-lod-duplicate = Detailstufe { $level } ist zweimal aufgeführt — eine Sichtweite je Stufe
check-lod-order = Die Sichtweite von Stufe { $level } ist nicht größer als die der Stufe davor — führe die Stufen mit steigender Weite auf, die gröbste zuletzt
check-lod-no-nodes = Detailstufe { $level } ist aufgeführt, aber kein Knoten heißt _LOD{ $level } — die Stufe zeichnet nichts. Nimm sie heraus, oder benenne die Knoten um
check-lod-missing = Das Modell bringt _LOD-Knoten mit, aber das Fahrzeug führt keine Stufen auf — alle Stufen zeichnen gleichzeitig übereinander. Übernimm die Stufen aus der Datei

## GNT — Geschwindigkeitsüberwachung Neigetechnik

blk-gnt = GNT
blk-gnt-hint = Geschwindigkeitsüberwachung Neigetechnik — gibt im GNT-Bereich die höheren Bogengeschwindigkeiten eines Neigezuges frei. Setzt einen Neigewinkel über null am Fahrzeug voraus
bake-gnt-without-tilt = GNT an einem Fahrzeug ohne Neigetechnik — Neigewinkel setzen, sonst entfällt die Ausrüstung

## Hauptmenü: das Datenblatt des Fahrzeugs in der Detailspalte

menu-fact-variant = Variante
menu-fact-class = Baureihe
menu-fact-manufacturer = Hersteller
menu-fact-build-year = Baujahr
menu-fact-epoch = Epoche
menu-fact-operator = Betreiber
menu-fact-country = Land
menu-fact-author = Autor

## Fahrzeugeditor: Überschrift des Prüfabschnitts

group-checks-errors = Prüfungen — { $errors } Fehler
group-checks-warnings = Prüfungen — { $warnings } Warnungen
group-checks-both = Prüfungen — { $errors } Fehler, { $warnings } Warnungen

# --- Hüllkurve -------------------------------------------------------------
tool-group-module = Modul
tool-envelope = Hüllkurve
tool-envelope-hint = Modulgrenze verlegen: Ecke ziehen, Klick auf eine Seite legt eine neue an, Entf löscht die gewählte Ecke
module-envelope = Hüllkurve
envelope-points = Ecken: { $count }
envelope-anchor-lat = Anker Breite
envelope-anchor-lon = Anker Länge
envelope-min-points = Ein Polygon braucht drei Ecken.
action-edit-envelope = Bearbeiten
action-reset-envelope = Zurücksetzen
action-reset-envelope-hint = Legt wieder eine quadratische Hüllkurve um den Anker — ein Modul ohne bekommt hier seine erste.
sel-envelope-summary = Hüllkurven-Ecke { $index } von { $count }
status-envelope-none = Dieses Modul hat noch keine Hüllkurve — unter Modulgrenzen zurücksetzen.
status-envelope-point-added = Ecke angelegt — an ihren Platz ziehen.
status-envelope-no-hit = Nichts getroffen: eine Ecke ziehen oder auf eine Seite klicken, um eine anzulegen.
status-outside-envelope = Außerhalb der Hüllkurve — dieser Boden gehört dem Nachbarmodul.
status-forest-baked-clipped = { $count } Bäume gesetzt, { $dropped } außerhalb der Hüllkurve verworfen
status-forest-imported-clipped = { $count } Bäume aus { $areas } Flächen, { $dropped } außerhalb der Hüllkurve verworfen
action-cancel = Abbrechen

# --- Menschen: Fußwege und Gehflächen ---------------------------------------
tool-group-people = Menschen
tool-walk-path = Fußweg
tool-walk-path-hint = Klicks setzen die Punkte eines Weges, den Menschen entlanggehen; Enter oder Rechtsklick schließt ihn ab, Esc bricht ab · an einem gezeichneten Weg: Punkt ziehen, Klick auf eine Seite des gewählten Weges legt einen neuen an, Entf löscht den gegriffenen Punkt
tool-walk-area = Gehfläche
tool-walk-area-hint = Klicks umreißen einen Platz, auf dem Menschen unterwegs sind; Enter oder Rechtsklick schließt ihn, Esc bricht ab · an einer gezeichneten Fläche: Ecke ziehen, Klick auf eine Seite der gewählten Fläche legt eine neue an, Entf löscht die gegriffene Ecke
walk-path-active = Fußweg — Punkte: { $points } · Enter oder Rechtsklick schließt ab, Esc bricht ab
walk-area-active = Gehfläche — Ecken: { $corners } · Enter oder Rechtsklick schließt sie, Esc bricht ab
walk-count = Fußwege: { $paths } · Gehflächen: { $areas }
walk-name = Name
walk-name-hint = Freie Bezeichnung — Panel und Prüfung nennen den Weg danach
walk-width = Breite
walk-width-hint = Über diese Breite des Weges verteilen sich die Menschen
walk-people = Menschen
walk-people-hint = Wie viele gleichzeitig darauf unterwegs sind — null ist erlaubt, ein Weg darf angelegt werden, bevor er belebt wird
walk-share = Anteil Gehende
walk-share-hint = Anteil der Menschen, die zwischen Stellen umhergehen statt zu stehen, 0 … 1
walk-height = Höhe über Grund
walk-height-hint = Über dem Gelände unter jedem Punkt — eine Fußgängerbrücke, ein modellierter Bahnsteig
walk-tags = Tags
walk-tags-hint = Kommagetrennt, kebab-case in Kleinbuchstaben wie überall
sel-walk-path-summary = Fußweg { $index } — Punkte: { $points }
sel-walk-area-summary = Gehfläche { $index } — Ecken: { $corners }
sel-walk-vertex = Punkt { $index } von { $count } gegriffen — Entf löscht ihn
status-walk-path-points = Ein Fußweg braucht mindestens zwei Punkte — den nächsten klicken
status-walk-area-points = Eine Gehfläche braucht mindestens drei Ecken — die nächste klicken
status-walk-vertex-added = Punkt angelegt — an seinen Platz ziehen.

## Rangieren

# Warum der Rangierer das Kuppeln oder Abkuppeln abgelehnt hat. Jeder Grund ist
# eine Bedingung, die nicht erfüllt war; sie stehen im HUD hinter „nicht möglich“.
shunt-refused-no-train = kein Zug vorhanden
shunt-refused-no-coupler = der Zugverband hat diese Kupplung nicht
shunt-refused-couplers = die Kupplungen passen nicht zueinander
shunt-refused-moving = noch in Bewegung
shunt-refused-too-fast = zu schnell aufeinander zu
shunt-refused-nothing-in-reach = nichts in Reichweite
shunt-refused-same-train = das ist dieser Zug

# Die beiden Enden eines Zugverbands.
shunt-end-head = Spitze
shunt-end-tail = Schluss

# Welche Kupplung ein Fahrzeug trägt.
coupler-screw = Schraubenkupplung
coupler-center-buffer = Mittelpufferkupplung
coupler-bar = Kurzkupplungsstange

# Wo Fahrzeuge stehen: ein Gleis auf der Strecke oder ihr Rand.
yard-kind-stabling = Abstellgleis
yard-kind-portal = Portal

# Warum ein Zug nicht aufs Gleis gestellt oder von der Strecke genommen werden konnte.
yard-refused-no-yard = die Strecke hat kein Gleis dieses Namens
yard-refused-no-train = kein Zug vorhanden
yard-refused-too-long = länger als das Gleis
yard-refused-occupied = das Gleis ist besetzt
yard-refused-off-the-graph = das Gleis hinter der Marke reicht nicht
yard-refused-not-a-portal = nur ein Portal nimmt einen Zug von der Strecke
yard-refused-not-there = der Zug steht nicht an diesem Portal
yard-refused-moving = noch in Bewegung

# --- Felder ------------------------------------------------------------------
# Ackerflächen aus den Agrarregistern (InVeKoS), gezeichnet nach dem
# Wachstumsstadium ihrer Kultur am Tag der Fahrt.

# Die zwölf Kulturgruppen, die der Simulator unterscheiden kann. Nicht die
# Codeliste des Registers — die hat Hunderte Einträge, und aus dem Zug sieht
# niemand den Unterschied zwischen zwei Winterweizensorten.
crop-winter-cereal = Wintergetreide
crop-summer-cereal = Sommergetreide
crop-maize = Mais
crop-rapeseed = Raps
crop-sugar-beet = Zuckerrüben
crop-potato = Kartoffeln
crop-legume = Hülsenfrüchte
crop-grassland = Grünland
crop-vegetable = Gemüse
crop-orchard = Obstanlage
crop-vineyard = Weinberg
crop-fallow = Brache
crop-other = Sonstiges

# Woher die Kultur eines Feldes bekannt ist.
field-level-declared = beantragt
field-level-group = aus der Kulturgruppe
field-level-drawn = aus der Anbaustatistik

# Was am Tag der Fahrt auf dem Feld passiert.
growth-bare = gepflügt
growth-emerging = im Auflaufen
growth-growing = im Wachstum
growth-flowering = in Blüte
growth-ripening = in der Reife
growth-ripe = reif
growth-stubble = Stoppel

# Das Panel.
heading-fields = Felder
tool-field = Feld
tool-field-hint = Klicks umreißen ein Feld; Enter oder Rechtsklick schließt es · Esc bricht ab · der übliche Weg zu Feldern ist der Import
field-count = { $count } Felder
field-list-empty = Noch keine Felder — importieren oder mit dem Feldwerkzeug zeichnen
field-crop = Kultur
field-crop-hint = Was hier wächst — die Gruppe, die der Simulator zeichnet, nicht der Code des Registers
field-direction = Bearbeitungsrichtung
field-direction-hint = Wie das Feld bearbeitet wurde, gegen Gitter-Ost; Furchen und Fahrgassen laufen entlang
field-area = Fläche
field-growth = Heute
field-growth-hint = Was die Kultur am angezeigten Datum macht — aus Kultur, Datum und dem Saatwert des Feldes, nichts davon gespeichert
field-growth-detail = { $cover } % Deckung · { $height } m
field-active = Feld: { $corners } Ecken — Enter schließt es
field-source-row = { $land } { $year }
field-attribution = Quellenvermerk

# Der Importdialog.
action-import-fields = Felder importieren…
field-import-title = Felder importieren
field-import-intro = Ackerflächen aus den Agrarregistern der Länder. Es wird nichts geschrieben, bevor du es sagst.
field-import-scope = Umfang
field-import-scope-hint = Das ganze Modul innerhalb seiner Hüllkurve oder nur das ausgewählte Feld
field-import-scope-module = Das ganze Modul
field-import-scope-selection = Das ausgewählte Feld
field-import-cut = An der Grenze schneiden
field-import-cut-hint = Felder an der Hüllkurve schneiden, damit der Rest dem Nachbarn gehört. Aus behält jedes Feld ganz, dessen Mitte drinnen liegt
field-import-min-area = Kleinstes Feld
field-import-min-area-hint = Darunter ist eine Parzelle ein Randstreifen, und eine Strecke will keine zehntausend davon
field-import-clearance = Abstand zum Gleis
field-import-clearance-hint = Wie weit die Felder vom Bahnkörper wegbleiben, damit keins auf dem Damm liegt, den das Gelände auf Schienenhöhe zieht
field-import-refresh = Neu abrufen
field-import-refresh-hint = Die Dienste fragen statt den Zwischenspeicher zu lesen — für ein Register, das weitergezogen ist
field-import-start = Importieren
field-import-stop = Anhalten
field-import-stopped = Angehalten — was bis dahin gefunden wurde, steht unten.
field-import-failed = Der Import ist nicht fertig geworden.
field-import-of = { $done } von { $total }
field-import-locating = Ermittle die Bundesländer
field-import-fetching = Frage das Register
field-import-mapping = Lese die Kulturen
field-import-cleaning = Schneide zurecht
field-import-done = Fertig
field-import-found = { $fields } Felder, { $hectares } ha
field-import-counts = { $parcels } Parzellen · { $small } zu klein · { $outside } außerhalb · { $split } geteilt
field-import-tiles = { $fetched } abgerufen, { $cached } aus dem Zwischenspeicher
field-import-unknown-codes = Kulturcodes, die keine Tabelle kennt: { $codes }
field-import-commit = Ins Modul übernehmen
field-import-again = Einstellungen ändern
field-import-no-envelope = Das Modul hat keine Hüllkurve, in die importiert werden könnte — erst eine anlegen
field-import-no-selection = Es ist kein Feld ausgewählt

# Was jedes Land veröffentlicht, und unter welcher Lizenz.
field-source-gsa = Schläge mit Kulturart
field-source-lpis = nur Feldblöcke
field-source-osm = kein Register — stattdessen OpenStreetMap
field-source-abroad = außerhalb der Register
field-source-abroad-hint = Der Ansatz ist national: die Register, ihre Schemata und ihre Kulturcodelisten sind es alle. Außerhalb Deutschlands kommen die Felder stattdessen aus OpenStreetMap — dünner als ein Register, und share-alike
field-source-none = nichts veröffentlicht
field-licence-unclear = Lizenz unklar
field-licence-unclear-hint = Dieses Land hat keine offene Lizenz genannt. Bauen kann man damit; vor einer Veröffentlichung muss die Antwort schriftlich vorliegen.
field-licence-unclear-land = { $land }: Lizenz vor der Veröffentlichung schriftlich klären

status-fields-imported = { $count } Felder übernommen
status-field-points = Ein Feld braucht mindestens drei Ecken — die nächste anklicken
check-field-small = Feld { $field } hat weniger als drei Ecken
check-field-crop = Feld { $field } nennt eine Kultur, die keine Tabelle kennt — es wird als nackter Boden gezeichnet
