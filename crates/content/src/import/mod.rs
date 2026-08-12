//! Streckenimport aus OSM und DGM (Plan Kap. 15).
//!
//! Ablauf: Overpass-JSON → Wegkette → lokales ENU → Abtastung → Trassierung →
//! [`LineSource`]. Die CRS-Umrechnung passiert genau hier; zur Laufzeit gibt es kein UTM
//! (Plan 4.2).

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

/// Einstellungen des Imports.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportOptions {
    /// Name der entstehenden Strecke.
    pub name: String,
    /// Trassierungsparameter.
    pub alignment: AlignmentOptions,
    /// Maximale Kantenlänge [m] — längere Strecken werden aufgeteilt.
    pub max_edge_length: f64,
    /// Geoid-Undulation für die Höhenumrechnung [m].
    pub geoid_offset: f64,
    /// Zulässige Geschwindigkeit, wenn OSM keine angibt [km/h].
    pub default_speed: f64,
    /// Höhe, wenn kein DGM vorliegt [m].
    pub default_height: f64,
    /// Optional: Weg-ID, bei der die Kette beginnen soll.
    pub start_way: Option<i64>,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            name: "Importierte Strecke".into(),
            alignment: AlignmentOptions::default(),
            max_edge_length: 2_000.0,
            geoid_offset: 46.0,
            default_speed: 100.0,
            default_height: 100.0,
            start_way: None,
        }
    }
}

/// Was der Import gefunden hat.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportReport {
    pub length: f64,
    pub edges: usize,
    pub points: usize,
    /// Größte Abweichung der Trassierung von den OSM-Punkten [m].
    ///
    /// Achtung: gemessen wird gegen die **OSM-Linie**, nicht gegen die echte Trasse.
    /// OSM selbst liegt aus Luftbildern nur auf wenige Meter genau.
    pub max_deviation: f64,
    /// Rekonstruierte Entwurfselemente (Geraden, Übergangsbögen, Kreisbögen).
    pub elements: usize,
    /// Davon Kreisbögen.
    pub arcs: usize,
    /// Größte eingebaute Überhöhung [mm].
    pub max_cant: f64,
    /// Kleinster rekonstruierter Radius [m].
    pub min_radius: Option<f64>,
    /// Anteil der Stützpunkte mit DGM-Höhe.
    pub height_coverage: f64,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImportError {
    Osm(OsmError),
    /// Strecke zu kurz für die gewählte Abtastung.
    TooShort,
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportError::Osm(e) => write!(f, "{e}"),
            ImportError::TooShort => write!(f, "Strecke zu kurz für die gewählte Abtastung"),
        }
    }
}

/// Importiert eine Strecke aus Overpass-JSON und optionalem Höhenraster.
pub fn import_line(
    osm_json: &str,
    heights: Option<&mut TerrainSource>,
    options: &ImportOptions,
) -> Result<(LineSource, ImportReport), ImportError> {
    let railway = osm::parse(osm_json).map_err(ImportError::Osm)?;
    let route = railway.chain(options.start_way).map_err(ImportError::Osm)?;

    // Lokales ENU-Frame am Streckenanfang — die Trassierung rechnet eben.
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

    // Höhe und zulässige Geschwindigkeit je Stützpunkt.
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

    // In Kanten aufteilen: kurze Kanten halten den Fehler der ebenen Trassierung klein.
    // Geschnitten wird an Elementgrenzen, damit kein Bogen zerrissen wird.
    let chunks = split_into_edges(&fitted, options.max_edge_length);

    let mut nodes = vec![NodeSource::Buffer];
    for _ in 1..chunks.len() {
        nodes.push(NodeSource::Joint);
    }
    nodes.push(NodeSource::Buffer);

    // Kompassrichtung des Streckenanfangs (0° = Nord, im Uhrzeigersinn).
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
            railway
                .name()
                .unwrap_or_else(|| "Importierte Strecke".into())
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
        warnings.push("kein DGM angegeben — Strecke liegt eben".into());
    }
    // Bis etwa 15 m ist die Abweichung Auflösungsgrenze, nicht Fehler: Bogenanfang und
    // -ende lassen sich aus einer Punktfolge nicht genauer bestimmen, und OSM selbst liegt
    // nur auf wenige Meter genau. Darüber stimmt etwas nicht — meist ein Abschnitt, der
    // aus mehreren Bögen besteht.
    if fitted.max_deviation > 15.0 {
        warnings.push(format!(
            "Trassierung weicht bis {:.1} m von den OSM-Punkten ab — Abschnitt prüfen",
            fitted.max_deviation
        ));
    }
    // Beim Verketten teilen sich benachbarte Wege je einen Knoten. Bleibt die Kette
    // deutlich darunter, hing ein Weg nicht an der Strecke (Abzweig, Betriebsgleis).
    let expected: usize = railway.ways.iter().map(|w| w.nodes.len()).sum::<usize>()
        - (railway.ways.len().saturating_sub(1));
    if route.len() + 1 < expected {
        warnings.push(format!(
            "nur {} von {} OSM-Knoten verkettet — Abzweige ignoriert",
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

/// Teilt die Segmentkette in Kanten, ohne Elemente zu zerschneiden.
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

/// Lokaler ENU-Punkt → geodätische Koordinaten (Grad).
fn frame_to_geodetic(frame: &EnuFrame, p: DVec2) -> (f64, f64) {
    let ecef = frame.to_ecef_curved(DVec3::new(p.x, p.y, 0.0));
    let (lat, lon, _) = geo::from_ecef(ecef);
    (lat.to_degrees(), lon.to_degrees())
}

/// Schneidet ein Stufenprofil auf `[offset, offset + len)` zu und verschiebt es an den
/// Kantenanfang.
fn shift_profile(steps: &[(f64, f64)], offset: f64, len: f64) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    // Wert, der am Kantenanfang gilt.
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
