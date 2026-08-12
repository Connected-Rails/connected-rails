//! OSM-Gleisdaten einlesen (Plan Kap. 15).
//!
//! Eingabeformat ist **Overpass-JSON** — das, was die Overpass-API bzw. Overpass Turbo
//! direkt ausliefert:
//!
//! ```overpassql
//! [out:json];
//! way["railway"="rail"](50.9,10.0,51.0,10.3);
//! (._;>;);
//! out body;
//! ```
//!
//! ponytail: JSON statt `.osm.pbf`. Overpass liefert genau das, was für eine Strecke
//! gebraucht wird (ein paar tausend Knoten); ein PBF-Reader wäre erst nötig, wenn ganze
//! Bundesländer eingelesen werden sollen.

use serde::Deserialize;
use std::collections::HashMap;

/// Ein Element der Overpass-Antwort (Knoten, Weg oder Relation).
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

/// Ein Gleisweg aus OSM.
#[derive(Debug, Clone, PartialEq)]
pub struct OsmWay {
    pub id: i64,
    pub nodes: Vec<i64>,
    /// `maxspeed` in km/h, falls angegeben.
    pub maxspeed: Option<f64>,
    pub name: Option<String>,
    pub electrified: bool,
}

/// Eingelesene Gleisdaten.
#[derive(Debug, Clone, Default)]
pub struct OsmRailway {
    /// Knotenkoordinaten (Grad).
    pub nodes: HashMap<i64, (f64, f64)>,
    pub ways: Vec<OsmWay>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OsmError {
    Json(String),
    /// Keine Gleiswege gefunden.
    NoRailway,
    /// Zu wenige Punkte für eine Strecke.
    TooShort,
}

impl std::fmt::Display for OsmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OsmError::Json(e) => write!(f, "Overpass-JSON nicht lesbar: {e}"),
            OsmError::NoRailway => write!(f, "keine Wege mit railway=rail gefunden"),
            OsmError::TooShort => write!(f, "zu wenige Punkte für eine Strecke"),
        }
    }
}

/// Liest Overpass-JSON und behält alle Wege mit `railway=rail`.
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

/// Ein Punkt der zusammengesetzten Strecke.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoutePoint {
    pub lat: f64,
    pub lon: f64,
    /// Zulässige Geschwindigkeit ab hier [km/h], falls in OSM angegeben.
    pub maxspeed: Option<f64>,
}

impl OsmRailway {
    /// Verkettet die Wege zu einer durchgehenden Punktfolge.
    ///
    /// Greedy über gemeinsame Endknoten, in beide Richtungen; Wege werden bei Bedarf
    /// umgedreht. Abzweige werden ignoriert — es entsteht ein Strang.
    /// ponytail: kein Routing über Weichen. Für eine Pilotstrecke (ein Streckengleis)
    /// reicht das; für Bahnhofsköpfe braucht es später eine echte Graphsuche.
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

        // Nach vorn verlängern.
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
        // Nach hinten verlängern.
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

        // Punktfolge aufbauen, doppelte Verbindungsknoten überspringen.
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

    /// Weg, der am Knoten `node` beginnt oder endet (für die Verlängerung nach vorn).
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

    /// Dasselbe für die Verlängerung nach hinten — der Weg muss auf `node` enden.
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

    /// Name der Strecke aus den Wegnamen (häufigster Name).
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
