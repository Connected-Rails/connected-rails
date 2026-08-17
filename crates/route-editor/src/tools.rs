//! Editing tools of the route editor (plan ch. 15, editor v1: tracks + devices).
//!
//! Picking happens on the map plane — the horizontal plane through the focus
//! point — because the editor looks straight down at it. Track drawing is an
//! arc-to-point tool: every click appends the one circular arc (or straight)
//! that leaves the alignment tangentially and hits the clicked point, so the
//! drawn track is G1-continuous by construction.

use crate::{Focus, Ghost, Line, Origin, TrackObjects};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use content::LineSource;
use content::route::{
    DeviceSource, EdgeSource, EdgeStart, FlankSource, GeoPoint, MarkerSource, NodeSource,
    ObjectSource, TerrainEdit, TerrainEditSource, TreeSource,
};
use glam::{DVec2, DVec3};
use i18n::t;
use track_model::{DeviceKind, Facing, Segment, TrackNetwork, TrackPose};
use world_coords::{EcefPos, EnuFrame, RenderOrigin, geo};

/// Throw time a freshly placed turnout gets [s] — the file format's own
/// default; the selection panel edits it per switch afterwards.
const DEFAULT_THROW_TIME: f64 = 6.0;

/// Active tool of the viewport.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Select,
    DrawTrack,
    PlaceDevice,
    PlaceSwitch,
    PlaceObject,
    /// One tree per click, free of the track.
    PlaceTree,
    /// Forest brush: clicks collect a polygon; Enter/right-click bakes it into
    /// single trees, so every tree of the wood stays individually editable.
    PlaceForest,
    /// Marking brush: sweep over the map to mark trees and objects in bulk,
    /// then delete them together.
    Brush,
    /// One reference marker per click, into the layer the panel names.
    PlaceMarker,
    /// Terrain brush: every click stamps one stroke that raises, lowers or
    /// levels the ground around it.
    TerrainBrush,
    /// DGM tiles: clicks pick single terrain tiles for the height import.
    PickTile,
}

/// What the Select tool holds.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Selection {
    #[default]
    None,
    Edge(usize),
    Device(usize),
    Object(usize),
    Tree(usize),
    Marker(usize),
    TerrainEdit(usize),
}

/// One item the marking brush swept over.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Tree(usize),
    Object(usize),
}

/// What the interlocking panel points at right now — the row under the mouse.
/// Sections and routes are lists of indices; on the map they are stretches of
/// track, and this is what puts the two together.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Highlight {
    Section(usize),
    Route(usize),
}

/// Tool state, selection and what the UI pass leaves behind for the input
/// systems: the free viewport rect and whether a text field has focus.
#[derive(Resource, Default)]
pub struct EditorState {
    pub tool: Tool,
    pub selection: Selection,
    pub drawing: Option<Drawing>,
    /// Active support-point drag of the Select tool: `(edge, point index)`.
    pub drag: Option<(usize, usize)>,
    /// Kind the Place-device tool stamps.
    pub device_kind: Option<DeviceKind>,
    /// The Place-switch tool draws a trailing connection instead of a facing
    /// turnout — the branch then leaves against the clicked track's direction.
    pub switch_trailing: bool,
    /// Section or route the interlocking panel points at; the map draws it.
    /// Set by the panel every frame, so it follows the mouse by itself.
    pub highlight: Option<Highlight>,
    /// Overlap length the route derivation walks out behind the exit signal
    /// [m]; `None` = the regular length of the rulebook for the speed the
    /// route ends at (`content::route::regular_overlap`).
    pub overlap_length: Option<f64>,
    /// Panel section to scroll to on the next frame — a row that belongs
    /// somewhere else (a signal's routes) sends the panel there.
    pub jump_to: Option<&'static str>,
    /// Object (`"<mod>:<name>"`) the Place-object tool stamps.
    pub object: Option<String>,
    /// Tree object the tree and forest tools use; `None` = placeholder tree.
    pub tree_object: Option<String>,
    /// Corner points of the forest polygon being drawn.
    pub forest_points: Vec<EcefPos>,
    /// Forest brush density [m² per tree]; `None` = 500.
    pub forest_area: Option<f64>,
    /// Layer the marker tool writes into; `None` = `"reference"`.
    pub marker_layer: Option<String>,
    /// Label the marker tool stamps — empty is allowed.
    pub marker_label: String,
    /// Marker layers switched off: hidden on the map, unpickable, untouched in
    /// the file. Session state, not saved — a hidden layer that stays hidden
    /// after a restart is a layer someone searches for in vain.
    pub hidden_layers: std::collections::HashSet<String>,
    /// Radius of the terrain brush [m]; `None` = 60.
    pub terrain_radius: Option<f64>,
    /// How much one terrain stroke raises (+) or lowers (−) the ground [m];
    /// `None` = 2.
    pub terrain_amount: Option<f64>,
    /// The terrain brush levels to rail height instead of raising or lowering.
    pub terrain_level: bool,
    /// DGM directory or file the height import reads from.
    pub dgm_source: Option<String>,
    /// UTM zone of that delivery; `None` = 32.
    pub dgm_zone: Option<u8>,
    /// Grid spacing the module's own height tiles are written at [m];
    /// `None` = 10, which is well below the 4 m the terrain builds at the
    /// track without being the 1 m of the original delivery.
    pub dgm_cell: Option<f64>,
    /// Terrain tiles picked for a partial import; empty = the whole module.
    pub picked_tiles: Vec<content::TileKey>,
    /// Tiles the module already has height data for — read from disk after
    /// every import and when a line is opened, not per frame.
    pub dgm_present: Vec<content::TileKey>,
    /// Items the marking brush has swept over — deleted together.
    pub marked: Vec<Mark>,
    /// Radius of the marking brush [m]; `None` = 30.
    pub brush_radius: Option<f64>,
    /// Repeat spacing of the object panel [m]; `None` = the 65 m of a
    /// standard catenary span.
    pub repeat_interval: Option<f64>,
    /// Repeat end position [m along the edge]; `None` = the edge's end.
    pub repeat_until: Option<f64>,
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

    /// Layer the marker tool writes into.
    pub fn marker_layer(&self) -> String {
        match &self.marker_layer {
            Some(layer) if !layer.trim().is_empty() => layer.trim().to_string(),
            _ => DEFAULT_MARKER_LAYER.to_string(),
        }
    }

    pub fn layer_visible(&self, layer: &str) -> bool {
        !self.hidden_layers.contains(layer)
    }

    /// UTM zone of the DGM delivery the height import reads.
    pub fn dgm_zone(&self) -> u8 {
        self.dgm_zone.unwrap_or(32)
    }

    /// Grid spacing the module's height tiles are written at [m].
    pub fn dgm_cell(&self) -> f64 {
        self.dgm_cell.unwrap_or(10.0).clamp(1.0, 100.0)
    }

    /// The terrain tile grid the editor shows — the same one the app builds on.
    pub fn terrain_options(&self) -> content::TerrainOptions {
        content::TerrainOptions {
            zone: self.dgm_zone(),
            ..Default::default()
        }
    }
}

