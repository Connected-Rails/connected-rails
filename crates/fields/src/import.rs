//! The import itself: a box on the map in, a list of fields out.
//!
//! Everything the other modules do is arranged here, in the order plan ch. 4
//! sets out. Which states does this box touch; ask each one, tile by tile, cache
//! first; make the state's own attributes into ours; drop what is too small,
//! cut what leaves the module, punch the track's formation out of what is left;
//! work out which way each field was worked in. Nothing is written to a line —
//! the caller shows the result and the user decides.
//!
//! It is written to be driven from a worker thread: [`run`] reports through a
//! callback that can also stop it, so the editor's dialog shows what is
//! happening and its Cancel button means it.
//!
//! Two defences worth naming, because both come from the services misbehaving
//! rather than from theory. A box that yields more than one answer can hold is
//! quartered and asked again, so a request never falls over on a dense
//! landscape. And an attribute that is not where it is expected never stops the
//! import: a crop code no table knows falls through to the next thing that is
//! known about the parcel — its InVeKoS group, then the regional statistics —
//! and is reported rather than raised, because a line with one plausible field
//! is worth more than an import that refused (plan ch. 9, "Schemabrüche").

use crate::attribution::Attribution;
use crate::cache::{self, FieldCache, Stamp};
use crate::crops::{CropClass, CropTable};
use crate::geometry::{self, Op};
use crate::land::{Access, Land};
use crate::model::{FieldFeature, Level};
use crate::stats;
use crate::wfs::{self, Query, RawFeature, RequestConfig, ServiceError};
use glam::DVec2;
use std::collections::{HashMap, HashSet};

/// The piece of world to import, in degrees.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Area {
    /// The boundary, `(lat, lon)`, closed implicitly. The module envelope, or
    /// whatever the user has selected.
    pub boundary: Vec<(f64, f64)>,
    /// Track centrelines, `(lat, lon)` — what gets punched out of the fields.
    /// Empty means nothing is punched out.
    pub track: Vec<Vec<(f64, f64)>>,
}

impl Area {
    /// The bounding box in degrees, `(min_lat, min_lon, max_lat, max_lon)`.
    pub fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
        let first = *self.boundary.first()?;
        let mut b = (first.0, first.1, first.0, first.1);
        for (lat, lon) in &self.boundary {
            b.0 = b.0.min(*lat);
            b.1 = b.1.min(*lon);
            b.2 = b.2.max(*lat);
            b.3 = b.3.max(*lon);
        }
        Some(b)
    }
}

/// What to do with a field that only partly lies in the area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clip {
    /// Cut it at the boundary. Right for a module envelope: the neighbouring
    /// module owns the rest, and two modules must not draw the same ground.
    Cut,
    /// Keep it whole if its middle is inside. Right for a selection, where the
    /// user means "that field", not "that rectangle".
    Whole,
}

/// How far the fields stay clear of the track [m], when nothing else says so —
/// the terrain's own blend zone, the foot of the embankment, plus a margin.
pub const TRACK_CLEARANCE: f64 = 15.0;

/// How the import is to be run.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportOptions {
    pub clip: Clip,
    /// Douglas-Peucker tolerance [m]. A surveyed boundary has a vertex every
    /// few metres; at 1.5 m nothing is visible from a train and the vertex
    /// count falls by an order of magnitude.
    pub simplify: f64,
    /// Fields below this are dropped [m²]. Half a hectare: below that a parcel
    /// is a margin strip or a corner, and a line does not want ten thousand of
    /// them.
    pub min_area: f64,
    /// Half-width of the strip kept clear of the track [m]. The default is the
    /// terrain's own blend zone — the foot of the embankment — plus a margin,
    /// so no field lies on the embankment the ground pulls up to rail height.
    pub track_clearance: f64,
    /// Upper bound on what one import may produce. A whole state through a
    /// mis-drawn envelope is a hung editor, not a feature.
    pub max_fields: usize,
    /// UTM zone the result is delivered in — the line's own.
    pub zone: u8,
    pub request: RequestConfig,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            clip: Clip::Cut,
            simplify: 1.5,
            min_area: 5_000.0,
            track_clearance: TRACK_CLEARANCE,
            max_fields: 20_000,
            zone: 32,
            request: RequestConfig::default(),
        }
    }
}

/// Where the import has got to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Working out which states the area touches.
    Locating,
    /// Asking the services, or reading the cache.
    Fetching,
    /// Turning each state's attributes into ours.
    Mapping,
    /// Thinning, clipping, punching out, measuring.
    Cleaning,
    Done,
}

impl Stage {
    pub fn key(self) -> &'static str {
        match self {
            Stage::Locating => "field-import-locating",
            Stage::Fetching => "field-import-fetching",
            Stage::Mapping => "field-import-mapping",
            Stage::Cleaning => "field-import-cleaning",
            Stage::Done => "field-import-done",
        }
    }
}

/// One report of progress. `done` of `total` within the stage.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportProgress {
    pub stage: Stage,
    pub done: usize,
    pub total: usize,
    /// What is being worked on — a state's name, usually. Shown next to the bar.
    pub note: String,
}

impl ImportProgress {
    /// `0.0 ..= 1.0`, or `None` for a stage whose length is not known yet.
    pub fn fraction(&self) -> Option<f32> {
        (self.total > 0).then(|| (self.done as f32 / self.total as f32).clamp(0.0, 1.0))
    }
}

/// What the import found.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ImportReport {
    pub fields: Vec<FieldFeature>,
    pub attribution: Attribution,
    /// What each state was asked for and when — goes into the line, so a module
    /// says which register it portrays.
    pub stamps: Vec<Stamp>,
    /// Tiles that came from the services, and tiles that came from disk.
    pub fetched: usize,
    pub cached: usize,
    /// Parcels the services returned, before anything was dropped.
    pub parcels: usize,
    /// Dropped for being smaller than [`ImportOptions::min_area`].
    pub too_small: usize,
    /// Dropped for lying outside the area entirely.
    pub outside: usize,
    /// Fields that came back in more than one piece — cut by the track, or by
    /// the module boundary.
    pub split: usize,
    /// Crop codes the tables did not know, by state and code.
    pub unknown_codes: Vec<String>,
    pub warnings: Vec<String>,
    /// Set when the callback asked to stop. The fields found so far are still
    /// in the report; the caller decides whether to keep them.
    pub cancelled: bool,
}

impl ImportReport {
    /// How many fields of each crop — the summary the dialog shows before
    /// anything is committed.
    pub fn by_crop(&self) -> Vec<(CropClass, usize)> {
        let mut counts: HashMap<CropClass, usize> = HashMap::new();
        for field in &self.fields {
            *counts.entry(field.crop).or_default() += 1;
        }
        let mut out: Vec<(CropClass, usize)> = counts.into_iter().collect();
        out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        out
    }

    /// Total area of what was found [ha].
    pub fn hectares(&self) -> f64 {
        self.fields.iter().map(|f| f.area()).sum::<f64>() / 10_000.0
    }
}

