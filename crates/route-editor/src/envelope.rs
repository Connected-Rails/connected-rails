//! The module envelope: the closed polygon that says how far the module
//! reaches, and the tool that reshapes it.
//!
//! A module is not a rectangle. A line follows a valley, a boundary is agreed
//! with the neighbour at the signal it makes sense at, and the ground between
//! two modules has to belong to exactly one of them — Zusi builds modules the
//! same way, around a *Hüllkurve* that terrain and scenery may not cross. The
//! polygon lives in [`content::route::LineSource::envelope`]; everything here
//! is how it is picked, dragged and drawn.
//!
//! The envelope is created with the module ([`crate::new_module`]) as a square
//! around the anchor, so it is never absent by accident — a module that came
//! from before envelopes bounds nothing until the panel gives it one.
//!
//! The geometry of picking a vertex, hitting a side and putting a vertex on it
//! is written once, over any polyline of latitude/longitude pairs
//! ([`LatLon`]): the walkways ([`crate::walkways`]) are reshaped with the same
//! gestures and share it.

use crate::tools::{EditorState, PICK_PIXELS, ScreenPick, Selection, Tool};
use crate::{Focus, Line};
use bevy::prelude::*;
use content::route::{EnvelopePoint, WalkPoint};
use glam::DVec3;
use world_coords::{EcefPos, RenderOrigin, geo};

/// Colour of the boundary — the warn yellow the module boundaries already use,
/// so the two read as one family.
const COLOR: Color = Color::srgb(0.89, 0.71, 0.30);
/// The same, dimmed: what the boundary looks like while another tool is active.
const COLOR_IDLE: Color = Color::srgba(0.89, 0.71, 0.30, 0.35);

/// The height the envelope is drawn at [m ellipsoidal].
///
/// Fixed to the module's anchor, and so to one height for the whole polygon:
/// the boundary is a closed line that has to keep its shape, and a corner that
/// took its height from the terrain under it would drag the line into every
/// hollow it crosses. The point marks — trees, reference markers, terrain
/// strokes — do the opposite and stand on the ground each of them is on
/// (`terrain::Marks`). A module without an anchor has nothing to tie to and
/// falls back to the height of the view point.
pub fn height(line: &Line, focus: &Focus) -> f64 {
    match line.source.anchor {
        Some(anchor) => geo::ellipsoidal_height(anchor.height, line.source.geoid_offset),
        None => geo::from_ecef(focus.position).2,
    }
}

/// A vertex that is a latitude/longitude pair. The envelope's corners and the
/// walkways' vertices both are exactly that, and picking, inserting and
/// dragging one is the same job for both — so the helpers below take either.
pub trait LatLon: Copy {
    fn lat(&self) -> f64;
    fn lon(&self) -> f64;
    fn at(lat: f64, lon: f64) -> Self;
}

impl LatLon for EnvelopePoint {
    fn lat(&self) -> f64 {
        self.lat
    }

    fn lon(&self) -> f64 {
        self.lon
    }

    fn at(lat: f64, lon: f64) -> Self {
        Self { lat, lon }
    }
}

impl LatLon for WalkPoint {
    fn lat(&self) -> f64 {
        self.lat
    }

    fn lon(&self) -> f64 {
        self.lon
    }

    fn at(lat: f64, lon: f64) -> Self {
        Self { lat, lon }
    }
}

/// The vertex nearest `click` within `radius`, and how near: `(vertex,
/// distance)`. Measured in whatever space the positions come in — the envelope
/// and the walkways hand in screen pixels (see [`ScreenPick`]), so a corner at
/// the horizon is as grabbable as one under the camera.
pub fn nearest_vertex(
    positions: impl IntoIterator<Item = (usize, DVec3)>,
    click: DVec3,
    radius: f64,
) -> Option<(usize, f64)> {
    positions
        .into_iter()
        .map(|(i, p)| (i, p.distance(click)))
        .filter(|(_, distance)| *distance <= radius)
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// The sides of a polyline as `(side, from, to)`: side `i` runs from vertex
/// `i` to `i + 1`, and a `closed` ring has one more, from the last vertex back
/// to the first. A single vertex has no side either way.
pub fn sides<T: Copy>(points: &[T], closed: bool) -> impl Iterator<Item = (usize, T, T)> + '_ {
    let count = match (closed, points.len()) {
        (_, 0 | 1) => 0,
        (true, n) => n,
        (false, n) => n - 1,
    };
    (0..count).map(move |i| (i, points[i], points[(i + 1) % points.len()]))
}

/// The side nearest `click` within `radius`, where on it the nearest point
/// lies (0 … 1), and how near: `(side, t, distance)`. Same space rule as
/// [`nearest_vertex`]: the envelope measures in metres on its own plane, the
/// walkways in pixels on the screen.
pub fn nearest_side(
    sides: impl IntoIterator<Item = (usize, DVec3, DVec3)>,
    click: DVec3,
    radius: f64,
) -> Option<(usize, f64, f64)> {
    sides
        .into_iter()
        .map(|(i, a, b)| {
            let along = b - a;
            let length2 = along.length_squared().max(1e-9);
            let t = ((click - a).dot(along) / length2).clamp(0.0, 1.0);
            (i, t, (a + along * t).distance(click))
        })
        .filter(|(_, _, distance)| *distance <= radius)
        .min_by(|a, b| a.2.total_cmp(&b.2))
}

