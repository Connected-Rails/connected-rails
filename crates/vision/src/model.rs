//! The model registry: what a model is, as a file the user can edit.
//!
//! `ai.ron` sits next to `imagery.ron` and is written on first start, exactly
//! like the imagery configuration. Two of its entries — the car detector and
//! the tree crown detector — name weights that **ship with the game**, in
//! `models/`, so that the feature works on a fresh clone with nothing fetched.
//! The rest are descriptions of models a user may bring, and for those the
//! entry says where the `.onnx` file is expected and the editor says plainly
//! when it is not there yet ([`ModelSpec::missing`]).
//!
//! What ships is still a *description* plus a file, never a model built into
//! the binary: swapping either detector is editing this file and dropping an
//! `.onnx` beside it. The two that ship are converted from published
//! pre-trained detectors by `tools/vision/export_models.py`, and they do not
//! share a licence — `models/LICENSES.md` is the record.
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
    /// An anchored dense head — torchvision's RetinaNet, which is what the
    /// tree crown model is. Two tensors, `[1, anchors, classes]` of logits
    /// and `[1, anchors, 4]` of offsets, and **no boxes in them at all**: an
    /// offset is measured from the anchor it belongs to, and the anchors are
    /// not in the file. They are rebuilt from the input size, which is why
    /// this head exists as a variant rather than as another shape the
    /// Ultralytics decoder copes with.
    ///
    /// It is here because it is what the shipped tree detector needs, and
    /// that in turn because the only crown models worth having are published
    /// this way — the whole DeepForest family is torchvision underneath.
    Retina { confidence: f32, iou: f32 },
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
            Head::Boxes { confidence, .. }
            | Head::Oriented { confidence, .. }
            | Head::Retina { confidence, .. } => confidence,
        }
    }

    pub fn iou(self) -> f32 {
        match self {
            Head::Boxes { iou, .. } | Head::Oriented { iou, .. } | Head::Retina { iou, .. } => iou,
        }
    }

    /// Rows of the output tensor that come before the class scores — for the
    /// two Ultralytics layouts, which are the ones read row by row.
    pub fn box_rows(self) -> usize {
        match self {
            Head::Boxes { .. } | Head::Oriented { .. } | Head::Retina { .. } => 4,
        }
    }

    /// Rows after the class scores.
    pub fn tail_rows(self) -> usize {
        match self {
            Head::Boxes { .. } | Head::Retina { .. } => 0,
            Head::Oriented { .. } => 1,
        }
    }
}

/// What a find of a class becomes on the module.
///
/// The two are different lists in the line file and different things to a
/// builder. An object is placed against the track and carries a heading — a
/// car points somewhere. A tree stands on the ground, points nowhere, and
/// carries a size instead: what a photograph says about a tree is how wide its
/// crown is, and that is the number that decides which of the installed trees
/// is planted and how big it is grown.
///
/// A model says which of the two each of its classes is, so nothing in the
/// pipeline below the registry has to know what a tree is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Placement {
    /// Scenery placed against the track — `ObjectSource` in the line file.
    #[default]
    Object,
    /// A tree in the line's own tree list, grown to the crown that was found —
    /// `TreeSource`. Selectable and deletable one by one afterwards like any
    /// other tree, whether it was planted by hand, by the forest brush or by
    /// this.
    Tree,
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
    /// What a find of this class becomes — see [`Placement`].
    #[serde(default)]
    pub kind: Placement,
    /// Tag used instead of `place` where the crown reads as needle-leaf
    /// ([`crate::canopy`]). Empty leaves `place` standing for every find,
    /// which is what a model with species classes of its own wants — there the
    /// model has already said what the tree is, and a guess from the pixels
    /// would only overrule it.
    #[serde(default)]
    pub conifer: String,
    /// Sizes accepted [m] on the long axis, where the factor of two around
    /// `size` is the wrong rule.
    ///
    /// Trees need it and cars do not. Every car is within a factor of two of
    /// every other car; a crown is anything from a three-metre thorn at the
    /// fence to a twenty-five-metre oak in the station forecourt, and both are
    /// right. Stating the range outright is honest about a class where the
    /// size is not a check on the detection but the point of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<(f64, f64)>,
}

impl ClassSpec {
    /// A class the editor does nothing with.
    pub fn ignored(name: &str) -> Self {
        Self {
            name: name.into(),
            place: String::new(),
            size: (0.0, 0.0),
            confidence: None,
            kind: Placement::Object,
            conifer: String::new(),
            span: None,
        }
    }

    pub fn placed(name: &str, place: &str, size: (f64, f64)) -> Self {
        Self {
            name: name.into(),
            place: place.into(),
            size,
            confidence: None,
            kind: Placement::Object,
            conifer: String::new(),
            span: None,
        }
    }