/// Runs an import.
///
/// `progress` is called at every step and returns whether to carry on; the
/// editor's Cancel button is a `false` from it. It is called from this thread,
/// which is not the editor's — sending down a channel is what it is for.
pub fn run(
    area: &Area,
    options: &ImportOptions,
    cache: &FieldCache,
    table: &CropTable,
    progress: &mut dyn FnMut(ImportProgress) -> bool,
) -> ImportReport {
    let mut report = ImportReport::default();
    let mut say = |report: &mut ImportReport, stage, done, total, note: &str| -> bool {
        let go_on = progress(ImportProgress {
            stage,
            done,
            total,
            note: note.to_string(),
        });
        if !go_on {
            report.cancelled = true;
        }
        go_on
    };

    // 1. Which states.
    if !say(&mut report, Stage::Locating, 0, 0, "") {
        return report;
    }
    let Some((min_lat, min_lon, max_lat, max_lon)) = area.bounds() else {
        report.warnings.push("the area has no boundary".into());
        return report;
    };
    let lands = Land::touching(min_lat, min_lon, max_lat, max_lon);

    // 2. Fetch, per state, tile by tile.
    let mut raw: Vec<FieldFeature> = Vec::new();
    let mut seen: HashSet<(Option<Land>, String)> = HashSet::new();

    // Outside Germany there is no register to ask — no state boundary holds the
    // box, so no service does either. The approach is national by nature: the
    // registers are national, their schemas are national, and their crop code
    // lists are national. What is *not* national is OpenStreetMap, so a module
    // in Austria or the Netherlands takes the same fallback Rhineland-Palatinate
    // does, and gets the shape of the countryside with its crops drawn from the
    // statistics (plan ch. 9, "Kein Ausland").
    if lands.is_empty() {
        if !say(&mut report, Stage::Fetching, 0, 1, origin_name(None)) {
            return report;
        }
        report.warnings.push(
            "outside the German registers — the fields come from OpenStreetMap, \
             which is share-alike and thinner than a register"
                .into(),
        );
        if let Some((stamp, fields)) = fetch_osm(
            None,
            min_lat,
            min_lon,
            max_lat,
            max_lon,
            options,
            table,
            cache,
            &mut report,
        ) {
            report.stamps.push(stamp);
            for field in fields {
                report.parcels += 1;
                if !field.id.is_empty() && !seen.insert((None, field.id.clone())) {
                    continue;
                }
                raw.push(field);
            }
        }
    }
    for (index, land) in lands.iter().copied().enumerate() {
        let service = land.service();
        match service.access {
            Access::None => {
                report.warnings.push(format!(
                    "{} publishes no field data; see plan ch. 3 for the fallback",
                    land.name()
                ));
                continue;
            }
            Access::Download => {
                report.warnings.push(format!(
                    "{} only offers a whole-state download ({})",
                    land.name(),
                    service.url
                ));
                continue;
            }
            // The one state with no register: OpenStreetMap covers it whole
            // rather than tile by tile, because Overpass is asked once for a
            // box and charges for the asking, not for the area.
            Access::Osm => {
                // One request, so one step — but it is the slow one, and the
                // dialog has to say whose it is.
                if !say(
                    &mut report,
                    Stage::Fetching,
                    index,
                    lands.len(),
                    land.name(),
                ) {
                    return report;
                }
                let fetched = fetch_osm(
                    Some(land),
                    min_lat,
                    min_lon,
                    max_lat,
                    max_lon,
                    options,
                    table,
                    cache,
                    &mut report,
                );
                if let Some((stamp, fields)) = fetched {
                    if !report.stamps.contains(&stamp) {
                        report.stamps.push(stamp);
                    }
                    for field in fields {
                        report.parcels += 1;
                        if !field.id.is_empty() && !seen.insert((Some(land), field.id.clone())) {
                            continue;
                        }
                        raw.push(field);
                    }
                }
                continue;
            }
            Access::Wfs | Access::WfsPointJoin => {}
        }

        let zone = land.utm_zone();
        let (min, max) = box_in_zone(min_lat, min_lon, max_lat, max_lon, zone);
        let keys = cache::tiles_in(min, max);
        for (done, key) in keys.iter().copied().enumerate() {
            if !say(
                &mut report,
                Stage::Fetching,
                index * keys.len() + done,
                lands.len() * keys.len(),
                land.name(),
            ) {
                return report;
            }
            let (stamp, fields) = match cache.get(Some(land), key) {
                Some(hit) => {
                    report.cached += 1;
                    hit
                }
                None if cache.offline => continue,
                None => match fetch_tile(land, key, options, table, &mut report) {
                    Some(pair) => {
                        report.fetched += 1;
                        cache.put(Some(land), key, &pair.0, &pair.1);
                        pair
                    }
                    // The warning says why; the counters only count what came.
                    None => continue,
                },
            };
            if !report.stamps.contains(&stamp) {
                report.stamps.push(stamp);
            }
            for field in fields {
                report.parcels += 1;
                // A field on a tile seam comes back from both tiles.
                if !field.id.is_empty() && !seen.insert((Some(land), field.id.clone())) {
                    continue;
                }
                raw.push(field);
            }
            if raw.len() > options.max_fields {
                report.warnings.push(format!(
                    "stopped at {} fields — the area is too large",
                    options.max_fields
                ));
                break;
            }
        }
        // The cap is on the import, not on one state: without this the next
        // state would start again from under it.
        if raw.len() > options.max_fields {
            break;
        }
    }

    // 3. Into the line's own zone and coordinates.
    if !say(&mut report, Stage::Mapping, 0, raw.len(), "") {
        return report;
    }
    for field in &mut raw {
        reproject(field, options.zone);
    }

    // 4. Clean up.
    let boundary = ring_in_zone(&area.boundary, options.zone);
    let corridors: Vec<Vec<DVec2>> = area
        .track
        .iter()
        .flat_map(|line| {
            geometry::corridor(&ring_in_zone(line, options.zone), options.track_clearance)
        })
        .collect();

    let total = raw.len();
    for (done, field) in raw.into_iter().enumerate() {
        if done % 64 == 0 && !say(&mut report, Stage::Cleaning, done, total, "") {
            return report;
        }
        let pieces = shape(&field, &boundary, &corridors, options, &mut report);
        if pieces.len() > 1 {
            report.split += pieces.len() - 1;
        }
        for piece in pieces {
            report.attribution.add(piece.land, piece.year);
            report.fields.push(piece);
        }
    }
    // A stable order, so two runs of the same import write the same file.
    report
        .fields
        .sort_by(|a, b| a.id.cmp(&b.id).then(a.centre().x.total_cmp(&b.centre().x)));
    say(&mut report, Stage::Done, total, total, "");
    report
}

/// The OpenStreetMap fallback, for a state with no register of its own.
///
/// One box, not a grid of tiles: Overpass answers a query, and asking it four
/// times for quarters of the same area is four times the load for the same
/// polygons. The answer is cached under the tile the box's south-west corner
/// falls in, so a second import of the same module reads it back.
#[allow(clippy::too_many_arguments)]
fn fetch_osm(
    land: Option<Land>,
    min_lat: f64,
    min_lon: f64,
    max_lat: f64,
    max_lon: f64,
    options: &ImportOptions,
    table: &CropTable,
    cache: &FieldCache,
    report: &mut ImportReport,
) -> Option<(Stamp, Vec<FieldFeature>)> {
    // A state publishes in the zone it has chosen; abroad there is no such
    // choice and the zone is the one the ground is actually in.
    let zone = land.map_or_else(
        || crate::land::utm_zone_at((min_lon + max_lon) / 2.0),
        |l| l.utm_zone(),
    );
    let (min, _) = box_in_zone(min_lat, min_lon, max_lat, max_lon, zone);
    let key = cache::tile_at(min);
    if let Some((stamp, fields)) = cache.get(land, key) {
        report.cached += 1;
        return Some((stamp, fields));
    }
    if cache.offline {
        return None;
    }
    report.fetched += 1;
    let parcels = match crate::osm::fetch(min_lat, min_lon, max_lat, max_lon, &options.request) {
        Ok(parcels) => parcels,
        Err(e) => {
            report.warnings.push(format!("{}: {e}", origin_name(land)));
            return None;
        }
    };
    let fields: Vec<FieldFeature> = parcels
        .iter()
        .map(|parcel| osm_field(land, zone, parcel, table))
        .collect();
    let stamp = Stamp {
        land: cache::origin_code(land).to_string(),
        // OpenStreetMap has no application year — it is as of when it was
        // fetched, which is what `fetched` says.
        year: None,
        fetched: cache::now(),
    };
    cache.put(land, key, &stamp, &fields);
    Some((stamp, fields))
}

/// One OSM way as a field. `landuse` says what kind of ground it is; the crop
/// itself is drawn from the statistics unless a mapper wrote one down.
fn osm_field(
    land: Option<Land>,
    zone: u8,
    parcel: &crate::osm::Parcel,
    table: &CropTable,
) -> FieldFeature {
    let tag = |key: &str| parcel.tags.get(key).map(String::as_str).unwrap_or("");
    let landuse = if tag("landuse").is_empty() {
        tag("natural")
    } else {
        tag("landuse")
    };
    let mut field = FieldFeature {
        polygon: crate::osm::ring_in_zone(parcel, zone),
        zone,
        land,
        year: None,
        code_raw: landuse.to_string(),
        code_text: String::new(),
        crop: CropClass::Other,
        level: Level::Drawn,
        direction: 0.0,
        area_ha: 0.0,
        organic: None,
        id: parcel.id.clone(),
    };
    let seed = field.seed();

    // A mapper who wrote the crop down outranks any statistics.
    if let Some(crop) = crop_tag(tag("crop")).or_else(|| crop_tag(tag("produce"))) {
        field.crop = crop;
        field.level = Level::Declared;
        field.code_text = tag("crop").to_string();
        return field;
    }
    field.crop = match landuse {
        "meadow" | "grassland" => CropClass::Grassland,
        "vineyard" => CropClass::Vineyard,
        "orchard" => CropClass::Orchard,
        "greenhouse_horticulture" | "plant_nursery" => CropClass::Vegetable,
        // Arable, and nothing more said: draw it.
        _ => table
            .arable_weights(region_of(land))
            .and_then(|weights| stats::draw(weights, seed))
            .unwrap_or(CropClass::Other),
    };
    field
}

