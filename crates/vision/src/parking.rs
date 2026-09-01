//! From cars to car parks: what a crowd of detections says about the ground
//! under it.
//!
//! No model is asked where the car park is. It does not need to be — a car
//! park *is* the cars, and a detector that finds forty of them standing in
//! rows has already drawn the outline. Clustering them is both cheaper and
//! better than a second model: it cannot hallucinate a car park in an empty
//! field, it works with whatever detector the user has, and the rows it finds
//! are the rows the cars are actually in.
//!
//! What comes out is a rectangle, not a free outline. Rectangles are what
//! German car parks are, they survive a missing car at the corner, and — the
//! practical reason — the editor can pave one with the road it already has:
//! a ribbon along the rows, as wide as the rows are deep, with no markings.
//! That needs no new file format, no new mesh and no new tool.

use crate::detect::{GeoDetection, distance};
use world_coords::geo;

/// A car park found under a cluster of cars.
#[derive(Debug, Clone, PartialEq)]
pub struct Lot {
    /// Middle of the rectangle [deg].
    pub lat: f64,
    pub lon: f64,
    /// The four corners [deg], in order around the rectangle.
    pub polygon: Vec<(f64, f64)>,
    /// The two ends of the centre line along the rows [deg] — what the editor
    /// lays the paved ribbon along.
    pub line: ((f64, f64), (f64, f64)),
    /// How wide to lay it [m] — the depth of the rows, aisles included.
    pub width: f64,
    /// Length of the centre line [m].
    pub length: f64,
    /// Direction the rows run, degrees clockwise from north, 0 … 180.
    pub row_heading: f64,
    /// Direction the cars point, degrees clockwise from north, 0 … 180.
    pub car_heading: f64,
    /// How many cars are standing in it.
    pub cars: usize,
}

/// Groups cars into car parks.
///
/// `radius` is how far apart two cars may be and still be in the same car
/// park — bay to bay is under three metres, across an aisle is seven, so
/// something in the high teens joins a car park up without joining two of
/// them that face each other across a street. `min_cars` is what stops three
/// cars on a verge from becoming a car park.
pub fn lots(cars: &[GeoDetection], radius: f64, min_cars: usize, zone: u8) -> Vec<Lot> {
    let mut lots = Vec::new();
    for group in cluster(cars, radius) {
        if group.len() < min_cars.max(1) {
            continue;
        }
        if let Some(lot) = rectangle(&group, cars, zone) {
            lots.push(lot);
        }
    }
    lots.sort_by_key(|lot| std::cmp::Reverse(lot.cars));
    lots
}

/// Single-linkage clustering: two cars closer than `radius` are in the same
/// car park, and so is anything reachable through them. That is the right
/// rule here — a car park is a chain of bays, not a blob around a centre, and
/// a rule that measured from a centre would cut a long one in half.
fn cluster(cars: &[GeoDetection], radius: f64) -> Vec<Vec<usize>> {
    let mut parent: Vec<usize> = (0..cars.len()).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for i in 0..cars.len() {
        for j in (i + 1)..cars.len() {
            if distance(cars[i].lat, cars[i].lon, cars[j].lat, cars[j].lon) <= radius {
                let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                if a != b {
                    parent[a] = b;
                }
            }
        }
    }
    let mut groups: std::collections::HashMap<usize, Vec<usize>> = std::collections::HashMap::new();
    for i in 0..cars.len() {
        let root = find(&mut parent, i);
        groups.entry(root).or_default().push(i);
    }
    let mut out: Vec<Vec<usize>> = groups.into_values().collect();
    // Stable order, so the same imagery gives the same car parks in the same
    // order twice running — the editor names them by number.
    out.sort_by_key(|g| g.first().copied().unwrap_or(0));
    out
}

