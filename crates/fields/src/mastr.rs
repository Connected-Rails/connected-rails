//! What actually stands there: the Marktstammdatenregister.
//!
//! OpenStreetMap knows *where* a wind turbine stands — surveyed by people who
//! walked past it — but rarely *what* it is. Over two German boxes of a
//! thousand turbines the mappers wrote `manufacturer` on a third to a half of
//! them, `model` on about the same, `height:hub` on a fifth and
//! `rotor:diameter` on one in fourteen. A turbine without a hub height and a
//! rotor diameter is a guess at the two numbers a viewer actually perceives.
//!
//! The Bundesnetzagentur's register has both, for every unit in the country:
//! every generating plant in Germany has to be registered with its
//! manufacturer, type designation, hub height, rotor diameter, rated power,
//! commissioning date and coordinates (§ 3 MaStRV), and the register publishes
//! that. In the box the numbers above come from, all 105 units carried a hub
//! height and a rotor diameter.
//!
//! What is asked here is the *extended public unit data*, the same JSON the
//! register's own web front end reads. It is open, it takes no key, and it
//! filters on the WGS84 coordinates — so a module's envelope box is one query.
//! Compare that with the field registers of [`crate::wfs`]: the shape of the
//! request differs, the deal is the same.
//!
//! **Licence.** The register is published as open data (dl-de/by-2-0,
//! Bundesnetzagentur, Marktstammdatenregister). A module built on it carries
//! the source note, like a module built on a state's DGM does.
//!
//! Nothing here decides anything: this fetches rows and hands them over.
//! Matching them to what OSM surveyed, and turning a machine into something to
//! look at, is `content::wind`'s business.

use crate::wfs::{RequestConfig, ServiceError, encode};
use serde::Deserialize;

/// The register's extended public data on generating units — what its own grid
/// view reads.
pub const REGISTER: &str = "https://www.marktstammdatenregister.de/MaStR/Einheit/EinheitJson/GetErweiterteOeffentlicheEinheitStromerzeugung";

/// The register's own key for wind as the energy carrier. The filter takes the
/// key, not the word, and the keys are the register's stable ones (`2497` is
/// wind, `2495` solar).
const WIND: &str = "2497";

/// Rows per request. The register answers 500 in about a second and some two
/// megabytes; a module's box holds far fewer, and the page loop below is for
/// the one that does not.
const PAGE: usize = 500;

/// How many pages are ever asked for. Ten of them is five thousand turbines,
/// which no module envelope holds — the cap is there so a filter that fails to
/// narrow anything cannot walk the whole country.
const MAX_PAGES: usize = 10;

/// What the register says the state of a unit is.
///
/// The numbers are the register's own (`Betriebs-Status`), which is why they
/// are matched rather than the German words beside them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Approved and not built yet — nothing stands there.
    Planned,
    /// Turning.
    Operating,
    /// Shut down for the time being; the machine is still up.
    Suspended,
    /// Taken down for good.
    Decommissioned,
    /// A code the register has added since.
    Unknown,
}

impl Status {
    fn of(id: i64) -> Self {
        match id {
            31 => Status::Planned,
            35 => Status::Operating,
            37 => Status::Suspended,
            38 => Status::Decommissioned,
            _ => Status::Unknown,
        }
    }

    /// Whether the machine is physically there — which is the only question a
    /// landscape asks. A unit shut down for the season still stands.
    pub fn standing(self) -> bool {
        matches!(
            self,
            Status::Operating | Status::Suspended | Status::Unknown
        )
    }
}

/// One wind turbine as the register holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct WindUnit {
    /// The unit's MaStR number (`SEE945374201878`) — what OSM's `ref:mastr`
    /// carries, and what a builder can look the machine up under.
    pub mastr: String,
    /// Where the register puts it [deg].
    pub lat: f64,
    pub lon: f64,
    /// Manufacturer, cleaned of its company form (`ENERCON GmbH` → `Enercon`);
    /// empty where the register says `Sonstige` or nothing.
    pub manufacturer: String,
    /// The type designation as the register holds it (`E-70 E4`, `V112`),
    /// cleaned of a repeated manufacturer and of doubled spaces. Free text —
    /// the same machine is `E70 E4`, `E-70/4` and `Enercon E-70` in three
    /// entries — so it is a label, not a key.
    pub model: String,
    /// Hub height over ground [m]; 0 where the register has none.
    pub hub_height: f64,
    /// Rotor diameter [m]; 0 where the register has none.
    pub rotor_diameter: f64,
    /// Rated power [kW] — the register's gross figure.
    pub power_kw: f64,
    pub status: Status,
    /// Name of the wind farm the unit belongs to; empty for a lone turbine.
    pub park: String,
}

