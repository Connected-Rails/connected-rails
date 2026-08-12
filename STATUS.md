# Umsetzungsstand gegen PLAN.md

Stand: 12.08.2026 · `cargo test --workspace`: **141 Tests grün** · clippy und fmt sauber.

## Meilensteine

| M | Inhalt | Stand |
|---|---|---|
| **M0** | Workspace, `world-coords` (ECEF f64 + Floating Origin) | **fertig** — Abnahmetest „300 km ohne Jitter/Sprung" grün |
| **M1** | `track-model`, prozedurales Gleisrendering, Streaming | **teilweise** — Graph, Klothoiden, `eval`, Weichen (inkl. Auffahren), Gleismeshes fertig; **Tile-Streaming (Plan 4.3) fehlt** |
| **M2** | Längsdynamik + Bremse, E-Lok + Wagen, Basis-Cab | **fertig bis auf Audio** — Auslauf gegen Davis, Schnellbremsweg, Anfahren am Berg, Kupplungsspiel als Tests; **Sound (Kap. 13) fehlt** |
| **M3** | Sifa + PZB 90, Signale, Editor v1 | **teilweise** — Sifa und PZB 90 vollständig mit Regelfall-Tests; Signallogik H/V + Ks vorhanden, aber ohne Lampenbild-Rendering; **Editor** existiert als Draufsicht mit Luftbild-Overlay, kann aber noch nichts bearbeiten |
| **M4** | Stellwerk, KI-Züge, Fahrplan | **fertig** — Fahrstraßen mit Verschluss/Auflösung, Selbstblock, KI hält an Signal und Bahnsteig |
| **M5** | LZB 80 + AFB, MFA, Schaltwerk-Lok | **teilweise** — LZB mit Führung, Bremskurve, Ende- und Ausfallverfahren; BR 110 vorhanden; **AFB fehlt**, MFA nur als HUD-Text |
| **M6** | 3D-Cab interaktiv, Aufrüsten, Audio, Wetter/Nacht | **teilweise** — Aufrüstkette simuliert und per Tastatur bedienbar, Wetterwechsel über Szenario-Aktionen, Gelände aus dem DGM; kein 3D-Führerstand, kein Audio, keine Vegetation/Texturierung |
| **M7** | Pilotstrecke aus OSM/DGM, Szenarien, Bewertung, Save/Load | **weitgehend fertig** — Szenariosystem, Bewertung, Save/Load und der OSM-/DGM-Importer stehen; es fehlt nur noch eine echte Pilotstrecke (Datenbeschaffung) |

## Was inhaltlich steht

- **Koordinaten (Kap. 4):** ECEF f64, ENU-Frames, Floating Origin mit Rebase alle 4 km inkl.
  neuer ENU-Rotation, Erdkrümmungskorrektur der Tangentialebene, UTM↔geodätisch für den Import.
- **Gleismodell (Kap. 5):** Ein Segmenttyp (`k0`, `dk`) für Gerade, Bogen und Klothoide;
  Weichen mit Umlaufzeit, Verschluss und Auffahrerkennung; Trackside-Geräte mit RON-Payload.
- **Fahrdynamik (Kap. 6):** Davis, Bogen- und Steigungswiderstand, Kupplungen mit Spiel und
  Bruchkraft, Curtius/Kniffler-Kraftschluss mit Schleudern/Gleiten, Sanden, Gleitschutz.
- **Bremse (Kap. 7):** HL als Knotenkette, KE-Steuerventil (Dreidrucksystem, Erschöpfbarkeit),
  Bremsstellungen G/P/R(+Mg), Zusatzbremse, Blending mit der E-Bremse, Bremshundertstel.
- **Elektrik (Kap. 8):** Aufrüstkette, Stromabnehmer-Laufzeit, Hauptschalterabfall in
  Schutzstrecken, drei Traktionstypen (Schaltwerk, Umrichter, Diesel).
