//! The run: walk the region in windows, ask the model, put the answers on the
//! map.
//!
//! Everything here is about the two coordinate changes that decide whether a
//! detection lands in the right place:
//!
//! * **Resolution.** The model was trained at a ground resolution
//!   ([`ModelSpec::ground_sample`]); the imagery comes at whatever a zoom
//!   level happens to give. So a window is cut at the size that *holds* the
//!   model's field of view and scaled to the model's input — a car is then
//!   the number of pixels long the model expects, which is most of the
//!   difference between finding a car park and finding nothing.
//! * **Seams.** Windows overlap, so anything cut in half by one window is
//!   whole in the next, and the same car is then found twice. The duplicates
//!   are settled in metres on the ground ([`merge`]) rather than in pixels,
//!   because that is the one frame both windows agree in.

use crate::model::ModelSpec;
use crate::region::Region;
use crate::sheet::Sheet;

/// What a model says about one thing it found, in the model's input pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Detection {
    pub class: usize,
    pub score: f32,
    pub cx: f32,
    pub cy: f32,
    /// Extent along the box's own x axis.
    pub w: f32,
    /// Extent along the box's own y axis.
    pub h: f32,
    /// Rotation of the box [rad], counter-clockwise in image axes; zero for a
    /// model without an oriented head.
    pub angle: f32,
}

/// A thing found, on the ground.
#[derive(Debug, Clone, PartialEq)]
pub struct GeoDetection {
    /// Index into [`ModelSpec::classes`].
    pub class: usize,
    /// Tag of the objects that may be placed here — [`crate::ClassSpec::place`].
    pub place: String,
    pub score: f32,
    pub lat: f64,
    pub lon: f64,
    /// Long axis [m].
    pub length: f64,
    /// Short axis [m].
    pub width: f64,
    /// Direction of the long axis, degrees clockwise from north, 0 … 180.
    ///
    /// Not a heading in the driving sense — a photograph cannot say which end
    /// of a parked car is the front. Which way round it is placed is the
    /// editor's decision.
    pub heading: f64,
}

/// What a finished walk has to show for itself.
///
/// `blank` is the one number that is not decoration: a window is skipped when
/// the imagery under it could not be had, and a run that found nothing because
/// every window was blank is a different problem from one that found nothing
/// because there was nothing there. Offline mode with a cold cache produces
/// the first, and without this it would look like the second.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub found: Vec<GeoDetection>,
    /// Windows the region asked for.
    pub windows: usize,
    /// Of those, the ones with too little imagery to be worth inferring.
    pub blank: usize,
    /// Tiles asked of the provider.
    pub tiles: usize,
}

/// How far the run has got, for the dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub window: usize,
    pub windows: usize,
    pub found: usize,
    /// Tiles asked of the provider so far.
    pub tiles: usize,
}

/// A model that can be asked about one window.
pub trait Detector {
    /// `pixels` is RGB8, exactly [`crate::InputSpec::width`] by
    /// [`crate::InputSpec::height`].
    fn detect(&mut self, pixels: &[u8], width: u32, height: u32) -> Result<Vec<Detection>, String>;
}

