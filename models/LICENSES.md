# Detector licences

The two `.onnx` files in this directory are the weights the route editor's
imagery detection runs (README, *Detecting from the aerial imagery*). Neither
was trained here: both are published, pre-trained detectors, converted to ONNX
by [`tools/vision/export_models.py`](../tools/vision/export_models.py) and
otherwise unmodified. Each keeps the licence of the work it came from, and the
two are **not the same licence** — read both before redistributing the game.

## `deepforest-tree.onnx` — tree crowns — MIT

The crown detector is **DeepForest** by the Weecology lab (Ben Weinstein, Sergio
Marconi, Ethan White et al., University of Florida), published under the
[MIT licence](https://github.com/weecology/DeepForest/blob/main/LICENSE). The
weights are the `weecology/deepforest-tree` release on Hugging Face
(<https://huggingface.co/weecology/deepforest-tree>), a torchvision RetinaNet
with a ResNet-50 FPN backbone trained on the airborne survey of the
[NSF NEON](https://www.neonscience.org/) observatory network, whose data are
published for free use with attribution.

Please cite the work if you build on it:

> Weinstein, B.G., Marconi, S., Bohlman, S., Zare, A., White, E. (2019).
> Individual tree-crown detection in RGB imagery using semi-supervised deep
> learning neural networks. *Remote Sensing*, 11(11), 1309.

MIT permits use, modification and commercial distribution as long as the
copyright notice and the licence text travel with it:

```
MIT License

Copyright (c) 2019 Weecology

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

What is in the file is the backbone and the detection head at a fixed input of
768 × 768. The resize, the normalisation, the anchor decoding and the
non-maximum suppression that the Python package performs around them are the
editor's own (`crates/vision/src/onnx.rs`), which is why the file holds no
boxes — only logits and offsets against an anchor grid that is rebuilt from the
input size.

## `yolov8n-obb.onnx` — parked cars and lorries — AGPL-3.0

The vehicle detector is **Ultralytics YOLOv8n-OBB**, trained by Ultralytics on
**DOTA v1.0**, and it is published under the
[GNU Affero General Public License v3.0](https://www.gnu.org/licenses/agpl-3.0.html).
That is a copyleft licence with a network clause, and it is the reason this
file is called out here rather than merely listed:

* **Shipping it makes the AGPL apply to it.** Anyone who redistributes this
  repository, or a game built from it, redistributes an AGPL-3.0 work and takes
  on the obligations that go with it. The EUPL v1.2 that covers the rest of the
  repository names AGPL-3.0 among its compatible licences (EUPL Appendix), so
  the combination is provided for — but the result is that the combined work
  travels under the AGPL, not that the AGPL file quietly becomes EUPL.
* **The training data has terms of its own.** DOTA is released for academic
  research. Weights trained on it inherit that context, whatever the licence on
  the code that produced them.

If neither is acceptable for how you intend to distribute this — a closed
release, or a commercial one — then **delete `yolov8n-obb.onnx`**. Nothing
breaks: the editor lists the model, says the weights are not installed, and the
tree detector, the imports and everything else carry on. Put a detector of your
own in its place by naming it in `ai.ron`; the registry is a file, not code
(README, *Models*).

The tree detector is unaffected by any of this. It is MIT.
