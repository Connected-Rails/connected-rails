//! Asking a state's service for the parcels in a box.
//!
//! All the services that matter speak WFS 2.0 and can answer in GeoJSON, so
//! this is one request builder and one reader rather than a GML parser per
//! state. What differs between them is spelling — `GEOJSON` here,
//! `application/json` there — and that sits in [`crate::land::Service`].
//!
//! Two things learned the hard way, and the reason for the guards below:
//!
//! * **A request without a bounding box can answer with the whole state.** North
//!   Rhine-Westphalia's parcels are 150 MB, and the service happily starts
//!   sending them. Every request here carries a box, and the reader stops at
//!   [`RequestConfig::max_bytes`] whatever the server intends.
//! * **`+` in a URL is a space.** `application/geo+json` has to be percent-
//!   encoded or the service answers 400. Everything goes through [`encode`].

use crate::land::{Access, Land, Service};
use glam::DVec2;
use serde_json::Value;
use std::time::Duration;

/// What went wrong asking a service.
#[derive(Debug, Clone, PartialEq)]
pub enum ServiceError {
    /// The state publishes nothing to ask for.
    NotPublished(Land),
    /// The data exists but only as a whole-state download.
    NeedsDownload(Land),
    /// Network, DNS, TLS, timeout.
    Network(String),
    /// The service answered, with something other than GeoJSON — an OGC
    /// exception report, an HTML error page, a truncated body.
    NotGeoJson(String),
    /// The answer hit [`RequestConfig::max_bytes`]. The box is too big; the
    /// import halves it and asks again.
    TooMuch,
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServiceError::NotPublished(l) => write!(f, "{} publishes no field data", l.code()),
            ServiceError::NeedsDownload(l) => {
                write!(f, "{} only offers a whole-state download", l.code())
            }
            ServiceError::Network(e) => write!(f, "{e}"),
            ServiceError::NotGeoJson(e) => write!(f, "no GeoJSON came back: {e}"),
            ServiceError::TooMuch => write!(f, "the answer was too large for one request"),
        }
    }
}

/// How the services are talked to. The editor's setting, not a constant, so a
/// slow line can raise the timeout without a rebuild.
#[derive(Debug, Clone, PartialEq)]
pub struct RequestConfig {
    pub user_agent: String,
    pub timeout: Duration,
    /// Hard ceiling on one answer [bytes].
    ///
    /// This, rather than a feature count, is what bounds a request. A `COUNT`
    /// would make a service *truncate* a dense box silently; the ceiling makes
    /// it fail loudly, and [`crate::import`] quarters the box and asks again.
    pub max_bytes: usize,
}

impl Default for RequestConfig {
    fn default() -> Self {
        Self {
            // The services are public and unmetered, and an agent string that
            // says who is calling is what keeps them that way.
            user_agent: concat!("trainsim-fields/", env!("CARGO_PKG_VERSION")).into(),
            timeout: Duration::from_secs(60),
            // A box of a few square kilometres holds a few thousand parcels at
            // some hundreds of bytes each; 64 MB is well past that and well
            // short of a state.
            max_bytes: 64 * 1024 * 1024,
        }
    }
}

/// One feature as the service sent it, before anything is made of it.
#[derive(Debug, Clone, PartialEq)]
pub struct RawFeature {
    /// The outer rings. A multipolygon keeps all of them; holes are dropped
    /// (see [`crate::geometry::clip`]).
    pub rings: Vec<Vec<DVec2>>,
    /// A parcel published as a point rather than a polygon — Saxony.
    pub point: Option<DVec2>,
    pub properties: serde_json::Map<String, Value>,
}

impl RawFeature {
    /// A property by name, case-insensitively: the same field is `AREA_HA`,
    /// `declaredarea` and `declaredArea` in three services.
    pub fn get(&self, name: &str) -> Option<&Value> {
        if let Some(value) = self.properties.get(name) {
            return Some(value);
        }
        self.properties
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value)
    }

    /// A property as text, whatever JSON type it arrived as.
    pub fn text(&self, name: &str) -> Option<String> {
        match self.get(name)? {
            Value::String(s) => Some(s.clone()),
            Value::Null => None,
            other => Some(other.to_string()),
        }
    }

    /// A property as a number, including one that arrived as a string.
    pub fn number(&self, name: &str) -> Option<f64> {
        match self.get(name)? {
            Value::Number(n) => n.as_f64(),
            Value::String(s) => s.trim().replace(',', ".").parse().ok(),
            _ => None,
        }
    }

    /// A property as a flag. The services write `true`, `"true"`, `"TRUE"`,
    /// `"N"` and `"J"` for the same thing.
    pub fn flag(&self, name: &str) -> Option<bool> {
        match self.get(name)? {
            Value::Bool(b) => Some(*b),
            Value::String(s) => match s.trim().to_ascii_uppercase().as_str() {
                "TRUE" | "J" | "JA" | "Y" | "YES" | "1" => Some(true),
                "FALSE" | "N" | "NEIN" | "NO" | "0" => Some(false),
                _ => None,
            },
            Value::Number(n) => n.as_f64().map(|v| v != 0.0),
            _ => None,
        }
    }
}

