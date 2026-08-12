//! Gleisgeometrie, Topologie und Streckenausrüstung (Plan Kap. 5).
//!
//! Alles fährt auf Bogenlänge `s` entlang von Kanten; die Geometrie wird erst beim
//! Auswerten nach ECEF aufgelöst.

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
