#!/usr/bin/env python3
"""Builds the detectors in `models/` — the weights the imagery detection runs.

Two models ship with the game, and this is where they come from. Neither is
trained here: both are published, pre-trained detectors, converted into the one
format the editor's runtime reads (ONNX, run by tract in process). What this
script does is the conversion, and it does it reproducibly, because a weights
file nobody can rebuild is a weights file nobody can check.

    models/yolov8n-obb.onnx    parked cars and lorries   (Ultralytics, AGPL-3.0)
    models/deepforest-tree.onnx  tree crowns             (DeepForest, MIT)

The tree export is the interesting half. DeepForest is a torchvision RetinaNet,
and the whole of it — resize, normalise, decode, non-maximum suppression —
would export as a graph full of dynamic shapes and control flow. It does not
need to: the editor already scales the window, subtracts the mean and settles
overlaps, and what it cannot do is guess anchors. So only the backbone and the
head are exported, at a fixed size, and the anchor grid is rebuilt in Rust
(`crates/vision/src/onnx.rs`). The numbers this script prints at the end are
what the test over there asserts against.

    uv venv --python 3.12 cache/vision/venv
    VIRTUAL_ENV=cache/vision/venv uv pip install torch torchvision onnx \\
        onnxruntime ultralytics deepforest
    cache/vision/venv/bin/python tools/vision/export_models.py [--only cars|trees]

Rebuilding is only necessary to move to another detector or another input size.
The models in the repository are the output of this script, unmodified.
"""

import argparse
import shutil
import sys
import warnings
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MODELS = ROOT / "models"
CACHE = ROOT / "cache" / "vision"

# The size the tree model is exported for. A multiple of the coarsest feature
# stride (128), which is what lets the anchor grid be rebuilt by integer
# division; 800, which is what DeepForest's own transform resizes to, is not —
# its coarsest feature map is 7 cells for a stride that would say 6.
TREE_INPUT = 768

# What the car detector was trained at. Ultralytics' DOTA weights are 1024, and
# a smaller export would put the cars at the wrong number of pixels across.
CAR_INPUT = 1024


def export_cars() -> Path:
    """YOLOv8n-OBB on DOTA v1 → ONNX.

    Ultralytics exports the layout the editor already reads, so this is one
    call. The licence is not one line, though: these weights are AGPL-3.0, and
    shipping them is a decision recorded in `models/LICENSES.md`.
    """
    from ultralytics import YOLO

    CACHE.mkdir(parents=True, exist_ok=True)
    weights = CACHE / "yolov8n-obb.pt"
    if not weights.is_file():
        print(f"fetching {weights.name} …")
        YOLO("yolov8n-obb.pt")  # downloads into the working directory
        downloaded = Path("yolov8n-obb.pt")
        if downloaded.is_file():
            shutil.move(str(downloaded), weights)
    model = YOLO(str(weights))
    exported = Path(model.export(format="onnx", imgsz=CAR_INPUT, opset=17))
    target = MODELS / "yolov8n-obb.onnx"
    MODELS.mkdir(parents=True, exist_ok=True)
    shutil.move(str(exported), target)
    return target


def export_trees() -> Path:
    """DeepForest's NEON crown model → ONNX, as two head tensors.

    What comes out is `cls_logits [1, anchors, 1]` and
    `bbox_regression [1, anchors, 4]`. Neither holds a box: an offset is
    measured from the anchor it belongs to, and the anchors are rebuilt on the
    Rust side from the input size alone.
    """
    import torch
    import torch.nn as nn
    from deepforest import main as deepforest

    class Raw(nn.Module):
        def __init__(self, net):
            super().__init__()
            self.net = net

        def forward(self, x):
            out = self.net.head(list(self.net.backbone(x).values()))
            return out["cls_logits"], out["bbox_regression"]

    trained = deepforest.deepforest()
    trained.load_model("weecology/deepforest-tree")
    net = trained.model.eval()
    raw = Raw(net).eval()

    MODELS.mkdir(parents=True, exist_ok=True)
    target = MODELS / "deepforest-tree.onnx"
    sample = torch.rand(1, 3, TREE_INPUT, TREE_INPUT)
    torch.onnx.export(
        raw,
        (sample,),
        str(target),
        input_names=["images"],
        output_names=["cls_logits", "bbox_regression"],
        opset_version=17,
        dynamo=False,
    )
    fingerprint(net, sample)
    return target


def fingerprint(net, sample) -> None:
    """Prints the anchor grid the Rust side has to reproduce.

    `crates/vision/src/onnx.rs` rebuilds this grid, and its test asserts these
    exact numbers. They are printed rather than written to a file because they
    belong in the test, where a reader can see what is being claimed.
    """
    import torch
    import torchvision

    features = list(net.backbone(sample).values())
    images = torchvision.models.detection.image_list.ImageList(
        sample, [(TREE_INPUT, TREE_INPUT)]
    )
    anchors = net.anchor_generator(images, features)[0]
    rows = lambda block: [[round(v, 1) for v in row] for row in block.tolist()]
    level1 = 96 * 96 * 9
    print("\nanchor grid, for the test in crates/vision/src/onnx.rs:")
    print(f"  count      {anchors.shape[0]}")
    print(f"  first five {rows(anchors[:5])}")
    print(f"  level 1    {rows(anchors[level1:level1 + 9])}")
    print(f"  at 50000   {rows(anchors[50_000:50_001])}")
    print(f"  last five  {rows(anchors[-5:])}")
    print(f"  sum        {float(anchors.sum()):.1f}")


def check(path: Path) -> None:
    """Runs the exported file once, so a broken export fails here."""
    import numpy as np
    import onnxruntime as ort

    session = ort.InferenceSession(str(path), providers=["CPUExecutionProvider"])
    shape = session.get_inputs()[0].shape
    dummy = np.zeros([d if isinstance(d, int) else 1 for d in shape], dtype=np.float32)
    outputs = session.run(None, {session.get_inputs()[0].name: dummy})
    size = path.stat().st_size / 1e6
    print(
        f"{path.relative_to(ROOT)}: {size:.1f} MB, input {shape}, "
        f"outputs {[list(o.shape) for o in outputs]}"
    )


def main() -> None:
    warnings.filterwarnings("ignore")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--only", choices=["cars", "trees"], help="build one of them")
    args = parser.parse_args()

    built = []
    if args.only in (None, "cars"):
        built.append(export_cars())
    if args.only in (None, "trees"):
        built.append(export_trees())
    print()
    for path in built:
        check(path)
    print(
        "\nThe weights are tracked with Git LFS (.gitattributes) and licensed "
        "as `models/LICENSES.md` records."
    )


if __name__ == "__main__":
    sys.exit(main())
