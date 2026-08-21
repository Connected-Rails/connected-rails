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

use crate::tools::{EditorState, ScreenPick, Selection, Tool};
use crate::{Focus, Line};
use bevy::prelude::*;
use content::route::EnvelopePoint;
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

/// Map position of a corner at the envelope's own [`height`].
pub fn point_pos(point: &EnvelopePoint, height: f64) -> EcefPos {
    geo::to_ecef_deg(point.lat, point.lon, height)
}

/// The corner under the cursor, if one is within grabbing distance.
pub fn pick_point(line: &Line, pick: &ScreenPick, focus: &Focus) -> Option<usize> {
    let height = height(line, focus);
    line.source
        .envelope
        .iter()
        .enumerate()
        .filter_map(|(i, p)| pick.hits(point_pos(p, height)).map(|d| (i, d)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
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
    let click = p.0;
    (0..positions.len())
        .map(|i| {
            let a = positions[i];
            let b = positions[(i + 1) % positions.len()];
            let along = b - a;
            let length2 = along.length_squared().max(1e-9);
            let t = ((click - a).dot(along) / length2).clamp(0.0, 1.0);
            (i, (a + along * t).distance(click), t)
        })
        .filter(|(_, distance, _)| *distance <= radius)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _, t)| (i, t))
}

/// Puts a corner on side `side` at the fraction `t` along it.
pub fn insert_point(line: &mut Line, side: usize, t: f64) -> usize {
    let corners = &line.source.envelope;
    let a = corners[side];
    let b = corners[(side + 1) % corners.len()];
    let point = EnvelopePoint {
        lat: a.lat + (b.lat - a.lat) * t,
        lon: a.lon + (b.lon - a.lon) * t,
    };
    let index = side + 1;
    line.source.envelope.insert(index, point);
    index
}

/// Moves corner `index` to the position under the cursor.
pub fn drag_point(line: &mut Line, index: usize, p: EcefPos) {
    let Some(corner) = line.source.envelope.get_mut(index) else {
        return;
    };
    let (lat, lon, _) = geo::from_ecef(p);
    corner.lat = lat.to_degrees();
    corner.lon = lon.to_degrees();
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
}
