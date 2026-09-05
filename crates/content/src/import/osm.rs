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

use crate::route::{
    CenterLine, MarkerSource, RoadSource, RoadSurface, WaterSource, WindTurbineSource,
};
use serde::Deserialize;
use std::collections::HashMap;

/// One member of a relation.
#[derive(Debug, Clone, Deserialize)]
pub struct Member {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(rename = "ref")]
    pub id: i64,
    #[serde(default)]
    pub role: String,
}

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
    pub members: Vec<Member>,
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

/// Water bodies from an Overpass extract, as [`WaterSource`]s — the route
/// editor's "Import water" reads them into the line's `waters`. An extract
/// without any is fine (empty result, not an error).
///
/// Query, analogous to the track one:
///
/// ```overpassql
/// [out:json];
/// (
///   way["natural"="water"](52.0,10.0,52.1,10.2);
///   relation["natural"="water"](52.0,10.0,52.1,10.2);
///   way["waterway"~"^(riverbank|dock)$"](52.0,10.0,52.1,10.2);
///   way["landuse"~"^(reservoir|basin)$"](52.0,10.0,52.1,10.2);
/// );
/// (._;>;);
/// out body;
/// ```
///
/// Multipolygon relations are assembled: the `outer` members are chained over
/// their shared end nodes into rings — one body per ring, a braided river
/// therefore becomes several — and each `inner` ring (an island) becomes a
/// hole of the smallest outer that contains it. Ways a relation claims are
/// not read again on their own, so a lake is not imported twice.
// ponytail: the chaining is the same greedy end-node walk the track import
// does. A ring split over ways that share a node with *three* ways — the
// extractor cut through a junction — chains greedily and may join the wrong
// pair; the result is still a ring on the water, only with a kink where the
// wrong turn was taken. A river mapped as a centre line (`waterway=river`)
// is deliberately not read: a line has no width, and buffering it here would
// invent banks the map does not vouch for.
pub fn parse_water(json: &str) -> Result<Vec<WaterSource>, OsmError> {
    let response: OverpassResponse =
        serde_json::from_str(json).map_err(|e| OsmError::Json(e.to_string()))?;

    let mut nodes: HashMap<i64, (f64, f64)> = HashMap::new();
    // Ways by id, node list and tags — the relation members are usually
    // untagged, the tags sit on the relation, but a standalone way carries
    // its own.
    let mut ways: HashMap<i64, (Vec<i64>, HashMap<String, String>)> = HashMap::new();
    let mut relations: Vec<WaterRelation> = Vec::new();
    for e in &response.elements {
        match e.kind.as_str() {
            "node" => {
                if let (Some(lat), Some(lon)) = (e.lat, e.lon) {
                    nodes.insert(e.id, (lat, lon));
                }
            }
            "way" => {
                ways.insert(e.id, (e.nodes.clone(), e.tags.clone()));
            }
            "relation" => {
                if e.tags.get("type").map(String::as_str) != Some("multipolygon") {
                    continue;
                }
                let Some((tag, name)) = water_tag(&e.tags) else {
                    continue;
                };
                let (mut outer, mut inner) = (Vec::new(), Vec::new());
                for member in &e.members {
                    if member.kind != "way" {
                        continue;
                    }
                    // No role means outer, by the multipolygon convention.
                    match member.role.as_str() {
                        "inner" | "enclave" => inner.push(member.id),
                        _ => outer.push(member.id),
                    }
                }
                relations.push(WaterRelation {
                    outer,
                    inner,
                    tag,
                    name,
                });
            }
            _ => {}
        }
    }

    // The ways the relations claim: they build the relation's rings, not
    // bodies of their own.
    let claimed: std::collections::HashSet<i64> = relations
        .iter()
        .flat_map(|relation| relation.outer.iter().chain(&relation.inner).copied())
        .collect();

    let mut waters = Vec::new();
    // Standalone closed ways — a lake drawn as one way, still the common
    // case. In id order, so the same extract always imports the same line.
    let mut standalone: Vec<i64> = ways
        .keys()
        .copied()
        .filter(|id| !claimed.contains(id))
        .collect();
    standalone.sort_unstable();
    for id in standalone {
        let (way, tags) = &ways[&id];
        let Some((tag, name)) = water_tag(tags) else {
            continue;
        };
        let Some(polygon) = ring_of(way, &nodes) else {
            continue;
        };
        waters.push(WaterSource {
            name: name.unwrap_or_default(),
            polygon,
            holes: Vec::new(),
            tags: vec![tag.into()],
        });
    }
    // Relations: outer rings chained, inners attached to the outer that
    // holds them.
    for relation in relations {
        let outers = chain_rings(&relation.outer, &ways, &nodes);
        if outers.is_empty() {
            continue;
        }
        let inners = chain_rings(&relation.inner, &ways, &nodes);
        for (i, polygon) in outers.iter().enumerate() {
            let Some(seen) = polygon.first() else {
                continue;
            };
            // The smallest containing outer wins, so an inner is not glued
            // to the whole braided river when one braid holds it.
            let owner = outers
                .iter()
                .enumerate()
                .filter(|(_, candidate)| contains_ring(candidate, seen))
                .min_by(|a, b| ring_degree_area(a.1).total_cmp(&ring_degree_area(b.1)));
            if owner.is_some_and(|(j, _)| j != i) {
                // Inside another outer of the same relation: a lake on an
                // island, which the multipolygon semantics would call an
                // outer of its own — the extract said inner, and it is
                // nobody's hole.
                continue;
            }
            let holes: Vec<Vec<crate::route::WaterPoint>> = inners
                .iter()
                .filter(|hole| hole.first().is_some_and(|p| contains_ring(polygon, p)))
                .cloned()
                .collect();
            // The relation's own name, else that of a member way.
            let name = relation
                .name
                .clone()
                .or_else(|| {
                    relation.outer.iter().find_map(|id| {
                        ways.get(id)
                            .and_then(|(_, tags)| tags.get("name"))
                            .filter(|n| !n.is_empty())
                            .cloned()
                    })
                })
                .unwrap_or_default();
            waters.push(WaterSource {
                name,
                polygon: polygon.clone(),
                holes,
                tags: vec![relation.tag.into()],
            });
        }
    }
    Ok(waters)
}

/// Closes a way into a ring of points: the closing node dropped, the nodes
/// resolved, three corners at least. `None` for an open or unresolvable way.
fn ring_of(way: &[i64], nodes: &HashMap<i64, (f64, f64)>) -> Option<Vec<crate::route::WaterPoint>> {
    let mut way = way.to_vec();
    if way.len() > 1 && way.first() == way.last() {
        way.pop();
    }
    let polygon: Vec<crate::route::WaterPoint> = way
        .iter()
        .filter_map(|id| nodes.get(id))
        .map(|&(lat, lon)| crate::route::WaterPoint { lat, lon })
        .collect();
    (polygon.len() >= 3).then_some(polygon)
}