/// The oriented rectangle a group of cars stands in.
fn rectangle(group: &[usize], cars: &[GeoDetection], zone: u8) -> Option<Lot> {
    if group.is_empty() {
        return None;
    }
    let car_heading = mean_heading(group.iter().map(|&i| cars[i].heading));
    // The rows run across the cars: a bay is entered from the side, so a row
    // of cars standing shoulder to shoulder runs at a right angle to the way
    // they point.
    let row_heading = (car_heading + 90.0).rem_euclid(180.0);
    let along = direction(row_heading);
    let across = direction((row_heading + 90.0).rem_euclid(360.0));

    let metric: Vec<(f64, f64)> = group
        .iter()
        .map(|&i| geo::to_utm(cars[i].lat.to_radians(), cars[i].lon.to_radians(), zone))
        .collect();
    let project = |axis: (f64, f64)| {
        metric
            .iter()
            .map(|(e, n)| e * axis.0 + n * axis.1)
            .fold((f64::MAX, f64::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)))
    };
    let (min_along, max_along) = project(along);
    let (min_across, max_across) = project(across);

    // Half a car either way, plus a metre of tarmac, so the surface reaches
    // past the cars standing on it rather than ending under their wheels.
    let car_length = median(group.iter().map(|&i| cars[i].length)).max(3.0);
    let car_width = median(group.iter().map(|&i| cars[i].width)).max(1.5);
    let pad_along = car_width / 2.0 + 1.0;
    let pad_across = car_length / 2.0 + 1.0;
    let (lo_along, hi_along) = (min_along - pad_along, max_along + pad_along);
    let (lo_across, hi_across) = (min_across - pad_across, max_across + pad_across);

    let centre_along = (lo_along + hi_along) / 2.0;
    let centre_across = (lo_across + hi_across) / 2.0;
    let point = |a: f64, c: f64| {
        let (lat, lon) =
            geo::from_utm(a * along.0 + c * across.0, a * along.1 + c * across.1, zone);
        (lat.to_degrees(), lon.to_degrees())
    };
    let (lat, lon) = point(centre_along, centre_across);
    Some(Lot {
        lat,
        lon,
        polygon: vec![
            point(lo_along, lo_across),
            point(hi_along, lo_across),
            point(hi_along, hi_across),
            point(lo_along, hi_across),
        ],
        line: (
            point(lo_along, centre_across),
            point(hi_along, centre_across),
        ),
        width: hi_across - lo_across,
        length: hi_along - lo_along,
        row_heading,
        car_heading,
        cars: group.len(),
    })
}

/// Unit vector of a compass direction, as `(east, north)`.
fn direction(degrees: f64) -> (f64, f64) {
    let r = degrees.to_radians();
    (r.sin(), r.cos())
}

/// Mean of directions that have no front and no back.
///
/// A car pointing north and one pointing south are parked the same way, so
/// the angles are doubled before they are averaged and halved afterwards.
/// Averaging 5° and 175° directly would give 90° — a car park turned a
/// quarter turn out of true.
pub fn mean_heading(headings: impl Iterator<Item = f64>) -> f64 {
    let (mut x, mut y, mut n) = (0.0, 0.0, 0usize);
    for h in headings {
        let a = (2.0 * h).to_radians();
        x += a.cos();
        y += a.sin();
        n += 1;
    }
    if n == 0 || (x.abs() < 1e-12 && y.abs() < 1e-12) {
        return 0.0;
    }
    (y.atan2(x).to_degrees() / 2.0).rem_euclid(180.0)
}

