---
name: screenshot
description: Screenshot von Simulator oder Editor aufnehmen und ansehen. Nutzen, sobald etwas Optisches zu prüfen ist — Gelände, Gleis, HUD, Luftbild-Overlay, Kamera, Beleuchtung — und immer nach einer Änderung an Rendering, UI oder Editor-Darstellung, bevor sie als fertig gemeldet wird. Trigger: "sieht das richtig aus", "zeig mir", "Screenshot", "Bild vom Editor", "wie sieht X aus", grafische Fehler.
---

# Screenshot

Beide Binaries können sich selbst fotografieren und beenden sich danach — kein Fenster-Gefummel, kein manuelles Zutun.

```bash
cargo build -p app -p editor            # einmal, danach direkt die Binaries aufrufen

./target/debug/train-sim.exe --screenshot screenshots/hud.png
./target/debug/train-sim-editor.exe strecke.ron --screenshot screenshots/editor.png
```

Danach das PNG mit **Read** ansehen — das Bild ist die Antwort, nicht die Logausgabe.

## Optionen

| Flag | Wirkung |
|---|---|
| `--screenshot <datei.png>` | Aufnahme des Fensters, danach Ende. Verzeichnis wird angelegt. |
| `--frames N` | Aufnahme erst nach N Frames (≈ N/60 Sekunden). Ohne Angabe: 60. |

`--frames` ist der einzige Hebel auf den Zeitpunkt: mehr Frames = mehr Simulationszeit vor dem Bild
(KI-Züge sind gefahren, Luftbildkacheln sind geladen). 300 Frames für geladenes Overlay, 60 reichen
für Geometrie und HUD.

Editor zusätzlich: Streckendatei als erstes Argument (ohne: Beispielstrecke), `--imagery <konfig.ron>`.

## Arbeitsweise

- Ablage unter `screenshots/` (gitignored), sprechender Name pro Sache: `screenshots/ueberhoehung-bogen.png`.
- Vorher/Nachher bei Fixes: erst mit altem Stand aufnehmen, ändern, mit gleichem `--frames` erneut — sonst
  vergleicht man zwei verschiedene Zeitpunkte.
- Der Build dauert beim ersten Mal Minuten, danach Sekunden. Binary direkt aufrufen statt `cargo run`.

## Grenzen

- Es wird immer der **Startzustand plus N Frames** aufgenommen: keine Tastatureingaben, also
  Führerstandskamera (F2/F3 und Fahrschalter sind nur interaktiv erreichbar) und Zug im Anfangszustand.
  Fahrdynamik prüft man über `cargo test`, nicht über Bilder.
- Braucht eine GPU und eine Desktop-Sitzung. In headless-CI schlägt es fehl — dort bleibt `--frames` als
  reiner Rendering-Smoke-Test.
