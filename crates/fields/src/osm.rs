//! The fallback: farmland out of OpenStreetMap.
//!
//! Rhineland-Palatinate publishes no InVeKoS service at all — the entry in the
//! BLE's register is empty, and the application portal is behind a login. A
//! module there would otherwise get no fields whatever, which is worse than
//! approximate ones (plan ch. 3).
//!
//! So: `landuse=farmland` and its neighbours out of Overpass. What that buys is
//! the *shape* of the countryside — which piece is farmed, which is meadow,
//! which is vineyard — surveyed by people who walked it, and usually generalised
//! rather than surveyed to the centimetre. What it does not buy is the crop: a
//! farmland polygon says "arable", so the crop is drawn from the regional
//! statistics exactly as it is for a state that publishes only field blocks
//! ([`crate::stats`]). Where a mapper *has* written `crop=*`, that is used.
//!
//! The same path is what a module outside Germany gets. The approach is
//! national by nature — the registers are national, so are their schemas and
//! their crop code lists — and OpenStreetMap is the one source that is not, so
//! a line in Austria or the Netherlands gets the shape of its countryside from
//! here until somebody reads that country's own register (plan ch. 9).
//!
//! **Licence.** OpenStreetMap is ODbL, which is share-alike: a module built on
//! it carries that obligation, and the import records it in the attribution
//! like every other source. The line import already reads OSM for track and
//! woodland, so this changes nothing about a line's standing — but it is the
//! reason the fallback is a fallback and not the first choice.

use crate::wfs::{RequestConfig, ServiceError, encode};
use glam::DVec2;
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;

/// Where the query goes. Overpass is donated capacity: the query below asks for
/// one module's worth of polygons, and the import caches what it gets.
pub const OVERPASS: &str = "https://overpass-api.de/api/interpreter";

/// One OSM way that is farmed, as the import needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct Parcel {
    /// The ring in degrees, `(lat, lon)`.
    pub ring: Vec<(f64, f64)>,
    /// The tags that decided what it is — `landuse`, `crop`, `produce`.
    pub tags: HashMap<String, String>,
    /// `way/12345`, so the parcel keeps an identity across imports and seeds
    /// its own crop draw.
    pub id: String,
}

/// The Overpass QL for the farmed land in a box.
///
/// Closed ways only. A multipolygon relation — a field with a wood in the
/// middle — is skipped, the same way [`crate::geometry`] drops holes; both
/// wait for a polygon type that has them.
pub fn query(min_lat: f64, min_lon: f64, max_lat: f64, max_lon: f64) -> String {
    // Overpass takes its box south, west, north, east.
    let bbox = format!("{min_lat:.6},{min_lon:.6},{max_lat:.6},{max_lon:.6}");
    format!(
        "[out:json][timeout:90];(\
         way[\"landuse\"~\"^(farmland|meadow|vineyard|orchard|greenhouse_horticulture|plant_nursery)$\"]({bbox});\
         way[\"natural\"=\"grassland\"]({bbox});\
         );(._;>;);out body;"
    )
}

/// How long to wait before asking Overpass again when it says it is busy.
///
/// Overpass is donated capacity and answers 429 or 504 under load — routinely,
/// and transiently. Its own etiquette is to back off and try again rather than
/// to give up, and one wait of a few seconds turns most of those into an answer.
const BUSY_WAIT: Duration = Duration::from_secs(4);

/// Whether an error is Overpass saying "not now" rather than "no".
fn busy(error: &ServiceError) -> bool {
    match error {
        ServiceError::Network(message) => {
            message.contains("429") || message.contains("504") || message.contains("503")
        }
        _ => false,
    }
}

/// Asks Overpass for the farmed land in a box, in degrees.
///
/// Once, and once more after a wait if the first answer was "busy" — see
/// [`BUSY_WAIT`].
pub fn fetch(
    min_lat: f64,
    min_lon: f64,
    max_lat: f64,
    max_lon: f64,
    config: &RequestConfig,
) -> Result<Vec<Parcel>, ServiceError> {
    match fetch_once(min_lat, min_lon, max_lat, max_lon, config) {
        Err(e) if busy(&e) => {
            std::thread::sleep(BUSY_WAIT);
            fetch_once(min_lat, min_lon, max_lat, max_lon, config)
        }
        other => other,
    }
}

