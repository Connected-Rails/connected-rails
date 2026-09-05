//! Fields from the agricultural registers: what actually grows beside the line.
//!
//! Every EU member state has to publish what its farmers declared (Art. 67(3)
//! of Regulation (EU) 2021/2116), and the German states do it as web services.
//! That is a map of the countryside with the crop written on each parcel, free
//! to use, and it is the difference between a line that runs past green noise
//! and one that runs past winter wheat, maize and a beet field in October.
//!
//! The data comes at two levels, and which one a state publishes decides how
//! much this crate can do:
//!
//! * **GSA** — the *Schlag*, the parcel as it was applied for, with its crop
//!   code. That is what a passenger perceives as one field, and it is what the
//!   import wants. Lower Saxony, North Rhine-Westphalia, Brandenburg, Saxony
//!   and Thuringia publish it.
//! * **LPIS** — the *Feldblock*, the outer boundary of the farmed land, with
//!   arable/grassland and nothing finer. One block can hold half a dozen crops.
//!   Bavaria, Hesse and the others stop here, and the crop is drawn from the
//!   regional statistics instead ([`stats`]).
//!
//! The flow is [`import::run`]: work out which states the box touches
//! ([`land`]), ask each one's service ([`wfs`]), map its own crop code onto the
//! dozen groups the simulator can draw ([`crops`]), clean the geometry up
//! ([`geometry`]) and hand back [`FieldFeature`]s. Nothing here writes to a
//! line — the editor shows what came back and the user commits it.
//!
//! The crate is where the register clients live, and the field registers are
//! not the only one: [`mastr`] asks the Bundesnetzagentur's
//! Marktstammdatenregister what wind turbine stands at a point, because
//! OpenStreetMap surveys where they stand and the register knows what they are
//! (`content::wind`). [`osm`] is here for the same reason — a fetcher belongs
//! with the other fetchers.
//!
//! No Bevy and no ECS: this is a fetch-and-convert library, the same way
//! [`imagery`](../imagery/index.html) is, and the editor is what hooks it up.

pub mod attribution;
pub mod cache;
pub mod crops;
pub mod geometry;
pub mod import;
pub mod land;
pub mod mastr;
pub mod model;
pub mod osm;
pub mod phenology;
pub mod stats;
pub mod wfs;

pub use attribution::Attribution;
pub use cache::FieldCache;
pub use crops::{CropClass, CropTable};
pub use import::{Area, Clip, ImportOptions, ImportProgress, ImportReport, Stage};
pub use land::Land;
pub use land::{Access, Level as DataLevel, Licence, Service};
pub use mastr::{Status as UnitStatus, WindUnit};
pub use model::{FieldFeature, Level};
pub use phenology::{Growth, Stage as GrowthStage};
pub use wfs::{RequestConfig, ServiceError};
