//! Footpaths and walk areas: the ways and places the people walk on, and the
//! two tools that draw and reshape them (plan ch. 12, MODS.md *People*).
//!
//! Both are polylines over the ground — a path open, an area closed — held in
//! [`LineSource::walk_paths`] and [`LineSource::walk_areas`] as latitude and
//! longitude only. The ground under each vertex gives it its height
//! (`terrain::Marks`), plus what the walkway itself adds: a footbridge, a
//! modelled platform. Drawing works like the forest brush — clicks collect
//! vertices, Enter or a right click finishes the way — and reshaping works
//! like the envelope tool: a click on a vertex picks it up, a click on a side
//! of the selected walkway puts a vertex there, Delete takes the picked one
//! out. The geometry of all that is the envelope's ([`crate::envelope`]).

use crate::envelope::{self, LatLon};
use crate::terrain::Marks;
use crate::tools::{self, EditorState, MARK_LIFT, Selection, Tool};
use crate::ui::row;
use crate::{Focus, Line};
use bevy::prelude::*;
use bevy_egui::egui;
use content::LineSource;
use content::route::{WalkAreaSource, WalkPathSource, WalkPoint};
use editor_ui::space;
use glam::DVec3;
use i18n::t;
use world_coords::{EcefPos, EnuFrame, RenderOrigin, geo};

/// Colour of the walkways — a teal of their own, so a way is never mistaken
/// for the envelope's warn yellow or the track's blue.
const COLOR: Color = Color::srgb(0.30, 0.78, 0.72);
/// The same, dimmed: while neither walkway tool is up, the ways are context.
const COLOR_IDLE: Color = Color::srgba(0.30, 0.78, 0.72, 0.40);
/// The accent every selection on this map wears.
const ACCENT: Color = Color::srgb(0.36, 0.61, 0.96);

/// What a walkway drawn here starts with — the file format's own defaults
/// (`content::route`, pinned by a test), so a way drawn in the editor and one
/// typed into the file read the same.
pub const DEFAULT_WIDTH: f64 = 2.0;
pub const DEFAULT_PATH_PEOPLE: u32 = 4;
pub const DEFAULT_AREA_PEOPLE: u32 = 6;
pub const DEFAULT_WALKING_SHARE: f64 = 0.5;

/// The two kinds of walkway, and what tells them apart: a path is open and
/// needs two vertices, an area is a ring and needs three.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Path,
    Area,
}

impl Kind {
    /// The kind `tool` draws and reshapes, if it is a walkway tool.
    pub fn of_tool(tool: Tool) -> Option<Self> {
        match tool {
            Tool::PlaceWalkPath => Some(Self::Path),
            Tool::PlaceWalkArea => Some(Self::Area),
            _ => None,
        }
    }

    /// The walkway `selection` holds, if it holds one.
    pub fn of_selection(selection: Selection) -> Option<(Self, usize)> {
        match selection {
            Selection::WalkPath(i) => Some((Self::Path, i)),
            Selection::WalkArea(i) => Some((Self::Area, i)),
            _ => None,
        }
    }

    pub fn selection(self, index: usize) -> Selection {
        match self {
            Self::Path => Selection::WalkPath(index),
            Self::Area => Selection::WalkArea(index),
        }
    }

    /// An area closes back on its first corner; a path ends at its last.
    pub fn closed(self) -> bool {
        self == Self::Area
    }

    /// Fewest vertices a walkway of this kind can have — a start and an end,
    /// or a triangle — which is the minimum the rule check enforces, too.
    pub fn min_vertices(self) -> usize {
        match self {
            Self::Path => 2,
            Self::Area => 3,
        }
    }

    /// How many walkways of this kind the line has.
    pub fn count(self, source: &LineSource) -> usize {
        match self {
            Self::Path => source.walk_paths.len(),
            Self::Area => source.walk_areas.len(),
        }
    }
}