/// Everything one request needs.
#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    pub land: Land,
    /// The box to ask for, in the state's own UTM zone [m].
    pub min: DVec2,
    pub max: DVec2,
}

/// Asks one state for the parcels in a box.
///
/// The whole answer is read into memory. A box the size of a module holds a few
/// thousand parcels, which is a few megabytes — streaming would buy nothing and
/// cost the ability to say "this is not GeoJSON" before parsing.
pub fn fetch(query: &Query, config: &RequestConfig) -> Result<Vec<RawFeature>, ServiceError> {
    let service = query.land.service();
    match service.access {
        Access::None => return Err(ServiceError::NotPublished(query.land)),
        Access::Download => return Err(ServiceError::NeedsDownload(query.land)),
        // OpenStreetMap is not a WFS and does not answer in metres — the
        // import takes it through `crate::osm` instead.
        Access::Osm => return Err(ServiceError::NotPublished(query.land)),
        Access::Wfs | Access::WfsPointJoin => {}
    }
    let body = get(&url(&service, service.layer, query), config)?;
    parse(&body)
}

/// The polygons a [`Access::WfsPointJoin`] state's points are joined against.
pub fn fetch_join(query: &Query, config: &RequestConfig) -> Result<Vec<RawFeature>, ServiceError> {
    let service = query.land.service();
    if service.access != Access::WfsPointJoin || service.join.is_empty() {
        return Ok(Vec::new());
    }
    let body = get(&url(&service, service.join, query), config)?;
    parse(&body)
}

/// The `GetFeature` URL for one layer and one box.
fn url(service: &Service, layer: &str, query: &Query) -> String {
    let crs = format!(
        "urn:ogc:def:crs:EPSG::{}",
        25800 + query.land.utm_zone() as u32
    );
    let bbox = format!(
        "{:.1},{:.1},{:.1},{:.1},{crs}",
        query.min.x, query.min.y, query.max.x, query.max.y
    );
    let mut out = String::from(service.url);
    out.push(if out.contains('?') { '&' } else { '?' });
    for (key, value) in [
        ("SERVICE", "WFS"),
        ("VERSION", "2.0.0"),
        ("REQUEST", "GetFeature"),
        ("TYPENAMES", layer),
        ("SRSNAME", crs.as_str()),
        ("BBOX", bbox.as_str()),
        ("OUTPUTFORMAT", service.format),
    ] {
        out.push_str(key);
        out.push('=');
        out.push_str(&encode(value));
        out.push('&');
    }
    out.pop();
    out
}

/// Percent-encoding for a query value. Written out rather than pulled in: the
/// set of characters that matter here is `+`, `:`, `,` and the space.
pub fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Fetches a URL, refusing to read past `max_bytes`.
fn get(url: &str, config: &RequestConfig) -> Result<Vec<u8>, ServiceError> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(config.timeout))
        .user_agent(&config.user_agent)
        .build()
        .into();
    let mut response = agent
        .get(url)
        .call()
        .map_err(|e| ServiceError::Network(e.to_string()))?;
    let mut body = Vec::new();
    // One byte over the ceiling, so a body that exactly fills it is still
    // recognised as truncated rather than parsed as if it were whole.
    let read = std::io::Read::take(response.body_mut().as_reader(), config.max_bytes as u64 + 1);
    std::io::copy(&mut std::io::BufReader::new(read), &mut body)
        .map_err(|e| ServiceError::Network(e.to_string()))?;
    if body.len() > config.max_bytes {
        return Err(ServiceError::TooMuch);
    }
    Ok(body)
}

/// Reads a GeoJSON `FeatureCollection`.
pub fn parse(body: &[u8]) -> Result<Vec<RawFeature>, ServiceError> {
    let text = std::str::from_utf8(body)
        .map_err(|e| ServiceError::NotGeoJson(e.to_string()))?
        .trim_start_matches('\u{feff}')
        .trim();
    if text.starts_with('<') {
        // An OGC exception report or an HTML error page. Its first line is
        // worth more than "expected value at line 1".
        let hint = text
            .lines()
            .find(|l| l.contains("Exception") || l.contains("<title>"))
            .unwrap_or(text.lines().next().unwrap_or(""))
            .trim();
        return Err(ServiceError::NotGeoJson(hint.chars().take(200).collect()));
    }
    let value: Value =
        serde_json::from_str(text).map_err(|e| ServiceError::NotGeoJson(e.to_string()))?;
    let Some(features) = value.get("features").and_then(Value::as_array) else {
        return Err(ServiceError::NotGeoJson(
            "no feature collection".to_string(),
        ));
    };
    Ok(features.iter().filter_map(feature).collect())
}