/// Walks `region` and returns everything the model found in it.
///
/// `progress` is called once per window and returns `false` to stop; what has
/// been found up to then is returned rather than thrown away, so a Stop after
/// two minutes still leaves something to commit.
pub fn run(
    sheet: &mut Sheet,
    detector: &mut dyn Detector,
    spec: &ModelSpec,
    region: &Region,
    progress: &mut dyn FnMut(Progress) -> bool,
) -> Result<Outcome, String> {
    let Some((west, south, east, north)) = region.bounds() else {
        return Ok(Outcome {
            found: Vec::new(),
            windows: 0,
            blank: 0,
            tiles: 0,
        });
    };
    let (left, top) = sheet.pixel_of(north, west);
    let (right, bottom) = sheet.pixel_of(south, east);
    let (left, top) = (left.floor() as i64, top.floor() as i64);
    let (right, bottom) = (right.ceil() as i64, bottom.ceil() as i64);

    // The window in sheet pixels: as much ground as the model's input covers
    // at the resolution it was trained for.
    let middle = (south + north) / 2.0;
    let metres = sheet.meters_per_pixel(middle);
    let scale = (spec.ground_sample / metres).max(0.05);
    let window_w = ((spec.input.width as f64 * scale).round() as i64).max(16);
    let window_h = ((spec.input.height as f64 * scale).round() as i64).max(16);
    let step_x = ((window_w as f64 * (1.0 - spec.overlap.clamp(0.0, 0.8))).round() as i64).max(1);
    let step_y = ((window_h as f64 * (1.0 - spec.overlap.clamp(0.0, 0.8))).round() as i64).max(1);

    // Which windows are worth anything. Counted first so the progress bar
    // means something — a corridor skips most of its bounding box, and a bar
    // that counted the box would crawl to 30 % and then finish.
    let mut wanted = Vec::new();
    let mut y = top;
    while y < bottom {
        let mut x = left;
        while x < right {
            let (n, w) = sheet.lat_lon_at(x as f64, y as f64);
            let (s, e) = sheet.lat_lon_at((x + window_w) as f64, (y + window_h) as f64);
            if region.covers_box(w.min(e), s.min(n), w.max(e), s.max(n)) {
                wanted.push((x, y));
            }
            x += step_x;
        }
        y += step_y;
    }

    let mut found: Vec<GeoDetection> = Vec::new();
    let mut blank = 0;
    for (index, (x, y)) in wanted.iter().copied().enumerate() {
        if !progress(Progress {
            window: index,
            windows: wanted.len(),
            found: found.len(),
            tiles: sheet.requested,
        }) {
            break;
        }
        let window = sheet.window(x, y, window_w as u32, window_h as u32);
        // A window that is mostly hole is not worth a second of inference,
        // and what it would find at the edge of the coverage is noise.
        if window.coverage() < 0.5 {
            blank += 1;
            continue;
        }
        let scaled = resize(
            &window.pixels,
            window.width,
            window.height,
            spec.input.width,
            spec.input.height,
        );
        let raw = detector.detect(&scaled, spec.input.width, spec.input.height)?;
        for detection in suppress(raw, spec.head.iou()) {
            if let Some(geo) = place(&detection, spec, sheet, x, y, window_w, window_h) {
                found.push(geo);
            }
        }
    }

    // The region has the last word: the corridor's width, the drawn area, and
    // above all the clearance from the rails.
    found.retain(|d| region.contains(d.lat, d.lon));
    Ok(Outcome {
        found: merge(found),
        windows: wanted.len(),
        blank,
        tiles: sheet.requested,
    })
}

/// One detection from the model's own pixels onto the map.
fn place(
    detection: &Detection,
    spec: &ModelSpec,
    sheet: &Sheet,
    left: i64,
    top: i64,
    window_w: i64,
    window_h: i64,
) -> Option<GeoDetection> {
    let class = spec.classes.get(detection.class)?;
    if class.place.is_empty() || detection.score < spec.confidence_of(detection.class) {
        return None;
    }
    // Model input → window pixels → sheet pixels.
    //
    // No half-pixel anywhere, and that is worth stating because it looks like
    // an oversight. The model gives a box centre in the input image's own
    // continuous coordinates, spanning nought to its width — the same space
    // the tile grid is addressed in, where a whole number is the edge between
    // two pixels rather than the middle of one. Between two such spaces the
    // mapping is a bare scale. (`resize` does carry halves, because it works
    // in pixel *indices*, where a whole number is the middle of a pixel; the
    // two conventions agree once both are put in the same one, and the test
    // below is what says so.)
    let fx = window_w as f64 / spec.input.width.max(1) as f64;
    let fy = window_h as f64 / spec.input.height.max(1) as f64;
    let px = left as f64 + detection.cx as f64 * fx;
    let py = top as f64 + detection.cy as f64 * fy;
    let (lat, lon) = sheet.lat_lon_at(px, py);
    let metres = sheet.meters_per_pixel(lat);
    let w = detection.w as f64 * fx * metres;
    let h = detection.h as f64 * fy * metres;
    let (length, width) = (w.max(h), w.min(h));
    if !class.plausible(length) {
        return None;
    }
    Some(GeoDetection {
        class: detection.class,
        place: class.place.clone(),
        score: detection.score,
        lat,
        lon,
        length,
        width,
        heading: heading_of(detection),
    })
}