/// The key a field's cropping statistics are looked up by: the state's code,
/// or `*` where there is no state — [`CropTable::arable_weights`] falls back to
/// the general row for anything it does not have.
fn region_of(land: Option<Land>) -> &'static str {
    land.map_or("*", |l| l.code())
}

/// What a field's origin is called on screen and in a warning.
fn origin_name(land: Option<Land>) -> &'static str {
    // A project name, so it reads the same in every language.
    land.map_or("OpenStreetMap", |l| l.name())
}

/// OpenStreetMap's `crop=*` values, for the few that are common enough to be
/// worth reading. An unknown one falls through to the draw.
fn crop_tag(value: &str) -> Option<CropClass> {
    Some(match value.trim().to_ascii_lowercase().as_str() {
        "wheat" | "rye" | "barley" | "triticale" | "spelt" | "cereal" | "grain" => {
            CropClass::WinterCereal
        }
        "oat" | "oats" | "millet" | "buckwheat" => CropClass::SummerCereal,
        "maize" | "corn" | "sweet_corn" => CropClass::Maize,
        "rape" | "rapeseed" | "canola" | "mustard" => CropClass::Rapeseed,
        "sugar_beet" | "sugarbeet" | "beet" => CropClass::SugarBeet,
        "potato" | "potatoes" => CropClass::Potato,
        "soy" | "soybean" | "bean" | "beans" | "pea" | "peas" | "lupin" | "legume" => {
            CropClass::Legume
        }
        "grass" | "hay" | "fodder" => CropClass::Grassland,
        "vegetable" | "vegetables" | "asparagus" | "strawberry" | "cabbage" | "onion" => {
            CropClass::Vegetable
        }
        "grape" | "grapes" | "wine" => CropClass::Vineyard,
        "apple" | "pear" | "cherry" | "plum" | "fruit" | "nut" | "hazelnut" | "walnut" => {
            CropClass::Orchard
        }
        "sunflower" | "hemp" | "flax" | "tobacco" | "hop" | "hops" => CropClass::Other,
        _ => return None,
    })
}

/// Asks one state for one tile, splitting the box when the answer is too big.
fn fetch_tile(
    land: Land,
    key: cache::TileKey,
    options: &ImportOptions,
    table: &CropTable,
    report: &mut ImportReport,
) -> Option<(Stamp, Vec<FieldFeature>)> {
    let min = cache::tile_min(key);
    let max = min + DVec2::splat(cache::TILE);
    let raw = fetch_box(land, min, max, &options.request, 0, report)?;

    // A point-join state needs the polygons as well.
    let mut fields = Vec::new();
    if land.service().access == Access::WfsPointJoin {
        let query = Query { land, min, max };
        match wfs::fetch_join(&query, &options.request) {
            Ok(polygons) => fields = join_points(land, &raw, &polygons, table, report),
            Err(e) => report.warnings.push(format!("{}: {e}", land.name())),
        }
    } else {
        for feature in &raw {
            fields.extend(normalise(land, feature, table, report));
        }
    }

    let year = fields.iter().find_map(|f| f.year);
    Some((
        Stamp {
            land: land.code().to_string(),
            year,
            fetched: cache::now(),
        },
        fields,
    ))
}

