//! Command line tool: re-fetches what a module takes from public data and
//! writes the line file back — the headless form of the route editor's
//! File ▸ Import fields, File ▸ Import roads and Height data (DGM) panel.
//!
//! ```text
//! import-module --line mods/example/lines/boerde.ron
//!               [--no-fields] [--no-roads] [--no-power]
//!               [--tracks] [--narrow] [--refresh-fields]
//!               [--dgm <dir>] [--fetch-dgm nrw] [--zone 32] [--cell 10]
//!               [--no-fit-track] [--list-dgm-tiles]
//! ```
//!
//! * **Fields** — the agricultural registers (InVeKoS), the module envelope
//!   as the area, the track punched out. Replaces what an earlier import put
//!   into the module; hand-drawn fields stay.
//! * **Roads** — Overpass over the envelope's box. Replaces the road list:
//!   the module is being rebuilt, so the previous import's roads go with it.
//! * **Heights** — the corridor's terrain tiles are sampled out of a DGM
//!   delivery into `<mod>/heights/<line>/` (one ESRI ASCII grid each) and
//!   the line records them, so the module carries its ground. With
//!   `--fetch-dgm nrw` the missing GeoTIFF sheets are downloaded from NRW's
//!   open data first. The track is fitted to the imported ground unless
//!   `--no-fit-track` says otherwise — a module whose rails ignore the DGM
//!   runs in a cutting for its whole length.

use content::TerrainBuilder;
use content::import::dgm::{HeightTile, TerrainSource};
use content::route::{EdgeStart, HeightSource, LineSource, PowerLineSource, RoadSource};
use fields::{Area, Clip, CropTable, FieldCache, ImportOptions, ImportReport};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use track_model::TrackNetwork;
use world_coords::geo;

