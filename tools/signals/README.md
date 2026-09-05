# Signal preview and visual regression

Signal work is reviewed with the same Bevy renderer used by the simulator. This
avoids three recurring sources of error: perspective-dependent proportions,
Blender/game material differences and inspecting only one side of a model.

## Live component workbench (recommended)

Start one immutable review session before editing a component:

```sh
python tools/signals/workbench.py hp-gitter \
  --component fluegel1 --kind geometry \
  --reference /path/to/orthogonal-front-photo.jpg --port 4178
```

Open `http://localhost:4178/`. The first pass launches the real windowless Bevy
binary and records full/front/side/rear, installed-detail, isolated-detail and
animation views. The browser then watches the procedural source. After a save it
regenerates only the canonical model, incrementally rebuilds the renderer and
shows the fixed before image beside the new image and a pixel difference. Exact
framed and whole-signal bounds are reported in centimetres.

The session is intentionally scoped. `--kind geometry` permits geometry changes
only on `fluegel1`; PBR shading and animation bindings remain locked even on that
blade. Other choices are `material`, `animation` and `assembly`. Shared edits
must name every deliberately affected family, for example:

```sh
python tools/signals/workbench.py hp-gitter \
  --component fluegel1 --allow-node fluegel2 --kind material
```

Before taking any new screenshot, the workbench hashes every named glTF node's
consumed mesh bytes, placement, material/texture graph and source image bytes,
plus every node-owned RON motion/lamp binding. A shared paint change leaking
onto the mast, a reversed unrelated motion, or a silently moved lamp therefore
blocks the iteration and names the collateral node. The original images and
guard manifest never change during a session; start a new session only after a
component has been reviewed and accepted.

Sessions live below the ignored `target/signal-workbench/` tree because their
immutable baseline is review evidence rather than a disposable `/tmp` cache. If
the terminal or browser process stops, resume the exact same baseline with:

```sh
python tools/signals/workbench.py hp-gitter \
  --component fluegel1 --kind geometry \
  --resume target/signal-workbench/<session> --port 4178
```

Resume validates the profile, component, edit domain, allowed node set and full
camera plan against `session.json`; a mismatched invocation fails instead of
quietly creating incomparable screenshots. References remain fixed as well.

Fixed Hauptsignal geometry is split into reviewable real-world assemblies:
`mast_foundation`, `mast_structure`, `mast_board`, `mast_head`, `mast_rods` and
`mast_drive`. Select the narrowest prefix that describes the intended edit.
For example, use `--component mast_board --kind geometry` for the red/white
enamel plates. That session fails if the lattice or head changes even by one
accessor byte. Use the broader `--component mast --kind assembly` only for a
deliberate cross-assembly refactor, and require a zero-pixel/zero-centimetre
result before beginning any visual correction.

The safe iteration order is: (1) start a fresh immutable baseline, (2) make one
component/domain change, (3) accept only when the collateral guard, every camera
view and the dimensions pass, (4) run the full generator/review gate, then (5)
start a new baseline for the next component. Never reuse a baseline after
accepting a component; that would mix two independent hypotheses.

Interactive generation uses:

```sh
python tools/gen_form_signals.py --only form_hp_8m_gitter_2fl
```

This writes and validates just one canonical model. The ordinary command without
`--only` remains the mandatory family/release gate for all 188 geometry variants,
42 coupled configurations and the showcase line.

## Fast single view

```sh
python tools/signals/preview.py form_hp_8m_gitter_2fl \
  --aspect hp2 --view front --focus head
```

The first run incrementally builds `trainsim-signal-editor`; later screenshots
normally take under a second. Output goes to
`/tmp/connected-rails-signal-preview/<model>/`. The renderer:

- selects one model RON directly, without loading a route or train;
- applies `hp0`, `hp1`, `hp2`, `vr0`, `vr1`, `vr2`, `sh0` or `sh1` as a settled
  pose, including lamps, colour filters and every motion-bound component;
- selects exactly one LOD;
- frames the actual mesh bounds in an orthographic camera;
- emits a UI-free PNG at a stable scale through a truly windowless offscreen
  target, independent of desktop resolution and compositor window rules;
- emits a `.bounds.json` sidecar with the visible assembly's min/max/size in
  metres, so centimetre drift is checked numerically instead of inferred from
  pixels; the same sidecar records every moving node's exact travel, velocity
  and effective angle/distance in that screenshot, so direction and rebound
  are reviewable as numbers as well as pixels;
- rejects an output whose pixel size differs from the requested size.

Independent preview commands may run in parallel. Contact sheets use private
temporary tile directories even when several output PNGs share one parent,
so concurrent front/rear/variant checks cannot corrupt one another.

The default `--background neutral` keeps both white fronts and matte-black
mechanisms readable. Use `--background light` for the dedicated rear-side
silhouette check, or `--background dark` to expose pale edge halos. The chosen
backdrop is recorded in the manifest, so baseline comparisons never depend on
an editor theme.