/// One GeoJSON feature. A feature whose geometry is missing or of a kind that
/// is not a parcel is skipped rather than failing the request — a service that
/// mixes them in should not cost the other five thousand.
fn feature(value: &Value) -> Option<RawFeature> {
    let properties = match value.get("properties") {
        Some(Value::Object(map)) => map.clone(),
        _ => serde_json::Map::new(),
    };
    // Some services put the id outside the properties.
    let mut properties = properties;
    if let Some(Value::String(id)) = value.get("id")
        && !properties.contains_key("id")
    {
        properties.insert("id".into(), Value::String(id.clone()));
    }
    let geometry = value.get("geometry")?;
    let kind = geometry.get("type")?.as_str()?;
    let coordinates = geometry.get("coordinates")?;
    let mut rings = Vec::new();
    let mut point = None;
    match kind {
        "Polygon" => rings.extend(polygon(coordinates)),
        "MultiPolygon" => {
            for part in coordinates.as_array()? {
                rings.extend(polygon(part));
            }
        }
        "Point" => point = pair(coordinates),
        "MultiPoint" => point = coordinates.as_array()?.first().and_then(pair),
        _ => return None,
    }
    if rings.is_empty() && point.is_none() {
        return None;
    }
    Some(RawFeature {
        rings,
        point,
        properties,
    })
}

/// The outer ring of a polygon. Inner rings — holes — are dropped.
fn polygon(value: &Value) -> Option<Vec<DVec2>> {
    let ring = value.as_array()?.first()?.as_array()?;
    let points: Vec<DVec2> = ring.iter().filter_map(pair).collect();
    (points.len() >= 3).then_some(points)
}

