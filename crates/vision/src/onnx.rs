//! The local runtime: an ONNX file, run in this process, on this machine.
//!
//! [tract] is a pure-Rust inference runtime. That choice is the whole reason
//! the feature can be shipped at all: no ONNX Runtime to bundle per platform,
//! no shared library to find at start-up, nothing downloaded while building,
//! and a route editor that still compiles on a machine that has never seen a
//! model. It runs on the processor, which for this job is the right trade —
//! a corridor through a module is a few dozen windows, and a few dozen
//! windows is under a minute.
//!
//! Two export layouts are decoded, because between them they are what can
//! actually be obtained.
//!
//! The **Ultralytics** one is a single tensor, `[1, rows, anchors]`: four rows
//! of box, one row per class, and — for an oriented model — a last row of
//! angle. Both the row-major and the transposed export are read, since which
//! one comes out depends on the exporter's version and getting it wrong is
//! silent nonsense rather than an error.
//!
//! The **torchvision RetinaNet** one ([`Head::Retina`]) is two tensors and no
//! boxes: logits per anchor, offsets per anchor, and the anchors themselves
//! nowhere in the file. They are a fixed grid that follows from the input
//! size, and [`anchors`] rebuilds it — the only place in this crate where a
//! detail of somebody else's library is reproduced rather than read, and so
//! the one place with a test that checks the numbers against theirs.
//!
//! [tract]: https://github.com/sonos/tract

use crate::detect::{Detection, Detector};
use crate::model::{ChannelOrder, Head, InputSpec, Layout, ModelSpec};
use std::path::Path;
use std::sync::Arc;
use tract_onnx::prelude::*;

/// A loaded model, ready to be asked about windows.
pub struct OnnxDetector {
    plan: Arc<TypedRunnableModel>,
    input: InputSpec,
    head: Head,
    classes: usize,
    /// Lowest confidence any class of this model asks for — everything under
    /// it is dropped before a box is even built.
    floor: f32,
}

impl OnnxDetector {
    /// Loads the weights and fixes the input shape.
    ///
    /// The shape has to be fixed: an Ultralytics export has a dynamic batch
    /// dimension, and often a dynamic image size, which tract cannot optimise
    /// and would not know how to allocate for.
    pub fn load(spec: &ModelSpec, path: &Path) -> Result<Self, String> {
        let shape = match spec.input.layout {
            Layout::Nchw => [1, 3, spec.input.height as usize, spec.input.width as usize],
            Layout::Nhwc => [1, spec.input.height as usize, spec.input.width as usize, 3],
        };
        let plan = tract_onnx::onnx()
            .model_for_path(path)
            .map_err(|e| format!("{}: {e}", path.display()))?
            .with_input_fact(0, f32::fact(shape).into())
            .map_err(|e| format!("{}: {e}", path.display()))?
            .into_optimized()
            .map_err(|e| format!("{}: {e}", path.display()))?
            .into_runnable()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let floor = (0..spec.classes.len())
            .map(|c| spec.confidence_of(c))
            .fold(spec.head.confidence(), f32::min);
        Ok(Self {
            plan,
            input: spec.input,
            head: spec.head,
            classes: spec.classes.len(),
            floor,
        })
    }
}

impl Detector for OnnxDetector {
    fn detect(&mut self, pixels: &[u8], width: u32, height: u32) -> Result<Vec<Detection>, String> {
        if width != self.input.width || height != self.input.height {
            return Err(format!(
                "window is {width}x{height}, the model wants {}x{}",
                self.input.width, self.input.height
            ));
        }
        let tensor = tensor_from(pixels, &self.input)?;
        let outputs = self
            .plan
            .run(tvec!(tensor.into()))
            .map_err(|e| e.to_string())?;
        if let Head::Retina { .. } = self.head {
            let (logits, offsets) = retina_outputs(&outputs, self.classes)?;
            return decode_retina(
                logits,
                offsets,
                (self.input.width, self.input.height),
                self.classes,
                self.floor,
            );
        }
        let first = outputs.first().ok_or("the model returned nothing")?;
        let view = first
            .to_plain_array_view::<f32>()
            .map_err(|e| e.to_string())?;
        decode(
            view.iter().copied(),
            view.shape(),
            self.head,
            self.classes,
            self.floor,
        )
    }
}