/// One `GetFeature`, quartering the box when the service sends too much.
fn fetch_box(
    land: Land,
    min: DVec2,
    max: DVec2,
    config: &RequestConfig,
    depth: u32,
    report: &mut ImportReport,
) -> Option<Vec<RawFeature>> {
    let query = Query { land, min, max };
    match wfs::fetch(&query, config) {
        Ok(features) => Some(features),
        Err(ServiceError::TooMuch) if depth < 4 => {
            // Quarter it. Four levels takes a two-kilometre tile down to 125 m,
            // which no landscape fills with 64 MB of parcels.
            let mid = (min + max) / 2.0;
            let mut all = Vec::new();
            for (lo, hi) in [
                (min, mid),
                (DVec2::new(mid.x, min.y), DVec2::new(max.x, mid.y)),
                (DVec2::new(min.x, mid.y), DVec2::new(mid.x, max.y)),
                (mid, max),
            ] {
                all.extend(fetch_box(land, lo, hi, config, depth + 1, report)?);
            }
            Some(all)
        }
        Err(e) => {
            let message = format!("{}: {e}", land.name());
            if !report.warnings.contains(&message) {
                report.warnings.push(message);
            }
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Attributes
//
// Every state names the same thing differently, and the names change between
// application years. Rather than a reader per state, each attribute has a list
// of the names it is known by and the first one present wins — a schema that
// shifts by a column then costs a line in a list instead of a broken import.
// ---------------------------------------------------------------------------

/// The state's own detail code — only North Rhine-Westphalia and Brandenburg
/// publish one fine enough to be worth a table.
const CODE_KEYS: &[&str] = &["CODE", "nutzcode", "kulturcode", "code_nr", "bnk"];
/// The crop as the service spells it.
const TEXT_KEYS: &[&str] = &[
    "CODE_TXT",
    "kulturart",
    "nutzung_txt",
    "bezeichnung",
    "name",
];
/// The InVeKoS group — the harmonised `GT`, `OE`, `HF` … list.
const GROUP_KEYS: &[&str] = &["USE_CODE", "mainCrop", "hauptfrucht", "use_code"];
const AREA_KEYS: &[&str] = &[
    "AREA_HA",
    "declaredarea",
    "declaredArea",
    "flaeche_ha",
    "flaeche",
    "groesse",
];
const ORGANIC_KEYS: &[&str] = &["ORGANICFAR", "organicfarming", "organicFarming", "oeko"];
const YEAR_KEYS: &[&str] = &[
    "VALIDFROM",
    "validfrom",
    "validFrom",
    "antragsjahr",
    "jahr",
    "beginlifespanversion",
];
const ID_KEYS: &[&str] = &["id", "gml_id", "GmlID", "FLIK", "flik", "OBJECTID"];
/// Whether the block is grassland rather than arable — an LPIS service's only
/// word on what grows there.
const LANDUSE_KEYS: &[&str] = &[
    "D_PG",
    "hauptbodennutzung",
    "bodennutzung",
    "flaechentyp",
    "nutzungsart",
    "landusetype",
];

fn first_text(feature: &RawFeature, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| feature.text(k))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn first_number(feature: &RawFeature, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|k| feature.number(k))
}

fn first_flag(feature: &RawFeature, keys: &[&str]) -> Option<bool> {
    keys.iter().find_map(|k| feature.flag(k))
}

/// The application year out of whatever the service wrote — `2026`,
/// `"2025-12-02"`, `"01.01.2025"`, `"5.5.2026"`.
fn year_of(feature: &RawFeature) -> Option<u32> {
    let text = first_text(feature, YEAR_KEYS)?;
    // The first run of four digits in the string is the year in all of them.
    let bytes = text.as_bytes();
    for start in 0..bytes.len().saturating_sub(3) {
        if bytes[start..start + 4].iter().all(u8::is_ascii_digit) {
            let year: u32 = text[start..start + 4].parse().ok()?;
            if (1990..2100).contains(&year) {
                return Some(year);
            }
        }
    }
    None
}

/// The InVeKoS group code, including the INSPIRE spelling where it arrives as
/// the tail of a codelist URL (`…/de.iacs/CropValue/HF`).
fn group_of(feature: &RawFeature) -> Option<String> {
    if let Some(group) = first_text(feature, GROUP_KEYS) {
        return Some(group.to_ascii_uppercase());
    }
    let description = feature.text("description")?;
    let tail = description.rsplit('/').next()?.trim();
    (!tail.is_empty() && tail.len() <= 4).then(|| tail.to_ascii_uppercase())
}

/// Whether an LPIS field block says grassland.
fn is_grassland(feature: &RawFeature) -> bool {
    let Some(text) = first_text(feature, LANDUSE_KEYS) else {
        return false;
    };
    let text = text.to_ascii_lowercase();
    // "DGL", "Dauergrünland", "GL", "grassland", "permanent grassland"; the
    // NRW flag `D_PG` is a plain J/N.
    text.starts_with('j')
        || text.contains("grün")
        || text.contains("gruen")
        || text.contains("grass")
        || text == "dgl"
        || text == "gl"
}

/// One service feature as a field.
fn normalise(
    land: Land,
    feature: &RawFeature,
    table: &CropTable,
    report: &mut ImportReport,
) -> Option<FieldFeature> {
    let ring = feature.rings.first()?;
    let mut field = FieldFeature {
        polygon: ring.clone(),
        zone: land.utm_zone(),
        land: Some(land),
        year: year_of(feature),
        code_raw: String::new(),
        code_text: first_text(feature, TEXT_KEYS).unwrap_or_default(),
        crop: CropClass::Other,
        level: Level::Drawn,
        direction: 0.0,
        area_ha: first_number(feature, AREA_KEYS).unwrap_or(0.0),
        organic: first_flag(feature, ORGANIC_KEYS),
        id: first_text(feature, ID_KEYS).unwrap_or_default(),
    };
    resolve_crop(&mut field, feature, table, report);
    Some(field)
}

/// The crop, by the plan's cascade: the state's own code first, its InVeKoS
/// group next, and the regional statistics last.
fn resolve_crop(
    field: &mut FieldFeature,
    feature: &RawFeature,
    table: &CropTable,
    report: &mut ImportReport,
) {
    let seed = field.seed();

    // 1. The detail code, where the state publishes one and the table knows it.
    // A field from outside the registers has no state and so no code list.
    if let Some(code) = first_text(feature, CODE_KEYS) {
        field.code_raw = code.clone();
        if let Some(land) = field.land {
            if let Some(entry) = table.lookup(land, &code) {
                field.crop = entry.class;
                field.level = Level::Declared;
                if field.code_text.is_empty() {
                    field.code_text = entry.label.clone();
                }
                return;
            }
            // A code the table has never seen. Not fatal — it falls through to
            // the group — but it is exactly what a schema change looks like, so
            // it is reported (plan ch. 9).
            if table.code_count(land) > 0 {
                let note = format!("{} {code}", land.code());
                if !report.unknown_codes.contains(&note) {
                    report.unknown_codes.push(note);
                }
            }
        }
    }

    // 2. The InVeKoS group.
    if let Some(group) = group_of(feature) {
        if field.code_raw.is_empty() {
            field.code_raw = group.clone();
        }
        if let Some(weights) = table.group_weights(&group)
            && let Some(class) = stats::draw(weights, seed)
        {
            field.crop = class;
            // A group with one possible answer is as good as declared.
            field.level = if weights.len() == 1 {
                Level::Declared
            } else {
                Level::Group
            };
            return;
        }
    }

    // 3. Nothing but the block. Grassland says itself; arable is drawn from
    // what the region grows.
    if is_grassland(feature) {
        field.crop = CropClass::Grassland;
        field.level = Level::Drawn;
        return;
    }
    if let Some(weights) = table.arable_weights(region_of(field.land))
        && let Some(class) = stats::draw(weights, seed)
    {
        field.crop = class;
        field.level = Level::Drawn;
        return;
    }
    field.crop = CropClass::Other;
    field.level = Level::Drawn;
}

/// Saxony: the crop is on a point, the shape is on a reference parcel. Each
/// point is given to the polygon it falls in.
fn join_points(
    land: Land,
    points: &[RawFeature],
    polygons: &[RawFeature],
    table: &CropTable,
    report: &mut ImportReport,
) -> Vec<FieldFeature> {
    let mut out = Vec::new();
    for polygon in polygons {
        let Some(ring) = polygon.rings.first() else {
            continue;
        };
        // The parcel the point sits in gives the crop; the block gives the
        // shape. A block with several points keeps the first — the rest are
        // other crops in the same block, which needs a boundary nobody
        // publishes.
        let found = points
            .iter()
            .find(|p| p.point.is_some_and(|p| geometry::contains(ring, p)));
        let attributes = found.unwrap_or(polygon);
        let mut field = FieldFeature {
            polygon: ring.clone(),
            zone: land.utm_zone(),
            land: Some(land),
            year: year_of(attributes).or_else(|| year_of(polygon)),
            code_raw: String::new(),
            code_text: first_text(attributes, TEXT_KEYS).unwrap_or_default(),
            crop: CropClass::Other,
            level: Level::Drawn,
            direction: 0.0,
            area_ha: first_number(attributes, AREA_KEYS).unwrap_or(0.0),
            organic: first_flag(attributes, ORGANIC_KEYS),
            id: first_text(polygon, ID_KEYS).unwrap_or_default(),
        };
        resolve_crop(&mut field, attributes, table, report);
        out.push(field);
    }
    out
}

// ---------------------------------------------------------------------------
// Shaping
// ---------------------------------------------------------------------------

/// Moves a field into another UTM zone, through geodetic coordinates. A no-op
/// where the zones already match, which is the usual case.
fn reproject(field: &mut FieldFeature, zone: u8) {
    if field.zone == zone {
        return;
    }
    let from = field.zone;
    for p in &mut field.polygon {
        let (lat, lon) = world_coords::geo::from_utm(p.x, p.y, from);
        let (e, n) = world_coords::geo::to_utm(lat, lon, zone);
        *p = DVec2::new(e, n);
    }
    field.zone = zone;
}

/// A `(lat, lon)` ring in degrees as UTM metres.
fn ring_in_zone(ring: &[(f64, f64)], zone: u8) -> Vec<DVec2> {
    ring.iter()
        .map(|(lat, lon)| {
            let (e, n) = world_coords::geo::to_utm(lat.to_radians(), lon.to_radians(), zone);
            DVec2::new(e, n)
        })
        .collect()
}

/// The query box in a zone's metres, from its corners in degrees. All four
/// corners are converted, not two: a UTM box is not a rectangle in degrees, and
/// taking only the diagonal would cut the corners off.
fn box_in_zone(min_lat: f64, min_lon: f64, max_lat: f64, max_lon: f64, zone: u8) -> (DVec2, DVec2) {
    let corners = [
        (min_lat, min_lon),
        (min_lat, max_lon),
        (max_lat, min_lon),
        (max_lat, max_lon),
    ];
    let points = ring_in_zone(&corners, zone);
    geometry::bounds(&points)
}

/// Punches the track corridors out of one ring, and returns the pieces. Every
/// corridor quad in turn, over whatever is left — the same pass the import
/// runs, and the one a hand-drawn field goes through on closing.
pub fn punch(piece: &[DVec2], corridors: &[Vec<DVec2>]) -> Vec<Vec<DVec2>> {
    let mut pieces = vec![piece.to_vec()];
    for quad in corridors {
        if pieces.is_empty() {
            break;
        }
        pieces = pieces
            .iter()
            .flat_map(|piece| punch_one(piece, quad))
            .collect();
    }
    pieces
}

/// One corridor quad out of one piece.
///
/// A quad the punch leaves standing — the difference found no usable crossing
/// and fell back to containment — with its middle inside the piece is the end
/// of a siding standing in a field, and it is exactly where the crop then
/// draws over the rails. Such a quad is stretched along its own axis, the way
/// the track came, until the cut crosses the piece's boundary; the field is
/// then notched or split the normal way.
fn punch_one(piece: &[DVec2], quad: &[DVec2]) -> Vec<Vec<DVec2>> {
    let punched = geometry::clip(piece, quad, Op::Difference);
    let untouched = punched.len() == 1
        && ((geometry::area(&punched[0]) - geometry::area(piece)).abs()
            < 1e-6 * geometry::area(piece).abs().max(1.0));
    if !untouched || !geometry::contains(piece, geometry::centroid(quad)) {
        return punched;
    }
    // Enclosed. A step of the quad's own width is enough for consecutive quads
    // to overlap; the piece's diagonal bounds the walk.
    let (min, max) = geometry::bounds(piece);
    let width = quad[0].distance(quad[3]).max(1.0);
    let steps = (min.distance(max) / width).ceil() as usize + 1;
    let mut stretched = quad.to_vec();
    for _ in 0..steps {
        stretched = geometry::stretch(&stretched, width);
        if geometry::crossings(piece, &stretched) > 0 {
            return geometry::clip(piece, &stretched, Op::Difference);
        }
    }
    // No exit found: leave the field whole rather than invent geometry.
    punched
}

/// Thins, clips and measures one field — the pieces of it that survive.
fn shape(
    field: &FieldFeature,
    boundary: &[DVec2],
    corridors: &[Vec<DVec2>],
    options: &ImportOptions,
    report: &mut ImportReport,
) -> Vec<FieldFeature> {
    let ring = geometry::simplify(&field.polygon, options.simplify);
    if ring.len() < 3 {
        return Vec::new();
    }

    // Against the area first: most fields of a fetched tile are outside it, and
    // there is no point punching a track out of those.
    let pieces = match options.clip {
        Clip::Cut => geometry::clip(&ring, boundary, Op::Intersect),
        Clip::Whole => {
            if geometry::contains(boundary, geometry::centroid(&ring)) {
                vec![ring]
            } else {
                Vec::new()
            }
        }
    };
    if pieces.is_empty() {
        report.outside += 1;
        return Vec::new();
    }

    // Then the track. Each corridor quad in turn, over whatever is left.
    let pieces: Vec<Vec<DVec2>> = pieces
        .iter()
        .flat_map(|piece| punch(piece, corridors))
        .collect();

    let mut out = Vec::new();
    for (index, piece) in pieces.into_iter().enumerate() {
        if geometry::area(&piece).abs() < options.min_area {
            report.too_small += 1;
            continue;
        }
        let mut part = field.clone();
        // A field cut in two is two fields, and each needs an id of its own or
        // the second would be taken for a duplicate of the first.
        if index > 0 && !part.id.is_empty() {
            part.id = format!("{}#{index}", part.id);
        }
        part.direction = geometry::min_area_rect(&piece)
            .map(|rect| rect.angle)
            .unwrap_or(0.0);
        part.polygon = piece;
        out.push(part);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wfs;

    fn table() -> CropTable {
        CropTable::built_in()
    }

    fn no_report() -> ImportReport {
        ImportReport::default()
    }

    fn feature(json: &str) -> RawFeature {
        let body = format!(r#"{{"type":"FeatureCollection","features":[{json}]}}"#);
        wfs::parse(body.as_bytes()).expect("parses").remove(0)
    }

    const RING: &str = r#""geometry":{"type":"Polygon","coordinates":[[[440000,5715000],[440300,5715000],[440300,5715300],[440000,5715300]]]}"#;

    #[test]
    fn north_rhine_westphalia_maps_its_own_code() {
        let f = feature(&format!(
            r#"{{"type":"Feature",{RING},"properties":{{"ID":9511274,"CODE":115,"CODE_TXT":"Winterweichweizen","USE_CODE":"GT","AREA_HA":6.08,"ORGANICFAR":"false","VALIDFROM":2026}}}}"#
        ));
        let mut report = no_report();
        let field = normalise(Land::Nw, &f, &table(), &mut report).expect("a field");
        assert_eq!(field.crop, CropClass::WinterCereal);
        assert_eq!(field.level, Level::Declared);
        assert_eq!(field.code_raw, "115");
        assert_eq!(field.code_text, "Winterweichweizen");
        assert_eq!(field.year, Some(2026));
        assert_eq!(field.organic, Some(false));
        assert!(report.unknown_codes.is_empty());
    }

    #[test]
    fn lower_saxony_falls_back_to_the_group() {
        let f = feature(&format!(
            r#"{{"type":"Feature","id":"GSA.89077",{RING},"properties":{{"description":"https://registry.gdi-de.org/codelist/de.iacs/CropValue/GT","name":"Cereals","declaredarea":1.17,"organicfarming":false,"validfrom":"2025-12-02"}}}}"#
        ));
        let mut report = no_report();
        let field = normalise(Land::Ni, &f, &table(), &mut report).expect("a field");
        // GT can be winter cereal, summer cereal or maize; whichever is drawn,
        // it is one of those and it is marked as a draw.
        assert!(
            matches!(
                field.crop,
                CropClass::WinterCereal | CropClass::SummerCereal | CropClass::Maize
            ),
            "{:?}",
            field.crop
        );
        assert_eq!(field.level, Level::Group);
        assert_eq!(field.code_raw, "GT");
        assert_eq!(field.year, Some(2025));
    }

    #[test]
    fn a_group_with_one_answer_counts_as_declared() {
        let f = feature(&format!(
            r#"{{"type":"Feature",{RING},"properties":{{"mainCrop":"GM"}}}}"#
        ));
        let mut report = no_report();
        let field = normalise(Land::Sn, &f, &table(), &mut report).expect("a field");
        assert_eq!(field.crop, CropClass::Vegetable);
        assert_eq!(field.level, Level::Declared);
    }

    #[test]
    fn an_unknown_code_is_a_warning_not_a_crash() {
        // Neither the code nor the group is in any table — what a state
        // renumbering its crop list between application years looks like.
        let f = feature(&format!(
            r#"{{"type":"Feature",{RING},"properties":{{"CODE":9999,"USE_CODE":"XX"}}}}"#
        ));
        let mut report = no_report();
        let field = normalise(Land::Nw, &f, &table(), &mut report).expect("a field");
        // The parcel still gets a plausible crop, off the statistics, and the
        // code that nobody knew is named so the table can be corrected.
        assert_eq!(field.level, Level::Drawn);
        assert_eq!(field.code_raw, "9999");
        assert_eq!(report.unknown_codes, vec!["NW 9999"]);
    }

    #[test]
    fn a_field_block_that_says_grassland_is_grassland() {
        let f = feature(&format!(
            r#"{{"type":"Feature",{RING},"properties":{{"hauptbodennutzung":"Dauergrünland"}}}}"#
        ));
        let mut report = no_report();
        let field = normalise(Land::By, &f, &table(), &mut report).expect("a field");
        assert_eq!(field.crop, CropClass::Grassland);
        assert_eq!(field.level, Level::Drawn);
    }

    #[test]
    fn a_field_block_that_says_nothing_is_drawn_from_the_statistics() {
        let mut crops = HashMap::new();
        for seed in 0..200u32 {
            let f = feature(&format!(
                r#"{{"type":"Feature",{RING},"properties":{{"id":"block-{seed}"}}}}"#
            ));
            let mut report = no_report();
            let field = normalise(Land::He, &f, &table(), &mut report).expect("a field");
            assert_eq!(field.level, Level::Drawn);
            *crops.entry(field.crop).or_insert(0) += 1;
        }
        // Winter cereal is the largest share of arable land, so it has to be
        // the largest share of the draws.
        let winner = crops.iter().max_by_key(|(_, n)| **n).map(|(c, _)| *c);
        assert_eq!(winner, Some(CropClass::WinterCereal), "{crops:?}");
        assert!(crops.len() >= 4, "the draw collapsed onto {crops:?}");
    }

    #[test]
    fn the_year_is_read_out_of_every_spelling() {
        for (written, expected) in [
            (r#""VALIDFROM":2026"#, Some(2026)),
            (r#""validfrom":"2025-12-02""#, Some(2025)),
            (r#""validFrom":"01.01.2025""#, Some(2025)),
            (r#""VALIDFROM":"5.5.2026""#, Some(2026)),
            (r#""VALIDFROM":"n/a""#, None),
        ] {
            let f = feature(&format!(
                r#"{{"type":"Feature",{RING},"properties":{{{written}}}}}"#
            ));
            assert_eq!(year_of(&f), expected, "{written}");
        }
    }

    #[test]
    fn saxony_joins_its_points_onto_its_blocks() {
        let points = wfs::parse(
            br#"{"type":"FeatureCollection","features":[
            {"type":"Feature","geometry":{"type":"Point","coordinates":[350100,5670100]},
             "properties":{"id":"AP.1","mainCrop":"GL","declaredArea":3.5,"organicFarming":"TRUE"}}]}"#,
        )
        .expect("parses");
        let blocks = wfs::parse(
            br#"{"type":"FeatureCollection","features":[
            {"type":"Feature","geometry":{"type":"Polygon","coordinates":[[[350000,5670000],[350300,5670000],[350300,5670300],[350000,5670300]]]},
             "properties":{"id":"RP.7"}},
            {"type":"Feature","geometry":{"type":"Polygon","coordinates":[[[351000,5670000],[351300,5670000],[351300,5670300],[351000,5670300]]]},
             "properties":{"id":"RP.8"}}]}"#,
        )
        .expect("parses");
        let mut report = no_report();
        let fields = join_points(Land::Sn, &points, &blocks, &table(), &mut report);
        assert_eq!(fields.len(), 2);
        // The block the point is in takes the point's crop and its id.
        assert_eq!(fields[0].id, "RP.7");
        assert_eq!(fields[0].crop, CropClass::Grassland);
        assert_eq!(fields[0].organic, Some(true));
        // The one without a point still gets a shape, drawn rather than
        // declared.
        assert_eq!(fields[1].id, "RP.8");
        assert_eq!(fields[1].level, Level::Drawn);
    }

    fn osm_parcel(id: &str, tags: &[(&str, &str)]) -> crate::osm::Parcel {
        crate::osm::Parcel {
            // A square in Rhine-Hesse, big enough to survive the minimum area.
            ring: vec![
                (49.900, 8.100),
                (49.900, 8.104),
                (49.903, 8.104),
                (49.903, 8.100),
            ],
            tags: tags
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            id: id.into(),
        }
    }

    #[test]
    fn openstreetmap_land_use_becomes_a_crop() {
        let table = table();
        for (landuse, expected) in [
            ("meadow", CropClass::Grassland),
            ("vineyard", CropClass::Vineyard),
            ("orchard", CropClass::Orchard),
            ("greenhouse_horticulture", CropClass::Vegetable),
        ] {
            let field = osm_field(
                Some(Land::Rp),
                32,
                &osm_parcel("way/1", &[("landuse", landuse)]),
                &table,
            );
            assert_eq!(field.crop, expected, "{landuse}");
            assert_eq!(field.level, Level::Drawn);
            assert_eq!(field.land, Some(Land::Rp));
            assert!(!field.polygon.is_empty());
        }
    }

    #[test]
    fn openstreetmap_farmland_is_drawn_from_the_statistics() {
        let table = table();
        let mut crops = HashMap::new();
        for id in 0..200u32 {
            let parcel = osm_parcel(&format!("way/{id}"), &[("landuse", "farmland")]);
            let field = osm_field(Some(Land::Rp), 32, &parcel, &table);
            assert_eq!(field.level, Level::Drawn);
            *crops.entry(field.crop).or_insert(0) += 1;
        }
        assert!(crops.len() >= 4, "the draw collapsed onto {crops:?}");
        let winner = crops.iter().max_by_key(|(_, n)| **n).map(|(c, _)| *c);
        assert_eq!(winner, Some(CropClass::WinterCereal), "{crops:?}");
    }

    #[test]
    fn a_mapper_who_wrote_the_crop_down_outranks_the_draw() {
        let table = table();
        let parcel = osm_parcel("way/7", &[("landuse", "farmland"), ("crop", "maize")]);
        let field = osm_field(Some(Land::Rp), 32, &parcel, &table);
        assert_eq!(field.crop, CropClass::Maize);
        assert_eq!(field.level, Level::Declared);
        assert_eq!(field.code_text, "maize");
    }

    #[test]
    fn an_unknown_crop_tag_falls_through_rather_than_failing() {
        assert_eq!(crop_tag("wheat"), Some(CropClass::WinterCereal));
        assert_eq!(crop_tag("Grape"), Some(CropClass::Vineyard));
        assert_eq!(crop_tag("triffid"), None);
        let table = table();
        let parcel = osm_parcel("way/8", &[("landuse", "farmland"), ("crop", "triffid")]);
        assert_eq!(
            osm_field(Some(Land::Rp), 32, &parcel, &table).level,
            Level::Drawn
        );
    }

    #[test]
    fn the_same_openstreetmap_way_always_draws_the_same_crop() {
        let table = table();
        let parcel = osm_parcel("way/4711", &[("landuse", "farmland")]);
        let first = osm_field(Some(Land::Rp), 32, &parcel, &table).crop;
        for _ in 0..10 {
            assert_eq!(osm_field(Some(Land::Rp), 32, &parcel, &table).crop, first);
        }
    }

    #[test]
    fn a_query_box_keeps_its_corners() {
        // Degrees to metres bends the box; all four corners have to be inside.
        let (min, max) = box_in_zone(51.5, 8.0, 51.6, 8.2, 32);
        for (lat, lon) in [(51.5f64, 8.0f64), (51.5, 8.2), (51.6, 8.0), (51.6, 8.2)] {
            let (e, n) = world_coords::geo::to_utm(lat.to_radians(), lon.to_radians(), 32);
            assert!(e >= min.x && e <= max.x, "{e}");
            assert!(n >= min.y && n <= max.y, "{n}");
        }
    }

    fn a_field(ring: Vec<DVec2>) -> FieldFeature {
        FieldFeature {
            polygon: ring,
            zone: 32,
            land: Some(Land::Nw),
            year: Some(2026),
            code_raw: "115".into(),
            code_text: "Winterweichweizen".into(),
            crop: CropClass::WinterCereal,
            level: Level::Declared,
            direction: 0.0,
            area_ha: 1.0,
            organic: None,
            id: "f1".into(),
        }
    }

    fn square(x: f64, y: f64, size: f64) -> Vec<DVec2> {
        vec![
            DVec2::new(x, y),
            DVec2::new(x + size, y),
            DVec2::new(x + size, y + size),
            DVec2::new(x, y + size),
        ]
    }

    #[test]
    fn shaping_cuts_a_field_to_the_module() {
        let field = a_field(square(0.0, 0.0, 400.0));
        // Overlapping the field's south-west corner, so the boundary genuinely
        // crosses two of its sides rather than lying on them.
        let boundary = square(-50.0, -50.0, 250.0);
        let mut report = no_report();
        let options = ImportOptions::default();
        let pieces = shape(&field, &boundary, &[], &options, &mut report);
        assert_eq!(pieces.len(), 1);
        assert!(
            (pieces[0].area() - 40_000.0).abs() < 1.0,
            "{}",
            pieces[0].area()
        );
    }

    #[test]
    fn shaping_keeps_a_whole_field_when_asked_to() {
        let field = a_field(square(0.0, 0.0, 400.0));
        let boundary = square(100.0, 100.0, 200.0);
        let mut report = no_report();
        let options = ImportOptions {
            clip: Clip::Whole,
            ..Default::default()
        };
        let pieces = shape(&field, &boundary, &[], &options, &mut report);
        assert_eq!(pieces.len(), 1);
        assert!((pieces[0].area() - 160_000.0).abs() < 1.0);
    }

    #[test]
    fn the_track_splits_a_field_in_two() {
        let field = a_field(square(0.0, 0.0, 400.0));
        let boundary = square(-100.0, -100.0, 600.0);
        let track = geometry::corridor(&[DVec2::new(-50.0, 200.0), DVec2::new(450.0, 200.0)], 45.0);
        let mut report = no_report();
        let pieces = shape(
            &field,
            &boundary,
            &track,
            &ImportOptions::default(),
            &mut report,
        );
        assert_eq!(pieces.len(), 2, "{pieces:?}");
        // Both keep the crop; only the ids differ, so neither is taken for the
        // other's duplicate.
        assert!(pieces.iter().all(|p| p.crop == CropClass::WinterCereal));
        assert_ne!(pieces[0].id, pieces[1].id);
        let total: f64 = pieces.iter().map(|p| p.area()).sum();
        assert!(
            (total - (160_000.0 - 400.0 * 90.0)).abs() < 200.0,
            "{total}"
        );
    }

    #[test]
    fn a_margin_strip_is_dropped() {
        // Ten metres by twenty: a headland, not a field.
        let field = a_field(vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(20.0, 0.0),
            DVec2::new(20.0, 10.0),
            DVec2::new(0.0, 10.0),
        ]);
        let mut report = no_report();
        let pieces = shape(
            &field,
            &square(-100.0, -100.0, 600.0),
            &[],
            &ImportOptions::default(),
            &mut report,
        );
        assert!(pieces.is_empty());
        assert_eq!(report.too_small, 1);
    }

    #[test]
    fn shaping_finds_the_working_direction() {
        // A 300 x 60 field lying north-east.
        let angle: f64 = 45f64.to_radians();
        let (s, c) = angle.sin_cos();
        let ring: Vec<DVec2> = [(0.0, 0.0), (300.0, 0.0), (300.0, 60.0), (0.0, 60.0)]
            .into_iter()
            .map(|(x, y): (f64, f64)| DVec2::new(x * c - y * s, x * s + y * c))
            .collect();
        let field = a_field(ring);
        let mut report = no_report();
        let pieces = shape(
            &field,
            &square(-500.0, -500.0, 1500.0),
            &[],
            &ImportOptions::default(),
            &mut report,
        );
        assert_eq!(pieces.len(), 1);
        assert!(
            (pieces[0].direction - angle).abs() < 1e-3,
            "{}",
            pieces[0].direction
        );
    }

    #[test]
    fn outside_germany_the_import_takes_the_fallback_rather_than_giving_up() {
        // A box in the North Sea: no state holds it, so no register does. The
        // import says which way it went and carries on — a module in Austria or
        // the Netherlands has to get *something*.
        let area = Area {
            boundary: vec![(54.6, 5.9), (54.6, 6.0), (54.7, 6.0), (54.7, 5.9)],
            track: Vec::new(),
        };
        let mut cache = FieldCache::new(std::env::temp_dir().join("fields-abroad"));
        // Offline, so the test does not ask Overpass: the point is which path
        // is taken, and the live test is what proves it fetches.
        cache.offline = true;
        let report = run(
            &area,
            &ImportOptions::default(),
            &cache,
            &table(),
            &mut |_| true,
        );
        assert!(!report.cancelled);
        assert_eq!(report.warnings.len(), 1);
        assert!(
            report.warnings[0].contains("OpenStreetMap"),
            "{:?}",
            report.warnings
        );
        // Nothing came back, because nothing was asked — but nothing broke.
        assert!(report.fields.is_empty());
        assert_eq!(report.fetched, 0);
    }

    #[test]
    fn a_field_from_abroad_names_no_state_and_takes_its_own_zone() {
        let table = table();
        // Marchfeld, east of Vienna: zone 33, and no German state anywhere near.
        let parcel = crate::osm::Parcel {
            ring: vec![(48.20, 16.70), (48.20, 16.71), (48.21, 16.71)],
            tags: [("landuse".to_string(), "farmland".to_string())]
                .into_iter()
                .collect(),
            id: "way/1".into(),
        };
        let zone = crate::land::utm_zone_at(16.705);
        assert_eq!(zone, 33);
        let field = osm_field(None, zone, &parcel, &table);
        assert_eq!(field.land, None);
        assert_eq!(field.zone, 33);
        assert_eq!(field.level, Level::Drawn);
        // The statistics have no Austrian row, so the general one stands in.
        assert_ne!(field.crop, CropClass::Other);
        // And it comes back out in degrees where it went in.
        let ring = field.to_degrees();
        assert!((ring[0].0 - 48.20).abs() < 1e-6, "{:?}", ring[0]);
        assert!((ring[0].1 - 16.70).abs() < 1e-6, "{:?}", ring[0]);
    }

    #[test]
    fn cancelling_stops_the_import() {
        let area = Area {
            boundary: vec![(51.56, 8.10), (51.56, 8.12), (51.58, 8.12), (51.58, 8.10)],
            track: Vec::new(),
        };
        let mut cache = FieldCache::new(std::env::temp_dir().join("fields-cancel"));
        cache.offline = true;
        let mut calls = 0;
        let report = run(
            &area,
            &ImportOptions::default(),
            &cache,
            &table(),
            &mut |_| {
                calls += 1;
                false
            },
        );
        assert!(report.cancelled);
        assert_eq!(calls, 1);
        assert!(report.fields.is_empty());
    }

    #[test]
    fn an_offline_import_of_an_empty_cache_finds_nothing_and_asks_nobody() {
        let area = Area {
            boundary: vec![(51.56, 8.10), (51.56, 8.12), (51.58, 8.12), (51.58, 8.10)],
            track: Vec::new(),
        };
        let mut cache = FieldCache::new(std::env::temp_dir().join("fields-empty"));
        cache.offline = true;
        let report = run(
            &area,
            &ImportOptions::default(),
            &cache,
            &table(),
            &mut |_| true,
        );
        assert!(report.fields.is_empty());
        assert_eq!(report.fetched, 0);
        assert_eq!(report.cached, 0);
        assert!(!report.cancelled);
    }

    #[test]
    fn the_report_counts_what_it_found() {
        let mut report = ImportReport::default();
        for (id, crop) in [
            ("a", CropClass::WinterCereal),
            ("b", CropClass::WinterCereal),
            ("c", CropClass::Maize),
        ] {
            let mut field = a_field(square(0.0, 0.0, 100.0));
            field.id = id.into();
            field.crop = crop;
            report.fields.push(field);
        }
        assert_eq!(
            report.by_crop(),
            vec![(CropClass::WinterCereal, 2), (CropClass::Maize, 1)]
        );
        assert!((report.hectares() - 3.0).abs() < 1e-9);
    }

    /// Centreline samples still inside a surviving piece: the crop then draws
    /// over the rails.
    fn covered<'a>(pieces: impl IntoIterator<Item = &'a [DVec2]>, line: &[DVec2]) -> Vec<DVec2> {
        let pieces: Vec<&[DVec2]> = pieces.into_iter().collect();
        let mut hits = Vec::new();
        for pair in line.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let len = a.distance(b);
            let n = (len / 3.0).ceil() as usize;
            for i in 0..=n {
                let p = a + (b - a) * (i as f64 / n as f64);
                for piece in &pieces {
                    if geometry::contains(piece, p) {
                        hits.push(p);
                        break;
                    }
                }
            }
        }
        hits
    }

    #[test]
    fn a_siding_ending_inside_a_field_is_carved_out() {
        // The track comes in from the west and stops 150 m inside the field —
        // a buffer stop in the middle of the crop. The last corridor quads
        // never cross the boundary, and without the repair pass the
        // containment fallback would keep the whole parcel, rails and all.
        let field = a_field(square(0.0, 0.0, 300.0));
        let track = [DVec2::new(-100.0, 150.0), DVec2::new(150.0, 150.0)];
        let corridors = geometry::corridor(&track, TRACK_CLEARANCE);
        let mut report = no_report();
        let pieces = shape(
            &field,
            &square(-1_000.0, -1_000.0, 2_600.0),
            &corridors,
            &ImportOptions::default(),
            &mut report,
        );
        assert_eq!(pieces.len(), 1, "{pieces:?}");
        // 300 x 300 less the 30 m wide swathe from the boundary to the stop.
        let area: f64 = pieces.iter().map(|f| f.area()).sum();
        assert!((area - (90_000.0 - 30.0 * 165.0)).abs() < 1.0, "{area}");
        assert!(
            covered(pieces.iter().map(|f| f.polygon.as_slice()), &track).is_empty(),
            "crop over the rails"
        );
    }

    #[test]
    fn a_track_wholly_inside_a_field_carves_a_dead_end() {
        // Both ends inside, no boundary crossing anywhere: the corridor has to
        // be notched in from the field's edge all the same.
        let field = a_field(square(0.0, 0.0, 300.0));
        let track = [DVec2::new(50.0, 150.0), DVec2::new(250.0, 150.0)];
        let corridors = geometry::corridor(&track, TRACK_CLEARANCE);
        let mut report = no_report();
        let pieces = shape(
            &field,
            &square(-1_000.0, -1_000.0, 2_600.0),
            &corridors,
            &ImportOptions::default(),
            &mut report,
        );
        assert_eq!(pieces.len(), 1, "{pieces:?}");
        let area: f64 = pieces.iter().map(|f| f.area()).sum();
        // The swathe runs from the field's edge — the notch has to reach the
        // boundary to be connected — to the corridor's far end.
        assert!((area - (90_000.0 - 30.0 * 265.0)).abs() < 1.0, "{area}");
        assert!(
            covered(pieces.iter().map(|f| f.polygon.as_slice()), &track).is_empty(),
            "crop over the rails"
        );
    }

    /// Irregular fields and wandering tracks, the way the registers and a real
    /// line produce them: no surviving piece may still cover its track.
    #[test]
    fn no_stress_case_leaves_the_track_under_the_crop() {
        let mut rng = Lcg(2024);
        let mut untouched = 0;
        for case in 0..200 {
            let n = 30 + case % 40;
            let field: Vec<DVec2> = (0..n)
                .map(|i| {
                    let a = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
                    let r = 180.0 * rng.range(0.7, 1.3);
                    DVec2::new(200.0 + r * a.cos(), 200.0 + r * a.sin())
                })
                .collect();
            let mut line = Vec::new();
            let mut x = rng.range(-100.0, 50.0);
            let mut y = rng.range(0.0, 400.0);
            let dir = rng.range(-0.4, 0.4);
            let legs = 2 + case % 4;
            for _ in 0..=legs * 10 {
                line.push(DVec2::new(x, y));
                x += 20.0;
                y += dir * 20.0 + rng.range(-6.0, 6.0);
            }
            let pieces = punch(&field, &geometry::corridor(&line, TRACK_CLEARANCE));
            assert!(
                covered(pieces.iter().map(|p| p.as_slice()), &line).is_empty(),
                "case {case} leaves the track under the crop"
            );
            // Where the track runs through the field at all, the punch has to
            // take the corridor away, not leave it standing.
            if !covered([field.as_slice()], &line).is_empty()
                && pieces.len() == 1
                && (geometry::area(&pieces[0]).abs() - geometry::area(&field).abs()).abs() < 1.0
            {
                untouched += 1;
            }
        }
        assert_eq!(untouched, 0, "cases where the punch did nothing at all");
    }

    /// A small deterministic generator, so a failure says which case to look
    /// at and the same seed brings it back.
    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self) -> f64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((self.0 >> 11) as f64) / (1u64 << 53) as f64
        }

        fn range(&mut self, a: f64, b: f64) -> f64 {
            a + (b - a) * self.next()
        }
    }
}

