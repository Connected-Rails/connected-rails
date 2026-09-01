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
//! What is decoded here is the Ultralytics export layout, because that is
//! what a user can actually obtain: one tensor, `[1, rows, anchors]`, four
//! rows of box, one row per class, and — for an oriented model — a last row
//! of angle. Both the row-major and the transposed export are read, since
//! which one comes out depends on the exporter's version and getting it
//! wrong is silent nonsense rather than an error.
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

/// Turns the output tensor into boxes.
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
            Head::Boxes { .. } => 0.0,
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
