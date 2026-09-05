//! Transform gizmo of the selection — the handles an Unreal viewport puts on a
//! selected actor, mapped onto the fields the items here actually have.
//!
//! A signal or a catenary mast does not sit at a world coordinate: it sits `s`
//! metres along an edge, `lateral_offset` metres beside it and `height` metres
//! above the railhead. Dragging the red arrow therefore slides it *along the
//! track*, not along world X — which is what someone placing equipment means,
//! and what keeps the saved file readable. Trees, markers and terrain strokes
//! are free of the track and get the east/north pair instead.
//!
//! ponytail: no scale handle, because nothing in the file format has a scale;
//! and rotation is the object's `yaw_deg` alone — a mast leaning out of the
//! vertical is not something the run can show.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use glam::DVec3;
use world_coords::{EcefPos, EnuFrame, RenderOrigin, geo};

use crate::tools::{self, EditorState, Selection};
use crate::{Focus, Line, Origin};

/// How near the cursor a handle has to be to grab it [logical pixels].
const GRAB_PIXELS: f32 = 12.0;
/// Handle length as a share of the distance to the camera — keeps the gizmo
/// roughly the same size on screen at any range.
const HANDLE_SHARE: f64 = 0.16;
/// Shortest handle [m], so the gizmo does not vanish inside a close-up.
const MIN_HANDLE: f64 = 1.5;

/// What a handle edits. Not world X/Y/Z: the axes of the thing that is
/// selected.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    /// Arc length along the edge.
    Along,
    /// Offset across the track, positive to the right.
    Across,
    /// Height above the railhead.
    Up,
    /// Free ground movement of an item that is not bound to the track.
    East,
    North,
    /// Rotation about the up axis.
    Yaw,
}

/// Which handles the gizmo shows.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum GizmoMode {
    #[default]
    Translate,
    Rotate,
}

/// An axis being dragged. The handle line is frozen at the press: one that
/// moved with the item it drags would feed back into itself and run away.
struct Drag {
    axis: Axis,
    at: EcefPos,
    dir: DVec3,
    /// Where the cursor sat on the handle last frame — metres along the axis,
    /// or radians around the ring.
    last: f64,
}

#[derive(Resource, Default)]
pub struct GizmoState {
    pub mode: GizmoMode,
    drag: Option<Drag>,
    /// Handle under the cursor — drawn in the active colour, as a hint that it
    /// can be grabbed.
    hovered: Option<Axis>,
}

impl GizmoState {
    /// Whether the gizmo owns the mouse right now — the select tool must not
    /// reselect underneath a handle drag.
    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }
}

/// Origin and handles of the current selection, in world coordinates.
fn handles(
    line: &Line,
    selection: Selection,
    focus: &Focus,
    marks: &crate::terrain::Marks,
) -> Option<(EcefPos, Vec<(Axis, DVec3)>)> {
    let at = tools::selection_pos(line, selection, focus, marks)?;
    // On the track: the pose at the item's arc length gives the three axes.
    let on_track = |edge: u32, s: f64| {
        let edge = line.net.edges().get(edge as usize)?;
        let pose = edge.eval(s.clamp(0.0, edge.length()));
        let tangent = pose.tangent.normalize_or_zero();
        let up = pose.up.normalize_or_zero();
        Some((tangent, tangent.cross(up).normalize_or_zero(), up))
    };
    match selection {
        Selection::Device(i) => {
            let device = line.source.devices.get(i)?;
            let (along, across, _) = on_track(device.edge, device.s)?;
            Some((at, vec![(Axis::Along, along), (Axis::Across, across)]))
        }
        Selection::Object(i) => {
            let object = line.source.objects.get(i)?;
            let (along, across, up) = on_track(object.edge, object.s)?;
            Some((
                at,
                vec![
                    (Axis::Along, along),
                    (Axis::Across, across),
                    (Axis::Up, up),
                    (Axis::Yaw, up),
                ],
            ))
        }
        Selection::Building(i) => {
            let building = line.source.buildings.get(i)?;
            let (along, across, up) = on_track(building.edge, building.s)?;
            Some((
                at,
                vec![
                    (Axis::Along, along),
                    (Axis::Across, across),
                    (Axis::Up, up),
                    (Axis::Yaw, up),
                ],
            ))
        }
        // Free of the track — a tree stands where it stands.
        Selection::Tree(_) | Selection::Marker(_) | Selection::TerrainEdit(_) => {
            let frame = EnuFrame::at(at);
            Some((
                at,
                vec![(Axis::East, frame.east), (Axis::North, frame.north)],
            ))
        }
        // An edge is dragged by its support points, which it already has; a
        // walkway is reshaped vertex by vertex with its own tool; a water
        // body has no handles — it is picked whole, and reshaped in the file.
        Selection::Edge(_)
        | Selection::TrackArea(_)
        | Selection::EnvelopePoint(_)
        | Selection::WalkPath(_)
        | Selection::WalkArea(_)
        | Selection::Field(_)
        | Selection::Water(_)
        | Selection::Road(_)
        | Selection::None => None,
    }
}