/// One row of the register's answer. Everything is optional: the grid serves
/// one row shape for solar roofs, biogas plants and wind turbines alike, so
/// most of a wind unit's columns are null on everything else and the other way
/// round.
#[derive(Debug, Deserialize)]
struct Row {
    #[serde(rename = "MaStRNummer")]
    mastr: Option<String>,
    #[serde(rename = "Breitengrad")]
    lat: Option<f64>,
    #[serde(rename = "Laengengrad")]
    lon: Option<f64>,
    #[serde(rename = "HerstellerWindenergieanlageBezeichnung")]
    manufacturer: Option<String>,
    #[serde(rename = "Typenbezeichnung")]
    model: Option<String>,
    #[serde(rename = "NabenhoeheWindenergieanlage")]
    hub_height: Option<f64>,
    #[serde(rename = "RotordurchmesserWindenergieanlage")]
    rotor_diameter: Option<f64>,
    #[serde(rename = "Bruttoleistung")]
    power_kw: Option<f64>,
    #[serde(rename = "BetriebsStatusId")]
    status: Option<i64>,
    #[serde(rename = "WindparkName")]
    park: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Answer {
    #[serde(rename = "Data")]
    data: Vec<Row>,
    /// How many rows the filter matches in total, however many this page holds.
    #[serde(rename = "Total")]
    total: Option<usize>,
}

/// The URL for one page of the wind turbines in a box.
///
/// The filter language is the register's own:
/// `field~operator~'value'~and~field~operator~'value'`, with the fields named
/// as the front end shows them — umlauts, spaces and all, which is why every
/// value goes through [`encode`].
pub fn wind_url(
    min_lat: f64,
    min_lon: f64,
    max_lat: f64,
    max_lon: f64,
    page: usize,
    page_size: usize,
) -> String {
    let filter = format!(
        "Energieträger~eq~'{WIND}'\
         ~and~Koordinate: Breitengrad (WGS84)~gt~'{min_lat:.6}'\
         ~and~Koordinate: Breitengrad (WGS84)~lt~'{max_lat:.6}'\
         ~and~Koordinate: Längengrad (WGS84)~gt~'{min_lon:.6}'\
         ~and~Koordinate: Längengrad (WGS84)~lt~'{max_lon:.6}'"
    );
    format!(
        "{REGISTER}?filter={}&page={page}&pageSize={page_size}",
        encode(&filter)
    )
}

/// Every wind turbine the register holds in a box, whatever its state — a unit
/// that has been taken down is worth knowing about, because it is why OSM has
/// a turbine there and the register does not, or the other way round.
pub fn fetch_wind(
    min_lat: f64,
    min_lon: f64,
    max_lat: f64,
    max_lon: f64,
    config: &RequestConfig,
) -> Result<Vec<WindUnit>, ServiceError> {
    let mut out = Vec::new();
    for page in 1..=MAX_PAGES {
        let url = wind_url(min_lat, min_lon, max_lat, max_lon, page, PAGE);
        let (units, rows, total) = parse_page(&get(&url, config)?)?;
        out.extend(units);
        // The count the register sends is what the *filter* matches, and the
        // rows are what this page held — the loop stops on either, and on
        // neither the number of units, because a page can be short of units
        // and full of rows when some carried no coordinates.
        let done = match total {
            Some(total) => page * PAGE >= total,
            None => rows < PAGE,
        };
        if done || rows == 0 {
            break;
        }
    }
    Ok(out)
}

/// Reads one page of the register's answer: the units it holds and how many
/// the filter matches in all.
///
/// A row without coordinates or without a MaStR number is dropped — there is
/// nothing to put on the ground and nothing to match it by.
pub fn parse_wind(json: &str) -> Result<(Vec<WindUnit>, Option<usize>), ServiceError> {
    let (units, _, total) = parse_page(json)?;
    Ok((units, total))
}

/// The same, plus how many rows the page held before the ones with nothing to
/// place were dropped — which is what [`fetch_wind`]'s page loop counts.
fn parse_page(json: &str) -> Result<(Vec<WindUnit>, usize, Option<usize>), ServiceError> {
    let answer: Answer =
        serde_json::from_str(json).map_err(|e| ServiceError::NotGeoJson(e.to_string()))?;
    let rows = answer.data.len();
    let units = answer
        .data
        .into_iter()
        .filter_map(|row| {
            let manufacturer = clean_manufacturer(row.manufacturer.as_deref().unwrap_or(""));
            Some(WindUnit {
                mastr: row.mastr.filter(|s| !s.is_empty())?,
                lat: row.lat?,
                lon: row.lon?,
                model: clean_model(row.model.as_deref().unwrap_or(""), &manufacturer),
                manufacturer,
                hub_height: row.hub_height.unwrap_or(0.0).max(0.0),
                rotor_diameter: row.rotor_diameter.unwrap_or(0.0).max(0.0),
                power_kw: row.power_kw.unwrap_or(0.0).max(0.0),
                status: Status::of(row.status.unwrap_or(0)),
                park: row.park.unwrap_or_default(),
            })
        })
        .collect();
    Ok((units, rows, answer.total))
}

/// The manufacturer without its company form: the register writes
/// `ENERCON GmbH`, `Vestas Deutschland GmbH` and `REpower Systems SE` for what
/// a person calls Enercon, Vestas and REpower. `Sonstige` is the register's
/// "other", which says nothing, so it becomes nothing.
///
/// The capitalisation is fixed as well — `ENERCON` is a logo, not a spelling,
/// and `Enercon` is what OpenStreetMap's mappers write, so the two sources
/// agree on the name. Only a word shouted in full is turned down; `GE` is too
/// short to be a shout and `REpower` was never one.
fn clean_manufacturer(raw: &str) -> String {
    // Stripped until nothing comes off any more: the forms stack, and
    // `VENSYS Energy AG` only loses its `Energy` once the `AG` is gone.
    let mut name = raw.trim();
    loop {
        let before = name;
        for tail in [
            " GmbH & Co. KG",
            " GmbH",
            " AG",
            " SE",
            " KG",
            " B.V.",
            " A/S",
            " Deutschland",
            " Systems",
            " Energy",
        ] {
            name = name.strip_suffix(tail).unwrap_or(name).trim_end();
        }
        if name == before {
            break;
        }
    }
    let name = name.trim();
    if name.is_empty() || name.eq_ignore_ascii_case("Sonstige") {
        return String::new();
    }
    if name.len() > 2 && name.chars().all(|c| !c.is_lowercase()) {
        let mut chars = name.chars();
        let head = chars.next().unwrap_or_default();
        return format!("{head}{}", chars.as_str().to_lowercase());
    }
    name.to_string()
}

/// The type designation, tidied: the doubled spaces the register is full of
/// collapsed, and a repeated manufacturer taken off the front, so
/// `Vestas V112` beside a manufacturer of `Vestas` is `V112` and the display
/// name does not say it twice.
fn clean_model(raw: &str, manufacturer: &str) -> String {
    let mut model = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if !manufacturer.is_empty() {
        let head = format!("{manufacturer} ");
        if model.len() > head.len() && model[..head.len()].eq_ignore_ascii_case(&head) {
            model = model[head.len()..].to_string();
        }
    }
    model
}

/// Fetches a URL, refusing to read past [`RequestConfig::max_bytes`] — the same
/// guard [`crate::wfs`] fetches under, and for the same reason: a filter that
/// slips would otherwise start sending the register.
fn get(url: &str, config: &RequestConfig) -> Result<String, ServiceError> {
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
    let read = std::io::Read::take(response.body_mut().as_reader(), config.max_bytes as u64 + 1);
    std::io::copy(&mut std::io::BufReader::new(read), &mut body)
        .map_err(|e| ServiceError::Network(e.to_string()))?;
    if body.len() > config.max_bytes {
        return Err(ServiceError::TooMuch);
    }
    String::from_utf8(body).map_err(|e| ServiceError::NotGeoJson(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One row as the register sent it, cut down to the columns that are read.
    const SAMPLE: &str = r#"{"Data":[
        {"MaStRNummer":"SEE945374201878","Breitengrad":54.268364,"Laengengrad":9.013525,
         "HerstellerWindenergieanlageBezeichnung":"ENERCON GmbH","Typenbezeichnung":"E-70  E4",
         "NabenhoeheWindenergieanlage":65.0,"RotordurchmesserWindenergieanlage":71.0,
         "Bruttoleistung":2300.0,"BetriebsStatusId":35,"WindparkName":"Hemme Jarrenwisch 9"},
        {"MaStRNummer":"SEE902056632382","Breitengrad":54.206056,"Laengengrad":9.005182,
         "HerstellerWindenergieanlageBezeichnung":"Vestas Deutschland GmbH",
         "Typenbezeichnung":"Vestas V112","NabenhoeheWindenergieanlage":119.0,
         "RotordurchmesserWindenergieanlage":112.0,"Bruttoleistung":3450.0,
         "BetriebsStatusId":38,"WindparkName":null},
        {"MaStRNummer":"SEE000000000000","Breitengrad":null,"Laengengrad":null,
         "HerstellerWindenergieanlageBezeichnung":"Sonstige","Typenbezeichnung":null,
         "BetriebsStatusId":31}
    ],"Total":2}"#;

    #[test]
    fn the_registers_rows_come_out_as_turbines() {
        let (units, total) = parse_wind(SAMPLE).expect("parses");
        assert_eq!(total, Some(2));
        // The row without coordinates is dropped: there is nowhere to put it.
        assert_eq!(units.len(), 2);

        let enercon = &units[0];
        assert_eq!(enercon.mastr, "SEE945374201878");
        assert_eq!(enercon.manufacturer, "Enercon");
        // The doubled space the register writes is collapsed.
        assert_eq!(enercon.model, "E-70 E4");
        assert_eq!(enercon.hub_height, 65.0);
        assert_eq!(enercon.rotor_diameter, 71.0);
        assert_eq!(enercon.status, Status::Operating);
        assert!(enercon.status.standing());

        let vestas = &units[1];
        assert_eq!(vestas.manufacturer, "Vestas");
        // The manufacturer is not said twice.
        assert_eq!(vestas.model, "V112");
        assert_eq!(vestas.status, Status::Decommissioned);
        assert!(!vestas.status.standing());
        assert!(vestas.park.is_empty());
    }

    #[test]
    fn the_company_form_is_not_part_of_the_name() {
        assert_eq!(clean_manufacturer("ENERCON GmbH"), "Enercon");
        assert_eq!(clean_manufacturer("Vestas Deutschland GmbH"), "Vestas");
        assert_eq!(clean_manufacturer("REpower Systems SE"), "REpower");
        assert_eq!(clean_manufacturer("Nordex Energy GmbH"), "Nordex");
        assert_eq!(clean_manufacturer("VENSYS Energy AG"), "Vensys");
        assert_eq!(clean_manufacturer("GE Wind Energy GmbH"), "GE Wind");
        // The register's "other" is not a manufacturer.
        assert_eq!(clean_manufacturer("Sonstige"), "");
        assert_eq!(clean_manufacturer(""), "");
    }

    #[test]
    fn the_box_is_what_the_filter_asks_for() {
        let url = wind_url(52.0, 10.0, 52.1, 10.2, 1, 500);
        assert!(url.starts_with(REGISTER));
        assert!(url.ends_with("&page=1&pageSize=500"));
        // Everything of the filter is encoded — the field names carry spaces,
        // brackets and umlauts, and an unencoded one is a 400.
        let filter = url
            .split_once("?filter=")
            .and_then(|(_, rest)| rest.split('&').next())
            .expect("has a filter");
        assert!(!filter.contains(' '));
        assert!(filter.contains("2497"));
        assert!(filter.contains("52.000000"));
        assert!(filter.contains("10.200000"));
    }

    #[test]
    fn an_answer_without_rows_is_not_an_error() {
        let (units, total) = parse_wind(r#"{"Data":[],"Total":0}"#).expect("parses");
        assert!(units.is_empty());
        assert_eq!(total, Some(0));
    }
}
