//! Reading OSM track data (plan ch. 15).
//!
//! The input format is **Overpass JSON** — exactly what the Overpass API or Overpass
//! Turbo delivers:
//!
//! ```overpassql
//! [out:json];
//! way["railway"="rail"](50.9,10.0,51.0,10.3);
//! (._;>;);
//! out body;
//! ```
//!
//! ponytail: JSON instead of `.osm.pbf`. Overpass delivers exactly what is needed for a
//! line (a few thousand nodes); a PBF reader would only be necessary if whole federal
//! states had to be read in.

use crate::route::MarkerSource;
use serde::Deserialize;
use std::collections::HashMap;

/// One element of the Overpass response (node, way or relation).
#[derive(Debug, Clone, Deserialize)]
pub struct Element {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: i64,
    #[serde(default)]
    pub lat: Option<f64>,
    #[serde(default)]
    pub lon: Option<f64>,
    #[serde(default)]
    pub nodes: Vec<i64>,
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OverpassResponse {
    pub elements: Vec<Element>,
}

/// A track way from OSM.
#[derive(Debug, Clone, PartialEq)]
pub struct OsmWay {
    pub id: i64,
    pub nodes: Vec<i64>,
    /// `maxspeed` in km/h, if given.
    pub maxspeed: Option<f64>,
    pub name: Option<String>,
    pub electrified: bool,
}

/// Track data that has been read in.
#[derive(Debug, Clone, Default)]
pub struct OsmRailway {
    /// Node coordinates (degrees).
    pub nodes: HashMap<i64, (f64, f64)>,
    pub ways: Vec<OsmWay>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OsmError {
    Json(String),
    /// No track ways found.
    NoRailway,
    /// Too few points for a line.
    TooShort,
}

impl std::fmt::Display for OsmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OsmError::Json(e) => write!(f, "Overpass JSON not readable: {e}"),
            OsmError::NoRailway => write!(f, "no ways with railway=rail found"),
            OsmError::TooShort => write!(f, "too few points for a line"),
        }
    }
}

/// Reads Overpass JSON and keeps all ways with `railway=rail`.
pub fn parse(json: &str) -> Result<OsmRailway, OsmError> {
    let response: OverpassResponse =
        serde_json::from_str(json).map_err(|e| OsmError::Json(e.to_string()))?;

    let mut railway = OsmRailway::default();
    for e in response.elements {
        match e.kind.as_str() {
            "node" => {
                if let (Some(lat), Some(lon)) = (e.lat, e.lon) {
                    railway.nodes.insert(e.id, (lat, lon));
                }
            }
            "way" => {
                if e.tags.get("railway").map(String::as_str) != Some("rail") {
                    continue;
                }
                railway.ways.push(OsmWay {
                    id: e.id,
                    nodes: e.nodes,
                    maxspeed: e.tags.get("maxspeed").and_then(|v| v.parse().ok()),
                    name: e.tags.get("name").cloned(),
                    electrified: e
                        .tags
                        .get("electrified")
                        .is_some_and(|v| v != "no" && v != "none"),
                });
            }
            _ => {}
        }
    }
    if railway.ways.is_empty() {
        return Err(OsmError::NoRailway);
    }
    Ok(railway)
}