/// Chains ways over their shared end nodes into rings — the same greedy walk
/// the track import uses, in both directions and without its preference. A
/// ring the extract cut open at the bounding box does not close and is left
/// out; the rest of the relation still imports.
fn chain_rings(
    members: &[i64],
    ways: &HashMap<i64, (Vec<i64>, HashMap<String, String>)>,
    nodes: &HashMap<i64, (f64, f64)>,
) -> Vec<Vec<crate::route::WaterPoint>> {
    let pool: Vec<&Vec<i64>> = members
        .iter()
        .filter_map(|id| ways.get(id).map(|(way, _)| way))
        .collect();
    if pool.is_empty() {
        return Vec::new();
    }
    let end = |chain: &[(usize, bool)], at_end: bool| {
        let &(index, reversed) = if at_end {
            chain.last().expect("seeded")
        } else {
            chain.first().expect("seeded")
        };
        let way = pool[index];
        let mut node = if at_end { way[way.len() - 1] } else { way[0] };
        if reversed {
            node = if at_end { way[0] } else { way[way.len() - 1] };
        }
        node
    };
    let closed = |chain: &[(usize, bool)]| end(chain, true) == end(chain, false);

    let mut used = vec![false; pool.len()];
    let mut rings = Vec::new();
    for start in 0..pool.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let mut chain: Vec<(usize, bool)> = vec![(start, false)];
        loop {
            if closed(&chain) {
                break;
            }
            // One step in either direction per pass, until the ring closes
            // or neither end finds a partner.
            let tail = end(&chain, true);
            let forward = pool.iter().enumerate().find_map(|(i, way)| {
                if used[i] {
                    return None;
                }
                let (first, last) = (way[0], way[way.len() - 1]);
                (first == tail)
                    .then_some((i, false))
                    .or((last == tail).then_some((i, true)))
            });
            if let Some((next, flip)) = forward {
                used[next] = true;
                chain.push((next, flip));
                continue;
            }
            let head = end(&chain, false);
            let backward = pool.iter().enumerate().find_map(|(i, way)| {
                if used[i] {
                    return None;
                }
                let (first, last) = (way[0], way[way.len() - 1]);
                (last == head)
                    .then_some((i, false))
                    .or((first == head).then_some((i, true)))
            });
            if let Some((prev, flip)) = backward {
                used[prev] = true;
                chain.insert(0, (prev, flip));
                continue;
            }
            break;
        }
        if !closed(&chain) {
            continue;
        }
        let mut seq: Vec<i64> = Vec::new();
        for &(index, reversed) in &chain {
            let mut way = pool[index].clone();
            if reversed {
                way.reverse();
            }
            // The node a partner joins on appears twice.
            let skip = usize::from(!seq.is_empty());
            seq.extend(way.into_iter().skip(skip));
        }
        if let Some(polygon) = ring_of(&seq, nodes) {
            rings.push(polygon);
        }
    }
    rings
}

/// Whether the point `p` (degrees) lies inside the ring — the containment the
/// inner-to-outer assignment and nothing else needs, so the cheap degree-space
/// ray cast is enough.
fn contains_ring(ring: &[crate::route::WaterPoint], p: &crate::route::WaterPoint) -> bool {
    crate::terrain::point_in_polygon(
        glam::DVec2::new(p.lat, p.lon),
        &ring
            .iter()
            .map(|q| glam::DVec2::new(q.lat, q.lon))
            .collect::<Vec<_>>(),
    )
}

/// Area of a ring in square degrees — only ever compared against a neighbour
/// of the same relation, so the projection's latitude distortion cancels.
fn ring_degree_area(ring: &[crate::route::WaterPoint]) -> f64 {
    let mut total = 0.0;
    let mut j = ring.len().saturating_sub(1);
    for i in 0..ring.len() {
        total += (ring[j].lat - ring[i].lat) * (ring[j].lon + ring[i].lon);
        j = i;
    }
    (total / 2.0).abs()
}

/// One water multipolygon waiting for its rings: the member way ids by role,
/// the water family the relation carries, and its own name.
struct WaterRelation {
    outer: Vec<i64>,
    inner: Vec<i64>,
    tag: &'static str,
    name: Option<String>,
}

/// Which water family a way or relation belongs to — the first tag pair that
/// matches wins, and `waterway=river` and friends name a centre line, not an
/// area, so they are not read. The third element is what goes into
/// `WaterSource::tags`.
fn water_tag(tags: &HashMap<String, String>) -> Option<(&'static str, Option<String>)> {
    const FAMILIES: &[(&str, &str, &str)] = &[
        ("natural", "water", "water"),
        ("waterway", "riverbank", "riverbank"),
        ("waterway", "dock", "dock"),
        ("landuse", "reservoir", "reservoir"),
        ("landuse", "basin", "basin"),
    ];
    for (key, value, tag) in FAMILIES {
        if tags.get(*key).map(String::as_str) == Some(*value) {
            return Some((tag, tags.get("name").filter(|n| !n.is_empty()).cloned()));
        }
    }
    None
}

/// The `highway=*` classes the road import reads, and the editor preset each
/// lands on (see [`crate::roads::PRESETS`] — width, surface and markings come
/// from there, and every one of them stays editable afterwards).
///
/// The order decides nothing — the pairs are distinct keys — but the classes
/// a German road network is made of are all here: the divided roads come in
/// as one way per carriageway, so the motorway presets are one *Fahrbahn*;
/// `trunk` and `primary` name the same built road (Bundesstraße) for OSM's
/// purposes and land on the same preset; `footway`, `cycleway`, `path` and
/// `steps` are deliberately absent — a line's footpaths are the people
/// module's business, not the road surface's.
///
/// `track` lands on the **gravel** preset, not a paved one: `highway=track`
/// says what a way is used for, not what it is built of, and the German ones
/// are unpaved far more often than not. The ones that are paved say so —
/// `surface=*` or `tracktype=grade1` — and [`surface_of`] reads them.
const ROAD_CLASSES: &[(&str, &str)] = &[
    ("motorway", "motorway"),
    ("motorway_link", "motorway"),
    ("trunk", "federal"),
    ("trunk_link", "federal"),
    ("primary", "federal"),
    ("primary_link", "federal"),
    ("secondary", "secondary"),
    ("secondary_link", "secondary"),
    ("tertiary", "secondary"),
    ("tertiary_link", "secondary"),
    ("unclassified", "residential"),
    ("residential", "residential"),
    ("living_street", "living"),
    ("pedestrian", "living"),
    ("road", "residential"),
    ("service", "service"),
    ("track", "farm-gravel"),
];

/// What OSM's `surface=*` says a carriageway is made of, for the values a
/// German extract carries. The dark bound surfaces are asphalt; the pale hard
/// ones — the slabs, the setts, the block paving — are concrete; and the
/// unpaved family is gravel, which is what a Feldweg is. A value the table
/// does not know leaves the class preset in charge rather than guessing.
///
/// The lookup is on the value's first word: `concrete:plates` is concrete,
/// `gravel;dirt` is gravel.
const SURFACES: &[(&str, RoadSurface)] = &[
    ("asphalt", RoadSurface::Asphalt),
    ("paved", RoadSurface::Asphalt),
    ("bitumen", RoadSurface::Asphalt),
    ("chipseal", RoadSurface::Asphalt),
    ("tarmac", RoadSurface::Asphalt),
    ("concrete", RoadSurface::Concrete),
    ("cement", RoadSurface::Concrete),
    ("paving_stones", RoadSurface::Concrete),
    ("sett", RoadSurface::Concrete),
    ("cobblestone", RoadSurface::Concrete),
    ("bricks", RoadSurface::Concrete),
    ("brick", RoadSurface::Concrete),
    ("unpaved", RoadSurface::Gravel),
    ("gravel", RoadSurface::Gravel),
    ("fine_gravel", RoadSurface::Gravel),
    ("compacted", RoadSurface::Gravel),
    ("pebblestone", RoadSurface::Gravel),
    ("ground", RoadSurface::Gravel),
    ("dirt", RoadSurface::Gravel),
    ("earth", RoadSurface::Gravel),
    ("soil", RoadSurface::Gravel),
    ("mud", RoadSurface::Gravel),
    ("sand", RoadSurface::Gravel),
    ("grass", RoadSurface::Gravel),
    ("grass_paver", RoadSurface::Gravel),
    ("woodchips", RoadSurface::Gravel),
    ("rock", RoadSurface::Gravel),
    ("stone", RoadSurface::Gravel),
];