`--no-build` is accepted only while the renderer and its relevant Rust
dependencies are newer than their sources. A stale binary is rejected instead
of quietly producing a plausible-looking screenshot from yesterday's code.
The manifest also records the renderer executable's SHA-256 hash.

For a category-limited edit, protect the untouched half explicitly. A material
pass reuses the manifest from its before-render as a geometry lock:

```sh
python tools/signals/preview.py form_hp_8m_gitter_2fl --matrix --focus head \
  --protect-geometry /tmp/before/manifest.json --output /tmp/after
```

`--protect-shading MANIFEST` is the inverse guard for a geometry/mechanism pass.
Both checks fail closed if the model, glTF dependency set or protected hash is
missing or different; they never update the guard file.

When geometry of one component is intentionally changing, use the per-node
guard instead of disabling protection for the complete glTF. For example, a
shared upper/lower lamp edit may change only the two lamp-node families:

```sh
python tools/signals/preview.py form_hp_8m_gitter_2fl \
  --aspect hp0 --view rear --target-node laterne1 \
  --protect-unrelated-geometry /tmp/hp-before/hp0-rear-full-lod0-bglight.json \
  --allow-geometry-node laterne1 --allow-geometry-node laterne2 \
  --output /tmp/hp-after
```

The manifest hashes each named mesh node from only its consumed accessor bytes
and its complete ancestor transform chain. The command therefore permits the
listed nodes across all LOD suffixes, while failing if a blade, mast, selector,
balance or any other node changes, moves, appears or disappears. With only one
edited family, `--target-node` is automatically used as the allowed prefix.

Single views may be `front`, `rear`, `left`, `right` or any of the four
front/rear-left/right quarter views. The standard matrix uses six representative
directions and omits only the two mirrored quarter views.
`--focus full` shows the whole signal; `head`, `detail` and `base` use fixed
3.2 m, 1.25 m and 2.5 m inspection windows. `detail` is the stable PBR
microscope for enamel, glass, fasteners and edge wear and defaults to
1600 × 1200 px. `--window 2400x1800` can override any default output size.

For a moving fitting, frame its glTF node directly instead of hoping that a
top crop contains it:

```sh
python tools/signals/preview.py form_hp_8m_gitter_2fl \
  --aspect hp0 --view rear --focus detail --target-node gewicht1 \
  --reference /path/to/rear-mechanism-photo.jpg --output /tmp/hp-weight-check
```

The named component determines the complete orthographic camera frame and is
not cut again by the generic `head`/`detail` window; `focus` still selects the
default pixel resolution. The rest of the signal remains rendered around it,
which exposes a wrong attachment or depth plane. Prefix matching deliberately
works across `_LOD0`, `_LOD1` and `_LOD2`. A nonexistent or hidden node fails the capture. For a single view,
`--output` accepts either an explicit `.png` or a directory; suffixless paths
are always directories, so a mistyped output cannot masquerade as an image.

If mast or linkage hides the detail, add `--isolate-target` for a second,
unobstructed render:

```sh
python tools/signals/preview.py form_hp_8m_gitter_2fl \
  --aspect hp0 --view rear --focus detail --target-node laterne1 \
  --isolate-target --background light --output /tmp/hp-lantern-isolated
```

Isolation uses a capture-only render layer. It does not remove or rewrite any
glTF node, and the bounds sidecar still records the complete loaded assembly.
Always retain the ordinary target render as the attachment/depth check; the
isolated image is the complementary material and silhouette microscope.

## One-command review matrix

```sh
python tools/signals/preview.py form_vr_4_87m_3begr \
  --matrix --focus head --background neutral \
  --reference /path/to/front-photo.jpg \
  --reference /path/to/rear-photo.jpg
```

This renders every valid aspect from six directions and writes a labelled
`contact-sheet.png` plus `manifest.json`. The manifest hashes the model RON,
glTF and every external PBR map. It also records independent geometry and
shading fingerprints for each glTF, so a material-only edit can prove that no
blade, mast or fitting moved.

Use `--generate` when the procedural source changed. Usually it is faster to run
the generator once and then take several preview matrices with `--no-build`.

## Deterministic animation film strip

```sh
python tools/signals/preview.py form_hp_8m_gitter_2fl \
  --animation hp1:hp0 --view front --focus head
```

This writes `animation-strip.png` with exact samples from the simulator's own
motion integrator. It does not depend on frame rate or on catching a live window
at the right instant. The default times cover fall, first impact, rebound and
settling. Override them for a mechanism with a different travel time:

```sh
python tools/signals/preview.py form_vr_4_87m_3begr \
  --animation vr0:vr2 --animation-times 0,0.3,0.6,0.9,1.2,1.6,2.0 \
  --view front-right --focus head
```

At `t=0` both mechanics and lights show the source aspect. Later samples select
the target lights while arms, discs and colour filters follow their configured
motion profiles. The generated manifest records source, target, sample times and
hashes of the exact RON/glTF inputs.