/// Forest polygons (`landuse=forest` / `natural=wood`) from an Overpass extract,
/// as `(lat, lon)` rings in degrees — the route editor's "Import forest" reads
/// them into [`crate::route::ForestSource`]s. An extract without any is fine
/// (empty result, not an error).
///
/// Query, analogous to the track one:
///
/// ```overpassql
/// [out:json];
/// (way["landuse"="forest"](52.0,10.0,52.1,10.2); way["natural"="wood"](52.0,10.0,52.1,10.2););
/// (._;>;);
/// out body;
/// ```
// ponytail: closed ways only — multipolygon relations (forests with clearings)
// come in as their outer ways or not at all; the relation assembly joins the
// importer once a real line needs it.
pub fn parse_forests(json: &str) -> Result<Vec<Vec<(f64, f64)>>, OsmError> {
    let response: OverpassResponse =
        serde_json::from_str(json).map_err(|e| OsmError::Json(e.to_string()))?;

    let mut nodes: HashMap<i64, (f64, f64)> = HashMap::new();
    let mut ways: Vec<Vec<i64>> = Vec::new();
    for e in response.elements {
        match e.kind.as_str() {
            "node" => {
                if let (Some(lat), Some(lon)) = (e.lat, e.lon) {
                    nodes.insert(e.id, (lat, lon));
                }
            }
            "way" => {
                let forest = e.tags.get("landuse").map(String::as_str) == Some("forest")
                    || e.tags.get("natural").map(String::as_str) == Some("wood");
                if forest {
                    ways.push(e.nodes);
                }
            }
            _ => {}
        }
    }

    let mut polygons = Vec::new();
    for mut way in ways {
        // Closed ways repeat their first node at the end — the ring is implicit.
        if way.len() > 1 && way.first() == way.last() {
            way.pop();
        }
        let ring: Vec<(f64, f64)> = way.iter().filter_map(|id| nodes.get(id).copied()).collect();
        if ring.len() >= 3 {
            polygons.push(ring);
        }
    }
    Ok(polygons)
}

/// Tags the marker import recognises, and the layer each one lands in.
/// `"*"` matches any value — a way tagged `bridge=viaduct` is a bridge like
/// any other.
///
/// The order decides: the first match wins, so a `railway=platform` way that
/// also carries `bridge=yes` stays a platform.
const MARKER_LAYERS: &[(&str, &str, &str)] = &[
    ("railway", "level_crossing", "level-crossing"),
    ("railway", "crossing", "level-crossing"),
    ("railway", "platform", "platform"),
    ("railway", "station", "station"),
    ("railway", "halt", "station"),
    ("railway", "signal", "signal"),
    ("railway", "switch", "switch"),
    ("railway", "buffer_stop", "buffer-stop"),
    ("railway", "milestone", "kilometre-mark"),
    ("bridge", "*", "bridge"),
    ("tunnel", "*", "tunnel"),
    ("power", "tower", "power-tower"),
    ("man_made", "tower", "tower"),
    ("man_made", "water_tower", "tower"),
];

/// Reference markers from an Overpass extract: everything in [`MARKER_LAYERS`]
/// becomes one [`MarkerSource`] in the layer of the tag it matched. An extract
/// without any is fine (empty result, not an error).
///
/// Query — whatever is wanted, the filter below sorts it out:
///
/// ```overpassql
/// [out:json];
/// (node["railway"](52.0,10.0,52.1,10.2); way["railway"](52.0,10.0,52.1,10.2););
/// (._;>;);
/// out body;
/// ```
// ponytail: a way becomes its midpoint. A marker says "something belongs
// here", and for that a platform is as useful as a point as it is as an
// outline; carrying the outline would mean a second primitive to draw, pick
// and delete. Relations are skipped like in `parse_forests`.
pub fn parse_markers(json: &str) -> Result<Vec<MarkerSource>, OsmError> {
    let response: OverpassResponse =
        serde_json::from_str(json).map_err(|e| OsmError::Json(e.to_string()))?;

    let mut nodes: HashMap<i64, (f64, f64)> = HashMap::new();
    let mut markers = Vec::new();
    // Ways are resolved after the pass — Overpass lists them before their nodes.
    let mut ways: Vec<(Vec<i64>, &'static str, String)> = Vec::new();

    for e in &response.elements {
        if let (Some(lat), Some(lon)) = (e.lat, e.lon) {
            nodes.insert(e.id, (lat, lon));
        }
    }
    for e in &response.elements {
        let Some(layer) = layer_of(&e.tags) else {
            continue;
        };
        let label = label_of(&e.tags, layer);
        match e.kind.as_str() {
            "node" => {
                if let Some(&(lat, lon)) = nodes.get(&e.id) {
                    markers.push(MarkerSource {
                        layer: layer.into(),
                        label,
                        lat,
                        lon,
                    });
                }
            }
            "way" => ways.push((e.nodes.clone(), layer, label)),
            _ => {}
        }
    }
    for (way, layer, label) in ways {
        let points: Vec<(f64, f64)> = way.iter().filter_map(|id| nodes.get(id).copied()).collect();
        if points.is_empty() {
            continue;
        }
        let n = points.len() as f64;
        markers.push(MarkerSource {
            layer: layer.into(),
            label,
            lat: points.iter().map(|p| p.0).sum::<f64>() / n,
            lon: points.iter().map(|p| p.1).sum::<f64>() / n,
        });
    }
    Ok(markers)
}

/// The first layer of [`MARKER_LAYERS`] whose tag the element carries.
fn layer_of(tags: &HashMap<String, String>) -> Option<&'static str> {
    MARKER_LAYERS.iter().find_map(|(key, value, layer)| {
        let actual = tags.get(*key)?;
        // `bridge=no` is not a bridge — the negations are worth the two lines.
        let matches = (*value == "*" && actual != "no" && actual != "none") || actual == value;
        matches.then_some(*layer)
    })
}