/// Layer a hand-placed marker lands in when none is named.
pub const DEFAULT_MARKER_LAYER: &str = "reference";

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
    /// The edge this drawing branches off (switch tool): `(edge, s)`.
    pub branch_of: Option<(usize, f64)>,
    /// Trailing turnout: the branch leaves against the running direction of
    /// the clicked track, so the far half of the split becomes the root.
    pub trailing: bool,
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
            branch_of: None,
            trailing: false,
            end: DVec2::ZERO,
            end_heading: 0.0,
        }
    }

    /// Branch drawing for the switch tool: starts on the track at `pose`
    /// (`edge`, `s`) with the track's own heading fixed, so the branch leaves
    /// tangentially — a turnout, not a crossing.
    ///
    /// `trailing` turns the heading around: the branch then runs back along
    /// the clicked track, which is what a trailing connection looks like from
    /// the driver of that track — the fork lies behind them, not ahead.
    pub fn branch_at(
        pose: &TrackPose,
        geoid_offset: f64,
        edge: usize,
        s: f64,
        trailing: bool,
    ) -> Self {
        let mut drawing = Self::start_at(pose.pos, geoid_offset);
        let along = if trailing {
            -pose.tangent
        } else {
            pose.tangent
        };
        let tangent = drawing.frame.dir_to_local(along);
        let heading = tangent.y.atan2(tangent.x);
        drawing.heading_deg = Some((90.0 - heading.to_degrees()).rem_euclid(360.0));
        drawing.end_heading = heading;
        drawing.branch_of = Some((edge, s));
        drawing.trailing = trailing;
        drawing
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

/// The positions a repeat run stamps: every `interval` metres from `start`
/// (exclusive) to `end` (inclusive, within float noise).
///
/// ponytail: repetition stays on one edge — a row that runs across joints and
/// switches needs path-following, and the next edge is one click away.
pub fn repeat_positions(start: f64, interval: f64, end: f64) -> Vec<f64> {
    if interval < 1.0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut s = start + interval;
    while s <= end + 1e-9 {
        out.push(s);
        s += interval;
    }
    out
}

/// Stamps copies of object `index` along its edge, every `interval` metres up
/// to `until` (clamped to the edge). Copies carry the instance's own offset,
/// rotation and height — the row repeats what stands, not the registry
/// default. Returns how many were placed; one undo step covers them all.
pub fn repeat_object(line: &mut Line, index: usize, interval: f64, until: f64) -> usize {
    let Some(template) = line.source.objects.get(index).cloned() else {
        return 0;
    };
    let Some(edge) = line.net.edges().get(template.edge as usize) else {
        return 0;
    };
    let positions = repeat_positions(template.s, interval, until.min(edge.length()));
    let placed = positions.len();
    for s in positions {
        line.source.objects.push(ObjectSource {
            s,
            ..template.clone()
        });
    }
    placed
}

/// World position of a scenery object, offset and height included.
pub fn object_pos(net: &TrackNetwork, object: &ObjectSource) -> Option<EcefPos> {
    let edge = net.edges().get(object.edge as usize)?;
    let pose = edge.eval(object.s.clamp(0.0, edge.length()));
    let right = pose.tangent.cross(pose.up).normalize_or_zero();
    Some(EcefPos(
        pose.pos.0 + right * object.lateral_offset + pose.up * object.height,
    ))
}

/// How close a click has to come, scaled with the view height. Still the
/// measure for what a click *places* (ghost snapping, tile picking) — what a
/// click *selects* is measured on screen, see [`ScreenPick`].
fn pick_radius(focus: &Focus) -> f64 {
    (focus.height * 0.02).max(8.0)
}

/// How near the cursor an item has to be to be picked [logical pixels].
const PICK_PIXELS: f32 = 12.0;

/// Selection measured on screen instead of in the world: in the 3D view a
/// metre at the horizon is a fraction of a pixel, so a world-space radius
/// picks half a hillside there while barely reaching the nearest signal. What
/// is under the cursor is a question about pixels.
pub struct ScreenPick<'a> {
    camera: &'a Camera,
    transform: &'a GlobalTransform,
    origin: &'a RenderOrigin,
    cursor: Vec2,
}

impl ScreenPick<'_> {
    /// Pixels between the cursor and `p`; `None` when it is off screen.
    pub fn distance(&self, p: EcefPos) -> Option<f32> {
        self.camera
            .world_to_viewport(self.transform, self.origin.to_render(p))
            .ok()
            .map(|screen| screen.distance(self.cursor))
    }

    /// The same, but only within grabbing distance.
    pub fn hits(&self, p: EcefPos) -> Option<f32> {
        self.distance(p).filter(|d| *d <= PICK_PIXELS)
    }
}

/// Where the selection sits — what the gizmo stands on and `F` frames.
pub fn selection_pos(line: &Line, selection: Selection, focus: &Focus) -> Option<EcefPos> {
    match selection {
        Selection::Edge(i) => {
            let edge = line.net.edges().get(i)?;
            Some(edge.eval(edge.length() / 2.0).pos)
        }
        Selection::Device(i) => device_pos(&line.net, line.source.devices.get(i)?),
        Selection::Object(i) => object_pos(&line.net, line.source.objects.get(i)?),
        Selection::Tree(i) => Some(tree_pos(line.source.trees.get(i)?, focus)),
        Selection::Marker(i) => Some(marker_pos(line.source.markers.get(i)?, focus)),
        Selection::TerrainEdit(i) => Some(terrain_pos(line.source.terrain.get(i)?, focus)),
        Selection::None => None,
    }
}

/// The edge whose ribbon runs nearest the cursor, within grabbing distance.
///
/// ponytail: samples every edge every 5 m and projects the samples — a linear
/// scan per click, like [`nearest_on_network`]; a screen-space index steps in
/// when a whole federal state feels sluggish.
fn nearest_edge(line: &Line, pick: &ScreenPick) -> Option<usize> {
    line.net
        .edges()
        .iter()
        .enumerate()
        .filter_map(|(i, edge)| {
            let steps = ((edge.length() / 5.0).ceil() as usize).max(1);
            let nearest = (0..=steps)
                .filter_map(|j| {
                    pick.distance(edge.eval(edge.length() * j as f64 / steps as f64).pos)
                })
                .min_by(f32::total_cmp)?;
            Some((i, nearest))
        })
        .filter(|(_, d)| *d <= PICK_PIXELS)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// World position of a source node — where its first edge starts or ends.
pub fn node_pos(source: &LineSource, net: &TrackNetwork, node: u32) -> Option<EcefPos> {
    source.edges.iter().enumerate().find_map(|(i, e)| {
        let edge = net.edges().get(i)?;
        if e.from == node {
            Some(edge.eval(0.0).pos)
        } else if e.to == node {
            Some(edge.end_pose().pos)
        } else {
            None
        }
    })
}

/// Snaps a picked point onto the nearest ghost boundary within the pick
/// radius — hitting the neighbour's agreed coordinates is what the ghost is
/// loaded for. Horizontal distance only: the map plane and the ghost's rails
/// need not share a height.
pub fn snap_ghost(p: EcefPos, ghost: &Ghost, focus: &Focus) -> EcefPos {
    let frame = EnuFrame::at(p);
    ghost
        .boundaries
        .iter()
        .map(|(_, b)| {
            let local = frame.to_local(*b);
            (*b, DVec2::new(local.x, local.y).length())
        })
        .filter(|(_, d)| *d <= pick_radius(focus))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(b, _)| b)
        .unwrap_or(p)
}

