//! Where the model is allowed to look, and where a find may be used.
//!
//! This is the answer to the two things a builder actually asks for. "Along
//! the track, out to eighty metres" is [`Shape::Corridor`]; "in here" — an
//! area drawn in the viewport — is [`Shape::Polygon`]. Both carry the same
//! second condition, [`Region::keep_clear`]: however the area was chosen,
//! nothing is placed within so many metres of a rail. A car standing in the
//! four-foot is worse than no car at all, and the clearance is the one rule
//! that has to hold in either mode.
//!
//! A region is also what makes the run affordable. [`Region::covers_box`] is
//! asked before a window is fetched, so a corridor eighty metres wide across
//! a ten-kilometre module downloads and infers a ribbon rather than a square.
//!
//! Distances are metres in the module's own UTM zone — the same arithmetic
//! the roads and the fields use, and near enough to exact at the scale of a
//! module that no correction is worth its complexity.

use world_coords::geo;

/// The shape the user chose.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    /// Everything out to `radius` metres either side of the track.
    Corridor { radius: f64 },
    /// Everything inside a closed polygon, in degrees, in either winding.
    Polygon(Vec<(f64, f64)>),
}

/// A point in the region's metric frame [m].
#[derive(Debug, Clone, Copy, PartialEq)]
struct Metric {
    e: f64,
    n: f64,
}

/// The area to work in, in the form the run needs it.
#[derive(Debug, Clone)]
pub struct Region {
    pub shape: Shape,
    /// How close to a rail anything may come [m]. Holds in both shapes.
    pub keep_clear: f64,
    /// UTM zone the metres are measured in.
    pub zone: u8,
    /// The track as polylines in metres — one per edge, sampled fine enough
    /// that the straight line between two samples is the curve.
    track: Vec<Vec<Metric>>,
    /// The polygon in metres, empty for a corridor.
    polygon: Vec<Metric>,
    /// The carriageways nothing may be placed on.
    carriageways: Vec<Carriageway>,
}

/// A road's running surface: its centre line in the region's metres, and how
/// far the tarmac reaches either side of it.
#[derive(Debug, Clone)]
struct Carriageway {
    line: Vec<Metric>,
    half_width: f64,
}

impl Region {
    /// `track` is one polyline of degrees per track edge.
    pub fn new(shape: Shape, track: &[Vec<(f64, f64)>], keep_clear: f64, zone: u8) -> Self {
        let to_metric = |(lat, lon): &(f64, f64)| Metric {
            e: geo::to_utm(lat.to_radians(), lon.to_radians(), zone).0,
            n: geo::to_utm(lat.to_radians(), lon.to_radians(), zone).1,
        };
        let polygon = match &shape {
            Shape::Polygon(points) => points.iter().map(to_metric).collect(),
            Shape::Corridor { .. } => Vec::new(),
        };
        Self {
            shape,
            keep_clear,
            zone,
            track: track
                .iter()
                .filter(|line| line.len() >= 2)
                .map(|line| line.iter().map(to_metric).collect())
                .collect(),
            polygon,
            carriageways: Vec::new(),
        }
    }

    /// The roads whose running surface is out of bounds.
    ///
    /// One `(centre line in degrees, kerb-to-kerb width [m])` per road. A car
    /// park paved as a ribbon is not one of these and must not be passed in —
    /// it is the one place a car most belongs.
    #[must_use]
    pub fn off_the_carriageway(mut self, roads: &[(Vec<(f64, f64)>, f64)]) -> Self {
        let zone = self.zone;
        self.carriageways = roads
            .iter()
            .filter(|(line, width)| line.len() >= 2 && *width > 0.0)
            .map(|(line, width)| Carriageway {
                line: line
                    .iter()
                    .map(|(lat, lon)| {
                        let (e, n) = geo::to_utm(lat.to_radians(), lon.to_radians(), zone);
                        Metric { e, n }
                    })
                    .collect(),
                half_width: width / 2.0,
            })
            .collect();
        self
    }

