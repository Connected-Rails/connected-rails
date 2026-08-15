//! Editing tools of the route editor (plan ch. 15, editor v1: tracks + devices).
//!
//! Picking happens on the map plane — the horizontal plane through the focus
//! point — because the editor looks straight down at it. Track drawing is an
//! arc-to-point tool: every click appends the one circular arc (or straight)
//! that leaves the alignment tangentially and hits the clicked point, so the
//! drawn track is G1-continuous by construction.

use crate::{Focus, Line, Origin};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use content::route::{DeviceSource, EdgeSource, EdgeStart, GeoPoint, NodeSource};
use glam::{DVec2, DVec3};
use i18n::t;
use track_model::{DeviceKind, Facing, Segment, TrackNetwork};
use world_coords::{EcefPos, EnuFrame, RenderOrigin, geo};

/// Active tool of the viewport.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Select,
    DrawTrack,
    PlaceDevice,
}

/// What the Select tool holds.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Selection {
    #[default]
    None,
    Edge(usize),
    Device(usize),
}

/// Tool state, selection and what the UI pass leaves behind for the input
/// systems: the free viewport rect and whether a text field has focus.
#[derive(Resource, Default)]
pub struct EditorState {
    pub tool: Tool,
    pub selection: Selection,
    pub drawing: Option<Drawing>,
    /// Kind the Place-device tool stamps.
    pub device_kind: Option<DeviceKind>,
    /// Free viewport in logical pixels. The panels dock into a hand-built
    /// background `Ui`, which egui's area hit test never sees — so "is the
    /// mouse over UI?" is answered against this rect, not by egui.
    pub viewport: Rect,
    /// A text field owns the keyboard — Delete/Enter/WASD belong to it then.
    pub typing: bool,
    /// Owner for native dialogs; a parentless dialog may open behind the window.
    pub window: Option<bevy::window::RawHandleWrapper>,
    /// Comment-loss warning shown once per session (see the vehicle editor).
    pub warned_about_comments: bool,
    /// Whether the user has moved the map or used a tool yet — until then the
    /// viewport shows how.
    pub map_used: bool,
}

impl EditorState {
    pub fn device_kind(&self) -> DeviceKind {
        self.device_kind.clone().unwrap_or(DeviceKind::Signal)
    }
}

/// A track being drawn. The first click anchors the ENU frame and the start
/// point, the second fixes the initial heading, every further click appends a
/// tangent-continuous arc.
///
/// ponytail: the whole alignment lives in the first point's EN plane —
/// metre-true for the few km a hand-drawn track spans; per-segment
/// re-anchoring steps in when someone draws across a whole map sheet.
pub struct Drawing {
    frame: EnuFrame,
    pub start: GeoPoint,
    /// Compass heading of the first segment [deg]; `None` until the second click.
    pub heading_deg: Option<f64>,
    pub segments: Vec<Segment>,
    /// End of the drawn alignment in the frame's EN plane.
    end: DVec2,
    /// Math heading at the end [rad], 0 = east, counter-clockwise.
    end_heading: f64,
}

impl Drawing {
    pub fn start_at(p: EcefPos, geoid_offset: f64) -> Self {
        let (lat, lon, height) = geo::from_ecef(p);
        Self {
            frame: EnuFrame::at(p),
            start: GeoPoint {
                lat: lat.to_degrees(),
                lon: lon.to_degrees(),
                height: height - geoid_offset,
            },
            heading_deg: None,
            segments: Vec::new(),
            end: DVec2::ZERO,
            end_heading: 0.0,
        }
    }

    fn local(&self, p: EcefPos) -> DVec2 {
        let l = self.frame.to_local(p);
        DVec2::new(l.x, l.y)
    }

    /// The segment a click at `p` would append, with the heading after it.
    fn preview(&self, p: EcefPos) -> Option<(Segment, f64)> {
        let target = self.local(p);
        match self.heading_deg {
            None => {
                let len = target.length();
                (len > 1.0).then(|| (Segment::straight(len), target.y.atan2(target.x)))
            }
            Some(_) => segment_to(self.end, self.end_heading, target),
        }
    }

