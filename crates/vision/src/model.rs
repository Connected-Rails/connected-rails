//! The model registry: what a model is, as a file the user can edit.
//!
//! `ai.ron` sits next to `imagery.ron` and is written on first start, exactly
//! like the imagery configuration. It lists models, not weights — the weights
//! are big, and most of them are licensed in a way that forbids shipping them
//! with a game. Each entry says where its `.onnx` file is expected, and the
//! editor says plainly when it is not there yet ([`ModelSpec::missing`]).
//!
//! A [`ModelSpec`] is deliberately mechanical: how the picture goes in
//! ([`InputSpec`]), what shape comes out ([`Head`]), and what each class means
//! on a module ([`ClassSpec`]). Between them they describe every single-stage
//! detector in common use, so the next model is an entry in this file rather
//! than a match arm somewhere in the editor.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Memory layout of the input tensor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Layout {
    /// `[1, 3, h, w]` — what PyTorch exports, and what all of YOLO uses.
    #[default]
    Nchw,
    /// `[1, h, w, 3]` — what TensorFlow exports.
    Nhwc,
}

/// Order of the colour channels the model was trained on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ChannelOrder {
    #[default]
    Rgb,
    /// OpenCV's order — anything trained through `cv2.imread` wants this.
    Bgr,
}

/// How a window of imagery becomes the model's input tensor.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct InputSpec {
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub layout: Layout,
    #[serde(default)]
    pub order: ChannelOrder,
    /// What a byte is multiplied by — `1/255` for the usual 0…1 input.
    #[serde(default = "default_scale")]
    pub scale: f32,
    /// Subtracted after the scaling, per channel. Zero for YOLO; ImageNet
    /// statistics for a backbone that was trained with them.
    #[serde(default)]
    pub mean: [f32; 3],
    /// Divided by after the mean, per channel.
    #[serde(default = "default_std")]
    pub std: [f32; 3],
}

fn default_scale() -> f32 {
    1.0 / 255.0
}

fn default_std() -> [f32; 3] {
    [1.0, 1.0, 1.0]
}

impl Default for InputSpec {
    fn default() -> Self {
        Self {
            width: 1024,
            height: 1024,
            layout: Layout::default(),
            order: ChannelOrder::default(),
            scale: default_scale(),
            mean: [0.0; 3],
            std: default_std(),
        }
    }
}

/// The output head — what the numbers coming out of the model mean.
///
/// Both variants are the Ultralytics export layout, which is what almost
/// every model published as `.onnx` for aerial work is: one tensor
/// `[1, rows, anchors]`, the first four rows the box in input pixels
/// (centre x, centre y, width, height), then one row per class holding the
/// score, and — for an oriented model — one last row holding the angle.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Head {
    /// Axis-aligned boxes: `[1, 4 + classes, anchors]`.
    ///
    /// A car found by such a model has no heading of its own; the editor takes
    /// one from the box's long axis, which is right for a car parked square in
    /// its bay and can be a quarter turn out for one parked at an angle.
    Boxes { confidence: f32, iou: f32 },
    /// Oriented boxes: `[1, 5 + classes, anchors]`, the last row the rotation
    /// [rad]. This is the head worth having for parked cars — it says which
    /// way each one points, and a car park is nothing but that.
    Oriented { confidence: f32, iou: f32 },
}

impl Default for Head {
    fn default() -> Self {
        Head::Oriented {
            confidence: 0.25,
            iou: 0.45,
        }
    }
}

impl Head {
    pub fn confidence(self) -> f32 {
        match self {
            Head::Boxes { confidence, .. } | Head::Oriented { confidence, .. } => confidence,
        }
    }

    pub fn iou(self) -> f32 {
        match self {
            Head::Boxes { iou, .. } | Head::Oriented { iou, .. } => iou,
        }
    }