    /// Whether the point stands on a road's running surface.
    ///
    /// Measured to the kerb and no further. A car parked at the roadside has
    /// its centre just beyond the kerb line, which is exactly where the rule
    /// has to let it stand: kerbside parking is most of the traffic beside a
    /// street, and a margin drawn to be safe would delete it.
    fn on_a_carriageway(&self, p: Metric) -> bool {
        self.carriageways.iter().any(|road| {
            road.line
                .windows(2)
                .any(|pair| point_to_segment(p, pair[0], pair[1]) < road.half_width)
        })
    }

    /// Metres from a point in degrees to the nearest rail — [`f64::MAX`] where
    /// there is no track at all, so a module without one is "far away from the
    /// track" rather than "too close to it".
    pub fn track_distance(&self, lat: f64, lon: f64) -> f64 {
        let (e, n) = geo::to_utm(lat.to_radians(), lon.to_radians(), self.zone);
        self.track_distance_metric(Metric { e, n })
    }

    fn track_distance_metric(&self, p: Metric) -> f64 {
        let mut best = f64::MAX;
        for line in &self.track {
            for pair in line.windows(2) {
                best = best.min(point_to_segment(p, pair[0], pair[1]));
            }
        }
        best
    }

    /// Whether something found at this point may be used.
    pub fn contains(&self, lat: f64, lon: f64) -> bool {
        let (e, n) = geo::to_utm(lat.to_radians(), lon.to_radians(), self.zone);
        let p = Metric { e, n };
        let distance = self.track_distance_metric(p);
        if distance < self.keep_clear {
            return false;
        }
        if self.on_a_carriageway(p) {
            return false;
        }
        match &self.shape {
            Shape::Corridor { radius } => distance <= *radius,
            Shape::Polygon(_) => inside(&self.polygon, p),
        }
    }

    /// Whether a window of imagery is worth fetching at all: does the region
    /// reach into this box of degrees `(west, south, east, north)`?
    ///
    /// Conservative on purpose — a window that only clips the corner of the
    /// corridor is still fetched. Being wrong the other way would drop cars.
    pub fn covers_box(&self, west: f64, south: f64, east: f64, north: f64) -> bool {
        let corner = |lat: f64, lon: f64| {
            let (e, n) = geo::to_utm(lat.to_radians(), lon.to_radians(), self.zone);
            Metric { e, n }
        };
        let sw = corner(south, west);
        let ne = corner(north, east);
        let (min_e, max_e) = (sw.e.min(ne.e), sw.e.max(ne.e));
        let (min_n, max_n) = (sw.n.min(ne.n), sw.n.max(ne.n));
        match &self.shape {
            Shape::Corridor { radius } => self.track.iter().any(|line| {
                line.windows(2).any(|pair| {
                    segment_near_box(pair[0], pair[1], min_e, min_n, max_e, max_n, *radius)
                })
            }),
            Shape::Polygon(_) => {
                if self.polygon.len() < 3 {
                    return false;
                }
                // Either the box holds a corner of the polygon, or the polygon
                // holds a corner of the box, or their edges cross. Together
                // that covers every way two convex-or-not shapes can meet.
                let in_box =
                    |p: Metric| p.e >= min_e && p.e <= max_e && p.n >= min_n && p.n <= max_n;
                if self.polygon.iter().copied().any(in_box) {
                    return true;
                }
                let corners = [
                    Metric { e: min_e, n: min_n },
                    Metric { e: max_e, n: min_n },
                    Metric { e: max_e, n: max_n },
                    Metric { e: min_e, n: max_n },
                ];
                if corners.iter().copied().any(|c| inside(&self.polygon, c)) {
                    return true;
                }
                for i in 0..self.polygon.len() {
                    let a = self.polygon[i];
                    let b = self.polygon[(i + 1) % self.polygon.len()];
                    for j in 0..4 {
                        if segments_cross(a, b, corners[j], corners[(j + 1) % 4]) {
                            return true;
                        }
                    }
                }
                false
            }
        }
    }