/// The window as the model's input tensor.
fn tensor_from(pixels: &[u8], input: &InputSpec) -> Result<Tensor, String> {
    let (w, h) = (input.width as usize, input.height as usize);
    if pixels.len() < w * h * 3 {
        return Err("window is smaller than the model's input".into());
    }
    let mut data = vec![0f32; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            let source = (y * w + x) * 3;
            for c in 0..3 {
                // The channel of the *model*; the picture is always RGB here.
                let from = match input.order {
                    ChannelOrder::Rgb => c,
                    ChannelOrder::Bgr => 2 - c,
                };
                let value =
                    (pixels[source + from] as f32 * input.scale - input.mean[c]) / input.std[c];
                let at = match input.layout {
                    Layout::Nchw => c * w * h + y * w + x,
                    Layout::Nhwc => (y * w + x) * 3 + c,
                };
                data[at] = value;
            }
        }
    }
    let shape = match input.layout {
        Layout::Nchw => vec![1, 3, h, w],
        Layout::Nhwc => vec![1, h, w, 3],
    };
    Tensor::from_shape(&shape, &data).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// The anchored dense head (torchvision RetinaNet)
// ---------------------------------------------------------------------------

/// Feature strides of a RetinaNet on a ResNet-FPN backbone, finest first.
const RETINA_STRIDES: [u32; 5] = [8, 16, 32, 64, 128];
/// The anchor base size at each of those levels.
const RETINA_BASES: [f32; 5] = [32.0, 64.0, 128.0, 256.0, 512.0];
/// Aspect ratios — height over width — repeated at every level.
const RETINA_ASPECTS: [f32; 3] = [0.5, 1.0, 2.0];
/// `ln(1000 / 16)`: torchvision clamps the size offsets to this before the
/// exponential, so that one absurd logit cannot produce a box the size of a
/// county. Reproduced here because a box that differs from theirs is a box in
/// the wrong place.
const RETINA_CLIP: f32 = 4.135_166_5;

/// Which of the two output tensors is which.
///
/// The exporter names them, tract hands them back in graph order, and nothing
/// guarantees the two agree. The offsets are the tensor four wide, which
/// settles it for every class count but four — and where a model really did
/// have four classes, the declared order (logits first) decides, which is what
/// every exporter of this family emits.
fn retina_outputs<'a>(
    outputs: &'a [TValue],
    classes: usize,
) -> Result<(&'a [f32], &'a [f32]), String> {
    let (first, second) = match (outputs.first(), outputs.get(1)) {
        (Some(first), Some(second)) => (first, second),
        _ => return Err("an anchored head returns two tensors, this model returned one".into()),
    };
    let width = |t: &TValue| t.shape().last().copied().unwrap_or(0);
    let (logits, offsets) = if classes != 4 && width(first) == 4 {
        (second, first)
    } else {
        (first, second)
    };
    let plain = |t: &'a TValue| -> Result<&'a [f32], String> {
        t.to_plain_array_view::<f32>()
            .map_err(|e| e.to_string())?
            .to_slice()
            .ok_or_else(|| "the model returned a tensor that is not laid out plainly".to_string())
    };
    Ok((plain(logits)?, plain(offsets)?))
}

/// Half to even, the rounding Python and PyTorch do and Rust does not.
///
/// It matters in exactly one place and it matters there absolutely: the middle
/// anchor of the second level is 101 wide, its half is 50.5, and torchvision
/// rounds that to 50 where `f32::round` gives 51. Every box decoded from that
/// anchor would be a pixel out, which is a metre and a half of ground.
fn round_half_even(v: f32) -> f32 {
    let rounded = v.round();
    if (v - v.trunc()).abs() == 0.5 && rounded % 2.0 != 0.0 {
        rounded - v.signum()
    } else {
        rounded
    }
}

