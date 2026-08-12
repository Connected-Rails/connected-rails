//! Track geometry, topology and trackside equipment (plan ch. 5).
//!
//! Everything runs on arc length `s` along edges; the geometry is only resolved to
//! ECEF when evaluated.

pub mod device;
pub mod geometry;
pub mod network;
pub mod position;
pub mod profile;

pub use device::{DeviceId, DeviceKind, Facing, TracksideDevice};
pub use geometry::Segment;
pub use network::{
    Blocked, EdgeEnd, EdgeId, EdgeSide, NodeId, NodeKind, Switch, SwitchPosition, TrackEdge,
    TrackNetwork, TrackNode, TrackPose,
};
pub use position::{AdvanceError, PassedDevice, TrackPosition};
pub use profile::StepProfile;
