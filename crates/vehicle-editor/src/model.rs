//! glTF import: node inspection, LOD detection, part suggestions (plan ch. 15.2).
//!
//! **Nothing has to be marked in Blender.** The editor lists every node of the file and you
//! bind it; the binding lands in the vehicle RON, not in the model. Two conventions are
//! recognised on top of that, so that a well-prepared file needs no clicking at all:
//!
//! - **Levels of detail:** node name ending in `_LOD0`, `_LOD1`, … (works from Blender
//!   without an add-on — that is just the object name).
//! - **Moving parts:** name prefixes (`door_`, `pant_`, `sw_`, `gauge_`, `lamp_`, `wheel_`)
//!   or a Blender custom property `ts_function` (`ts_motion`, `ts_axis`, `ts_amount`),
//!   which the glTF exporter writes into `extras`.

use bevy::gltf::GltfNode;
use bevy::prelude::*;
use sim_core::train::{Lod, Motion, Part};

/// One node of the imported file, as the editor shows it.
#[derive(Debug, Clone)]
pub struct Node {
    pub name: String,
    /// Level of detail from the name suffix.
    pub lod: Option<u8>,
    /// Function suggested by name prefix or `extras`.
    pub suggestion: Option<Part>,
}

/// Default view distances per LOD level [m] — starting values, editable afterwards.
pub const DEFAULT_LOD_DISTANCES: [f64; 4] = [150.0, 400.0, 1_000.0, 4_000.0];

/// Reads names, LOD level and part suggestion out of the loaded glTF.
pub fn inspect(gltf: &bevy::gltf::Gltf, nodes: &Assets<GltfNode>) -> Vec<Node> {
    let mut list: Vec<Node> = gltf
        .named_nodes
        .iter()
        .map(|(name, handle)| {
            let extras = nodes
                .get(handle)
                .and_then(|n| n.extras.as_ref())
                .map(|e| e.value.as_str());
            Node {
                name: name.to_string(),
                lod: lod_level(name),
                suggestion: suggest(name, extras),
            }
        })
        .collect();
    list.sort_by(|a, b| a.name.cmp(&b.name));
    list
}

pub use sim_core::train::lod_level;

/// The levels of detail present in the file, with default distances.
pub fn detect_lods(nodes: &[Node]) -> Vec<Lod> {
    let mut levels: Vec<u8> = nodes.iter().filter_map(|n| n.lod).collect();
    levels.sort_unstable();
    levels.dedup();
    levels
        .into_iter()
        .map(|level| Lod {
            level,
            distance: DEFAULT_LOD_DISTANCES
                .get(level as usize)
                .copied()
                .unwrap_or(4_000.0),
        })
        .collect()
}

/// Function and motion of a node, from `extras` if present, otherwise from the name.
fn suggest(name: &str, extras: Option<&str>) -> Option<Part> {
    if let Some(part) = from_extras(name, extras?) {
        return Some(part);
    }
    from_name(name)
}

/// Blender custom properties end up in glTF `extras`:
/// `ts_function` (required), `ts_motion` (`rotate`/`translate`/`visibility`/`emissive`),
/// `ts_axis` (`"0 0 1"`), `ts_amount` (degrees or metres).
fn from_extras(name: &str, extras: &str) -> Option<Part> {
    let value: serde_json::Value = serde_json::from_str(extras).ok()?;
    let function = value.get("ts_function")?.as_str()?.to_string();
    let amount = value
        .get("ts_amount")
        .and_then(|a| a.as_f64())
        .unwrap_or(90.0) as f32;
    let axis = value
        .get("ts_axis")
        .and_then(|a| a.as_str())
        .and_then(parse_axis)
        .unwrap_or([0.0, 0.0, 1.0]);
    let motion = match value.get("ts_motion").and_then(|m| m.as_str()) {
        Some("rotate") => Motion::Rotate {
            axis,
            degrees: amount,
        },
        Some("translate") => Motion::Translate {
            axis,
            metres: amount,
        },
        Some("emissive") => Motion::Emissive,
        _ => Motion::Visibility,
    };
    Some(Part {
        node: name.to_string(),
        function,
        motion,
    })
}