/// The point `t` of the way along side `side` of `points` — where a vertex
/// added on that side goes, which is the only place it can go without folding
/// the polyline over itself. Interpolated in degrees: over the few hundred
/// metres of a side, the difference to a line on the ellipsoid is far below
/// the width of the drawn one.
pub fn point_on_side<P: LatLon>(points: &[P], side: usize, t: f64) -> P {
    let a = points[side];
    let b = points[(side + 1) % points.len()];
    P::at(
        a.lat() + (b.lat() - a.lat()) * t,
        a.lon() + (b.lon() - a.lon()) * t,
    )
}

/// Puts `point` where the cursor is, on the map. The height is dropped — a
/// vertex is a place, and the ground (or the envelope's own plane) answers
/// for its height.
pub fn move_to<P: LatLon>(point: &mut P, p: EcefPos) {
    let (lat, lon, _) = geo::from_ecef(p);
    *point = P::at(lat.to_degrees(), lon.to_degrees());
}

/// Map position of a corner at the envelope's own [`height`].
pub fn point_pos(point: &EnvelopePoint, height: f64) -> EcefPos {
    geo::to_ecef_deg(point.lat, point.lon, height)
}

/// The corner under the cursor, if one is within grabbing distance.
pub fn pick_point(line: &Line, pick: &ScreenPick, focus: &Focus) -> Option<usize> {
    let height = height(line, focus);
    let on_screen = line
        .source
        .envelope
        .iter()
        .enumerate()
        .filter_map(|(i, p)| Some((i, pick.screen(point_pos(p, height))?)));
    nearest_vertex(on_screen, pick.cursor(), PICK_PIXELS as f64).map(|(i, _)| i)
}

/// Which side of the polygon the click landed on, and where on it.
///
/// A click on a side is how a corner is added — the new one goes between the
/// two the side runs from, which is the only place it can go without crossing
/// the polygon over itself.
pub fn pick_side(line: &Line, p: EcefPos, focus: &Focus, radius: f64) -> Option<(usize, f64)> {
    let corners = &line.source.envelope;
    if corners.len() < 2 {
        return None;
    }
    let height = height(line, focus);
    let positions: Vec<DVec3> = corners.iter().map(|c| point_pos(c, height).0).collect();
    nearest_side(sides(&positions, true), p.0, radius).map(|(side, t, _)| (side, t))
}

/// Puts a corner on side `side` at the fraction `t` along it.
pub fn insert_point(line: &mut Line, side: usize, t: f64) -> usize {
    let point = point_on_side(&line.source.envelope, side, t);
    let index = side + 1;
    line.source.envelope.insert(index, point);
    index
}

/// Moves corner `index` to the position under the cursor.
pub fn drag_point(line: &mut Line, index: usize, p: EcefPos) {
    if let Some(corner) = line.source.envelope.get_mut(index) {
        move_to(corner, p);
    }
}

/// Removes corner `index` — never below the three a polygon needs.
///
/// Returns whether it went; the caller says so in the status bar, because a
/// Delete that silently does nothing reads as a broken key.
pub fn remove_point(line: &mut Line, index: usize) -> bool {
    if line.source.envelope.len() <= 3 || index >= line.source.envelope.len() {
        return false;
    }
    line.source.envelope.remove(index);
    true
}