/// End to end against the real services. Ignored by default: it needs a network
/// and it asks a public service, neither of which belongs in `cargo test`.
///
///     cargo test -p fields -- --ignored --nocapture
#[cfg(test)]
mod live {
    use super::*;

    /// Whether Overpass turned this run away. It is donated capacity and
    /// answers 429 or 504 under load; the import already backs off once, and a
    /// test that fails on the second refusal is testing somebody else's uptime
    /// rather than this code. The German register services are dedicated and
    /// are held to the stricter standard.
    fn overpass_busy(report: &ImportReport) -> bool {
        let busy = report
            .warnings
            .iter()
            .any(|w| w.contains("503") || w.contains("504") || w.contains("429"));
        if busy {
            println!("Overpass is busy — skipping: {:?}", report.warnings);
        }
        busy
    }

    #[test]
    #[ignore = "asks a public service over the network"]
    fn a_module_abroad_still_gets_fields() {
        // The Marchfeld, east of Vienna: as arable as the Boerde, in UTM zone
        // 33, and outside every German register. Austria publishes its own
        // IACS data; until that is read, this is what a line there gets.
        let area = Area {
            boundary: vec![
                (48.190, 16.690),
                (48.190, 16.730),
                (48.215, 16.730),
                (48.215, 16.690),
            ],
            track: Vec::new(),
        };
        let cache = FieldCache::new(std::env::temp_dir().join("fields-live-at"));
        cache.clear();
        // A module there would be built in zone 33; the fetch works that out
        // from the longitude, the delivery takes it from the line.
        let options = ImportOptions {
            zone: 33,
            ..Default::default()
        };
        let report = run(&area, &options, &cache, &CropTable::built_in(), &mut |p| {
            println!("{:?} {}/{} {}", p.stage, p.done, p.total, p.note);
            true
        });
        println!("warnings: {:?}", report.warnings);
        println!(
            "{} ways -> {} fields, {:.1} ha",
            report.parcels,
            report.fields.len(),
            report.hectares()
        );
        for (crop, count) in report.by_crop() {
            println!("  {:>14} {count}", crop.id());
        }
        println!("credits: {}", report.attribution.block());
        if overpass_busy(&report) {
            return;
        }

        assert!(!report.fields.is_empty(), "the Marchfeld is farmed");
        // No state is claimed, and the fields come back in the line's zone.
        assert!(report.fields.iter().all(|f| f.land.is_none()));
        assert!(report.fields.iter().all(|f| f.zone == 33));
        // And they land where they were asked for, not a zone's width away.
        for field in &report.fields {
            for (lat, lon) in field.to_degrees() {
                assert!((48.1..48.3).contains(&lat), "{lat}");
                assert!((16.6..16.8).contains(&lon), "{lon}");
            }
        }
        // The one warning is the one that says which way it went.
        assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
        assert!(report.warnings[0].contains("OpenStreetMap"));
        assert!(report.attribution.block().contains("ODbL"));
        cache.clear();
    }