fn parse_axis(text: &str) -> Option<[f32; 3]> {
    let mut values = text
        .split([' ', ',', ';'])
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok());
    Some([values.next()?, values.next()?, values.next()?])
}

/// Name prefixes as the zero-configuration fallback.
fn from_name(name: &str) -> Option<Part> {
    let lower = name.to_lowercase();
    let (function, motion) = if let Some(rest) = lower.strip_prefix("door_") {
        (
            format!("door_{rest}"),
            Motion::Translate {
                axis: [0.0, 0.0, 1.0],
                metres: 0.8,
            },
        )
    } else if lower.starts_with("pant_") || lower.starts_with("pantograph") {
        (
            "pantograph".to_string(),
            Motion::Rotate {
                axis: [1.0, 0.0, 0.0],
                degrees: 45.0,
            },
        )
    } else if let Some(rest) = lower.strip_prefix("sw_") {
        (
            format!("switch:{rest}"),
            Motion::Rotate {
                axis: [1.0, 0.0, 0.0],
                degrees: 30.0,
            },
        )
    } else if let Some(rest) = lower.strip_prefix("gauge_") {
        (
            format!("gauge:{rest}"),
            Motion::Rotate {
                axis: [0.0, 0.0, 1.0],
                degrees: -270.0,
            },
        )
    } else if let Some(rest) = lower.strip_prefix("lamp_") {
        (format!("lamp:{rest}"), Motion::Visibility)
    } else if lower.starts_with("wheel_") || lower.starts_with("axle_") {
        (
            "wheel".to_string(),
            Motion::Rotate {
                axis: [1.0, 0.0, 0.0],
                degrees: 360.0,
            },
        )
    } else {
        return None;
    };
    Some(Part {
        node: name.to_string(),
        function,
        motion,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lod_levels_come_from_the_name_suffix() {
        assert_eq!(lod_level("body_LOD0"), Some(0));
        assert_eq!(lod_level("bogie_front_LOD2"), Some(2));
        assert_eq!(lod_level("body"), None);
        assert_eq!(lod_level("body_LODx"), None);
    }

    #[test]
    fn detected_lods_are_sorted_and_deduplicated() {
        let nodes: Vec<Node> = ["a_LOD1", "b_LOD0", "c_LOD1", "d"]
            .iter()
            .map(|n| Node {
                name: n.to_string(),
                lod: lod_level(n),
                suggestion: None,
            })
            .collect();
        let lods = detect_lods(&nodes);
        assert_eq!(lods.len(), 2);
        assert_eq!(lods[0].level, 0);
        assert_eq!(lods[0].distance, DEFAULT_LOD_DISTANCES[0]);
        assert_eq!(lods[1].level, 1);
    }

    #[test]
    fn name_prefixes_suggest_a_function() {
        let door = from_name("door_left").expect("recognised");
        assert_eq!(door.function, "door_left");
        assert!(matches!(door.motion, Motion::Translate { .. }));
        assert_eq!(
            from_name("sw_throttle").expect("recognised").function,
            "switch:throttle"
        );
        assert!(from_name("body").is_none());
    }

    /// Blender custom properties win over the name — that is the point of marking them.
    #[test]
    fn extras_win_over_the_name() {
        let extras =
            r#"{"ts_function":"door_right","ts_motion":"rotate","ts_axis":"0 1 0","ts_amount":95}"#;
        let part = suggest("some_node", Some(extras)).expect("recognised");
        assert_eq!(part.function, "door_right");
        assert_eq!(
            part.motion,
            Motion::Rotate {
                axis: [0.0, 1.0, 0.0],
                degrees: 95.0
            }
        );
    }
}