/// North Rhine-Westphalia's open data: DGM1 as GeoTIFF, one tile per square
/// kilometre, named after its south-west corner in kilometres. (C) Geobasis
/// NRW, dl-de/by-2-0 — a module built on it carries the source note on.
const NRW_BASE: &str = "https://www.opengeodata.nrw.de/produkte/geobasis/hm/dgm1_tiff/dgm1_tiff/";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|a| a == "--help") {
        eprintln!(
            "Usage: import-module --line <file.ron> [--no-fields] [--no-roads] [--no-power]\n\
             \x20                     [--tracks] [--narrow] [--refresh-fields]\n\
             \x20                     [--dgm <dir>] [--fetch-dgm nrw] [--zone 32] [--cell 10]\n\
             \x20                     [--no-fit-track] [--list-dgm-tiles]"
        );
        return ExitCode::from(2);
    }
    let flag = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let set = |name: &str| args.iter().any(|a| a == name);

    let Some(line_path) = flag("--line").map(PathBuf::from) else {
        eprintln!("Error: --line <file.ron> is required");
        return ExitCode::from(2);
    };
    let do_fields = !set("--no-fields");
    let do_roads = !set("--no-roads");
    let do_power = !set("--no-power");
    let tracks = set("--tracks");
    let narrow = set("--narrow");
    let refresh_fields = set("--refresh-fields");
    let dgm_dir = flag("--dgm").map(PathBuf::from);
    let fetch_nrw = flag("--fetch-dgm").is_some_and(|v| v == "nrw");
    let zone: u8 = flag("--zone").and_then(|v| v.parse().ok()).unwrap_or(32);
    let cell: f64 = flag("--cell").and_then(|v| v.parse().ok()).unwrap_or(10.0);
    let fit = !set("--no-fit-track");
    let list_tiles = set("--list-dgm-tiles");

    let Ok(text) = std::fs::read_to_string(&line_path) else {
        eprintln!("Error: {} is not readable", line_path.display());
        return ExitCode::FAILURE;
    };
    // The leading comment block is documentation, not data — a rebuilt file
    // keeps it (the counts in it are the author's to refresh).
    let header = text
        .lines()
        .take_while(|l| l.trim_start().starts_with("//") || l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let mut line = match LineSource::from_ron(&text) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Error: {} is not a line file: {e}", line_path.display());
            return ExitCode::FAILURE;
        }
    };
    let compiled = match line.compile() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: the line does not compile: {e:?}");
            return ExitCode::FAILURE;
        }
    };
    let net = compiled.net;

    // The corridor's terrain tiles — the same set the editor's DGM panel
    // cuts and the runtime's terrain streaming builds.
    let options = content::TerrainOptions {
        zone,
        geoid_offset: line.geoid_offset,
        ..Default::default()
    };
    let tiles = TerrainBuilder::new(&net, Vec::new(), options).corridor_keys();
    let sheets = needed_sheets(&tiles, options.tile_size, cell);

    if list_tiles {
        for (x, y) in sheets {
            println!("dgm1_{zone}_{x}_{y}_1_nw");
        }
        return ExitCode::SUCCESS;
    }

    if do_fields {
        let report = import_fields(&line, &net, refresh_fields, zone);
        print_field_report(&report);
        line.apply_field_import(&report, true);
    }

    if do_roads {
        let Some(bbox) = envelope_bbox(&line) else {
            eprintln!("Error: the line has no envelope to import inside");
            return ExitCode::FAILURE;
        };
        match import_roads(bbox, tracks, narrow) {
            Ok(roads) => {
                print_road_report(&roads, zone);
                line.roads = roads;
            }
            Err(e) => {
                eprintln!("Error: the road import failed: {e}");
                return ExitCode::FAILURE;
            }
        }
    }

    if do_power {
        let Some(bbox) = envelope_bbox(&line) else {
            eprintln!("Error: the line has no envelope to import inside");
            return ExitCode::FAILURE;
        };
        match import_power(bbox) {
            Ok(power) => {
                print_power_report(&power, zone);
                line.power_lines = power;
            }
            Err(e) => {
                eprintln!("Error: the overhead line import failed: {e}");
                return ExitCode::FAILURE;
            }
        }
    }

    // The corridor's terrain tiles — the same set the editor's DGM panel
    // cuts and the runtime's terrain streaming builds.
    let tiles = TerrainBuilder::new(&net, Vec::new(), options).corridor_keys();
    let sheets = needed_sheets(&tiles, options.tile_size, cell);

    if let (Some(dir), true) = (&dgm_dir, fetch_nrw)
        && let Err(e) = fetch_missing_nrw(dir, &sheets, zone)
    {
        eprintln!("Error: fetching the DGM delivery failed: {e}");
        return ExitCode::FAILURE;
    }

    if let Some(dir) = &dgm_dir {
        let source = match TerrainSource::from_dir(dir, zone) {
            Ok(s) if s.tile_count() > 0 => s,
            Ok(_) => {
                eprintln!("Error: {} holds no DGM tiles", dir.display());
                return ExitCode::FAILURE;
            }
            Err(e) => {
                eprintln!("Error: {} is not a DGM directory: {e}", dir.display());
                return ExitCode::FAILURE;
            }
        };
        eprintln!(
            "DGM: {} sheet(s) in the delivery, {} needed by the corridor",
            source.tile_count(),
            sheets.len()
        );
        source.set_cache_limit(16);

        let Some((height_dir, qualified)) = height_dir(&line_path) else {
            eprintln!(
                "Error: {} is not inside a mod (<mod>/lines/<line>.ron)",
                line_path.display()
            );
            return ExitCode::FAILURE;
        };
        let (written, empty) = cut_heights(&source, &tiles, &options, cell, &height_dir);
        eprintln!(
            "Heights: {written} tile(s) written to {}, {empty} without data",
            height_dir.display()
        );
        line.heights = vec![HeightSource {
            path: qualified,
            zone,
        }];

        if fit && let Ok(false) = fit_track_to_ground(&mut line, &net, &source) {
            eprintln!("Track: the ground is not covered everywhere, left as it is");
        }
    }

    let out = format!("{header}\n{}", line.to_ron());
    if let Err(e) = std::fs::write(&line_path, out) {
        eprintln!("Error: could not write {}: {e}", line_path.display());
        return ExitCode::FAILURE;
    }
    eprintln!("Line: {} written", line_path.display());
    ExitCode::SUCCESS
}