/// The gizmo as it stands: where it sits, which handles it shows with their
/// directions, and how long they are drawn [m].
type Handles = (EcefPos, Vec<(Axis, DVec3)>, f64);

/// Handles of `mode` — rotation is one ring, translation the arrows.
fn handles_for(
    line: &Line,
    selection: Selection,
    focus: &Focus,
    marks: &crate::terrain::Marks,
    mode: GizmoMode,
) -> Option<Handles> {
    let (at, all) = handles(line, selection, focus, marks)?;
    let wanted: Vec<(Axis, DVec3)> = all
        .into_iter()
        .filter(|(axis, _)| (*axis == Axis::Yaw) == (mode == GizmoMode::Rotate))
        .collect();
    if wanted.is_empty() {
        return None;
    }
    let length = (focus.camera_pos().0.distance(at.0) * HANDLE_SHARE).max(MIN_HANDLE);
    Some((at, wanted, length))
}

/// Grabbing, dragging and letting go of a handle.
#[allow(clippy::too_many_arguments)]
pub fn input(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    origin: Res<Origin>,
    focus: Res<Focus>,
    marks: Res<crate::terrain::Marks>,
    state: Res<EditorState>,
    mut gizmo: ResMut<GizmoState>,
    mut line: ResMut<Line>,
) {
    // W and E pick the mode, as in Unreal — the letters are free the moment
    // the right button is let go, where they stop flying the camera.
    if !state.typing && !buttons.pressed(MouseButton::Right) {
        if keys.just_pressed(KeyCode::KeyW) {
            gizmo.mode = GizmoMode::Translate;
        }
        if keys.just_pressed(KeyCode::KeyE) {
            gizmo.mode = GizmoMode::Rotate;
        }
    }

    if !buttons.pressed(MouseButton::Left) {
        gizmo.drag = None;
    }
    let cursor = windows
        .single()
        .ok()
        .and_then(|w| w.cursor_position())
        .filter(|c| state.over_viewport(*c));
    let Some((cursor, (camera, transform))) = cursor.zip(cameras.single().ok()) else {
        gizmo.hovered = None;
        return;
    };

    // An active drag owns the mouse until the button goes up.
    if let Some(drag) = &mut gizmo.drag {
        let now = match drag.axis {
            Axis::Yaw => ring_angle(camera, transform, cursor, &origin.0, drag.at, drag.dir),
            _ => axis_param(camera, transform, cursor, &origin.0, drag.at, drag.dir),
        };
        if let Some(now) = now {
            let delta = wrapped_delta(drag.axis, now - drag.last);
            drag.last = now;
            if delta != 0.0 {
                apply(
                    &mut line,
                    state.selection,
                    drag.axis,
                    drag.dir,
                    delta,
                    &focus,
                );
            }
        }
        return;
    }

    // Alt belongs to the camera orbit; a gizmo that grabbed under it would
    // make the selection jump every time the view is swung.
    if keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight) {
        gizmo.hovered = None;
        return;
    }
    let Some((at, wanted, length)) =
        handles_for(&line, state.selection, &focus, &marks, gizmo.mode)
    else {
        gizmo.hovered = None;
        return;
    };
    gizmo.hovered = wanted
        .iter()
        .filter_map(|(axis, dir)| {
            let d = match axis {
                Axis::Yaw => ring_distance(camera, transform, cursor, &origin.0, at, *dir, length),
                _ => handle_distance(camera, transform, cursor, &origin.0, at, *dir, length),
            }?;
            (d <= GRAB_PIXELS).then_some((*axis, d))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(axis, _)| axis);

    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(axis) = gizmo.hovered else {
        return;
    };
    let dir = wanted
        .iter()
        .find(|(a, _)| *a == axis)
        .map(|(_, d)| *d)
        .unwrap_or(DVec3::Z);
    let start = match axis {
        Axis::Yaw => ring_angle(camera, transform, cursor, &origin.0, at, dir),
        _ => axis_param(camera, transform, cursor, &origin.0, at, dir),
    };
    if let Some(last) = start {
        gizmo.drag = Some(Drag {
            axis,
            at,
            dir,
            last,
        });
    }
}