/// The vertices of walkway `index` of `kind` — one accessor for both lists.
pub fn vertices(source: &LineSource, kind: Kind, index: usize) -> Option<&[WalkPoint]> {
    match kind {
        Kind::Path => source.walk_paths.get(index).map(|p| p.points.as_slice()),
        Kind::Area => source.walk_areas.get(index).map(|a| a.polygon.as_slice()),
    }
}

fn vertices_mut(source: &mut LineSource, kind: Kind, index: usize) -> Option<&mut Vec<WalkPoint>> {
    match kind {
        Kind::Path => source.walk_paths.get_mut(index).map(|p| &mut p.points),
        Kind::Area => source.walk_areas.get_mut(index).map(|a| &mut a.polygon),
    }
}

/// What the walkway itself adds to the ground under it [m].
fn lift(source: &LineSource, kind: Kind, index: usize) -> f64 {
    match kind {
        Kind::Path => source.walk_paths.get(index).map_or(0.0, |p| p.height),
        Kind::Area => source.walk_areas.get(index).map_or(0.0, |a| a.height),
    }
}

/// Where vertex `vertex` of walkway `index` stands: on the ground under it,
/// plus the walkway's own height.
pub fn vertex_pos(
    line: &Line,
    marks: &Marks,
    kind: Kind,
    index: usize,
    vertex: usize,
) -> Option<EcefPos> {
    let point = vertices(&line.source, kind, index)?.get(vertex)?;
    let height = marks.walk_height(kind, index, vertex) + lift(&line.source, kind, index);
    Some(geo::to_ecef_deg(point.lat, point.lon, height))
}

/// All vertices of walkway `index`, positioned like [`vertex_pos`].
pub fn positions(line: &Line, marks: &Marks, kind: Kind, index: usize) -> Vec<EcefPos> {
    let count = vertices(&line.source, kind, index).map_or(0, |v| v.len());
    (0..count)
        .filter_map(|k| vertex_pos(line, marks, kind, index, k))
        .collect()
}

/// What a click with a walkway tool lands on, in the order the tool tries
/// them: a vertex of any walkway of the kind outranks every side, or a vertex
/// could never be picked up again once placed; a side of the selected walkway
/// takes a new vertex; a side of any other walkway selects it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Hit {
    Vertex { index: usize, vertex: usize },
    Side { index: usize, side: usize, t: f64 },
    Body { index: usize },
}