/// `name`, else `ref` (the kilometre of a milestone lives there), else the
/// layer itself — a marker without any text at all is hard to tell apart.
fn label_of(tags: &HashMap<String, String>, layer: &str) -> String {
    for key in ["name", "ref", "railway:position"] {
        if let Some(value) = tags.get(key) {
            return value.clone();
        }
    }
    layer.to_string()
}

/// A point of the assembled line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoutePoint {
    pub lat: f64,
    pub lon: f64,
    /// Permitted speed from here on [km/h], if given in OSM.
    pub maxspeed: Option<f64>,
}

impl OsmRailway {
    /// Chains the ways into one continuous point sequence.
    ///
    /// Greedy over shared end nodes, in both directions; ways are reversed when needed.
    /// Junctions are ignored — a single strand results.
    /// ponytail: no routing across switches. For a pilot line (a single running track)
    /// that is enough; station throats will later need a real graph search.
    pub fn chain(&self, start_way: Option<i64>) -> Result<Vec<RoutePoint>, OsmError> {
        if self.ways.is_empty() {
            return Err(OsmError::NoRailway);
        }
        let start = start_way
            .and_then(|id| self.ways.iter().position(|w| w.id == id))
            .unwrap_or(0);

        let mut used = vec![false; self.ways.len()];
        used[start] = true;
        let mut chain: Vec<usize> = vec![start];
        let mut reversed: Vec<bool> = vec![false];

        // Extend forwards.
        loop {
            let last = *chain.last().unwrap();
            let end = *self
                .way_nodes(last, reversed[chain.len() - 1])
                .last()
                .unwrap();
            let Some((next, flip)) = self.find_connecting(end, &used) else {
                break;
            };
            used[next] = true;
            chain.push(next);
            reversed.push(flip);
        }
        // Extend backwards.
        loop {
            let first = chain[0];
            let start_node = self.way_nodes(first, reversed[0])[0];
            let Some((prev, flip)) = self.find_connecting_end(start_node, &used) else {
                break;
            };
            used[prev] = true;
            chain.insert(0, prev);
            reversed.insert(0, flip);
        }

        // Build the point sequence, skipping duplicated connecting nodes.
        let mut points: Vec<RoutePoint> = Vec::new();
        for (i, &way) in chain.iter().enumerate() {
            let nodes = self.way_nodes(way, reversed[i]);
            let speed = self.ways[way].maxspeed;
            for (j, node) in nodes.iter().enumerate() {
                if i > 0 && j == 0 {
                    continue;
                }
                if let Some(&(lat, lon)) = self.nodes.get(node) {
                    points.push(RoutePoint {
                        lat,
                        lon,
                        maxspeed: speed,
                    });
                }
            }
        }
        if points.len() < 3 {
            return Err(OsmError::TooShort);
        }
        Ok(points)
    }

    fn way_nodes(&self, index: usize, reversed: bool) -> Vec<i64> {
        let mut nodes = self.ways[index].nodes.clone();
        if reversed {
            nodes.reverse();
        }
        nodes
    }

    /// Way that starts or ends at node `node` (for extending forwards).
    fn find_connecting(&self, node: i64, used: &[bool]) -> Option<(usize, bool)> {
        for (i, way) in self.ways.iter().enumerate() {
            if used[i] || way.nodes.len() < 2 {
                continue;
            }
            if way.nodes[0] == node {
                return Some((i, false));
            }
            if *way.nodes.last().unwrap() == node {
                return Some((i, true));
            }
        }
        None
    }

