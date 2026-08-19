//! Place search (Nominatim): turns "Göttingen Bahnhof" into a latitude and a
//! longitude, so the module anchor can be typed as a name instead of a number.
//!
//! Lives here because this is where the map already is — same HTTP client, same
//! user agent, same OpenStreetMap terms of use.

use std::sync::mpsc::{Receiver, channel};

/// A place the search found.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    /// What Nominatim calls the place — street, town, country in one line.
    pub name: String,
    pub lat: f64,
    pub lon: f64,
}

/// How many hits a search reports at most.
const LIMIT: usize = 8;

/// Looks up a place by name.
///
/// Runs on its own thread and answers exactly once through the returned
/// receiver, so the caller polls with `try_recv` and the editor never waits on
/// the network. Dropping the receiver drops the answer — a search the user has
/// already moved on from costs nothing.
pub fn search(query: &str, user_agent: &str) -> Receiver<Result<Vec<Place>, String>> {
    let (sender, receiver) = channel();
    let query = query.to_string();
    let user_agent = user_agent.to_string();
    std::thread::spawn(move || {
        let _ = sender.send(run(&query, &user_agent));
    });
    receiver
}

fn run(query: &str, user_agent: &str) -> Result<Vec<Place>, String> {
    // Nominatim's usage policy asks for an identifying user agent and no bulk
    // querying; one request per typed search is what this is.
    let url = format!(
        "https://nominatim.openstreetmap.org/search?format=jsonv2&limit={LIMIT}&q={}",
        encode(query)
    );
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(15)))
        .user_agent(user_agent)
        .build()
        .into();
    let body = agent
        .get(&url)
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    parse(&body)
}

fn parse(body: &str) -> Result<Vec<Place>, String> {
    let hits: Vec<serde_json::Value> = serde_json::from_str(body).map_err(|e| e.to_string())?;
    Ok(hits
        .iter()
        .filter_map(|hit| {
            // Nominatim reports the coordinates as strings, not as numbers.
            let lat = hit.get("lat")?.as_str()?.parse().ok()?;
            let lon = hit.get("lon")?.as_str()?.parse().ok()?;
            let name = hit
                .get("display_name")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            Some(Place { name, lat, lon })
        })
        .collect())
}

/// Percent-encodes a query for a URL.
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            b' ' => out.push('+'),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_is_encoded() {
        assert_eq!(encode("Göttingen Bahnhof"), "G%C3%B6ttingen+Bahnhof");
        assert_eq!(encode("a&b=c"), "a%26b%3Dc");
    }

    #[test]
    fn hits_are_read_as_numbers() {
        let body = r#"[
            {"lat": "51.5344", "lon": "9.9328", "display_name": "Göttingen, Germany"},
            {"lat": "not a number", "lon": "9.0", "display_name": "broken"},
            {"display_name": "no coordinates"}
        ]"#;
        let places = parse(body).unwrap();
        assert_eq!(places.len(), 1);
        assert_eq!(places[0].name, "Göttingen, Germany");
        assert!((places[0].lat - 51.5344).abs() < 1e-9);
        assert!((places[0].lon - 9.9328).abs() < 1e-9);
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(parse("<html>rate limited</html>").is_err());
    }
}