fn median(values: impl Iterator<Item = f64>) -> f64 {
    let mut values: Vec<f64> = values.collect();
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A car park of `rows` rows and `per_row` bays, all cars pointing the
    /// same way, at a place in the Rhineland.
    fn car_park(rows: usize, per_row: usize, heading: f64) -> Vec<GeoDetection> {
        const METRE_LAT: f64 = 1.0 / 111_132.0;
        let metre_lon = 1.0 / (111_132.0 * 51.0_f64.to_radians().cos());
        let mut cars = Vec::new();
        for row in 0..rows {
            for bay in 0..per_row {
                cars.push(GeoDetection {
                    class: 0,
                    place: "car".into(),
                    score: 0.9,
                    // Bays 2.6 m apart along a row, rows 6 m apart.
                    lat: 51.0 + row as f64 * 6.0 * METRE_LAT,
                    lon: 7.0 + bay as f64 * 2.6 * metre_lon,
                    length: 4.4,
                    width: 1.8,
                    heading,
                });
            }
        }
        cars
    }

    #[test]
    fn a_full_car_park_is_one_lot() {
        // Cars pointing north (0°), so the rows run east-west (90°).
        let cars = car_park(3, 10, 0.0);
        let found = lots(&cars, 18.0, 4, 32);
        assert_eq!(found.len(), 1);
        let lot = &found[0];
        assert_eq!(lot.cars, 30);
        assert!((lot.row_heading - 90.0).abs() < 1.0, "{}", lot.row_heading);
        // Ten bays 2.6 m apart plus a margin, and three rows 6 m apart plus
        // half a car either end.
        assert!((lot.length - 27.0).abs() < 2.0, "{}", lot.length);
        assert!((lot.width - 17.4).abs() < 2.0, "{}", lot.width);
        assert_eq!(lot.polygon.len(), 4);
    }

    #[test]
    fn the_paved_line_runs_along_the_rows_through_the_middle() {
        let cars = car_park(2, 8, 0.0);
        let lot = &lots(&cars, 18.0, 4, 32)[0];
        let (a, b) = lot.line;
        let along = distance(a.0, a.1, b.0, b.1);
        assert!(
            (along - lot.length).abs() < 0.5,
            "{along} vs {}",
            lot.length
        );
        // The middle of the line is the middle of the rectangle.
        let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        assert!(distance(mid.0, mid.1, lot.lat, lot.lon) < 0.5);
    }

    #[test]
    fn two_car_parks_a_street_apart_stay_two() {
        let mut cars = car_park(2, 6, 0.0);
        const METRE_LAT: f64 = 1.0 / 111_132.0;
        for mut car in car_park(2, 6, 0.0) {
            car.lat += 40.0 * METRE_LAT;
            cars.push(car);
        }
        assert_eq!(lots(&cars, 18.0, 4, 32).len(), 2);
    }

    #[test]
    fn three_cars_on_a_verge_are_not_a_car_park() {
        let cars = car_park(1, 3, 0.0);
        assert!(lots(&cars, 18.0, 4, 32).is_empty());
    }

    #[test]
    fn a_car_park_turned_forty_five_degrees_keeps_its_shape() {
        // The rows still have to come out at a right angle to the cars.
        let cars = car_park(3, 10, 45.0);
        let lot = &lots(&cars, 18.0, 4, 32)[0];
        assert!((lot.car_heading - 45.0).abs() < 1.0, "{}", lot.car_heading);
        assert!((lot.row_heading - 135.0).abs() < 1.0, "{}", lot.row_heading);
    }

    #[test]
    fn opposite_headings_average_to_the_same_line() {
        // North and south are the same way of parking.
        let mean = mean_heading([5.0, 175.0].into_iter());
        assert!(mean < 1.0 || mean > 179.0, "{mean}");
        let square = mean_heading([88.0, 92.0].into_iter());
        assert!((square - 90.0).abs() < 0.5, "{square}");
        assert_eq!(mean_heading(std::iter::empty()), 0.0);
    }

    #[test]
    fn the_biggest_car_park_is_reported_first() {
        let mut cars = car_park(1, 5, 0.0);
        const METRE_LAT: f64 = 1.0 / 111_132.0;
        for mut car in car_park(3, 10, 0.0) {
            car.lat += 200.0 * METRE_LAT;
            cars.push(car);
        }
        let found = lots(&cars, 18.0, 4, 32);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].cars, 30);
        assert_eq!(found[1].cars, 5);
    }
}