/// The nine base anchors of one level, in corner form about the origin.
fn base_anchors(base: f32) -> [[f32; 4]; 9] {
    // The base size and it times the cube roots of two, each cut to a whole
    // number before anything else happens — `int(x * 2 ** (1/3))`.
    let sizes = [
        base,
        (base * 2f32.powf(1.0 / 3.0)).trunc(),
        (base * 2f32.powf(2.0 / 3.0)).trunc(),
    ];
    let mut out = [[0.0f32; 4]; 9];
    for (a, aspect) in RETINA_ASPECTS.iter().enumerate() {
        let high = aspect.sqrt();
        let wide = 1.0 / high;
        for (s, size) in sizes.iter().enumerate() {
            let half_w = round_half_even(wide * size / 2.0);
            let half_h = round_half_even(high * size / 2.0);
            // Aspect-major, then size: the order torchvision flattens them in,
            // and the order the head's own rows are in.
            out[a * 3 + s] = [-half_w, -half_h, half_w, half_h];
        }
    }
    out
}

/// The anchor grid the offsets are measured from, in corner form.
///
/// Reproduced from `torchvision.models.detection.anchor_utils.AnchorGenerator`
/// with the RetinaNet defaults — the ones the shipped tree model was trained
/// with. `expected` is what the model's own output says there should be, and
/// disagreeing with it is an error rather than a guess: a grid that is one
/// level short still decodes, into boxes that are quietly nonsense.
///
/// The input has to be a multiple of the coarsest stride. Otherwise the
/// network's feature maps are rounded *up* where this rounds down, the counts
/// disagree, and the error says so.
fn anchors(width: u32, height: u32, expected: usize) -> Result<Vec<[f32; 4]>, String> {
    let coarsest = RETINA_STRIDES[RETINA_STRIDES.len() - 1];
    if !width.is_multiple_of(coarsest) || !height.is_multiple_of(coarsest) {
        return Err(format!(
            "an anchored head needs an input that is a multiple of {coarsest}, not {width}x{height}"
        ));
    }
    let mut out = Vec::with_capacity(expected);
    for (level, stride) in RETINA_STRIDES.iter().enumerate() {
        let base = base_anchors(RETINA_BASES[level]);
        for y in 0..(height / stride) {
            for x in 0..(width / stride) {
                let (sx, sy) = ((x * stride) as f32, (y * stride) as f32);
                for anchor in base {
                    out.push([
                        anchor[0] + sx,
                        anchor[1] + sy,
                        anchor[2] + sx,
                        anchor[3] + sy,
                    ]);
                }
            }
        }
    }
    if out.len() != expected {
        return Err(format!(
            "the model has {expected} anchors, an anchored head on {width}x{height} has {} — \
             check `input` in ai.ron against the model's own",
            out.len()
        ));
    }
    Ok(out)
}

/// Logits and offsets into boxes in the input's own pixels.
fn decode_retina(
    logits: &[f32],
    offsets: &[f32],
    input: (u32, u32),
    classes: usize,
    floor: f32,
) -> Result<Vec<Detection>, String> {
    let classes = classes.max(1);
    let count = logits.len() / classes;
    if offsets.len() != count * 4 {
        return Err(format!(
            "{count} anchors of logits against {} of offsets",
            offsets.len() / 4
        ));
    }
    let grid = anchors(input.0, input.1, count)?;
    // The threshold, moved to the other side of the sigmoid: comparing logits
    // is one subtraction where the sigmoid is an exponential, and all but a
    // handful of a hundred thousand anchors are rejected here.
    let cut = if floor <= 0.0 {
        f32::NEG_INFINITY
    } else if floor >= 1.0 {
        f32::INFINITY
    } else {
        (floor / (1.0 - floor)).ln()
    };

    let mut found = Vec::new();
    for (index, anchor) in grid.iter().enumerate() {
        let mut best = (0usize, f32::NEG_INFINITY);
        for class in 0..classes {
            let logit = logits[index * classes + class];
            if logit > best.1 {
                best = (class, logit);
            }
        }
        if best.1 < cut {
            continue;
        }
        let (width, height) = (anchor[2] - anchor[0], anchor[3] - anchor[1]);
        let (cx, cy) = (anchor[0] + width / 2.0, anchor[1] + height / 2.0);
        let offset = &offsets[index * 4..index * 4 + 4];
        found.push(Detection {
            class: best.0,
            score: 1.0 / (1.0 + (-best.1).exp()),
            cx: offset[0] * width + cx,
            cy: offset[1] * height + cy,
            w: offset[2].min(RETINA_CLIP).exp() * width,
            h: offset[3].min(RETINA_CLIP).exp() * height,
            angle: 0.0,
        });
    }
    Ok(found)
}