/// Direction of the box's long axis, degrees clockwise from north.
///
/// The image's x runs east and its y runs *south*, so an angle that turns
/// counter-clockwise in image axes turns clockwise on the ground — which is
/// the same direction a compass counts in, and the reason there is no sign
/// flip here beyond the one on the northing.
fn heading_of(detection: &Detection) -> f64 {
    // The long axis is x when the box is wider than tall, else y — a quarter
    // turn on.
    let along = if detection.w >= detection.h {
        detection.angle as f64
    } else {
        detection.angle as f64 + std::f64::consts::FRAC_PI_2
    };
    let (east, north) = (along.cos(), -along.sin());
    let degrees = east.atan2(north).to_degrees();
    degrees.rem_euclid(180.0)
}

/// Non-maximum suppression inside one window.
///
/// On the enclosing axis-aligned boxes, not on the rotated ones. Two cars in
/// neighbouring bays overlap little enough that the approximation never
/// merges them, and the rotated intersection is a polygon clip for a decision
/// that is a threshold anyway.
fn suppress(mut detections: Vec<Detection>, threshold: f32) -> Vec<Detection> {
    detections.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut kept: Vec<Detection> = Vec::new();
    for candidate in detections {
        let overlaps = kept
            .iter()
            .any(|k| k.class == candidate.class && iou(k, &candidate) > threshold);
        if !overlaps {
            kept.push(candidate);
        }
    }
    kept
}

/// Intersection over union of the enclosing axis-aligned boxes.
fn iou(a: &Detection, b: &Detection) -> f32 {
    let extent = |d: &Detection| {
        let (c, s) = (d.angle.cos().abs(), d.angle.sin().abs());
        ((d.w * c + d.h * s) / 2.0, (d.w * s + d.h * c) / 2.0)
    };
    let (aw, ah) = extent(a);
    let (bw, bh) = extent(b);
    let overlap_x = (a.cx + aw).min(b.cx + bw) - (a.cx - aw).max(b.cx - bw);
    let overlap_y = (a.cy + ah).min(b.cy + bh) - (a.cy - ah).max(b.cy - bh);
    if overlap_x <= 0.0 || overlap_y <= 0.0 {
        return 0.0;
    }
    let intersection = overlap_x * overlap_y;
    let union = 4.0 * aw * ah + 4.0 * bw * bh - intersection;
    if union <= 0.0 {
        0.0
    } else {
        intersection / union
    }
}

/// Settles the duplicates two overlapping windows produced, in metres.
///
/// Distance rather than overlap: what the seam produces is the *same* car
/// twice, a metre or two apart, and two real cars in neighbouring bays are
/// never that close. The threshold is measured against the shorter of the
/// two, so a lorry does not swallow the car parked beside it.
pub fn merge(mut found: Vec<GeoDetection>) -> Vec<GeoDetection> {
    found.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut kept: Vec<GeoDetection> = Vec::new();
    for candidate in found {
        let duplicate = kept.iter().any(|k| {
            let limit = k.length.min(candidate.length) * 0.6;
            distance(k.lat, k.lon, candidate.lat, candidate.lon) < limit.max(1.0)
        });
        if !duplicate {
            kept.push(candidate);
        }
    }
    kept
}

/// Metres between two places, on the sphere — good to a fraction of a percent
/// at the distances a duplicate can be apart.
pub fn distance(lat_a: f64, lon_a: f64, lat_b: f64, lon_b: f64) -> f64 {
    const METRES_PER_DEGREE: f64 = 111_132.0;
    let mid = ((lat_a + lat_b) / 2.0).to_radians();
    let dn = (lat_b - lat_a) * METRES_PER_DEGREE;
    let de = (lon_b - lon_a) * METRES_PER_DEGREE * mid.cos();
    (dn * dn + de * de).sqrt()
}