/// Roads from an Overpass extract, as [`RoadSource`]s — the route editor's
/// "Import roads" reads them into the line's `roads`. An extract without any
/// is fine (empty result, not an error).
///
/// Query, analogous to the track one:
///
/// ```overpassql
/// [out:json];
/// way["highway"](52.0,10.0,52.1,10.2);
/// (._;>;);
/// out body;
/// ```
///
/// OSM maps a street as its centre line, so that is what a road is here: the
/// class decides the preset — width, surface and markings, all editable —
/// and where the mapper said more, it wins: `surface=*` and `tracktype=*` over
/// the preset's surface ([`surface_of`]), `width=*`/`lanes=*` over its width,
/// `oneway=yes` takes the centre line out, an unpaved surface takes every
/// marking out, and `bridge=*` flags the way as flying (see
/// [`crate::route::RoadSource::bridge`]). A carriageway of a divided road
/// (`oneway=yes`) carries no centre line, which is what makes an Autobahn
/// read as two carriageways rather than one striped one.
// ponytail: the OSM `width` tag is often absent and occasionally nonsense
// (a lane count, a feet figure). What is parsed here is the plain metres
// number; everything else falls back to the class preset, which is a
// planning value a builder can correct in the panel. Kerbs, parking lanes
// and `sidewalk=*` are not read: the carriageway is what the road *is*.
pub fn parse_roads(json: &str) -> Result<Vec<RoadSource>, OsmError> {
    let response: OverpassResponse =
        serde_json::from_str(json).map_err(|e| OsmError::Json(e.to_string()))?;

    let mut nodes: HashMap<i64, (f64, f64)> = HashMap::new();
    let mut roads = Vec::new();
    for e in &response.elements {
        if let (Some(lat), Some(lon)) = (e.lat, e.lon) {
            nodes.insert(e.id, (lat, lon));
        }
    }
    for e in &response.elements {
        if e.kind != "way" {
            continue;
        }
        let Some(class) = e.tags.get("highway") else {
            continue;
        };
        // Unpaved tracks and paths are not roads; the classes above are.
        let Some(preset) = road_preset_id(class).and_then(crate::roads::preset) else {
            continue;
        };
        let points: Vec<crate::route::RoadPoint> = e
            .nodes
            .iter()
            .filter_map(|id| nodes.get(id))
            .map(|&(lat, lon)| crate::route::RoadPoint { lat, lon })
            .collect();
        if points.len() < 2 {
            continue;
        }
        let surface = surface_of(&e.tags, preset);
        let width = width_of(&e.tags)
            .or_else(|| lanes_width(&e.tags))
            .unwrap_or(preset.width);
        // A one-way way is one carriageway — no centre line, however wide —
        // and the narrowest classes never had one.
        let center_line = if two_way(class, &e.tags) {
            preset.center_line
        } else {
            CenterLine::None
        };
        // Nobody paints on a loose surface: an unpaved way carries neither
        // centre line nor Randlinie, whatever its class would have had. A
        // `residential` way through a village that says `surface=gravel` is a
        // gravel lane, not a striped street.
        let (center_line, edge_lines) = match surface {
            RoadSurface::Gravel => (CenterLine::None, false),
            _ => (center_line, preset.edge_lines),
        };
        // A bridge flies: over the hollow the way spans — a valley, a river,
        // a cutting — the carriageway holds the straight line between its own
        // ends instead of following the ground. Any `bridge=*` but `no`
        // counts; a viaduct is a bridge like any other.
        let bridge = e.tags.get("bridge").is_some_and(|v| v != "no");
        roads.push(RoadSource {
            name: name_of(&e.tags),
            points,
            width,
            surface,
            center_line,
            edge_lines,
            bridge,
            tags: vec![format!("highway-{}", e.tags["highway"])],
        });
    }
    Ok(roads)
}

/// The Overpass QL for the roads of a box — exactly the classes
/// [`ROAD_CLASSES`] reads, so the editor's live fetch and a hand-downloaded
/// extract agree on what comes back.
pub fn roads_query(min_lat: f64, min_lon: f64, max_lat: f64, max_lon: f64) -> String {
    // Overpass takes its box south, west, north, east.
    let bbox = format!("{min_lat:.6},{min_lon:.6},{max_lat:.6},{max_lon:.6}");
    let classes = ROAD_CLASSES
        .iter()
        .map(|(class, _)| *class)
        .collect::<Vec<_>>()
        .join("|");
    format!(
        "[out:json][timeout:120];\
         way[\"highway\"~\"^({classes})$\"]({bbox});\
         (._;>;);out body;"
    )
}

/// The preset id a `highway=*` class lands on.
fn road_preset_id(class: &str) -> Option<&'static str> {
    ROAD_CLASSES
        .iter()
        .find(|(key, _)| *key == class)
        .map(|(_, preset)| *preset)
}

/// What the carriageway is made of: the mapper's `surface=*` where the table
/// knows the value, else what a field track's `tracktype=*` implies, else the
/// class preset's own surface.
///
/// The tag order is what keeps a Feldweg a Feldweg. `highway=track` says
/// nothing about the material — it is the *use* of the way, and the German
/// ones run from a concrete Betonspurbahn to two ruts through a field — so the
/// class alone may not pave it. `tracktype=*` is the tag that grades how solid
/// it is, and it is on far more German tracks than `surface=*` is: `grade1`
/// is the solid one, everything below it is loose, whatever it is made of.
///
/// A solid track reads as **asphalt**, not as concrete slabs. The tag does not
/// say which, but the mappers who did write a surface do: over a Börde box of
/// 327 field tracks, the paved ones say `asphalt` or `paved` 52 times and
/// `concrete` twice. A Betonspurbahn is a real thing and it comes out of
/// `surface=concrete*` — it is not what an ungraded majority should be turned
/// into.
fn surface_of(tags: &HashMap<String, String>, preset: &crate::roads::RoadPreset) -> RoadSurface {
    if let Some(surface) = tags.get("surface").and_then(|value| named_surface(value)) {
        return surface;
    }
    match tags.get("tracktype").map(String::as_str) {
        Some("grade1") => RoadSurface::Asphalt,
        Some("grade2" | "grade3" | "grade4" | "grade5") => RoadSurface::Gravel,
        _ => preset.surface,
    }
}

/// The surface a `surface=*` value names, by its first word — `concrete:plates`
/// is concrete, `gravel;dirt` is gravel. A value [`SURFACES`] does not know is
/// no answer: the preset gives one.
fn named_surface(value: &str) -> Option<RoadSurface> {
    let value = value.trim().to_ascii_lowercase();
    let word = value
        .split([';', ':', '/'])
        .next()
        .unwrap_or_default()
        .trim();
    SURFACES
        .iter()
        .find(|(name, _)| *name == word)
        .map(|(_, surface)| *surface)
}

/// The carriageway width the mapper wrote [m], if it parses: a plain number
/// (`width=6.5`), else nothing — the preset answers for everything else.
fn width_of(tags: &HashMap<String, String>) -> Option<f64> {
    tags.get("width")
        .and_then(|w| {
            // `width=6`, `width=6.5`, `width=6 m` — anything after the number
            // (and its comma) is a unit the tag does not vouch for.
            let text = &w[..w.len().min(8)];
            let number: String = text
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == ',')
                .collect();
            number.replace(',', ".").trim().parse().ok()
        })
        .filter(|w| (1.0..=30.0).contains(w))
}

/// The width from a `lanes=*` count [m]: 3.5 m the lane, and the centre line
/// is not on the carriageway — it is painted *on* it, so the lanes make the
/// whole width. A lane count the tag does not vouch for (missing, absurd)
/// leaves the preset in charge.
fn lanes_width(tags: &HashMap<String, String>) -> Option<f64> {
    tags.get("lanes")
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|lanes| (1..=8).contains(lanes))
        .map(|lanes| lanes as f64 * 3.5)
}
/// Whether a road runs in both directions and is wide enough to stripe: a
/// one-way way is one carriageway, and a farm track nobody stripes.
///
/// A **roundabout** counts as one-way whether or not the mapper wrote the
/// tag, because `junction=roundabout` implies it — and a ring with a centre
/// line down the middle of it is the one marking no German roundabout has.
fn two_way(class: &str, tags: &HashMap<String, String>) -> bool {
    let one_way = tags.get("oneway").is_some_and(|v| v == "yes" || v == "-1")
        || tags
            .get("junction")
            .is_some_and(|v| v == "roundabout" || v == "circular")
        || class.ends_with("_link")
        || tags.get("lanes").and_then(|v| v.parse::<u32>().ok()) == Some(1);
    let narrow = matches!(class, "service" | "living_street" | "pedestrian" | "track");
    !one_way && !narrow
}