/// Draws the boundary: bright with its corner handles while the envelope tool
/// is up, a dim outline otherwise. Every other tool needs to see where the
/// module ends, but not to be invited to drag it.
pub fn draw(
    gizmos: &mut Gizmos,
    line: &Line,
    origin: &RenderOrigin,
    focus: &Focus,
    state: &EditorState,
) {
    let corners = &line.source.envelope;
    if corners.len() < 3 {
        return;
    }
    let active = state.tool == Tool::EditEnvelope;
    let color = if active { COLOR } else { COLOR_IDLE };
    // A metre off the ground, like every other gizmo on this map — the aerial
    // imagery is draped just above the terrain, and a line at ground level
    // disappears underneath it.
    let height = height(line, focus);
    let world: Vec<EcefPos> = corners.iter().map(|c| point_pos(c, height)).collect();
    let positions: Vec<Vec3> = world
        .iter()
        .map(|p| origin.to_render(*p) + origin.dir_to_render(world_coords::EnuFrame::at(*p).up))
        .collect();
    for i in 0..positions.len() {
        gizmos.line(positions[i], positions[(i + 1) % positions.len()], color);
    }
    if !active {
        return;
    }
    // Handles scale with the view, like every other grab point on this map.
    let radius = (focus.height * 0.006).max(2.0) as f32;
    for (i, p) in world.iter().enumerate() {
        let selected = state.selection == Selection::EnvelopePoint(i);
        let color = if selected {
            Color::srgb(0.36, 0.61, 0.96)
        } else {
            COLOR
        };
        crate::tools::ground_circle(
            gizmos,
            origin,
            *p,
            if selected { radius * 1.4 } else { radius },
            color,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use content::route::{DEFAULT_ENVELOPE_HALF_SIZE, GeoPoint, default_envelope};

    fn square() -> Vec<EnvelopePoint> {
        default_envelope(
            GeoPoint {
                lat: 52.0,
                lon: 10.0,
                height: 0.0,
            },
            DEFAULT_ENVELOPE_HALF_SIZE,
        )
    }

    #[test]
    fn a_corner_goes_in_between_its_side() {
        let mut line = Line {
            source: content::LineSource {
                envelope: square(),
                ..Default::default()
            },
            net: Default::default(),
            path: None,
            dirty: false,
            needs_rebuild: false,
            terrain_change: Default::default(),
            recenter: false,
            issues: Vec::new(),
        };
        let (a, b) = (line.source.envelope[0], line.source.envelope[1]);
        let index = insert_point(&mut line, 0, 0.5);
        assert_eq!(index, 1);
        assert_eq!(line.source.envelope.len(), 5);
        let new = line.source.envelope[1];
        assert!((new.lat - (a.lat + b.lat) / 2.0).abs() < 1e-12);
        assert!((new.lon - (a.lon + b.lon) / 2.0).abs() < 1e-12);
        // The neighbours it was put between are still its neighbours.
        assert_eq!(line.source.envelope[0], a);
        assert_eq!(line.source.envelope[2], b);
    }

    #[test]
    fn a_polygon_never_falls_below_three_corners() {
        let mut line = Line {
            source: content::LineSource {
                envelope: square(),
                ..Default::default()
            },
            net: Default::default(),
            path: None,
            dirty: false,
            needs_rebuild: false,
            terrain_change: Default::default(),
            recenter: false,
            issues: Vec::new(),
        };
        assert!(remove_point(&mut line, 0));
        assert_eq!(line.source.envelope.len(), 3);
        assert!(!remove_point(&mut line, 0));
        assert_eq!(line.source.envelope.len(), 3);
    }

    /// The closing side exists only for a ring — a footpath has no side from
    /// its end back to its start, a walk area and the envelope do.
    #[test]
    fn sides_wrap_only_for_a_ring() {
        let points = [DVec3::ZERO, DVec3::X, DVec3::Y];
        assert_eq!(sides(&points, false).count(), 2);
        let ring: Vec<_> = sides(&points, true).collect();
        assert_eq!(ring.len(), 3);
        assert_eq!(ring[2], (2, DVec3::Y, DVec3::ZERO));
        // One vertex is no side, closed or not.
        assert_eq!(sides(&points[..1], true).count(), 0);
        assert_eq!(sides(&points[..1], false).count(), 0);
    }

    #[test]
    fn the_nearest_side_says_where_it_was_hit() {
        let points = [
            DVec3::ZERO,
            DVec3::new(10.0, 0.0, 0.0),
            DVec3::new(10.0, 10.0, 0.0),
        ];
        // A metre below the first side, a quarter of the way along it.
        let (side, t, distance) =
            nearest_side(sides(&points, false), DVec3::new(2.5, -1.0, 0.0), 2.0).unwrap();
        assert_eq!(side, 0);
        assert!((t - 0.25).abs() < 1e-9);
        assert!((distance - 1.0).abs() < 1e-9);
        // Out of reach: nothing.
        assert!(nearest_side(sides(&points, false), DVec3::new(2.5, -5.0, 0.0), 2.0).is_none());
        // On the diagonal back to the start — a side only the ring has.
        let on_diagonal = DVec3::new(5.0, 5.0, 0.0);
        assert!(nearest_side(sides(&points, false), on_diagonal, 1.0).is_none());
        assert_eq!(
            nearest_side(sides(&points, true), on_diagonal, 1.0).map(|hit| hit.0),
            Some(2)
        );
        // The vertex pick answers with the distance, so callers can compare
        // hits across several polylines.
        assert_eq!(
            nearest_vertex(
                points.iter().copied().enumerate(),
                DVec3::new(9.5, 0.0, 0.0),
                1.0
            ),
            Some((1, 0.5))
        );
    }

    /// The interpolation works on either kind of vertex.
    #[test]
    fn a_point_on_a_side_is_interpolated_for_any_vertex_kind() {
        let path = [
            WalkPoint {
                lat: 52.0,
                lon: 10.0,
            },
            WalkPoint {
                lat: 52.0,
                lon: 10.002,
            },
        ];
        let mid = point_on_side(&path, 0, 0.5);
        assert!((mid.lat - 52.0).abs() < 1e-12);
        assert!((mid.lon - 10.001).abs() < 1e-12);
        // Side 1 of a two-vertex ring runs back to the first vertex.
        let back = point_on_side(&path, 1, 0.25);
        assert!((back.lon - 10.0015).abs() < 1e-12);
    }
}