    #[test]
    #[ignore = "asks a public service over the network"]
    fn rhineland_palatinate_through_openstreetmap() {
        // Rhine-Hesse, west of Mainz: vineyards and arable, and no InVeKoS
        // service anywhere in the state.
        let area = Area {
            boundary: vec![
                (49.880, 8.060),
                (49.880, 8.090),
                (49.900, 8.090),
                (49.900, 8.060),
            ],
            track: Vec::new(),
        };
        let cache = FieldCache::new(std::env::temp_dir().join("fields-live-rp"));
        cache.clear();
        let report = run(
            &area,
            &ImportOptions::default(),
            &cache,
            &CropTable::built_in(),
            &mut |p| {
                println!("{:?} {}/{} {}", p.stage, p.done, p.total, p.note);
                true
            },
        );
        println!("warnings: {:?}", report.warnings);
        println!(
            "{} ways -> {} fields, {:.1} ha",
            report.parcels,
            report.fields.len(),
            report.hectares()
        );
        for (crop, count) in report.by_crop() {
            println!("  {:>14} {count}", crop.id());
        }
        println!("credits: {}", report.attribution.block());
        if overpass_busy(&report) {
            return;
        }

        assert!(!report.fields.is_empty(), "Rhine-Hesse is farmed");
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert!(report.fields.iter().all(|f| f.land == Some(Land::Rp)));
        // OpenStreetMap is share-alike, and the module has to say so.
        assert!(report.attribution.block().contains("OpenStreetMap"));
        assert!(report.attribution.block().contains("ODbL"));
        cache.clear();
    }

