# TrainSim-DE

Deutscher Zugsimulator auf Bevy — Umsetzung von [PLAN.md](PLAN.md).
Aktueller Stand und offene Punkte: [STATUS.md](STATUS.md).

## Bauen und starten

```bash
cargo test --workspace     # alle Abnahmetests (headless, ohne GPU)
cargo run -p app           # Simulator starten
cargo run -p app -- --frames 120   # Rendering-Smoke-Test (CI)
```

## Strecke importieren

Gleisdaten bei [Overpass Turbo](https://overpass-turbo.eu) als JSON exportieren:

```overpassql
[out:json];
way["railway"="rail"](50.90,10.00,51.00,10.30);
(._;>;);
out body;
```

**Aus OSM übernommen werden** die Geometrie der `railway=rail`-Wege, `maxspeed` und
`name`. Weichen, Signale, Bahnsteige und Bahnübergänge kommen (noch) nicht mit — die
Strecke entsteht als ein Strang und wird anschließend in der RON-Datei ausgerüstet.

Aus der Punktfolge wird keine geglättete Kurve, sondern eine **Trassierung**: gerade
Abschnitte und Bögen werden getrennt, der Radius über den ganzen Bogen ausgeglichen (das
Punktrauschen mittelt sich mit √n heraus) und auf den nächsten Regelradius gerundet, wenn
er nah genug liegt. Übergangsbögen und **Überhöhung** lassen sich aus OSM nicht messen und
kommen deshalb aus dem Regelwerk: `ü = 11,8 · v²/R` abzüglich des zugelassenen
Fehlbetrags, gedeckelt bei 160 mm, Rampenlänge 1:10·v. Ergebnis ist eine Kette aus
Gerade – Klothoide – Kreisbogen – Klothoide – Gerade.

Grenzen, die man kennen sollte: OSM liegt aus Luftbildern auf ±2…5 m genau, und Anfang
und Ende eines Bogens sind aus einer Punktfolge nur auf etwa zehn Meter bestimmbar. Radius,
Drehwinkel und Überhöhung werden dagegen genau getroffen — also genau die Größen, die man
beim Fahren spürt. Der Importbericht nennt Radien, Überhöhung und die Abweichung zur
OSM-Linie.

**Höhen** kommen aus dem DGM der Länder. `--dgm` nimmt eine Datei *oder ein ganzes
Verzeichnis* mit Kachelblättern (auch mit Unterordnern):

```bash
cargo run -p content --bin import-line -- strecke.json --dgm ./dgm1_niedersachsen --epsg 25832 --name "Musterbahn" --out strecke.ron
```

Unterstützt werden XYZ (`x y z`, UTM) und ESRI ASCII Grid (`.asc`). Die Blattgrenzen
werden aus dem Dateinamen gelesen (`dgm1_32_389_5711_1_ni.xyz`), sodass beim Start nichts
geladen wird; jede Kachel kommt erst in den Speicher, wenn eine Abfrage hineinfällt, und
höchstens acht bleiben gleichzeitig geladen. Damit ist auch ein DGM1 eines ganzen
Bundeslandes (mehrere tausend Kacheln) verwendbar.

Das Werkzeug meldet Länge, Kantenzahl, Höhenabdeckung und die größte Abweichung der
Trassierung von den OSM-Punkten.

## Workspace

| Crate | Inhalt |
|---|---|
| `world-coords` | ECEF-f64-Weltkoordinaten, Floating Origin, Geodäsie (Plan Kap. 4) |
| `track-model` | Gleisgeometrie (Gerade/Bogen/Klothoide), Topologie, Weichen, Streckenausrüstung (Kap. 5) |
| `sim-core` | Fahrdynamik, Druckluftbremse, Elektrik, Zugsicherung, Stellwerk, Fahrplan, Szenario und Bewertung — **ohne Bevy**, deterministisch (Kap. 6–11) |
| `content` | Fahrzeugdatenbank, Streckenquellformat (RON) + Compiler, Szenarien, OSM-/DGM-Importer (Kap. 15) |
| `ai-driver` | KI-Triebfahrzeugführer, Vorausschau (Kap. 11) |
| `imagery` | Luftbild-Kacheln: Anbieter, Web-Mercator-Rechnung, Cache, Abruf (Kap. 15) |
| `app` | Bevy-App: Rendering, Kameras, Eingabe, HUD (Kap. 12) |
| `editor` | Streckeneditor: Draufsicht mit Luftbild-Overlay (Kap. 15) |

`sim-core` ist eine reine Rust-Bibliothek mit festem Zeitschritt (200 Hz). Die Bevy-App
tickt sie und spiegelt den Zustand in ECS-Komponenten — Simulationslogik gehört dort nicht hinein.

## Tastenbelegung

| Taste | Funktion |
|---|---|
| `W` / `S` | Fahrschalter auf/ab (negativ = elektrische Bremse), `X` = Null |
| `R` / `F` / `T` | Richtungswender vorwärts / rückwärts / neutral |
| `A` / `D` | Führerbremsventil lösen / bremsen |
| `Q` / `E` / `Z` | Abschluss / Schnellbremsung / Füllen |
| `C` / `V` | Zusatzbremse anlegen / lösen |
| `G` | Sanden |
| `Leertaste` | Sifa |
| `Bild ↓` / `Ende` / `Entf` | PZB Wachsam / Frei / Befehl |
| `N` / `M` | LZB Übernahme / Ende |
| `H` | Signalhorn |
| `1`–`4` | Batterie / Stromabnehmer / Hauptschalter / Luftpresser |
| `F1`–`F3` | Kamera: Führerstand / Außen / Streckenkamera |
| Pfeiltasten | Blickrichtung, `Num +/-` Kameraabstand |

## Beispielstrecke

`content::musterbahn()` — 7 km: 3 km Gerade (160 km/h), 1 km Bogen R = 1200 m mit
Überhöhungsrampe (130 km/h), 3 km mit 8-‰-Steigung. Blocksignal bei km 2,0 mit Vorsignal,
1000/500/2000-Hz-Magneten und LZB-Linienleiter im letzten Abschnitt.

## Gelände

Aus demselben DGM baut `content::terrain` die Geländemeshes — nur im Korridor um die
Strecke und mit abgestufter Auflösung:

| Abstand zum Gleis | Rasterweite | Dreiecke je km² |
|---|---|---|
| bis 96 m | 4 m | 125 000 |
| bis 384 m | 8 m | 31 000 |
| bis 768 m | 16 m | 8 000 |
| darüber | 32 m | 2 000 |

Zum Vergleich: DGM1 unverändert wären 2 000 000 Dreiecke je km². Dazu kommen 512-m-Kacheln
(eigene Entität je Kachel → Frustum-Culling, zusätzlich Sichtweitenbegrenzung je LOD-Stufe),
Schürzen an den Kachelrändern gegen Risse zwischen den Stufen und ein Einschnitt/Damm-Profil,
das das Gelände nahe am Gleis auf die Schienenhöhe zieht.

Die App zeigt das Gelände automatisch (ohne DGM eben):

```bash
cargo run -p app -- --dgm ./dgm1_niedersachsen --epsg 25832
```

## Editor mit Luftbild-Overlay

```bash
cargo run -p editor                              # Beispielstrecke
cargo run -p editor -- strecke.ron --imagery meine_karten.ron
```

Die Overlay-Konfiguration (`imagery.ron`) wird beim ersten Start angelegt und ist
vollständig editierbar: Anbieter, Deckkraft, Zoomstufe oder Wunschauflösung, Laderadius,
Kachelobergrenze, Bildversatz gegen die Gleislage, Höhe des Overlays, Cache (Ort, Budget,
Speicherkacheln, Offlinebetrieb, Höchstalter) und Abrufverhalten (User-Agent, Timeout,
Parallelität, Wiederholungen). Änderungen lassen sich im Betrieb mit F5 neu laden und mit
F2 zurückschreiben.

**Anbieter** sind Daten, keine fest verdrahtete Liste. Mitgeliefert sind Esri World
Imagery, BKG TopPlusOpen, OpenStreetMap und eine WMS-Vorlage für die Orthophotos der
Landesvermessungsämter. Eigene Dienste kommen als Eintrag dazu — entweder als
Kachelvorlage mit den Platzhaltern `{z}` `{x}` `{y}` `{-y}` `{s}` `{key}` oder als WMS,
dessen `BBOX` aus der Kachel in EPSG:3857 gebildet wird:

```ron
(
    id: "dop_nrw",
    name: "DOP Nordrhein-Westfalen",
    url: Wms(
        endpoint: "https://www.wms.nrw.de/geobasis/wms_nw_dop",
        layers: "nw_dop_rgb",
        version: "1.3.0",
        styles: "",
        extra: [("TRANSPARENT", "FALSE")],
    ),
    max_zoom: 20,
    tile_size: 512,
    format: Jpeg,
    attribution: "Geobasis NRW",
)
```

Verfügbarkeit und Nutzungsbedingungen jedes Dienstes sind vor dem Einsatz zu prüfen; für
Massenabrufe gehören eigene Zugänge in die Konfiguration.

**Cache:** Kacheln landen unter `<cache>/<anbieter>/<z>/<x>/<y>.<ext>`, davor liegt ein
Arbeitsspeicher-Cache. Einmal geladen, ist die Strecke offline bearbeitbar (`L` schaltet
den Offlinebetrieb um). Der Plattenplatz ist gedeckelt; ist das Budget voll, fliegen die
ältesten Kacheln zuerst. Das HUD zeigt Treffer, Ladevorgänge, Verworfenes und Belegung.

| Taste | Funktion |
|---|---|
| `WASD` / Pfeile | Blickpunkt verschieben, `Bild ↑/↓` Höhe |
| `O` | Overlay ein/aus |
| `P` | Anbieter wechseln |
| `[` `]` | Deckkraft |
| `,` `.` | Zoomstufe, `Z` zurück auf Wunschauflösung |
| Ziffernblock `4/6/8/2` | Bildversatz (mit Umschalt in 5-m-Schritten), `5` zurücksetzen |
| `L` | Offlinebetrieb | 
| `C` / `R` | Cache leeren / Fehlversuche zurücksetzen |
| `F5` / `F2` | Konfiguration laden / speichern |

## Szenarien

Ein Szenario ist eine RON-Datei aus Ereignissen — Auslöser plus Aktionen:

```ron
(
    name: "Regionalbahn nach Musterstadt",
    player_train: 0,
    events: [
        (name: "abfahrt", trigger: Time(5.0),
         actions: [Announcement("RE 4711, Abfahrt frei.")]),
        (name: "regen", trigger: After(event: "abfahrt", delay: 60.0),
         actions: [SetRail(Wet), Message("Regen setzt ein.")]),
        (name: "ziel", trigger: TrainStopped(train: 0, edge: (2), s: 2600.0, radius: 50.0),
         actions: [Finish(success: true, reason: "Musterstadt erreicht")]),
    ],
)
```

Bewertet werden Fahrplantreue, Halteplatz-Genauigkeit, Zwangsbremsungen,
Geschwindigkeitsüberschreitungen und Fahrenergie; das HUD zeigt Meldungen und Punktestand.