    /// Appends the segment towards `p`; a click behind the heading is ignored.
    pub fn click(&mut self, p: EcefPos) {
        let Some((segment, end_heading)) = self.preview(p) else {
            return;
        };
        if self.heading_deg.is_none() {
            self.heading_deg = Some((90.0 - end_heading.to_degrees()).rem_euclid(360.0));
        }
        self.end = self.local(p);
        self.end_heading = end_heading;
        self.segments.push(segment);
    }

    /// Render polyline of the alignment so far; `cursor` appends the segment
    /// the next click would create.
    pub fn polyline(&self, cursor: Option<EcefPos>, origin: &RenderOrigin) -> Vec<Vec3> {
        let mut heading = self
            .heading_deg
            .map(|d| (90.0 - d).to_radians())
            .unwrap_or(0.0);
        let mut segments = self.segments.clone();
        if let Some(p) = cursor
            && let Some((segment, _)) = self.preview(p)
        {
            if self.heading_deg.is_none() {
                let target = self.local(p);
                heading = target.y.atan2(target.x);
            }
            segments.push(segment);
        }
        let mut position = DVec2::ZERO;
        let mut points = vec![self.to_render(position, origin)];
        for segment in &segments {
            let steps = (segment.len / 5.0).ceil().max(1.0) as usize;
            for i in 1..=steps {
                let (p, _) = advance(
                    position,
                    heading,
                    segment,
                    segment.len * i as f64 / steps as f64,
                );
                points.push(self.to_render(p, origin));
            }
            let (p, h) = advance(position, heading, segment, segment.len);
            position = p;
            heading = h;
        }
        points
    }

    fn to_render(&self, p: DVec2, origin: &RenderOrigin) -> Vec3 {
        origin.to_render(self.frame.to_ecef(DVec3::new(p.x, p.y, 0.5)))
    }
}

/// Position and heading after `s` metres of `segment`, starting at `p` with
/// math heading `h`. Closed form — the drawing tool only produces `dk = 0`.
fn advance(p: DVec2, h: f64, segment: &Segment, s: f64) -> (DVec2, f64) {
    let h1 = h + segment.k0 * s;
    if segment.k0.abs() < 1e-9 {
        (p + DVec2::new(h.cos(), h.sin()) * s, h1)
    } else {
        let k = segment.k0;
        (
            p + DVec2::new((h1.sin() - h.sin()) / k, (h.cos() - h1.cos()) / k),
            h1,
        )
    }
}

/// Tangent-continuous segment from `from` with math heading `heading` to
/// `target`: a straight where the deviation is negligible, else the one
/// circular arc that leaves tangentially and passes through the point.
/// `None` when the target lies too far behind the heading for a sane arc.
fn segment_to(from: DVec2, heading: f64, target: DVec2) -> Option<(Segment, f64)> {
    let chord = target - from;
    let len = chord.length();
    if len < 1.0 {
        return None;
    }
    let dir = DVec2::new(heading.cos(), heading.sin());
    // Angle from the heading to the chord; the arc turns twice that.
    let phi = dir.perp_dot(chord).atan2(dir.dot(chord));
    if phi.abs() > 2.4 {
        return None;
    }
    if phi.abs() < 1e-4 {
        return Some((Segment::straight(len), heading));
    }
    let segment = Segment {
        len: len * phi / phi.sin(),
        k0: 2.0 * phi.sin() / len,
        dk: 0.0,
    };
    Some((segment, heading + 2.0 * phi))
}