/// Turns the output tensor into boxes — the Ultralytics layouts.
///
/// Split out from the runtime so it can be tested without a model file — the
/// decoding is where the mistakes are, and a mistake here places every car in
/// the module in the wrong spot rather than failing.
fn decode(
    values: impl Iterator<Item = f32>,
    shape: &[usize],
    head: Head,
    classes: usize,
    floor: f32,
) -> Result<Vec<Detection>, String> {
    let rows = head.box_rows() + classes + head.tail_rows();
    let values: Vec<f32> = values.collect();
    // `[1, rows, anchors]` or `[1, anchors, rows]` — whichever axis is as long
    // as the layout says it should be.
    let (anchors, transposed) = match shape {
        [1, a, b] if *a == rows => (*b, false),
        [1, a, b] if *b == rows => (*a, true),
        [a, b] if *a == rows => (*b, false),
        [a, b] if *b == rows => (*a, true),
        _ => {
            return Err(format!(
                "output shape {shape:?} does not fit {rows} rows — check `classes` and `head` in ai.ron"
            ));
        }
    };
    let at = |row: usize, anchor: usize| -> f32 {
        let index = if transposed {
            anchor * rows + row
        } else {
            row * anchors + anchor
        };
        values.get(index).copied().unwrap_or(0.0)
    };

    let mut found = Vec::new();
    for anchor in 0..anchors {
        let mut best = (0usize, 0f32);
        for class in 0..classes {
            let score = at(head.box_rows() + class, anchor);
            if score > best.1 {
                best = (class, score);
            }
        }
        if best.1 < floor {
            continue;
        }
        let angle = match head {
            Head::Oriented { .. } => at(head.box_rows() + classes, anchor),
            // An anchored head never reaches here — it is decoded against its
            // own grid — and it has no angle in any case.
            Head::Boxes { .. } | Head::Retina { .. } => 0.0,
        };
        found.push(Detection {
            class: best.0,
            score: best.1,
            cx: at(0, anchor),
            cy: at(1, anchor),
            w: at(2, anchor),
            h: at(3, anchor),
            angle,
        });
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    /// The anchor grid against torchvision's own, at the size the shipped
    /// tree model runs at.
    ///
    /// Every number here was printed by
    /// `torchvision.models.detection.anchor_utils.AnchorGenerator` on the
    /// model that ships (see `tools/vision/export_models.py`). It is the one
    /// place in this crate that reproduces somebody else's arithmetic instead
    /// of reading their output, and an anchor a pixel out is a crown a metre
    /// and a half from where it stands — silently, with no error anywhere.
    #[test]
    fn the_anchor_grid_is_torchvisions() {
        let grid = anchors(768, 768, 110_484).expect("the grid the model was exported for");
        assert_eq!(grid.len(), 110_484);
        assert_eq!(
            &grid[..5],
            &[
                [-23.0, -11.0, 23.0, 11.0],
                [-28.0, -14.0, 28.0, 14.0],
                [-35.0, -18.0, 35.0, 18.0],
                [-16.0, -16.0, 16.0, 16.0],
                [-20.0, -20.0, 20.0, 20.0],
            ]
        );
        // The second level begins after the first level's 96 × 96 cells, and
        // its middle anchor is the one the rounding turns on: 101 wide, half
        // of it 50.5, and torchvision makes that 50.
        let level1 = 96 * 96 * 9;
        assert_eq!(
            &grid[level1..level1 + 9],
            &[
                [-45.0, -23.0, 45.0, 23.0],
                [-57.0, -28.0, 57.0, 28.0],
                [-71.0, -36.0, 71.0, 36.0],
                [-32.0, -32.0, 32.0, 32.0],
                [-40.0, -40.0, 40.0, 40.0],
                [-50.0, -50.0, 50.0, 50.0],
                [-23.0, -45.0, 23.0, 45.0],
                [-28.0, -57.0, 28.0, 57.0],
                [-36.0, -71.0, 36.0, 71.0],
            ]
        );
        // One from the middle of the second level, and the last five — which
        // are only right if every stride, every grid and the order of the
        // levels are.
        assert_eq!(grid[50_000], [639.0, 431.0, 689.0, 481.0]);
        assert_eq!(
            &grid[grid.len() - 5..],
            &[
                [318.0, 318.0, 962.0, 962.0],
                [234.0, 234.0, 1046.0, 1046.0],
                [459.0, 278.0, 821.0, 1002.0],
                [412.0, 184.0, 868.0, 1096.0],
                [353.0, 66.0, 927.0, 1214.0],
            ]
        );
        // And a fingerprint over all of them, to the precision a float sum of
        // this size has.
        let sum: f64 = grid.iter().flatten().map(|v| *v as f64).sum();
        assert!(
            (sum - 167_132_160.0).abs() < 200.0,
            "{sum} against torchvision's 167132160"
        );
    }

    #[test]
    fn an_input_the_grid_cannot_be_built_for_is_refused() {
        // Not a multiple of the coarsest stride: the network's own feature
        // maps round up where this rounds down.
        let err = anchors(800, 800, 120_087).unwrap_err();
        assert!(err.contains("multiple of 128"), "{err}");
        // The right size, the wrong count — a model with more levels, or an
        // `input` in `ai.ron` that is not the one it was exported for.
        let err = anchors(768, 768, 99).unwrap_err();
        assert!(err.contains("99 anchors"), "{err}");
    }

    /// Half to even, where Rust rounds half away from zero.
    #[test]
    fn the_rounding_is_pythons() {
        assert_eq!(round_half_even(50.5), 50.0);
        assert_eq!(round_half_even(51.5), 52.0);
        assert_eq!(round_half_even(-50.5), -50.0);
        assert_eq!(round_half_even(22.6), 23.0);
        assert_eq!(round_half_even(11.3), 11.0);
    }

    /// An offset of nothing leaves the box on its anchor; the score comes
    /// through a sigmoid; and everything under the floor is gone before a box
    /// is built at all.
    #[test]
    fn an_anchored_box_is_its_anchor_plus_the_offset() {
        // One class, and the first anchor of a 128 × 128 input: 46 by 22 at
        // the origin.
        let cells = 16 * 16 + 8 * 8 + 4 * 4 + 2 * 2 + 1;
        let grid = anchors(128, 128, cells * 9).unwrap();
        let anchors_n = grid.len();
        let mut logits = vec![-20.0f32; anchors_n];
        let mut offsets = vec![0.0f32; anchors_n * 4];
        logits[0] = 0.0; // sigmoid 0.5
        let found = decode_retina(&logits, &offsets, (128, 128), 1, 0.25).unwrap();
        assert_eq!(found.len(), 1, "everything else is below the floor");
        let box0 = found[0];
        assert!((box0.score - 0.5).abs() < 1e-6);
        assert!((box0.cx - 0.0).abs() < 1e-6 && (box0.cy - 0.0).abs() < 1e-6);
        assert!((box0.w - 46.0).abs() < 1e-4 && (box0.h - 22.0).abs() < 1e-4);

        // And an offset moves it by a share of the anchor's own size.
        offsets[0] = 0.5; // half an anchor width east
        offsets[2] = std::f32::consts::LN_2; // twice as wide
        let found = decode_retina(&logits, &offsets, (128, 128), 1, 0.25).unwrap();
        assert!((found[0].cx - 23.0).abs() < 1e-4, "{}", found[0].cx);
        assert!((found[0].w - 92.0).abs() < 1e-3, "{}", found[0].w);
    }

    use super::*;

    /// Three anchors of an oriented head with two classes: rows are
    /// cx, cy, w, h, score0, score1, angle.
    fn oriented() -> (Vec<f32>, Vec<usize>) {
        let rows: Vec<Vec<f32>> = vec![
            vec![10.0, 20.0, 30.0], // cx
            vec![11.0, 21.0, 31.0], // cy
            vec![16.0, 17.0, 18.0], // w
            vec![6.0, 7.0, 8.0],    // h
            vec![0.9, 0.1, 0.05],   // class 0
            vec![0.2, 0.8, 0.01],   // class 1
            vec![0.0, 0.5, 1.0],    // angle
        ];
        let flat: Vec<f32> = rows.iter().flatten().copied().collect();
        (flat, vec![1, 7, 3])
    }

    #[test]
    fn an_oriented_head_is_read_row_by_row() {
        let (values, shape) = oriented();
        let found = decode(
            values.into_iter(),
            &shape,
            Head::Oriented {
                confidence: 0.25,
                iou: 0.45,
            },
            2,
            0.25,
        )
        .unwrap();
        assert_eq!(found.len(), 2, "the third anchor is under the floor");
        assert_eq!(found[0].class, 0);
        assert!((found[0].score - 0.9).abs() < 1e-6);
        assert!((found[0].cx - 10.0).abs() < 1e-6);
        assert!((found[0].w - 16.0).abs() < 1e-6);
        assert!((found[0].angle - 0.0).abs() < 1e-6);
        assert_eq!(found[1].class, 1);
        assert!((found[1].angle - 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_transposed_export_reads_the_same() {
        let (values, shape) = oriented();
        let (rows, anchors) = (shape[1], shape[2]);
        let mut transposed = vec![0f32; values.len()];
        for r in 0..rows {
            for a in 0..anchors {
                transposed[a * rows + r] = values[r * anchors + a];
            }
        }
        let head = Head::Oriented {
            confidence: 0.25,
            iou: 0.45,
        };
        let straight = decode(values.into_iter(), &shape, head, 2, 0.25).unwrap();
        let other = decode(transposed.into_iter(), &[1, anchors, rows], head, 2, 0.25).unwrap();
        assert_eq!(straight, other);
    }

    #[test]
    fn a_plain_head_has_no_angle_row() {
        // Four box rows and two classes, no tail.
        let values: Vec<f32> = vec![
            5.0, 6.0, // cx
            7.0, 8.0, // cy
            9.0, 9.0, // w
            3.0, 3.0, // h
            0.8, 0.1, // class 0
            0.1, 0.7, // class 1
        ];
        let found = decode(
            values.into_iter(),
            &[1, 6, 2],
            Head::Boxes {
                confidence: 0.3,
                iou: 0.45,
            },
            2,
            0.3,
        )
        .unwrap();
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|d| d.angle == 0.0));
    }

    #[test]
    fn a_class_list_that_does_not_fit_the_output_says_so() {
        let (values, shape) = oriented();
        let error = decode(
            values.into_iter(),
            &shape,
            Head::Oriented {
                confidence: 0.25,
                iou: 0.45,
            },
            // Fifteen classes against a seven-row output.
            15,
            0.25,
        )
        .unwrap_err();
        assert!(error.contains("ai.ron"), "{error}");
    }

    #[test]
    fn the_input_tensor_is_normalised_and_laid_out_as_asked() {
        let input = InputSpec {
            width: 2,
            height: 1,
            layout: Layout::Nchw,
            order: ChannelOrder::Rgb,
            scale: 1.0 / 255.0,
            mean: [0.0; 3],
            std: [1.0; 3],
        };
        // Two pixels: pure red, pure blue.
        let pixels = [255u8, 0, 0, 0, 0, 255];
        let tensor = tensor_from(&pixels, &input).unwrap();
        let view = tensor.to_plain_array_view::<f32>().unwrap();
        assert_eq!(view.shape(), &[1, 3, 1, 2]);
        let data: Vec<f32> = view.iter().copied().collect();
        // Plane R, then G, then B — each two pixels wide.
        assert_eq!(data, vec![1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn bgr_swaps_the_channels_and_nhwc_interleaves_them() {
        let input = InputSpec {
            width: 1,
            height: 1,
            layout: Layout::Nhwc,
            order: ChannelOrder::Bgr,
            scale: 1.0,
            mean: [0.0; 3],
            std: [1.0; 3],
        };
        let tensor = tensor_from(&[10, 20, 30], &input).unwrap();
        let view = tensor.to_plain_array_view::<f32>().unwrap();
        assert_eq!(view.shape(), &[1, 1, 1, 3]);
        assert_eq!(
            view.iter().copied().collect::<Vec<f32>>(),
            vec![30.0, 20.0, 10.0]
        );
    }
}