    /// Rows of the output tensor that come before the class scores.
    pub fn box_rows(self) -> usize {
        match self {
            Head::Boxes { .. } => 4,
            Head::Oriented { .. } => 4,
        }
    }

    /// Rows after the class scores.
    pub fn tail_rows(self) -> usize {
        match self {
            Head::Boxes { .. } => 0,
            Head::Oriented { .. } => 1,
        }
    }
}

/// What one of the model's classes is worth on a module.
///
/// `place` is the hinge of the whole design: it is a **tag**, and the editor
/// places a random object carrying that tag from the installed mods. A model
/// that finds lorries and a mod with lorries in it meet here, and neither
/// knows about the other.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClassSpec {
    /// The class as the model's own list has it, in its own order. Kept
    /// verbatim: it is what a user compares against the model card.
    pub name: String,
    /// Tag of the objects that may be placed for this class; empty means the
    /// class is ignored, which is what most classes of a general model are.
    #[serde(default)]
    pub place: String,
    /// Footprint of the real thing [m], length across width. A detection more
    /// than half again as big or small is dropped — a "car" eleven metres long
    /// is two cars the model ran together, and placing one there is worse than
    /// placing nothing.
    #[serde(default)]
    pub size: (f64, f64),
    /// Confidence this class needs, where it should differ from the head's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
}

impl ClassSpec {
    /// A class the editor does nothing with.
    pub fn ignored(name: &str) -> Self {
        Self {
            name: name.into(),
            place: String::new(),
            size: (0.0, 0.0),
            confidence: None,
        }
    }

    pub fn placed(name: &str, place: &str, size: (f64, f64)) -> Self {
        Self {
            name: name.into(),
            place: place.into(),
            size,
            confidence: None,
        }
    }

    /// Whether a detection of this size is plausible for the class.
    ///
    /// Measured on the long axis only. The short axis of an oriented box is
    /// the least reliable number a detector produces — two cars side by side
    /// in adjacent bays bleed into each other — and rejecting on it would
    /// throw away half a full car park.
    pub fn plausible(&self, length: f64) -> bool {
        if self.size.0 <= 0.0 {
            return true;
        }
        length >= self.size.0 * 0.5 && length <= self.size.0 * 2.0
    }
}

/// One model the editor can run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelSpec {
    /// Stable id, referenced by `active` and remembered in the editor.
    pub id: String,
    /// Name as the picker shows it. Content, not translated — it is the
    /// model's own name, like a provider's.
    pub name: String,
    /// The ONNX file. A relative path resolves next to `ai.ron`, so a
    /// configuration and its `models/` directory move together.
    pub file: PathBuf,
    #[serde(default)]
    pub input: InputSpec,
    #[serde(default)]
    pub head: Head,
    /// The model's classes, in the model's own order.
    pub classes: Vec<ClassSpec>,
    /// Ground sampling distance the model was trained at [m/px]. It decides
    /// which zoom level the imagery is fetched at: a detector trained on
    /// 30 cm imagery finds nothing at all on a 2 m tile, and very little on a
    /// 5 cm one — the cars are the wrong number of pixels across either way.
    #[serde(default = "default_ground_sample")]
    pub ground_sample: f64,
    /// How far two neighbouring windows overlap, as a share of the window.
    /// Anything cut by a window edge is found whole in the neighbour.
    #[serde(default = "default_overlap")]
    pub overlap: f64,
    /// Where the weights come from and under what licence — shown in the
    /// picker, because a user has to fetch them by hand.
    #[serde(default)]
    pub note: String,
}

fn default_ground_sample() -> f64 {
    0.3
}

fn default_overlap() -> f64 {
    0.2
}

impl ModelSpec {
    /// The weights file, resolved against the directory of `ai.ron`.
    pub fn path(&self, config_dir: &Path) -> PathBuf {
        if self.file.is_absolute() {
            self.file.clone()
        } else {
            config_dir.join(&self.file)
        }
    }

