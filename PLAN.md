# Entwicklungsplan: Deutscher Zugsimulator auf Bevy (Arbeitstitel „TrainSim-DE")

Dieser Plan ist als vollständige Arbeitsanweisung für eine ausführende KI/ein Entwicklerteam geschrieben.
Referenz-Featureumfang: MaSzyna EU07 (https://github.com/MaSzyna-EU07/maszyna), übertragen auf Deutschland
mit deutschen Zugsicherungssystemen (PZB, LZB, Sifa, …) und einer Länder-Abstraktionsschicht.

---

## 0. Zielbild

Ein Simulator in Simulationstiefe von MaSzyna (kein Arcade-Spiel):

- Physikalisch korrekte Fahrdynamik ganzer Züge (Längsdynamik, Kupplungen, Puffer).
- Vollständige Druckluftbremssimulation (Hauptluftleitung als Rohrmodell, nicht nur „Bremskraft-Slider").
- Elektrische Ausrüstung als Schaltungssimulation (Stromabnehmer, Hauptschalter, Fahrmotoren/Umrichter).
- Deutsche Zugsicherung: Sifa, PZB 90, LZB 80/CE, später ETCS — hinter einer länderneutralen Abstraktion.
- Deutsches Signalsystem (Ks, H/V, Hl) mit Stellwerkslogik (Fahrstraßen).
- KI-Züge mit Fahrplänen, Ereignis-/Szenariosystem.
- Begehbarer, voll bedienbarer 3D-Führerstand.
- Große Welten (100+ km Strecken), georeferenziert (ETRS89/UTM), ohne f32-Präzisionsartefakte,
  robust über UTM-Zonengrenzen hinweg.

Nicht-Ziele (v1): Multiplayer (nur architektonisch vorbereiten), Fahrgastsimulation, dynamische Güterlogistik.

---

## 1. Feature-Inventar aus MaSzyna (Soll-Abdeckung)

Jedes Feature bekommt im Plan unten ein Kapitel. Checkliste dessen, was MaSzyna kann und wir abdecken müssen:

| MaSzyna-Feature | Übernahme | Kapitel |
|---|---|---|
| Längsdynamik ganzer Züge (Kupplung/Puffer-Federn, Reißen von Kupplungen) | ja | 6 |
| Adhäsionsmodell, Schleudern/Gleiten, Sanden | ja | 6 |
| Druckluftbremse (Führerbremsventil, Steuerventile, HL/HB-Drücke, Bremsstellungen G/P/R(+Mg)) | ja | 7 |
| Elektrische Traktion (Fahrschalter, Anfahrwiderstände / Schaltwerk / Umrichter, Feldschwächung) | ja (dt. Fahrzeuge: Schaltwerk BR 110/140, Umrichter BR 101/185/423, Diesel BR 218/br 648) | 8 |
| Oberleitung mit Spannung/Stromabnehmer-Interaktion, Hauptschalter, Zugsammelschiene | ja | 8 |
| Zugsicherung (dort SHP/CA/Radiostop) | ersetzt durch Sifa/PZB/LZB + Abstraktion | 9 |
| Landesspezifische Signale (dort PL) | ersetzt durch Ks/H/V/Hl + Stellwerk | 10 |
| Zugfunk (dort Radio-Zew) | GSM-R-artiger Zugfunk, Nothalt-Ruf | 10.5 |
| KI-Fahrer, Fahrpläne, Zugbildung | ja | 11 |
| Event-/Szenariosystem (Trigger, Weichen stellen, Ansagen) | ja | 11.4 |
| Interaktiver 3D-Führerstand (alle Schalter klickbar, Instrumente) | ja | 12 |
| Innen-/Außenkameras, freie Kamera | ja | 12.4 |
| 3D-Sound (Fahrgeräusche, Ansagen, positional) | ja | 13 |
| Wetter, Tag/Nacht, Jahreszeiten | ja | 14 |
| Streckeneditor / Content-Pipeline (dort .scn/.e3d) | eigenes Format + Importer | 15 |
| Physik-Konsole/Debug-Tools | ja (egui-Overlays) | 16.3 |

---

## 2. Tech-Stack

- **Sprache:** Rust (stable), Edition 2024.
- **Engine:** Bevy, aktuellste stabile Version zum Projektstart (>= 0.15). Rendering, ECS, Asset-System,
  Audio (`bevy_audio` reicht zunächst; bei Bedarf `kira`).
- **UI:** `bevy_egui` für Debug/Editor-UI; Führerstand-Instrumente als 3D-Meshes + Shader, nicht als 2D-UI.
- **Geodäsie:** `proj4rs` oder `geodesy` (reines Rust) für ETRS89/UTM ↔ ECEF/ENU-Umrechnung im Importer.
- **Serialisierung:** `serde` + RON für handeditierbare Configs, binäres Format (`bincode`/eigenes Chunk-Format)
  für kompilierte Streckendaten.
- **Physik:** Eigenimplementierung (Längsdynamik/Pneumatik sind domänenspezifisch — Rapier o. Ä. hilft hier nicht,
  nur ggf. für Kollisionen/Ragdoll-Kram später).
- **Repo-Struktur:** Cargo-Workspace, ein Crate pro Domäne (siehe 3.1).

---

## 3. Architektur

### 3.1 Workspace-Layout

```
train-sim/
├─ crates/
│  ├─ sim-core/          # Fixed-Timestep-Simulation, KEINE Bevy-Abhängigkeit (testbar, headless)
│  │   ├─ physics/       #   Längsdynamik, Adhäsion
│  │   ├─ brakes/        #   Pneumatik
│  │   ├─ electric/      #   Traktion, Bordnetz
│  │   ├─ safety/        #   Zugsicherungs-Abstraktion + Länderimplementierungen
│  │   └─ interlock/     #   Signale, Fahrstraßen, Blocklogik
│  ├─ world-coords/      # f64-Weltkoordinaten, Origin-Shifting, Geo-Umrechnung
│  ├─ track-model/       # Gleisgeometrie, Topologie, Streckendaten-Format
│  ├─ content/           # Asset-Formate, Importer (inkl. Geo-Reprojektion), Fahrzeug-Definitionen
│  ├─ ai-driver/         # KI-Triebfahrzeugführer, Fahrplan
│  ├─ app/               # Bevy-App: Rendering, Kamera, Input, Audio, UI; bindet sim-core an ECS
│  └─ editor/            # Streckeneditor (eigene Bevy-App, nutzt app-Crates)
└─ assets/
```

**Kernregel:** `sim-core` ist eine reine Rust-Lib mit fixem Zeitschritt (empfohlen 100–200 Hz Physik,
Pneumatik ggf. Substeps), deterministisch, ohne Bevy. Die Bevy-App tickt sie und spiegelt Zustand in
ECS-Komponenten für Rendering/Audio/UI. Das macht alles headless testbar und hält Multiplayer später offen.

### 3.2 Datenfluss pro Frame

1. Input → Führerstand-Bedienelemente (Stellwerte).
2. `sim-core::step(dt)` in festen Schritten (Akkumulator): Elektrik → Traktion/Bremse → Längsdynamik →
   Position auf Gleisgraph → Zugsicherung → Stellwerk/Signale → KI.
3. Sync nach ECS: Fahrzeugposen (f64 → gerendert relativ zum Origin), Instrumentenwerte, Soundtrigger.
4. Rendering/Audio/UI.

---

## 4. Koordinatensystem & große Welten (kritisch, zuerst festzurren)

### 4.1 Problemstellung

- Deutschland liegt in UTM-Zone 32N und 33N (Grenze bei 12° Ost, quer durch Ostdeutschland).
  Strecken (z. B. Berlin–Hannover) kreuzen die Zonengrenze. UTM-Koordinaten zweier Zonen sind
  nicht zusammensteckbar; an der Naht entstehen Sprung + Richtungs-/Maßstabsfehler.
- f32 (Bevy `Transform`) hat bei UTM-Ostwerten (~500 000 m) nur noch ~3 cm Auflösung → Jitter.

### 4.2 Lösung (verbindlich)

**Interne Weltkoordinaten: ECEF in f64.** Ein globales, kartesisches, projektionsfreies System —
keine Zonen, keine Nähte, kein Verzerrungsproblem, ganz Deutschland (oder Europa) in einem Frame.

- Alle Simulation (Gleisgeometrie, Fahrzeugpositionen) rechnet in `DVec3` (f64) im ECEF-Frame
  (ETRS89-Ellipsoid). f64 hat bei Erdradius ~6,4e6 m eine Auflösung < 1 µm — mehr als genug.
- **Rendering: Floating Origin.** Ein `RenderOrigin`-Ressource hält Position (DVec3) + Rotation
  (lokales ENU-Frame am Originpunkt, damit „oben" = +Y bleibt). Beim Sync wird jede Pose als
  `(f64-Pose − Origin)` in f32-`Transform` geschrieben. Origin springt neu (an Kameraposition),
  sobald die Kamera > ~4 km vom Origin entfernt ist; beim Sprung wird auch die ENU-Rotation neu
  bestimmt → Erdkrümmung ist automatisch korrekt (weit entfernte Strecke liegt „unter dem Horizont"),
  ohne dass wir sie je explizit modellieren.
- **UTM nur im Importer:** Quelldaten (DB-Geodaten, OSM, DGM in UTM32/33) werden beim Streckenimport
  per `geodesy`/`proj4rs` nach ECEF reprojiziert. Zur Laufzeit existiert UTM nicht.
  Zonengrenzen sind damit vollständig ein Importer-Problem: Importer nimmt pro Datei deren CRS entgegen
  (EPSG-Code in den Metadaten) und rechnet um — fertig.
- Höhen: DGM-Höhen (DHHN2016) über Geoid-Offset (näherungsweise konstant pro Strecke, ~46–50 m in DE;
  v1: konstanter Offset pro Strecke im Streckenheader) auf ellipsoidische Höhe bringen.

`world-coords`-Crate liefert: `EcefPos(DVec3)`, `RenderOrigin`, Sync-System, `geo::to_ecef(lat/lon/h)`,
`enu_frame_at(pos)`. Abnahmetest: zwei Punkte 300 km auseinander, Kamera fährt hin und her, kein Jitter,
keine sichtbaren Sprünge beim Origin-Rebase.

### 4.3 Streaming

- Welt in Tiles (z. B. 2×2 km, Schlüssel = quantisierte ECEF/ENU-Koordinate der Strecke).
- Async-Laden über Bevy-Assets, Laderadius um Kamera + um alle aktiven Züge (KI-Züge simulieren
  auch ungeladen weiter — Gleisgraph + Fahrplandaten sind immer resident, nur Grafik/Detailkollision streamt).

---

## 5. Gleis- & Streckendatenmodell (`track-model`)

- **Topologie:** Graph aus `TrackNode` (Weichen, Stumpfgleisenden, Verbindungen) und `TrackEdge`.
- **Geometrie pro Edge:** Segmentliste aus Gerade / Kreisbogen / Klothoide (Übergangsbogen) + Überhöhungsrampe.
  Gespeichert im lokalen ENU-Frame des Tiles, zur Laufzeit nach ECEF aufgelöst.
  API: `eval(edge, s) -> (EcefPos, Tangente, Überhöhung)` — alles fährt auf Bogenlänge `s`.
- **Zugposition:** Kette von `(EdgeId, s, Richtung)` pro Radsatz/Drehgestell — Fahrzeuge sind gleisgebunden,
  keine Freikörper-3D-Physik fürs Fahren.
- **Weichen:** Zustand (Lage links/rechts, aufgefahren), Umlaufzeit, Verschluss durch Stellwerk.
- **Streckenausrüstung als Trackside-Objekte** mit Position `(EdgeId, s)`: Signale, PZB-Magnete,
  LZB-Linienleiter-Abschnitte, Balisen (ETCS-ready), Geschwindigkeitstafeln (Lf), Neigungswechsel, Bahnsteige,
  Haltetafeln, Blockgrenzen. Länderneutral als `TracksideDevice { kind: DeviceKind, payload: ron::Value }` —
  die Länder-Plugins (Kap. 9) interpretieren ihre Gerätetypen selbst.
- Geschwindigkeits- und Neigungsprofil als Stufenfunktion über `s` pro Strecke.

---

## 6. Fahrphysik (`sim-core::physics`)

Fixed Timestep, pro Zug:

- **Fahrzeug** = Starrkörper auf Gleis mit Masse (leer/beladen), rotierende Massen (Zuschlagfaktor),
  Länge, Laufwiderstand (Davis-Formel a+bv+cv², Fahrzeugdatenbank-Parameter), Bogen- und Steigungswiderstand
  aus Gleisgeometrie.
- **Zugverband:** Fahrzeuge über Kupplungselemente verbunden: Feder-Dämpfer mit Spiel (Schraubenkupplung:
  Zugfeder + Puffer getrennt, Losfahren „Zug strecken" spürbar; Mittelpufferkupplung steifer).
  Kupplungsbruch bei Überlast (konfigurierbar). Integration: semi-implizit Euler, Substepping wenn steif.
- **Adhäsion:** Kraftschlussgrenze µ(v, Schienenzustand [trocken/nass/Laub]), Schleudern/Gleiten pro
  Triebfahrzeug (v1: pro Fahrzeug, nicht pro Radsatz — `// ponytail`-Kommentar setzen), Sanden erhöht µ.
  Gleitschutz/Schleuderschutz als Fahrzeug-Feature.
- **Abnahmetests (headless):** Auslaufversuch gegen Davis-Sollkurve, Anfahren am Berg,
  Bremswegtabellen (siehe 7) gegen Literaturwerte (Minden-Werte) ±5 %.

---

## 7. Bremssystem (`sim-core::brakes`)

MaSzyna-Niveau, d. h. echte Pneumatik:

- **Hauptluftleitung (HL)** als 1D-Rohrmodell entlang des Zuges: pro Fahrzeug ein Volumenknoten,
  Drosselverbindungen zwischen Fahrzeugen → Druckwellen-Laufzeit, Durchschlagsgeschwindigkeit,
  langer Güterzug bremst hinten später. (v1: Knotenmodell reicht; kein PDE-Löser.)
- **Führerbremsventil** (Stellungen Füllen/Fahrt/Abschluss/Betriebsbremsstufen/Schnellbremsung),
  zeitabhängiges Füllen/Entlüften, Angleicher.
- **Steuerventil** pro Fahrzeug (KE-Ventil-Verhalten): Dreidrucksystem, Bremsstellungen **G/P/R**,
  Lastabbremsung (automatisch/manuell umstellbar), Lösestoß, Erschöpfbarkeit über R-Behälter-Volumen.
- **Weitere:** Zusatzbremse (direkte Bremse) auf Tfz, E-Bremse (dynamisch) mit Blending,
  Mg-Bremse (bei R+Mg und Schnellbremsung), Federspeicher/Handbremse, Notbremse (Fahrgast),
  Hauptluftbehälter + Kompressor (Druckwächter), Bremsprobe-Ablauf (voll/vereinfacht) als Szenario-Feature.
- **Ausgabe:** Klotz-/Scheibenbremskraft → in Längsdynamik; Bremszylinder-/HL-/HB-Manometerwerte für Führerstand.
- **Abnahmetest:** Bremshundertstel-Berechnung eines Beispielzugs, Schnellbremsweg aus 100 km/h gegen Sollwert.

---

## 8. Elektrik & Antrieb (`sim-core::electric`)

Komponentenbasierte Bordnetz-Simulation (gerichteter Schaltungsgraph, kein SPICE):

- **Hochspannung:** Stromabnehmer (Heben/Senken mit Laufzeit, Kontakt zur Oberleitung nur wo Fahrdraht
  vorhanden; Schutzstrecken = spannungslose Abschnitte als Trackside-Objekt), Hauptschalter,
  Oberspannungswandler, 15 kV 16,7 Hz (DE) — Spannungsniveau ist Länder-/Streckenparameter.
- **Traktionsstränge** (Fahrzeugdatenbank wählt Typ):
  1. **Trafo + Schaltwerk** (Altbau-E-Loks BR 110/140/141): Stufenschalter mit Schaltzeit, Fahrmotorkennlinien.
  2. **Umrichter/Drehstrom** (BR 101/185/423/ICE): Zug-/Bremskraft-Sollwert über AFB-fähigen Hebel,
     Kennfeld Zugkraft über v, Wirkungsgrad.
  3. **Diesel** (BR 218 hydraulisch, BR 648 mechanisch/hydraulisch): Motorkennfeld, Getriebe/Wandler.
- **Hilfsbetriebe:** Batterie, Zugsammelschiene (Heizung), Kompressor, Lüfter — als Verbraucher mit
  Zuständen, relevant für Aufrüst-Prozedur.
- **Aufrüsten** als vollständige Prozedur: Batterie ein → Bügel heben → Hauptschalter ein → Luftpresser →
  Zugsicherung testen. Checklisten-Ereignisse fürs Tutorial-System.

---

## 9. Zugsicherung (`sim-core::safety`) — Kernstück

### 9.1 Länder-Abstraktion

```rust
// Länderneutrale Schnittstelle. Jedes System ist eine Zustandsmaschine mit definierten Ein-/Ausgängen.
pub trait TrainProtectionSystem {
    fn update(&mut self, dt: f64, train: &TrainState, cab: &CabInputs,
              events: &[TracksideEvent]) -> ProtectionOutput;
    fn indicators(&self) -> &[Indicator];      // Leuchtmelder/Anzeigen für den Führerstand
    fn isolate(&mut self, isolated: bool);      // Störschalter
}

pub struct TracksideEvent { pub device: DeviceKind, pub payload: ron::Value, pub s_offset: f64 }
pub enum ProtectionAction { None, ForcedServiceBrake, EmergencyBrake, TractionCutOff }
pub struct ProtectionOutput { pub action: ProtectionAction, pub speed_limit: Option<f64>, /* Anzeigen … */ }
```

- Ein Fahrzeug trägt eine Liste `Vec<Box<dyn TrainProtectionSystem>>` (aus Fahrzeugdatenbank).
- Trackside-Geräte (Kap. 5) erzeugen `TracksideEvent`s, wenn ein antennentragendes Fahrzeug sie überfährt.
- **Länderpaket** = Bündel aus (Zugsicherungs-Implementierungen + Signalsystem-Definition + Trackside-Gerätetypen
  + Regelwerk-Parametern), als Rust-Modul/Plugin registriert. DE ist das erste; die polnischen (SHP/CA)
  oder österreichischen Systeme wären spätere Pakete gegen dieselbe API.

### 9.2 Sifa (zuerst implementieren — einfachste Zustandsmaschine)

- Zeit-Zeit-Sifa (Standard DE): Pedal/Taster; nach 30 s ohne Bedienung → Leuchtmelder, +2,5 s → Hupe,
  +2,5 s → Zwangsbremsung (Schnellbremsung), Lösen erst nach Pedalwechsel. Parameter je Fahrzeug.
- Abschaltbar (Störschalter, protokolliert).

### 9.3 PZB 90 (vollständig, das Herzstück für DE-Betrieb)

- **Streckenseite:** 500-Hz-, 1000-Hz-, 2000-Hz-Gleismagnete als Trackside-Geräte; Wirksamkeit
  signalabhängig (1000 Hz aktiv bei Vr0/Vr2, 2000 Hz bei Hp0 — Kopplung ans Signalsystem, Kap. 10).
- **Fahrzeugseite, komplette PZB-90-Logik:**
  - Zugarten O/M/U mit allen Prüfgeschwindigkeiten und Bremskurven (165/125/105 → 85/70/55 usw.).
  - 1000-Hz-Beeinflussung: Wachsam innerhalb 4 s, Überwachung 1000 Hz für 1250 m, Bremskurve,
    Befreiung nach 700 m (Frei-Taste), LM 1000 Hz.
  - 500-Hz-Beeinflussung: sofortige Überwachung (65/50/40 abfallend auf 45/35/25), 250 m, keine Befreiung.
  - Restriktive Überwachung (45/25 km/h) nach Halt oder v < 10 km/h für > 15 s innerhalb Überwachung.
  - 2000 Hz → Zwangsbremsung; Befehl-40-Taste (Vorbeifahrt am Halt zeigenden Signal mit Befehl, ≤ 40 km/h,
    LM Befehl).
  - Zwangsbremslogik: Schnellbremsung bis Stillstand, Freigabe per Wachsam+Bedingungen.
  - Alle Leuchtmelder (85/70/55, 1000 Hz, 500 Hz, Befehl) + Wachsam/Frei/Befehl-Taster als CabInputs.
- **Abnahme:** Testszenarien pro Regelfall (Tabelle der PZB-90-Überwachungsfälle), headless als Unit-Tests.

### 9.4 LZB 80/CE

- **Streckenseite:** Linienleiter-Abschnitte (Bereichskennung, Blockeinteilung) als Trackside-Abschnittsobjekt;
  LZB-Zentrale als Teil der Stellwerkssimulation vergibt Fahrterlaubnis (Zielentfernung, Zielgeschwindigkeit)
  aus Fahrstraßen-/Blockzustand.
- **Fahrzeugseite:** Aufnahmeprüfung/Übernahme (Ü-Taster), Führung per Soll-/Ziel-Geschwindigkeit und
  Zielentfernung (MFA-Anzeigen: v-Soll, v-Ziel, Zielentfernung, Ü/G/EL/ENDE/V40/B-Leuchtmelder),
  Bremskurvenüberwachung mit Zwangsbremsung, Ende-Verfahren (LZB-Ende → Übergabe an PZB), Ausfall-Verfahren.
  CIR-ELKE-Modus als Parameter (kürzere Blöcke, höhere Grenzen — reine Dateneinstellung).
- Unter LZB-Führung: PZB-Magnete unterdrückt (korrektes Zusammenspiel LZB↔PZB).
- **AFB** (separates Fahrzeug-Feature, kein Zugsicherungssystem): v-Soll-Regler, nutzt LZB-Sollwerte.

### 9.5 Weitere DE-Systeme (nach LZB, Reihenfolge nach Bedarf)

- **ETCS** L1 Limited Supervision/L2 (Balisen sind als Trackside-Kind schon vorgesehen; DMI-Anzeige;
  Umfang v2 — Architektur muss es nur nicht verbauen).
- **ZBS** (Berliner S-Bahn), **GNT/Neigetechnik**, **Türsteuerung TAV/TB0**: v2+, gleiche Trait-API.

### 9.6 Zugfunk

- GSM-R-artig: Kanalwahl, Registrierung per Zugnummer, Notruf (löst bei KI-Zügen im Umkreis Schnellbremsung
  aus), Ansagen vom „Fdl" (Szenario-Skript). v1: UI + Notruf, keine Sprachsimulation.

---

## 10. Signalsystem & Stellwerk (`sim-core::interlock`)

### 10.1 Signalsystem-Abstraktion

- Ein Signal = Zustandsautomat mit Begriffen (`SignalAspect`), Definition datengetrieben pro Länderpaket:
  Begriffe, Lampenbilder (für Rendering), Verknüpfungsregeln (Vorsignal zeigt x, wenn Hauptsignal y),
  zugehörige Zugsicherungs-Wirkungen (welcher Magnet wann aktiv).
- **DE-Paket v1:** Ks-System (Ks1/Ks2, Zs3/Zs3v-Anzeiger, Mastschilder, Zs1, Kennlicht) **und**
  H/V-System (Hp0/1/2, Vr0/1/2, Sh1) — beides ist verbreitet. Hl (Ostnetz) v2. Lf-/Ne-/Zs-Tafeln als
  passive Trackside-Objekte.
- **Beispiel Datendefinition:** `signals_de.ron` beschreibt Begriffe & Regeln; Renderer mappt Begriff →
  Lampen an Signal-Mesh.

### 10.2 Stellwerkslogik

- **Fahrstraßen:** definiert als Weg von Start- zu Zielsignal über Weichenlagen; Verschluss (Weichen
  festgelegt), Flankenschutz (v1: nur Weichenverschluss, kein vollständiger Flankenschutzgraph —
  `// ponytail`), Durchrutschweg, Auflösung durch Zugfahrt (Achszähler-Abschnitte = Gleisfreimeldung).
- **Blocksicherung** auf der Strecke: Selbstblock (Signal hinter Zug auf Halt, frei wenn Block geräumt).
- **Betrieb:** v1 vollautomatisch aus Fahrplan (Zuglenkung: Fahrstraße wird angefordert, wenn Zug laut
  Fahrplan naht). Manuelles Stellwerk-UI (Spieler als Fdl) = v2, Architektur (Anfrage → Verschluss →
  Signal) trägt das bereits.
- LZB-Zentrale (9.4) liest denselben Block-/Fahrstraßenzustand.

---

## 11. KI & Fahrplan (`ai-driver`)

- **Fahrplan-Datenmodell:** Züge mit Zugnummer, Gattung, Fahrzeugkonfiguration, Ankunft/Abfahrt je
  Betriebsstelle, Gleisangabe. RON/CSV-Import.
- **KI-Triebfahrzeugführer** (fährt dieselbe Fahrzeugsimulation wie der Spieler — keine Cheat-Physik):
  Geschwindigkeitsregler mit vorausschauender Bremskurvenplanung auf Basis Streckenprofil + Signalbegriffe
  (die KI „sieht" Signale/La-Stellen über den Gleisgraph voraus), Halt am Bahnsteig (Haltetafel-Position),
  Abfahrt nach Fahrplan + Abfahrsignal, Bedienung von Sifa/PZB (Wachsam quittieren), Störfall = einfach
  liegenbleiben + Funkmeldung (v1).
- **Zugbildung:** Rangieren v2; v1 Züge spawnen/despawnen an Schattenbahnhöfen (Portale am Streckenrand).
- **Szenario-/Eventsystem:** Trigger (Zeit, Zugposition, Zustand) → Aktionen (Signal/Weiche, Ansage,
  Wetterwechsel, Punktewertung, Nachricht). RON-basiert, entspricht MaSzynas Eventsystem.
- **Bewertung:** Fahrplantreue, Halteplatz-Genauigkeit, verbotene Zwangsbremsungen, Energieverbrauch → Log + Score.

---

## 12. Führerstand, Input, Kameras (`app`)

- **3D-Führerstand:** Fahrzeugmodell mit benannten Interaktions-Nodes (`lever_fbv`, `btn_pzb_wachsam`, …).
  Mapping-Datei pro Fahrzeug verbindet Node ↔ Sim-Input (Achse/Taster/Schalter mit Rastungen).
  Maus-Interaktion per Raycast (klicken/ziehen), daneben komplette Tastaturbelegung und Gamepad/RailDriver
  (v2: RailDriver-HID).
- **Instrumente:** Zeiger als rotierende Sub-Meshes (Manometer HL/HB/C, Tacho), MFA/EBuLa/Displays als
  Render-to-Texture (egui-in-world oder eigene Shader). Leuchtmelder = Emissive-Toggle.
- **Zugverband begehbar:** v2. v1: Führerstand + Außenkameras.
- **Kameras:** Führerstand (Kopf frei schwenkbar), Außen-Orbit, frei fliegend, Gleis-/Vorbeifahrt-Kamera.
- **Zugriff auf alles per Tastatur** (MaSzyna-Prinzip): vollständige Bedienbarkeit ohne Maus.

## 13. Audio

- Positionaler 3D-Sound: Fahrmotor/Dieselmotor (RPM-abhängige Loops mit Crossfade), Schaltwerk,
  Kompressor, Bremsenzischen (an Ventil-Events der Pneumatik gekoppelt!), Klotzbremsen-Kreischen,
  Schienenstöße geschwindigkeitsabhängig, Kurvenquietschen, Weichenpoltern (an Gleisgeometrie-Events),
  Signalhorn/Makrofon, Sifa-Hupe, PZB-Zwangsbremsung, Ansagen.
- Sound-Definitionen in Fahrzeugdatei (Event → Sample + Kurven). Innen-/Außenfilter (Lowpass im Cab).

## 14. Umgebung & Rendering

- Terrain aus DGM (Importer, Kap. 15), Textur-Splatting; Vegetation/Gebäude als gestreamte Instanzen.
- Oberleitung prozedural aus Streckendaten (Masten, Kettenwerk als Mesh/Shader-Kurven).
- Tag/Nacht (Sonnenstand nach Datum/Ort — Position ist ja georeferenziert), Wetter (Regen/Schnee/Nebel,
  beeinflusst Adhäsion Kap. 6 und Sicht), Jahreszeiten v2.
- Nachtbeleuchtung: Signale, Bahnsteige, Führerstand-Instrumentenbeleuchtung, Fernlicht/Spitzensignal.

## 15. Content-Pipeline & Editor

- **Fahrzeugdefinition:** RON-Datei (Masse, Davis, Bremsausrüstung, Traktionstyp+Kennfelder,
  Zugsicherungsausrüstung, Sounds, Cab-Mapping) + glTF-Modelle. Entspricht MaSzynas .fiz/.mmd, aber lesbar.
- **Streckenquellformat:** editierbares RON (Gleisgraph, Geometrie, Trackside-Geräte, Fahrstraßen,
  Szenerie-Platzierungen) → Compiler baut binäre Tiles (Kap. 4.3).
- **Importer:** OSM-Gleisdaten (Grobtrassierung) + DGM (Geländehöhe) als Startpunkt einer Strecke;
  CRS-Reprojektion hier (Kap. 4.2). Kein MaSzyna-.scn-Importer (Aufwand > Nutzen, anderes Land).
- **Editor** (`editor`-Crate, eigene Bevy-App): Gleisverlegung (Klothoiden-Werkzeug), Trackside-Geräte
  setzen (mit Regelprüfung: „1000-Hz-Magnet fehlt am Vorsignal"), Fahrstraßen-Definition, Szenerie,
  Fahrplan-Editor. Editor kommt früh (M3), sonst gibt es keinen Content.

## 16. Querschnittsthemen

1. **Determinismus:** sim-core mit fixem Timestep + eigener RNG mit Seed → Replays, Regressionstests, MP-Option.
2. **Save/Load:** kompletter sim-core-Zustand serialisierbar (serde) — von Anfang an, nachrüsten ist Hölle.
3. **Debug-Tools:** egui-Overlays: Bremsdruck-Diagramm längs des Zuges, Kraftschluss, Zugsicherungs-Zustandsmaschine,
   Gleisgraph-Ansicht, Signal-/Fahrstraßenzustand, Zeitraffer/Pause/Einzelschritt.
4. **Lokalisierung:** UI-Strings extern (fluent), DE zuerst, EN mitführen.
5. **Modding:** alle Inhalte aus `assets/` + RON — Fahrzeuge/Strecken/Länderpakete ohne Recompile
   (Länderpaket-Logik in Rust bleibt Compile-Zeit; Daten sind frei).

---

## 17. Meilensteine (jeweils mit Abnahmekriterium)

| M | Inhalt | Abnahme |
|---|---|---|
| **M0** | Workspace, `world-coords` (ECEF f64 + Floating Origin), Testszene 300 km, Kamera | Kein Jitter/Sprung bei 300 km Distanzflug |
| **M1** | `track-model` (Graph, Klothoiden, eval), prozedurales Gleisrendering, Fahrzeug fährt per Konstantgeschwindigkeit auf Testoval, Streaming-Rohbau | Fahrt über Weiche, Tile-Streaming ohne Ruckler |
| **M2** | Längsdynamik + Bremse (Kap. 6+7), eine E-Lok (Umrichter-Typ) + 5 Wagen, Basissounds, Basis-Cab (Tastatur) | Headless-Bremswegtests grün; anfahren/bremsen fühlbar korrekt |
| **M3** | Sifa + PZB 90 komplett, H/V+Ks-Signale statisch, **Editor v1** (Gleise+Geräte) | Alle PZB-Testfälle als Unit-Tests grün; Testfahrt mit 1000/500/2000-Hz-Fällen |
| **M4** | Stellwerk (Fahrstraßen, Selbstblock), KI-Züge + Fahrplan, Signal-Dynamik | Spieler + 3 KI-Züge auf 20-km-Strecke, konfliktfrei nach Fahrplan |
| **M5** | LZB 80 + AFB, MFA-Anzeigen, Schaltwerk-Lok (BR 110) als 2. Traktionstyp | LZB-Führung inkl. Ende-/Ausfallverfahren; Übergang LZB→PZB |
| **M6** | 3D-Cab voll interaktiv (Maus), Aufrüst-Prozedur, Audio-Vollausbau, Wetter+Nacht | „Kalte Lok" bis Streckenfahrt komplett per Maus im Cab |
| **M7** | Pilotstrecke (~30 km real, aus OSM/DGM importiert, über nichts weniger als eine UTM-Zonengrenze wenn machbar), Szenariosystem, Bewertung, Save/Load | Durchspielbares 45-min-Szenario mit Wertung |
| v2+ | ETCS, ZBS, GNT, Diesel-Detailmodelle, Rangieren, Fdl-Modus, Multiplayer, RailDriver | — |

Reihenfolge-Begründung: Koordinaten zuerst (Fundament, nicht nachrüstbar), Physik vor Zugsicherung
(Zwangsbremsung braucht echte Bremse), Editor vor Content, LZB nach Stellwerk (braucht Blockdaten).

## 18. Teststrategie

- `sim-core` headless: Unit-Tests je Zustandsmaschine (PZB-Fälle tabellengetrieben!), Physik gegen
  Literaturwerte, Determinismus-Test (2 Läufe, gleicher Seed, identischer Zustands-Hash).
- Szenario-Regressionstests: aufgezeichnete Input-Replays, Soll-Endzustand.
- CI: `cargo test` + clippy + fmt; Rendering nur Smoke-Test (App startet, 100 Frames).

## 19. Risiken

| Risiko | Gegenmaßnahme |
|---|---|
| Pneumatik-Modell wird numerisch instabil | Substepping, implizite Knotenlösung, früh Referenztests |
| Bevy-Breaking-Changes | sim-core Bevy-frei; App-Schicht dünn halten; Bevy-Version pro Meilenstein pinnen |
| Content-Flaschenhals (Strecken bauen ist teuer) | Editor früh (M3), OSM/DGM-Import, prozedurale Oberleitung/Szenerie |
| Zugsicherungs-Regelwerk falsch umgesetzt | Testfälle aus Richtlinie 483 (PZB/LZB-Bedienvorschriften) ableiten, tabellengetrieben |
| f64-ECEF-Fehler schleichen sich in f32-Pfade | `EcefPos` als Newtype, Clippy-Lint/Review: kein `as f32` außerhalb des Origin-Sync |
