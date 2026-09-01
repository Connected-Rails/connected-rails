#!/usr/bin/env python3
"""Builds `tiny-obb.onnx`, the fixture `tests/onnx.rs` runs against.

There is no way to test an inference backend without a model, and no model
that may be shipped with the game — every published detector is either too
big or licensed so it cannot travel with a route editor. So this builds the
smallest thing that is shaped like one: an oriented head with two classes and
a single anchor, whose every number is the mean brightness of the window times
a constant.

That is enough to prove the whole path in one go — the scaling of a byte to a
float, the NCHW layout, the run itself, and the decoding of the output — and
it fails loudly if any of them is wrong, because a bright window has to come
back with exactly the constants and a dark one with nothing at all.

    python3 -m venv /tmp/onnxvenv && /tmp/onnxvenv/bin/pip install onnx
    /tmp/onnxvenv/bin/python crates/vision/tests/fixtures/make_tiny_obb.py
"""

from pathlib import Path

import numpy as np
import onnx
from onnx import TensorProto, helper, numpy_helper

# cx, cy, w, h, score of class 0, score of class 1, angle — the Ultralytics
# oriented layout, one anchor wide.
ROWS = np.array([[4.0], [2.0], [6.0], [3.0], [0.9], [0.1], [0.5]], dtype=np.float32)

graph = helper.make_graph(
    nodes=[
        # The mean over every pixel and channel, as a scalar.
        helper.make_node(
            "ReduceMean", ["images"], ["brightness"], axes=[0, 1, 2, 3], keepdims=0
        ),
        # …which every output number is multiplied by.
        helper.make_node("Mul", ["rows", "brightness"], ["output0"]),
    ],
    name="tiny-obb",
    inputs=[helper.make_tensor_value_info("images", TensorProto.FLOAT, [1, 3, 8, 8])],
    outputs=[helper.make_tensor_value_info("output0", TensorProto.FLOAT, [1, 7, 1])],
    initializer=[numpy_helper.from_array(ROWS.reshape(1, 7, 1), "rows")],
)
model = helper.make_model(
    graph,
    producer_name="connected-rails",
    # 13, not the newest: it is the last opset where ReduceMean takes its axes
    # as an attribute, which is what every exporter in the wild still writes.
    opset_imports=[helper.make_operatorsetid("", 13)],
)
model.ir_version = 8
onnx.checker.check_model(model)
path = Path(__file__).with_name("tiny-obb.onnx")
path.write_bytes(model.SerializeToString())
print(f"{path} ({path.stat().st_size} bytes)")