    /// The box of degrees the whole region fits in `(west, south, east,
    /// north)` — where the run starts walking.
    pub fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
        let points: Vec<Metric> = match &self.shape {
            Shape::Corridor { radius } => self
                .track
                .iter()
                .flatten()
                .flat_map(|p| {
                    [
                        Metric {
                            e: p.e - radius,
                            n: p.n - radius,
                        },
                        Metric {
                            e: p.e + radius,
                            n: p.n + radius,
                        },
                    ]
                })
                .collect(),
            Shape::Polygon(_) => self.polygon.clone(),
        };
        if points.is_empty() {
            return None;
        }
        let min_e = points.iter().map(|p| p.e).fold(f64::MAX, f64::min);
        let max_e = points.iter().map(|p| p.e).fold(f64::MIN, f64::max);
        let min_n = points.iter().map(|p| p.n).fold(f64::MAX, f64::min);
        let max_n = points.iter().map(|p| p.n).fold(f64::MIN, f64::max);
        // Both diagonals: a UTM box is not a box in degrees, and the corner
        // that sticks out furthest depends on the hemisphere.
        let corners = [
            geo::from_utm(min_e, min_n, self.zone),
            geo::from_utm(max_e, min_n, self.zone),
            geo::from_utm(min_e, max_n, self.zone),
            geo::from_utm(max_e, max_n, self.zone),
        ];
        let lats: Vec<f64> = corners.iter().map(|(lat, _)| lat.to_degrees()).collect();
        let lons: Vec<f64> = corners.iter().map(|(_, lon)| lon.to_degrees()).collect();
        Some((
            lons.iter().copied().fold(f64::MAX, f64::min),
            lats.iter().copied().fold(f64::MAX, f64::min),
            lons.iter().copied().fold(f64::MIN, f64::max),
            lats.iter().copied().fold(f64::MIN, f64::max),
        ))
    }
}

/// Distance from a point to a segment [m].
fn point_to_segment(p: Metric, a: Metric, b: Metric) -> f64 {
    let (dx, dy) = (b.e - a.e, b.n - a.n);
    let length2 = dx * dx + dy * dy;
    if length2 < 1e-9 {
        return ((p.e - a.e).powi(2) + (p.n - a.n).powi(2)).sqrt();
    }
    let t = (((p.e - a.e) * dx + (p.n - a.n) * dy) / length2).clamp(0.0, 1.0);
    let (cx, cy) = (a.e + t * dx, a.n + t * dy);
    ((p.e - cx).powi(2) + (p.n - cy).powi(2)).sqrt()
}

/// Whether a segment comes within `radius` of an axis-aligned box.
fn segment_near_box(
    a: Metric,
    b: Metric,
    min_e: f64,
    min_n: f64,
    max_e: f64,
    max_n: f64,
    radius: f64,
) -> bool {
    // The grown box first: it is one comparison and rejects nearly everything.
    let (lo_e, hi_e) = (a.e.min(b.e), a.e.max(b.e));
    let (lo_n, hi_n) = (a.n.min(b.n), a.n.max(b.n));
    if hi_e < min_e - radius
        || lo_e > max_e + radius
        || hi_n < min_n - radius
        || lo_n > max_n + radius
    {
        return false;
    }
    // Then the real distance: the segment against the box's four sides, and
    // the box's corners against the segment. A segment that runs through the
    // box is caught by the first corner test.
    let corners = [
        Metric { e: min_e, n: min_n },
        Metric { e: max_e, n: min_n },
        Metric { e: max_e, n: max_n },
        Metric { e: min_e, n: max_n },
    ];
    if corners.iter().any(|&c| point_to_segment(c, a, b) <= radius) {
        return true;
    }
    for i in 0..4 {
        let (c, d) = (corners[i], corners[(i + 1) % 4]);
        if point_to_segment(a, c, d) <= radius || point_to_segment(b, c, d) <= radius {
            return true;
        }
    }
    false
}