/// Support points of edge `i` — the segment boundaries, as world positions.
///
/// ponytail: empty for edges with transition curves (`dk ≠ 0`) — the
/// arc-to-point refit would flatten them; re-fitting clothoids around a moved
/// point is an alignment-aware pass of its own.
pub fn support_points(line: &Line, i: usize) -> Vec<EcefPos> {
    let Some(edge) = line.net.edges().get(i) else {
        return Vec::new();
    };
    if edge.segments.iter().any(|g| g.dk != 0.0) {
        return Vec::new();
    }
    let mut s = 0.0;
    let mut points = vec![edge.eval(0.0).pos];
    for segment in &edge.segments {
        s += segment.len;
        points.push(edge.eval(s).pos);
    }
    points
}

/// Index of the first draggable support point: the start is only free on a
/// geo-anchored edge — a `Continue` start belongs to the previous edge.
pub fn first_draggable(source: &LineSource, edge: usize) -> usize {
    match source.edges.get(edge).map(|e| e.start) {
        Some(EdgeStart::Geo { .. }) => 0,
        _ => 1,
    }
}

/// Handle under the cursor: index into the selected edge's support points.
fn pick_support_point(line: &Line, edge: usize, pick: &ScreenPick) -> Option<usize> {
    support_points(line, edge)
        .iter()
        .enumerate()
        .skip(first_draggable(&line.source, edge))
        .filter_map(|(k, q)| Some((k, pick.hits(*q)?)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(k, _)| k)
}

/// Re-fits a support-point chain: one tangent-continuous arc or straight per
/// target, like the draw tool. `None` when a target falls behind the heading.
fn refit_chain(heading0: f64, targets: &[DVec2]) -> Option<Vec<Segment>> {
    let mut position = DVec2::ZERO;
    let mut heading = heading0;
    let mut segments = Vec::with_capacity(targets.len());
    for target in targets {
        let (segment, next) = segment_to(position, heading, *target)?;
        segments.push(segment);
        position = *target;
        heading = next;
    }
    Some(segments)
}

/// Moves support point `point` of `edge` to `p` and refits the whole chain
/// arc-to-point through the unchanged points — exactly what redrawing the
/// edge through the same clicks would produce. A target the chain cannot
/// reach freezes the drag instead of bending the track somewhere else.
fn drag_support_point(line: &mut Line, edge: usize, point: usize, p: EcefPos) {
    let mut points = support_points(line, edge);
    if point >= points.len() {
        return;
    }
    points[point] = p;
    let heading0 = line.net.edges()[edge].heading0;
    // The frame moves with the anchor; over the few km of one edge the frames
    // are parallel enough (same tolerance the draw tool works in).
    let frame = EnuFrame::at(points[0]);
    let targets: Vec<DVec2> = points[1..]
        .iter()
        .map(|q| {
            let local = frame.to_local(*q);
            DVec2::new(local.x, local.y)
        })
        .collect();
    let Some(segments) = refit_chain(heading0, &targets) else {
        return;
    };
    let Some(source) = line.source.edges.get_mut(edge) else {
        return;
    };
    source.segments = segments;
    if point == 0
        && let EdgeStart::Geo {
            point: geo_point, ..
        } = &mut source.start
    {
        let (lat, lon, _) = geo::from_ecef(p);
        geo_point.lat = lat.to_degrees();
        geo_point.lon = lon.to_degrees();
        // Height stays — the map plane knows nothing about the railhead.
    }
}

/// Removes whatever is selected — the Delete key and the Edit menu share it.
pub fn delete_selection(line: &mut Line, state: &mut EditorState) {
    match std::mem::take(&mut state.selection) {
        Selection::Edge(i) => line.source.remove_edge(i),
        Selection::Device(i) => line.source.remove_device(i),
        // Nothing references objects, trees or forests by index — a plain
        // remove suffices.
        Selection::Object(i) => {
            if i < line.source.objects.len() {
                line.source.objects.remove(i);
            }
        }
        Selection::Tree(i) => {
            if i < line.source.trees.len() {
                line.source.trees.remove(i);
            }
        }
        Selection::Marker(i) => {
            if i < line.source.markers.len() {
                line.source.markers.remove(i);
            }
        }
        Selection::TerrainEdit(i) => {
            if i < line.source.terrain.len() {
                line.source.terrain.remove(i);
            }
        }
        Selection::None => {}
    }
}

/// Deletes a whole marker layer — one undo step, like the marking brush.
pub fn delete_layer(line: &mut Line, state: &mut EditorState, layer: &str) {
    line.source.markers.retain(|m| m.layer != layer);
    state.hidden_layers.remove(layer);
    if let Selection::Marker(_) = state.selection {
        state.selection = Selection::None;
    }
}

/// Deletes everything the marking brush swept over — one undo step.
pub fn delete_marked(line: &mut Line, state: &mut EditorState) {
    let mut trees: Vec<usize> = Vec::new();
    let mut objects: Vec<usize> = Vec::new();
    for mark in state.marked.drain(..) {
        match mark {
            Mark::Tree(i) => trees.push(i),
            Mark::Object(i) => objects.push(i),
        }
    }
    // Descending order, so earlier removals do not shift later indices.
    for list in [&mut trees, &mut objects] {
        list.sort_unstable();
        list.dedup();
    }
    for i in trees.into_iter().rev() {
        if i < line.source.trees.len() {
            line.source.trees.remove(i);
        }
    }
    for i in objects.into_iter().rev() {
        if i < line.source.objects.len() {
            line.source.objects.remove(i);
        }
    }
    // Tree/object indices shifted under the selection.
    state.selection = Selection::None;
}

/// Marks every tree and object within `radius` of `p` — the brush sweep.
fn mark_within(state: &mut EditorState, line: &Line, focus: &Focus, p: EcefPos, radius: f64) {
    for (i, tree) in line.source.trees.iter().enumerate() {
        if tree_pos(tree, focus).distance(p) <= radius && !state.marked.contains(&Mark::Tree(i)) {
            state.marked.push(Mark::Tree(i));
        }
    }
    for (i, object) in line.source.objects.iter().enumerate() {
        let near = object_pos(&line.net, object).is_some_and(|q| q.distance(p) <= radius);
        if near && !state.marked.contains(&Mark::Object(i)) {
            state.marked.push(Mark::Object(i));
        }
    }
}

/// Bakes the forest brush polygon into single trees — Enter and right-click
/// share it. Fewer than three corners are reported, not saved. Every baked
/// tree is an ordinary [`TreeSource`], so a wood from the brush is edited tree
/// by tree like everything else.
pub fn finish_forest(
    line: &mut Line,
    state: &mut EditorState,
    overlay: &mut crate::overlay::Overlay,
) {
    let points = std::mem::take(&mut state.forest_points);
    if points.len() < 3 {
        overlay.status = t!("status-forest-points");
        return;
    }
    let polygon: Vec<(f64, f64)> = points
        .iter()
        .map(|p| {
            let (lat, lon, _) = geo::from_ecef(*p);
            (lat.to_degrees(), lon.to_degrees())
        })
        .collect();
    let objects: Vec<String> = state.tree_object.iter().cloned().collect();
    let trees = content::terrain::fill_polygon(
        &polygon,
        &objects,
        state.forest_area.unwrap_or(500.0),
        line.source.trees.len() as u64,
        utm_zone_of(polygon[0].1),
        |lat, lon| clear_of_track(&line.net, lat, lon),
    );
    overlay.status = t!("status-forest-baked", count = trees.len());
    line.source.trees.extend(trees);
    state.selection = Selection::None;
}

/// UTM zone containing the longitude [deg] — the fill samples in that grid.
pub fn utm_zone_of(lon: f64) -> u8 {
    (((lon + 180.0) / 6.0).floor() as i32).clamp(0, 59) as u8 + 1
}

/// Keeps baked trees off the track strip the terrain flattens.
pub fn clear_of_track(net: &TrackNetwork, lat: f64, lon: f64) -> bool {
    let p = geo::to_ecef_deg(lat, lon, 0.0);
    // `nearest_on_network` measures in 3D; a probe at ellipsoid height sits far
    // below the rails, so the vertical part alone would pass the clearance.
    // Compare horizontally instead: against the nearest track point's lat/lon.
    match nearest_on_network(net, p) {
        Some((edge, s, _)) => {
            let pose = net.edges()[edge].eval(s);
            let (tlat, tlon, _) = geo::from_ecef(pose.pos);
            let track = geo::to_ecef_deg(tlat.to_degrees(), tlon.to_degrees(), 0.0);
            track.distance(p) > content::terrain::TREE_TRACK_CLEARANCE
        }
        None => true,
    }
}

/// Map position of a tree: its geo position lifted onto the focus plane — the
/// terrain height only exists in the app, and the editor looks straight down.
pub fn tree_pos(tree: &TreeSource, focus: &Focus) -> EcefPos {
    let (_, _, height) = geo::from_ecef(focus.position);
    geo::to_ecef_deg(tree.lat, tree.lon, height)
}

/// The same for a reference marker.
pub fn marker_pos(marker: &MarkerSource, focus: &Focus) -> EcefPos {
    let (_, _, height) = geo::from_ecef(focus.position);
    geo::to_ecef_deg(marker.lat, marker.lon, height)
}

/// The same for a terrain brush stroke.
pub fn terrain_pos(edit: &TerrainEditSource, focus: &Focus) -> EcefPos {
    let (_, _, height) = geo::from_ecef(focus.position);
    geo::to_ecef_deg(edit.lat, edit.lon, height)
}

/// The terrain tiles of this line's corridor — what a full height import
/// covers, and the grid the tile picker works on.
pub fn corridor_tiles(line: &Line, options: content::TerrainOptions) -> Vec<content::TileKey> {
    content::TerrainBuilder::new(&line.net, Vec::new(), options).corridor_keys()
}

/// Tile the map point `p` falls into.
pub fn tile_of(p: EcefPos, options: content::TerrainOptions) -> content::TileKey {
    content::terrain::tile_at(content::terrain::to_utm(p, &options), &options)
}

/// The four corners of a tile on the focus plane — for drawing the grid.
fn tile_corners(
    k: content::TileKey,
    options: content::TerrainOptions,
    focus: &Focus,
) -> [EcefPos; 5] {
    let min = content::terrain::tile_min(k, options.tile_size);
    let (_, _, height) = geo::from_ecef(focus.position);
    let corner = |dx: f64, dy: f64| {
        let (lat, lon) = geo::from_utm(min.x + dx, min.y + dy, options.zone);
        geo::to_ecef(lat, lon, height)
    };
    let size = options.tile_size;
    [
        corner(0.0, 0.0),
        corner(size, 0.0),
        corner(size, size),
        corner(0.0, size),
        corner(0.0, 0.0),
    ]
}

/// The marker layers present, each with how many markers it holds. Sorted, so
/// the panel does not reshuffle from frame to frame.
pub fn marker_layers(line: &Line) -> std::collections::BTreeMap<String, usize> {
    let mut layers = std::collections::BTreeMap::new();
    for marker in &line.source.markers {
        *layers.entry(marker.layer.clone()).or_insert(0) += 1;
    }
    layers
}

/// Turns the finished drawing into track: a free drawing becomes two buffer
/// nodes and one edge; a branch drawing (switch tool) splits its base edge and
/// wires the joint into a turnout whose diverging leg is the drawing. `false`
/// only when the split failed — the drawing is gone either way.
pub fn finish_drawing(line: &mut Line, state: &mut EditorState) -> bool {
    let Some(drawing) = state.drawing.take() else {
        return true;
    };
    let (Some(heading_deg), false) = (drawing.heading_deg, drawing.segments.is_empty()) else {
        return true;
    };
    if let Some((base, s)) = drawing.branch_of {
        let Some((joint, straight)) = line.source.split_edge(base, s) else {
            return false;
        };
        let buffer = line.source.nodes.len() as u32;
        line.source.nodes.push(NodeSource::Buffer);
        let branch = line.source.edges.len();
        let trailing = drawing.trailing;
        line.source.edges.push(EdgeSource {
            from: joint,
            to: buffer,
            // Facing: Continue = end pose of the first half = the cut,
            // tangentially. Trailing: the branch runs the other way, and a
            // `Continue` can only ever mean "onwards" — the cut's own
            // coordinates with the reversed heading say the same thing.
            start: if trailing {
                EdgeStart::Geo {
                    point: drawing.start,
                    heading_deg,
                }
            } else {
                EdgeStart::Continue { edge: base as u32 }
            },
            segments: drawing.segments,
            grade: vec![],
            cant: vec![],
            speed: vec![],
            track_type: vec![],
        });
        // Facing: a train reaches the fork over the first half, so that end is
        // the root. Trailing: it comes from the far side — the second half is
        // the root and the first half becomes the straight leg, which is
        // exactly what makes a move over the base track a trailing one.
        line.source.nodes[joint as usize] = if trailing {
            NodeSource::Switch {
                root: (straight, false),
                straight: (base as u32, true),
                diverging: (branch as u32, false),
                throw_time: DEFAULT_THROW_TIME,
            }
        } else {
            NodeSource::Switch {
                root: (base as u32, true),
                straight: (straight, false),
                diverging: (branch as u32, false),
                throw_time: DEFAULT_THROW_TIME,
            }
        };
        state.selection = Selection::Edge(branch);
        return true;
    }
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
        track_type: vec![],
    });
    state.selection = Selection::Edge(line.source.edges.len() - 1);
    true
}

