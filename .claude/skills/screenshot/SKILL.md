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

**Der Simulator macht das ohne Fenster.** Mit `--screenshot` legt `train-sim` gar
kein Fenster an: er rendert in ein Bild und schreibt das. Nichts erscheint auf
dem Desktop, nichts nimmt den Fokus, und — der Punkt — nichts kann die Größe
verändern. Das Bild ist immer 1920x1080, `--window 2560x1440` setzt eine andere
feste Größe. Vorher wurde das Fenster des Compositors fotografiert, und dessen
Größe hing daran, was am Rechner sonst gerade lief; zwei Aufnahmen vor und nach
einer Änderung waren dann nicht vergleichbar. Die drei Editoren öffnen weiterhin
ein Fenster (`--window WxH` fixiert dort wenigstens die Größe).

**Ohne `--features dev` bauen.** Ein Dev-Build linkt Bevy dynamisch; das Binary findet die
DLL dann nur über `cargo run -p <crate> --features dev -- --screenshot …`, direkt aufgerufen
bricht es mit „cannot open shared object file" ab.

## Optionen für alle

| Flag | Wirkung |
|---|---|
| `--screenshot <datei.png>` | Aufnahme des Fensters, danach Ende. Verzeichnis wird angelegt. |
| `--frames N` | Aufnahme erst nach N Frames (≈ N/60 Sekunden). Ohne Angabe: 60. |
| `--window BxH` | Feste Bildgröße. Simulator: die Größe des fensterlosen Rendertargets (Vorgabe 1920x1080). Editoren: die Fenstergröße. |
| `--height M` | Nur Moduleditor: Starthöhe des Blickpunkts über der Strecke in Metern (Vorgabe 900). 60 zeigt Bäume und Objekte, 900 das Modul. |
| `--at KM` | Nur Moduleditor: welchen Kilometer der Strecke der Blick trifft (Vorgabe: die Mitte). Ein fünf Kilometer langes Modul hat eine interessante Ecke, und die ist selten die Mitte — aus der Mitte in der Höhe, die bis ans andere Ende reicht, ist das Gesuchte ein Pixel. |
| `--drawer [kategorie]` | Nur Moduleditor: Inhalte-Schublade offen aufnehmen — sonst nur per `Ctrl`+`Space` erreichbar. Kategorie optional: `objects` (Vorgabe), `signal-types`, `signal-models`, `track-types`. |
| `--tool <name>` | Nur Moduleditor: mit diesem Werkzeug in der Hand starten — der Werkzeug-Abschnitt zeigt nur die Optionen des aktiven. Namen wie die i18n-Keys ohne Präfix: `select`, `draw`, `split`, `join`, `offset`, `crossover`, `gradient`, `area`, `device`, `object`, `marker`, `tree`, `forest`, `brush`, `terrain-raise`, `terrain-lower`, `terrain-level`, `terrain-rail`, `tile`, `walk-path`, `walk-area`, `envelope`. |

`--frames` ist der einzige Hebel auf den Zeitpunkt: mehr Frames = mehr Simulationszeit vor dem
Bild (KI-Züge sind gefahren, Luftbildkacheln sind geladen). 300 Frames für geladenes Overlay,
60 reichen für Geometrie und HUD.

## Nur Simulator (`train-sim`)

| Flag | Wirkung |
|---|---|
| `--hud <stufe>` | `full`, `reduced` oder `off` — die drei Stufen der Anzeige (F7). Schreibt die Einstellungsdatei nicht. |
| `--overlays` | Öffnet Tastenhilfe (F5) und Diagnose (F6) von Anfang an — sonst nur per Tastendruck erreichbar. |
| `--menu [seite]` | Fotografiert das Hauptmenü statt der Welt dahinter. Seite optional: `root` (Vorgabe), `line`, `loco`, `scenario`, `mods`, `settings`, `controls`. |
| `--camera <modus>` | `outside` für die Außenkamera (Fahrzeugmodelle), `walk` für zu Fuß, `fly` für die Freikamera des Konsolenbefehls `fly` — alles sonst nur per Taste oder Konsole erreichbar. |
| `--fly R,H,V` | Nur mit `--camera fly`: wo die Freikamera steht, in Metern **rechts**, **über** und **vor** dem Zug. Vorgabe `25,6,0`. |
| `--look R,H,V` | Wohin sie schaut, im selben Bezug. Vorgabe `0,2,0` — der Zug. Ohne die beiden kann eine Aufnahme nur den Zug zeigen, und alles über etwa zehn Metern läuft oben aus dem Bild. |
| `--time HH:MM` | Startuhrzeit der Fahrt, etwa `21:40` für den Nachthimmel. |
| `--date JJJJ-MM-TT` | Startdatum — entscheidet über die Jahreszeit von Boden und Bewuchs. |
| `--weather <preset>` | `clear`, `cloudy`, `overcast`, `fog`, `drizzle`, `rain`, `storm`, `thunderstorm`, `sleet`, `snow`, `blizzard`, `hail`, `frost`. Zusammen mit `--screenshot` sofort gesetzt statt eingezogen. |
| `--wipers <0-3>` | Startet mit laufendem Scheibenwischer — ein Führerstandshebel, und ein Screenshot hat keine Hände. |
| `--character <name\|datei>` | Modell für den Fußgänger: eine Person aus den Mods (`people:f01_lena`) oder eine Datei auf den `mods://`-Pfaden der Fahrzeugmodelle. Ohne Flag die erste Person mit der Rolle `Player`. |

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