    /// Whether the weights are not installed — the one thing the picker has to
    /// say before anything is run.
    pub fn missing(&self, config_dir: &Path) -> bool {
        !self.path(config_dir).is_file()
    }

    /// The classes this model places something for.
    pub fn placing(&self) -> impl Iterator<Item = (usize, &ClassSpec)> {
        self.classes
            .iter()
            .enumerate()
            .filter(|(_, c)| !c.place.is_empty())
    }

    /// Confidence a class has to reach.
    pub fn confidence_of(&self, class: usize) -> f32 {
        self.classes
            .get(class)
            .and_then(|c| c.confidence)
            .unwrap_or_else(|| self.head.confidence())
    }
}

/// The registry as the file has it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VisionConfig {
    /// Id of the model the editor starts on.
    pub active: String,
    pub models: Vec<ModelSpec>,
}

impl Default for VisionConfig {
    fn default() -> Self {
        Self {
            active: "dota-obb".into(),
            models: predefined_models(),
        }
    }
}

impl VisionConfig {
    /// Loads the registry; if the file is missing, the default is written —
    /// the imagery configuration's pattern, so both files appear on first
    /// start and can be edited the same way.
    pub fn load_or_create(path: impl Into<PathBuf>) -> (Self, Option<String>) {
        let path = path.into();
        match std::fs::read_to_string(&path) {
            Ok(text) => match ron::from_str::<Self>(&text) {
                Ok(config) => (config, None),
                Err(e) => (
                    Self::default(),
                    Some(i18n::t!(
                        "status-config-unreadable",
                        file = path.display(),
                        error = e
                    )),
                ),
            },
            Err(_) => {
                let config = Self::default();
                let message = match config.save(&path) {
                    Ok(()) => i18n::t!("status-config-created", file = path.display()),
                    Err(e) => i18n::t!(
                        "status-config-not-writable",
                        file = path.display(),
                        error = e
                    ),
                };
                (config, Some(message))
            }
        }
    }

    pub fn save(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        std::fs::write(path, text)
    }

    /// The active model, or the first one where the id is unknown.
    pub fn model(&self) -> Option<&ModelSpec> {
        self.models
            .iter()
            .find(|m| m.id == self.active)
            .or_else(|| self.models.first())
    }

    pub fn model_by_id(&self, id: &str) -> Option<&ModelSpec> {
        self.models.iter().find(|m| m.id == id)
    }
}