/// Mouse and keyboard input of the tools.
#[allow(clippy::too_many_arguments)]
pub fn tool_input(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    camera: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    origin: Res<Origin>,
    focus: Res<Focus>,
    ghost: Res<Ghost>,
    objects: Res<TrackObjects>,
    gizmo: Res<crate::gizmo::GizmoState>,
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
        Selection::Object(i) if i >= line.source.objects.len() => {
            state.selection = Selection::None;
        }
        Selection::Tree(i) if i >= line.source.trees.len() => {
            state.selection = Selection::None;
        }
        _ => {}
    }
    // Stale marks likewise — one out-of-range index and the sweep is void.
    let stale = state.marked.iter().any(|m| match m {
        Mark::Tree(i) => *i >= line.source.trees.len(),
        Mark::Object(i) => *i >= line.source.objects.len(),
    });
    if stale {
        state.marked.clear();
    }
    // A gizmo handle owns the mouse while it is dragged — a click that is
    // moving a signal must not also reselect what lies under it.
    if state.typing || gizmo.is_dragging() {
        return;
    }

    let cursor = windows
        .single()
        .ok()
        .and_then(|w| w.cursor_position())
        .filter(|c| state.viewport.contains(*c));
    let view = cursor.zip(camera.single().ok());
    // Ground point under the cursor, while it is over the free viewport.
    let picked = view.and_then(|(c, (camera, camera_transform))| {
        pick_ground(camera, camera_transform, c, &origin.0, &focus)
    });
    // …and the same cursor as a screen-space probe, for selecting.
    let pick = view.map(|(cursor, (camera, transform))| ScreenPick {
        camera,
        transform,
        origin: &origin.0,
        cursor,
    });

    // An active support-point drag owns the mouse until the button goes up.
    if let Some((edge, point)) = state.drag {
        if !buttons.pressed(MouseButton::Left) {
            state.drag = None;
        } else if let Some(p) = picked {
            drag_support_point(&mut line, edge, point, snap_ghost(p, &ghost, &focus));
        }
        return;
    }

    if keys.just_pressed(KeyCode::Escape) {
        if !state.forest_points.is_empty() {
            state.forest_points.clear();
        } else if !state.marked.is_empty() {
            state.marked.clear();
        } else if state.drawing.take().is_none() {
            state.selection = Selection::None;
        }
    }
    if keys.just_pressed(KeyCode::Delete) {
        if state.marked.is_empty() {
            delete_selection(&mut line, &mut state);
        } else {
            delete_marked(&mut line, &mut state);
        }
    }
    if keys.just_pressed(KeyCode::Enter) || buttons.just_pressed(MouseButton::Right) {
        if !state.forest_points.is_empty() {
            finish_forest(&mut line, &mut state, &mut overlay);
        } else if !finish_drawing(&mut line, &mut state) {
            overlay.status = t!("status-split-failed");
        }
    }
    // Tool switching from the keyboard, as every map editor has it.
    for (key, tool) in [
        (KeyCode::Digit1, Tool::Select),
        (KeyCode::Digit2, Tool::DrawTrack),
        (KeyCode::Digit3, Tool::PlaceDevice),
        (KeyCode::Digit4, Tool::PlaceSwitch),
        (KeyCode::Digit5, Tool::PlaceObject),
        (KeyCode::Digit6, Tool::PlaceTree),
        (KeyCode::Digit7, Tool::PlaceForest),
        (KeyCode::Digit8, Tool::Brush),
        (KeyCode::Digit9, Tool::PlaceMarker),
        (KeyCode::Digit0, Tool::TerrainBrush),
    ] {
        if keys.just_pressed(key) && state.tool != tool {
            state.tool = tool;
            state.drawing = None;
            state.forest_points.clear();
        }
    }

    // The marking brush sweeps while the button is held — every frame, not
    // only on the press, like the support-point drag above.
    if state.tool == Tool::Brush {
        if buttons.pressed(MouseButton::Left)
            && let Some(p) = picked
        {
            state.map_used = true;
            let radius = state.brush_radius.unwrap_or(30.0);
            mark_within(&mut state, &line, &focus, p, radius);
        }
        return;
    }

    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(p) = picked else {
        return;
    };
    state.map_used = true;

    match state.tool {
        Tool::DrawTrack => {
            let p = snap_ghost(p, &ghost, &focus);
            match &mut state.drawing {
                None => state.drawing = Some(Drawing::start_at(p, line.source.geoid_offset)),
                Some(drawing) => drawing.click(p),
            }
        }
        Tool::PlaceSwitch => {
            let trailing = state.switch_trailing;
            match &mut state.drawing {
                Some(drawing) => drawing.click(snap_ghost(p, &ghost, &focus)),
                None => match nearest_on_network(&line.net, p) {
                    Some((edge, s, distance)) if distance <= pick_radius(&focus) => {
                        let length = line.net.edges()[edge].length();
                        if s < 1.0 || s > length - 1.0 {
                            overlay.status = t!("status-split-at-end");
                        } else {
                            let pose = line.net.edges()[edge].eval(s);
                            state.drawing = Some(Drawing::branch_at(
                                &pose,
                                line.source.geoid_offset,
                                edge,
                                s,
                                trailing,
                            ));
                        }
                    }
                    _ => overlay.status = t!("status-no-track-hit"),
                },
            }
        }
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
        Tool::PlaceObject => {
            // The chosen object, or the first installed one — its defaults
            // are what "placed relative to the track" means.
            let name = state
                .object
                .clone()
                .or_else(|| objects.map.keys().next().cloned());
            let Some(name) = name else {
                overlay.status = t!("status-no-objects");
                return;
            };
            state.object = Some(name.clone());
            match nearest_on_network(&line.net, p) {
                Some((edge, s, distance)) if distance <= pick_radius(&focus) => {
                    let spec = objects.map.get(&name);
                    line.source.objects.push(ObjectSource {
                        object: name,
                        edge: edge as u32,
                        s,
                        lateral_offset: spec.map_or(0.0, |o| o.lateral_offset),
                        yaw_deg: spec.map_or(0.0, |o| o.yaw_deg),
                        height: spec.map_or(0.0, |o| o.height),
                        snap_to_terrain: false,
                    });
                    state.selection = Selection::Object(line.source.objects.len() - 1);
                }
                _ => overlay.status = t!("status-no-track-hit"),
            }
        }
        Tool::PlaceTree => {
            let (lat, lon, _) = geo::from_ecef(p);
            line.source.trees.push(TreeSource {
                object: state.tree_object.clone().unwrap_or_default(),
                lat: lat.to_degrees(),
                lon: lon.to_degrees(),
                yaw_deg: 0.0,
                scale: 1.0,
            });
            state.selection = Selection::Tree(line.source.trees.len() - 1);
        }
        Tool::PlaceForest => {
            state.forest_points.push(p);
        }
        Tool::PickTile => {
            let key = tile_of(p, state.terrain_options());
            match state.picked_tiles.iter().position(|k| *k == key) {
                Some(i) => {
                    state.picked_tiles.remove(i);
                }
                None => state.picked_tiles.push(key),
            }
        }
        Tool::TerrainBrush => {
            let (lat, lon, _) = geo::from_ecef(p);
            let edit = if state.terrain_level {
                // Level to the nearest rail — that is what levelling means on a
                // railway, and the editor knows the rail height without a DGM.
                match nearest_on_network(&line.net, p) {
                    Some((edge, s, _)) => {
                        let (_, _, height) = geo::from_ecef(line.net.edges()[edge].eval(s).pos);
                        TerrainEdit::Level(height)
                    }
                    None => {
                        overlay.status = t!("status-no-track-hit");
                        return;
                    }
                }
            } else {
                TerrainEdit::Raise(state.terrain_amount.unwrap_or(2.0))
            };
            line.source.terrain.push(TerrainEditSource {
                lat: lat.to_degrees(),
                lon: lon.to_degrees(),
                radius: state.terrain_radius.unwrap_or(60.0),
                edit,
            });
            state.selection = Selection::TerrainEdit(line.source.terrain.len() - 1);
        }
        Tool::PlaceMarker => {
            let (lat, lon, _) = geo::from_ecef(p);
            let layer = state.marker_layer();
            // A marker in a hidden layer would vanish the moment it is set.
            state.hidden_layers.remove(&layer);
            line.source.markers.push(MarkerSource {
                layer,
                label: state.marker_label.clone(),
                lat: lat.to_degrees(),
                lon: lon.to_degrees(),
            });
            state.selection = Selection::Marker(line.source.markers.len() - 1);
        }
        Tool::Select => {
            let Some(pick) = pick.as_ref() else {
                return;
            };
            // A handle of the selected edge outranks reselection.
            if let Selection::Edge(i) = state.selection
                && let Some(k) = pick_support_point(&line, i, pick)
            {
                state.drag = Some((i, k));
                return;
            }
            // Nearest point candidate wins; equipment before furniture before
            // trees on a tie (the iteration order below).
            let device = line
                .source
                .devices
                .iter()
                .enumerate()
                .filter_map(|(i, d)| Some((Selection::Device(i), device_pos(&line.net, d)?)))
                .collect::<Vec<_>>();
            let objects_ = line
                .source
                .objects
                .iter()
                .enumerate()
                .filter_map(|(i, o)| Some((Selection::Object(i), object_pos(&line.net, o)?)))
                .collect::<Vec<_>>();
            let trees = line
                .source
                .trees
                .iter()
                .enumerate()
                .map(|(i, t)| (Selection::Tree(i), tree_pos(t, &focus)))
                .collect::<Vec<_>>();
            let terrain = line
                .source
                .terrain
                .iter()
                .enumerate()
                .map(|(i, e)| (Selection::TerrainEdit(i), terrain_pos(e, &focus)))
                .collect::<Vec<_>>();
            // Hidden layers are not pickable — out of sight, out of reach.
            let markers = line
                .source
                .markers
                .iter()
                .enumerate()
                .filter(|(_, m)| state.layer_visible(&m.layer))
                .map(|(i, m)| (Selection::Marker(i), marker_pos(m, &focus)))
                .collect::<Vec<_>>();
            let nearest = device
                .into_iter()
                .chain(objects_)
                .chain(trees)
                .chain(markers)
                .chain(terrain)
                .filter_map(|(sel, pos)| Some((sel, pick.hits(pos)?)))
                .min_by(|a, b| a.1.total_cmp(&b.1));
            // Point candidates first, the track last.
            state.selection = match nearest {
                Some((sel, _)) => sel,
                None => nearest_edge(&line, pick).map_or(Selection::None, Selection::Edge),
            };
        }
        // Handled above — the brush owns the whole press, not just the click.
        Tool::Brush => {}
    }
}

