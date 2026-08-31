//! Track geometry, topology and trackside equipment (plan ch. 5).
//!
//! Everything runs on arc length `s` along edges; the geometry is only resolved to
//! ECEF when evaluated.

pub mod device;
pub mod geometry;
pub mod network;
pub mod oberbau;
pub mod position;
pub mod power;
pub mod profile;
pub mod track_object;
pub mod track_type;

pub use device::{DeviceId, DeviceKind, Facing, PlatformPayload, TracksideDevice};
pub use geometry::Segment;
pub use network::{
    Blocked, EdgeEnd, EdgeId, EdgeSide, NodeId, NodeKind, Switch, SwitchPosition, TrackEdge,
    TrackNetwork, TrackNode, TrackPose,
};
pub use oberbau::{
    Fastening, GAUGE, GAUGE_MEASURE, Oberbau, RAIL_CANT, REGEL_PLANUM, RailPoint, RailProfile,
    RailSection, SleeperKind,
};
pub use position::{AdvanceError, PassedDevice, TrackPosition};
pub use power::{Electrification, PowerSystem, electrification_from_id, electrification_id};
pub use profile::StepProfile;
pub use track_object::TrackObject;
pub use track_type::TrackType;