    /// A class whose finds are planted: the tag for a broadleaf, the tag for a
    /// needle-leaf where the crown reads as one, and the crown diameters the
    /// class covers [m].
    ///
    /// `conifer` may be empty, and is for the single-class crown detectors
    /// that most published tree models are: they say *tree* and nothing more,
    /// and the split has to come from the pixels or not at all.
    pub fn tree(name: &str, place: &str, conifer: &str, span: (f64, f64)) -> Self {
        Self {
            name: name.into(),
            place: place.into(),
            // The middle of the range is what an average crown of the class
            // is, which is all `size` is ever asked for once `span` is set.
            size: ((span.0 + span.1) / 2.0, (span.0 + span.1) / 2.0),
            confidence: None,
            kind: Placement::Tree,
            conifer: conifer.into(),
            span: Some(span),
        }
    }

    /// Whether a detection of this size is plausible for the class.
    ///
    /// Measured on the long axis only. The short axis of an oriented box is
    /// the least reliable number a detector produces — two cars side by side
    /// in adjacent bays bleed into each other — and rejecting on it would
    /// throw away half a full car park.
    pub fn plausible(&self, length: f64) -> bool {
        if let Some((low, high)) = self.span {
            return length >= low && length <= high;
        }
        if self.size.0 <= 0.0 {
            return true;
        }
        length >= self.size.0 * 0.5 && length <= self.size.0 * 2.0
    }