/// A ring drag crossing the ±π seam is a small step, not a full turn back.
fn wrapped_delta(axis: Axis, delta: f64) -> f64 {
    if axis != Axis::Yaw {
        return delta;
    }
    let turn = std::f64::consts::TAU;
    (delta + std::f64::consts::PI).rem_euclid(turn) - std::f64::consts::PI
}

/// Applies one frame of a handle drag: `delta` metres along the axis, or
/// radians around the ring. Every axis writes the field it stands for, so an
/// undo step reads like what the user did.
fn apply(line: &mut Line, selection: Selection, axis: Axis, dir: DVec3, delta: f64, focus: &Focus) {
    match selection {
        Selection::Device(i) => {
            let Some(edge) = line.source.devices.get(i).map(|d| d.edge) else {
                return;
            };
            let length = edge_length(line, edge);
            let Some(device) = line.source.devices.get_mut(i) else {
                return;
            };
            match axis {
                Axis::Along => device.s = (device.s + delta).clamp(0.0, length),
                Axis::Across => device.lateral_offset += delta,
                _ => {}
            }
        }
        Selection::Object(i) => {
            let Some(edge) = line.source.objects.get(i).map(|o| o.edge) else {
                return;
            };
            let length = edge_length(line, edge);
            let Some(object) = line.source.objects.get_mut(i) else {
                return;
            };
            match axis {
                Axis::Along => object.s = (object.s + delta).clamp(0.0, length),
                Axis::Across => object.lateral_offset += delta,
                Axis::Up => object.height += delta,
                // `yaw_deg` runs clockwise seen from above, the ring angle
                // counter-clockwise — the sign flip is the whole conversion.
                Axis::Yaw => {
                    object.yaw_deg = (object.yaw_deg - delta.to_degrees()).rem_euclid(360.0)
                }
                _ => {}
            }
        }
        Selection::Building(i) => {
            let Some(edge) = line.source.buildings.get(i).map(|building| building.edge) else {
                return;
            };
            let length = edge_length(line, edge);
            let Some(building) = line.source.buildings.get_mut(i) else {
                return;
            };
            match axis {
                Axis::Along => building.s = (building.s + delta).clamp(0.0, length),
                Axis::Across => building.lateral_offset += delta,
                Axis::Up => building.height += delta,
                Axis::Yaw => {
                    building.yaw_deg = (building.yaw_deg - delta.to_degrees()).rem_euclid(360.0)
                }
                _ => {}
            }
        }
        Selection::Tree(i) => {
            if let Some(tree) = line.source.trees.get_mut(i) {
                move_geo(&mut tree.lat, &mut tree.lon, focus, dir, delta);
            }
        }
        Selection::Marker(i) => {
            if let Some(marker) = line.source.markers.get_mut(i) {
                move_geo(&mut marker.lat, &mut marker.lon, focus, dir, delta);
            }
        }
        Selection::TerrainEdit(i) => {
            if let Some(edit) = line.source.terrain.get_mut(i) {
                move_geo(&mut edit.lat, &mut edit.lon, focus, dir, delta);
            }
        }
        Selection::Edge(_)
        | Selection::TrackArea(_)
        | Selection::EnvelopePoint(_)
        | Selection::WalkPath(_)
        | Selection::WalkArea(_)
        | Selection::Field(_)
        | Selection::Water(_)
        | Selection::Road(_)
        | Selection::None => {}
    }
}

fn edge_length(line: &Line, edge: u32) -> f64 {
    line.net
        .edges()
        .get(edge as usize)
        .map_or(0.0, |e| e.length())
}

/// Moves a geographic position `delta` metres along `dir` — through ECEF, so a
/// metre stays a metre at any latitude.
fn move_geo(lat: &mut f64, lon: &mut f64, focus: &Focus, dir: DVec3, delta: f64) {
    let height = geo::from_ecef(focus.position).2;
    let p = geo::to_ecef_deg(*lat, *lon, height);
    let (new_lat, new_lon, _) = geo::from_ecef(EcefPos(p.0 + dir * delta));
    *lat = new_lat.to_degrees();
    *lon = new_lon.to_degrees();
}