/// Cursor → point on the map plane (the horizontal plane through the focus).
pub fn pick_ground(
    camera: &Camera,
    camera_transform: &GlobalTransform,
    cursor: Vec2,
    origin: &RenderOrigin,
    focus: &Focus,
) -> Option<EcefPos> {
    let ray = camera.viewport_to_world(camera_transform, cursor).ok()?;
    let frame = EnuFrame::at(focus.position);
    let plane_point = origin.to_render(focus.position);
    let normal = origin.dir_to_render(frame.up);
    let denominator = ray.direction.dot(normal);
    if denominator.abs() < 1e-6 {
        return None;
    }
    let t = (plane_point - ray.origin).dot(normal) / denominator;
    (t > 0.0).then(|| origin.from_render(ray.get_point(t)))
}

/// Closest point of the network to `p`: `(edge index, s, distance)`.
///
/// ponytail: a linear scan over sampled edges per click — fine at editor
/// scale; a spatial index steps in when a whole federal state feels sluggish.
pub fn nearest_on_network(net: &TrackNetwork, p: EcefPos) -> Option<(usize, f64, f64)> {
    let mut best: Option<(usize, f64, f64)> = None;
    for (i, edge) in net.edges().iter().enumerate() {
        let length = edge.length();
        let mut step = 10.0_f64.min(length.max(0.01));
        let mut s_best = 0.0;
        let mut d_best = f64::MAX;
        let probe = |s: f64, d_best: &mut f64, s_best: &mut f64| {
            let d = edge.eval(s).pos.distance(p);
            if d < *d_best {
                *d_best = d;
                *s_best = s;
            }
        };
        let coarse = (length / step).ceil() as usize;
        for j in 0..=coarse {
            probe((j as f64 * step).min(length), &mut d_best, &mut s_best);
        }
        for _ in 0..2 {
            let fine = step / 10.0;
            let mut s = (s_best - step).max(0.0);
            let hi = (s_best + step).min(length);
            while s <= hi {
                probe(s, &mut d_best, &mut s_best);
                s += fine;
            }
            step = fine;
        }
        if best.is_none_or(|(_, _, d)| d_best < d) {
            best = Some((i, s_best, d_best));
        }
    }
    best
}

/// World position of a device, lateral offset included.
pub fn device_pos(net: &TrackNetwork, device: &DeviceSource) -> Option<EcefPos> {
    let edge = net.edges().get(device.edge as usize)?;
    let pose = edge.eval(device.s.clamp(0.0, edge.length()));
    let right = pose.tangent.cross(pose.up).normalize_or_zero();
    Some(EcefPos(pose.pos.0 + right * device.lateral_offset))
}

/// How close a click has to come, scaled with the view height.
fn pick_radius(focus: &Focus) -> f64 {
    (focus.height * 0.02).max(8.0)
}

/// Removes whatever is selected — the Delete key and the Edit menu share it.
pub fn delete_selection(line: &mut Line, state: &mut EditorState) {
    match std::mem::take(&mut state.selection) {
        Selection::Edge(i) => line.source.remove_edge(i),
        Selection::Device(i) => line.source.remove_device(i),
        Selection::None => {}
    }
}

/// Turns the finished drawing into two buffer nodes and one edge.
pub fn finish_drawing(line: &mut Line, state: &mut EditorState) {
    let Some(drawing) = state.drawing.take() else {
        return;
    };
    let (Some(heading_deg), false) = (drawing.heading_deg, drawing.segments.is_empty()) else {
        return;
    };
    let node = line.source.nodes.len() as u32;
    line.source.nodes.push(NodeSource::Buffer);
    line.source.nodes.push(NodeSource::Buffer);
    line.source.edges.push(EdgeSource {
        from: node,
        to: node + 1,
        start: EdgeStart::Geo {
            point: drawing.start,
            heading_deg,
        },
        segments: drawing.segments,
        grade: vec![],
        cant: vec![],
        speed: vec![],
    });
    state.selection = Selection::Edge(line.source.edges.len() - 1);
}