fn name_of(tags: &HashMap<String, String>) -> String {
    for key in ["name", "ref"] {
        if let Some(value) = tags.get(key)
            && !value.is_empty()
        {
            return value.clone();
        }
    }
    String::new()
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

/// The Overpass QL for the overhead lines of a box: the transmission and
/// distribution lines, with their mast nodes — `(._;>;)` pulls the nodes in, and
/// the nodes are where the mast picture is written (`design=*`).
pub fn power_query(min_lat: f64, min_lon: f64, max_lat: f64, max_lon: f64) -> String {
    // Overpass takes its box south, west, north, east.
    let bbox = format!("{min_lat:.6},{min_lon:.6},{max_lat:.6},{max_lon:.6}");
    format!(
        "[out:json][timeout:120];\
         way[\"power\"~\"^(line|minor_line)$\"]({bbox});\
         (._;>;);out body;"
    )
}

/// The overhead lines of an Overpass extract.
///
/// A `power=line` way is a chain of mast nodes, and what stands at those nodes
/// is decided by three tags: `design=*` on the masts says what the mast picture
/// is, `voltage=*` on the way says how big it is, and `frequency=16.7` says it
/// is not a public grid line at all but the railway's own. Between them they
/// pick one of [`crate::power::PRESETS`], and the preset stamps the mast
/// objects, the crossarm heights and the conductor positions into the line —
/// the same trade the road import makes with its width.
///
/// **Every node of the way becomes a mast.** OSM tags the towers themselves,
/// but not always, and a bend in a power line without a mast at it is not a
/// thing that exists. So the geometry decides: every vertex carries a mast, and
/// a vertex the line turns more than fifteen degrees at — like both ends of the
/// way — carries a tension mast.
pub fn parse_power_lines(json: &str) -> Result<Vec<crate::route::PowerLineSource>, OsmError> {
    let response: OverpassResponse =
        serde_json::from_str(json).map_err(|e| OsmError::Json(e.to_string()))?;

    let mut nodes: HashMap<i64, (f64, f64)> = HashMap::new();
    let mut designs: HashMap<i64, String> = HashMap::new();
    for e in &response.elements {
        if let (Some(lat), Some(lon)) = (e.lat, e.lon) {
            nodes.insert(e.id, (lat, lon));
            if let Some(design) = e.tags.get("design") {
                designs.insert(e.id, design.clone());
            }
        }
    }

    let mut lines = Vec::new();
    for e in &response.elements {
        if e.kind != "way" {
            continue;
        }
        let Some(kind) = e.tags.get("power") else {
            continue;
        };
        if kind != "line" && kind != "minor_line" {
            continue;
        }
        let mut points: Vec<crate::route::PowerPoint> = e
            .nodes
            .iter()
            .filter_map(|id| nodes.get(id))
            .map(|&(lat, lon)| crate::route::PowerPoint {
                lat,
                lon,
                tension: false,
            })
            .collect();
        if points.len() < 2 {
            continue;
        }
        mark_tension(&mut points);

        // The mast picture the mapper wrote most often on this way's masts. A
        // line is one design from end to end in practice; where the mappers
        // disagree, the majority is the line.
        let design = majority_design(&e.nodes, &designs);
        let volts = voltage_kv(&e.tags);
        let railway = e
            .tags
            .get("frequency")
            .and_then(|f| f.parse::<f64>().ok())
            .is_some_and(|f| (10.0..20.0).contains(&f));
        let id = pick_type(design.as_deref(), volts, railway, kind == "minor_line");
        let Some(preset) = crate::power::preset(id) else {
            continue;
        };

        let mut tags = vec![id.to_string()];
        if let Some(kv) = volts {
            tags.push(format!("{kv}kv"));
        }
        if railway {
            tags.push("bahnstrom".to_string());
        }
        lines.push(crate::power::source_from(
            preset,
            name_of(&e.tags),
            points,
            tags,
        ));
    }
    Ok(lines)
}

/// Marks the masts that take the pull: both ends of the way, and every mast the
/// line turns hard at.
fn mark_tension(points: &mut [crate::route::PowerPoint]) {
    let n = points.len();
    let turns: Vec<bool> = (0..n)
        .map(|i| {
            if i == 0 || i + 1 == n {
                return true;
            }
            let before = crate::power::bearing(&points[i - 1], &points[i]);
            let after = crate::power::bearing(&points[i], &points[i + 1]);
            crate::power::turns_hard(before, after)
        })
        .collect();
    for (point, tension) in points.iter_mut().zip(turns) {
        point.tension = tension;
    }
}

/// The `design=*` most of a way's masts carry.
fn majority_design(way: &[i64], designs: &HashMap<i64, String>) -> Option<String> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for id in way {
        if let Some(design) = designs.get(id) {
            *counts.entry(design.as_str()).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .max_by_key(|&(design, count)| (count, std::cmp::Reverse(design)))
        .map(|(design, _)| design.to_string())
}

/// The line's voltage [kV]. `voltage=380000;110000` is a mast carrying both
/// levels; the biggest one decides what the mast looks like.
fn voltage_kv(tags: &HashMap<String, String>) -> Option<u32> {
    tags.get("voltage")?
        .split(';')
        .filter_map(|v| v.trim().parse::<u32>().ok())
        .max()
        .map(|volts| volts / 1000)
}

/// Which atlas type a way lands on.
///
/// Design first, voltage second — a `design=donau` at 220 kV is a Donaumast
/// built smaller, not a different shape. Where the mappers wrote no design at
/// all, which is most of the German network, the voltage alone decides and the
/// answer is a Donaumast: it is what the great majority of German lines
/// actually stand on.
fn pick_type(design: Option<&str>, volts: Option<u32>, railway: bool, minor: bool) -> &'static str {
    if railway {
        return match design {
            Some("two-level") => "bahnstrommast-110-zweiebenen",
            _ => "bahnstrommast-110",
        };
    }
    if minor {
        return match (design, volts) {
            (_, Some(0)) => "holzmast-nsp",
            (Some("triangle" | "asymmetric" | "armless_three-level"), _) => {
                "betonmast-20kv-dreieck"
            }
            _ => "betonmast-20kv-einebene",
        };
    }
    // 380, 220 or 110 — the three levels the German grid is built in.
    let level = match volts {
        Some(v) if v >= 300 => 380,
        Some(v) if v >= 180 => 220,
        _ => 110,
    };
    match (design, level) {
        (Some("barrel"), _) => "tonnenmast-380",
        (Some("three-level" | "four-level"), _) => "tannenbaummast-220",
        (Some("portal" | "h-frame"), _) => "portalmast-380",
        (Some("asymmetric" | "delta" | "y-frame"), _) => "kompaktmast-380",
        (Some("one-level"), 380) => "einebenenmast-380",
        (Some("one-level"), _) => "einebenenmast-110",
        (_, 380) => "donaumast-380",
        (_, 220) => "donaumast-220",
        (_, _) => "donaumast-110",
    }
}

/// The Overpass QL for the wind turbines of a box.
///
/// Both spellings are asked for: `generator:source=wind` is the tag that says
/// what drives the generator, `generator:method=wind_turbine` the one that says
/// how, and a turbine in the wild carries either or both. Ways come along
/// because a few turbines are mapped as the circle of their foundation rather
/// than as a point, and `(._;>;)` pulls the nodes those circles are made of in.
pub fn wind_query(min_lat: f64, min_lon: f64, max_lat: f64, max_lon: f64) -> String {
    // Overpass takes its box south, west, north, east.
    let bbox = format!("{min_lat:.6},{min_lon:.6},{max_lat:.6},{max_lon:.6}");
    format!(
        "[out:json][timeout:120];(\
         node[\"generator:source\"=\"wind\"]({bbox});\
         way[\"generator:source\"=\"wind\"]({bbox});\
         node[\"generator:method\"=\"wind_turbine\"]({bbox});\
         way[\"generator:method\"=\"wind_turbine\"]({bbox});\
         );(._;>;);out body;"
    )
}

/// The wind turbines of an Overpass extract.
///
/// What OSM reliably has is the **position**: somebody stood at the foot of the
/// tower. What it has less often is the machine — over two German boxes of a
/// thousand turbines, `manufacturer` and `model` were on a third to a half of
/// them, `height:hub` on a fifth and `rotor:diameter` on one in fourteen. So
/// what is read here is everything the mapper wrote, and the gaps are left for
/// the register to fill ([`crate::wind::match_register`]); where nothing fills
/// them, the rated power gives the dimensions ([`crate::wind::estimate`]) and
/// the turbine is tagged `estimated` so the file says so.
///
/// `ref:mastr` is the tag that matters most and reads like nothing:
/// half the turbines carry their number in the Marktstammdatenregister, which
/// is the machine's identity and turns the match with the register from a guess
/// at a distance into a lookup.
// ponytail: `height=*` is deliberately not read, although it is on more
// turbines than `height:hub` is. Where a mapper wrote both, `height` is the tip
// height and the two agree to the metre (119 + 112/2 = 175). Where only
// `height` is there, it is as often the hub height: an MM82 tagged `height=59`
// has a hub of 59 and a tip of 100, and an E-101 tagged `height=135` stands on
// a 135 m tower. A tag that means two things cannot be turned into one number,
// and a hub height that is 40 m out is a turbine of the wrong generation.
pub fn parse_wind_turbines(json: &str) -> Result<Vec<WindTurbineSource>, OsmError> {
    let response: OverpassResponse =
        serde_json::from_str(json).map_err(|e| OsmError::Json(e.to_string()))?;

    let mut nodes: HashMap<i64, (f64, f64)> = HashMap::new();
    for e in &response.elements {
        if let (Some(lat), Some(lon)) = (e.lat, e.lon) {
            nodes.insert(e.id, (lat, lon));
        }
    }

    let mut out = Vec::new();
    for e in &response.elements {
        if !is_wind_turbine(&e.tags) {
            continue;
        }
        // A turbine mapped as its foundation becomes the middle of it — the
        // same trade the marker import makes with a platform.
        let Some((lat, lon)) = position(e, &nodes) else {
            continue;
        };

        let hub = metres(e.tags.get("height:hub")).unwrap_or(0.0);
        let rotor = metres(e.tags.get("rotor:diameter")).unwrap_or(0.0);
        let power = power_kw(e.tags.get("generator:output:electricity")).unwrap_or(0.0);
        // The estimate fills whichever of the two the mapper left out; a
        // turbine both of whose numbers are surveyed is not estimated at all.
        let (guessed_hub, guessed_rotor) = crate::wind::estimate(power);
        let estimated = hub <= 0.0 || rotor <= 0.0;
        let rotor = if rotor > 0.0 { rotor } else { guessed_rotor };
        let hub = if hub > 0.0 { hub } else { guessed_hub };

        out.push(crate::wind::source_from(
            lat,
            lon,
            hub,
            rotor,
            machine(&e.tags),
            e.tags.get("ref:mastr").cloned().unwrap_or_default(),
            estimated,
        ));
    }
    Ok(out)
}

/// Whether an element's tags say it is a wind turbine. A `power=plant` area
/// carrying `plant:source=wind` is the wind farm around them, not a machine,
/// and is left where it is.
fn is_wind_turbine(tags: &HashMap<String, String>) -> bool {
    let generator = tags.get("power").map(String::as_str) == Some("generator")
        || tags.contains_key("generator:source")
        || tags.contains_key("generator:method");
    generator
        && (tags.get("generator:source").map(String::as_str) == Some("wind")
            || tags.get("generator:method").map(String::as_str) == Some("wind_turbine"))
}

/// Where an element stands: its own coordinates, or the middle of the way's
/// nodes.
fn position(element: &Element, nodes: &HashMap<i64, (f64, f64)>) -> Option<(f64, f64)> {
    if let (Some(lat), Some(lon)) = (element.lat, element.lon) {
        return Some((lat, lon));
    }
    // A closed way repeats its first node at the end; counting it twice would
    // pull the middle towards that corner.
    let mut ids = element.nodes.as_slice();
    if ids.len() > 1 && ids.first() == ids.last() {
        ids = &ids[..ids.len() - 1];
    }
    let ring: Vec<(f64, f64)> = ids.iter().filter_map(|id| nodes.get(id).copied()).collect();
    if ring.is_empty() {
        return None;
    }
    let n = ring.len() as f64;
    Some((
        ring.iter().map(|p| p.0).sum::<f64>() / n,
        ring.iter().map(|p| p.1).sum::<f64>() / n,
    ))
}

/// The machine's name out of `manufacturer=*` and `model=*` — `Enercon E-115
/// EP3`. A model that already carries the manufacturer does not get it twice,
/// and either tag alone is still worth writing down.
fn machine(tags: &HashMap<String, String>) -> String {
    let make = tags.get("manufacturer").map(String::as_str).unwrap_or("");
    let model = tags.get("model").map(String::as_str).unwrap_or("");
    let (make, model) = (make.trim(), model.trim());
    if make.is_empty() {
        return model.to_string();
    }
    if model.is_empty() {
        return make.to_string();
    }
    let head = format!("{make} ");
    if model.len() > head.len() && model[..head.len()].eq_ignore_ascii_case(&head) {
        return model.to_string();
    }
    format!("{make} {model}")
}

/// A tag that names a length in metres: `112`, `112 m`, `112.5`. The unit is
/// written about as often as it is left out, and it is always metres.
fn metres(value: Option<&String>) -> Option<f64> {
    let text = value?.trim();
    let number: String = text
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == ',')
        .collect();
    number
        .replace(',', ".")
        .parse::<f64>()
        .ok()
        .filter(|v| *v > 0.0)
}