/// The module envelope as a box, south-west to north-east — what the Overpass
/// query is asked with.
fn envelope_bbox(line: &LineSource) -> Option<(f64, f64, f64, f64)> {
    let corners = &line.envelope;
    (corners.len() >= 3).then(|| {
        (
            corners.iter().map(|p| p.lat).fold(f64::MAX, f64::min),
            corners.iter().map(|p| p.lon).fold(f64::MAX, f64::min),
            corners.iter().map(|p| p.lat).fold(f64::MIN, f64::max),
            corners.iter().map(|p| p.lon).fold(f64::MIN, f64::max),
        )
    })
}

/// The track, sampled as a polyline per edge, in degrees — what the field
/// import punches its corridors out of. Every twenty metres, as the editor's
/// import does: closer adds vertices the corridor quads do not need.
fn track_polylines(net: &TrackNetwork) -> Vec<Vec<(f64, f64)>> {
    net.edges()
        .iter()
        .filter_map(|edge| {
            let length = edge.length();
            (length > 0.0).then(|| {
                let steps = (length / 20.0).ceil().max(1.0) as usize;
                (0..=steps)
                    .map(|i| {
                        let s = length * i as f64 / steps as f64;
                        let (lat, lon, _) = geo::from_ecef(edge.eval(s).pos);
                        (lat.to_degrees(), lon.to_degrees())
                    })
                    .collect()
            })
        })
        .collect()
}

/// Asks the agricultural registers for the module envelope. Same options as
/// the editor's dialog defaults: cut at the boundary, half a hectare at
/// least, the terrain's blend zone kept clear of the track.
fn import_fields(line: &LineSource, net: &TrackNetwork, refresh: bool, zone: u8) -> ImportReport {
    let boundary: Vec<(f64, f64)> = line.envelope.iter().map(|p| (p.lat, p.lon)).collect();
    let area = Area {
        boundary,
        track: track_polylines(net),
    };
    let options = ImportOptions {
        clip: Clip::Cut,
        min_area: 5_000.0,
        track_clearance: fields::import::TRACK_CLEARANCE,
        zone,
        ..Default::default()
    };
    let mut cache = FieldCache::new("cache/fields");
    cache.refresh = refresh;
    cache.offline = false;
    let table = CropTable::built_in();
    let mut stage = None;
    fields::import::run(&area, &options, &cache, &table, &mut |p| {
        // Progress on stderr, one line per stage change.
        if stage != Some(p.stage) || p.done == p.total {
            stage = Some(p.stage);
            eprintln!("Fields: {:?} {}/{}", p.stage, p.done, p.total);
        }
        true
    })
}

/// Prints the field import's summary — the counts a module's header comment
/// quotes.
fn print_field_report(report: &ImportReport) {
    let mut crops: BTreeMap<&str, usize> = BTreeMap::new();
    for field in &report.fields {
        *crops.entry(field.crop.id()).or_default() += 1;
    }
    let hectares: f64 = report.fields.iter().map(|f| f.area_ha).sum();
    eprintln!(
        "Fields: {} field(s), {hectares:.0} ha ({} fetched, {} from the cache, {} dropped as too small, {} outside, {} split)",
        report.fields.len(),
        report.fetched,
        report.cached,
        report.too_small,
        report.outside,
        report.split
    );
    for (id, count) in &crops {
        eprintln!("    {id}: {count}");
    }
    for warning in &report.warnings {
        eprintln!("    warning: {warning}");
    }
    for (land, year) in &report.attribution.used {
        eprintln!("    source: {land} {year:?}");
    }
}