/// Mouse and keyboard input of the three tools.
#[allow(clippy::too_many_arguments)]
pub fn tool_input(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    camera: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    origin: Res<Origin>,
    focus: Res<Focus>,
    mut state: ResMut<EditorState>,
    mut line: ResMut<Line>,
    mut overlay: ResMut<crate::overlay::Overlay>,
) {
    // A stale selection (after undo, or an edit elsewhere) is cleared, not chased.
    match state.selection {
        Selection::Edge(i) if i >= line.source.edges.len() => state.selection = Selection::None,
        Selection::Device(i) if i >= line.source.devices.len() => {
            state.selection = Selection::None;
        }
        _ => {}
    }
    if state.typing {
        return;
    }

    if keys.just_pressed(KeyCode::Escape) && state.drawing.take().is_none() {
        state.selection = Selection::None;
    }
    if keys.just_pressed(KeyCode::Delete) {
        delete_selection(&mut line, &mut state);
    }
    if keys.just_pressed(KeyCode::Enter) || buttons.just_pressed(MouseButton::Right) {
        finish_drawing(&mut line, &mut state);
    }
    // Tool switching from the keyboard, as every map editor has it.
    for (key, tool) in [
        (KeyCode::Digit1, Tool::Select),
        (KeyCode::Digit2, Tool::DrawTrack),
        (KeyCode::Digit3, Tool::PlaceDevice),
    ] {
        if keys.just_pressed(key) && state.tool != tool {
            state.tool = tool;
            state.drawing = None;
        }
    }

    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(cursor) = windows.single().ok().and_then(|w| w.cursor_position()) else {
        return;
    };
    if !state.viewport.contains(cursor) {
        return;
    }
    let Ok((camera, camera_transform)) = camera.single() else {
        return;
    };
    let Some(p) = pick_ground(camera, camera_transform, cursor, &origin.0, &focus) else {
        return;
    };
    state.map_used = true;

    match state.tool {
        Tool::DrawTrack => match &mut state.drawing {
            None => state.drawing = Some(Drawing::start_at(p, line.source.geoid_offset)),
            Some(drawing) => drawing.click(p),
        },
        Tool::PlaceDevice => {
            match nearest_on_network(&line.net, p) {
                Some((edge, s, distance)) if distance <= pick_radius(&focus) => {
                    let kind = state.device_kind();
                    line.source.devices.push(DeviceSource {
                        kind,
                        edge: edge as u32,
                        s,
                        facing: Facing::default(),
                        lateral_offset: 0.0,
                        payload: String::new(),
                    });
                    state.selection = Selection::Device(line.source.devices.len() - 1);
                }
                _ => overlay.status = t!("status-no-track-hit"),
            };
        }
        Tool::Select => {
            let radius = pick_radius(&focus);
            let device = line
                .source
                .devices
                .iter()
                .enumerate()
                .filter_map(|(i, d)| Some((i, device_pos(&line.net, d)?.distance(p))))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .filter(|(_, d)| *d <= radius);
            state.selection = match device {
                Some((i, _)) => Selection::Device(i),
                None => match nearest_on_network(&line.net, p) {
                    Some((i, _, d)) if d <= radius => Selection::Edge(i),
                    _ => Selection::None,
                },
            };
        }
    }
}