/// Bilinear resampling of an RGB8 buffer.
///
/// Bilinear and not a box filter: the scale is between one and two, where the
/// two are within a hair of each other, and a car eighteen pixels long has to
/// keep its edges — that is what the model is looking at.
fn resize(pixels: &[u8], from_w: u32, from_h: u32, to_w: u32, to_h: u32) -> Vec<u8> {
    if from_w == to_w && from_h == to_h {
        return pixels.to_vec();
    }
    let mut out = vec![0u8; (to_w as usize) * (to_h as usize) * 3];
    let sx = from_w as f64 / to_w as f64;
    let sy = from_h as f64 / to_h as f64;
    for y in 0..to_h {
        let fy = ((y as f64 + 0.5) * sy - 0.5).max(0.0);
        let y0 = fy.floor() as u32;
        let y1 = (y0 + 1).min(from_h - 1);
        let ty = fy - y0 as f64;
        for x in 0..to_w {
            let fx = ((x as f64 + 0.5) * sx - 0.5).max(0.0);
            let x0 = fx.floor() as u32;
            let x1 = (x0 + 1).min(from_w - 1);
            let tx = fx - x0 as f64;
            for c in 0..3 {
                let at = |px: u32, py: u32| {
                    pixels
                        .get(((py * from_w + px) * 3 + c) as usize)
                        .copied()
                        .unwrap_or(0) as f64
                };
                let top = at(x0, y0) * (1.0 - tx) + at(x1, y0) * tx;
                let bottom = at(x0, y1) * (1.0 - tx) + at(x1, y1) * tx;
                let value = top * (1.0 - ty) + bottom * ty;
                out[((y * to_w + x) * 3 + c) as usize] = value.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ClassSpec, InputSpec, VisionConfig};
    use crate::region::Shape;
    use imagery::DecodedTile;

    fn detection(cx: f32, cy: f32, w: f32, h: f32, angle: f32) -> Detection {
        Detection {
            class: 10,
            score: 0.9,
            cx,
            cy,
            w,
            h,
            angle,
        }
    }

    #[test]
    fn a_box_lying_east_west_reads_as_ninety_degrees() {
        let east_west = heading_of(&detection(0.0, 0.0, 20.0, 8.0, 0.0));
        assert!((east_west - 90.0).abs() < 1e-6, "{east_west}");
        let north_south = heading_of(&detection(0.0, 0.0, 8.0, 20.0, 0.0));
        assert!(north_south.abs() < 1e-6, "{north_south}");
    }

    #[test]
    fn a_box_turned_a_quarter_turn_reads_the_same_as_its_transpose() {
        let a = heading_of(&detection(0.0, 0.0, 20.0, 8.0, std::f32::consts::FRAC_PI_2));
        let b = heading_of(&detection(0.0, 0.0, 8.0, 20.0, 0.0));
        assert!((a - b).abs() < 1e-4, "{a} vs {b}");
    }

    #[test]
    fn a_box_turned_on_screen_reads_as_the_compass_would() {
        // The image's y runs south, so an eighth of a turn from the x axis
        // points south-east — 135° on the compass.
        let heading = heading_of(&detection(0.0, 0.0, 20.0, 8.0, std::f32::consts::FRAC_PI_4));
        assert!((heading - 135.0).abs() < 1e-4, "{heading}");
        // And an eighth the other way points north-east.
        let back = heading_of(&detection(
            0.0,
            0.0,
            20.0,
            8.0,
            -std::f32::consts::FRAC_PI_4,
        ));
        assert!((back - 45.0).abs() < 1e-4, "{back}");
    }

    #[test]
    fn the_same_box_twice_is_suppressed_and_the_neighbour_is_not() {
        let kept = suppress(
            vec![
                detection(100.0, 100.0, 20.0, 10.0, 0.0),
                Detection {
                    score: 0.5,
                    ..detection(101.0, 100.0, 20.0, 10.0, 0.0)
                },
                // Two bays over.
                Detection {
                    score: 0.8,
                    ..detection(100.0, 130.0, 20.0, 10.0, 0.0)
                },
            ],
            0.45,
        );
        assert_eq!(kept.len(), 2);
        assert!((kept[0].score - 0.9).abs() < 1e-6, "the best one is kept");
    }

    #[test]
    fn the_same_car_found_in_two_windows_is_one_car() {
        let car = |lat: f64, lon: f64, score: f32| GeoDetection {
            class: 10,
            place: "car".into(),
            score,
            lat,
            lon,
            length: 4.4,
            width: 1.8,
            heading: 90.0,
        };
        // A metre apart: the seam. Ten metres apart: two cars.
        let merged = merge(vec![
            car(51.0, 7.0, 0.9),
            car(51.000009, 7.0, 0.7),
            car(51.0001, 7.0, 0.8),
        ]);
        assert_eq!(merged.len(), 2);
        assert!((merged[0].score - 0.9).abs() < 1e-6);
    }

    #[test]
    fn resizing_keeps_a_flat_picture_flat() {
        let pixels = vec![37u8; 64 * 64 * 3];
        let out = resize(&pixels, 64, 64, 32, 32);
        assert_eq!(out.len(), 32 * 32 * 3);
        assert!(out.iter().all(|&b| b == 37), "no ringing on a flat field");
    }

    #[test]
    fn resizing_to_the_same_size_is_a_copy() {
        let pixels: Vec<u8> = (0..(8 * 8 * 3)).map(|i| (i % 251) as u8).collect();
        assert_eq!(resize(&pixels, 8, 8, 8, 8), pixels);
    }

    /// A detector that reports a car park spread over every window it is
    /// shown — enough to prove the walk, the mapping and the clearance. A grid
    /// and not a single car in the middle: the clearance is the thing under
    /// test, and it can only be tested by finds that land at different
    /// distances from the track.
    struct CarPark {
        windows: usize,
        per_side: u32,
    }

    impl Detector for CarPark {
        fn detect(
            &mut self,
            _pixels: &[u8],
            width: u32,
            height: u32,
        ) -> Result<Vec<Detection>, String> {
            self.windows += 1;
            let mut cars = Vec::new();
            for row in 0..self.per_side {
                for column in 0..self.per_side {
                    cars.push(Detection {
                        class: 0,
                        score: 0.9,
                        cx: (column as f32 + 0.5) * width as f32 / self.per_side as f32,
                        cy: (row as f32 + 0.5) * height as f32 / self.per_side as f32,
                        // 4.4 m at 0.3 m/px is about fifteen pixels.
                        w: 15.0,
                        h: 6.0,
                        angle: 0.0,
                    });
                }
            }
            Ok(cars)
        }
    }

    fn grey_sheet() -> Sheet {
        Sheet::new(19, 256, 64, |id| {
            Some(DecodedTile {
                tile: id,
                width: 256,
                height: 256,
                pixels: vec![128; 256 * 256 * 4],
            })
        })
    }

    fn car_spec() -> ModelSpec {
        ModelSpec {
            classes: vec![ClassSpec::placed("car", "car", (4.4, 1.8))],
            input: InputSpec {
                width: 256,
                height: 256,
                ..Default::default()
            },
            ..VisionConfig::default().models[0].clone()
        }
    }

    /// A find in the middle of what the model was shown belongs in the middle
    /// of the window it was shown, on the map.
    ///
    /// The one thing that pins the pixel convention. Adding the half-pixel
    /// that `resize` carries looks right and is not — the model's box centre
    /// and the tile grid are already in the same continuous space — and it
    /// puts every find a fifth of a metre out. Small enough to be argued
    /// about, which is why it is measured here instead.
    #[test]
    fn a_find_in_the_middle_of_the_window_lands_in_the_middle_of_it() {
        let sheet = grey_sheet();
        let spec = car_spec();
        let (window_w, window_h) = (600_i64, 600_i64);
        // Somewhere in Germany, so the metres a pixel is worth are the ones
        // this actually runs at.
        let (cx, cy) = sheet.pixel_of(51.0, 7.0);
        let (left, top) = (cx as i64 - window_w / 2, cy as i64 - window_h / 2);
        let middle = spec.input.width as f32 / 2.0;
        // Four and a half metres of car, in the model's own pixels.
        let metres = sheet.meters_per_pixel(51.0) * window_w as f64 / spec.input.width as f64;
        let length = (4.5 / metres) as f32;
        let mut car = detection(middle, middle, length, length / 2.4, 0.0);
        car.class = 0;
        let found = place(&car, &spec, &sheet, left, top, window_w, window_h)
            .expect("a car in the middle of the window");
        let (want_lat, want_lon) = sheet.lat_lon_at(
            left as f64 + window_w as f64 / 2.0,
            top as f64 + window_h as f64 / 2.0,
        );
        let off = distance(found.lat, found.lon, want_lat, want_lon);
        assert!(off < 0.01, "{off:.3} m off the middle of its own window");
    }

    #[test]
    fn the_walk_stays_inside_the_corridor_and_off_the_track() {
        let track = vec![vec![(51.0, 7.0), (51.0, 7.004)]];
        let region = Region::new(Shape::Corridor { radius: 60.0 }, &track, 8.0, 32);
        let mut sheet = grey_sheet();
        let mut detector = CarPark {
            windows: 0,
            per_side: 5,
        };
        let mut seen = Vec::new();
        let outcome = run(&mut sheet, &mut detector, &car_spec(), &region, &mut |p| {
            seen.push(p);
            true
        })
        .unwrap();

        assert!(detector.windows > 0, "the corridor was walked");
        assert_eq!(seen.len(), detector.windows);
        assert!(seen.iter().all(|p| p.windows == seen.len()));
        assert_eq!(outcome.windows, detector.windows);
        assert_eq!(outcome.blank, 0, "the imagery was there for every window");
        for car in &outcome.found {
            assert!(
                region.track_distance(car.lat, car.lon) >= 8.0,
                "a car came within the clearance"
            );
            assert!(region.contains(car.lat, car.lon));
            assert_eq!(car.place, "car");
            assert!((car.length - 4.4).abs() < 1.5, "{}", car.length);
        }
    }

    #[test]
    fn stopping_keeps_what_was_found_so_far() {
        let track = vec![vec![(51.0, 7.0), (51.0, 7.02)]];
        let region = Region::new(Shape::Corridor { radius: 60.0 }, &track, 8.0, 32);
        let mut sheet = grey_sheet();
        let mut detector = CarPark {
            windows: 0,
            per_side: 5,
        };
        let found = run(&mut sheet, &mut detector, &car_spec(), &region, &mut |p| {
            p.window < 2
        })
        .unwrap();
        assert_eq!(detector.windows, 2, "stopped at the third window");
        assert!(
            !found.found.is_empty(),
            "what was found is kept, not thrown away"
        );
    }

    #[test]
    fn a_run_without_imagery_says_the_windows_were_blank() {
        // Offline with a cold cache: the walk happens, no window carries a
        // picture, and nothing is inferred. "Nothing found" would be a lie.
        let track = vec![vec![(51.0, 7.0), (51.0, 7.004)]];
        let region = Region::new(Shape::Corridor { radius: 60.0 }, &track, 8.0, 32);
        let mut sheet = Sheet::new(19, 256, 64, |_| None);
        let mut detector = CarPark {
            windows: 0,
            per_side: 5,
        };
        let outcome = run(&mut sheet, &mut detector, &car_spec(), &region, &mut |_| {
            true
        })
        .unwrap();
        assert!(outcome.windows > 0, "the corridor was walked");
        assert_eq!(outcome.blank, outcome.windows, "and every window was empty");
        assert!(outcome.found.is_empty());
        assert_eq!(detector.windows, 0, "the model was never asked");
    }

    #[test]
    fn a_region_with_nothing_in_it_runs_no_windows() {
        let region = Region::new(Shape::Corridor { radius: 60.0 }, &[], 8.0, 32);
        let mut sheet = grey_sheet();
        let mut detector = CarPark {
            windows: 0,
            per_side: 5,
        };
        let found = run(&mut sheet, &mut detector, &car_spec(), &region, &mut |_| {
            true
        })
        .unwrap();
        assert!(found.found.is_empty());
        assert_eq!(found.windows, 0);
        assert_eq!(detector.windows, 0);
        assert_eq!(sheet.requested, 0, "and fetches no imagery either");
    }
}