/// The Overpass query for the envelope's box, sent the way the editor's road
/// import sends it — same etiquette, one retry when Overpass is busy.
fn import_roads(
    bbox: (f64, f64, f64, f64),
    tracks: bool,
    narrow: bool,
) -> Result<Vec<RoadSource>, String> {
    let query = content::import::roads_query(bbox.0, bbox.1, bbox.2, bbox.3);
    let config = fields::RequestConfig::default();
    let json = fields::osm::fetch_raw(&query, &config).map_err(|e| e.to_string())?;
    let roads = content::import::parse_roads(&json).map_err(|e| e.to_string())?;
    Ok(roads
        .into_iter()
        .filter(|road| {
            road.tags
                .first()
                .and_then(|t| t.strip_prefix("highway-"))
                .is_some_and(|class| allowed(tracks, narrow, class))
        })
        .collect())
}

/// Whether an OSM class passes the dialog's filters. The narrow classes and
/// the tracks are opt-in — both are many, and thin.
fn allowed(tracks: bool, narrow: bool, class: &str) -> bool {
    match class {
        "track" => tracks,
        "living_street" | "pedestrian" => narrow,
        "service" => tracks || narrow,
        _ => true,
    }
}

/// The overhead lines of the envelope's box, fetched the way the roads are.
fn import_power(bbox: (f64, f64, f64, f64)) -> Result<Vec<PowerLineSource>, String> {
    let query = content::import::power_query(bbox.0, bbox.1, bbox.2, bbox.3);
    let config = fields::RequestConfig::default();
    let json = fields::osm::fetch_raw(&query, &config).map_err(|e| e.to_string())?;
    content::import::parse_power_lines(&json).map_err(|e| e.to_string())
}

/// Prints the overhead line import's summary — the mast types and how many of
/// each stand on the module, which is what says whether the type choice came
/// out right before anybody starts the editor.
fn print_power_report(lines: &[PowerLineSource], zone: u8) {
    let mut types: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for line in lines {
        let id = line.tags.first().map(String::as_str).unwrap_or("other");
        let entry = types.entry(id).or_default();
        entry.0 += 1;
        entry.1 += line.points.len();
    }
    let length: f64 = lines.iter().map(|l| l.length(zone)).sum();
    let masts: usize = lines.iter().map(|l| l.points.len()).sum();
    eprintln!(
        "Overhead lines: {} line(s), {masts} mast(s), {:.1} km",
        lines.len(),
        length / 1000.0
    );
    for (id, (count, masts)) in &types {
        eprintln!("    {id}: {count} line(s), {masts} mast(s)");
    }
}

/// Prints the road import's summary — classes and centre-line length, as the
/// module's header quotes them.
fn print_road_report(roads: &[RoadSource], zone: u8) {
    let mut classes: BTreeMap<&str, usize> = BTreeMap::new();
    for road in roads {
        let class = road
            .tags
            .first()
            .and_then(|t| t.strip_prefix("highway-"))
            .unwrap_or("other");
        *classes.entry(class).or_default() += 1;
    }
    let length: f64 = roads.iter().map(|r| r.length(zone)).sum();
    eprintln!(
        "Roads: {} carriageway(s), {:.1} km of centre line",
        roads.len(),
        length / 1000.0
    );
    for (class, count) in &classes {
        eprintln!("    {class}: {count}");
    }
}

/// `<mod>/heights/<line>` and `<id>:heights/<line>` — where the module's own
/// height tiles live and how the line names them.
fn height_dir(line_path: &Path) -> Option<(PathBuf, String)> {
    let stem = line_path.file_stem()?.to_str()?.to_string();
    // `<mod>/lines/<line>.ron` — the mod directory is two levels up.
    let mod_dir = line_path.parent()?.parent()?;
    let manifest = std::fs::read_to_string(mod_dir.join("mod.ron")).ok()?;
    let id = manifest
        .lines()
        .find_map(|l| l.trim().strip_prefix("id:")?.trim().split('"').nth(1))?
        .to_string();
    Some((
        mod_dir.join("heights").join(&stem),
        format!("{id}:heights/{stem}"),
    ))
}