    #[test]
    #[ignore = "asks a public service over the network"]
    fn a_square_kilometre_of_the_soester_boerde() {
        // The Soester Boerde proper, north-east of the town: wheat, barley,
        // sugar beet and maize, in fields of five hectares and up.
        let area = Area {
            boundary: vec![
                (51.585, 8.140),
                (51.585, 8.170),
                (51.600, 8.170),
                (51.600, 8.140),
            ],
            track: Vec::new(),
        };
        let cache = FieldCache::new(std::env::temp_dir().join("fields-live"));
        cache.clear();
        let report = run(
            &area,
            &ImportOptions::default(),
            &cache,
            &CropTable::built_in(),
            &mut |p| {
                println!("{:?} {}/{} {}", p.stage, p.done, p.total, p.note);
                true
            },
        );
        println!("warnings: {:?}", report.warnings);
        println!("unknown codes: {:?}", report.unknown_codes);
        println!(
            "{} parcels -> {} fields, {:.1} ha, {} split, {} too small, {} outside",
            report.parcels,
            report.fields.len(),
            report.hectares(),
            report.split,
            report.too_small,
            report.outside
        );
        for (crop, count) in report.by_crop() {
            println!("  {:>14} {count}", crop.id());
        }
        println!("credits: {}", report.attribution.block());

        assert!(!report.fields.is_empty(), "the Boerde is not empty");
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert!(
            report
                .fields
                .iter()
                .any(|f| f.crop == CropClass::WinterCereal),
            "no winter cereal in the Soester Boerde"
        );
        // Everything came from North Rhine-Westphalia's own code list.
        assert!(report.fields.iter().all(|f| f.level == Level::Declared));
        assert!(report.attribution.block().contains("dl-de/by-2-0"));

        // The second run must be the cache, and must produce the same fields.
        let again = run(
            &area,
            &ImportOptions::default(),
            &cache,
            &CropTable::built_in(),
            &mut |_| true,
        );
        assert_eq!(again.fetched, 0);
        assert!(again.cached > 0);
        assert_eq!(again.fields, report.fields);
        cache.clear();
    }
}