/// Where the cursor ray comes closest to the handle line [m from `at`] — the
/// closest-approach parameter of two lines, which is what makes an arrow drag
/// follow the mouse at any camera angle.
fn axis_param(
    camera: &Camera,
    transform: &GlobalTransform,
    cursor: Vec2,
    origin: &RenderOrigin,
    at: EcefPos,
    dir: DVec3,
) -> Option<f64> {
    let ray = camera.viewport_to_world(transform, cursor).ok()?;
    let d = origin.dir_to_render(dir).normalize_or_zero();
    let r = *ray.direction;
    let w0 = origin.to_render(at) - ray.origin;
    let b = d.dot(r);
    // Both are unit vectors, so the determinant is 1 − b²; it vanishes when
    // the axis is looked at end-on, and then no drag distance is meaningful.
    let denominator = 1.0 - b * b;
    if denominator.abs() < 1e-5 {
        return None;
    }
    Some(((b * r.dot(w0) - d.dot(w0)) / denominator) as f64)
}

/// Angle of the cursor around the gizmo, in the plane the ring lies in [rad].
fn ring_angle(
    camera: &Camera,
    transform: &GlobalTransform,
    cursor: Vec2,
    origin: &RenderOrigin,
    at: EcefPos,
    normal: DVec3,
) -> Option<f64> {
    let ray = camera.viewport_to_world(transform, cursor).ok()?;
    let n = origin.dir_to_render(normal).normalize_or_zero();
    let centre = origin.to_render(at);
    let denominator = ray.direction.dot(n);
    if denominator.abs() < 1e-6 {
        return None;
    }
    let t = (centre - ray.origin).dot(n) / denominator;
    if t <= 0.0 {
        return None;
    }
    let frame = EnuFrame::at(at);
    let local = frame.dir_to_local(origin.from_render(ray.get_point(t)).0 - at.0);
    Some(local.y.atan2(local.x))
}

/// Pixels from the cursor to the drawn arrow.
fn handle_distance(
    camera: &Camera,
    transform: &GlobalTransform,
    cursor: Vec2,
    origin: &RenderOrigin,
    at: EcefPos,
    dir: DVec3,
    length: f64,
) -> Option<f32> {
    let a = camera
        .world_to_viewport(transform, origin.to_render(at))
        .ok()?;
    let tip = EcefPos(at.0 + dir * length);
    let b = camera
        .world_to_viewport(transform, origin.to_render(tip))
        .ok()?;
    Some(segment_distance(a, b, cursor))
}

/// Pixels from the cursor to the drawn ring — the nearest of its sample points.
fn ring_distance(
    camera: &Camera,
    transform: &GlobalTransform,
    cursor: Vec2,
    origin: &RenderOrigin,
    at: EcefPos,
    normal: DVec3,
    radius: f64,
) -> Option<f32> {
    ring_points(at, normal, radius)
        .filter_map(|p| {
            camera
                .world_to_viewport(transform, origin.to_render(p))
                .ok()
        })
        .map(|p| p.distance(cursor))
        .min_by(f32::total_cmp)
}

/// The ring as world points — one source for drawing it and for hitting it.
fn ring_points(at: EcefPos, normal: DVec3, radius: f64) -> impl Iterator<Item = EcefPos> {
    const STEPS: usize = 48;
    let frame = EnuFrame::at(at);
    // In the item's own plane, so the ring lies flat on the ground it turns on.
    let u = frame.east - normal * frame.east.dot(normal);
    let u = u.normalize_or_zero();
    let v = normal.cross(u);
    (0..=STEPS).map(move |i| {
        let angle = std::f64::consts::TAU * i as f64 / STEPS as f64;
        EcefPos(at.0 + (u * angle.cos() + v * angle.sin()) * radius)
    })
}

fn segment_distance(a: Vec2, b: Vec2, cursor: Vec2) -> f32 {
    let ab = b - a;
    let length_squared = ab.length_squared();
    if length_squared < 1e-6 {
        return a.distance(cursor);
    }
    let t = ((cursor - a).dot(ab) / length_squared).clamp(0.0, 1.0);
    a.lerp(b, t).distance(cursor)
}

/// Unreal's axis colours — the grabbed or hovered one turns yellow.
fn axis_color(axis: Axis, active: bool) -> Color {
    if active {
        return Color::srgb(1.0, 0.85, 0.20);
    }
    match axis {
        Axis::Along | Axis::East => Color::srgb(0.92, 0.26, 0.26),
        Axis::Across | Axis::North => Color::srgb(0.34, 0.85, 0.38),
        Axis::Up => Color::srgb(0.30, 0.55, 0.96),
        Axis::Yaw => Color::srgb(0.30, 0.55, 0.96),
    }
}