/// Finds what the click hits. `project` maps a map position into the space
/// the click is measured in — the screen, for the tool (see
/// [`crate::tools::ScreenPick`]), so a vertex at the horizon is as grabbable
/// as one under the camera — and `radius` is the reach in that space.
pub fn pick(
    line: &Line,
    marks: &Marks,
    kind: Kind,
    selected: Option<usize>,
    project: impl Fn(EcefPos) -> Option<DVec3>,
    click: DVec3,
    radius: f64,
) -> Option<Hit> {
    let projected: Vec<Vec<Option<DVec3>>> = (0..kind.count(&line.source))
        .map(|index| {
            positions(line, marks, kind, index)
                .into_iter()
                .map(&project)
                .collect()
        })
        .collect();
    let vertex = projected
        .iter()
        .enumerate()
        .filter_map(|(index, points)| {
            let on_screen = points
                .iter()
                .enumerate()
                .filter_map(|(k, p)| Some((k, (*p)?)));
            let (vertex, distance) = envelope::nearest_vertex(on_screen, click, radius)?;
            Some((index, vertex, distance))
        })
        .min_by(|a, b| a.2.total_cmp(&b.2));
    if let Some((index, vertex, _)) = vertex {
        return Some(Hit::Vertex { index, vertex });
    }
    // A side with an end off screen is skipped, not the whole way.
    let side_of = |index: usize| {
        let sides = envelope::sides(&projected[index], kind.closed())
            .filter_map(|(i, a, b)| Some((i, a?, b?)));
        envelope::nearest_side(sides, click, radius)
    };
    if let Some(index) = selected.filter(|i| *i < projected.len())
        && let Some((side, t, _)) = side_of(index)
    {
        return Some(Hit::Side { index, side, t });
    }
    (0..projected.len())
        .filter(|index| Some(*index) != selected)
        .filter_map(|index| Some((index, side_of(index)?.2)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(index, _)| Hit::Body { index })
}

/// Puts a vertex on side `side` of walkway `index`, `t` of the way along it,
/// and answers with the new vertex's index.
pub fn insert_vertex(
    line: &mut Line,
    kind: Kind,
    index: usize,
    side: usize,
    t: f64,
) -> Option<usize> {
    let points = vertices_mut(&mut line.source, kind, index)?;
    if side >= points.len() {
        return None;
    }
    let point = envelope::point_on_side(points, side, t);
    let vertex = side + 1;
    points.insert(vertex, point);
    Some(vertex)
}

/// Moves a vertex to the position under the cursor.
pub fn drag_vertex(line: &mut Line, kind: Kind, index: usize, vertex: usize, p: EcefPos) {
    if let Some(point) =
        vertices_mut(&mut line.source, kind, index).and_then(|points| points.get_mut(vertex))
    {
        envelope::move_to(point, p);
    }
}

/// Takes vertex `vertex` out of walkway `index` — unless that would leave
/// fewer than the kind needs, in which case nothing happens and `false` says
/// so. The caller then removes the whole walkway: a path of one point is no
/// path, and there is nothing left worth keeping.
pub fn remove_vertex(line: &mut Line, kind: Kind, index: usize, vertex: usize) -> bool {
    let Some(points) = vertices_mut(&mut line.source, kind, index) else {
        return false;
    };
    if points.len() <= kind.min_vertices() || vertex >= points.len() {
        return false;
    }
    points.remove(vertex);
    true
}

/// Removes walkway `index` altogether. Nothing references a walkway by index
/// — the people are a function of the line — so a plain remove suffices.
pub fn remove(line: &mut Line, kind: Kind, index: usize) {
    match kind {
        Kind::Path if index < line.source.walk_paths.len() => {
            line.source.walk_paths.remove(index);
        }
        Kind::Area if index < line.source.walk_areas.len() => {
            line.source.walk_areas.remove(index);
        }
        _ => {}
    }
}

/// A vertex where the click landed — the height is dropped, the ground
/// answers for it.
fn vertex_at(p: EcefPos) -> WalkPoint {
    let (lat, lon, _) = geo::from_ecef(p);
    WalkPoint::at(lat.to_degrees(), lon.to_degrees())
}

/// Finishes the walkway being drawn — Enter and right-click share it. Too few
/// vertices are reported (the status comes back for the bar) and the drawing
/// goes on: the next click adds the missing one, which is what the message
/// asks for. The new walkway takes the tool's options and becomes the
/// selection, so its fields are on screen at once.
pub fn finish(line: &mut Line, state: &mut EditorState) -> Option<String> {
    let Some(kind) = Kind::of_tool(state.tool) else {
        // The tool changed under the drawing; `select_tool` clears it, this
        // is the belt to those braces.
        state.walk_points.clear();
        return None;
    };
    if state.walk_points.len() < kind.min_vertices() {
        return Some(match kind {
            Kind::Path => t!("status-walk-path-points"),
            Kind::Area => t!("status-walk-area-points"),
        });
    }
    let points: Vec<WalkPoint> = state.walk_points.drain(..).map(vertex_at).collect();
    let height = state.walk_height.unwrap_or(0.0);
    let index = match kind {
        Kind::Path => {
            line.source.walk_paths.push(WalkPathSource {
                name: String::new(),
                points,
                width: state.walk_width.unwrap_or(DEFAULT_WIDTH),
                people: state.walk_path_people.unwrap_or(DEFAULT_PATH_PEOPLE),
                height,
                tags: Vec::new(),
            });
            line.source.walk_paths.len() - 1
        }
        Kind::Area => {
            line.source.walk_areas.push(WalkAreaSource {
                name: String::new(),
                polygon: points,
                people: state.walk_area_people.unwrap_or(DEFAULT_AREA_PEOPLE),
                walking_share: state.walk_share.unwrap_or(DEFAULT_WALKING_SHARE),
                height,
                tags: Vec::new(),
            });
            line.source.walk_areas.len() - 1
        }
    };
    state.selection = kind.selection(index);
    state.walk_vertex = None;
    None
}

/// The line through `world`, lifted off the ground like every mark on this
/// map so it does not sink into the draped imagery; `closed` runs it back to
/// the first vertex.
fn polyline(
    gizmos: &mut Gizmos,
    origin: &RenderOrigin,
    world: &[EcefPos],
    closed: bool,
    color: Color,
) {
    let lifted =
        |p: &EcefPos| origin.to_render(*p) + origin.dir_to_render(EnuFrame::at(*p).up) * MARK_LIFT;
    let back = closed.then(|| world.first()).flatten();
    gizmos.linestrip(world.iter().chain(back).map(lifted), color);
}

/// Draws every walkway: teal lines on the ground, paths open and areas
/// closed, dimmed while neither walkway tool is up; the selected one in the
/// accent. Vertex handles only for the kind the tool in hand reshapes — every
/// other tool sees the ways, but is not invited to drag them. The way being
/// drawn is the vertices so far plus the cursor as the next one, like the
/// forest brush's ring.
pub fn draw(
    gizmos: &mut Gizmos,
    line: &Line,
    origin: &RenderOrigin,
    focus: &Focus,
    marks: &Marks,
    state: &EditorState,
    cursor: Option<EcefPos>,
) {
    let active = Kind::of_tool(state.tool);
    // Handles scale with the view, like every other grab point on this map.
    let handle = (focus.height * 0.006).max(2.0) as f32;
    for kind in [Kind::Path, Kind::Area] {
        for index in 0..kind.count(&line.source) {
            let world = positions(line, marks, kind, index);
            let selected = state.selection == kind.selection(index);
            let color = if selected {
                ACCENT
            } else if active.is_some() {
                COLOR
            } else {
                COLOR_IDLE
            };
            polyline(gizmos, origin, &world, kind.closed(), color);
            if active != Some(kind) {
                continue;
            }
            for (k, p) in world.iter().enumerate() {
                let picked = selected && state.walk_vertex == Some(k);
                tools::ground_circle(
                    gizmos,
                    origin,
                    *p,
                    if picked { handle * 1.4 } else { handle },
                    if picked { ACCENT } else { color },
                );
            }
        }
    }
    if let Some(kind) = active
        && !state.walk_points.is_empty()
    {
        let points: Vec<EcefPos> = state.walk_points.iter().copied().chain(cursor).collect();
        // Closed only once there is a ring to close: two corners and the cursor.
        polyline(
            gizmos,
            origin,
            &points,
            kind.closed() && points.len() >= 3,
            ACCENT,
        );
        for p in &state.walk_points {
            tools::ground_circle(gizmos, origin, *p, handle, ACCENT);
        }
    }
}

/// A comma-separated tag list in one text field. Split as typed, empties and
/// all — dropping a trailing comma while it is being typed would make the
/// second tag impossible to start — and tidied when the field is left.
fn tags_field(ui: &mut egui::Ui, tags: &mut Vec<String>) {
    let mut text = tags.join(", ");
    let response = ui.add(egui::TextEdit::singleline(&mut text).desired_width(space::FIELD));
    if response.changed() {
        *tags = text.split(',').map(|tag| tag.trim().to_string()).collect();
    }
    if response.lost_focus() {
        tags.retain(|tag| !tag.is_empty());
    }
}

fn people_field(ui: &mut egui::Ui, people: &mut u32) -> egui::Response {
    editor_ui::field(ui, people, 1.0, 0.0..=200.0, "")
}

fn height_field(ui: &mut egui::Ui, height: &mut f64) -> egui::Response {
    editor_ui::field(ui, height, 0.1, -5.0..=50.0, "m")
}

fn width_field(ui: &mut egui::Ui, width: &mut f64) -> egui::Response {
    editor_ui::field(ui, width, 0.1, 0.5..=20.0, "m")
}

fn share_field(ui: &mut egui::Ui, share: &mut f64) -> egui::Response {
    editor_ui::field(ui, share, 0.05, 0.0..=1.0, "")
}

/// The Tool section's options: what the next drawn walkway is given — the
/// World Editor's panel for the piece about to be placed, never for one
/// already lying. Below them the count of what lies, and the way in progress.
pub fn tool_rows(ui: &mut egui::Ui, line: &Line, state: &mut EditorState, kind: Kind) {
    editor_ui::form_grid("place-walkway").show(ui, |ui| {
        match kind {
            Kind::Path => {
                row(ui, "walk-width", |ui| {
                    let mut width = state.walk_width.unwrap_or(DEFAULT_WIDTH);
                    if width_field(ui, &mut width).changed() {
                        state.walk_width = Some(width);
                    }
                });
                row(ui, "walk-people", |ui| {
                    let mut people = state.walk_path_people.unwrap_or(DEFAULT_PATH_PEOPLE);
                    if people_field(ui, &mut people).changed() {
                        state.walk_path_people = Some(people);
                    }
                });
            }
            Kind::Area => {
                row(ui, "walk-people", |ui| {
                    let mut people = state.walk_area_people.unwrap_or(DEFAULT_AREA_PEOPLE);
                    if people_field(ui, &mut people).changed() {
                        state.walk_area_people = Some(people);
                    }
                });
                row(ui, "walk-share", |ui| {
                    let mut share = state.walk_share.unwrap_or(DEFAULT_WALKING_SHARE);
                    if share_field(ui, &mut share).changed() {
                        state.walk_share = Some(share);
                    }
                });
            }
        }
        row(ui, "walk-height", |ui| {
            let mut height = state.walk_height.unwrap_or(0.0);
            if height_field(ui, &mut height).changed() {
                state.walk_height = Some(height);
            }
        });
    });
    ui.small(t!(
        "walk-count",
        paths = line.source.walk_paths.len(),
        areas = line.source.walk_areas.len()
    ));
    if !state.walk_points.is_empty() {
        ui.add_space(space::XS);
        ui.small(match kind {
            Kind::Path => t!("walk-path-active", points = state.walk_points.len()),
            Kind::Area => t!("walk-area-active", corners = state.walk_points.len()),
        });
    }
}

/// The selection panel of a walkway: its name, the figures the people take
/// from it, the height it stands above the ground and its tags — and the
/// picked vertex's coordinates, which is where a vertex is placed exactly.
/// The delete button takes the whole way; a single vertex goes with the key.
pub fn rows(
    ui: &mut egui::Ui,
    line: &mut Line,
    state: &mut EditorState,
    marks: &Marks,
    focus: &mut Focus,
    kind: Kind,
    index: usize,
) {
    let Some(count) = vertices(&line.source, kind, index).map(|v| v.len()) else {
        return;
    };
    let picked = state.walk_vertex.filter(|k| *k < count);
    // Where the centre button goes: the picked vertex, else the first.
    let position = vertex_pos(line, marks, kind, index, picked.unwrap_or(0));
    match kind {
        Kind::Path => ui.label(t!("sel-walk-path-summary", index = index, points = count)),
        Kind::Area => ui.label(t!("sel-walk-area-summary", index = index, corners = count)),
    };
    editor_ui::form_grid("sel-walkway").show(ui, |ui| match kind {
        Kind::Path => {
            let path = &mut line.source.walk_paths[index];
            row(ui, "walk-name", |ui| {
                ui.add(egui::TextEdit::singleline(&mut path.name).desired_width(space::FIELD));
            });
            row(ui, "walk-width", |ui| {
                width_field(ui, &mut path.width);
            });
            row(ui, "walk-people", |ui| {
                people_field(ui, &mut path.people);
            });
            row(ui, "walk-height", |ui| {
                height_field(ui, &mut path.height);
            });
            row(ui, "walk-tags", |ui| tags_field(ui, &mut path.tags));
        }
        Kind::Area => {
            let area = &mut line.source.walk_areas[index];
            row(ui, "walk-name", |ui| {
                ui.add(egui::TextEdit::singleline(&mut area.name).desired_width(space::FIELD));
            });
            row(ui, "walk-people", |ui| {
                people_field(ui, &mut area.people);
            });
            row(ui, "walk-share", |ui| {
                share_field(ui, &mut area.walking_share);
            });
            row(ui, "walk-height", |ui| {
                height_field(ui, &mut area.height);
            });
            row(ui, "walk-tags", |ui| tags_field(ui, &mut area.tags));
        }
    });
    if let Some(vertex) = picked
        && let Some(point) =
            vertices_mut(&mut line.source, kind, index).and_then(|points| points.get_mut(vertex))
    {
        ui.add_space(space::XS);
        ui.small(t!("sel-walk-vertex", index = vertex + 1, count = count));
        editor_ui::form_grid("sel-walk-vertex").show(ui, |ui| {
            row(ui, "new-module-lat", |ui| {
                editor_ui::field(ui, &mut point.lat, 0.0001, -85.0..=85.0, "°");
            });
            row(ui, "new-module-lon", |ui| {
                editor_ui::field(ui, &mut point.lon, 0.0001, -180.0..=180.0, "°");
            });
        });
    }
    ui.add_space(space::XS);
    ui.horizontal(|ui| {
        if ui.button(t!("action-center")).clicked()
            && let Some(p) = position
        {
            focus.position = p;
        }
        if ui.button(t!("action-delete")).clicked() {
            state.walk_vertex = None;
            tools::delete_selection(line, state);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line_with(walk_paths: Vec<WalkPathSource>, walk_areas: Vec<WalkAreaSource>) -> Line {
        Line {
            source: LineSource {
                walk_paths,
                walk_areas,
                ..Default::default()
            },
            net: Default::default(),
            path: None,
            dirty: false,
            needs_rebuild: false,
            terrain_change: Default::default(),
            recenter: false,
            issues: Vec::new(),
        }
    }

    fn points(of: &[(f64, f64)]) -> Vec<WalkPoint> {
        of.iter()
            .map(|(lat, lon)| WalkPoint::at(*lat, *lon))
            .collect()
    }

    fn path(of: &[(f64, f64)]) -> WalkPathSource {
        WalkPathSource {
            name: String::new(),
            points: points(of),
            width: DEFAULT_WIDTH,
            people: DEFAULT_PATH_PEOPLE,
            height: 0.0,
            tags: Vec::new(),
        }
    }

    fn area(of: &[(f64, f64)]) -> WalkAreaSource {
        WalkAreaSource {
            name: String::new(),
            polygon: points(of),
            people: DEFAULT_AREA_PEOPLE,
            walking_share: DEFAULT_WALKING_SHARE,
            height: 0.0,
            tags: Vec::new(),
        }
    }

    /// Metres on the map plane around the bench — what the screen projection
    /// is to the tool, without a camera.
    fn local(p: EcefPos) -> Option<DVec3> {
        let frame = EnuFrame::at(geo::to_ecef_deg(52.0, 10.0, 0.0));
        let l = frame.to_local(p);
        Some(DVec3::new(l.x, l.y, 0.0))
    }

    fn at(lat: f64, lon: f64) -> DVec3 {
        local(geo::to_ecef_deg(lat, lon, 0.0)).unwrap()
    }

    #[test]
    fn a_vertex_is_picked_before_a_side_and_a_side_selects_or_adds() {
        // A path east along the parallel, an area north of it.
        let line = line_with(
            vec![path(&[(52.0, 10.0), (52.0, 10.002)])],
            vec![area(&[(52.001, 10.0), (52.001, 10.002), (52.002, 10.001)])],
        );
        let marks = Marks::default();
        let reach = 5.0;
        // Two metres beside the far vertex: the vertex, not the side it ends.
        let near_end = at(52.0, 10.002) + DVec3::new(2.0, 0.0, 0.0);
        assert_eq!(
            pick(&line, &marks, Kind::Path, None, local, near_end, reach),
            Some(Hit::Vertex {
                index: 0,
                vertex: 1
            })
        );
        // Halfway along, off the vertices: the side. Of another way it
        // selects, of the selected way it takes a vertex at the hit.
        let middle = at(52.0, 10.001) + DVec3::new(0.0, 2.0, 0.0);
        assert_eq!(
            pick(&line, &marks, Kind::Path, None, local, middle, reach),
            Some(Hit::Body { index: 0 })
        );
        match pick(&line, &marks, Kind::Path, Some(0), local, middle, reach) {
            Some(Hit::Side {
                index: 0,
                side: 0,
                t,
            }) => assert!((t - 0.5).abs() < 0.01, "{t}"),
            other => panic!("{other:?}"),
        }
        // Out of reach: nothing — the click starts a new way.
        let far = middle + DVec3::new(0.0, 20.0, 0.0);
        assert_eq!(
            pick(&line, &marks, Kind::Path, Some(0), local, far, reach),
            None
        );
        // The kind is filtered: the path tool never picks the area.
        let on_area = at(52.001, 10.001);
        assert_eq!(
            pick(&line, &marks, Kind::Path, None, local, on_area, reach),
            None
        );
        // The area's closing side, from the last corner back to the first,
        // is a side too — that is what makes it a ring.
        let closing = at(52.0015, 10.0005);
        match pick(&line, &marks, Kind::Area, Some(0), local, closing, reach) {
            Some(Hit::Side {
                index: 0,
                side: 2,
                t,
            }) => assert!((t - 0.5).abs() < 0.02, "{t}"),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            pick(
                &line,
                &marks,
                Kind::Area,
                None,
                local,
                at(52.002, 10.001),
                reach
            ),
            Some(Hit::Vertex {
                index: 0,
                vertex: 2
            })
        );
    }

    #[test]
    fn a_vertex_goes_in_on_a_side_and_comes_out_down_to_the_minimum() {
        let mut line = line_with(
            vec![path(&[(52.0, 10.0), (52.0, 10.002)])],
            vec![area(&[(52.001, 10.0), (52.001, 10.002), (52.002, 10.001)])],
        );
        assert_eq!(insert_vertex(&mut line, Kind::Path, 0, 0, 0.5), Some(1));
        let points = &line.source.walk_paths[0].points;
        assert_eq!(points.len(), 3);
        assert!((points[1].lon - 10.001).abs() < 1e-12);
        // No side 5 on a three-vertex path.
        assert_eq!(insert_vertex(&mut line, Kind::Path, 0, 5, 0.5), None);
        // Dragging moves the vertex on the map; the height plays no part.
        drag_vertex(
            &mut line,
            Kind::Path,
            0,
            1,
            geo::to_ecef_deg(52.0005, 10.001, 123.0),
        );
        let moved = line.source.walk_paths[0].points[1];
        assert!((moved.lat - 52.0005).abs() < 1e-9);
        assert!((moved.lon - 10.001).abs() < 1e-9);
        // Down to two, and no further.
        assert!(remove_vertex(&mut line, Kind::Path, 0, 1));
        assert!(!remove_vertex(&mut line, Kind::Path, 0, 0));
        assert_eq!(line.source.walk_paths[0].points.len(), 2);
        // An area keeps its triangle; the closing side takes a corner at the end.
        assert!(!remove_vertex(&mut line, Kind::Area, 0, 0));
        assert_eq!(insert_vertex(&mut line, Kind::Area, 0, 2, 0.5), Some(3));
        assert_eq!(line.source.walk_areas[0].polygon.len(), 4);
        assert!(remove_vertex(&mut line, Kind::Area, 0, 3));
    }

    #[test]
    fn delete_takes_the_picked_vertex_or_the_whole_way() {
        let mut line = line_with(
            vec![path(&[(52.0, 10.0), (52.0, 10.001), (52.0, 10.002)])],
            vec![area(&[(52.001, 10.0), (52.001, 10.002), (52.002, 10.001)])],
        );
        let mut state = EditorState {
            selection: Selection::WalkPath(0),
            walk_vertex: Some(2),
            ..Default::default()
        };
        tools::delete_selection(&mut line, &mut state);
        assert_eq!(line.source.walk_paths[0].points.len(), 2);
        // The way stays selected for the next vertex; none is held.
        assert_eq!(state.selection, Selection::WalkPath(0));
        assert_eq!(state.walk_vertex, None);
        // Below the minimum the whole path goes.
        state.walk_vertex = Some(0);
        tools::delete_selection(&mut line, &mut state);
        assert!(line.source.walk_paths.is_empty());
        assert_eq!(state.selection, Selection::None);
        // Without a picked vertex the whole area goes.
        state.selection = Selection::WalkArea(0);
        tools::delete_selection(&mut line, &mut state);
        assert!(line.source.walk_areas.is_empty());
    }

    #[test]
    fn finishing_needs_the_minimum_and_selects_the_new_way() {
        let mut line = line_with(Vec::new(), Vec::new());
        let mut state = EditorState {
            tool: Tool::PlaceWalkPath,
            walk_width: Some(3.0),
            ..Default::default()
        };
        state.walk_points.push(geo::to_ecef_deg(52.0, 10.0, 0.0));
        assert!(
            finish(&mut line, &mut state).is_some(),
            "one point is no path"
        );
        assert!(line.source.walk_paths.is_empty());
        assert_eq!(state.walk_points.len(), 1, "keeps collecting");
        state.walk_points.push(geo::to_ecef_deg(52.0, 10.001, 0.0));
        assert!(finish(&mut line, &mut state).is_none());
        assert!(state.walk_points.is_empty());
        assert_eq!(state.selection, Selection::WalkPath(0));
        let path = &line.source.walk_paths[0];
        assert_eq!(path.width, 3.0, "the tool's option");
        assert_eq!(path.people, DEFAULT_PATH_PEOPLE, "the file's default");
        assert!((path.points[1].lon - 10.001).abs() < 1e-9);

        state.tool = Tool::PlaceWalkArea;
        for lon in [10.0, 10.001] {
            state.walk_points.push(geo::to_ecef_deg(52.001, lon, 0.0));
        }
        assert!(
            finish(&mut line, &mut state).is_some(),
            "two corners are no area"
        );
        state
            .walk_points
            .push(geo::to_ecef_deg(52.002, 10.001, 0.0));
        assert!(finish(&mut line, &mut state).is_none());
        assert_eq!(state.selection, Selection::WalkArea(0));
        let area = &line.source.walk_areas[0];
        assert_eq!(area.polygon.len(), 3);
        assert_eq!(area.walking_share, DEFAULT_WALKING_SHARE);
    }

    /// The editor's defaults are the file format's — a way drawn here and one
    /// typed into a `.ron` read the same.
    #[test]
    fn the_defaults_are_the_file_formats() {
        let path: WalkPathSource = ron::from_str("(points: [])").unwrap();
        assert_eq!(path.width, DEFAULT_WIDTH);
        assert_eq!(path.people, DEFAULT_PATH_PEOPLE);
        let area: WalkAreaSource = ron::from_str("(polygon: [])").unwrap();
        assert_eq!(area.people, DEFAULT_AREA_PEOPLE);
        assert_eq!(area.walking_share, DEFAULT_WALKING_SHARE);
    }
}