/// The rated power a `generator:output:electricity` names [kW]. The tag is
/// `4.2 MW` as often as `3200 kW`, and `yes` often enough to be worth passing
/// over rather than reading as a number.
fn power_kw(value: Option<&String>) -> Option<f64> {
    let text = value?.trim();
    let number: String = text
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == ',')
        .collect();
    let amount = number.replace(',', ".").parse::<f64>().ok()?;
    let unit = text[number.len()..].trim().to_ascii_lowercase();
    let factor = match unit.as_str() {
        "mw" => 1_000.0,
        "kw" | "" => 1.0,
        "w" => 0.001,
        // A unit nobody writes on a wind turbine: better no number than a
        // number that is out by a thousand.
        _ => return None,
    };
    Some(amount * factor).filter(|v| *v > 0.0)
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
    fn water_bodies_come_out_of_overpass_json() {
        let json = r#"{"elements": [
            {"type": "node", "id": 1, "lat": 52.0, "lon": 10.0},
            {"type": "node", "id": 2, "lat": 52.0, "lon": 10.01},
            {"type": "node", "id": 3, "lat": 52.01, "lon": 10.01},
            {"type": "node", "id": 4, "lat": 52.01, "lon": 10.0},
            {"type": "way", "id": 10, "nodes": [1, 2, 3, 4, 1],
             "tags": {"natural": "water", "water": "pond", "name": "Mühlteich"}},
            {"type": "way", "id": 11, "nodes": [1, 2, 3, 4, 1], "tags": {"waterway": "riverbank"}},
            {"type": "way", "id": 12, "nodes": [1, 2, 3, 4, 1], "tags": {"waterway": "river"}},
            {"type": "way", "id": 13, "nodes": [1, 2, 3, 4, 1], "tags": {"landuse": "reservoir"}},
            {"type": "way", "id": 14, "nodes": [1, 2, 1], "tags": {"natural": "water"}}
        ]}"#;
        let waters = parse_water(json).expect("parses");

        // The centre-line river is skipped, the two-point ring dropped, the
        // closing node removed, the name and the matched family carried.
        assert_eq!(waters.len(), 3);
        assert_eq!(waters[0].name, "Mühlteich");
        assert_eq!(waters[0].tags, vec!["water"]);
        assert_eq!(waters[0].polygon.len(), 4);
        assert_eq!(waters[1].tags, vec!["riverbank"]);
        assert_eq!(waters[2].tags, vec!["reservoir"]);

        // A water-free extract is empty, not an error.
        assert_eq!(parse_water(r#"{"elements": []}"#), Ok(vec![]));
    }

    /// A lake mapped as a multipolygon: the outer split over two ways, an
    /// island as the inner. The members chain into one ring, the island
    /// becomes a hole of it, and the member ways are not read again on
    /// their own.
    #[test]
    fn a_multipolygon_assembles_into_a_body_with_its_island() {
        let json = r#"{"elements": [
            {"type": "node", "id": 1, "lat": 52.00, "lon": 10.00},
            {"type": "node", "id": 2, "lat": 52.00, "lon": 10.04},
            {"type": "node", "id": 3, "lat": 52.02, "lon": 10.04},
            {"type": "node", "id": 4, "lat": 52.02, "lon": 10.00},
            {"type": "node", "id": 5, "lat": 52.005, "lon": 10.015},
            {"type": "node", "id": 6, "lat": 52.015, "lon": 10.02},
            {"type": "node", "id": 7, "lat": 52.005, "lon": 10.025},
            {"type": "relation", "id": 90, "members": [
                {"type": "way", "ref": 20, "role": "outer"},
                {"type": "way", "ref": 21, "role": "outer"},
                {"type": "way", "ref": 22, "role": "inner"}
             ],
             "tags": {"type": "multipolygon", "natural": "water", "name": "Plöner See"}},
            {"type": "way", "id": 20, "nodes": [1, 2]},
            {"type": "way", "id": 21, "nodes": [2, 3, 4, 1]},
            {"type": "way", "id": 22, "nodes": [5, 6, 7, 5]}
        ]}"#;
        let waters = parse_water(json).expect("parses");

        // One body, not three: the member ways belong to the relation.
        assert_eq!(waters.len(), 1, "{waters:?}");
        let lake = &waters[0];
        assert_eq!(lake.name, "Plöner See");
        // The outer chained over the shared node 2: 1, 2, 3, 4.
        assert_eq!(lake.polygon.len(), 4);
        assert_eq!(lake.polygon[0].lat, 52.00);
        // The island is the lake's one hole, with the closing node dropped.
        assert_eq!(lake.holes.len(), 1);
        assert_eq!(lake.holes[0].len(), 3);
        // And the hole is where the island is: open water is water, the
        // island is not, and the outside is neither.
        assert!(lake.contains(52.01, 10.005), "open water misses");
        assert!(!lake.contains(52.01, 10.02), "the island centre is water");
        assert!(!lake.contains(52.03, 10.02), "the outside is water");
    }

    /// A braided river mapped as one relation with two outer rings: two
    /// bodies, and an inner is glued to the braid that holds it, not to
    /// the other one.
    #[test]
    fn two_outers_are_two_bodies_and_inners_follow_their_braid() {
        let json = r#"{"elements": [
            {"type": "node", "id": 1, "lat": 52.00, "lon": 10.00},
            {"type": "node", "id": 2, "lat": 52.00, "lon": 10.01},
            {"type": "node", "id": 3, "lat": 52.01, "lon": 10.01},
            {"type": "node", "id": 4, "lat": 52.01, "lon": 10.00},
            {"type": "node", "id": 5, "lat": 52.02, "lon": 10.00},
            {"type": "node", "id": 6, "lat": 52.02, "lon": 10.01},
            {"type": "node", "id": 7, "lat": 52.03, "lon": 10.01},
            {"type": "node", "id": 8, "lat": 52.03, "lon": 10.00},
            {"type": "node", "id": 9, "lat": 52.004, "lon": 10.005},
            {"type": "node", "id": 10, "lat": 52.0045, "lon": 10.006},
            {"type": "node", "id": 11, "lat": 52.0035, "lon": 10.006},
            {"type": "relation", "id": 91, "members": [
                {"type": "way", "ref": 30, "role": "outer"},
                {"type": "way", "ref": 31, "role": "outer"},
                {"type": "way", "ref": 32, "role": "inner"}
             ],
             "tags": {"type": "multipolygon", "waterway": "riverbank"}},
            {"type": "way", "id": 30, "nodes": [1, 2, 3, 4, 1]},
            {"type": "way", "id": 31, "nodes": [5, 6, 7, 8, 5]},
            {"type": "way", "id": 32, "nodes": [9, 10, 11, 9]}
        ]}"#;
        let waters = parse_water(json).expect("parses");
        assert_eq!(waters.len(), 2, "{waters:?}");
        // The southern braid carries the (tiny) island; the northern one
        // has none.
        let with_island = waters
            .iter()
            .find(|w| !w.holes.is_empty())
            .expect("one braid holds the island");
        assert!((with_island.polygon[0].lat - 52.00).abs() < 1e-9);
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

    /// A Bundesstraße, a one-way motorway carriageway and a farm track come
    /// out as three roads with their own widths, surfaces and markings; the
    /// footway is not a road at all.
    /// A roundabout carries no centre line, whether or not the mapper wrote
    /// `oneway`: `junction=roundabout` says it, and a ring striped down the
    /// middle is the one marking no German roundabout has.
    #[test]
    fn a_roundabout_is_one_carriageway() {
        let json = r#"{"elements": [
            {"type": "node", "id": 1, "lat": 52.0000, "lon": 10.0000},
            {"type": "node", "id": 2, "lat": 52.0002, "lon": 10.0002},
            {"type": "node", "id": 3, "lat": 52.0000, "lon": 10.0004},
            {"type": "way", "id": 20, "nodes": [1, 2, 3, 1],
             "tags": {"highway": "tertiary", "junction": "roundabout"}},
            {"type": "way", "id": 21, "nodes": [1, 3],
             "tags": {"highway": "tertiary"}}
        ]}"#;
        let roads = parse_roads(json).expect("parses");
        assert_eq!(roads.len(), 2);
        assert_eq!(roads[0].center_line, CenterLine::None, "the ring");
        assert_eq!(
            roads[1].center_line,
            CenterLine::Dashed,
            "the road that leaves it"
        );
    }

    #[test]
    fn roads_come_out_of_overpass_json() {
        let json = r#"{"elements": [
            {"type": "node", "id": 1, "lat": 52.0, "lon": 10.0},
            {"type": "node", "id": 2, "lat": 52.0, "lon": 10.01},
            {"type": "node", "id": 3, "lat": 52.01, "lon": 10.01},
            {"type": "node", "id": 4, "lat": 52.01, "lon": 10.0},
            {"type": "way", "id": 10, "nodes": [1, 2],
             "tags": {"highway": "primary", "name": "Bördestraße", "surface": "asphalt"}},
            {"type": "way", "id": 11, "nodes": [2, 3],
             "tags": {"highway": "motorway", "oneway": "yes", "lanes": "3", "bridge": "viaduct"}},
            {"type": "way", "id": 12, "nodes": [3, 4],
             "tags": {"highway": "track", "surface": "concrete:plates", "width": "3", "bridge": "no"}},
            {"type": "way", "id": 13, "nodes": [1, 4],
             "tags": {"highway": "footway"}},
            {"type": "way", "id": 14, "nodes": [1, 4],
             "tags": {"highway": "residential", "width": "7 m"}}
        ]}"#;
        let roads = parse_roads(json).expect("parses");

        // The footway is not a road; the other four are.
        assert_eq!(roads.len(), 4);
        let primary = &roads[0];
        assert_eq!(primary.name, "Bördestraße");
        assert_eq!(primary.tags, vec!["highway-primary"]);
        assert_eq!(primary.surface, RoadSurface::Asphalt);
        assert_eq!(primary.center_line, CenterLine::Dashed);
        assert!((primary.width - 7.5).abs() < 1e-9, "the preset's width");
        assert!(!primary.bridge, "no bridge tag, no bridge");

        let motorway = &roads[1];
        // One-way: one carriageway, three lanes of 3.5 m, no centre line.
        assert_eq!(motorway.center_line, CenterLine::None);
        assert!((motorway.width - 10.5).abs() < 1e-9, "{}", motorway.width);
        // The viaduct flies.
        assert!(motorway.bridge, "bridge=viaduct");

        let track = &roads[2];
        assert_eq!(track.surface, RoadSurface::Concrete, "the plates");
        assert!((track.width - 3.0).abs() < 1e-9, "the mapper's width");
        assert!(!track.bridge, "bridge=no is no bridge");

        let residential = &roads[3];
        assert!(
            (residential.width - 7.0).abs() < 1e-9,
            "{}",
            residential.width
        );
        // A residential street is innerorts: the shorter RMS dash.
        assert_eq!(residential.center_line, CenterLine::DashedUrban);

        assert_eq!(parse_roads(r#"{"elements": []}"#), Ok(vec![]));
    }

    /// A field track is unpaved until the mapper says otherwise. The class
    /// alone paves nothing: `highway=track` is what the way is used for, and
    /// the German ones are gravel far more often than concrete. What decides
    /// is `surface=*` first, then `tracktype=*` — and a loose surface takes
    /// the markings out, whatever the class would have painted.
    #[test]
    fn a_field_track_is_unpaved_unless_the_tags_say_so() {
        let json = r#"{"elements": [
            {"type": "node", "id": 1, "lat": 52.0, "lon": 10.0},
            {"type": "node", "id": 2, "lat": 52.0, "lon": 10.01},
            {"type": "way", "id": 10, "nodes": [1, 2], "tags": {"highway": "track"}},
            {"type": "way", "id": 11, "nodes": [1, 2],
             "tags": {"highway": "track", "tracktype": "grade3"}},
            {"type": "way", "id": 12, "nodes": [1, 2],
             "tags": {"highway": "track", "tracktype": "grade1"}},
            {"type": "way", "id": 13, "nodes": [1, 2],
             "tags": {"highway": "track", "tracktype": "grade1", "surface": "gravel"}},
            {"type": "way", "id": 14, "nodes": [1, 2],
             "tags": {"highway": "track", "surface": "Compacted"}},
            {"type": "way", "id": 15, "nodes": [1, 2],
             "tags": {"highway": "residential", "surface": "gravel;dirt"}},
            {"type": "way", "id": 16, "nodes": [1, 2],
             "tags": {"highway": "residential", "surface": "sett"}},
            {"type": "way", "id": 17, "nodes": [1, 2],
             "tags": {"highway": "secondary", "surface": "something_new"}}
        ]}"#;
        let roads = parse_roads(json).expect("parses");
        assert_eq!(roads.len(), 8);

        // A bare track: gravel, three metres, nothing painted on it.
        assert_eq!(roads[0].surface, RoadSurface::Gravel);
        assert!((roads[0].width - 3.0).abs() < 1e-9);
        assert_eq!(roads[0].center_line, CenterLine::None);
        assert!(!roads[0].edge_lines);

        assert_eq!(roads[1].surface, RoadSurface::Gravel, "grade3 is loose");
        assert_eq!(roads[2].surface, RoadSurface::Asphalt, "grade1 is solid");
        assert_eq!(
            roads[3].surface,
            RoadSurface::Gravel,
            "the surface the mapper wrote beats the grade"
        );
        assert_eq!(roads[4].surface, RoadSurface::Gravel, "case is no matter");

        // An unpaved village lane is a gravel lane, not a striped street.
        assert_eq!(roads[5].surface, RoadSurface::Gravel, "the first value");
        assert_eq!(roads[5].center_line, CenterLine::None);
        assert!(!roads[5].edge_lines);

        // Setts are hard and pale: concrete, and marked like the class says.
        assert_eq!(roads[6].surface, RoadSurface::Concrete);
        assert_eq!(roads[6].center_line, CenterLine::DashedUrban);
        assert!(roads[6].edge_lines);

        // A value the table does not know leaves the class preset in charge.
        assert_eq!(roads[7].surface, RoadSurface::Asphalt, "the preset's own");
    }

    /// A road cut by the extract at the box stays whole — the two corners
    /// the extract carried are the road, and the neighbour imports the rest.
    #[test]
    fn a_road_cut_at_the_box_is_whole_by_the_corner_it_has() {
        let json = r#"{"elements": [
            {"type": "node", "id": 1, "lat": 52.0, "lon": 10.0},
            {"type": "node", "id": 2, "lat": 52.0, "lon": 10.01},
            {"type": "way", "id": 20, "nodes": [1, 2], "tags": {"highway": "primary"}}
        ]}"#;
        let roads = parse_roads(json).expect("parses");
        assert_eq!(roads.len(), 1);
        assert_eq!(roads[0].points.len(), 2);
    }

    /// A 380 kV way with `design=donau` on its masts becomes a line of
    /// Donaumasten, with the objects and the crossarms of the atlas entry.
    #[test]
    fn a_donau_way_becomes_donaumasten() {
        let json = r#"{"elements": [
            {"type": "node", "id": 1, "lat": 52.0, "lon": 10.0, "tags": {"power": "tower", "design": "donau"}},
            {"type": "node", "id": 2, "lat": 52.0, "lon": 10.004, "tags": {"power": "tower", "design": "donau"}},
            {"type": "node", "id": 3, "lat": 52.0, "lon": 10.008, "tags": {"power": "tower", "design": "donau"}},
            {"type": "way", "id": 10, "nodes": [1, 2, 3],
             "tags": {"power": "line", "voltage": "380000", "name": "Nord-Sued"}}
        ]}"#;
        let lines = parse_power_lines(json).expect("parses");
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        assert_eq!(line.name, "Nord-Sued");
        assert_eq!(line.object, "pylons:donaumast_380_trag");
        assert_eq!(line.points.len(), 3);
        assert_eq!(line.arms.len(), 2, "the Donaumast's two crossarms");
        assert!(line.tags.contains(&"donaumast-380".to_string()));
        assert!(line.tags.contains(&"380kv".to_string()));
        // The ends take the pull, the straight mast in between does not.
        assert!(line.points[0].tension);
        assert!(!line.points[1].tension);
        assert!(line.points[2].tension);
    }

    /// A way at 16.7 Hz is the railway's own line, whatever its voltage says —
    /// four conductors on one crossarm, not six on two.
    #[test]
    fn a_16_7_hz_line_is_a_bahnstrom_line() {
        let json = r#"{"elements": [
            {"type": "node", "id": 1, "lat": 52.0, "lon": 10.0},
            {"type": "node", "id": 2, "lat": 52.0, "lon": 10.004},
            {"type": "way", "id": 10, "nodes": [1, 2],
             "tags": {"power": "line", "voltage": "110000", "frequency": "16.7"}}
        ]}"#;
        let lines = parse_power_lines(json).expect("parses");
        assert_eq!(lines[0].object, "pylons:bahnstrommast_110_trag");
        assert_eq!(lines[0].arms.len(), 1);
        assert_eq!(lines[0].arms[0].conductors, 4);
        assert!(lines[0].tags.contains(&"bahnstrom".to_string()));
    }

    /// `power=minor_line` is the medium-voltage grid: a concrete pole, and the
    /// pole picture follows the design the mappers wrote.
    #[test]
    fn a_minor_line_is_a_concrete_pole_line() {
        let json = r#"{"elements": [
            {"type": "node", "id": 1, "lat": 52.0, "lon": 10.0, "tags": {"power": "pole", "design": "triangle"}},
            {"type": "node", "id": 2, "lat": 52.0, "lon": 10.001, "tags": {"power": "pole", "design": "triangle"}},
            {"type": "way", "id": 10, "nodes": [1, 2], "tags": {"power": "minor_line"}}
        ]}"#;
        let lines = parse_power_lines(json).expect("parses");
        assert_eq!(lines[0].object, "pylons:betonmast_20kv_dreieck_trag");
        assert!(lines[0].height < 15.0, "a pole, not a tower");
    }

    /// No `design=*` anywhere, which is most of the German network: the voltage
    /// alone decides, and the answer is the mast most German lines stand on.
    #[test]
    fn an_untagged_way_falls_back_on_the_voltage() {
        let way = |voltage: &str| {
            format!(
                r#"{{"elements": [
                {{"type": "node", "id": 1, "lat": 52.0, "lon": 10.0}},
                {{"type": "node", "id": 2, "lat": 52.0, "lon": 10.004}},
                {{"type": "way", "id": 10, "nodes": [1, 2],
                  "tags": {{"power": "line", "voltage": "{voltage}"}}}}
            ]}}"#
            )
        };
        for (voltage, object) in [
            ("380000", "pylons:donaumast_380_trag"),
            ("220000", "pylons:donaumast_220_trag"),
            ("110000", "pylons:donaumast_110_trag"),
            // A mast carrying two levels is drawn as the bigger of them.
            ("380000;110000", "pylons:donaumast_380_trag"),
        ] {
            let lines = parse_power_lines(&way(voltage)).expect("parses");
            assert_eq!(lines[0].object, object, "at {voltage} V");
        }
    }

    /// A hard corner needs a mast built to be pulled sideways; a degree or two
    /// does not.
    #[test]
    fn the_line_turns_on_a_tension_mast() {
        let json = r#"{"elements": [
            {"type": "node", "id": 1, "lat": 52.0, "lon": 10.0},
            {"type": "node", "id": 2, "lat": 52.0, "lon": 10.004},
            {"type": "node", "id": 3, "lat": 52.004, "lon": 10.004},
            {"type": "node", "id": 4, "lat": 52.008, "lon": 10.00405},
            {"type": "way", "id": 10, "nodes": [1, 2, 3, 4],
             "tags": {"power": "line", "voltage": "110000"}}
        ]}"#;
        let lines = parse_power_lines(json).expect("parses");
        let points = &lines[0].points;
        assert!(points[1].tension, "the right angle");
        assert!(!points[2].tension, "a degree or two is not a corner");
    }

    /// Nothing power-related in the extract, and nothing comes back.
    #[test]
    fn an_extract_without_power_lines_yields_none() {
        assert_eq!(parse_power_lines(r#"{"elements": []}"#), Ok(vec![]));
    }

    /// The query asks for the two way classes the parser reads, and for their
    /// nodes — the design tags are on the nodes, not on the way.
    #[test]
    fn the_power_query_asks_for_ways_and_their_nodes() {
        let query = power_query(52.0, 10.0, 52.1, 10.1);
        assert!(query.contains("power"));
        assert!(query.contains("line|minor_line"));
        assert!(query.contains("(._;>;)"), "the nodes come too");
        assert!(query.contains("52.000000,10.000000,52.100000,10.100000"));
    }

    /// A turbine as the mappers of Dithmarschen write one, one with nothing but
    /// its power, one mapped as its foundation, and a wind farm outline that is
    /// not a machine.
    #[test]
    fn wind_turbines_come_out_of_overpass_json() {
        let json = r#"{"elements": [
            {"type": "node", "id": 1, "lat": 54.2683, "lon": 9.0135, "tags": {
                "power": "generator", "generator:source": "wind",
                "generator:method": "wind_turbine", "generator:type": "horizontal_axis",
                "manufacturer": "Enercon", "model": "E-70 E4",
                "generator:output:electricity": "2.3 MW",
                "height:hub": "64", "rotor:diameter": "71 m",
                "ref:mastr": "SEE945374201878"}},
            {"type": "node", "id": 2, "lat": 54.2, "lon": 9.0, "tags": {
                "power": "generator", "generator:source": "wind",
                "generator:output:electricity": "3450 kW"}},
            {"type": "node", "id": 10, "lat": 54.10, "lon": 9.10},
            {"type": "node", "id": 11, "lat": 54.10, "lon": 9.12},
            {"type": "node", "id": 12, "lat": 54.12, "lon": 9.12},
            {"type": "node", "id": 13, "lat": 54.12, "lon": 9.10},
            {"type": "way", "id": 20, "nodes": [10, 11, 12, 13, 10], "tags": {
                "power": "generator", "generator:source": "wind"}},
            {"type": "way", "id": 21, "nodes": [10, 11, 12, 13, 10], "tags": {
                "power": "plant", "plant:source": "wind"}}
        ]}"#;
        let turbines = parse_wind_turbines(json).expect("parses");
        // The farm outline is not a machine; the foundation ring is one.
        assert_eq!(turbines.len(), 3);

        let surveyed = &turbines[0];
        assert_eq!(surveyed.model, "Enercon E-70 E4");
        assert_eq!(surveyed.mastr, "SEE945374201878");
        assert_eq!(surveyed.hub_height, 64.0);
        // The unit is written on the tag as often as it is left off.
        assert_eq!(surveyed.rotor_diameter, 71.0);
        assert_eq!(surveyed.yaw_deg, crate::wind::PREVAILING_BEARING);
        assert_eq!(surveyed.tags, vec!["wea-80"]);
        // An Enercon of the 2 MW class stands on the Enercon build of it.
        assert_eq!(surveyed.object, "wind:wea_80_enercon");

        // Nothing but a rated power: the dimensions are worked out and the
        // file says so.
        let guessed = &turbines[1];
        assert!(guessed.tags.iter().any(|t| t == "estimated"));
        assert!((100.0..120.0).contains(&guessed.rotor_diameter));
        assert!(guessed.model.is_empty());

        // The foundation ring becomes its middle.
        let ring = &turbines[2];
        assert!((ring.lat - 54.11).abs() < 1e-9);
        assert!((ring.lon - 9.11).abs() < 1e-9);

        // A turbine-free extract is empty, not an error.
        assert_eq!(parse_wind_turbines(r#"{"elements": []}"#), Ok(vec![]));
    }

    /// The tag values the German extracts actually carry.
    #[test]
    fn the_turbine_tags_are_read_as_they_are_written() {
        assert_eq!(power_kw(Some(&"4.2 MW".to_string())), Some(4200.0));
        assert_eq!(power_kw(Some(&"3200 kW".to_string())), Some(3200.0));
        assert_eq!(power_kw(Some(&"600kW".to_string())), Some(600.0));
        assert_eq!(power_kw(Some(&"2000".to_string())), Some(2000.0));
        // "yes" says a turbine produces electricity, which was never in doubt.
        assert_eq!(power_kw(Some(&"yes".to_string())), None);
        assert_eq!(metres(Some(&"112 m".to_string())), Some(112.0));
        assert_eq!(metres(Some(&"122.5".to_string())), Some(122.5));
        assert_eq!(metres(Some(&"unknown".to_string())), None);
        // The manufacturer is not said twice.
        let mut tags = HashMap::new();
        tags.insert("manufacturer".to_string(), "Vestas".to_string());
        tags.insert("model".to_string(), "Vestas V112".to_string());
        assert_eq!(machine(&tags), "Vestas V112");
    }

    /// The query asks for both spellings of a turbine, and for the nodes of a
    /// turbine mapped as an area.
    #[test]
    fn the_wind_query_asks_for_both_spellings() {
        let query = wind_query(52.0, 10.0, 52.1, 10.1);
        assert!(query.contains("generator:source\"=\"wind"));
        assert!(query.contains("generator:method\"=\"wind_turbine"));
        assert!(query.contains("(._;>;)"), "the ways' nodes come too");
        assert!(query.contains("52.000000,10.000000,52.100000,10.100000"));
    }
}