fn pair(value: &Value) -> Option<DVec2> {
    let a = value.as_array()?;
    Some(DVec2::new(a.first()?.as_f64()?, a.get(1)?.as_f64()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plus_signs_are_encoded() {
        // The bug that made Lower Saxony answer 400 to every request.
        assert_eq!(encode("application/geo+json"), "application%2Fgeo%2Bjson");
        assert_eq!(
            encode("urn:ogc:def:crs:EPSG::25832"),
            "urn%3Aogc%3Adef%3Acrs%3AEPSG%3A%3A25832"
        );
    }

    #[test]
    fn a_request_always_carries_its_box() {
        let query = Query {
            land: Land::Nw,
            min: DVec2::new(440_000.0, 5_715_000.0),
            max: DVec2::new(441_000.0, 5_716_000.0),
        };
        let service = Land::Nw.service();
        let url = url(&service, service.layer, &query);
        assert!(url.contains("BBOX="), "{url}");
        assert!(url.contains("440000.0"), "{url}");
        assert!(url.contains("25832"), "{url}");
        assert!(url.starts_with("https://www.wfs.nrw.de/umwelt/lwk_eufoerderung?"));
    }

    #[test]
    fn the_eastern_states_are_asked_in_zone_33() {
        let query = Query {
            land: Land::Sn,
            min: DVec2::new(350_000.0, 5_670_000.0),
            max: DVec2::new(351_000.0, 5_671_000.0),
        };
        let service = Land::Sn.service();
        assert!(url(&service, service.layer, &query).contains("25833"));
    }

    #[test]
    fn a_state_without_a_service_says_so() {
        let query = Query {
            land: Land::Rp,
            min: DVec2::ZERO,
            max: DVec2::ONE,
        };
        assert_eq!(
            fetch(&query, &RequestConfig::default()),
            Err(ServiceError::NotPublished(Land::Rp))
        );
        let query = Query {
            land: Land::Sh,
            ..query
        };
        assert_eq!(
            fetch(&query, &RequestConfig::default()),
            Err(ServiceError::NeedsDownload(Land::Sh))
        );
    }

    /// The shape North Rhine-Westphalia actually answers with.
    const NW_SAMPLE: &str = r#"{"type":"FeatureCollection","crs":{"type":"name","properties":{"name":"EPSG:25832"}},"features":[
{"type":"Feature","geometry":{"type":"MultiPolygon","coordinates":[[[[440079.9,5715549.1],[440081.3,5715546.1],[440115.7,5715463.3],[440079.9,5715549.1]]]]},"properties":{"OBJECTID":86994,"ID":9511274,"FLIK":"DENWLI0544140725","AREA_HA":6.07999992,"CODE":115,"CODE_TXT":"Winterweichweizen","USE_CODE":"GT","ORGANICFAR":"false","VALIDFROM":2026}}]}"#;

    #[test]
    fn north_rhine_westphalia_reads() {
        let features = parse(NW_SAMPLE.as_bytes()).expect("parses");
        assert_eq!(features.len(), 1);
        let f = &features[0];
        assert_eq!(f.rings.len(), 1);
        assert_eq!(f.rings[0].len(), 4);
        assert_eq!(f.text("CODE"), Some("115".into()));
        assert_eq!(f.text("CODE_TXT"), Some("Winterweichweizen".into()));
        assert_eq!(f.number("AREA_HA"), Some(6.07999992));
        assert_eq!(f.flag("ORGANICFAR"), Some(false));
        // Case does not matter: the same field is `AREA_HA` and `declaredarea`.
        assert_eq!(f.number("area_ha"), Some(6.07999992));
    }

    /// Lower Saxony's INSPIRE answer: the crop is a codelist URL.
    const NI_SAMPLE: &str = r#"{"type":"FeatureCollection","features":[{"type":"Feature","id":"GSA_AgriculturalParcel.89077","geometry":{"type":"MultiPolygon","coordinates":[[[[540290.4,5800247.9],[540243.4,5800266.2],[540235.4,5800268.2],[540290.4,5800247.9]]]]},"properties":{"gml_id":"DE.NI.IACS_GSA.AgriculturalParcel_DENILI2048500007_2025_134209","description":"https://registry.gdi-de.org/codelist/de.iacs/CropValue/HF","name":"Root crops","declaredarea":1.1769,"organicfarming":false,"validfrom":"2025-12-02"}}]}"#;

    #[test]
    fn lower_saxony_reads() {
        let features = parse(NI_SAMPLE.as_bytes()).expect("parses");
        let f = &features[0];
        assert_eq!(f.number("declaredarea"), Some(1.1769));
        assert_eq!(f.flag("organicfarming"), Some(false));
        assert!(f.text("description").unwrap().ends_with("/HF"));
        // The id sits outside the properties and is folded in.
        assert_eq!(f.text("id"), Some("GSA_AgriculturalParcel.89077".into()));
    }

    /// Saxony publishes the parcel as a point.
    const SN_SAMPLE: &str = r#"{"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"Point","coordinates":[350338.5,5670023.7]},"properties":{"id":"DE.SN.GSA.AP.456428000","declaredArea":3.5237,"organicFarming":"TRUE","mainCrop":"GL","validFrom":"01.01.2025"}}]}"#;

    #[test]
    fn saxony_reads_as_a_point() {
        let features = parse(SN_SAMPLE.as_bytes()).expect("parses");
        let f = &features[0];
        assert!(f.rings.is_empty());
        assert_eq!(f.point, Some(DVec2::new(350_338.5, 5_670_023.7)));
        assert_eq!(f.text("mainCrop"), Some("GL".into()));
        assert_eq!(f.flag("organicFarming"), Some(true));
    }

    #[test]
    fn an_exception_report_is_not_geojson() {
        let body = br#"<?xml version="1.0"?><ows:ExceptionReport><ows:Exception exceptionCode="InvalidParameterValue"/></ows:ExceptionReport>"#;
        match parse(body) {
            Err(ServiceError::NotGeoJson(hint)) => assert!(hint.contains("Exception"), "{hint}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_empty_collection_is_not_an_error() {
        let features = parse(br#"{"type":"FeatureCollection","features":[]}"#).expect("parses");
        assert!(features.is_empty());
    }

    #[test]
    fn a_feature_without_geometry_is_skipped_not_fatal() {
        let body = br#"{"type":"FeatureCollection","features":[
            {"type":"Feature","geometry":null,"properties":{"CODE":1}},
            {"type":"Feature","geometry":{"type":"Point","coordinates":[1.0,2.0]},"properties":{"CODE":2}}]}"#;
        let features = parse(body).expect("parses");
        assert_eq!(features.len(), 1);
        assert_eq!(features[0].number("CODE"), Some(2.0));
    }

    #[test]
    fn holes_are_dropped_and_the_outer_ring_kept() {
        let body = br#"{"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"Polygon","coordinates":[
            [[0,0],[100,0],[100,100],[0,100]],
            [[40,40],[60,40],[60,60],[40,60]]]},"properties":{}}]}"#;
        let features = parse(body).expect("parses");
        assert_eq!(features[0].rings.len(), 1);
        assert_eq!(features[0].rings[0].len(), 4);
    }
}