/// The DGM sheets (1 km × 1 km, named after their south-west corner) a
/// corridor's terrain tiles are sampled from — every kilometre the tiles'
/// extents, plus their one-cell border, touch.
fn needed_sheets(tiles: &[content::TileKey], tile_size: f64, cell: f64) -> BTreeSet<(i64, i64)> {
    let mut sheets = BTreeSet::new();
    for key in tiles {
        let min = content::terrain::tile_min(*key, tile_size);
        let lo = min - glam::dvec2(cell, cell);
        let hi = min + glam::dvec2(tile_size + cell, tile_size + cell);
        for x in (lo.x / 1000.0).floor() as i64..=(hi.x / 1000.0).floor() as i64 {
            for y in (lo.y / 1000.0).floor() as i64..=(hi.y / 1000.0).floor() as i64 {
                sheets.insert((x, y));
            }
        }
    }
    sheets
}

/// Downloads the corridor's DGM1 sheets that the delivery directory does not
/// hold yet. NRW publishes every tile of its DGM1 on open data; the index
/// page is asked once, then each missing tile is fetched by name (the year
/// suffix in the names varies with the survey date).
fn fetch_missing_nrw(dir: &Path, sheets: &BTreeSet<(i64, i64)>, zone: u8) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let have: Vec<String> = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    let missing: Vec<(i64, i64)> = sheets
        .iter()
        .copied()
        .filter(|(x, y)| {
            !have
                .iter()
                .any(|name| name.starts_with(&format!("dgm1_{zone}_{x}_{y}_1_nw")))
        })
        .collect();
    if missing.is_empty() {
        eprintln!("DGM: all {} sheet(s) already in the delivery", sheets.len());
        return Ok(());
    }

    eprintln!(
        "DGM: fetching the index of NRW's open data ({} sheet(s) missing)…",
        missing.len()
    );
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(120)))
        .user_agent("connected-rails-import-module")
        .build()
        .into();
    let index = agent
        .get(NRW_BASE)
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    for (x, y) in missing {
        let prefix = format!("dgm1_{zone}_{x}_{y}_1_nw");
        let Some(start) = index.find(&prefix) else {
            eprintln!("    {prefix}: not in NRW's delivery, skipped");
            continue;
        };
        // The name runs to the first character a file name cannot have — the
        // index line continues with the closing quote.
        let rest = &index[start..];
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
            .collect();
        eprintln!("    {name}…");
        let bytes = agent
            .get(&format!("{NRW_BASE}{name}"))
            .call()
            .map_err(|e| e.to_string())?
            .body_mut()
            .read_to_vec()
            .map_err(|e| e.to_string())?;
        std::fs::write(dir.join(&name), bytes).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Samples the corridor's terrain tiles out of the delivery — the module
/// carries its ground as one ESRI ASCII grid per tile, exactly what the
/// editor's Height data (DGM) panel writes. Tiles the delivery has no data
/// for are skipped rather than shipped as a plate of zeros.
fn cut_heights(
    source: &TerrainSource,
    tiles: &[content::TileKey],
    options: &content::TerrainOptions,
    cell: f64,
    dir: &Path,
) -> (usize, usize) {
    std::fs::create_dir_all(dir).ok();
    let sources = std::slice::from_ref(source);
    let (mut written, mut empty) = (0, 0);
    for key in tiles {
        let min = content::terrain::tile_min(*key, options.tile_size);
        let tile = HeightTile::sample(
            sources,
            options.zone,
            (min.x, min.y),
            options.tile_size,
            cell,
        );
        if tile.is_empty() {
            empty += 1;
            continue;
        }
        let file = dir.join(format!("x{}_y{}.asc", key.0, key.1));
        match std::fs::write(&file, tile.to_asc()) {
            Ok(()) => written += 1,
            Err(e) => eprintln!("    {}: {e}", file.display()),
        }
    }
    (written, empty)
}

/// Moves the track onto the imported ground: the start height and the grade
/// profile of every edge are fitted to the DGM in half-kilometre nodes, the
/// grades rounded to a tenth of a permille the way an alignment is. Returns
/// `false` — and changes nothing — when the ground is not covered under the
/// whole track: a half-fitted line is worse than an unfitted one.
fn fit_track_to_ground(
    line: &mut LineSource,
    net: &TrackNetwork,
    source: &TerrainSource,
) -> Result<bool, String> {
    const NODE: f64 = 500.0;
    const SAMPLE: f64 = 25.0;

    // The ground under every edge, as (s, height) samples in NHN.
    let mut profiles: Vec<Vec<(f64, f64)>> = Vec::new();
    for edge in net.edges() {
        let length = edge.length();
        let steps = (length / SAMPLE).ceil().max(1.0) as usize;
        let mut samples = Vec::with_capacity(steps + 1);
        for i in 0..=steps {
            let s = length * i as f64 / steps as f64;
            let (lat, lon, _) = geo::from_ecef(edge.eval(s).pos);
            if let Some(h) = source.height_at(lat.to_degrees(), lon.to_degrees()) {
                samples.push((s, h));
            }
        }
        let covered = !samples.is_empty()
            && samples[0].0 < 1e-9
            && length - samples[samples.len() - 1].0 < SAMPLE + 1e-9;
        if !covered {
            return Ok(false);
        }
        profiles.push(samples);
    }

    // What a node stands on: not one point of the DGM — at 1 m spacing that
    // is a field bank or a ditch, and the track would wiggle with every
    // hedge — but the average around it, a quarter kilometre either side.
    // An alignment follows the trend of the land, and the terrain builder
    // shapes the detail it keeps towards the rails.
    let trend = |samples: &[(f64, f64)], s: f64| -> Option<f64> {
        const WINDOW: f64 = 125.0;
        let mut sum = (0.0, 0usize);
        for (x, h) in samples {
            if (*x - s).abs() <= WINDOW {
                sum = (sum.0 + h, sum.1 + 1);
            }
        }
        (sum.1 > 0).then(|| sum.0 / sum.1 as f64)
    };

    // Edges in file order; a `Continue` edge stands on the height the edge
    // before it ends at, so the fit carries the inherited height along.
    let mut inherited: Option<f64> = None;
    for (index, ((edge, track), samples)) in line
        .edges
        .iter_mut()
        .zip(net.edges())
        .zip(profiles.iter())
        .enumerate()
    {
        let length = track.length();
        let is_geo = matches!(edge.start, EdgeStart::Geo { .. });
        let anchor = match (is_geo, inherited) {
            (_, Some(h)) if !is_geo => h,
            _ => trend(samples, 0.0).ok_or("no height at the start of an edge")?,
        };

        // Half-kilometre nodes, the last one at the edge's end.
        let mut nodes: Vec<f64> = Vec::new();
        let mut s = 0.0;
        while s < length - 1e-9 {
            nodes.push(s);
            s += NODE;
        }
        nodes.push(length);
        let mut heights = Vec::with_capacity(nodes.len());
        for &n in &nodes {
            heights.push(trend(samples, n).ok_or("no height at a node")?);
        }

        // One grade per node, rounded to 0.1 ‰ against the height the
        // profile actually reaches — the rounding never drifts further than
        // the next node.
        let mut grade = Vec::with_capacity(nodes.len());
        let mut effective = anchor;
        for i in 0..nodes.len() - 1 {
            let ds = nodes[i + 1] - nodes[i];
            let permille = ((heights[i + 1] - effective) / ds * 1000.0 * 10.0).round() / 10.0;
            grade.push((nodes[i], permille));
            effective += permille * ds / 1000.0;
        }

        if is_geo {
            let EdgeStart::Geo { point, .. } = &mut edge.start else {
                unreachable!("checked above")
            };
            point.height = (anchor * 100.0).round() / 100.0;
        }
        edge.grade = grade;
        inherited = Some(effective);
        eprintln!(
            "Track: edge {index} fitted, {} grade node(s), start at {:.2} m NHN",
            nodes.len(),
            anchor
        );
    }
    Ok(true)
}