/// Draws the handles of the current selection.
pub fn draw(
    gizmo: Res<GizmoState>,
    state: Res<EditorState>,
    line: Res<Line>,
    origin: Res<Origin>,
    focus: Res<Focus>,
    marks: Res<crate::terrain::Marks>,
    mut gizmos: Gizmos,
) {
    let Some((at, wanted, length)) =
        handles_for(&line, state.selection, &focus, &marks, gizmo.mode)
    else {
        return;
    };
    let active = gizmo.drag.as_ref().map(|d| d.axis).or(gizmo.hovered);
    let centre = origin.0.to_render(at);
    for (axis, dir) in wanted {
        let color = axis_color(axis, active == Some(axis));
        if axis == Axis::Yaw {
            gizmos.linestrip(
                ring_points(at, dir, length).map(|p| origin.0.to_render(p)),
                color,
            );
        } else {
            let tip = centre + origin.0.dir_to_render(dir) * length as f32;
            gizmos.arrow(centre, tip, color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ring drag across the ±π seam is a small step, not a turn back.
    #[test]
    fn ring_deltas_wrap() {
        let epsilon = 0.05;
        let seam = -std::f64::consts::TAU + 2.0 * epsilon;
        assert!((wrapped_delta(Axis::Yaw, seam) - 2.0 * epsilon).abs() < 1e-9);
        // A straight axis takes its delta as it comes.
        assert_eq!(wrapped_delta(Axis::Along, seam), seam);
    }

    /// The ring lies in the plane its normal defines, at the given radius.
    #[test]
    fn ring_lies_flat_around_the_item() {
        let at = geo::to_ecef_deg(52.0, 10.0, 146.0);
        let normal = EnuFrame::at(at).up;
        for p in ring_points(at, normal, 25.0) {
            let offset = p.0 - at.0;
            assert!((offset.length() - 25.0).abs() < 1e-6, "{}", offset.length());
            assert!(offset.dot(normal).abs() < 1e-6);
        }
    }

    /// Sliding a device along the track changes its arc length and stays on
    /// the edge — dragging past the joint must not write an `s` the edge has
    /// no point for.
    #[test]
    fn along_track_moves_and_clamps() {
        let source = content::musterbahn();
        let mut line = Line {
            net: source.compile().expect("compiles").net,
            source,
            path: None,
            dirty: false,
            needs_rebuild: false,
            terrain_change: Default::default(),
            recenter: false,
            issues: Vec::new(),
        };
        let focus = Focus {
            position: geo::to_ecef_deg(52.0, 10.0, 146.0),
            height: 500.0,
            yaw: 0.0,
            pitch: 0.9,
            speed_step: crate::view::DEFAULT_SPEED_STEP,
            speed_scalar: 1.0,
        };
        let device = line.source.devices.first().expect("example has devices");
        let (edge, before) = (device.edge, device.s);
        let length = edge_length(&line, edge);

        apply(
            &mut line,
            Selection::Device(0),
            Axis::Along,
            DVec3::X,
            12.0,
            &focus,
        );
        assert!((line.source.devices[0].s - (before + 12.0)).abs() < 1e-9);

        apply(
            &mut line,
            Selection::Device(0),
            Axis::Along,
            DVec3::X,
            1e6,
            &focus,
        );
        assert_eq!(line.source.devices[0].s, length);
        apply(
            &mut line,
            Selection::Device(0),
            Axis::Along,
            DVec3::X,
            -1e6,
            &focus,
        );
        assert_eq!(line.source.devices[0].s, 0.0);
    }

    /// A tree dragged east lands east of where it stood, by the metres asked
    /// for — the conversion runs through ECEF, not through a degree constant.
    #[test]
    fn geo_drag_moves_by_metres() {
        let focus = Focus {
            position: geo::to_ecef_deg(52.0, 10.0, 146.0),
            height: 500.0,
            yaw: 0.0,
            pitch: crate::view::DEFAULT_PITCH,
            speed_step: crate::view::DEFAULT_SPEED_STEP,
            speed_scalar: 1.0,
        };
        let (mut lat, mut lon) = (52.0, 10.0);
        let before = geo::to_ecef_deg(lat, lon, 146.0);
        let east = EnuFrame::at(before).east;
        move_geo(&mut lat, &mut lon, &focus, east, 100.0);
        let after = geo::to_ecef_deg(lat, lon, 146.0);
        assert!((after.0.distance(before.0) - 100.0).abs() < 0.01);
        assert!(lon > 10.0 && (lat - 52.0).abs() < 1e-6, "{lat} {lon}");
    }
}