fn fetch_once(
    min_lat: f64,
    min_lon: f64,
    max_lat: f64,
    max_lon: f64,
    config: &RequestConfig,
) -> Result<Vec<Parcel>, ServiceError> {
    let url = format!(
        "{OVERPASS}?data={}",
        encode(&query(min_lat, min_lon, max_lat, max_lon))
    );
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(config.timeout))
        .user_agent(&config.user_agent)
        .build()
        .into();
    let mut response = agent
        .get(&url)
        .call()
        .map_err(|e| ServiceError::Network(e.to_string()))?;
    let mut body = Vec::new();
    let read = std::io::Read::take(response.body_mut().as_reader(), config.max_bytes as u64 + 1);
    std::io::copy(&mut std::io::BufReader::new(read), &mut body)
        .map_err(|e| ServiceError::Network(e.to_string()))?;
    if body.len() > config.max_bytes {
        return Err(ServiceError::TooMuch);
    }
    parse(std::str::from_utf8(&body).map_err(|e| ServiceError::NotGeoJson(e.to_string()))?)
}

/// Reads an Overpass JSON answer.
pub fn parse(text: &str) -> Result<Vec<Parcel>, ServiceError> {
    let text = text.trim();
    if text.starts_with('<') {
        // Overpass answers HTML when it is out of capacity, and says so in the
        // body rather than the status.
        return Err(ServiceError::NotGeoJson(
            text.lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(200)
                .collect(),
        ));
    }
    let value: Value =
        serde_json::from_str(text).map_err(|e| ServiceError::NotGeoJson(e.to_string()))?;
    let Some(elements) = value.get("elements").and_then(Value::as_array) else {
        return Err(ServiceError::NotGeoJson("no elements".into()));
    };

    // Nodes first: a way is a list of references to them.
    let mut nodes: HashMap<i64, (f64, f64)> = HashMap::new();
    for element in elements {
        if element.get("type").and_then(Value::as_str) != Some("node") {
            continue;
        }
        let (Some(id), Some(lat), Some(lon)) = (
            element.get("id").and_then(Value::as_i64),
            element.get("lat").and_then(Value::as_f64),
            element.get("lon").and_then(Value::as_f64),
        ) else {
            continue;
        };
        nodes.insert(id, (lat, lon));
    }

    let mut out = Vec::new();
    for element in elements {
        if element.get("type").and_then(Value::as_str) != Some("way") {
            continue;
        }
        let Some(id) = element.get("id").and_then(Value::as_i64) else {
            continue;
        };
        let Some(refs) = element.get("nodes").and_then(Value::as_array) else {
            continue;
        };
        let tags: HashMap<String, String> = element
            .get("tags")
            .and_then(Value::as_object)
            .map(|map| {
                map.iter()
                    .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                    .collect()
            })
            .unwrap_or_default();
        // Only the ways that carry the tags: `(._;>;)` returns the nodes of
        // every way as bare nodes, and a few of those are tagged themselves.
        if !is_farmed(&tags) {
            continue;
        }
        let mut ring: Vec<(f64, f64)> = refs
            .iter()
            .filter_map(|r| nodes.get(&r.as_i64()?).copied())
            .collect();
        // A closed way repeats its first node; the rings here do not.
        if ring.len() > 1 && ring[0] == ring[ring.len() - 1] {
            ring.pop();
        }
        if ring.len() < 3 {
            continue;
        }
        out.push(Parcel {
            ring,
            tags,
            id: format!("way/{id}"),
        });
    }
    Ok(out)
}

/// Whether a way's tags say it is farmed.
fn is_farmed(tags: &HashMap<String, String>) -> bool {
    matches!(
        tags.get("landuse").map(String::as_str),
        Some(
            "farmland"
                | "meadow"
                | "vineyard"
                | "orchard"
                | "greenhouse_horticulture"
                | "plant_nursery"
        )
    ) || tags.get("natural").map(String::as_str) == Some("grassland")
}

/// Asks Overpass with a raw query and hands the JSON body back — what a
/// reader of another OSM layer (the route editor's road import, say) needs
/// when it has its own query and its own parser. One retry after a wait if
/// the first answer was "busy", the same etiquette [`fetch`] follows.
pub fn fetch_raw(query: &str, config: &RequestConfig) -> Result<String, ServiceError> {
    match fetch_raw_once(query, config) {
        Err(e) if busy(&e) => {
            std::thread::sleep(BUSY_WAIT);
            fetch_raw_once(query, config)
        }
        other => other,
    }
}

