# The detectors in `models/`

The route editor reads the aerial photograph with two local models — one for
the vehicles standing about a station, one for tree crowns (README, *Detecting
from the aerial imagery*). Both **ship with the game**, and both are converted
here from published, pre-trained detectors. Nothing is trained in this
directory.

```
Ultralytics yolov8n-obb.pt ──┐
(DOTA v1, AGPL-3.0)          ├── export_models.py ──▶ models/yolov8n-obb.onnx
weecology/deepforest-tree ───┘                        models/deepforest-tree.onnx
(NEON crowns, MIT)                                    (Git LFS)
```

```bash
uv venv --python 3.12 cache/vision/venv
VIRTUAL_ENV=cache/vision/venv uv pip install torch torchvision onnx \
    onnxruntime ultralytics deepforest
cache/vision/venv/bin/python tools/vision/export_models.py
cache/vision/venv/bin/python tools/vision/export_models.py --only trees
```

The downloads land in `cache/vision/` (gitignored) and the finished weights in
`models/`. Rebuilding is only needed to move to another detector or another
input size; what is in the repository is this script's output, unmodified.

## Why the tree model is exported the way it is

DeepForest is a torchvision RetinaNet, and the whole of it — resize, normalise,
decode, non-maximum suppression — would export as a graph full of dynamic
shapes and control flow, which is exactly the kind of graph a small pure-Rust
runtime is bad at. It does not need to. The editor already scales the window to
the model's resolution, subtracts the ImageNet mean and settles overlapping
boxes; those are `InputSpec` and `suppress` in `crates/vision`. The one thing it
cannot do is guess anchors.

So only the **backbone and the head** are exported, at a fixed 768 × 768, and
the anchor grid is rebuilt in Rust (`crates/vision/src/onnx.rs`, `Head::Retina`).
The script prints the grid torchvision produces at the end of a run; those
numbers are what the Rust test asserts against, down to the anchor whose half
width is 50.5 and which rounds to 50 because Python rounds halves to even.

**768 and not 800.** DeepForest's own transform resizes to 800, whose coarsest
feature map is 7 cells wide where dividing 800 by the stride of 128 says 6. At
768 every level divides exactly, so the grid follows from the input size alone
and there is nothing to special-case.

**0.05 m per pixel.** That is not the resolution DeepForest was trained on — it
reads ten-centimetre imagery and doubles it before the network sees anything.
Five centimetres is the scale at which a crown arrives the number of pixels
across that the model expects, which is what `ground_sample` in `ai.ron` means.
Where the imagery provider cannot go that fine, the window is enlarged instead
and the crowns come out softer; finer imagery is the single biggest thing that
improves this model's results.

## Checking a rebuild

`export_models.py` runs each file once through onnxruntime and prints the input
and output shapes, which catches a broken export. What it cannot catch is a
*different* export — a model that runs and finds nothing. Two things do:

* `cargo test -p vision` — the anchor grid against torchvision's own numbers,
  and, when the weights are present, that both files load and run
  (`crates/vision/tests/shipped.rs`).
* Running the detection over ground you know, in the editor, and looking at it.

The reference figures, for the tree model on DeepForest's own test image
(`OSBS_029.png`, 400 × 400): the Python package finds 55 crowns, this pipeline
finds 50 at a confidence of 0.3, and the boxes they share agree to about a
pixel — a tenth of a metre on that survey.

## Licences

Not the same for the two, and it matters: MIT for the trees, **AGPL-3.0** for
the cars. [`models/LICENSES.md`](../../models/LICENSES.md) has both in full,
including what shipping the AGPL one means for a release and how to drop it.