## One-command review pack

`review.py` turns the individual renders into a repeatable gate. It regenerates
the procedural catalogue, builds the preview binary once, runs independent
views in parallel and writes an HTML index plus a machine-readable manifest:

```sh
python tools/signals/review.py hp-gitter --mode quick
python tools/signals/review.py vr-electric --mode standard \
  --reference /path/to/front-photo.jpg
python tools/signals/review.py core --mode full
```

Use `--list` to see all canonical constructions and suites. `quick` captures
front, side, rear, profile-specific component microscopes and compact critical
animation strips after each small edit.
`standard` covers every state from six directions plus all configured motions.
`full` adds a full-height matrix, base crops and explicit LOD1/LOD2 checks.

Reports go below `/tmp/connected-rails-signal-review/` unless `--output` is
supplied. Every run requires a new directory, so a stale image can never be
mistaken for output from the current source.

After recording one ordinary preview manifest before a component edit, the
entire quick pack can enforce both its per-node and edit-domain allow-list in one
command:

```sh
python tools/signals/review.py hp-gitter --mode quick \
  --before-manifest /tmp/hp-before.json \
  --changed-node laterne1 --changed-node laterne2 --edit-kind material
```

Every capture in that report then fails if geometry or animation changed at all,
or if shading changed outside the two lamp families. The report records the
guard path, allow-list and permitted domain. A before-manifest intentionally
guards exactly one profile, so it cannot be accidentally applied to a mixed
family suite.

Each profile owns a checked component list. Hp includes both blade poses, the
rear balance and the colour selector; three-aspect Vr includes the disc face
and folded side, additional wing, colour filter and drive (plus the gas bottle
only on gas models). Sh and free Ne 2 have their own targeted nodes. The tests
verify that every configured node prefix exists and that its requested aspect
is valid before a review is run.

## Visual regression guard

After a model has been accepted visually:

```sh
python tools/signals/preview.py form_hp_8m_gitter_2fl \
  --matrix --focus head --accept-baseline \
  --approval-note "user approved all six views in review message 42"
```

After later edits, check that unrelated views did not move:

```sh
python tools/signals/preview.py form_hp_8m_gitter_2fl \
  --matrix --focus head --compare-baseline
```

The comparison writes difference images beside the new matrix and fails when
the normalized RMSE exceeds `0.002` (configurable with `--max-rmse`). Baselines
live under `screenshots/signals/baseline/<model>/` only after explicit
acceptance; the tool never silently updates them. Acceptance requires a human
approval note, records the exact render/input manifest and seals every baseline
with its SHA-256 hash. A missing, edited or provenance-free baseline fails
closed.

## Recommended iteration loop

1. Start `workbench.py` for exactly one component and one edit kind. Keep the
   source photograph/drawing in that session. Its fixed full/front/side/rear,
   attached and isolated views make the Bevy result immediately inspectable.
2. Work first on one canonical model for that construction: 8-m Hp Gitter,
   8-m Hp Schmal, wire-driven electric-light Vr, Siemens-driven Vr, gas Vr or
   Sh. Change only one category at a time: dimensions, silhouette, mechanism or
   material. The workbench's component/domain guard must pass before proceeding.
3. Let the workbench use targeted generation while iterating. Run
   `python tools/gen_form_signals.py` without `--only` after the component is
   accepted; its whole-catalogue dimensions and node-binding assertions are the
   numerical propagation gate.
4. Run its `standard` review pack: the six-direction matrix for every valid
   aspect, the rear on a light background and every relevant animation strip.
5. Compare against the source photos and the last accepted baseline. Accept a
   new baseline only after the intended difference is understood in every view.
6. Only then run the family or `core --mode full` pack and check the whole
   188-variant geometry catalogue plus the coupled-drive showcase examples.
   Never repair a generated glTF or RON directly.

Coupled two-arm models carry the suffix `_gekuppelt`. Their preview matrix contains only
Hp 0 and Hp 2, and `--animation hp0:hp2` exercises the one shared mechanical command:

```sh
python tools/signals/preview.py form_hp_8m_gitter_2fl_gekuppelt \
  --animation hp0:hp2 --view front --focus head
```

The older `tools/render_signal_preview.py` remains useful as an independent
Blender cross-check, but it is no longer the primary approval renderer.

## Whole-catalogue integration shot

Once the canonical models pass their matrices, verify that the generated
catalogue also assembles in the simulator:

```sh
cargo run -p app -- --line example:formsignal_showcase --loco example:br101_afb \
  --camera fly --fly 0,18,280 --look 0,6,458 --hud off \
  --window 1920x1080 --frames 70 --screenshot /tmp/formsignal-showcase.png
```

This is an integration check for all 190 showcase placements, their selected
LODs and the ten-second demo script. It is intentionally not the silhouette
approval image: the per-model orthographic matrix is the authoritative close
inspection.