/// Point in polygon, by the crossing number.
fn inside(polygon: &[Metric], p: Metric) -> bool {
    if polygon.len() < 3 {
        return false;
    }
    let mut hit = false;
    let mut j = polygon.len() - 1;
    for i in 0..polygon.len() {
        let (a, b) = (polygon[i], polygon[j]);
        if (a.n > p.n) != (b.n > p.n) {
            let x = (b.e - a.e) * (p.n - a.n) / (b.n - a.n) + a.e;
            if p.e < x {
                hit = !hit;
            }
        }
        j = i;
    }
    hit
}

/// Whether two segments cross.
fn segments_cross(a: Metric, b: Metric, c: Metric, d: Metric) -> bool {
    let side = |p: Metric, q: Metric, r: Metric| {
        let v = (q.e - p.e) * (r.n - p.n) - (q.n - p.n) * (r.e - p.e);
        if v > 1e-9 {
            1
        } else if v < -1e-9 {
            -1
        } else {
            0
        }
    };
    let (d1, d2) = (side(a, b, c), side(a, b, d));
    let (d3, d4) = (side(c, d, a), side(c, d, b));
    d1 != d2 && d3 != d4
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A straight run of track through the middle of a German module.
    fn track() -> Vec<Vec<(f64, f64)>> {
        vec![vec![(51.0, 7.0), (51.0, 7.01)]]
    }

    fn corridor(radius: f64, keep_clear: f64) -> Region {
        Region::new(Shape::Corridor { radius }, &track(), keep_clear, 32)
    }

    /// Degrees of latitude for a distance in metres — a metre north is a
    /// metre north wherever you are.
    fn north_of(lat: f64, metres: f64) -> f64 {
        lat + metres / 111_132.0
    }

    /// A street 8 m kerb to kerb, 40 m north of the track and running east
    /// along the first fifth of it.
    fn with_a_street(radius: f64) -> Region {
        let lat = north_of(51.0, 40.0);
        corridor(radius, 5.0).off_the_carriageway(&[(vec![(lat, 7.0), (lat, 7.002)], 8.0)])
    }

    #[test]
    fn a_find_in_a_running_lane_is_refused() {
        let region = with_a_street(200.0);
        // On the centre line, and one metre off it — both are tarmac a lorry
        // is meant to come down.
        assert!(!region.contains(north_of(51.0, 40.0), 7.001));
        assert!(!region.contains(north_of(51.0, 41.0), 7.001));
        // And three and a half metres out, which is still inside the kerb.
        assert!(!region.contains(north_of(51.0, 43.5), 7.001));
    }

    #[test]
    fn a_car_parked_at_the_kerb_is_kept() {
        let region = with_a_street(200.0);
        // Four metres from the centre line is the kerb itself; a car standing
        // against it has its middle just beyond. This is most of the traffic
        // beside a street, and a clearance drawn to be safe would delete it.
        assert!(region.contains(north_of(51.0, 44.2), 7.001));
        assert!(region.contains(north_of(51.0, 46.0), 7.001));
    }

    #[test]
    fn a_street_only_blocks_where_it_runs() {
        let region = with_a_street(200.0);
        // Past the end of the way, on the line it would have if it went on —
        // and still well inside the corridor, so only the street could refuse
        // it.
        assert!(region.contains(north_of(51.0, 40.0), 7.006));
    }

    #[test]
    fn without_roads_nothing_is_on_one() {
        let region = corridor(200.0, 5.0);
        assert!(region.contains(north_of(51.0, 40.0), 7.005));
    }

    /// The clearance still wins: a street laid over the track does not make
    /// the four-foot placeable, and neither does it make it more forbidden.
    #[test]
    fn the_rules_stack_rather_than_replace_each_other() {
        let region = with_a_street(200.0);
        assert!(
            !region.contains(north_of(51.0, 1.0), 7.005),
            "in the four-foot"
        );
        assert!(
            !region.contains(north_of(51.0, 300.0), 7.005),
            "outside the corridor"
        );
        assert!(region.contains(north_of(51.0, 20.0), 7.005), "beside both");
    }

    #[test]
    fn the_corridor_reaches_out_to_its_radius_and_no_further() {
        let region = corridor(80.0, 6.0);
        assert!(region.contains(north_of(51.0, 40.0), 7.005));
        assert!(region.contains(north_of(51.0, 75.0), 7.005));
        assert!(!region.contains(north_of(51.0, 120.0), 7.005));
    }

    #[test]
    fn nothing_is_placed_within_the_clearance_of_a_rail() {
        let region = corridor(80.0, 6.0);
        assert!(
            !region.contains(51.0, 7.005),
            "on the track itself is the one place a car may never stand"
        );
        assert!(!region.contains(north_of(51.0, 3.0), 7.005));
        assert!(region.contains(north_of(51.0, 8.0), 7.005));
    }

    #[test]
    fn the_clearance_holds_in_an_area_too() {
        // An area drawn straight over the track: everything but the track bed.
        let polygon = vec![
            (north_of(51.0, -60.0), 7.002),
            (north_of(51.0, -60.0), 7.008),
            (north_of(51.0, 60.0), 7.008),
            (north_of(51.0, 60.0), 7.002),
        ];
        let region = Region::new(Shape::Polygon(polygon), &track(), 6.0, 32);
        assert!(region.contains(north_of(51.0, 30.0), 7.005));
        assert!(
            !region.contains(51.0, 7.005),
            "the area was drawn over the track, the clearance still holds"
        );
        assert!(
            !region.contains(north_of(51.0, 30.0), 7.02),
            "east of the area, however far from the track"
        );
    }

    #[test]
    fn a_window_beside_the_corridor_is_never_fetched() {
        let region = corridor(80.0, 6.0);
        // A window over the track.
        assert!(region.covers_box(7.004, 50.9995, 7.006, 51.0005));
        // One a kilometre north of it.
        let far = north_of(51.0, 1_000.0);
        assert!(!region.covers_box(7.004, far, 7.006, north_of(far, 100.0)));
        // One that only clips the edge of the corridor is still taken.
        let edge = north_of(51.0, 70.0);
        assert!(region.covers_box(7.004, edge, 7.006, north_of(edge, 100.0)));
    }

    #[test]
    fn a_window_inside_an_area_is_taken_and_one_outside_is_not() {
        let polygon = vec![(50.99, 6.99), (50.99, 7.02), (51.02, 7.02), (51.02, 6.99)];
        let region = Region::new(Shape::Polygon(polygon), &track(), 6.0, 32);
        assert!(region.covers_box(7.0, 51.0, 7.001, 51.001), "well inside");
        assert!(region.covers_box(6.985, 51.0, 6.995, 51.001), "straddling");
        assert!(!region.covers_box(7.1, 51.0, 7.11, 51.001), "well outside");
    }

    #[test]
    fn the_bounds_of_a_corridor_are_the_track_grown_by_its_radius() {
        let region = corridor(80.0, 6.0);
        let (west, south, east, north) = region.bounds().unwrap();
        assert!(west < 7.0 && east > 7.01, "grown east and west");
        assert!(south < 51.0 && north > 51.0, "grown north and south");
        // 80 m is roughly 0.0007° of latitude — the box must not be wildly
        // bigger than that.
        assert!(north - 51.0 < 0.002, "{north}");
    }

    #[test]
    fn a_region_without_track_is_far_from_the_track_rather_than_on_it() {
        let region = Region::new(
            Shape::Polygon(vec![(51.0, 7.0), (51.0, 7.01), (51.01, 7.01)]),
            &[],
            6.0,
            32,
        );
        assert_eq!(region.track_distance(51.0, 7.0), f64::MAX);
        assert!(region.contains(51.002, 7.008));
    }
}
