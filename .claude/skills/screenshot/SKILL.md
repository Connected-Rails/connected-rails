---
name: screenshot
description: Screenshot von Simulator oder Editor aufnehmen und ansehen. Nutzen, sobald etwas Optisches zu prüfen ist — Gelände, Gleis, HUD, Luftbild-Overlay, Kamera, Beleuchtung — und immer nach einer Änderung an Rendering, UI oder Editor-Darstellung, bevor sie als fertig gemeldet wird. Trigger: "sieht das richtig aus", "zeig mir", "Screenshot", "Bild vom Editor", "wie sieht X aus", grafische Fehler.
---

# Screenshot

Alle vier Binaries können sich selbst fotografieren und beenden sich danach — kein
Fenster-Gefummel, kein manuelles Zutun.

```bash
cargo build -p app -p route-editor -p signal-editor -p vehicle-editor   # einmal

./target/debug/train-sim.exe              --screenshot screenshots/hud.png
./target/debug/trainsim-route-editor.exe  modul.ron   --screenshot screenshots/modul.png
./target/debug/trainsim-signal-editor.exe signal.ron  --screenshot screenshots/signal.png
./target/debug/trainsim-vehicle-editor.exe fahrzeug.ron --screenshot screenshots/fahrzeug.png
```

Danach das PNG mit **Read** ansehen — das Bild ist die Antwort, nicht die Logausgabe.

**Ohne `--features dev` bauen.** Ein Dev-Build linkt Bevy dynamisch; das Binary findet die
DLL dann nur über `cargo run -p <crate> --features dev -- --screenshot …`, direkt aufgerufen
bricht es mit „cannot open shared object file" ab.

## Optionen für alle

| Flag | Wirkung |
|---|---|
| `--screenshot <datei.png>` | Aufnahme des Fensters, danach Ende. Verzeichnis wird angelegt. |
| `--frames N` | Aufnahme erst nach N Frames (≈ N/60 Sekunden). Ohne Angabe: 60. |
| `--height M` | Nur Moduleditor: Starthöhe des Blickpunkts über der Strecke in Metern (Vorgabe 900). 60 zeigt Bäume und Objekte, 900 das Modul. |
| `--drawer [kategorie]` | Nur Moduleditor: Inhalte-Schublade offen aufnehmen — sonst nur per `Ctrl`+`Space` erreichbar. Kategorie optional: `objects` (Vorgabe), `signal-types`, `signal-models`, `track-types`. |
| `--tool <name>` | Nur Moduleditor: mit diesem Werkzeug in der Hand starten — der Werkzeug-Abschnitt zeigt nur die Optionen des aktiven. Namen wie die i18n-Keys ohne Präfix: `select`, `draw`, `split`, `join`, `offset`, `crossover`, `gradient`, `area`, `device`, `object`, `marker`, `tree`, `forest`, `brush`, `terrain-raise`, `terrain-lower`, `terrain-level`, `terrain-rail`, `tile`, `envelope`. |

`--frames` ist der einzige Hebel auf den Zeitpunkt: mehr Frames = mehr Simulationszeit vor dem
Bild (KI-Züge sind gefahren, Luftbildkacheln sind geladen). 300 Frames für geladenes Overlay,
60 reichen für Geometrie und HUD.

## Nur Simulator (`train-sim`)

| Flag | Wirkung |
|---|---|
| `--hud <stufe>` | `full`, `reduced` oder `off` — die drei Stufen der Anzeige (F7). Schreibt die Einstellungsdatei nicht. |
| `--overlays` | Öffnet Tastenhilfe (F5) und Diagnose (F6) von Anfang an — sonst nur per Tastendruck erreichbar. |
| `--menu [seite]` | Fotografiert das Hauptmenü statt der Welt dahinter. Seite optional: `root` (Vorgabe), `line`, `loco`, `scenario`, `mods`, `settings`, `controls`. |
| `--camera <modus>` | `outside` für die Außenkamera (Fahrzeugmodelle), `walk` für zu Fuß — beides sonst nur über F4. |
| `--time HH:MM` | Startuhrzeit der Fahrt, etwa `21:40` für den Nachthimmel. |
| `--date JJJJ-MM-TT` | Startdatum — entscheidet über die Jahreszeit von Boden und Bewuchs. |
| `--weather <preset>` | `clear`, `cloudy`, `overcast`, `fog`, `drizzle`, `rain`, `storm`, `thunderstorm`, `sleet`, `snow`, `blizzard`, `hail`, `frost`. Zusammen mit `--screenshot` sofort gesetzt statt eingezogen. |
| `--wipers <0-3>` | Startet mit laufendem Scheibenwischer — ein Führerstandshebel, und ein Screenshot hat keine Hände. |
| `--character <datei>` | Modell für den Fußgänger, gleiche `mods://`-Pfade wie Fahrzeugmodelle. |

## Nur Moduleditor (`trainsim-route-editor`)

Modul­datei als erstes Argument (ohne: Beispielmodul), `--imagery <konfig.ron>` für eine andere
Bildmaterial-Konfiguration als `imagery.ron`, `--window 1280x2000` für eine feste Fenstergröße
(hohe Fenster zeigen das ganze Panel).

## Nur Fahrzeugeditor (`trainsim-vehicle-editor`)

`--window 1280x2000` — feste Fenstergröße für reproduzierbare Bilder.

## Arbeitsweise

- Ablage unter `screenshots/` (gitignored), sprechender Name pro Sache: `screenshots/ueberhoehung-bogen.png`.
- Vorher/Nachher bei Fixes: erst mit altem Stand aufnehmen, ändern, mit gleichem `--frames` erneut — sonst
  vergleicht man zwei verschiedene Zeitpunkte.
- Der Build dauert beim ersten Mal Minuten, danach Sekunden. Binary direkt aufrufen statt `cargo run`.

## Grenzen

- Es wird immer der **Startzustand plus N Frames** aufgenommen: keine Tastatureingaben, also
  Führerstandskamera und Fahrschalter nur so weit, wie ein Flag oben hinreicht. Fahrdynamik prüft
  man über `cargo test`, nicht über Bilder. Im Menü ersetzt `--menu <seite>` die fehlende Tastatur;
  die Auswahl steht dabei immer auf der ersten Zeile.
- Dialoge der Editoren, die erst ein Menübefehl öffnet (etwa „Neues Modul"), sind so nicht
  erreichbar — dafür bleibt nur der laufende Editor und ein Blick von Hand. Die
  Inhalte-Schublade ist die Ausnahme: `--drawer` ersetzt dort die fehlende Tastatur.
- Braucht eine GPU und eine Desktop-Sitzung. In headless-CI schlägt es fehl — dort bleibt `--frames` als
  reiner Rendering-Smoke-Test.