    /// The same for extending backwards — the way must end at `node`.
    fn find_connecting_end(&self, node: i64, used: &[bool]) -> Option<(usize, bool)> {
        for (i, way) in self.ways.iter().enumerate() {
            if used[i] || way.nodes.len() < 2 {
                continue;
            }
            if *way.nodes.last().unwrap() == node {
                return Some((i, false));
            }
            if way.nodes[0] == node {
                return Some((i, true));
            }
        }
        None
    }

    /// Name of the line taken from the way names (most frequent name).
    pub fn name(&self) -> Option<String> {
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for w in &self.ways {
            if let Some(n) = &w.name {
                *counts.entry(n.as_str()).or_default() += 1;
            }
        }
        counts
            .into_iter()
            .max_by_key(|(_, c)| *c)
            .map(|(n, _)| n.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forests_come_out_of_overpass_json() {
        let json = r#"{"elements": [
            {"type": "node", "id": 1, "lat": 52.0, "lon": 10.0},
            {"type": "node", "id": 2, "lat": 52.0, "lon": 10.01},
            {"type": "node", "id": 3, "lat": 52.01, "lon": 10.01},
            {"type": "node", "id": 4, "lat": 52.01, "lon": 10.0},
            {"type": "way", "id": 10, "nodes": [1, 2, 3, 4, 1], "tags": {"landuse": "forest"}},
            {"type": "way", "id": 11, "nodes": [1, 2, 3, 1], "tags": {"natural": "wood"}},
            {"type": "way", "id": 12, "nodes": [1, 2, 3, 4, 1], "tags": {"landuse": "meadow"}},
            {"type": "way", "id": 13, "nodes": [1, 2, 1], "tags": {"landuse": "forest"}}
        ]}"#;
        let polygons = parse_forests(json).expect("parses");
        // The meadow is skipped, the two-point ring dropped, the closing node removed.
        assert_eq!(polygons.len(), 2);
        assert_eq!(polygons[0].len(), 4);
        assert_eq!(polygons[0][0], (52.0, 10.0));
        assert_eq!(polygons[1].len(), 3);

        // A forest-free extract is empty, not an error.
        assert_eq!(parse_forests(r#"{"elements": []}"#), Ok(vec![]));
    }

    #[test]
    fn markers_land_in_the_layer_of_their_tag() {
        let json = r#"{"elements": [
            {"type": "node", "id": 1, "lat": 52.0, "lon": 10.0,
             "tags": {"railway": "level_crossing", "name": "Dorfstraße"}},
            {"type": "node", "id": 2, "lat": 52.1, "lon": 10.1,
             "tags": {"railway": "milestone", "railway:position": "108.2"}},
            {"type": "node", "id": 3, "lat": 52.2, "lon": 10.2, "tags": {"highway": "bus_stop"}},
            {"type": "node", "id": 4, "lat": 52.0, "lon": 10.0},
            {"type": "node", "id": 5, "lat": 52.0, "lon": 10.2},
            {"type": "way", "id": 20, "nodes": [4, 5], "tags": {"railway": "platform"}},
            {"type": "way", "id": 21, "nodes": [4, 5], "tags": {"bridge": "no"}}
        ]}"#;
        let markers = parse_markers(json).expect("parses");

        // The bus stop and the `bridge=no` way are not markers.
        assert_eq!(markers.len(), 3);
        assert_eq!(markers[0].layer, "level-crossing");
        assert_eq!(markers[0].label, "Dorfstraße");
        // No `name`: the kilometre out of `railway:position` is the label.
        assert_eq!(markers[1].layer, "kilometre-mark");
        assert_eq!(markers[1].label, "108.2");
        // The way became its midpoint, and no label of its own means the layer.
        let platform = &markers[2];
        assert_eq!(platform.layer, "platform");
        assert_eq!(platform.label, "platform");
        assert!((platform.lon - 10.1).abs() < 1e-9, "{}", platform.lon);

        assert_eq!(parse_markers(r#"{"elements": []}"#), Ok(vec![]));
    }
}