/// Circle gizmo lying flat on the ground at `p`.
fn ground_circle(
    gizmos: &mut Gizmos,
    origin: &RenderOrigin,
    p: EcefPos,
    radius: f32,
    color: Color,
) {
    let up = origin.dir_to_render(EnuFrame::at(p).up);
    let rotation = Quat::from_rotation_arc(Vec3::Z, up);
    gizmos.circle(
        Isometry3d::new(origin.to_render(p) + up, rotation),
        radius,
        color,
    );
}

/// Track ribbon of one edge as a line on the ground.
fn edge_line(gizmos: &mut Gizmos, origin: &RenderOrigin, edge: &track_model::TrackEdge, c: Color) {
    let steps = ((edge.length() / 10.0).ceil() as usize).max(2);
    let points = (0..=steps).map(|j| {
        let pose = edge.eval(edge.length() * j as f64 / steps as f64);
        origin.to_render(pose.pos) + origin.dir_to_render(pose.up)
    });
    gizmos.linestrip(points, c);
}

/// The section or route the interlocking panel points at, drawn where it
/// actually lies: the tracks of its sections in green, the overlap behind the
/// exit signal in orange, its switches and its two signals as circles. Index
/// lists are unreadable as a check — the map is the check.
fn draw_highlight(
    gizmos: &mut Gizmos,
    line: &Line,
    origin: &RenderOrigin,
    focus: &Focus,
    highlight: Highlight,
) {
    let green = Color::srgb(0.35, 0.80, 0.55);
    let orange = Color::srgb(0.95, 0.60, 0.25);
    let mut draw_section = |section: u32, color: Color| {
        let Some(section) = line.source.sections.get(section as usize) else {
            return;
        };
        for edge in &section.edges {
            if let Some(edge) = line.net.edges().get(*edge as usize) {
                edge_line(gizmos, origin, edge, color);
            }
        }
    };
    let route = match highlight {
        Highlight::Section(i) => {
            draw_section(i as u32, green);
            return;
        }
        Highlight::Route(i) => match line.source.routes.get(i) {
            Some(route) => route,
            None => return,
        },
    };
    for section in &route.sections {
        draw_section(*section, green);
    }
    for section in &route.overlap {
        draw_section(*section, orange);
    }
    let radius = (focus.height * 0.012).max(4.0) as f32;
    for (node, _) in &route.switches {
        if let Some(p) = node_pos(&line.source, &line.net, *node) {
            ground_circle(gizmos, origin, p, radius, green);
        }
    }
    let signal_pos = |signal: u32| {
        line.source
            .signals
            .get(signal as usize)
            .and_then(|s| line.source.devices.get(s.device as usize))
            .and_then(|d| device_pos(&line.net, d))
    };
    // Entry and exit signal — where the route begins and ends.
    for signal in [route.entry, route.exit] {
        if let Some(p) = signal_pos(signal) {
            ground_circle(gizmos, origin, p, radius * 1.5, green);
        }
    }
    // Flank protection in its own colour: it is not on the path, it is what
    // keeps the path free from the side.
    let violet = Color::srgb(0.72, 0.45, 0.92);
    for guard in &route.flank {
        let position = match guard {
            FlankSource::Switch(node, _) => node_pos(&line.source, &line.net, *node),
            FlankSource::Signal(signal) => signal_pos(*signal),
        };
        if let Some(p) = position {
            ground_circle(gizmos, origin, p, radius * 1.2, violet);
        }
    }
}

