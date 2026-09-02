//! Reading the aerial imagery with a local model: the registry, the runtime,
//! and what comes out the other end.
//!
//! The route editor already drapes a photograph of the real place over the
//! ground ([`imagery`]). Everything a builder then does by hand is in that
//! photograph — the cars in the station car park, the lorries at the goods
//! shed, the hedges along the fields. This crate is the machinery that lets a
//! model say where those things are, so the editor can place them.
//!
//! Three rules shape it, and they are the reason for every type here:
//!
//! 1. **The model is data, not code.** A model is one entry in `ai.ron`
//!    ([`ModelSpec`]): the file, the input it wants, the head it has, and what
//!    each of its classes is worth on a module. Adding a detector for level
//!    crossings, containers or solar farms is an entry and a mod with objects
//!    tagged for it — no Rust. That is what makes this extensible rather than
//!    a one-off car finder.
//! 2. **It runs locally.** [`onnx`] is a pure-Rust ONNX runtime in process:
//!    nothing leaves the machine, nothing is downloaded at build time, and a
//!    module can be built on a train with no signal.
//! 3. **It never looks at more than it was asked to.** The work is driven by
//!    a [`Region`] — a corridor along the track or an area drawn in the
//!    viewport. Windows outside it are never fetched and never inferred, which
//!    is the difference between forty seconds and forty minutes.
//!
//! The pipeline, in the order the modules read:
//!
//! ```text
//! Region ──▶ sheet::Sheet ──▶ detect::run ──▶ Vec<GeoDetection> ──▶ parking::lots
//!            (tiles, lazily)   (windows,          (metres,             (clusters,
//!                               NMS)               headings)            rectangles)
//!                                   │                    │
//!                              canopy::tag_for      Placement::Tree
//!                              (fir or lime)        (crown, not heading)
//! ```
//!
//! A find is one of two things ([`Placement`]), and the difference runs
//! through the whole crate. An **object** — a car, a lorry — is placed against
//! the track and what matters about it is which way it points. A **tree** is
//! planted on the ground and points nowhere; what matters about it is how wide
//! its crown is, because that is what decides which of the installed trees
//! goes there and how big it is grown. Both come out of the same walk over the
//! same imagery, and which one a class is, is one word in `ai.ron`.
//!
//! What this crate does *not* do is decide what a detection becomes. A car in
//! a photograph is a `GeoDetection` with a role of `"car"`; which model from
//! which mod is placed there, at what distance from the rails, and whether it
//! goes into the line file at all is the editor's business
//! (`route-editor/src/ai.rs`).

pub mod canopy;
pub mod detect;
pub mod model;
#[cfg(feature = "onnx")]
pub mod onnx;
pub mod parking;
pub mod region;
pub mod sheet;

pub use canopy::Crown;
pub use detect::{Detection, Detector, GeoDetection, Outcome, Progress, run};
pub use model::{ClassSpec, Head, InputSpec, Layout, ModelSpec, Placement, VisionConfig};
pub use parking::{Lot, lots};
pub use region::{Region, Shape};
pub use sheet::Sheet;

/// Backend of the loaded model, for the message the editor shows when a model
/// cannot be run — a missing file and a build without the runtime are
/// different problems with different remedies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// ONNX in process ([`onnx`]).
    Onnx,
    /// The crate was built without the inference runtime.
    Missing,
}

/// Loads a model, whichever runtime this build has.
///
/// The editor asks for a detector and gets one; which runtime answers is this
/// crate's business, and a build without one says so in the same place a
/// missing weights file would.
pub fn load_detector(
    spec: &ModelSpec,
    path: &std::path::Path,
) -> Result<Box<dyn Detector>, String> {
    #[cfg(feature = "onnx")]
    {
        Ok(Box::new(onnx::OnnxDetector::load(spec, path)?))
    }
    #[cfg(not(feature = "onnx"))]
    {
        let _ = (spec, path);
        Err(i18n::t!("ai-no-backend"))
    }
}

/// Which backends this build has.
pub fn backend() -> Backend {
    #[cfg(feature = "onnx")]
    {
        Backend::Onnx
    }
    #[cfg(not(feature = "onnx"))]
    {
        Backend::Missing
    }
}