fn fetch_raw_once(query: &str, config: &RequestConfig) -> Result<String, ServiceError> {
    let url = format!("{OVERPASS}?data={}", encode(query));
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(config.timeout))
        .user_agent(&config.user_agent)
        .build()
        .into();
    let mut response = agent
        .get(&url)
        .call()
        .map_err(|e| ServiceError::Network(e.to_string()))?;
    let mut body = Vec::new();
    let read = std::io::Read::take(response.body_mut().as_reader(), config.max_bytes as u64 + 1);
    std::io::copy(&mut std::io::BufReader::new(read), &mut body)
        .map_err(|e| ServiceError::Network(e.to_string()))?;
    if body.len() > config.max_bytes {
        return Err(ServiceError::TooMuch);
    }
    String::from_utf8(body).map_err(|e| ServiceError::NotGeoJson(e.to_string()))
}

/// The parcel's ring in a UTM zone's metres.
pub fn ring_in_zone(parcel: &Parcel, zone: u8) -> Vec<DVec2> {
    parcel
        .ring
        .iter()
        .map(|(lat, lon)| {
            let (e, n) = world_coords::geo::to_utm(lat.to_radians(), lon.to_radians(), zone);
            DVec2::new(e, n)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"elements":[
        {"type":"node","id":1,"lat":49.900,"lon":8.100},
        {"type":"node","id":2,"lat":49.900,"lon":8.104},
        {"type":"node","id":3,"lat":49.903,"lon":8.104},
        {"type":"node","id":4,"lat":49.903,"lon":8.100},
        {"type":"node","id":9,"lat":49.901,"lon":8.101,"tags":{"natural":"tree"}},
        {"type":"way","id":100,"nodes":[1,2,3,4,1],"tags":{"landuse":"farmland"}},
        {"type":"way","id":101,"nodes":[1,2,3],"tags":{"landuse":"vineyard","crop":"grape"}},
        {"type":"way","id":102,"nodes":[1,2,3,4],"tags":{"highway":"track"}},
        {"type":"way","id":103,"nodes":[1,2],"tags":{"landuse":"meadow"}}
    ]}"#;

    #[test]
    fn the_query_carries_its_box_the_way_overpass_wants_it() {
        let q = query(49.9, 8.1, 50.0, 8.2);
        // South, west, north, east.
        assert!(q.contains("49.900000,8.100000,50.000000,8.200000"), "{q}");
        assert!(q.contains("farmland"), "{q}");
        assert!(q.contains("out:json"), "{q}");
    }

    #[test]
    fn farmed_ways_come_out_and_the_rest_stays_behind() {
        let parcels = parse(SAMPLE).expect("parses");
        let ids: Vec<&str> = parcels.iter().map(|p| p.id.as_str()).collect();
        // The track is not farmed; the meadow has two nodes and is no polygon.
        assert_eq!(ids, vec!["way/100", "way/101"]);
    }

    #[test]
    fn a_closed_way_loses_its_repeated_node() {
        let parcels = parse(SAMPLE).expect("parses");
        assert_eq!(parcels[0].ring.len(), 4);
        assert_ne!(parcels[0].ring[0], parcels[0].ring[3]);
    }

    #[test]
    fn the_tags_come_along_for_the_crop() {
        let parcels = parse(SAMPLE).expect("parses");
        assert_eq!(parcels[0].tags.get("landuse").unwrap(), "farmland");
        assert_eq!(parcels[1].tags.get("crop").unwrap(), "grape");
    }

    #[test]
    fn an_empty_extract_is_not_an_error() {
        assert!(parse(r#"{"elements":[]}"#).expect("parses").is_empty());
    }

    #[test]
    fn a_busy_overpass_is_worth_asking_again_and_a_broken_one_is_not() {
        assert!(busy(&ServiceError::Network("http status: 504".into())));
        assert!(busy(&ServiceError::Network("http status: 429".into())));
        assert!(!busy(&ServiceError::Network("dns error".into())));
        assert!(!busy(&ServiceError::NotGeoJson("nonsense".into())));
        assert!(!busy(&ServiceError::TooMuch));
    }

    #[test]
    fn an_overloaded_overpass_says_so() {
        let body = "<html><head><title>OSM3S Response</title></head>";
        match parse(body) {
            Err(ServiceError::NotGeoJson(hint)) => assert!(hint.contains("html"), "{hint}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_ring_lands_in_metres() {
        let parcels = parse(SAMPLE).expect("parses");
        let ring = ring_in_zone(&parcels[0], 32);
        assert_eq!(ring.len(), 4);
        // Rhine-Hesse in zone 32: eastings around 435 km, northings 5 528 km.
        assert!((430_000.0..445_000.0).contains(&ring[0].x), "{:?}", ring[0]);
        assert!(
            (5_520_000.0..5_535_000.0).contains(&ring[0].y),
            "{:?}",
            ring[0]
        );
    }
}