/// Selection highlight, support-point handles, boundaries and drawing preview.
#[allow(clippy::too_many_arguments)]
pub fn draw_gizmos(
    state: Res<EditorState>,
    line: Res<Line>,
    ghost: Res<Ghost>,
    origin: Res<Origin>,
    focus: Res<Focus>,
    windows: Query<&Window, With<PrimaryWindow>>,
    camera: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut gizmos: Gizmos,
) {
    let accent = Color::srgb(0.36, 0.61, 0.96);

    if let Some(highlight) = state.highlight {
        draw_highlight(&mut gizmos, &line, &origin.0, &focus, highlight);
    }

    // Boundary markers: own in warn yellow, the ghost module's in its grey.
    let boundary_radius = (focus.height * 0.015).max(5.0) as f32;
    for boundary in &line.source.boundaries {
        if let Some(p) = node_pos(&line.source, &line.net, boundary.node) {
            ground_circle(
                &mut gizmos,
                &origin.0,
                p,
                boundary_radius,
                Color::srgb(0.89, 0.71, 0.30),
            );
        }
    }
    for (_, p) in &ghost.boundaries {
        ground_circle(
            &mut gizmos,
            &origin.0,
            *p,
            boundary_radius,
            Color::srgb(0.55, 0.57, 0.62),
        );
    }

    match state.selection {
        Selection::Edge(i) => {
            if let Some(edge) = line.net.edges().get(i) {
                edge_line(&mut gizmos, &origin.0, edge, accent);
            }
            // Draggable support points as handles.
            if state.tool == Tool::Select {
                let handle_radius = (focus.height * 0.008).max(2.5) as f32;
                for p in support_points(&line, i)
                    .iter()
                    .skip(first_draggable(&line.source, i))
                {
                    ground_circle(&mut gizmos, &origin.0, *p, handle_radius, accent);
                }
            }
        }
        Selection::Device(i) => {
            if let Some(device) = line.source.devices.get(i)
                && let Some(p) = device_pos(&line.net, device)
            {
                let radius = (focus.height * 0.012).max(4.0) as f32;
                ground_circle(&mut gizmos, &origin.0, p, radius, accent);
            }
        }
        Selection::Object(i) => {
            if let Some(object) = line.source.objects.get(i)
                && let Some(p) = object_pos(&line.net, object)
            {
                let radius = (focus.height * 0.012).max(4.0) as f32;
                ground_circle(&mut gizmos, &origin.0, p, radius, accent);
            }
        }
        // Trees, markers and terrain strokes are drawn below.
        Selection::Tree(_) | Selection::Marker(_) | Selection::TerrainEdit(_) => {}
        Selection::None => {}
    }

    // Terrain strokes as their true footprint: the circle is the radius the
    // stroke actually reaches, so overlapping ones show where the ground is
    // worked twice. Raising warm, lowering cold, levelling neutral.
    for (i, edit) in line.source.terrain.iter().enumerate() {
        let p = terrain_pos(edit, &focus);
        let color = match edit.edit {
            content::route::TerrainEdit::Raise(by) if by >= 0.0 => Color::srgb(0.90, 0.55, 0.30),
            content::route::TerrainEdit::Raise(_) => Color::srgb(0.35, 0.60, 0.90),
            content::route::TerrainEdit::Level(_) => Color::srgb(0.65, 0.65, 0.70),
        };
        let color = if state.selection == Selection::TerrainEdit(i) {
            accent
        } else {
            color
        };
        ground_circle(&mut gizmos, &origin.0, p, edit.radius as f32, color);
        // The centre, so a stroke stays findable when its radius fills the view.
        ground_circle(
            &mut gizmos,
            &origin.0,
            p,
            (edit.radius * 0.06) as f32,
            color,
        );
    }

    // Reference markers: a diamond each, so they read differently from the
    // round device circles and the tree crosses. Hidden layers draw nothing.
    let marker_color = Color::srgb(0.85, 0.75, 0.35);
    let size = (focus.height * 0.006).max(2.5) as f32;
    for (i, marker) in line.source.markers.iter().enumerate() {
        if !state.layer_visible(&marker.layer) {
            continue;
        }
        let p = marker_pos(marker, &focus);
        if state.selection == Selection::Marker(i) {
            ground_circle(
                &mut gizmos,
                &origin.0,
                p,
                (focus.height * 0.012).max(4.0) as f32,
                accent,
            );
        }
        let up = origin.0.dir_to_render(EnuFrame::at(p).up);
        let center = origin.0.to_render(p) + up;
        let (x, z) = (Vec3::X * size, Vec3::Z * size);
        gizmos.linestrip(
            [center + z, center + x, center - z, center - x, center + z],
            marker_color,
        );
    }

    // Trees on the map: a small cross each (a circle per tree would be tens of
    // thousands of gizmo segments over a baked wood), marked ones in orange,
    // the selected one as an accent circle. Beyond this height a tree is
    // sub-pixel — the dots would only be clutter.
    let vegetation = Color::srgb(0.42, 0.62, 0.35);
    let marked_color = Color::srgb(0.95, 0.55, 0.25);
    if focus.height < 2500.0 {
        let arm = (focus.height * 0.004).max(1.5) as f32;
        for (i, tree) in line.source.trees.iter().enumerate() {
            let p = tree_pos(tree, &focus);
            if state.selection == Selection::Tree(i) {
                let radius = (focus.height * 0.012).max(4.0) as f32;
                ground_circle(&mut gizmos, &origin.0, p, radius, accent);
                continue;
            }
            let color = if state.marked.contains(&Mark::Tree(i)) {
                marked_color
            } else {
                vegetation
            };
            let up = origin.0.dir_to_render(EnuFrame::at(p).up);
            let center = origin.0.to_render(p) + up;
            gizmos.line(center - Vec3::X * arm, center + Vec3::X * arm, color);
            gizmos.line(center - Vec3::Z * arm, center + Vec3::Z * arm, color);
        }
    }
    // Marked objects wear the same orange as marked trees.
    for mark in &state.marked {
        if let Mark::Object(i) = mark
            && let Some(object) = line.source.objects.get(*i)
            && let Some(p) = object_pos(&line.net, object)
        {
            let radius = (focus.height * 0.010).max(3.0) as f32;
            ground_circle(&mut gizmos, &origin.0, p, radius, marked_color);
        }
    }

    let cursor = windows
        .single()
        .ok()
        .and_then(|w| w.cursor_position())
        .filter(|c| state.viewport.contains(*c))
        .and_then(|c| {
            let (camera, camera_transform) = camera.single().ok()?;
            pick_ground(camera, camera_transform, c, &origin.0, &focus)
        });
    if let Some(drawing) = &state.drawing {
        gizmos.linestrip(drawing.polyline(cursor, &origin.0), accent);
    }
    // Forest brush preview: the ring so far, the cursor as the next corner.
    if !state.forest_points.is_empty() {
        let points = state.forest_points.iter().copied().chain(cursor).map(|p| {
            let up = origin.0.dir_to_render(EnuFrame::at(p).up);
            origin.0.to_render(p) + up
        });
        gizmos.linestrip(points, accent);
    }
    // Marking brush footprint under the cursor.
    if state.tool == Tool::Brush
        && let Some(p) = cursor
    {
        let radius = state.brush_radius.unwrap_or(30.0) as f32;
        ground_circle(&mut gizmos, &origin.0, p, radius, marked_color);
    }
    // The same for the terrain brush — its footprint is the stroke to come.
    if state.tool == Tool::TerrainBrush
        && let Some(p) = cursor
    {
        let radius = state.terrain_radius.unwrap_or(60.0) as f32;
        ground_circle(&mut gizmos, &origin.0, p, radius, accent);
    }

    // The DGM tile grid, only while the tile picker is in hand: green where the
    // module already has heights, accent where a tile is picked, faint where
    // neither. That is the whole status display the import needs.
    if state.tool == Tool::PickTile {
        let options = state.terrain_options();
        let have = Color::srgb(0.35, 0.80, 0.55);
        let missing = Color::srgb(0.45, 0.47, 0.52);
        for key in corridor_tiles(&line, options) {
            let color = if state.picked_tiles.contains(&key) {
                accent
            } else if state.dgm_present.contains(&key) {
                have
            } else {
                missing
            };
            let corners = tile_corners(key, options, &focus).map(|p| {
                let up = origin.0.dir_to_render(EnuFrame::at(p).up);
                origin.0.to_render(p) + up
            });
            gizmos.linestrip(corners, color);
        }
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
            objects: vec![],
            trees: vec![],
            markers: vec![],
            terrain: vec![],
            heights: vec![],
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
            issues: Vec::new(),
        };
        assert!(finish_drawing(&mut doc, &mut state));
        line = doc.source;
        let compiled = line.compile().expect("compiles");
        let end = compiled.net.edges()[0].end_pose().pos;
        assert!(
            end.distance(bend) < 0.5,
            "end missed the click by {} m",
            end.distance(bend)
        );
    }

    /// The switch tool splits the clicked edge and wires the drawn branch as
    /// the diverging leg — the result compiles and forks at the cut.
    #[test]
    fn a_finished_branch_becomes_a_turnout() {
        let source = content::musterbahn();
        let net = source.compile().unwrap().net;
        let mut doc = Line {
            source,
            net,
            path: None,
            dirty: false,
            needs_rebuild: false,
            recenter: false,
            issues: Vec::new(),
        };

        // Branch off edge 0 at km 1.5, curving away to the left.
        let pose = doc.net.edges()[0].eval(1500.0);
        let mut drawing = Drawing::branch_at(&pose, doc.source.geoid_offset, 0, 1500.0, false);
        let frame = EnuFrame::at(pose.pos);
        let tangent = frame.dir_to_local(pose.tangent);
        let left = DVec3::new(-tangent.y, tangent.x, 0.0);
        drawing.click(frame.to_ecef(tangent * 400.0 + left * 60.0));
        assert_eq!(drawing.segments.len(), 1);
        let mut state = EditorState {
            drawing: Some(drawing),
            ..Default::default()
        };
        assert!(finish_drawing(&mut doc, &mut state));

        let compiled = doc.source.compile().expect("turnout compiles");
        // Split into first half, curve, climb, second half, branch.
        assert_eq!(doc.source.edges.len(), 5);
        let switch = doc
            .source
            .nodes
            .iter()
            .find_map(|n| match n {
                NodeSource::Switch {
                    root,
                    straight,
                    diverging,
                    ..
                } => Some((*root, *straight, *diverging)),
                _ => None,
            })
            .expect("a switch node exists");
        assert_eq!(switch, ((0, true), (3, false), (4, false)));
        // Both legs leave the cut tangentially.
        let cut = compiled.net.edges()[0].end_pose();
        for leg in [3, 4] {
            let start = compiled.net.edges()[leg].eval(0.0);
            assert!(start.pos.distance(cut.pos) < 0.01, "leg {leg} detached");
            assert!(
                start.tangent.dot(cut.tangent) > 0.999_999,
                "leg {leg} kinks"
            );
        }
    }

    /// A trailing connection: the branch runs back along the clicked track, so
    /// the far half of the split becomes the root and a move over the base
    /// track is a trailing one. Geometry still meets at the cut.
    #[test]
    fn a_trailing_branch_puts_the_root_on_the_far_half() {
        let source = content::musterbahn();
        let net = source.compile().unwrap().net;
        let mut doc = Line {
            source,
            net,
            path: None,
            dirty: false,
            needs_rebuild: false,
            recenter: false,
            issues: Vec::new(),
        };

        let pose = doc.net.edges()[0].eval(1500.0);
        let mut drawing = Drawing::branch_at(&pose, doc.source.geoid_offset, 0, 1500.0, true);
        let frame = EnuFrame::at(pose.pos);
        let tangent = frame.dir_to_local(pose.tangent);
        let left = DVec3::new(-tangent.y, tangent.x, 0.0);
        // Backwards along the track, curving away — the driver of edge 0 has
        // the fork behind them.
        drawing.click(frame.to_ecef(-tangent * 400.0 + left * 60.0));
        let mut state = EditorState {
            drawing: Some(drawing),
            ..Default::default()
        };
        assert!(finish_drawing(&mut doc, &mut state));

        let compiled = doc.source.compile().expect("turnout compiles");
        let switch = doc
            .source
            .nodes
            .iter()
            .find_map(|n| match n {
                NodeSource::Switch {
                    root,
                    straight,
                    diverging,
                    ..
                } => Some((*root, *straight, *diverging)),
                _ => None,
            })
            .expect("a switch node exists");
        // Root = start of the second half, straight = end of the first.
        assert_eq!(switch, ((3, false), (0, true), (4, false)));

        let cut = compiled.net.edges()[0].end_pose();
        let branch = compiled.net.edges()[4].eval(0.0);
        assert!(
            branch.pos.distance(cut.pos) < 0.05,
            "branch detached by {} m",
            branch.pos.distance(cut.pos)
        );
        assert!(
            branch.tangent.dot(cut.tangent) < -0.999,
            "the branch has to leave against the track"
        );
    }

    /// The repeat function stamps a row of copies along the edge, carrying
    /// the instance's own offset and rotation, and stops where it was told.
    #[test]
    fn repeating_an_object_stamps_a_row() {
        let mut source = content::musterbahn();
        source.objects.push(ObjectSource {
            object: "ex:mast".into(),
            edge: 0,
            s: 100.0,
            lateral_offset: -3.5,
            yaw_deg: 15.0,
            height: 0.5,
            snap_to_terrain: false,
        });
        let net = source.compile().unwrap().net;
        let mut doc = Line {
            source,
            net,
            path: None,
            dirty: false,
            needs_rebuild: false,
            recenter: false,
            issues: Vec::new(),
        };

        let placed = repeat_object(&mut doc, 0, 65.0, 500.0);
        assert_eq!(placed, 6, "165, 230, 295, 360, 425, 490");
        assert_eq!(doc.source.objects.len(), 7);
        assert!((doc.source.objects[1].s - 165.0).abs() < 1e-9);
        let last = doc.source.objects.last().unwrap();
        assert!((last.s - 490.0).abs() < 1e-9);
        assert_eq!(last.lateral_offset, -3.5, "the instance's own offset");
        assert_eq!(last.yaw_deg, 15.0);
        assert_eq!(last.edge, 0);
        doc.source.compile().expect("still compiles");

        // The row is clamped to the edge (3000 m), and a degenerate spacing
        // places nothing instead of looping forever.
        assert_eq!(repeat_object(&mut doc, 0, 1000.0, 99_999.0), 2);
        assert_eq!(repeat_object(&mut doc, 0, 0.0, 500.0), 0);
    }

    /// Dragging a support point moves exactly that point; the refit chain
    /// still passes through the untouched ones.
    #[test]
    fn dragging_a_support_point_refits_the_chain() {
        let source = content::musterbahn();
        let net = source.compile().unwrap().net;
        let mut doc = Line {
            source,
            net,
            path: None,
            dirty: false,
            needs_rebuild: false,
            recenter: false,
            issues: Vec::new(),
        };

        // Edge 2 (straight, geo after a re-anchor? — it is `Continue`, so its
        // interior/end points drag, its start does not).
        assert_eq!(first_draggable(&doc.source, 2), 1);
        let points = support_points(&doc, 2);
        assert_eq!(points.len(), 2, "one straight segment, two support points");

        // Pull the end 200 m to the left of the old tangent.
        let end = doc.net.edges()[2].end_pose();
        let frame = EnuFrame::at(end.pos);
        let tangent = frame.dir_to_local(end.tangent);
        let target = frame.to_ecef(DVec3::new(-tangent.y, tangent.x, 0.0) * 200.0);
        drag_support_point(&mut doc, 2, 1, target);

        let compiled = doc.source.compile().expect("still compiles");
        let moved = compiled.net.edges()[2].end_pose().pos;
        assert!(
            moved.distance(target) < 1.0,
            "end missed the drag target by {} m",
            moved.distance(target)
        );
        // The start stayed where the curve hands over.
        let start = compiled.net.edges()[2].eval(0.0).pos;
        let handover = compiled.net.edges()[1].end_pose().pos;
        assert!(start.distance(handover) < 0.01);
    }
}