/// Selection highlight and drawing preview.
pub fn draw_gizmos(
    state: Res<EditorState>,
    line: Res<Line>,
    origin: Res<Origin>,
    focus: Res<Focus>,
    windows: Query<&Window, With<PrimaryWindow>>,
    camera: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut gizmos: Gizmos,
) {
    let accent = Color::srgb(0.36, 0.61, 0.96);
    match state.selection {
        Selection::Edge(i) => {
            if let Some(edge) = line.net.edges().get(i) {
                let steps = ((edge.length() / 10.0).ceil() as usize).max(2);
                let points = (0..=steps).map(|j| {
                    let pose = edge.eval(edge.length() * j as f64 / steps as f64);
                    let up = origin.0.dir_to_render(pose.up);
                    origin.0.to_render(pose.pos) + up
                });
                gizmos.linestrip(points, accent);
            }
        }
        Selection::Device(i) => {
            if let Some(device) = line.source.devices.get(i)
                && let Some(p) = device_pos(&line.net, device)
            {
                let up = origin.0.dir_to_render(EnuFrame::at(p).up);
                let rotation = Quat::from_rotation_arc(Vec3::Z, up);
                let radius = (focus.height * 0.012).max(4.0) as f32;
                gizmos.circle(
                    Isometry3d::new(origin.0.to_render(p) + up, rotation),
                    radius,
                    accent,
                );
            }
        }
        Selection::None => {}
    }

    if let Some(drawing) = &state.drawing {
        let cursor = windows
            .single()
            .ok()
            .and_then(|w| w.cursor_position())
            .filter(|c| state.viewport.contains(*c))
            .and_then(|c| {
                let (camera, camera_transform) = camera.single().ok()?;
                pick_ground(camera, camera_transform, c, &origin.0, &focus)
            });
        gizmos.linestrip(drawing.polyline(cursor, &origin.0), accent);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arc_to_point_hits_the_target() {
        // Quarter circle: east heading, target at (r, r) → radius r, turn 90° left.
        let r = 500.0;
        let (segment, heading) = segment_to(DVec2::ZERO, 0.0, DVec2::new(r, r)).unwrap();
        assert!((segment.k0 - 1.0 / r).abs() < 1e-9, "{}", segment.k0);
        assert!((segment.len - r * std::f64::consts::FRAC_PI_2).abs() < 1e-6);
        assert!((heading - std::f64::consts::FRAC_PI_2).abs() < 1e-9);

        // The closed form lands on the clicked point.
        let (end, _) = advance(DVec2::ZERO, 0.0, &segment, segment.len);
        assert!(end.distance(DVec2::new(r, r)) < 1e-6, "{end}");
    }

    #[test]
    fn a_point_dead_ahead_becomes_a_straight() {
        let (segment, heading) = segment_to(DVec2::ZERO, 0.0, DVec2::new(300.0, 0.0)).unwrap();
        assert_eq!(segment.k0, 0.0);
        assert!((segment.len - 300.0).abs() < 1e-9);
        assert_eq!(heading, 0.0);
    }

    #[test]
    fn a_point_behind_is_rejected() {
        assert!(segment_to(DVec2::ZERO, 0.0, DVec2::new(-100.0, 1.0)).is_none());
    }

    /// A drawn edge compiles to the geometry that was previewed: heading and
    /// segments survive the round trip through the compass-bearing format.
    #[test]
    fn a_finished_drawing_compiles_where_it_was_drawn() {
        let start = world_coords::geo::to_ecef_deg(52.0, 10.0, 146.0);
        let mut drawing = Drawing::start_at(start, 46.0);
        let frame = EnuFrame::at(start);
        // Second click 1 km north-east, third bending further east.
        let ne = frame.to_ecef(DVec3::new(700.0, 700.0, 0.0));
        let bend = frame.to_ecef(DVec3::new(1700.0, 1100.0, 0.0));
        drawing.click(ne);
        drawing.click(bend);
        assert_eq!(drawing.segments.len(), 2);

        let mut line = content::LineSource {
            name: "drawn".into(),
            geoid_offset: 46.0,
            nodes: vec![],
            edges: vec![],
            devices: vec![],
            sections: vec![],
            signals: vec![],
            routes: vec![],
            boundaries: vec![],
            script: None,
        };
        let mut state = EditorState {
            drawing: Some(drawing),
            ..Default::default()
        };
        let mut doc = Line {
            source: line.clone(),
            net: track_model::TrackNetwork::new(),
            path: None,
            dirty: false,
            needs_rebuild: false,
            recenter: false,
        };
        finish_drawing(&mut doc, &mut state);
        line = doc.source;
        let compiled = line.compile().expect("compiles");
        let end = compiled.net.edges()[0].end_pose().pos;
        assert!(
            end.distance(bend) < 0.5,
            "end missed the click by {} m",
            end.distance(bend)
        );
    }
}
