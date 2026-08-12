//! Line import from OSM and DGM (plan ch. 15).
//!
//! Flow: Overpass JSON → way chain → local ENU → sampling → alignment →
//! [`LineSource`]. The CRS conversion happens exactly here; at runtime there is no UTM
//! (plan 4.2).

pub mod alignment;
pub mod dgm;
pub mod fit;
pub mod osm;

use crate::route::{EdgeSource, EdgeStart, GeoPoint, LineSource, NodeSource};
use alignment::AlignmentOptions;
use dgm::TerrainSource;
use fit::SamplePoint;
use glam::{DVec2, DVec3};
use osm::OsmError;
use world_coords::{EnuFrame, geo};

/// Import settings.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportOptions {
    /// Name of the resulting line.
    pub name: String,
    /// Alignment parameters.
    pub alignment: AlignmentOptions,
    /// Maximum edge length [m] — longer lines are split up.
    pub max_edge_length: f64,
    /// Geoid undulation for the height conversion [m].
    pub geoid_offset: f64,
    /// Permitted speed when OSM does not give one [km/h].
    pub default_speed: f64,
    /// Height used when no DGM is available [m].
    pub default_height: f64,
    /// Optional: way ID at which the chain should start.
    pub start_way: Option<i64>,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            name: "Imported line".into(),
            alignment: AlignmentOptions::default(),
            max_edge_length: 2_000.0,
            geoid_offset: 46.0,
            default_speed: 100.0,
            default_height: 100.0,
            start_way: None,
        }
    }
}

/// What the import found.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportReport {
    pub length: f64,
    pub edges: usize,
    pub points: usize,
    /// Largest deviation of the alignment from the OSM points [m].
    ///
    /// Note: this is measured against the **OSM line**, not against the real track.
    /// OSM itself, taken from aerial imagery, is only accurate to a few metres.
    pub max_deviation: f64,
    /// Reconstructed design elements (straights, transition curves, circular arcs).
    pub elements: usize,
    /// Of those, circular arcs.
    pub arcs: usize,
    /// Largest applied cant [mm].
    pub max_cant: f64,
    /// Smallest reconstructed radius [m].
    pub min_radius: Option<f64>,
    /// Share of support points with a DGM height.
    pub height_coverage: f64,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImportError {
    Osm(OsmError),
    /// Line too short for the chosen sampling.
    TooShort,
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportError::Osm(e) => write!(f, "{e}"),
            ImportError::TooShort => write!(f, "line too short for the chosen sampling"),
        }
    }
}