- **Zugsicherung (Kap. 9):** Trait-Abstraktion + Länderpaket DE mit Sifa, vollständiger
  PZB 90 (O/M/U, 1000/500/2000 Hz, Befreiung, restriktive Überwachung, Befehl 40) und
  LZB 80 (Übernahme, v-Soll/v-Ziel/Zielentfernung, Ende- und Ausfallverfahren).
- **Stellwerk (Kap. 10):** Signalbegriffe, Vorsignalisierung, Selbstblock, Fahrstraßen,
  signalabhängige Magnetwirksamkeit.
- **KI (Kap. 11):** Vorausschau über den Gleisgraph, Bremskurve mit Reaktions- und
  Ansprechweg, Fahrplanhalt, bedient Sifa und PZB selbst.
- **Szenarien (Kap. 11.4):** RON-Ereignisse mit Auslösern (Zeit, Zugposition, Halt,
  Geschwindigkeit, Signalbegriff, Zwangsbremsung, Verkettung mit Verzögerung, `All`/`Any`)
  und Aktionen (Meldung, Ansage, Weiche, Fahrstraße, Wetter, Punkte, Szenarioende).
- **Bewertung (Kap. 11):** Fahrplantreue, Halteplatz-Genauigkeit, Zwangsbremsungen,
  Geschwindigkeitsüberschreitungen und Fahrenergie → aufgeschlüsselte Punktzahl.
- **Trassierung (Kap. 15):** aus der Punktfolge werden Entwurfselemente rekonstruiert —
  Abschnittstrennung, Radiusausgleich über den ganzen Bogen (Kåsa), Rundung auf Regelradien
  mit Toleranz, Übergangsbögen und Überhöhung nach Regelwerk (`ü = 11,8·v²/R` abzüglich
  Fehlbetrag, gedeckelt, Rampe 1:10·v). Richtung wird über Ausgleichsgeraden im gleitenden
  Fenster geschätzt, nicht aus Nachbardifferenzen — sonst überdeckt das Punktrauschen die
  Krümmung vollständig. Abnahme: eine regelkonform entworfene Trasse wird mit exaktem
  Radius, korrekter Überhöhung und < 6 m Lageabweichung zurückgewonnen, auch mit ±2 m
  Rauschen auf den Stützpunkten.
- **Import (Kap. 15):** Overpass-JSON → Wegkette → Trassierung → `LineSource`. Aus OSM
  kommen Geometrie, `maxspeed` und `name`; DGM-Kacheln (XYZ oder ESRI ASCII Grid) aus
  einer Datei oder einem ganzen Verzeichnis liefern das Neigungsprofil. Kacheln werden
  verzögert geladen (Blattschnitt aus dem Dateinamen) und in einem LRU gehalten, damit
  auch das DGM1 eines Bundeslandes benutzbar ist. CLI: `import-line`.
- **Gelände (Kap. 14):** Kacheln à 512 m nur im Streckenkorridor, Rasterweite nach
  Gleisabstand (4 m bis 32 m statt 1 m), Schürzen gegen LOD-Risse, Einschnitt/Damm am
  Gleis, Sichtweitenbegrenzung je LOD-Stufe in der App.
- **Luftbild-Overlay (Kap. 15):** eigenes Crate `imagery` mit Web-Mercator-Kachelrechnung,
  Anbieterkonfiguration (Kachelvorlage oder WMS, Platzhalter, Schlüssel, Zoomgrenzen,
  Attribution) und zweistufigem Cache (Arbeitsspeicher + Platte, Budget mit Verdrängung
  der ältesten Kacheln, Höchstalter, Offlinebetrieb). Abruf und Entschlüsselung laufen in
  Arbeitsthreads, der Editor legt die Kacheln georeferenziert unter das Gleisband.
  Alles über eine RON-Datei steuerbar, im Betrieb neu ladbar.
- **Querschnitt (Kap. 16):** fester Zeitschritt, Seed-RNG, Zustands-Hash mit
  Determinismustest, vollständige Serialisierung für Save/Load.

## Bewusst zurückgestellt

