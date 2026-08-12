//! Command line tool: OSM (Overpass JSON) + DGM → line source file (RON).
//!
//! ```text
//! import-line line.json [--dgm dgm.xyz --epsg 25832] [--name "Musterbahn"]
//!                       [--sample 20] [--smoothing 3] [--out line.ron]
//! ```

use content::import::dgm::TerrainSource;
use content::import::{ImportOptions, import_line};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "--help" {
        eprintln!(
            "Usage: import-line <overpass.json> [--dgm <file.xyz> --epsg <25832>] \
             [--name <name>] [--sample <m>] [--smoothing <n>] [--no-snap]              [--max-cant <mm>] [--out <file.ron>]"
        );
        return ExitCode::from(2);
    }

    let flag = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };

    let osm_path = &args[0];
    let Ok(osm_json) = std::fs::read_to_string(osm_path) else {
        eprintln!("Error: {osm_path} is not readable");
        return ExitCode::FAILURE;
    };

    let epsg: u32 = flag("--epsg").and_then(|v| v.parse().ok()).unwrap_or(25832);
    let Some(zone) = world_coords::geo::utm_zone_from_epsg(epsg) else {
        eprintln!("Error: EPSG:{epsg} is not a supported UTM zone (25831…25835)");
        return ExitCode::FAILURE;
    };

    // --dgm takes a single file or a whole directory full of tiles.
    let mut grid = match flag("--dgm") {
        Some(path) => {
            let p = std::path::Path::new(&path);
            let source = if p.is_dir() {
                TerrainSource::from_dir(p, zone)
            } else {
                std::fs::read_to_string(p).and_then(|text| {
                    content::import::dgm::HeightTile::parse(&text, zone)
                        .map(TerrainSource::from_tile)
                        .map_err(|e| {
                            std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
                        })
                })
            };
            match source {
                Ok(s) => {
                    eprintln!("DGM: {} tile(s) from {path}", s.tile_count());
                    Some(s)
                }
                Err(e) => {
                    eprintln!("Error in the DGM: {e}");
                    return ExitCode::FAILURE;
                }
            }
        }
        None => None,
    };

    let mut options = ImportOptions {
        name: flag("--name").unwrap_or_default(),
        ..Default::default()
    };
    if let Some(v) = flag("--sample").and_then(|v| v.parse().ok()) {
        options.alignment.sample = v;
    }
    if let Some(v) = flag("--smoothing").and_then(|v| v.parse().ok()) {
        options.alignment.smoothing = v;
    }
    if args.iter().any(|a| a == "--no-snap") {
        options.alignment.snap_radii = false;
    }
    if let Some(v) = flag("--max-cant").and_then(|v| v.parse().ok()) {
        options.alignment.cant.max_cant = v;
    }
    if let Some(v) = flag("--start-way").and_then(|v| v.parse().ok()) {
        options.start_way = Some(v);
    }

    let (line, report) = match import_line(&osm_json, grid.as_mut(), &options) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Import failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    eprintln!(
        "{}: {:.0} m, {} edges, {} elements ({} curves), heights {:.0} %",
        line.name,
        report.length,
        report.edges,
        report.elements,
        report.arcs,
        report.height_coverage * 100.0
    );
    eprintln!(
        "  smallest radius {}, largest cant {:.0} mm, deviation from the OSM line {:.1} m",
        report
            .min_radius
            .map(|r| format!("{r:.0} m"))
            .unwrap_or_else(|| "—".into()),
        report.max_cant,
        report.max_deviation
    );
    for w in &report.warnings {
        eprintln!("Note: {w}");
    }

    let ron = line.to_ron();
    match flag("--out") {
        Some(path) => {
            if std::fs::write(&path, ron).is_err() {
                eprintln!("Error: {path} is not writable");
                return ExitCode::FAILURE;
            }
            eprintln!("written: {path}");
        }
        None => println!("{ron}"),
    }
    ExitCode::SUCCESS
}