/// The models the editor knows how to talk to out of the box.
///
/// None of them ships with the game: every one is a file the user exports or
/// downloads, and `note` says from where. What ships is the description — the
/// input size, the head, the class list in the right order — because that is
/// the part that is tedious to get right and silently wrong when it is not.
pub fn predefined_models() -> Vec<ModelSpec> {
    vec![
        // The one the whole feature was built for. DOTA is aerial imagery with
        // oriented boxes, and two of its fifteen classes are exactly what a
        // station car park is made of.
        ModelSpec {
            id: "dota-obb".into(),
            name: "YOLOv8 OBB (DOTA v1)".into(),
            file: "models/yolov8n-obb.onnx".into(),
            input: InputSpec {
                width: 1024,
                height: 1024,
                ..Default::default()
            },
            head: Head::Oriented {
                confidence: 0.25,
                iou: 0.45,
            },
            classes: vec![
                ClassSpec::ignored("plane"),
                ClassSpec::ignored("ship"),
                ClassSpec::ignored("storage tank"),
                ClassSpec::ignored("baseball diamond"),
                ClassSpec::ignored("tennis court"),
                ClassSpec::ignored("basketball court"),
                ClassSpec::ignored("ground track field"),
                ClassSpec::ignored("harbor"),
                ClassSpec::ignored("bridge"),
                ClassSpec::placed("large vehicle", "lorry", (9.0, 2.5)),
                ClassSpec::placed("small vehicle", "car", (4.4, 1.8)),
                ClassSpec::ignored("helicopter"),
                ClassSpec::ignored("roundabout"),
                ClassSpec::ignored("soccer ball field"),
                ClassSpec::ignored("swimming pool"),
            ],
            ground_sample: 0.3,
            overlap: 0.2,
            note: "Ultralytics yolov8n-obb, trained on DOTAv1 (AGPL-3.0). \
                   Export: yolo export model=yolov8n-obb.pt format=onnx imgsz=1024"
                .into(),
        },
        // The same family without rotation — for anyone who already has a
        // plain detector trained on their own imagery. The heading then comes
        // from the box, which is worse and still usable.
        ModelSpec {
            id: "vehicles-boxes".into(),
            name: "YOLO (vehicles, axis-aligned)".into(),
            file: "models/vehicles.onnx".into(),
            input: InputSpec {
                width: 640,
                height: 640,
                ..Default::default()
            },
            head: Head::Boxes {
                confidence: 0.3,
                iou: 0.45,
            },
            classes: vec![
                ClassSpec::placed("car", "car", (4.4, 1.8)),
                ClassSpec::placed("truck", "lorry", (9.0, 2.5)),
                ClassSpec::placed("bus", "bus", (12.0, 2.6)),
            ],
            ground_sample: 0.15,
            overlap: 0.2,
            note: "Any single-stage detector exported from Ultralytics with these \
                   three classes; adjust `classes` to the model's own order."
                .into(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_registry_survives_a_round_trip() {
        let config = VisionConfig::default();
        let text = ron::ser::to_string_pretty(&config, ron::ser::PrettyConfig::default()).unwrap();
        let back: VisionConfig = ron::from_str(&text).unwrap();
        assert_eq!(config, back);
    }

    #[test]
    fn the_active_model_is_found_and_falls_back() {
        let mut config = VisionConfig::default();
        assert_eq!(config.model().unwrap().id, "dota-obb");
        config.active = "nothing-like-it".into();
        assert_eq!(
            config.model().unwrap().id,
            "dota-obb",
            "an unknown id falls back to the first entry rather than to nothing"
        );
    }

    #[test]
    fn the_dota_class_list_is_the_models_own_order() {
        let config = VisionConfig::default();
        let dota = config.model_by_id("dota-obb").unwrap();
        assert_eq!(dota.classes.len(), 15);
        assert_eq!(dota.classes[9].name, "large vehicle");
        assert_eq!(dota.classes[10].name, "small vehicle");
        assert_eq!(dota.classes[10].place, "car");
        // Two of fifteen do anything.
        assert_eq!(dota.placing().count(), 2);
    }

    #[test]
    fn a_relative_model_path_resolves_next_to_the_configuration() {
        let spec = &VisionConfig::default().models[0];
        let path = spec.path(Path::new("/tmp/somewhere"));
        assert_eq!(path, Path::new("/tmp/somewhere/models/yolov8n-obb.onnx"));
    }

    #[test]
    fn a_car_twice_the_length_is_not_a_car() {
        let car = ClassSpec::placed("small vehicle", "car", (4.4, 1.8));
        assert!(car.plausible(4.4));
        assert!(car.plausible(3.0));
        assert!(!car.plausible(1.5), "a length of 1.5 m is a shadow");
        assert!(!car.plausible(11.0), "11 m is two cars run together");
        // A class without a stated size accepts whatever comes.
        assert!(ClassSpec::ignored("x").plausible(100.0));
    }

    #[test]
    fn a_class_may_raise_the_confidence_over_the_heads() {
        let mut spec = VisionConfig::default().models[0].clone();
        assert!((spec.confidence_of(10) - 0.25).abs() < 1e-6);
        spec.classes[10].confidence = Some(0.6);
        assert!((spec.confidence_of(10) - 0.6).abs() < 1e-6);
        assert!(
            (spec.confidence_of(99) - 0.25).abs() < 1e-6,
            "a class that does not exist reads as the head's own threshold"
        );
    }
}