Jede Vereinfachung steht als `ponytail:`-Kommentar an der Codestelle, mit Upgrade-Pfad:

- **Rohrmodell der HL** ist ein Knoten-/Diffusionsmodell, keine Druckwelle.
- **Schlupf pro Fahrzeug**, nicht pro Radsatz.
- **Flankenschutz** ist nur Weichenverschluss.
- **LZB-Bremskurve** mit fester Verzögerung statt zugspezifischer Bremsbewertung.
- **Geoid-Undulation** als konstanter Offset pro Strecke.
- **Geräte-Payload** als RON-*Text* statt `ron::Value` (Value verliert Unit-Enum-Varianten).
- **Kein CRS-Framework:** UTM 32/33 direkt als Snyder-Reihe in `world-coords::geo` statt
  `proj4rs`/`geodesy` — für DE sind das genau zwei Projektionen. Kommen Gauß-Krüger oder
  Nachbarländer dazu, tritt `proj4rs` hinter dieselbe Signatur.
- **Importer verkettet einen Strang**, kein Routing über Weichen; Bahnhofsköpfe brauchen
  später eine echte Graphsuche.
- **DGM-Blattschnitt aus dem Dateinamen** (Konvention der Länder: `…_389_5711_…`); passt
  der Name nicht, wird die Kachel einmal gelesen, um ihre Ausdehnung zu bestimmen.
  Gepackte Lieferungen (`.gz`, `.zip`) müssen vorher entpackt werden.
- **Geländekacheln entstehen beim Start**, nicht zur Laufzeit — für 100-km-Strecken ist
  asynchrones Nachladen nachzurüsten.
- **Übergangsbogenlänge und Überhöhung stammen aus dem Regelwerk**, nicht aus den Daten:
  beide sind aus einer verrauschten Punktfolge nicht rückgewinnbar (die Abschnittsgrenze ist
  um über hundert Meter unsicher). Wo die Quelle davon abweicht, weicht die Rekonstruktion
  um einige Meter ab — dieselbe Größenordnung wie die Lagegenauigkeit von OSM selbst.
- **Kein Routing über Weichen** beim Verketten, und Bahnhofsbereiche mit mehreren Gleisen
  werden nicht getrennt.

## Nächste sinnvolle Schritte

1. **Editor mit Werkzeugen**: bisher zeigt er nur an. Als Nächstes Trassierung ziehen,
   Weichen und Signale setzen, Bahnsteige platzieren — das Luftbild dahinter ist die
   Vorlage dafür.
2. **Echte Pilotstrecke importieren** (Overpass-Abzug + DGM1 eines Landesvermessungsamts).
3. **Weichenkatalog**: Regelweichen (EW 190-1:9 … EW 1200-1:18,5) als Datentabelle mit
   Radius, Zweiglänge und abzweigender Geschwindigkeit; OSM liefert nur einen Knoten
   `railway=switch` ohne jede Geometrie.
4. **OSM-Ausrüstung mitnehmen**: Signale, Bahnsteige, Haltepunkte und Bahnübergänge —
   dann entsteht aus dem Import direkt eine ausgerüstete Strecke statt eines nackten
   Strangs.
5. **Bessere Quellen prüfen** als OSM: das RINF-Infrastrukturregister der EU
   (Geschwindigkeiten, Neigungen, Zugsicherung, teils Mindestradien) und die offenen
   Geodaten der DB.
6. **Terrain-Streaming (Kap. 4.3)** — Kacheln werden bisher beim Start gebaut; bei 100 km
   müssen sie zur Laufzeit nachgeladen und verworfen werden. Die Kachelstruktur trägt das
   bereits, es fehlt die asynchrone Erzeugung.
7. **Texturierung/Vegetation** — das Gelände ist einfarbig; Splatting und Instanzen fehlen.
8. **Audio (Kap. 13)** — die Ereignisse liefert `sim-core` bereits.
9. **3D-Führerstand (M6)** — der dickste Brocken.