/// Imports a line from Overpass JSON and an optional height grid.
pub fn import_line(
    osm_json: &str,
    heights: Option<&mut TerrainSource>,
    options: &ImportOptions,
) -> Result<(LineSource, ImportReport), ImportError> {
    let railway = osm::parse(osm_json).map_err(ImportError::Osm)?;
    let route = railway.chain(options.start_way).map_err(ImportError::Osm)?;

    // Local ENU frame at the start of the line — the alignment is computed in the plane.
    let first = geo::to_ecef_deg(route[0].lat, route[0].lon, 0.0);
    let frame = EnuFrame::at(first);
    let plan: Vec<DVec2> = route
        .iter()
        .map(|p| {
            let local = frame.to_local(geo::to_ecef_deg(p.lat, p.lon, 0.0));
            DVec2::new(local.x, local.y)
        })
        .collect();

    let resampled = fit::resample(&plan, options.alignment.sample);
    if resampled.len() < 3 {
        return Err(ImportError::TooShort);
    }

    // Height and permitted speed per support point.
    let mut with_height = 0usize;
    let mut heights = heights;
    let mut samples: Vec<SamplePoint> = Vec::with_capacity(resampled.len());
    for (pos, _s, segment) in &resampled {
        let source = &route[(*segment).min(route.len() - 1)];
        let height = heights
            .as_mut()
            .and_then(|g| {
                let (lat, lon) = frame_to_geodetic(&frame, *pos);
                g.height_at(lat, lon)
            })
            .inspect(|_| with_height += 1)
            .unwrap_or(options.default_height);
        samples.push(SamplePoint {
            pos: *pos,
            height,
            speed: source.maxspeed.unwrap_or(options.default_speed),
        });
    }

    let fitted = alignment::fit(&samples, &options.alignment);
    let total_length = fitted.length();

    // Split into edges: short edges keep the error of the planar alignment small.
    // Cuts are made at element boundaries so that no curve is torn apart.
    let chunks = split_into_edges(&fitted, options.max_edge_length);

    let mut nodes = vec![NodeSource::Buffer];
    for _ in 1..chunks.len() {
        nodes.push(NodeSource::Joint);
    }
    nodes.push(NodeSource::Buffer);

    // Compass heading at the start of the line (0° = north, clockwise).
    let start_dir = fitted.start_heading;
    let heading_deg = (90.0 - start_dir.to_degrees()).rem_euclid(360.0);

    let mut edges = Vec::with_capacity(chunks.len());
    let mut offset = 0.0;
    for (i, chunk) in chunks.iter().enumerate() {
        let len: f64 = chunk.iter().map(|s| s.len).sum();
        edges.push(EdgeSource {
            from: i as u32,
            to: i as u32 + 1,
            start: if i == 0 {
                EdgeStart::Geo {
                    point: GeoPoint {
                        lat: route[0].lat,
                        lon: route[0].lon,
                        height: samples[0].height,
                    },
                    heading_deg,
                }
            } else {
                EdgeStart::Continue { edge: i as u32 - 1 }
            },
            segments: chunk.to_vec(),
            grade: shift_profile(&fitted.grade, offset, len),
            cant: shift_profile(&fitted.cant, offset, len),
            speed: shift_profile(&fitted.speed, offset, len),
        });
        offset += len;
    }

    let line = LineSource {
        name: if options.name.is_empty() {
            railway.name().unwrap_or_else(|| "Imported line".into())
        } else {
            options.name.clone()
        },
        geoid_offset: options.geoid_offset,
        nodes,
        edges,
        devices: Vec::new(),
        sections: Vec::new(),
        signals: Vec::new(),
        routes: Vec::new(),
    };

    let mut warnings = Vec::new();
    if heights.is_none() {
        warnings.push("no DGM given — line is flat".into());
    }
    // Up to about 15 m the deviation is a resolution limit, not an error: the start and
    // end of a curve cannot be determined more precisely from a point sequence, and OSM
    // itself is only accurate to a few metres. Beyond that something is wrong — usually
    // a section that consists of several curves.
    if fitted.max_deviation > 15.0 {
        warnings.push(format!(
            "alignment deviates by up to {:.1} m from the OSM points — check section",
            fitted.max_deviation
        ));
    }
    // When chaining, neighbouring ways share one node each. If the chain stays clearly
    // below that, a way was not attached to the line (junction, service track).
    let expected: usize = railway.ways.iter().map(|w| w.nodes.len()).sum::<usize>()
        - (railway.ways.len().saturating_sub(1));
    if route.len() + 1 < expected {
        warnings.push(format!(
            "only {} of {} OSM nodes chained — junctions ignored",
            route.len(),
            expected
        ));
    }

    let report = ImportReport {
        length: total_length,
        edges: line.edges.len(),
        points: samples.len(),
        max_deviation: fitted.max_deviation,
        elements: fitted.elements.len(),
        arcs: fitted.arcs(),
        max_cant: fitted.cant.iter().map(|(_, c)| *c).fold(0.0, f64::max),
        min_radius: fitted
            .elements
            .iter()
            .filter_map(|e| e.radius.map(f64::abs))
            .min_by(f64::total_cmp),
        height_coverage: with_height as f64 / samples.len() as f64,
        warnings,
    };
    Ok((line, report))
}

/// Splits the segment chain into edges without cutting elements apart.
fn split_into_edges(
    alignment: &alignment::Alignment,
    max_length: f64,
) -> Vec<Vec<track_model::Segment>> {
    let mut chunks: Vec<Vec<track_model::Segment>> = Vec::new();
    let mut current: Vec<track_model::Segment> = Vec::new();
    let mut length = 0.0;
    for segment in &alignment.segments {
        if !current.is_empty() && length + segment.len > max_length {
            chunks.push(std::mem::take(&mut current));
            length = 0.0;
        }
        length += segment.len;
        current.push(*segment);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    if chunks.is_empty() {
        chunks.push(Vec::new());
    }
    chunks
}

/// Local ENU point → geodetic coordinates (degrees).
fn frame_to_geodetic(frame: &EnuFrame, p: DVec2) -> (f64, f64) {
    let ecef = frame.to_ecef_curved(DVec3::new(p.x, p.y, 0.0));
    let (lat, lon, _) = geo::from_ecef(ecef);
    (lat.to_degrees(), lon.to_degrees())
}

/// Clips a step profile to `[offset, offset + len)` and shifts it to the start of the
/// edge.
fn shift_profile(steps: &[(f64, f64)], offset: f64, len: f64) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    // Value that applies at the start of the edge.
    if let Some(active) = steps
        .iter()
        .rfind(|(s, _)| *s <= offset)
        .or_else(|| steps.first())
    {
        out.push((0.0, active.1));
    }
    for (s, v) in steps {
        if *s > offset && *s < offset + len {
            out.push((s - offset, *v));
        }
    }
    out
}