    /// Whether finds of this class are planted rather than placed.
    pub fn is_tree(&self) -> bool {
        self.kind == Placement::Tree
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
/// **Two of them ship with the game** — the car detector and the tree crown
/// detector, in `models/` — so that the feature works on a fresh clone with
/// nothing fetched and nothing signed up to. They are converted from published
/// pre-trained detectors by `tools/vision/export_models.py`, and what they
/// cost in licence terms is written down in `models/LICENSES.md`: the car one
/// is AGPL-3.0 and the tree one MIT.
///
/// The rest are descriptions without weights, for a user bringing their own
/// detector. What ships for those is the part that is tedious to get right and
/// silently wrong when it is not — the input size, the head, the class list in
/// the model's own order.
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
            note: "Ships with the game. Ultralytics yolov8n-obb on DOTAv1, \
                   AGPL-3.0 — see models/LICENSES.md. Rebuilt by \
                   tools/vision/export_models.py --only cars"
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
        // The tree detector that ships. DeepForest is the crown model of the
        // field — a torchvision RetinaNet trained on the NEON airborne survey,
        // MIT licensed, and the reason [`Head::Retina`] exists at all.
        //
        // Only the backbone and the head are in the file: the resize, the
        // normalisation and the suppression that the Python package does
        // around them are this crate's own work already, and what it cannot do
        // is guess anchors. Those are rebuilt from the input size.
        ModelSpec {
            id: "deepforest".into(),
            name: "DeepForest (Baumkronen)".into(),
            file: "models/deepforest-tree.onnx".into(),
            input: InputSpec {
                // A multiple of the coarsest feature stride, so the anchor
                // grid follows from the size by division. DeepForest's own
                // 800 is not, and its coarsest map is a cell wider than the
                // division would say.
                width: 768,
                height: 768,
                layout: Layout::Nchw,
                order: ChannelOrder::Rgb,
                scale: 1.0 / 255.0,
                // ImageNet, which is what the backbone was trained under and
                // what DeepForest's own transform subtracts.
                mean: [0.485, 0.456, 0.406],
                std: [0.229, 0.224, 0.225],
            },
            head: Head::Retina {
                confidence: 0.3,
                // Crowns touch and overlap in a closed wood, so the box that
                // survives has to be allowed to sit close to its neighbour.
                // DeepForest's own default is 0.05, which is stricter still.
                iou: 0.1,
            },
            classes: vec![ClassSpec::tree(
                "Tree",
                "laubbaum",
                "nadelbaum",
                (2.5, 26.0),
            )],
            // Five centimetres, which is *not* the resolution of the survey it
            // was trained on: DeepForest reads ten-centimetre imagery and
            // doubles it before the network sees anything, so five is the
            // scale a crown has to arrive at to be the number of pixels across
            // the model expects. Where the provider cannot go that fine the
            // window is enlarged instead, and the crowns come out softer —
            // finer imagery is the single biggest thing that helps this model.
            ground_sample: 0.05,
            overlap: 0.3,
            note: "Ships with the game. DeepForest (Weecology, MIT) trained on \
                   the NEON airborne survey; backbone and head only, anchors \
                   rebuilt by the editor. Rebuilt by \
                   tools/vision/export_models.py --only trees"
                .into(),
        },
        // Tree crowns. Every published crown detector is single-class — it
        // says *tree* and nothing else, because that is what the aerial
        // training sets are labelled with. What kind of tree it is comes from
        // the crown itself (`canopy`), and how big it is from the box, which
        // is the whole of what a photograph can say about a tree.
        ModelSpec {
            id: "tree-crowns".into(),
            name: "YOLO (Baumkronen)".into(),
            file: "models/tree-crowns.onnx".into(),
            input: InputSpec {
                width: 640,
                height: 640,
                ..Default::default()
            },
            head: Head::Boxes {
                // Higher than the cars want. A wood is thousands of crowns
                // and a false one is a tree in the middle of a meadow, which
                // is more conspicuous than a car park with a gap in it.
                confidence: 0.35,
                iou: 0.4,
            },
            classes: vec![ClassSpec::tree(
                "tree",
                "laubbaum",
                "nadelbaum",
                (2.5, 26.0),
            )],
            // Ten centimetres is what the aerial crown sets are labelled at,
            // and a crown is a much bigger thing than a car — the resolution
            // is spent on telling two touching crowns apart.
            ground_sample: 0.1,
            // More than the cars: crowns of a closed wood run into each other
            // across a window edge far more often than parked cars do.
            overlap: 0.3,
            note: "Any single-stage crown detector exported from Ultralytics — \
                   train one on an aerial crown set (NEON/DeepForest labels, \
                   Zenodo urban tree crowns). Export: yolo export model=crowns.pt \
                   format=onnx imgsz=640"
                .into(),
        },
        // The same thing from a model that was taught the difference. Where
        // it exists it is the better answer, and the entry is here to say so:
        // `conifer` stays empty, and the pixel guess is never consulted.
        ModelSpec {
            id: "tree-species".into(),
            name: "YOLO (Baumarten)".into(),
            file: "models/tree-species.onnx".into(),
            input: InputSpec {
                width: 640,
                height: 640,
                ..Default::default()
            },
            head: Head::Boxes {
                confidence: 0.35,
                iou: 0.4,
            },
            classes: vec![
                ClassSpec::tree("broadleaf", "laubbaum", "", (2.5, 26.0)),
                ClassSpec::tree("conifer", "nadelbaum", "", (2.0, 22.0)),
                // A hedge or a thicket: low, wide, and never a standard tree.
                ClassSpec::tree("shrub", "strauch", "", (1.0, 6.0)),
            ],
            ground_sample: 0.1,
            overlap: 0.3,
            note: "A crown detector with species classes of its own; adjust \
                   `classes` to the model's order and the tags of the tree mods \
                   you have installed."
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

    /// The registry is a file the user has been editing since before there
    /// were trees in it, and one written then has to keep working: every field
    /// the tree classes added defaults to what a car class always meant.
    #[test]
    fn a_class_written_before_the_trees_still_reads() {
        let old: ClassSpec =
            ron::from_str(r#"(name: "small vehicle", place: "car", size: (4.4, 1.8))"#).unwrap();
        assert_eq!(old.kind, Placement::Object);
        assert!(old.conifer.is_empty());
        assert_eq!(old.span, None);
        assert!(!old.is_tree());
        assert!(old.plausible(4.0));
    }

    #[test]
    fn a_tree_class_says_it_is_one_and_survives_the_file() {
        let config = VisionConfig::default();
        let crowns = config.model_by_id("tree-crowns").unwrap();
        assert_eq!(crowns.classes.len(), 1);
        let tree = &crowns.classes[0];
        assert!(tree.is_tree());
        assert_eq!(tree.place, "laubbaum");
        assert_eq!(
            tree.conifer, "nadelbaum",
            "the split has to come from the pixels"
        );

        // A model with species of its own never consults them.
        let species = config.model_by_id("tree-species").unwrap();
        assert_eq!(species.placing().count(), 3);
        assert!(species.classes.iter().all(|c| c.conifer.is_empty()));
        assert!(species.classes.iter().all(|c| c.is_tree()));

        let text = ron::ser::to_string_pretty(&config, ron::ser::PrettyConfig::default()).unwrap();
        assert_eq!(ron::from_str::<VisionConfig>(&text).unwrap(), config);
    }

    /// The whole reason `span` exists: a crown of three metres and a crown of
    /// twenty-four are both trees, and the factor of two around one size would
    /// have thrown away whichever end it was not centred on.
    #[test]
    fn a_stated_span_replaces_the_factor_of_two() {
        let tree = ClassSpec::tree("tree", "laubbaum", "nadelbaum", (2.5, 26.0));
        assert!(tree.plausible(2.5) && tree.plausible(26.0));
        assert!(tree.plausible(3.0), "a thorn at the fence");
        assert!(tree.plausible(24.0), "and an oak in the forecourt");
        assert!(!tree.plausible(2.0), "below it, the shadow of a bush");
        assert!(
            !tree.plausible(40.0),
            "and above it, half a wood the model ran together"
        );
        // Without a span the old rule still holds.
        let car = ClassSpec::placed("small vehicle", "car", (4.4, 1.8));
        assert!(car.plausible(4.4) && !car.plausible(11.0));
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
