//! Editing tools of the route editor (plan ch. 15, editor v1: tracks + devices).
//!
//! A click is projected onto the map plane — the horizontal plane through the
//! focus point — which is where a tool places what it places. What is already
//! placed is drawn where it stands: a tree, a reference marker and a terrain
//! stroke carry latitude and longitude only, and the ground gives them their
//! height (`terrain::Marks`).
//!
//! The track tools follow the World Editor of Train Simulator Classic: a
//! **standing end** is set by pressing and dragging (the drag is the heading),
//! the **running end** follows the mouse as one tangent arc per click — a
//! straight while Ctrl is held — and snaps onto open track ends; a press on the
//! middle of a track starts a branch and becomes a turnout on finish. Beside the
//! lay tool sit the tools that work on laid track: split, join, offset,
//! crossover and gradient. Every arc is arc-to-point, so the drawn track is
//! G1-continuous by construction.

use crate::terrain::Marks;
use crate::{Focus, Ghost, Line, Origin, TrackObjects};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy::world_serialization::WorldAsset;
use content::LineSource;
use content::TerrainOptions;
use content::import::alignment::{CantRules, ramp_cant};
use content::route::{
    DeviceSource, EdgeSource, EdgeStart, FlankSource, GeoPoint, MarkerSource, NodeSource,
    ObjectSource, SignalSource, TerrainEdit, TerrainEditSource, TreeSource,
};
use glam::{DQuat, DVec2, DVec3};
use i18n::t;
use sim_core::interlock::{SignalKind, SignalSystem};
use track_model::{DeviceKind, Facing, Segment, TrackNetwork, TrackPose};
use world_coords::{EcefPos, EnuFrame, RenderOrigin, geo};

/// Throw time a freshly placed turnout gets [s] — the file format's own
/// default; the selection panel edits it per switch afterwards.
const DEFAULT_THROW_TIME: f64 = 6.0;

/// Active tool of the viewport.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Tool {
    #[default]
    Select,
    /// Lay track: press and drag sets the standing end, every click appends a
    /// piece, a press on a track starts a branch (see [`Drawing`]).
    DrawTrack,
    /// Cuts a track in two at the click — two tracks on one joint.
    Split,
    /// Welds two open ends: the first click picks one, the second the other.
    /// Ends that meet are joined as they are, ends apart get a connecting
    /// piece of two tangent arcs.
    Join,
    /// Lays a parallel track beside the clicked one, on the side of the click.
    Offset,
    /// A crossover between two parallel tracks: the first click cuts the one
    /// it leaves, the second names the one it reaches.
    Crossover,
    /// Puts a gradient break point on a track; the selection panel edits the
    /// gradient between the points.
    Gradient,
    PlaceDevice,
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
    /// One raising stroke per click — the ground climbs by the set amount.
    TerrainRaise,
    /// One lowering stroke per click — the same amount, downward.
    TerrainLower,
    /// Flattens to the ground height under the click — the plateau gesture.
    TerrainLevel,
    /// Pulls the ground to the height of the nearest rail.
    TerrainRail,
    /// DGM tiles: clicks pick single terrain tiles for the height import.
    PickTile,
    /// Marks a stretch of track: the first click sets one end, the second the other. The
    /// stretch joins the selected area, or opens a new one where none is selected.
    MarkArea,
    /// Reshapes the module envelope: drag a corner, click a side to add one,
    /// `Delete` removes the selected corner (see [`crate::envelope`]).
    EditEnvelope,
    /// Footpath: clicks set the vertices of a way people walk along,
    /// Enter/right-click finishes it; on a drawn way the envelope's gestures
    /// reshape it (see [`crate::walkways`]).
    PlaceWalkPath,
    /// Walk area: the same as a closed polygon people are about on.
    PlaceWalkArea,
    /// Field: clicks set the corners of a piece of farmland, Enter/right-click
    /// closes it. The import (see [`crate::fields`]) is the usual way to get
    /// fields; this is for the corner it did not cover, and for a fictional
    /// module where there is no register to ask.
    PlaceField,
}

/// What the Select tool holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Selection {
    #[default]
    None,
    Edge(usize),
    Device(usize),
    Object(usize),
    Tree(usize),
    Marker(usize),
    TerrainEdit(usize),
    /// A marked stretch of track with properties.
    TrackArea(usize),
    /// A corner of the module envelope.
    EnvelopePoint(usize),
    /// A footpath — the whole way; the vertex held, if any, is
    /// [`EditorState::walk_vertex`].
    WalkPath(usize),
    /// A walk area, likewise.
    WalkArea(usize),
    /// A field — the whole outline; the corner held, if any, is
    /// [`EditorState::walk_vertex`], which the fields share.
    Field(usize),
    /// A body of water — the whole polygon; picked by clicking its surface.
    Water(usize),
}

/// The stroke the area brush is painting: one stretch of one track, growing under the
/// cursor. It stays on the track it started on — a brush that jumped to the neighbouring
/// track halfway through a station would paint the wrong one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AreaStroke {
    pub edge: usize,
    pub from: f64,
    pub to: f64,
}

impl AreaStroke {
    pub fn span(self) -> content::route::AreaSpan {
        content::route::AreaSpan::new(self.edge as u32, self.from, self.to)
    }

    pub fn length(self) -> f64 {
        (self.to - self.from).abs()
    }
}

/// One item in the multi-selection — swept by the marking brush, caught by
/// the select tool's circle, or Ctrl-clicked one by one.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Tree(usize),
    Object(usize),
    Device(usize),
    Marker(usize),
}

/// The mark a picked thing joins the multi-selection as — the point-like
/// things only; a track or an area stays single selection.
fn as_mark(selection: Selection) -> Option<Mark> {
    match selection {
        Selection::Tree(i) => Some(Mark::Tree(i)),
        Selection::Object(i) => Some(Mark::Object(i)),
        Selection::Device(i) => Some(Mark::Device(i)),
        Selection::Marker(i) => Some(Mark::Marker(i)),
        _ => None,
    }
}

/// What the interlocking panel points at right now — the row under the mouse.
/// Sections and routes are lists of indices; on the map they are stretches of
/// track, and this is what puts the two together.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Highlight {
    Section(usize),
    Route(usize),
}

/// The number keys, counted along the tools of the **active category** — the
/// toolbox shows one category at a time, so `1` is always its first tool.
/// Ten keys are more than any category has.
const DIGITS: [KeyCode; 10] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
    KeyCode::Digit0,
];

/// The select tool's palette entry — it belongs to no category: picking
/// something is wanted whatever box is up, so the toolbox carries it above
/// every category's tools and the number key `1` is always it.
pub const SELECT_ENTRY: ToolEntry = (Tool::Select, "tool-select", editor_ui::Icon::Select);

/// The category `tool` belongs to — an index into [`TOOL_GROUPS`]. The
/// select tool belongs to all of them and answers with the first.
pub fn category_of(tool: Tool) -> usize {
    TOOL_GROUPS
        .iter()
        .position(|(_, _, tools)| tools.iter().any(|(t, _, _)| *t == tool))
        .unwrap_or(0)
}

impl Tool {
    /// The tool `--tool <name>` names — the i18n key without its prefix
    /// (`select`, `draw`, `split`, …), so the two can never drift apart.
    pub fn parse(name: &str) -> Option<Self> {
        std::iter::once(&SELECT_ENTRY)
            .chain(TOOL_GROUPS.iter().flat_map(|(_, _, tools)| tools.iter()))
            .find(|(_, key, _)| key.trim_start_matches("tool-") == name)
            .map(|(tool, _, _)| *tool)
    }
}

/// The palette entry of `tool` — its i18n key and icon.
pub fn tool_entry(tool: Tool) -> &'static ToolEntry {
    if tool == Tool::Select {
        return &SELECT_ENTRY;
    }
    TOOL_GROUPS[category_of(tool)]
        .2
        .iter()
        .find(|(t, _, _)| *t == tool)
        .expect("every tool sits in one group")
}

/// The digit that picks `tool` while its category is up, for its tooltip:
/// `1` is always the select tool, the category's own tools count from `2`.
pub fn tool_digit(tool: Tool) -> Option<u8> {
    if tool == Tool::Select {
        return Some(1);
    }
    TOOL_GROUPS[category_of(tool)]
        .2
        .iter()
        .position(|(t, _, _)| *t == tool)
        .filter(|index| index + 2 <= 9)
        .map(|index| (index + 2) as u8)
}

/// One toolbox entry: the tool, its i18n key and the icon on its button.
pub type ToolEntry = (Tool, &'static str, editor_ui::Icon);

/// The toolbox, after Train Simulator Classic's World Editor: an upper box of
/// categories — the track itself, what is mounted along it, the landscape it
/// runs through, the people about on it, the module itself — and a lower box
/// with the tools of the one that is up. Which category a tool belongs to is
/// the first thing a builder needs from a palette, and one category at a time
/// keeps the lower box short enough to be read at a glance.
pub const TOOL_GROUPS: [(&str, editor_ui::Icon, &[ToolEntry]); 6] = [
    (
        "tool-group-track",
        editor_ui::Icon::Track,
        &[
            (Tool::DrawTrack, "tool-draw", editor_ui::Icon::DrawTrack),
            (Tool::Split, "tool-split", editor_ui::Icon::Split),
            (Tool::Join, "tool-join", editor_ui::Icon::Join),
            (Tool::Offset, "tool-offset", editor_ui::Icon::Offset),
            (
                Tool::Crossover,
                "tool-crossover",
                editor_ui::Icon::Crossover,
            ),
            (Tool::Gradient, "tool-gradient", editor_ui::Icon::Gradient),
            (Tool::MarkArea, "tool-area", editor_ui::Icon::Area),
        ],
    ),
    (
        "tool-group-equipment",
        editor_ui::Icon::Device,
        &[
            (Tool::PlaceDevice, "tool-device", editor_ui::Icon::Device),
            (Tool::PlaceObject, "tool-object", editor_ui::Icon::Object),
            (Tool::PlaceMarker, "tool-marker", editor_ui::Icon::Marker),
        ],
    ),
    (
        "tool-group-vegetation",
        editor_ui::Icon::Forest,
        &[
            (Tool::PlaceTree, "tool-tree", editor_ui::Icon::Tree),
            (Tool::PlaceForest, "tool-forest", editor_ui::Icon::Forest),
            (Tool::PlaceField, "tool-field", editor_ui::Icon::Field),
            (Tool::Brush, "tool-brush", editor_ui::Icon::Brush),
        ],
    ),
    (
        "tool-group-terrain",
        editor_ui::Icon::Terrain,
        &[
            (
                Tool::TerrainRaise,
                "tool-terrain-raise",
                editor_ui::Icon::TerrainRaise,
            ),
            (
                Tool::TerrainLower,
                "tool-terrain-lower",
                editor_ui::Icon::TerrainLower,
            ),
            (
                Tool::TerrainLevel,
                "tool-terrain-level",
                editor_ui::Icon::TerrainLevel,
            ),
            (
                Tool::TerrainRail,
                "tool-terrain-rail",
                editor_ui::Icon::TerrainRail,
            ),
            (Tool::PickTile, "tool-tile", editor_ui::Icon::Tiles),
        ],
    ),
    (
        "tool-group-people",
        editor_ui::Icon::People,
        &[
            (
                Tool::PlaceWalkPath,
                "tool-walk-path",
                editor_ui::Icon::WalkPath,
            ),
            (
                Tool::PlaceWalkArea,
                "tool-walk-area",
                editor_ui::Icon::WalkArea,
            ),
        ],
    ),
    (
        "tool-group-module",
        editor_ui::Icon::Module,
        &[(
            Tool::EditEnvelope,
            "tool-envelope",
            editor_ui::Icon::Envelope,
        )],
    ),
];

/// What the next laid piece is given — Train Simulator Classic's track
/// properties panel, which applies to the piece about to be laid and never to
/// one already lying there. The lay, join and offset tools all read it.
#[derive(Clone, Debug)]
pub struct LayOptions {
    /// Track type (`"<mod>:<name>"`); `None` = the default type. The content
    /// drawer arms it, like a track picked from the browser.
    pub track_type: Option<String>,
    /// Permitted speed [km/h]; `None` = the line's default.
    pub speed: Option<f64>,
    /// Gradient of the piece [‰], positive uphill.
    pub grade: f64,
    /// Electrification id (`"ac-15kv"`, `"none"`, …); `None` = nothing said.
    pub electrification: Option<String>,
    /// Whether the piece carries a formation — ballast bed and the embankment
    /// or cutting the terrain builds under it. Off, the piece lays bare rails:
    /// for track on the builder's own constructions (bridges, platforms,
    /// ground they shaped themselves).
    pub formation: bool,
    /// How many tracks one lay puts down — yards are laid several at a time.
    pub parallel: u32,
    /// Centre distance of parallel tracks [m]; 4 m is the German main line.
    pub spacing: f64,
    /// Round a drawn arc to the standard radii of the alignment rulebook.
    pub snap_radius: bool,
    /// The laid piece follows the ground: sampled terrain heights become its
    /// grade profile, and a free start drops onto the surface.
    pub snap_terrain: bool,
    /// Lay curves as clothoid–arc–clothoid with the rulebook's cant, instead
    /// of bare arcs. Off by default: an eased edge carries transition curves
    /// and therefore offers no draggable support points.
    pub easements: bool,
    /// Radius of the turnouts a crossover is built from [m].
    pub turnout_radius: f64,
}

impl Default for LayOptions {
    fn default() -> Self {
        Self {
            track_type: None,
            speed: None,
            grade: 0.0,
            electrification: None,
            formation: true,
            parallel: 1,
            spacing: 4.0,
            snap_radius: false,
            snap_terrain: false,
            easements: false,
            turnout_radius: 190.0,
        }
    }
}

impl LayOptions {
    /// The step profiles a new edge starts with: one step at `s = 0` per
    /// property the options set, nothing for the ones they leave alone.
    fn profiles(&self) -> Profiles {
        Profiles {
            grade: if self.grade != 0.0 {
                vec![(0.0, self.grade)]
            } else {
                Vec::new()
            },
            speed: self.speed.map(|v| vec![(0.0, v)]).unwrap_or_default(),
            track_type: self
                .track_type
                .clone()
                .map(|t| vec![(0.0, t)])
                .unwrap_or_default(),
            electrification: self
                .electrification
                .clone()
                .map(|e| vec![(0.0, e)])
                .unwrap_or_default(),
            formation: self.formation,
        }
    }
}

impl LayOptions {
    /// The easement construction the lay tool works with while the option is
    /// on: the default cant rulebook at the piece's speed — the line's
    /// default speed where none is set, since cant is meaningless without one.
    pub fn easement_rules(&self) -> Option<Easements> {
        self.easements.then(|| Easements {
            rules: CantRules::default(),
            speed: self.speed.unwrap_or(content::route::DEFAULT_SPEED),
        })
    }
}

/// The easement construction of the lay tool: the cant rulebook, and the
/// speed the cant and its ramps are computed for.
#[derive(Clone, Copy, Debug)]
pub struct Easements {
    pub rules: CantRules,
    pub speed: f64,
}

/// Signed cant [mm] for curvature `k`: the rulebook amount, tipping into the
/// curve — positive rolls the track left (`TrackEdge::eval`), so a
/// right-hand curve carries the minus.
pub(crate) fn signed_cant(k: f64, e: Easements) -> f64 {
    if k.abs() < 1e-9 {
        return 0.0;
    }
    e.rules.applied(1.0 / k.abs(), e.speed) * k.signum()
}

/// The step profiles of an edge, as the source file carries them.
#[derive(Default)]
struct Profiles {
    grade: Vec<(f64, f64)>,
    speed: Vec<(f64, f64)>,
    track_type: Vec<(f64, String)>,
    electrification: Vec<(f64, String)>,
    formation: bool,
}

impl Profiles {
    fn edge(self, from: u32, to: u32, start: EdgeStart, segments: Vec<Segment>) -> EdgeSource {
        EdgeSource {
            from,
            to,
            start,
            segments,
            grade: self.grade,
            cant: vec![],
            speed: self.speed,
            track_type: self.track_type,
            electrification: self.electrification,
            formation: self.formation,
        }
    }
}

/// What the status bar reads out about the piece under the cursor while
/// laying: its length, its radius (`None` for a straight), the cant an eased
/// piece would carry, and whether Ctrl holds it straight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Readout {
    pub length: f64,
    pub radius: Option<f64>,
    /// Cant of the piece [mm], for one built with easements.
    pub cant: Option<f64>,
    pub straight: bool,
}

/// An open end of the track — a buffer node, seen from the edge that ends
/// there. What the lay tool continues from and snaps onto, and what the join
/// tool welds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OpenEnd {
    pub node: u32,
    pub edge: usize,
    /// The end is the edge's `to` side (`true`) or its `from` side.
    pub at_end: bool,
    pub pos: EcefPos,
    /// Outward math heading in the ENU frame at `pos` [rad], 0 = east.
    pub heading: f64,
    /// Curvature walking outward [1/m] — what an entry transition of a
    /// continuation starts from.
    pub curvature: f64,
}

/// Tool state, selection and what the UI pass leaves behind for the input
/// systems: the free viewport rect and whether a text field has focus.
#[derive(Resource, Default)]
pub struct EditorState {
    pub tool: Tool,
    /// The toolbox category that is up — held on its own rather than derived
    /// from the tool, so taking the select tool (which belongs to every
    /// category) leaves the box and the panel where they are.
    pub category: usize,
    pub selection: Selection,
    pub drawing: Option<Drawing>,
    /// Active support-point drag of the Select tool: `(edge, point index)`.
    pub drag: Option<(usize, usize)>,
    /// Corner of the module envelope being dragged.
    pub envelope_drag: Option<usize>,
    /// Edge length the envelope is reset to [km]; `None` = the default a new
    /// module starts with.
    pub envelope_size: Option<f64>,
    /// Kind the Place-device tool stamps.
    pub device_kind: Option<DeviceKind>,
    /// What the next laid piece is given (see [`LayOptions`]).
    pub lay: LayOptions,
    /// How the join tool stakes out its connections (see
    /// [`crate::stake::StakeOptions`]).
    pub stake: crate::stake::StakeOptions,
    /// Length and radius of the piece under the cursor while laying — the
    /// status bar reads it out.
    pub readout: Option<Readout>,
    /// Open ends of the track this frame — computed once for the lay and join
    /// tools, drawn as their snap targets.
    pub open_ends: Vec<OpenEnd>,
    /// The open end the join tool picked first, waiting for the second.
    pub join_from: Option<OpenEnd>,
    /// The track the crossover tool picked first, with the cut position.
    pub crossover_from: Option<(usize, f64)>,
    /// Where the right button went down over the map: a click finishes the
    /// drawing, a drag is the camera look — told apart on release.
    pub right_press: Option<Vec2>,
    /// Section or route the interlocking panel points at; the map draws it.
    /// Set by the panel every frame, so it follows the mouse by itself.
    pub highlight: Option<Highlight>,
    /// Overlap length the route derivation walks out behind the exit signal
    /// [m]; `None` = the regular length of the rulebook for the speed the
    /// route ends at (`content::route::regular_overlap`).
    pub overlap_length: Option<f64>,
    /// The content drawer: what the installed mods bring, over the map.
    pub drawer: crate::content_drawer::Drawer,
    /// The properties panel is folded away — the World Editor's flyouts give
    /// the map the whole window. Session state, and any jump into the panel
    /// unfolds it again.
    pub panel_hidden: bool,
    /// The select tool's last pick, for the double click: when it was made
    /// and what it hit — two picks of the same thing within the window send
    /// the panel to the properties.
    pub last_select: Option<(f64, Selection)>,
    /// Panel section to scroll to on the next frame — a row that belongs
    /// somewhere else (a signal's routes) sends the panel there.
    pub jump_to: Option<&'static str>,
    /// Object (`"<mod>:<name>"`) the Place-object tool stamps.
    pub object: Option<String>,
    /// The next placed object stands on the terrain instead of at the track's
    /// height — the toolbox toggle, stamped into the object at placement.
    pub place_snap_to_terrain: bool,
    /// Signal type (`"<mod>:<name>"`) a signal placed with the Place-device
    /// tool gets; `None` = the device stays a bare signal without a type.
    pub signal_type: Option<String>,
    /// Signal model (`"<mod>:<name>"`) that overrides the type's default on a
    /// signal placed from here.
    pub signal_model: Option<String>,
    /// Tree object the tree and forest tools use; `None` = placeholder tree.
    pub tree_object: Option<String>,
    /// Crop the field tool gives the next field it closes; `None` = winter
    /// cereal, the commonest crop in the country.
    pub field_crop: Option<fields::CropClass>,
    /// Corner points of the forest polygon being drawn.
    pub forest_points: Vec<EcefPos>,
    /// Forest brush density [m² per tree]; `None` = 500.
    pub forest_area: Option<f64>,
    /// Vertices of the walkway being drawn — footpath or walk area, the tool
    /// in hand says which (see [`crate::walkways`]).
    pub walk_points: Vec<EcefPos>,
    /// The vertex of the selected walkway that was last picked: what Delete
    /// takes out, and whose coordinates the panel shows.
    pub walk_vertex: Option<usize>,
    /// Vertex of the selected walkway being dragged.
    pub walk_drag: Option<usize>,
    /// What the next drawn footpath is given — width [m] and people; `None`
    /// = the file format's defaults.
    pub walk_width: Option<f64>,
    pub walk_path_people: Option<u32>,
    /// The same for the next walk area: people, and the share of them walking.
    pub walk_area_people: Option<u32>,
    pub walk_share: Option<f64>,
    /// Height of the next walkway above the ground [m]; `None` = 0.
    pub walk_height: Option<f64>,
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
    /// How far one terrain stroke moves the ground [m]; the tool in hand
    /// decides up or down. `None` = 2.
    pub terrain_amount: Option<f64>,
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
    /// Centre of the select tool's circle selection while the button grows
    /// it — a press on empty ground, released to mark everything inside.
    pub select_circle: Option<EcefPos>,
    /// Radius of the marking brush [m]; `None` = 30.
    pub brush_radius: Option<f64>,
    /// The stroke the area brush is painting right now. Held while the button is down and
    /// committed on release — a marking in progress, not saved state.
    pub area_stroke: Option<AreaStroke>,
    /// Half-width of the area brush stroke [m]; `None` = 2.5, a good deal wider than the
    /// track it is painted over so it reads as laid on top of it.
    pub area_width: Option<f64>,
    /// Repeat spacing of the object panel [m]; `None` = the 65 m of a
    /// standard catenary span.
    pub repeat_interval: Option<f64>,
    /// Repeat end position [m along the edge]; `None` = the edge's end.
    pub repeat_until: Option<f64>,
    /// Free viewport in logical pixels. The panels dock into a hand-built
    /// background `Ui`, which egui's area hit test never sees — so "is the
    /// mouse over UI?" is answered against this rect, not by egui.
    pub viewport: Rect,
    /// The pointer is over an egui area of its own — a floating window, an
    /// open menu, a tooltip. Those are not cut out of [`Self::viewport`], so
    /// the mouse-over test asks egui about them (see [`Self::over_viewport`]).
    pub pointer_over_ui: bool,
    /// A text field owns the keyboard — Delete/Enter/WASD belong to it then.
    pub typing: bool,
    /// Owner for native dialogs; a parentless dialog may open behind the window.
    pub window: Option<bevy::window::RawHandleWrapper>,
    /// The file dialog that is up, and what it was opened for. It runs on a
    /// thread of its own (see `ui::ask_for_file`), so the answer arrives some
    /// frames later — behind a mutex because a `Receiver` is `Send` but not
    /// `Sync`, and this state is a Bevy resource.
    pub pending_file: Option<(
        crate::ui::FileAsk,
        std::sync::Mutex<std::sync::mpsc::Receiver<Option<std::path::PathBuf>>>,
    )>,
    /// Comment-loss warning shown once per session (see the vehicle editor).
    pub warned_about_comments: bool,
    /// Whether the user has moved the map or used a tool yet — until then the
    /// viewport shows how.
    pub map_used: bool,
}

impl EditorState {
    /// Whether the mouse is on the map itself, and not on anything drawn over
    /// it — what every mouse binding of the viewport is gated on.
    pub fn over_viewport(&self, cursor: Vec2) -> bool {
        self.viewport.contains(cursor) && !self.pointer_over_ui
    }

    /// The i18n key of the toolbox category that is up, clamped — the map
    /// draws its track markings only while the track category is.
    pub fn active_category(&self) -> &'static str {
        TOOL_GROUPS[self.category.min(TOOL_GROUPS.len() - 1)].0
    }

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

/// How far past the envelope the track may still be clicked [m].
///
/// A module boundary is exactly where a rail meets its neighbour's, so the last
/// metre of track sits *on* the polygon. Snapping to a ghost boundary lands
/// there to the millimetre, and a hand-drawn arc ends within a few metres of it
/// — without this tolerance the one click a module transition is made of would
/// be refused.
const BOUNDARY_MARGIN: f64 = 10.0;

/// How far outside the envelope this tool may still place something, or `None`
/// when the envelope does not bound it at all.
///
/// Everything the module owns stays inside its envelope. The landscape strictly
/// so; the track, its turnouts and its lineside equipment up to
/// [`BOUNDARY_MARGIN`], because they are what has to reach the boundary.
fn envelope_margin(tool: Tool) -> Option<f64> {
    match tool {
        Tool::PlaceTree
        | Tool::PlaceForest
        | Tool::PlaceObject
        | Tool::PlaceMarker
        | Tool::TerrainRaise
        | Tool::TerrainLower
        | Tool::TerrainLevel
        | Tool::TerrainRail => Some(0.0),
        Tool::DrawTrack | Tool::Offset | Tool::Crossover | Tool::PlaceDevice => {
            Some(BOUNDARY_MARGIN)
        }
        // Selecting, marking, picking tiles and editing the envelope place
        // nothing — and the envelope cannot bound itself. Splitting, joining
        // and grading work on track that is already inside. The walkway
        // tools check the vertex they add themselves: a click on a vertex
        // that lies outside has to be able to pick it up and bring it back.
        Tool::Select
        | Tool::Split
        | Tool::Join
        | Tool::Gradient
        | Tool::Brush
        | Tool::MarkArea
        | Tool::PickTile
        | Tool::EditEnvelope
        | Tool::PlaceWalkPath
        | Tool::PlaceWalkArea
        | Tool::PlaceField => None,
    }
}

/// Layer a hand-placed marker lands in when none is named.
pub const DEFAULT_MARKER_LAYER: &str = "reference";

/// Where the running end is pointing: a map point, and the open end it has
/// snapped onto, if any — the click then joins that end instead of landing
/// beside it.
#[derive(Clone, Copy, Debug)]
pub struct Target {
    pub pos: EcefPos,
    pub end: Option<OpenEnd>,
}

impl Target {
    pub fn free(pos: EcefPos) -> Self {
        Self { pos, end: None }
    }
}

/// A track being drawn, the way the World Editor lays it. The press sets the
/// standing end and the drag its heading (`aiming`); after that every click
/// appends one tangent arc — or a straight while Ctrl is held — towards the
/// running end, which snaps onto open track ends. A press on a track makes a
/// branch of it: the drag then only decides facing or trailing.
///
/// ponytail: the whole alignment lives in the first point's EN plane —
/// metre-true for the few km a hand-drawn track spans; per-segment
/// re-anchoring steps in when someone draws across a whole map sheet.
pub struct Drawing {
    frame: EnuFrame,
    pub start: GeoPoint,
    /// Compass heading of the first segment [deg]; `None` until the drag or
    /// the second click has fixed it.
    pub heading_deg: Option<f64>,
    pub segments: Vec<Segment>,
    /// The edge this drawing branches off: `(edge, s)`.
    pub branch_of: Option<(usize, f64)>,
    /// Trailing turnout: the branch leaves against the running direction of
    /// the clicked track, so the far half of the split becomes the root.
    pub trailing: bool,
    /// The open end the drawing continues from — its node takes the start.
    pub from_end: Option<OpenEnd>,
    /// The open end the last click landed on — its node takes the finish.
    pub to_end: Option<OpenEnd>,
    /// The button is still down after the press: the drag sets the heading.
    pub aiming: bool,
    /// Ctrl is held — the next piece is a straight.
    pub straight: bool,
    /// Standard radii a drawn arc is rounded to; empty = as drawn.
    pub radii: Vec<f64>,
    /// Easement construction while the lay option is on: curves become
    /// clothoid–arc–clothoid and collect their cant in [`Self::cant_steps`].
    pub easements: Option<Easements>,
    /// Cant steps `(s, mm)` under the eased pieces so far — written to the
    /// edge on finish.
    pub cant_steps: Vec<(f64, f64)>,
    /// Running direction of the branched track in the frame, for the drag to
    /// decide facing or trailing against.
    base_tangent: Option<DVec2>,
    /// End of the drawn alignment in the frame's EN plane.
    end: DVec2,
    /// Math heading at the end [rad], 0 = east, counter-clockwise.
    end_heading: f64,
    /// Curvature at the drawn end [1/m] — what the next entry transition
    /// starts from.
    end_curvature: f64,
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
            from_end: None,
            to_end: None,
            aiming: false,
            straight: false,
            radii: Vec::new(),
            easements: None,
            cant_steps: Vec::new(),
            base_tangent: None,
            end: DVec2::ZERO,
            end_heading: 0.0,
            end_curvature: 0.0,
        }
    }

    /// Branch drawing: starts on the track at `pose` (`edge`, `s`) with the
    /// track's own heading fixed, so the branch leaves tangentially — a
    /// turnout, not a crossing.
    ///
    /// `trailing` turns the heading around: the branch then runs back along
    /// the clicked track, which is what a trailing connection looks like from
    /// the driver of that track — the fork lies behind them, not ahead. The
    /// drag after the press flips it (see [`Self::aim`]).
    pub fn branch_at(
        pose: &TrackPose,
        geoid_offset: f64,
        edge: usize,
        s: f64,
        trailing: bool,
    ) -> Self {
        let mut drawing = Self::start_at(pose.pos, geoid_offset);
        let tangent = drawing.frame.dir_to_local(pose.tangent);
        drawing.base_tangent = Some(DVec2::new(tangent.x, tangent.y));
        drawing.branch_of = Some((edge, s));
        drawing.set_trailing(trailing);
        drawing
    }

    /// Continues from an open end: the end's node takes the start, and the
    /// heading is the end's outward heading — the new piece leaves the old
    /// one tangentially.
    pub fn continue_from(end: OpenEnd, geoid_offset: f64) -> Self {
        let mut drawing = Self::start_at(end.pos, geoid_offset);
        drawing.fix_heading(end.heading);
        drawing.from_end = Some(end);
        // An entry transition starts from what the old track ends with.
        drawing.end_curvature = end.curvature;
        drawing
    }

    fn fix_heading(&mut self, heading: f64) {
        self.heading_deg = Some((90.0 - heading.to_degrees()).rem_euclid(360.0));
        self.end_heading = heading;
    }

    fn set_trailing(&mut self, trailing: bool) {
        let Some(tangent) = self.base_tangent else {
            return;
        };
        let along = if trailing { -tangent } else { tangent };
        self.trailing = trailing;
        self.fix_heading(along.y.atan2(along.x));
    }

    /// The drag after the press: a free start takes its heading from it, a
    /// branch reads facing or trailing off it. A drag shorter than a metre
    /// says nothing — the free start then waits for the second click.
    pub fn aim(&mut self, p: EcefPos) {
        let drag = self.local(p);
        if drag.length() < 1.0 {
            return;
        }
        match self.base_tangent {
            Some(tangent) => self.set_trailing(drag.dot(tangent) < 0.0),
            None if self.from_end.is_none() => self.fix_heading(drag.y.atan2(drag.x)),
            None => {}
        }
    }

    fn local(&self, p: EcefPos) -> DVec2 {
        let l = self.frame.to_local(p);
        DVec2::new(l.x, l.y)
    }

    /// The segments a click at `target` would append, with the heading after
    /// them: one arc or straight towards a free point, two arcs onto an open
    /// end so the join is tangent at both sides.
    fn preview(&self, target: Target) -> Option<(Vec<Segment>, f64)> {
        let p = self.local(target.pos);
        let Some(_) = self.heading_deg else {
            // No heading yet: the second click fixes it with a straight.
            let len = p.length();
            return (len > 1.0).then(|| (vec![Segment::straight(len)], p.y.atan2(p.x)));
        };
        if let Some(end) = target.end {
            // Arriving at the end means running against its outward heading.
            let arrive = end.heading + std::f64::consts::PI;
            return biarc(self.end, self.end_heading, p, arrive)
                .map(|[(a, _), (b, h)]| (vec![a, b], h));
        }
        if self.straight {
            let dir = DVec2::new(self.end_heading.cos(), self.end_heading.sin());
            let len = (p - self.end).dot(dir);
            return (len > 1.0).then(|| (vec![Segment::straight(len)], self.end_heading));
        }
        // With easements on, a curve becomes clothoid–arc–clothoid; where the
        // fit finds no room (straight ahead, or too short for its ramps) the
        // piece falls back to the bare arc.
        if let Some(e) = self.easements
            && let Some(piece) = easement_to(
                self.end,
                self.end_heading,
                self.end_curvature,
                p,
                e,
                &self.radii,
            )
        {
            return Some(piece);
        }
        let (segment, heading) = segment_to(self.end, self.end_heading, p)?;
        Some((vec![snap_radius(segment, &self.radii)], heading))
    }

    /// Appends the piece towards `target`; a click behind the heading is
    /// ignored. A click on an open end closes the drawing onto it — except
    /// the heading-fixing first click, whose straight would arrive at the
    /// end's own angle rather than tangentially.
    pub fn click(&mut self, target: Target) {
        let had_heading = self.heading_deg.is_some();
        let Some((segments, end_heading)) = self.preview(target) else {
            return;
        };
        let mut position = self.end;
        let mut heading = self.end_heading;
        if !had_heading {
            // The heading-fixing click is a straight: its direction of travel
            // is the same before and after the piece.
            self.heading_deg = Some((90.0 - end_heading.to_degrees()).rem_euclid(360.0));
            heading = end_heading;
        }
        for segment in &segments {
            let (p, h) = advance(position, heading, segment, segment.len);
            position = p;
            heading = h;
        }
        self.end = position;
        self.end_heading = heading;
        // Cant under an eased piece — only the easement fit writes clothoids,
        // so their presence is what marks one; plain pieces carry none.
        if let Some(e) = self.easements
            && segments.iter().any(|s| s.dk != 0.0)
        {
            let start: f64 = self.segments.iter().map(|s| s.len).sum();
            append_cant(&mut self.cant_steps, start, &segments, e);
        }
        self.end_curvature = segments
            .last()
            .map_or(self.end_curvature, |s| s.end_curvature());
        self.segments.extend(segments);
        self.to_end = target.end.filter(|_| had_heading);
    }

    /// Length, radius and cant of the piece the next click would append.
    pub fn readout(&self, target: Target) -> Option<Readout> {
        let (segments, _) = self.preview(target)?;
        let length = segments.iter().map(|s| s.len).sum();
        // The tightest arc of the piece — a biarc has two; a clothoid counts
        // with the curvature it reaches.
        let radius = segments
            .iter()
            .map(|s| s.k0.abs().max(s.end_curvature().abs()))
            .filter(|k| *k > 1e-9)
            .map(|k| 1.0 / k)
            .min_by(f64::total_cmp);
        // The cant an eased piece would carry — plain pieces write none.
        let cant = self
            .easements
            .filter(|_| segments.iter().any(|s| s.dk != 0.0))
            .map(|e| {
                segments
                    .iter()
                    .map(|s| {
                        signed_cant(s.k0, e)
                            .abs()
                            .max(signed_cant(s.end_curvature(), e).abs())
                    })
                    .fold(0.0, f64::max)
            })
            .filter(|cant| *cant > 0.0);
        Some(Readout {
            length,
            radius,
            cant,
            straight: self.straight && self.heading_deg.is_some(),
        })
    }

    /// Render polyline of the alignment so far; `cursor` appends the piece
    /// the next click would create. With `ground` given the line lies where
    /// the finished piece will: on the terrain, a glued start blending onto
    /// it over the first [`TERRAIN_SNAP_STEP`], a landed end onto the far
    /// track's height over the last.
    pub fn polyline(
        &self,
        cursor: Option<Target>,
        origin: &RenderOrigin,
        ground: Option<&dyn Fn(EcefPos) -> Option<f64>>,
    ) -> Vec<Vec3> {
        let mut heading = self
            .heading_deg
            .map(|d| (90.0 - d).to_radians())
            .unwrap_or(0.0);
        let mut segments = self.segments.clone();
        let mut end_height = None;
        if let Some(target) = cursor
            && let Some((next, _)) = self.preview(target)
        {
            if self.heading_deg.is_none() {
                let p = self.local(target.pos);
                heading = p.y.atan2(p.x);
            }
            segments.extend(next);
            end_height = target.end.map(|e| geo::from_ecef(e.pos).2);
        }
        let total: f64 = segments.iter().map(|s| s.len).sum();
        // A start glued to other track keeps that track's height, like the
        // finish does; a free start stands on the ground with the rest.
        let start_height = (self.from_end.is_some() || self.branch_of.is_some())
            .then(|| geo::from_ecef(self.frame.to_ecef(DVec3::ZERO)).2);
        let drop = |p: DVec2, s: f64| -> Vec3 {
            let flat = self.frame.to_ecef(DVec3::new(p.x, p.y, 0.0));
            let Some(mut h) = ground.and_then(|g| g(flat)) else {
                return self.to_render(p, origin);
            };
            let blend = |from: f64, to: f64, w: f64| from + (to - from) * w.clamp(0.0, 1.0);
            if let Some(h0) = start_height {
                h = blend(h0, h, s / TERRAIN_SNAP_STEP);
            }
            if let Some(h1) = end_height {
                h = blend(h1, h, (total - s) / TERRAIN_SNAP_STEP);
            }
            let (lat, lon, _) = geo::from_ecef(flat);
            origin.to_render(geo::to_ecef_deg(
                lat.to_degrees(),
                lon.to_degrees(),
                h + 0.5,
            ))
        };
        let mut position = DVec2::ZERO;
        let mut done = 0.0;
        let mut points = vec![drop(position, 0.0)];
        for segment in &segments {
            let steps = (segment.len / 5.0).ceil().max(1.0) as usize;
            for i in 1..=steps {
                let along = segment.len * i as f64 / steps as f64;
                let (p, _) = advance(position, heading, segment, along);
                points.push(drop(p, done + along));
            }
            let (p, h) = advance(position, heading, segment, segment.len);
            position = p;
            heading = h;
            done += segment.len;
        }
        points
    }

    /// The standing end's arrow while aiming: from the start along the heading
    /// the drag has set so far — `None` until there is one.
    pub fn aim_arrow(&self, origin: &RenderOrigin, length: f64) -> Option<[Vec3; 2]> {
        self.heading_deg?;
        let dir = DVec2::new(self.end_heading.cos(), self.end_heading.sin());
        Some([
            self.to_render(DVec2::ZERO, origin),
            self.to_render(dir * length, origin),
        ])
    }

    fn to_render(&self, p: DVec2, origin: &RenderOrigin) -> Vec3 {
        origin.to_render(self.frame.to_ecef(DVec3::new(p.x, p.y, 0.5)))
    }
}

/// Rounds an arc to the nearest of `radii`, keeping its change of heading —
/// the running end then jumps onto the standard radius, as it does in the
/// World Editor with super-elevation on. A straight and an empty list pass.
fn snap_radius(segment: Segment, radii: &[f64]) -> Segment {
    if segment.k0.abs() < 1e-9 || radii.is_empty() {
        return segment;
    }
    let radius = 1.0 / segment.k0.abs();
    let snapped = radii
        .iter()
        .copied()
        .min_by(|a, b| (a - radius).abs().total_cmp(&(b - radius).abs()))
        .unwrap_or(radius);
    let turn = segment.heading_delta(segment.len);
    Segment {
        len: turn.abs() * snapped,
        k0: segment.k0.signum() / snapped,
        dk: 0.0,
    }
}

/// Clothoid–arc–clothoid towards `target`: enters with `k_start`, leaves at
/// zero curvature, transition lengths from the cant rulebook at the piece's
/// speed. Fitted with Newton so the chain still passes through the clicked
/// point — seeded with the plain arc, refined on curvature and arc length.
/// With standard radii given, the radius snaps onto the series afterwards and
/// only the arc length keeps the end near the click: the running end jumps,
/// as it does in the World Editor with super-elevation on. `None` where the
/// piece is straight, too tight (under 50 m radius — a turnout curve, which
/// carries no easements on the prototype either), or too short for its ramps
/// — the caller then lays the bare arc.
fn easement_to(
    from: DVec2,
    heading: f64,
    k_start: f64,
    target: DVec2,
    e: Easements,
    radii: &[f64],
) -> Option<(Vec<Segment>, f64)> {
    let (seed, _) = segment_to(from, heading, target)?;
    // Straight or nearly (R > 10 km): nothing worth easing.
    if seed.k0.abs() < 1e-4 {
        return None;
    }
    let build = |k: f64, arc_len: f64| -> Option<Vec<Segment>> {
        if arc_len < 1.0 || k.abs() < 1e-6 || k.abs() > 1.0 / 50.0 {
            return None;
        }
        let mut chain = Vec::with_capacity(3);
        if (k - k_start).abs() > 1e-9 {
            let du = (signed_cant(k, e) - signed_cant(k_start, e)).abs();
            chain.push(Segment::transition(
                e.rules.ramp_length(du, e.speed),
                k_start,
                k,
            ));
        }
        chain.push(Segment {
            len: arc_len,
            k0: k,
            dk: 0.0,
        });
        chain.push(Segment::transition(
            e.rules.ramp_length(signed_cant(k, e), e.speed),
            k,
            0.0,
        ));
        Some(chain)
    };
    let end_of = |chain: &[Segment]| -> (DVec2, f64) {
        let mut p = from;
        let mut h = heading;
        for segment in chain {
            let (q, g) = advance(p, h, segment, segment.len);
            p = q;
            h = g;
        }
        (p, h)
    };
    // Newton on (curvature, arc length) with a numeric Jacobian, seeded with
    // the plain arc less the room its ramps will take.
    let mut k = seed.k0;
    let mut arc_len = (seed.len - 2.0 * e.rules.ramp_length(signed_cant(k, e), e.speed)).max(5.0);
    let mut converged = false;
    for _ in 0..20 {
        let (end, _) = end_of(&build(k, arc_len)?);
        let err = end - target;
        if err.length() < 0.01 {
            converged = true;
            break;
        }
        let hk = (k.abs() * 1e-3).max(1e-9);
        let (end_k, _) = end_of(&build(k + hk, arc_len)?);
        let (end_l, _) = end_of(&build(k, arc_len + 0.1)?);
        let d_k = (end_k - end) / hk;
        let d_l = (end_l - end) / 0.1;
        let det = d_k.x * d_l.y - d_l.x * d_k.y;
        if det.abs() < 1e-12 {
            return None;
        }
        k += (-err.x * d_l.y + err.y * d_l.x) / det;
        arc_len = (arc_len + (-d_k.x * err.y + d_k.y * err.x) / det).max(1.0);
    }
    if !radii.is_empty() {
        // Radius pinned to the series; the arc length alone slides the end to
        // the point of closest approach along its own tangent.
        let radius = 1.0 / k.abs();
        let snapped = radii
            .iter()
            .copied()
            .min_by(|a, b| (a - radius).abs().total_cmp(&(b - radius).abs()))?;
        k = k.signum() / snapped;
        for _ in 0..8 {
            let (end, h) = end_of(&build(k, arc_len)?);
            let along = (target - end).dot(DVec2::new(h.cos(), h.sin()));
            if along.abs() < 0.01 {
                break;
            }
            arc_len = (arc_len + along).max(1.0);
        }
    } else if !converged {
        let (end, _) = end_of(&build(k, arc_len)?);
        if end.distance(target) > 0.05 {
            return None;
        }
    }
    let chain = build(k, arc_len)?;
    let (_, end_heading) = end_of(&chain);
    Some((chain, end_heading))
}

/// Writes the cant under one eased piece: a ramp over each transition, the
/// arc's value held in between — 10 m steps through the importer's own
/// [`ramp_cant`], so the editor and the import write the same profile. The
/// last transition runs back to zero, and the step profile holds that.
pub(crate) fn append_cant(
    steps: &mut Vec<(f64, f64)>,
    start: f64,
    segments: &[Segment],
    e: Easements,
) {
    let worth = segments
        .iter()
        .any(|s| signed_cant(s.k0, e).abs() > 0.0 || signed_cant(s.end_curvature(), e).abs() > 0.0);
    if !worth {
        return;
    }
    if steps.is_empty() && start > 0.0 {
        // Everything before the first eased piece is level.
        steps.push((0.0, 0.0));
    }
    let mut s = start;
    for segment in segments {
        let from = signed_cant(segment.k0, e);
        let to = signed_cant(segment.end_curvature(), e);
        if segment.dk != 0.0 && (from - to).abs() > 1e-9 {
            ramp_cant(steps, s, segment.len, from, to);
        } else if (steps.last().map_or(0.0, |(_, c)| *c) - from).abs() > 1e-9 {
            steps.push((s, from));
        }
        s += segment.len;
    }
}

/// Two tangent-continuous arcs from `from` (math heading `h0`) to `to`,
/// arriving with heading `h1` — the biarc with equal tangent lengths, the
/// one every CAD joins two ends with. `None` where no such pair exists (the
/// ends point away from each other) or the ends coincide.
fn biarc(from: DVec2, h0: f64, to: DVec2, h1: f64) -> Option<[(Segment, f64); 2]> {
    let t0 = DVec2::new(h0.cos(), h0.sin());
    let t1 = DVec2::new(h1.cos(), h1.sin());
    let v = to - from;
    if v.length() < 1.0 {
        return None;
    }
    // Tangent length d: |v - d(t0 + t1)| = 2d, the joint being the midpoint
    // between the two tangent points.
    let a = 2.0 * (1.0 - t0.dot(t1));
    let b = 2.0 * v.dot(t0 + t1);
    let c = -v.length_squared();
    let d = if a.abs() < 1e-9 {
        (b.abs() > 1e-9).then(|| -c / b)?
    } else {
        (-b + (b * b - 4.0 * a * c).sqrt()) / (2.0 * a)
    };
    // NaN (parallel degenerate input) and non-positive d both mean "no biarc".
    if d.is_nan() || d <= 0.0 {
        return None;
    }
    let joint = (from + t0 * d + to - t1 * d) / 2.0;
    let (first, mid) = segment_to(from, h0, joint)?;
    let (second, end) = segment_to(joint, mid, to)?;
    Some([(first, mid), (second, end)])
}

/// Position and heading after `s` metres of `segment`, starting at `p` with
/// math heading `h` — [`Segment::offset`] keeps straights and arcs in closed
/// form and integrates clothoids, so the eased pieces preview correctly too.
pub(crate) fn advance(p: DVec2, h: f64, segment: &Segment, s: f64) -> (DVec2, f64) {
    let off = segment.offset(s);
    let (sh, ch) = h.sin_cos();
    (
        p + DVec2::new(ch * off.x - sh * off.y, sh * off.x + ch * off.y),
        h + segment.heading_delta(s),
    )
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
    pick_plane(camera, camera_transform, cursor, origin, focus, None)
}

/// Cursor → point on a horizontal plane: the focus plane, or the one at
/// ellipsoidal `height`.
///
/// A tool whose geometry sits at a height of its own has to pick on *that*
/// plane. The envelope does: looking down it makes no difference, but in the 3D
/// view a probe on the focus plane lands metres away from the corner it is
/// supposed to be dragging.
pub fn pick_plane(
    camera: &Camera,
    camera_transform: &GlobalTransform,
    cursor: Vec2,
    origin: &RenderOrigin,
    focus: &Focus,
    height: Option<f64>,
) -> Option<EcefPos> {
    let ray = camera.viewport_to_world(camera_transform, cursor).ok()?;
    let on_plane = match height {
        Some(height) => {
            let (lat, lon, _) = geo::from_ecef(focus.position);
            geo::to_ecef_deg(lat.to_degrees(), lon.to_degrees(), height)
        }
        None => focus.position,
    };
    let frame = EnuFrame::at(on_plane);
    let plane_point = origin.to_render(on_plane);
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
        let (s, d) = nearest_on_edge(edge, p);
        if best.is_none_or(|(_, _, best)| d < best) {
            best = Some((i, s, d));
        }
    }
    best
}

/// The arc length of one edge nearest `p`, and how far away it is [m].
///
/// Coarse scan then two refinements — the same probe `nearest_on_network` uses, pulled
/// out so a brush that has hold of one track can keep asking that track alone.
pub fn nearest_on_edge(edge: &track_model::TrackEdge, p: EcefPos) -> (f64, f64) {
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
    (s_best, d_best)
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

/// Switches the active tool and drops whatever half-done gesture the old one
/// held — the toolbox buttons, the number keys and the content drawer all go
/// through here.
pub fn select_tool(state: &mut EditorState, tool: Tool) {
    state.tool = tool;
    // A category tool pulls its box up; the select tool belongs to every
    // category and leaves the box where it is.
    if tool != Tool::Select {
        state.category = category_of(tool);
    }
    state.drawing = None;
    state.forest_points.clear();
    state.walk_points.clear();
    state.walk_drag = None;
    state.join_from = None;
    state.crossover_from = None;
    state.select_circle = None;
}

/// Where the running end points: snapped onto an open end within reach —
/// except the one the drawing left from — else the free map point.
fn lay_target(ends: &[OpenEnd], drawing: &Drawing, p: EcefPos, focus: &Focus) -> Target {
    let end = nearest_open_end(ends, p, pick_radius(focus))
        .filter(|end| drawing.from_end.is_none_or(|from| from.node != end.node));
    match end {
        Some(end) => Target {
            pos: end.pos,
            end: Some(end),
        },
        None => Target::free(p),
    }
}

/// How near the cursor an item has to be to be picked [logical pixels].
pub(crate) const PICK_PIXELS: f32 = 12.0;

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
    /// Where `p` lands on the screen, as a point of the pixel plane — the
    /// space the polyline helpers of [`crate::envelope`] measure in; `None`
    /// when it is off screen.
    pub fn screen(&self, p: EcefPos) -> Option<DVec3> {
        self.camera
            .world_to_viewport(self.transform, self.origin.to_render(p))
            .ok()
            .map(|screen| DVec3::new(screen.x as f64, screen.y as f64, 0.0))
    }

    /// The cursor in that same plane.
    pub fn cursor(&self) -> DVec3 {
        DVec3::new(self.cursor.x as f64, self.cursor.y as f64, 0.0)
    }

    /// Pixels between the cursor and `p`; `None` when it is off screen.
    pub fn distance(&self, p: EcefPos) -> Option<f32> {
        self.screen(p)
            .map(|screen| screen.distance(self.cursor()) as f32)
    }

    /// The same, but only within grabbing distance.
    pub fn hits(&self, p: EcefPos) -> Option<f32> {
        self.distance(p).filter(|d| *d <= PICK_PIXELS)
    }
}

/// Where the selection sits — what the gizmo stands on and `F` frames.
pub fn selection_pos(
    line: &Line,
    selection: Selection,
    focus: &Focus,
    marks: &Marks,
) -> Option<EcefPos> {
    match selection {
        Selection::Edge(i) => {
            let edge = line.net.edges().get(i)?;
            Some(edge.eval(edge.length() / 2.0).pos)
        }
        Selection::Device(i) => device_pos(&line.net, line.source.devices.get(i)?),
        Selection::Object(i) => object_pos(&line.net, line.source.objects.get(i)?),
        Selection::Tree(i) => Some(marks.tree(i, line.source.trees.get(i)?)),
        Selection::Marker(i) => Some(marks.marker(i, line.source.markers.get(i)?)),
        Selection::TerrainEdit(i) => Some(marks.stroke(i, line.source.terrain.get(i)?)),
        // The middle of the first stretch it covers — where `F` frames it, and where the
        // map jumps to from the list.
        Selection::EnvelopePoint(i) => Some(crate::envelope::point_pos(
            line.source.envelope.get(i)?,
            crate::envelope::height(line, focus),
        )),
        Selection::TrackArea(i) => {
            let span = line.source.areas.get(i)?.spans.first()?;
            let edge = line.net.edges().get(span.edge as usize)?;
            let s = ((span.from + span.to) / 2.0).clamp(0.0, edge.length());
            Some(edge.eval(s).pos)
        }
        // The first vertex — where the rule check jumps to as well.
        Selection::WalkPath(_) | Selection::WalkArea(_) => {
            let (kind, index) = crate::walkways::Kind::of_selection(selection)?;
            crate::walkways::vertex_pos(line, marks, kind, index, 0)
        }
        // A field's own middle, at the module's height like its outline.
        Selection::Field(i) => {
            let field = line.source.fields.get(i)?;
            let (lat, lon) = field.centre();
            Some(geo::to_ecef_deg(
                lat,
                lon,
                crate::envelope::height(line, focus),
            ))
        }
        // A water body's middle, likewise — the gizmo-free selection's
        // anchor, and what `F` frames.
        Selection::Water(i) => {
            let water = line.source.waters.get(i)?;
            let (lat, lon) = water.centre();
            Some(geo::to_ecef_deg(
                lat,
                lon,
                crate::envelope::height(line, focus),
            ))
        }
        Selection::None => None,
    }
}

/// Commits the painted stroke: onto the selected area, or into a new one.
fn commit_stroke(line: &mut Line, state: &mut EditorState, stroke: AreaStroke) -> Option<String> {
    if stroke.length() < 1.0 {
        return Some(t!("status-area-too-short"));
    }
    let span = stroke.span();
    match state.selection {
        // With an area selected the stroke joins it — that is how an area comes to cover
        // several tracks, one stroke at a time.
        Selection::TrackArea(i) if i < line.source.areas.len() => {
            line.source.areas[i].spans.push(span);
        }
        _ => {
            line.source.areas.push(content::route::TrackAreaSource {
                name: t!("area-default-name", index = line.source.areas.len() + 1),
                width: state.area_width.unwrap_or(AREA_WIDTH),
                spans: vec![span],
                ..Default::default()
            });
            state.selection = Selection::TrackArea(line.source.areas.len() - 1);
        }
    }
    None
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

/// The body of water the picked ground point falls into, if any — last in
/// the pick order, so a click on open water selects the lake and a click
/// beside it deselects as before. A point on an island misses, like it
/// should; the bodies are tried from the last on, so a pond drawn over a
/// lake is the one a click between them finds.
fn pick_water(line: &Line, p: EcefPos) -> Option<Selection> {
    let (lat, lon, _) = geo::from_ecef(p);
    pick_water_at(line, lat.to_degrees(), lon.to_degrees())
}

/// [`pick_water`] in degrees — the pick itself, and what the tests ask.
fn pick_water_at(line: &Line, lat: f64, lon: f64) -> Option<Selection> {
    line.source
        .waters
        .iter()
        .enumerate()
        .rev()
        .find(|(_, w)| w.contains(lat, lon))
        .map(|(i, _)| Selection::Water(i))
}

/// The waterline of a body — outline and islands — as a gizmo strip on the
/// ground. A corner off the loaded tiles keeps the module's fallback height
/// for the frame or two until its tile has come, like every mark does.
fn water_outline(
    gizmos: &mut Gizmos,
    origin: &RenderOrigin,
    view: &crate::terrain::TerrainView,
    options: &TerrainOptions,
    water: &content::route::WaterSource,
) {
    let fallback = options.fallback_height + options.geoid_offset;
    let on_ground = |lat: f64, lon: f64| -> EcefPos {
        let (e, n) = geo::to_utm(lat.to_radians(), lon.to_radians(), options.zone);
        let height = view.height_at(glam::DVec2::new(e, n)).unwrap_or(fallback);
        geo::to_ecef_deg(lat, lon, height + MARK_LIFT as f64)
    };
    let mut strip = |ring: &[content::route::WaterPoint]| {
        if ring.len() < 3 {
            return;
        }
        let mut points: Vec<_> = ring.iter().map(|p| on_ground(p.lat, p.lon)).collect();
        points.push(points[0]);
        gizmos.linestrip(
            points.iter().map(|p| origin.to_render(*p)),
            Color::srgb(0.36, 0.61, 0.96),
        );
    };
    strip(&water.polygon);
    for hole in &water.holes {
        strip(hole);
    }
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
        // Nothing references an area by index; the properties go with the marking.
        Selection::TrackArea(i) => {
            if i < line.source.areas.len() {
                line.source.areas.remove(i);
            }
        }
        // A polygon cannot go below three corners, so this one can refuse — the
        // caller says so in the status bar.
        Selection::EnvelopePoint(i) => {
            if !crate::envelope::remove_point(line, i) {
                state.selection = Selection::EnvelopePoint(i);
            }
        }
        // The picked vertex goes and the way stays selected for the next
        // one; with no vertex held, or when taking it out would leave less
        // than a way, the whole walkway goes.
        selection @ (Selection::WalkPath(_) | Selection::WalkArea(_)) => {
            let Some((kind, index)) = crate::walkways::Kind::of_selection(selection) else {
                return;
            };
            match state.walk_vertex.take() {
                Some(vertex) if crate::walkways::remove_vertex(line, kind, index, vertex) => {
                    state.selection = selection;
                }
                _ => crate::walkways::remove(line, kind, index),
            }
        }
        // Nothing references a field by index, so a plain remove suffices.
        Selection::Field(i) => {
            if i < line.source.fields.len() {
                line.source.fields.remove(i);
            }
        }
        // A water body neither — and the ground under it rebuilds through
        // the change detector, which sees the polygon go.
        Selection::Water(i) => {
            if i < line.source.waters.len() {
                line.source.waters.remove(i);
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

/// Deletes everything in the multi-selection — brush sweep, circle or
/// Ctrl-clicks — as one undo step.
pub fn delete_marked(line: &mut Line, state: &mut EditorState) {
    let mut trees: Vec<usize> = Vec::new();
    let mut objects: Vec<usize> = Vec::new();
    let mut devices: Vec<usize> = Vec::new();
    let mut markers: Vec<usize> = Vec::new();
    for mark in state.marked.drain(..) {
        match mark {
            Mark::Tree(i) => trees.push(i),
            Mark::Object(i) => objects.push(i),
            Mark::Device(i) => devices.push(i),
            Mark::Marker(i) => markers.push(i),
        }
    }
    // Descending order, so earlier removals do not shift later indices.
    for list in [&mut trees, &mut objects, &mut devices, &mut markers] {
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
    // A device takes its signal and the signal's route references along.
    for i in devices.into_iter().rev() {
        line.source.remove_device(i);
    }
    for i in markers.into_iter().rev() {
        if i < line.source.markers.len() {
            line.source.markers.remove(i);
        }
    }
    // Indices shifted under the selection.
    state.selection = Selection::None;
}

/// Marks every tree and object within `radius` of `p` — the brush sweep.
fn mark_within(state: &mut EditorState, line: &Line, marks: &Marks, p: EcefPos, radius: f64) {
    for (i, tree) in line.source.trees.iter().enumerate() {
        if marks.tree(i, tree).distance(p) <= radius && !state.marked.contains(&Mark::Tree(i)) {
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

/// Marks everything within `radius` of `p` — the select tool's circle:
/// trees and objects like the brush, plus devices and the reference markers
/// of visible layers.
fn mark_circle(state: &mut EditorState, line: &Line, marks: &Marks, p: EcefPos, radius: f64) {
    mark_within(state, line, marks, p, radius);
    for (i, device) in line.source.devices.iter().enumerate() {
        let near = device_pos(&line.net, device).is_some_and(|q| q.distance(p) <= radius);
        if near && !state.marked.contains(&Mark::Device(i)) {
            state.marked.push(Mark::Device(i));
        }
    }
    for (i, marker) in line.source.markers.iter().enumerate() {
        if state.layer_visible(&marker.layer)
            && marks.marker(i, marker).distance(p) <= radius
            && !state.marked.contains(&Mark::Marker(i))
        {
            state.marked.push(Mark::Marker(i));
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
    // A wood drawn between two corners of a concave envelope can reach past it,
    // even though every one of its own corners was inside — the fill is what
    // has to be cut, not the outline.
    let (inside, outside): (Vec<_>, Vec<_>) = trees
        .into_iter()
        .partition(|t| line.source.envelope_contains(t.lat, t.lon));
    overlay.status = if outside.is_empty() {
        t!("status-forest-baked", count = inside.len())
    } else {
        t!(
            "status-forest-baked-clipped",
            count = inside.len(),
            dropped = outside.len()
        )
    };
    line.source.trees.extend(inside);
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

/// The terrain tiles of this line's corridor — what a full height import
/// covers, and the grid the tile picker works on.
pub fn corridor_tiles(line: &Line, options: content::TerrainOptions) -> Vec<content::TileKey> {
    content::TerrainBuilder::new(&line.net, Vec::new(), options).corridor_keys()
}

/// Tile the map point `p` falls into.
pub fn tile_of(p: EcefPos, options: content::TerrainOptions) -> content::TileKey {
    content::terrain::tile_at(content::terrain::to_utm(p, &options), &options)
}

/// The four corners of a tile at `height` — for drawing the grid.
///
/// One height for the whole grid, as for the module boundary: a rectangle over
/// half a kilometre of ground would take a corner into every hollow it spans.
fn tile_corners(
    k: content::TileKey,
    options: content::TerrainOptions,
    height: f64,
) -> [EcefPos; 5] {
    let min = content::terrain::tile_min(k, options.tile_size);
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
/// nodes and one edge; one that left an open end or landed on one shares that
/// end's node, which turns into a joint; a branch drawing splits its base edge
/// and wires the joint into a turnout whose diverging leg is the drawing.
/// `false` only when the split failed — the drawing is gone either way.
pub fn finish_drawing(
    line: &mut Line,
    state: &mut EditorState,
    ground: Option<&dyn Fn(EcefPos) -> Option<f64>>,
) -> bool {
    let ground = if state.lay.snap_terrain { ground } else { None };
    let Some(drawing) = state.drawing.take() else {
        return true;
    };
    let (Some(heading_deg), false) = (drawing.heading_deg, drawing.segments.is_empty()) else {
        return true;
    };
    let profiles = state.lay.profiles();
    // The far end: a new buffer, or the open end the drawing landed on.
    // Landing on the very end it left would make a loop of one edge, which
    // is no track — that end stays open.
    let to_end = drawing
        .to_end
        .filter(|end| drawing.from_end.is_none_or(|from| from.node != end.node));
    let to = match to_end {
        Some(end) => end.node,
        None => {
            line.source.nodes.push(NodeSource::Buffer);
            line.source.nodes.len() as u32 - 1
        }
    };
    let index = line.source.edges.len();
    if let Some((base, s)) = drawing.branch_of {
        let Some((joint, straight)) = line.source.split_edge(base, s) else {
            return false;
        };
        // The split appended an edge: the branch comes after it.
        let branch = line.source.edges.len();
        let trailing = drawing.trailing;
        // Facing: Continue = end pose of the first half = the cut,
        // tangentially. Trailing: the branch runs the other way, and a
        // `Continue` can only ever mean "onwards" — the cut's own
        // coordinates with the reversed heading say the same thing.
        let start = if trailing {
            EdgeStart::Geo {
                point: drawing.start,
                heading_deg,
            }
        } else {
            EdgeStart::Continue { edge: base as u32 }
        };
        line.source
            .edges
            .push(profiles.edge(joint, to, start, drawing.segments));
        if !drawing.cant_steps.is_empty() {
            line.source.edges.last_mut().expect("just pushed").cant = drawing.cant_steps.clone();
        }
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
        close_end(line, to_end);
        if let Some(g) = ground {
            snap_edge_to_terrain(
                line,
                branch,
                g,
                false,
                to_end.map(|e| geo::from_ecef(e.pos).2),
            );
        }
        state.selection = Selection::Edge(branch);
        return true;
    }
    let (from, start) = match drawing.from_end {
        // Continuing from an end: that node takes the start. Off the edge's
        // own end the geometry chains exactly; off its start it runs back
        // the other way, which only the coordinates can say.
        Some(end) => (
            end.node,
            if end.at_end {
                EdgeStart::Continue {
                    edge: end.edge as u32,
                }
            } else {
                EdgeStart::Geo {
                    point: drawing.start,
                    heading_deg,
                }
            },
        ),
        None => {
            line.source.nodes.push(NodeSource::Buffer);
            (
                line.source.nodes.len() as u32 - 1,
                EdgeStart::Geo {
                    point: drawing.start,
                    heading_deg,
                },
            )
        }
    };
    line.source
        .edges
        .push(profiles.edge(from, to, start, drawing.segments));
    if !drawing.cant_steps.is_empty() {
        line.source.edges.last_mut().expect("just pushed").cant = drawing.cant_steps.clone();
    }
    close_end(line, drawing.from_end);
    close_end(line, to_end);
    if let Some(g) = ground {
        snap_edge_to_terrain(
            line,
            index,
            g,
            drawing.from_end.is_none(),
            to_end.map(|e| geo::from_ecef(e.pos).2),
        );
    }
    // Yards are laid several tracks at a time: the copies run parallel to the
    // right, each one a track of its own.
    for n in 1..state.lay.parallel.max(1) {
        match line.source.compile() {
            Ok(compiled) => line.net = compiled.net,
            Err(_) => break,
        }
        let copy = offset_edge(line, index, -state.lay.spacing * n as f64);
        // Each copy stands on its own ground — a yard on a hillside.
        if let (Some(g), Some(copy)) = (ground, copy) {
            snap_edge_to_terrain(line, copy, g, true, None);
        }
    }
    state.selection = Selection::Edge(index);
    true
}

/// An open end that just gained its second edge is a joint now.
fn close_end(line: &mut Line, end: Option<OpenEnd>) {
    if let Some(end) = end
        && let Some(node) = line.source.nodes.get_mut(end.node as usize)
        && matches!(node, NodeSource::Buffer)
    {
        *node = NodeSource::Joint;
    }
}

/// Ground sample spacing of the terrain snap [m] — grade steps land at this
/// interval, coarse enough to keep an edge's profile list readable.
const TERRAIN_SNAP_STEP: f64 = 20.0;

/// Rewrites the vertical profile of edge `index` so the track follows the
/// ground: heights sampled every [`TERRAIN_SNAP_STEP`] along the compiled
/// alignment become grade steps. A free `Geo` start drops onto the surface;
/// a start glued to other track (`free_start = false`) keeps its height and
/// the first interval works the difference off. `end_height` pins the far
/// end the same way — a drawing that landed on an open end has to meet that
/// track, not the ground under it. Where the ground gives no answer the
/// profile is left alone.
fn snap_edge_to_terrain(
    line: &mut Line,
    index: usize,
    ground: &dyn Fn(EcefPos) -> Option<f64>,
    free_start: bool,
    end_height: Option<f64>,
) {
    let Ok(compiled) = line.source.compile() else {
        return;
    };
    let Some(edge) = compiled.net.edges().get(index) else {
        return;
    };
    let length = edge.length();
    if length < 1.0 {
        return;
    }
    let count = (length / TERRAIN_SNAP_STEP).ceil().max(1.0) as usize;
    let step = length / count as f64;
    let mut heights = Vec::with_capacity(count + 1);
    for i in 0..=count {
        let Some(h) = ground(edge.eval(step * i as f64).pos) else {
            return;
        };
        heights.push(h);
    }
    if !free_start {
        heights[0] = geo::from_ecef(edge.eval(0.0).pos).2;
    }
    if let Some(h) = end_height {
        *heights.last_mut().expect("count >= 1") = h;
    }
    let mut grade: Vec<(f64, f64)> = Vec::new();
    for (i, pair) in heights.windows(2).enumerate() {
        let g = (pair[1] - pair[0]) / step * 1000.0;
        if grade.last().is_none_or(|(_, last)| (g - last).abs() > 0.01) {
            grade.push((step * i as f64, g));
        }
    }
    // A profile that came out dead level is no profile at all.
    if grade.len() == 1 && grade[0].1.abs() < 0.01 {
        grade.clear();
    }
    let geoid = line.source.geoid_offset;
    let source = &mut line.source.edges[index];
    source.grade = grade;
    if free_start && let EdgeStart::Geo { point, .. } = &mut source.start {
        point.height = heights[0] - geoid;
    }
}

/// The open ends of the line — every buffer node, seen from its edge — as
/// world positions with their outward headings.
pub fn open_ends(line: &Line) -> Vec<OpenEnd> {
    let mut ends = Vec::new();
    for (i, (source, edge)) in line.source.edges.iter().zip(line.net.edges()).enumerate() {
        let outward = |pose: TrackPose, sign: f64| {
            let local = EnuFrame::at(pose.pos).dir_to_local(pose.tangent * sign);
            local.y.atan2(local.x)
        };
        if matches!(
            line.source.nodes.get(source.from as usize),
            Some(NodeSource::Buffer)
        ) {
            let pose = edge.eval(0.0);
            ends.push(OpenEnd {
                node: source.from,
                edge: i,
                at_end: false,
                pos: pose.pos,
                heading: outward(pose, -1.0),
                // Walking out of the start runs the edge backwards, which
                // turns a left curve into a right one.
                curvature: -pose.curvature,
            });
        }
        if matches!(
            line.source.nodes.get(source.to as usize),
            Some(NodeSource::Buffer)
        ) {
            let pose = edge.end_pose();
            ends.push(OpenEnd {
                node: source.to,
                edge: i,
                at_end: true,
                pos: pose.pos,
                heading: outward(pose, 1.0),
                curvature: pose.curvature,
            });
        }
    }
    ends
}

/// The open end within `radius` of `p` (horizontally), nearest first.
pub fn nearest_open_end(ends: &[OpenEnd], p: EcefPos, radius: f64) -> Option<OpenEnd> {
    let frame = EnuFrame::at(p);
    ends.iter()
        .map(|end| {
            let local = frame.to_local(end.pos);
            (*end, DVec2::new(local.x, local.y).length())
        })
        .filter(|(_, d)| *d <= radius)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(end, _)| end)
}

/// Welds two open ends. Ends that lie on one point share a node from now on;
/// ends apart are staked out by the calculator (see [`crate::stake`]):
/// transitions, arc and compensating straight — or the double arc with its
/// intermediate straight — by the staking options. The error names what
/// stood in the way, worded like the original's messages.
pub fn join_ends(
    line: &mut Line,
    lay: &LayOptions,
    stake: &crate::stake::StakeOptions,
    a: OpenEnd,
    b: OpenEnd,
) -> Result<(), String> {
    if a.node == b.node {
        return Err(t!("status-join-same-end"));
    }
    if a.pos.distance(b.pos) < 1.0 {
        line.source.merge_nodes(a.node, b.node);
        return Ok(());
    }
    let frame = EnuFrame::at(a.pos);
    let to = frame.to_local(b.pos);
    let arrive = b.heading + std::f64::consts::PI;
    let e = stake.easement_rules(lay.speed.unwrap_or(content::route::DEFAULT_SPEED));
    // Arriving at `b` runs its edge against the outward direction, which
    // flips the curvature seen by the chain.
    let staked = crate::stake::stake_out(
        a.heading,
        a.curvature,
        DVec2::new(to.x, to.y),
        arrive,
        -b.curvature,
        stake,
        e,
    )
    .map_err(|err| {
        t!(match err {
            crate::stake::StakeError::NotPlausible => "status-stake-not-plausible",
            crate::stake::StakeError::RadiusTooBig => "status-stake-radius-too-big",
            crate::stake::StakeError::ArcTooShort => "status-stake-arc-too-short",
            crate::stake::StakeError::DoubleImpossible => "status-stake-double-impossible",
        })
    })?;
    let start = if a.at_end {
        EdgeStart::Continue {
            edge: a.edge as u32,
        }
    } else {
        let (lat, lon, height) = geo::from_ecef(a.pos);
        EdgeStart::Geo {
            point: GeoPoint {
                lat: lat.to_degrees(),
                lon: lon.to_degrees(),
                height: height - line.source.geoid_offset,
            },
            heading_deg: (90.0 - a.heading.to_degrees()).rem_euclid(360.0),
        }
    };
    line.source
        .edges
        .push(lay.profiles().edge(a.node, b.node, start, staked.segments));
    if !staked.cant.is_empty() {
        line.source.edges.last_mut().expect("just pushed").cant = staked.cant;
    }
    close_end(line, Some(a));
    close_end(line, Some(b));
    Ok(())
}

/// Lays a parallel track `distance` metres beside edge `index` (positive =
/// left of its running direction): two new buffer nodes and an edge whose
/// segments are the offset curves — exact for straights and arcs, the
/// profiles copied as they stand. Returns the new edge's index.
///
/// ponytail: a clothoid's offset is no clothoid; its curvatures are mapped
/// end to end and the length scaled by the mean — centimetres at track
/// spacing, and the drawing tools lay no clothoids anyway.
pub fn offset_edge(line: &mut Line, index: usize, distance: f64) -> Option<usize> {
    let edge = line.net.edges().get(index)?;
    let source = line.source.edges.get(index)?.clone();
    let pose = edge.eval(0.0);
    let left = pose.up.cross(pose.tangent).normalize_or_zero();
    let start = EcefPos(pose.pos.0 + left * distance);
    let (lat, lon, height) = geo::from_ecef(start);
    let segments: Vec<Segment> = source
        .segments
        .iter()
        .map(|s| {
            // An offset curve's curvature k' = k / (1 - k·d); its length
            // scales by what a metre of the centre line becomes out there.
            let k0 = s.k0 / (1.0 - s.k0 * distance);
            let k1 = s.end_curvature() / (1.0 - s.end_curvature() * distance);
            let mid = s.curvature_at(s.len / 2.0);
            let len = s.len * (1.0 - mid * distance);
            Segment::transition(len.max(0.1), k0, k1)
        })
        .collect();
    let node = line.source.nodes.len() as u32;
    line.source.nodes.push(NodeSource::Buffer);
    line.source.nodes.push(NodeSource::Buffer);
    let new = line.source.edges.len();
    line.source.edges.push(EdgeSource {
        from: node,
        to: node + 1,
        start: EdgeStart::Geo {
            point: GeoPoint {
                lat: lat.to_degrees(),
                lon: lon.to_degrees(),
                height: height - line.source.geoid_offset,
            },
            heading_deg: (90.0 - edge.heading0.to_degrees()).rem_euclid(360.0),
        },
        segments,
        grade: source.grade,
        cant: source.cant,
        speed: source.speed,
        track_type: source.track_type,
        electrification: source.electrification,
        formation: source.formation,
    });
    Some(new)
}

/// Builds a crossover from edge `a` at `s` over to edge `b`: two turnouts of
/// `radius` joined by the S of their arcs, which lands on `b` exactly where
/// it runs parallel at the distance the arcs cover. Both tracks are cut
/// there and wired into turnouts. The error says what stood in the way.
pub fn crossover(
    line: &mut Line,
    lay: &LayOptions,
    a: usize,
    s: f64,
    b: usize,
    radius: f64,
) -> Result<(), String> {
    if a == b {
        return Err(t!("status-crossover-same-track"));
    }
    let (edge_a, edge_b) = match (line.net.edges().get(a), line.net.edges().get(b)) {
        (Some(ea), Some(eb)) => (ea, eb),
        _ => return Err(t!("status-no-track-hit")),
    };
    let pose = edge_a.eval(s);
    let frame = EnuFrame::at(pose.pos);
    let tangent = frame.dir_to_local(pose.tangent);
    let (tangent, left) = (
        DVec2::new(tangent.x, tangent.y).normalize(),
        DVec2::new(-tangent.y, tangent.x).normalize(),
    );
    // Which side the other track lies on, and how far: measured where it
    // passes the cut.
    let (s_b0, _) = nearest_on_edge(edge_b, pose.pos);
    let across = frame.to_local(edge_b.eval(s_b0).pos);
    let d = DVec2::new(across.x, across.y).dot(left);
    if d.abs() < 2.0 || d.abs() > 2.0 * radius {
        return Err(t!("status-crossover-not-parallel"));
    }
    // Two arcs of the turnout radius, each covering half the distance across.
    let theta = (1.0 - d.abs() / (2.0 * radius)).acos();
    let sign = d.signum();
    let arcs = vec![
        Segment {
            len: radius * theta,
            k0: sign / radius,
            dk: 0.0,
        },
        Segment {
            len: radius * theta,
            k0: -sign / radius,
            dk: 0.0,
        },
    ];
    let landing = tangent * (2.0 * radius * theta.sin()) + left * d;
    let landing = frame.to_ecef(DVec3::new(landing.x, landing.y, 0.0));
    let (s_b, miss) = nearest_on_edge(edge_b, landing);
    let pose_b = edge_b.eval(s_b);
    let along = pose.tangent.dot(pose_b.tangent);
    // The other track has to pass the landing point, and run parallel there
    // — either way round.
    if miss > 1.0 || along.abs() < 0.999 {
        return Err(t!("status-crossover-not-parallel"));
    }
    let same_way = along > 0.0;
    // Both cuts checked before the first one happens — a half-built crossover
    // would leave one track split for nothing.
    if s < 1.0 || s > edge_a.length() - 1.0 || s_b < 1.0 || s_b > edge_b.length() - 1.0 {
        return Err(t!("status-split-at-end"));
    }
    let Some((joint_a, a2)) = line.source.split_edge(a, s) else {
        return Err(t!("status-split-at-end"));
    };
    let Some((joint_b, b2)) = line.source.split_edge(b, s_b) else {
        return Err(t!("status-split-at-end"));
    };
    let diagonal = line.source.edges.len() as u32;
    line.source.edges.push(lay.profiles().edge(
        joint_a,
        joint_b,
        EdgeStart::Continue { edge: a as u32 },
        arcs,
    ));
    // Leaving `a`: a train over its first half faces the fork.
    line.source.nodes[joint_a as usize] = NodeSource::Switch {
        root: (a as u32, true),
        straight: (a2, false),
        diverging: (diagonal, false),
        throw_time: DEFAULT_THROW_TIME,
    };
    // Arriving on `b`: the diagonal trails in, and the leg it continues onto
    // is the root — the far half when both run the same way, the near half
    // when `b` runs the other way.
    line.source.nodes[joint_b as usize] = if same_way {
        NodeSource::Switch {
            root: (b2, false),
            straight: (b as u32, true),
            diverging: (diagonal, true),
            throw_time: DEFAULT_THROW_TIME,
        }
    } else {
        NodeSource::Switch {
            root: (b as u32, true),
            straight: (b2, false),
            diverging: (diagonal, true),
            throw_time: DEFAULT_THROW_TIME,
        }
    };
    Ok(())
}

/// Puts a gradient break point on edge `index` at `s`: a step that starts
/// with the gradient already there, for the panel to edit. Returns `false`
/// when one sits within a metre already.
pub fn add_grade_step(line: &mut Line, index: usize, s: f64) -> bool {
    let Some(edge) = line.source.edges.get_mut(index) else {
        return false;
    };
    if edge.grade.iter().any(|(x, _)| (x - s).abs() < 1.0) {
        return false;
    }
    let current = edge
        .grade
        .iter()
        .rev()
        .find(|(x, _)| *x <= s)
        .or(edge.grade.first())
        .map_or(0.0, |(_, g)| *g);
    edge.grade.push((s, current));
    edge.grade.sort_by(|a, b| a.0.total_cmp(&b.0));
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
    signal_types: Res<crate::signals::SignalTypes>,
    gizmo: Res<crate::gizmo::GizmoState>,
    marks: Res<crate::terrain::Marks>,
    terrain_view: Res<crate::terrain::TerrainView>,
    time: Res<Time>,
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
        Selection::EnvelopePoint(i) if i >= line.source.envelope.len() => {
            state.selection = Selection::None;
        }
        Selection::WalkPath(i) if i >= line.source.walk_paths.len() => {
            state.selection = Selection::None;
        }
        Selection::WalkArea(i) if i >= line.source.walk_areas.len() => {
            state.selection = Selection::None;
        }
        _ => {}
    }
    // The held vertex goes with the walkway it belongs to.
    if let Some(vertex) = state.walk_vertex
        && crate::walkways::Kind::of_selection(state.selection)
            .and_then(|(kind, index)| crate::walkways::vertices(&line.source, kind, index))
            .is_none_or(|points| vertex >= points.len())
    {
        state.walk_vertex = None;
    }
    // Stale marks likewise — one out-of-range index and the sweep is void.
    let stale = state.marked.iter().any(|m| match m {
        Mark::Tree(i) => *i >= line.source.trees.len(),
        Mark::Object(i) => *i >= line.source.objects.len(),
        Mark::Device(i) => *i >= line.source.devices.len(),
        Mark::Marker(i) => *i >= line.source.markers.len(),
    });
    if stale {
        state.marked.clear();
    }
    // Half-done first picks of the join and crossover tools go stale the same
    // way (undo, a delete in between) — dropped rather than welded wrongly.
    if state
        .join_from
        .is_some_and(|end| end.edge >= line.source.edges.len())
    {
        state.join_from = None;
    }
    if state
        .crossover_from
        .is_some_and(|(edge, _)| edge >= line.source.edges.len())
    {
        state.crossover_from = None;
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
        .filter(|c| state.over_viewport(*c));
    let view = cursor.zip(camera.single().ok());
    // Ground point under the cursor, while it is over the free viewport.
    let picked = view.and_then(|(c, (camera, camera_transform))| {
        pick_ground(camera, camera_transform, c, &origin.0, &focus)
    });
    // The envelope sits at a fixed height of its own, so it is picked on that
    // plane rather than on the focus plane.
    let picked_envelope = view.and_then(|(c, (camera, camera_transform))| {
        pick_plane(
            camera,
            camera_transform,
            c,
            &origin.0,
            &focus,
            Some(crate::envelope::height(&line, &focus)),
        )
    });
    // …and the same cursor as a screen-space probe, for selecting.
    let pick = view.map(|(cursor, (camera, transform))| ScreenPick {
        camera,
        transform,
        origin: &origin.0,
        cursor,
    });

    // A corner of the envelope owns the mouse the same way.
    if let Some(index) = state.envelope_drag {
        if !buttons.pressed(MouseButton::Left) {
            state.envelope_drag = None;
        } else if let Some(p) = picked_envelope {
            crate::envelope::drag_point(&mut line, index, p);
        }
        return;
    }

    // And a vertex of a walkway — dragged on the plane through its own
    // height: in the 3D view a probe on the focus plane lands metres away
    // from a vertex on a slope.
    if let Some(vertex) = state.walk_drag {
        if !buttons.pressed(MouseButton::Left) {
            state.walk_drag = None;
        } else if let Some((kind, index)) = crate::walkways::Kind::of_selection(state.selection)
            && let Some(at) = crate::walkways::vertex_pos(&line, &marks, kind, index, vertex)
            && let Some(p) = view.and_then(|(c, (camera, camera_transform))| {
                pick_plane(
                    camera,
                    camera_transform,
                    c,
                    &origin.0,
                    &focus,
                    Some(geo::from_ecef(at).2),
                )
            })
        {
            crate::walkways::drag_vertex(&mut line, kind, index, vertex, p);
        }
        return;
    }

    // An active support-point drag owns the mouse until the button goes up.
    if let Some((edge, point)) = state.drag {
        if !buttons.pressed(MouseButton::Left) {
            state.drag = None;
        } else if let Some(p) = picked {
            drag_support_point(&mut line, edge, point, snap_ghost(p, &ghost, &focus));
        }
        return;
    }

    // What the lay and join tools snap onto and continue from.
    state.open_ends = if matches!(state.tool, Tool::DrawTrack | Tool::Join) {
        open_ends(&line)
    } else {
        Vec::new()
    };
    // Ctrl holds the next piece straight; radius snap and easements follow
    // their options every frame, so flipping one mid-drawing takes effect on
    // the next click.
    let snap = state.lay.snap_radius;
    let easements = state.lay.easement_rules();
    if let Some(drawing) = &mut state.drawing {
        drawing.straight =
            keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
        drawing.radii = if snap {
            content::import::alignment::preferred_radii()
        } else {
            Vec::new()
        };
        drawing.easements = easements;
    }

    if keys.just_pressed(KeyCode::Escape) {
        if state.select_circle.take().is_some() {
            // The growing circle is dropped, nothing else changes hands.
        } else if !state.forest_points.is_empty() {
            state.forest_points.clear();
        } else if !state.walk_points.is_empty() {
            state.walk_points.clear();
        } else if !state.marked.is_empty() {
            state.marked.clear();
        } else if state.join_from.take().is_some() || state.crossover_from.take().is_some() {
            // The half-done pick of the join or crossover tool is dropped.
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
    // Enter finishes; so does a right *click* — told apart from the camera's
    // right-drag by the cursor standing still between press and release.
    if buttons.just_pressed(MouseButton::Right) {
        state.right_press = cursor;
    }
    let right_click = buttons.just_released(MouseButton::Right)
        && state
            .right_press
            .take()
            .zip(windows.single().ok().and_then(|w| w.cursor_position()))
            .is_some_and(|(down, up)| down.distance(up) < 4.0);
    // The ground the terrain snap reads: loaded tiles first, the builder's
    // blended surface where no tile is in the scene yet.
    let ground = |p: EcefPos| terrain_view.ground_height(p);
    if keys.just_pressed(KeyCode::Enter) || right_click {
        if !state.forest_points.is_empty() {
            finish_forest(&mut line, &mut state, &mut overlay);
        } else if !state.walk_points.is_empty() {
            // The field tool collects its corners in the same buffer the
            // walkways do — the gesture is the same, only what is made of it
            // at the end differs.
            let status = if state.tool == Tool::PlaceField {
                crate::fields::finish(&mut line, &mut state)
            } else {
                crate::walkways::finish(&mut line, &mut state)
            };
            if let Some(status) = status {
                overlay.status = status;
            }
        } else if !finish_drawing(&mut line, &mut state, Some(&ground)) {
            overlay.status = t!("status-split-failed");
        }
    }
    // Tool switching from the keyboard: `1` is always select, then the
    // active category's own tools — the key and the button agree.
    let boxed = state.category.min(TOOL_GROUPS.len() - 1);
    let numbered =
        std::iter::once(SELECT_ENTRY.0).chain(TOOL_GROUPS[boxed].2.iter().map(|(t, _, _)| *t));
    for (key, tool) in DIGITS.into_iter().zip(numbered) {
        if keys.just_pressed(key) && state.tool != tool {
            select_tool(&mut state, tool);
        }
    }

    // The standing end is aimed while the button stays down after the press:
    // the drag is the heading of a free start, and facing or trailing of a
    // branch. Letting go fixes it.
    if state.tool == Tool::DrawTrack {
        let snapped = picked.map(|p| snap_ghost(p, &ghost, &focus));
        if let Some(drawing) = &mut state.drawing
            && drawing.aiming
        {
            if buttons.pressed(MouseButton::Left) {
                if let Some(p) = snapped {
                    drawing.aim(p);
                }
            } else {
                drawing.aiming = false;
            }
        }
        // What the piece under the cursor would be — the status bar reads it.
        let readout = match (&state.drawing, snapped) {
            (Some(drawing), Some(p)) if !drawing.aiming => {
                drawing.readout(lay_target(&state.open_ends, drawing, p, &focus))
            }
            _ => None,
        };
        state.readout = readout;
    } else {
        state.readout = None;
    }

    // The select tool's circle: a press on empty ground grows a radius while
    // the button is held — Train Simulator Classic's area selection. The
    // release marks everything inside; with Ctrl held it adds to what is
    // marked, alone it replaces it. Under 2 m it was just a click.
    if let Some(center) = state.select_circle {
        if state.tool != Tool::Select {
            state.select_circle = None;
        } else if buttons.pressed(MouseButton::Left) {
            return;
        } else {
            state.select_circle = None;
            if let Some(p) = picked
                && center.distance(p) >= 2.0
            {
                let ctrl =
                    keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
                if !ctrl {
                    state.marked.clear();
                }
                mark_circle(&mut state, &line, &marks, center, center.distance(p));
            }
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
            mark_within(&mut state, &line, &marks, p, radius);
        }
        return;
    }

    // The area brush paints while the button is held: the press takes hold of a track,
    // the drag stretches the stroke along it, the release lays it down.
    if state.tool == Tool::MarkArea {
        if buttons.just_pressed(MouseButton::Left)
            && let Some(p) = picked
        {
            state.map_used = true;
            match nearest_on_network(&line.net, p) {
                Some((edge, s, d)) if d <= pick_radius(&focus) => {
                    state.area_stroke = Some(AreaStroke {
                        edge,
                        from: s,
                        to: s,
                    });
                }
                _ => overlay.status = t!("status-no-track-hit"),
            }
        }
        if buttons.pressed(MouseButton::Left)
            && let Some(stroke) = &mut state.area_stroke
            && let Some(p) = picked
            && let Some(edge) = line.net.edges().get(stroke.edge)
        {
            // Projected onto the track the stroke started on, so it follows that track
            // even where the cursor wanders off it — and never jumps to a neighbour
            // halfway through a station.
            stroke.to = nearest_on_edge(edge, p).0;
        }
        if !buttons.pressed(MouseButton::Left)
            && let Some(stroke) = state.area_stroke.take()
            && let Some(status) = commit_stroke(&mut line, &mut state, stroke)
        {
            overlay.status = status;
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

    // Everything the module owns stays inside the module. The envelope is what
    // it covers, and ground worked past it is the neighbour's — Zusi builds to
    // the same rule. The track is measured with a tolerance, see
    // [`envelope_margin`].
    if let Some(margin) = envelope_margin(state.tool) {
        let (lat, lon, _) = geo::from_ecef(p);
        if !line
            .source
            .envelope_contains_within(lat.to_degrees(), lon.to_degrees(), margin)
        {
            overlay.status = if margin > 0.0 {
                t!("status-outside-envelope-track")
            } else {
                t!("status-outside-envelope")
            };
            return;
        }
    }

    match state.tool {
        Tool::DrawTrack => {
            let p = snap_ghost(p, &ghost, &focus);
            let ends = state.open_ends.clone();
            let mut landed = false;
            match &mut state.drawing {
                // The press sets the standing end: on an open end it continues
                // that track, on a track's middle it starts the branch of a
                // turnout, on open ground it starts fresh — and the drag until
                // release aims it.
                None => {
                    if let Some(end) = nearest_open_end(&ends, p, pick_radius(&focus)) {
                        state.drawing = Some(Drawing::continue_from(end, line.source.geoid_offset));
                    } else {
                        match nearest_on_network(&line.net, p) {
                            Some((edge, s, distance)) if distance <= pick_radius(&focus) => {
                                let length = line.net.edges()[edge].length();
                                if s < 1.0 || s > length - 1.0 {
                                    overlay.status = t!("status-split-at-end");
                                } else {
                                    let pose = line.net.edges()[edge].eval(s);
                                    let mut drawing = Drawing::branch_at(
                                        &pose,
                                        line.source.geoid_offset,
                                        edge,
                                        s,
                                        false,
                                    );
                                    drawing.aiming = true;
                                    state.drawing = Some(drawing);
                                }
                            }
                            _ => {
                                let mut drawing = Drawing::start_at(p, line.source.geoid_offset);
                                drawing.aiming = true;
                                state.drawing = Some(drawing);
                            }
                        }
                    }
                }
                Some(drawing) => {
                    drawing.click(lay_target(&ends, drawing, p, &focus));
                    landed = drawing.to_end.is_some();
                }
            }
            // A click onto an open end closes the drawing there and then —
            // nothing more could be appended past a joint anyway.
            if landed && !finish_drawing(&mut line, &mut state, Some(&ground)) {
                overlay.status = t!("status-split-failed");
            }
        }
        Tool::Split => match nearest_on_network(&line.net, p) {
            Some((edge, s, distance)) if distance <= pick_radius(&focus) => {
                if line.source.split_edge(edge, s).is_none() {
                    overlay.status = t!("status-split-at-end");
                } else {
                    state.selection = Selection::Edge(edge);
                }
            }
            _ => overlay.status = t!("status-no-track-hit"),
        },
        Tool::Join => match nearest_open_end(&state.open_ends, p, pick_radius(&focus)) {
            Some(end) => match state.join_from.take() {
                None => state.join_from = Some(end),
                Some(first) => {
                    if let Err(status) = join_ends(&mut line, &state.lay, &state.stake, first, end)
                    {
                        overlay.status = status;
                    }
                }
            },
            None => overlay.status = t!("status-no-open-end"),
        },
        Tool::Offset => match nearest_on_network(&line.net, p) {
            Some((edge, s, distance)) if distance <= pick_radius(&focus) => {
                // The parallel goes to the side the click fell on.
                let pose = line.net.edges()[edge].eval(s);
                let left = pose.up.cross(pose.tangent).normalize_or_zero();
                let side = (p.0 - pose.pos.0).dot(left).signum();
                let spacing = state.lay.spacing.max(1.0);
                if let Some(new) = offset_edge(&mut line, edge, spacing * side) {
                    state.selection = Selection::Edge(new);
                }
            }
            _ => overlay.status = t!("status-no-track-hit"),
        },
        Tool::Crossover => match nearest_on_network(&line.net, p) {
            Some((edge, s, distance)) if distance <= pick_radius(&focus) => {
                match state.crossover_from.take() {
                    None => state.crossover_from = Some((edge, s)),
                    Some((a, s_a)) => {
                        let radius = state.lay.turnout_radius.max(50.0);
                        if let Err(status) = crossover(&mut line, &state.lay, a, s_a, edge, radius)
                        {
                            overlay.status = status;
                        }
                    }
                }
            }
            _ => overlay.status = t!("status-no-track-hit"),
        },
        Tool::Gradient => match nearest_on_network(&line.net, p) {
            Some((edge, s, distance)) if distance <= pick_radius(&focus) => {
                add_grade_step(&mut line, edge, s);
                state.selection = Selection::Edge(edge);
            }
            _ => overlay.status = t!("status-no-track-hit"),
        },
        Tool::PlaceDevice => {
            match nearest_on_network(&line.net, p) {
                Some((edge, s, distance)) if distance <= pick_radius(&focus) => {
                    let kind = state.device_kind();
                    line.source.devices.push(DeviceSource {
                        kind: kind.clone(),
                        edge: edge as u32,
                        s,
                        facing: Facing::default(),
                        lateral_offset: 0.0,
                        payload: String::new(),
                    });
                    let device = line.source.devices.len() - 1;
                    // A signal picked in the content drawer arms type and model
                    // here, and the placement carries them straight away — the
                    // device alone would stand there dark and modelless until
                    // someone typed the type into the panel by hand.
                    if kind == DeviceKind::Signal
                        && (state.signal_type.is_some() || state.signal_model.is_some())
                    {
                        let system = state
                            .signal_type
                            .as_deref()
                            .and_then(|name| signal_types.map.get(name))
                            .map_or(SignalSystem::Ks, |ty| ty.system);
                        line.source.signals.push(SignalSource {
                            kind: SignalKind::Main,
                            system,
                            device: device as u32,
                            next: None,
                            guarded: Vec::new(),
                            requires_route: false,
                            diverging_speed: None,
                            signal_type: state.signal_type.clone(),
                            model: state.signal_model.clone(),
                        });
                    }
                    state.selection = Selection::Device(device);
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
                        snap_to_terrain: state.place_snap_to_terrain,
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
        Tool::EditEnvelope => {
            if line.source.envelope.len() < 3 {
                overlay.status = t!("status-envelope-none");
                return;
            }
            // A corner outranks the side it sits on, or a corner could never be
            // picked up again once it has been placed.
            if let Some(pick) = pick.as_ref()
                && let Some(index) = crate::envelope::pick_point(&line, pick, &focus)
            {
                state.selection = Selection::EnvelopePoint(index);
                state.envelope_drag = Some(index);
                return;
            }
            let p = picked_envelope.unwrap_or(p);
            match crate::envelope::pick_side(&line, p, &focus, pick_radius(&focus)) {
                Some((side, t)) => {
                    let index = crate::envelope::insert_point(&mut line, side, t);
                    state.selection = Selection::EnvelopePoint(index);
                    // Straight into a drag: the corner was added where the click
                    // landed, and it is placed by moving it from there.
                    state.envelope_drag = Some(index);
                    overlay.status = t!("status-envelope-point-added");
                }
                None => overlay.status = t!("status-envelope-no-hit"),
            }
        }
        Tool::PlaceWalkPath | Tool::PlaceWalkArea => {
            use crate::walkways::{Hit, Kind};
            let Some(kind) = Kind::of_tool(state.tool) else {
                return;
            };
            // While a way is being drawn every click is its next vertex,
            // whatever it lands on — a way has to be drawable right beside
            // another one.
            let hit = if state.walk_points.is_empty() {
                let selected = Kind::of_selection(state.selection)
                    .filter(|(k, _)| *k == kind)
                    .map(|(_, index)| index);
                pick.as_ref().and_then(|pick| {
                    crate::walkways::pick(
                        &line,
                        &marks,
                        kind,
                        selected,
                        |q| pick.screen(q),
                        pick.cursor(),
                        PICK_PIXELS as f64,
                    )
                })
            } else {
                None
            };
            match hit {
                Some(Hit::Vertex { index, vertex }) => {
                    state.selection = kind.selection(index);
                    state.walk_vertex = Some(vertex);
                    state.walk_drag = Some(vertex);
                }
                Some(Hit::Side { index, side, t }) => {
                    if let Some(vertex) =
                        crate::walkways::insert_vertex(&mut line, kind, index, side, t)
                    {
                        state.selection = kind.selection(index);
                        state.walk_vertex = Some(vertex);
                        // Straight into a drag, like the envelope's corner: the
                        // vertex was added where the click landed, and it is
                        // placed by moving it from there.
                        state.walk_drag = Some(vertex);
                        overlay.status = t!("status-walk-vertex-added");
                    }
                }
                Some(Hit::Body { index }) => {
                    state.selection = kind.selection(index);
                    state.walk_vertex = None;
                }
                None => {
                    // A new vertex has to lie inside the module, like every
                    // tree and every stroke.
                    let (lat, lon, _) = geo::from_ecef(p);
                    if !line
                        .source
                        .envelope_contains(lat.to_degrees(), lon.to_degrees())
                    {
                        overlay.status = t!("status-outside-envelope");
                        return;
                    }
                    state.walk_points.push(p);
                }
            }
        }
        Tool::PlaceField => {
            // While an outline is being drawn every click is its next corner,
            // whatever it lands on — a field has to be drawable inside another
            // one's neighbourhood.
            if state.walk_points.is_empty()
                && let Some(index) = crate::fields::pick(&line, p)
            {
                state.selection = Selection::Field(index);
                return;
            }
            let (lat, lon, _) = geo::from_ecef(p);
            if !line
                .source
                .envelope_contains(lat.to_degrees(), lon.to_degrees())
            {
                overlay.status = t!("status-outside-envelope");
                return;
            }
            state.walk_points.push(p);
        }
        // Handled above, where the button is held rather than only clicked.
        Tool::MarkArea => {}
        Tool::PickTile => {
            let key = tile_of(p, state.terrain_options());
            match state.picked_tiles.iter().position(|k| *k == key) {
                Some(i) => {
                    state.picked_tiles.remove(i);
                }
                None => state.picked_tiles.push(key),
            }
        }
        Tool::TerrainRaise | Tool::TerrainLower | Tool::TerrainLevel | Tool::TerrainRail => {
            let edit = match state.tool {
                // Up or down by the shared amount — the tool is the sign.
                Tool::TerrainRaise => TerrainEdit::Raise(state.terrain_amount.unwrap_or(2.0).abs()),
                Tool::TerrainLower => {
                    TerrainEdit::Raise(-state.terrain_amount.unwrap_or(2.0).abs())
                }
                // Flatten to the ground height under the click — the World
                // Editor's plateau gesture. The tiles answer the height;
                // before they are built there is nothing to flatten to.
                Tool::TerrainLevel => match terrain_view.height_at_pos(p) {
                    Some(height) => TerrainEdit::Level(height),
                    None => {
                        overlay.status = t!("status-no-ground-height");
                        return;
                    }
                },
                // Level to the nearest rail — that is what levelling means on
                // a railway, and the editor knows the rail height without a
                // DGM.
                _ => match nearest_on_network(&line.net, p) {
                    Some((edge, s, _)) => {
                        let (_, _, height) = geo::from_ecef(line.net.edges()[edge].eval(s).pos);
                        TerrainEdit::Level(height)
                    }
                    None => {
                        overlay.status = t!("status-no-track-hit");
                        return;
                    }
                },
            };
            let (lat, lon, _) = geo::from_ecef(p);
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
                .map(|(i, t)| (Selection::Tree(i), marks.tree(i, t)))
                .collect::<Vec<_>>();
            let terrain = line
                .source
                .terrain
                .iter()
                .enumerate()
                .map(|(i, e)| (Selection::TerrainEdit(i), marks.stroke(i, e)))
                .collect::<Vec<_>>();
            // Hidden layers are not pickable — out of sight, out of reach.
            let markers = line
                .source
                .markers
                .iter()
                .enumerate()
                .filter(|(_, m)| state.layer_visible(&m.layer))
                .map(|(i, m)| (Selection::Marker(i), marks.marker(i, m)))
                .collect::<Vec<_>>();
            // Walkways by their vertices — picked as a whole; reshaping them
            // is their own tools' job.
            let mut walkways = Vec::new();
            for kind in [crate::walkways::Kind::Path, crate::walkways::Kind::Area] {
                for i in 0..kind.count(&line.source) {
                    for pos in crate::walkways::positions(&line, &marks, kind, i) {
                        walkways.push((kind.selection(i), pos));
                    }
                }
            }
            let nearest = device
                .into_iter()
                .chain(objects_)
                .chain(trees)
                .chain(markers)
                .chain(terrain)
                .chain(walkways)
                .filter_map(|(sel, pos)| Some((sel, pick.hits(pos)?)))
                .min_by(|a, b| a.1.total_cmp(&b.1));
            // Ctrl collects a multi-selection instead of replacing the
            // single one — the World Editor's add-to-selection hotkey. A
            // second Ctrl-click on the same thing takes it out again.
            let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
            if ctrl {
                match nearest.and_then(|(sel, _)| as_mark(sel)) {
                    Some(mark) => match state.marked.iter().position(|m| *m == mark) {
                        Some(k) => {
                            state.marked.remove(k);
                        }
                        None => state.marked.push(mark),
                    },
                    // Ctrl on empty ground grows the circle that adds — but
                    // not over water, where a click has a meaning of its
                    // own and the circle would start by surprise.
                    None if nearest.is_none()
                        && nearest_edge(&line, pick).is_none()
                        && pick_water(&line, p).is_none() =>
                    {
                        state.select_circle = Some(p);
                    }
                    None => {}
                }
                return;
            }
            // Point candidates first, the track last, the water under
            // everything: a lake picked by clicking it, but never against
            // the track that crosses it on an embankment.
            state.selection = match nearest {
                Some((sel, _)) => sel,
                None => nearest_edge(&line, pick)
                    .map(Selection::Edge)
                    .or_else(|| pick_water(&line, p))
                    .unwrap_or(Selection::None),
            };
            // A walkway picked here is the whole way; no vertex is held.
            state.walk_vertex = None;
            // A press that found nothing may grow into the circle selection;
            // one that stays under 2 m keeps meaning "deselect".
            if state.selection == Selection::None {
                state.select_circle = Some(p);
            }
            // The World Editor's double click: the second pick of the same
            // thing within the window sends the panel to its properties —
            // which also unfolds a folded panel.
            let now = time.elapsed_secs_f64();
            if let Some((at, what)) = state.last_select
                && now - at < 0.35
                && what == state.selection
                && state.selection != Selection::None
            {
                state.jump_to = Some("selection");
            }
            state.last_select = Some((now, state.selection));
        }
        // Handled above — the brush owns the whole press, not just the click.
        Tool::Brush => {}
    }
}

/// How far above the ground a mark is drawn \[m\].
///
/// Not a hair's breadth: the aerial photo is draped over the terrain with a
/// lift of its own (`height_offset` in `imagery.ron`, a metre by default,
/// because the drape's grid is coarser than the terrain's), and a mark at the
/// same height disappears into the picture. Raise this if that is raised.
pub(crate) const MARK_LIFT: f32 = 2.5;

/// Circle gizmo lying flat on the ground at `p`.
pub(crate) fn ground_circle(
    gizmos: &mut Gizmos,
    origin: &RenderOrigin,
    p: EcefPos,
    radius: f32,
    color: Color,
) {
    let up = origin.dir_to_render(EnuFrame::at(p).up);
    let rotation = Quat::from_rotation_arc(Vec3::Z, up);
    gizmos.circle(
        Isometry3d::new(origin.to_render(p) + up * MARK_LIFT, rotation),
        radius,
        color,
    );
}

/// Square gizmo lying flat on the ground at `p` — the World Editor's weld
/// marker shape: a circle is a device, a diamond a reference marker, the
/// square is a rail joint.
pub(crate) fn ground_square(
    gizmos: &mut Gizmos,
    origin: &RenderOrigin,
    p: EcefPos,
    half: f32,
    color: Color,
) {
    let center = origin.to_render(p) + origin.dir_to_render(EnuFrame::at(p).up) * MARK_LIFT;
    let (x, z) = (Vec3::X * half, Vec3::Z * half);
    gizmos.linestrip(
        [
            center - x - z,
            center + x - z,
            center + x + z,
            center - x + z,
            center - x - z,
        ],
        color,
    );
}

/// Arrow out of an edge end along the track, lying on the ground — the World
/// Editor's direction handle.
fn end_arrow(
    gizmos: &mut Gizmos,
    origin: &RenderOrigin,
    pose: TrackPose,
    sign: f64,
    len: f64,
    color: Color,
) {
    let dir3 = (pose.tangent * sign).normalize_or_zero();
    let side3 = dir3.cross(pose.up).normalize_or_zero();
    let base = origin.to_render(pose.pos) + origin.dir_to_render(pose.up) * MARK_LIFT;
    let dir = origin.dir_to_render(dir3);
    let side = origin.dir_to_render(side3);
    let tip = base + dir * len as f32;
    let head = (len * 0.3) as f32;
    gizmos.line(base, tip, color);
    gizmos.line(tip, tip - dir * head + side * (head * 0.6), color);
    gizmos.line(tip, tip - dir * head - side * (head * 0.6), color);
}

/// A V on the rail pointing uphill — the World Editor's slope arrow.
fn chevron(
    gizmos: &mut Gizmos,
    origin: &RenderOrigin,
    pose: TrackPose,
    sign: f64,
    arm: f64,
    color: Color,
) {
    let dir3 = (pose.tangent * sign).normalize_or_zero();
    let side3 = dir3.cross(pose.up).normalize_or_zero();
    let base = origin.to_render(pose.pos) + origin.dir_to_render(pose.up);
    let dir = origin.dir_to_render(dir3) * arm as f32;
    let side = origin.dir_to_render(side3) * (arm * 0.7) as f32;
    let tip = base + dir;
    gizmos.line(tip, base - dir + side, color);
    gizmos.line(tip, base - dir - side, color);
}

/// Track ribbon of one edge as a line on the ground.
fn edge_line(gizmos: &mut Gizmos, origin: &RenderOrigin, edge: &track_model::TrackEdge, c: Color) {
    span_line(
        gizmos,
        origin,
        edge,
        0.0,
        edge.length(),
        LineOffset {
            lift: 1.0,
            across: 0.0,
        },
        c,
    );
}

/// Where a line is drawn relative to the track: metres above it and metres beside it.
#[derive(Clone, Copy)]
struct LineOffset {
    lift: f64,
    across: f64,
}

/// A stretch `[from, to]` of one edge, offset from the track — what a marked area is
/// drawn as.
fn span_line(
    gizmos: &mut Gizmos,
    origin: &RenderOrigin,
    edge: &track_model::TrackEdge,
    from: f64,
    to: f64,
    offset: LineOffset,
    c: Color,
) {
    let LineOffset { lift, across } = offset;
    let (from, to) = (from.clamp(0.0, edge.length()), to.clamp(0.0, edge.length()));
    if to <= from {
        return;
    }
    let steps = (((to - from) / 10.0).ceil() as usize).max(2);
    let points = (0..=steps).map(|j| {
        let pose = edge.eval(from + (to - from) * j as f64 / steps as f64);
        let side = pose.tangent.cross(pose.up).normalize_or_zero();
        origin.to_render(pose.pos)
            + origin.dir_to_render(pose.up) * lift as f32
            + origin.dir_to_render(side) * across as f32
    });
    gizmos.linestrip(points, c);
}

/// Half-width the area brush paints with by default [m].
pub use content::route::DEFAULT_AREA_WIDTH as AREA_WIDTH;

/// How far above the track the stroke is painted [m] — above the ribbon (0.4) so it is
/// never hidden by it, and low enough to still read as lying on the track.
pub const AREA_LIFT: f64 = 0.7;

/// The stroke under the cursor: a filled-looking band of parallel lines, which is as close
/// to a painted stroke as a gizmo gets. It exists only while the button is down — once it
/// is laid down it is a mesh like the rest of them.
#[allow(clippy::too_many_arguments)]
fn stroke_band(
    gizmos: &mut Gizmos,
    origin: &RenderOrigin,
    edge: &track_model::TrackEdge,
    from: f64,
    to: f64,
    width: f64,
    c: Color,
) {
    const LINES: usize = 7;
    for i in 0..LINES {
        let t = i as f64 / (LINES - 1) as f64 * 2.0 - 1.0;
        span_line(
            gizmos,
            origin,
            edge,
            from,
            to,
            LineOffset {
                lift: AREA_LIFT + 0.2,
                across: t * width,
            },
            c,
        );
    }
    stroke_outline(gizmos, origin, edge, from, to, width, c);
}

/// The outline of a stroke: both long sides and both ends, so a stretch reads as a
/// stretch and not as a track that happens to be coloured.
#[allow(clippy::too_many_arguments)]
fn stroke_outline(
    gizmos: &mut Gizmos,
    origin: &RenderOrigin,
    edge: &track_model::TrackEdge,
    from: f64,
    to: f64,
    width: f64,
    c: Color,
) {
    let (from, to) = (from.min(to), from.max(to));
    let lift = AREA_LIFT + 0.3;
    for across in [-width, width] {
        span_line(
            gizmos,
            origin,
            edge,
            from,
            to,
            LineOffset { lift, across },
            c,
        );
    }
    for s in [from, to] {
        let s = s.clamp(0.0, edge.length());
        let pose = edge.eval(s);
        let side = origin.dir_to_render(pose.tangent.cross(pose.up).normalize_or_zero());
        let base = origin.to_render(pose.pos) + origin.dir_to_render(pose.up) * lift as f32;
        gizmos.line(base - side * width as f32, base + side * width as f32, c);
    }
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
    ground: crate::terrain::Ground,
    windows: Query<&Window, With<PrimaryWindow>>,
    camera: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut gizmos: Gizmos,
) {
    let accent = Color::srgb(0.36, 0.61, 0.96);
    // The tile grid lies on the ground the cursor is over — the height the
    // status bar reads out anyway. Without terrain it keeps the view point's.
    let grid_height = ground
        .view
        .cursor_height
        .unwrap_or_else(|| geo::from_ecef(focus.position).2);

    // The spline line of every edge while the track category is up — the
    // World Editor's loft line: over aerial imagery the grey rails vanish at
    // height, and the line is what keeps the alignment readable. Drawn first,
    // so the highlights and the selection paint over it. The selected edge is
    // skipped; it wears the accent line instead.
    let track_work = state.active_category() == "tool-group-track";
    if track_work {
        let spline = Color::srgba(0.40, 0.58, 0.90, 0.85);
        for (i, edge) in line.net.edges().iter().enumerate() {
            if state.selection == Selection::Edge(i) {
                continue;
            }
            edge_line(&mut gizmos, &origin.0, edge, spline);
        }
    }

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

    // The painted areas are meshes (`spawn_areas`); what is left for the gizmos is the
    // stroke under the cursor, which does not exist in the document yet.
    if let Some(stroke) = state.area_stroke
        && let Some(edge) = line.net.edges().get(stroke.edge)
    {
        let width = state.area_width.unwrap_or(AREA_WIDTH);
        stroke_band(
            &mut gizmos,
            &origin.0,
            edge,
            stroke.from.min(stroke.to),
            stroke.from.max(stroke.to),
            width,
            accent,
        );
    }

    match state.selection {
        Selection::Edge(i) => {
            if let Some(edge) = line.net.edges().get(i) {
                edge_line(&mut gizmos, &origin.0, edge, accent);
                // Direction handles, the World Editor's pair: red out of the
                // start, blue out of the end. The arrows say which way `s`
                // runs — what every metre figure in the panel is measured
                // along.
                let len = pick_radius(&focus);
                for (pose, sign, color) in [
                    (edge.eval(0.0), -1.0, Color::srgb(0.87, 0.28, 0.23)),
                    (edge.end_pose(), 1.0, Color::srgb(0.30, 0.56, 0.95)),
                ] {
                    end_arrow(&mut gizmos, &origin.0, pose, sign, len, color);
                }
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
        // The selected area keeps the colour it was painted in; what marks it as selected
        // is the accent outline around the stroke, the same one every other selection in
        // the editor wears.
        Selection::TrackArea(i) => {
            if let Some(area) = line.source.areas.get(i) {
                let width = area.width;
                for span in &area.spans {
                    if let Some(edge) = line.net.edges().get(span.edge as usize) {
                        stroke_outline(
                            &mut gizmos,
                            &origin.0,
                            edge,
                            span.from,
                            span.to,
                            width,
                            accent,
                        );
                    }
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
        // Trees, markers and terrain strokes are drawn below; the envelope
        // and the walkways draw their own vertices, selected one included.
        Selection::Tree(_)
        | Selection::Marker(_)
        | Selection::TerrainEdit(_)
        | Selection::EnvelopePoint(_)
        | Selection::WalkPath(_)
        | Selection::WalkArea(_) => {}
        // Fields draw their own outline and their working direction, in
        // `crate::fields::draw_outlines` below — every field, not only the
        // selected one.
        Selection::Field(_) => {}
        // The selected body's waterline, islands included — what a click
        // picked, on the ground the tiles put under it.
        Selection::Water(i) => {
            if let Some(water) = line.source.waters.get(i) {
                water_outline(
                    &mut gizmos,
                    &origin.0,
                    &ground.view,
                    &state.terrain_options(),
                    water,
                );
            }
        }
        Selection::None => {}
    }

    // The module boundary, under everything the tools draw on top of it.
    crate::envelope::draw(&mut gizmos, &line, &origin.0, &focus, &state);

    // Terrain strokes as their true footprint: the circle is the radius the
    // stroke actually reaches, so overlapping ones show where the ground is
    // worked twice. Raising warm, lowering cold, levelling neutral.
    for (i, edit) in line.source.terrain.iter().enumerate() {
        let p = ground.marks.stroke(i, edit);
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
        let p = ground.marks.marker(i, marker);
        if state.selection == Selection::Marker(i) {
            ground_circle(
                &mut gizmos,
                &origin.0,
                p,
                (focus.height * 0.012).max(4.0) as f32,
                accent,
            );
        }
        let center = origin.0.to_render(p) + origin.0.dir_to_render(EnuFrame::at(p).up) * MARK_LIFT;
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
            let p = ground.marks.tree(i, tree);
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
            let center =
                origin.0.to_render(p) + origin.0.dir_to_render(EnuFrame::at(p).up) * MARK_LIFT;
            gizmos.line(center - Vec3::X * arm, center + Vec3::X * arm, color);
            gizmos.line(center - Vec3::Z * arm, center + Vec3::Z * arm, color);
        }
    }
    // Marked objects, devices and markers wear the same orange as marked
    // trees.
    for mark in &state.marked {
        let pos = match *mark {
            Mark::Object(i) => line
                .source
                .objects
                .get(i)
                .and_then(|o| object_pos(&line.net, o)),
            Mark::Device(i) => line
                .source
                .devices
                .get(i)
                .and_then(|d| device_pos(&line.net, d)),
            Mark::Marker(i) => line
                .source
                .markers
                .get(i)
                .map(|m| ground.marks.marker(i, m)),
            // Trees are tinted in their own pass above.
            Mark::Tree(_) => None,
        };
        if let Some(p) = pos {
            let radius = (focus.height * 0.010).max(3.0) as f32;
            ground_circle(&mut gizmos, &origin.0, p, radius, marked_color);
        }
    }

    let cursor = windows
        .single()
        .ok()
        .and_then(|w| w.cursor_position())
        .filter(|c| state.over_viewport(*c))
        .and_then(|c| {
            let (camera, camera_transform) = camera.single().ok()?;
            pick_ground(camera, camera_transform, c, &origin.0, &focus)
        });
    // The select tool's circle, growing under the held button.
    if let Some(center) = state.select_circle
        && let Some(p) = cursor
    {
        let radius = center.distance(p).max(2.0) as f32;
        ground_circle(&mut gizmos, &origin.0, center, radius, marked_color);
    }
    // Rail joints while the track category is up, after the World Editor: a
    // square at every node — grey where edges weld (a switch is a weld of
    // three), red where an end is loose. The loose end is the thing to
    // continue from or to fix, so it is the one that shouts. While laying or
    // joining, the end the cursor would take turns accent and the join tool's
    // first pick is filled.
    if track_work {
        let half = (focus.height * 0.008).max(3.0) as f32;
        let loose = Color::srgb(0.87, 0.28, 0.23);
        let weld = Color::srgb(0.62, 0.64, 0.70);
        let reachable =
            cursor.and_then(|p| nearest_open_end(&state.open_ends, p, pick_radius(&focus)));
        let mut seen = vec![false; line.source.nodes.len()];
        for (source, edge) in line.source.edges.iter().zip(line.net.edges()) {
            for (node, pose) in [(source.from, edge.eval(0.0)), (source.to, edge.end_pose())] {
                let Some(kind) = line.source.nodes.get(node as usize) else {
                    continue;
                };
                if std::mem::replace(&mut seen[node as usize], true) {
                    continue;
                }
                let is_loose = matches!(kind, NodeSource::Buffer);
                let near = reachable.is_some_and(|r| r.node == node);
                let picked_first = state.join_from.is_some_and(|f| f.node == node);
                let color = match (is_loose, near || picked_first) {
                    (true, true) => accent,
                    (true, false) => loose,
                    (false, _) => weld,
                };
                ground_square(&mut gizmos, &origin.0, pose.pos, half, color);
                if picked_first {
                    ground_square(&mut gizmos, &origin.0, pose.pos, half * 0.5, color);
                }
            }
        }
    }
    // The crossover's first cut, until the second track is named.
    if let Some((edge, s)) = state.crossover_from
        && let Some(edge) = line.net.edges().get(edge)
    {
        let radius = (focus.height * 0.012).max(4.0) as f32;
        ground_circle(&mut gizmos, &origin.0, edge.eval(s).pos, radius, accent);
    }
    // Gradient break points of the selected edge while the gradient tool is
    // up — where the profile steps, a circle on the rail.
    if state.tool == Tool::Gradient
        && let Selection::Edge(i) = state.selection
        && let (Some(source), Some(edge)) = (line.source.edges.get(i), line.net.edges().get(i))
    {
        let radius = (focus.height * 0.010).max(3.5) as f32;
        for (s, _) in &source.grade {
            ground_circle(&mut gizmos, &origin.0, edge.eval(*s).pos, radius, accent);
        }
    }
    // Gradient chevrons while the gradient tool is up, after the World
    // Editor's slope arrows: a V on the rail every 60 m pointing uphill, on
    // every graded stretch of the line — a level one draws nothing, so the
    // picture is where the line climbs, not that it exists.
    if state.tool == Tool::Gradient {
        let amber = Color::srgb(0.89, 0.71, 0.30);
        let arm = (focus.height * 0.006).max(2.5);
        for (source, edge) in line.source.edges.iter().zip(line.net.edges()) {
            for (k, (start, grade)) in source.grade.iter().enumerate() {
                if *grade == 0.0 {
                    continue;
                }
                let end = source
                    .grade
                    .get(k + 1)
                    .map_or(edge.length(), |(s, _)| *s)
                    .min(edge.length());
                let mut s = *start + 30.0;
                while s < end {
                    chevron(
                        &mut gizmos,
                        &origin.0,
                        edge.eval(s),
                        grade.signum(),
                        arm,
                        amber,
                    );
                    s += 60.0;
                }
            }
        }
    }
    // Where the device tool would stamp: the snap point on the rail and a
    // tick along the track — the click lands here, not under the pointer.
    if state.tool == Tool::PlaceDevice
        && let Some(p) = cursor
        && let Some((edge, s, distance)) = nearest_on_network(&line.net, p)
        && distance <= pick_radius(&focus)
        && let Some(edge) = line.net.edges().get(edge)
    {
        let pose = edge.eval(s);
        let radius = (focus.height * 0.010).max(3.5) as f32;
        ground_circle(&mut gizmos, &origin.0, pose.pos, radius, accent);
        let base = origin.0.to_render(pose.pos) + origin.0.dir_to_render(pose.up) * MARK_LIFT;
        let dir = origin.0.dir_to_render(pose.tangent) * (radius * 2.0);
        gizmos.line(base - dir, base + dir, accent);
    }
    if let Some(drawing) = &state.drawing {
        // While Ctrl holds the piece straight the preview turns yellow, as the
        // World Editor's frame does; while the standing end is aimed, the
        // arrow shows the heading the drag has set.
        let color = if drawing.straight && drawing.heading_deg.is_some() {
            Color::srgb(0.89, 0.71, 0.30)
        } else {
            accent
        };
        if drawing.aiming {
            if let Some([from, to]) = drawing.aim_arrow(&origin.0, pick_radius(&focus) * 2.5) {
                gizmos.line(from, to, color);
            }
        } else {
            let target = cursor.map(|p| {
                lay_target(
                    &state.open_ends,
                    drawing,
                    snap_ghost(p, &ghost, &focus),
                    &focus,
                )
            });
            // The preview lies where the finish will put the piece: on the
            // ground while the terrain snap is on.
            let terrain_h = |p: EcefPos| ground.view.ground_height(p);
            let follow: Option<&dyn Fn(EcefPos) -> Option<f64>> = if state.lay.snap_terrain {
                Some(&terrain_h)
            } else {
                None
            };
            gizmos.linestrip(drawing.polyline(target, &origin.0, follow), color);
        }
    }
    // Footpaths and walk areas, with the way being drawn.
    crate::walkways::draw(
        &mut gizmos,
        &line,
        &origin.0,
        &focus,
        &ground.marks,
        &state,
        cursor,
    );
    // Fields: their outlines, and the one being drawn by hand.
    crate::fields::draw_outlines(&mut gizmos, &line, &state, &focus, &origin.0);
    if state.tool == Tool::PlaceField && !state.walk_points.is_empty() {
        let points = state.walk_points.iter().copied().chain(cursor).map(|p| {
            let up = origin.0.dir_to_render(EnuFrame::at(p).up);
            origin.0.to_render(p) + up
        });
        gizmos.linestrip(points, Color::srgb(0.44, 0.68, 0.32));
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
    // The same for the terrain tools — the footprint of the stroke to come,
    // in the colour the laid stroke will wear: warm raising, cold lowering,
    // grey levelling.
    let footprint = match state.tool {
        Tool::TerrainRaise => Some(Color::srgb(0.90, 0.55, 0.30)),
        Tool::TerrainLower => Some(Color::srgb(0.35, 0.60, 0.90)),
        Tool::TerrainLevel | Tool::TerrainRail => Some(Color::srgb(0.65, 0.65, 0.70)),
        _ => None,
    };
    if let Some(color) = footprint
        && let Some(p) = cursor
    {
        let radius = state.terrain_radius.unwrap_or(60.0) as f32;
        ground_circle(&mut gizmos, &origin.0, p, radius, color);
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
            let corners = tile_corners(key, options, grid_height).map(|p| {
                let up = origin.0.dir_to_render(EnuFrame::at(p).up);
                origin.0.to_render(p) + up * MARK_LIFT
            });
            gizmos.linestrip(corners, color);
        }
    }
}

/// The model at the cursor before the click — the World Editor shows the
/// thing about to be placed, not a bare pointer. One entity, respawned when
/// the picked model changes, moved every frame, hidden when nothing snaps.
#[derive(Resource, Default)]
pub struct GhostPreview {
    entity: Option<Entity>,
    /// Model file the entity was spawned with.
    model: Option<String>,
}

/// Marks the preview entity, so the transform query cannot catch anything else.
#[derive(Component)]
pub struct GhostModel;

/// Keeps the placement preview under the cursor: the object tool's model on
/// its track snap with the spec's own offset and rotation, the tree tool's
/// species upright on the ground. Placeholder trees have no model file and
/// draw nothing — the click is the preview there, as it always was.
#[allow(clippy::too_many_arguments)]
pub fn placement_preview(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut preview: ResMut<GhostPreview>,
    state: Res<EditorState>,
    line: Res<Line>,
    objects: Res<TrackObjects>,
    origin: Res<Origin>,
    focus: Res<Focus>,
    windows: Query<&Window, With<PrimaryWindow>>,
    camera: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut ghost: Query<(&mut Transform, &mut Visibility), With<GhostModel>>,
) {
    // What the active tool would place, by its model file.
    let name = match state.tool {
        Tool::PlaceObject => state
            .object
            .clone()
            .or_else(|| objects.map.keys().next().cloned()),
        Tool::PlaceTree => state.tree_object.clone(),
        _ => None,
    };
    let spec = name.as_deref().and_then(|name| objects.map.get(name));
    let model = spec.map(|spec| spec.model.clone());

    // Respawn when the model changes; despawn when no tool wants one.
    if preview.model != model {
        if let Some(entity) = preview.entity.take() {
            commands.entity(entity).despawn();
        }
        if let Some(model) = &model {
            let scene: Handle<WorldAsset> =
                assets.load(GltfAssetLabel::Scene(0).from_asset(world_render::asset_path(model)));
            preview.entity = Some(
                commands
                    .spawn((
                        WorldAssetRoot(scene),
                        Transform::default(),
                        Visibility::Hidden,
                        GhostModel,
                    ))
                    .id(),
            );
        }
        preview.model = model;
    }
    let Some(entity) = preview.entity else {
        return;
    };
    let Ok((mut transform, mut visibility)) = ghost.get_mut(entity) else {
        return;
    };

    let cursor = windows
        .single()
        .ok()
        .and_then(|w| w.cursor_position())
        .filter(|c| state.over_viewport(*c))
        .and_then(|c| {
            let (camera, camera_transform) = camera.single().ok()?;
            pick_ground(camera, camera_transform, c, &origin.0, &focus)
        });
    // The pose the click would stamp — the same maths as the placement and
    // the tile scatter, so the ghost stands exactly where the object will.
    let pose = cursor.and_then(|p| match state.tool {
        Tool::PlaceObject => {
            let spec = spec?;
            let (edge, s, distance) = nearest_on_network(&line.net, p)?;
            if distance > pick_radius(&focus) {
                return None;
            }
            let pose = line.net.edges().get(edge)?.eval(s);
            let right = pose.tangent.cross(pose.up).normalize_or_zero();
            // Terrain snap resolves against the height grid at build time;
            // the preview stands on the rail plane, which is where the eye
            // checks the spot anyway.
            let base = EcefPos(pose.pos.0 + right * spec.lateral_offset + pose.up * spec.height);
            let dir = DQuat::from_axis_angle(pose.up, -spec.yaw_deg.to_radians()) * pose.tangent;
            Some((base, dir, pose.up))
        }
        Tool::PlaceTree => {
            let frame = EnuFrame::at(p);
            Some((p, frame.north, frame.up))
        }
        _ => None,
    });
    match pose {
        Some((base, dir, up)) => {
            // The model's frame in render axes: forward along `dir`, up the
            // local vertical — `scatter_objects`' own construction, Bevy's
            // -Z = forward convention included.
            let f = origin.0.dir_to_render(dir).normalize_or_zero();
            let u = origin.0.dir_to_render(up).normalize_or_zero();
            let right = f.cross(u).normalize_or_zero();
            let rotation = if right.length_squared() < 0.5 {
                Quat::IDENTITY
            } else {
                Quat::from_mat3(&Mat3::from_cols(right, right.cross(f), -f))
            };
            *transform =
                Transform::from_translation(origin.0.to_render(base)).with_rotation(rotation);
            *visibility = Visibility::Visible;
        }
        None => *visibility = Visibility::Hidden,
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
        drawing.click(Target::free(ne));
        drawing.click(Target::free(bend));
        assert_eq!(drawing.segments.len(), 2);

        let mut line = content::LineSource {
            name: "drawn".into(),
            geoid_offset: 46.0,
            electrification: track_model::PowerSystem::Ac15kv.id().to_string(),
            nodes: vec![],
            edges: vec![],
            devices: vec![],
            objects: vec![],
            trees: vec![],
            markers: vec![],
            terrain: vec![],
            heights: vec![],
            sections: vec![],
            areas: Vec::new(),
            signals: vec![],
            routes: vec![],
            boundaries: vec![],
            script: None,
            ..Default::default()
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
            terrain_change: Default::default(),
            recenter: false,
            issues: Vec::new(),
        };
        assert!(finish_drawing(&mut doc, &mut state, None));
        line = doc.source;
        let compiled = line.compile().expect("compiles");
        let end = compiled.net.edges()[0].end_pose().pos;
        assert!(
            end.distance(bend) < 0.5,
            "end missed the click by {} m",
            end.distance(bend)
        );
    }

    /// Terrain snap: the laid piece follows the sampled ground — the heights
    /// become a grade profile, and the free start drops onto the surface.
    #[test]
    fn terrain_snap_follows_the_ground() {
        let start = world_coords::geo::to_ecef_deg(52.0, 10.0, 146.0);
        let mut drawing = Drawing::start_at(start, 46.0);
        let frame = EnuFrame::at(start);
        drawing.click(Target::free(frame.to_ecef(DVec3::new(400.0, 0.0, 0.0))));
        drawing.click(Target::free(frame.to_ecef(DVec3::new(800.0, 0.0, 0.0))));
        let mut state = EditorState {
            drawing: Some(drawing),
            ..Default::default()
        };
        state.lay.snap_terrain = true;
        let mut doc = Line {
            source: content::LineSource {
                name: "drawn".into(),
                geoid_offset: 46.0,
                electrification: track_model::PowerSystem::Ac15kv.id().to_string(),
                ..Default::default()
            },
            net: track_model::TrackNetwork::new(),
            path: None,
            dirty: false,
            needs_rebuild: false,
            terrain_change: Default::default(),
            recenter: false,
            issues: Vec::new(),
        };
        // The ground: an 8 ‰ eastward slope, 130 m ellipsoidal at the start.
        let ground = |p: EcefPos| Some(130.0 + 0.008 * frame.to_local(p).x);
        assert!(finish_drawing(&mut doc, &mut state, Some(&ground)));

        let edge = &doc.source.edges[0];
        assert!(!edge.grade.is_empty());
        for (s, g) in &edge.grade {
            assert!((g - 8.0).abs() < 0.1, "grade {g} ‰ at {s} m");
        }
        // The start dropped onto the ground — stored above the geoid.
        let content::route::EdgeStart::Geo { point, .. } = &edge.start else {
            panic!("free start stays geo-anchored");
        };
        assert!(
            (point.height - (130.0 - 46.0)).abs() < 0.05,
            "{}",
            point.height
        );
        // Integrated over 800 m the profile climbs onto the slope's far end.
        let compiled = doc.source.compile().expect("compiles");
        let end = world_coords::geo::from_ecef(compiled.net.edges()[0].end_pose().pos).2;
        assert!((end - (130.0 + 0.008 * 800.0)).abs() < 0.2, "{end}");
    }

    /// The lay preview lies where the finish will put the piece: a free
    /// start stands on the ground with the rest, a glued start keeps its
    /// height and blends onto the ground over the first sample interval.
    #[test]
    fn the_preview_lies_on_the_ground() {
        let start = world_coords::geo::to_ecef_deg(52.0, 10.0, 100.0);
        let frame = EnuFrame::at(start);
        let origin = RenderOrigin::new(start);
        // Terrain 10 m above the drawing plane, everywhere.
        let ground = |_: EcefPos| Some(110.0);

        let mut free = Drawing::start_at(start, 46.0);
        free.click(Target::free(frame.to_ecef(DVec3::new(400.0, 0.0, 0.0))));
        let points = free.polyline(None, &origin, Some(&ground));
        assert!((points[0].y - 10.5).abs() < 0.6, "{}", points[0].y);
        assert!((points.last().unwrap().y - 10.5).abs() < 0.6);

        let mut glued = Drawing::continue_from(
            OpenEnd {
                node: 0,
                edge: 0,
                at_end: true,
                pos: start,
                heading: 0.0,
                curvature: 0.0,
            },
            46.0,
        );
        glued.click(Target::free(frame.to_ecef(DVec3::new(400.0, 0.0, 0.0))));
        let points = glued.polyline(None, &origin, Some(&ground));
        // Start at the old track's height, on the ground from 20 m on.
        assert!((points[0].y - 0.5).abs() < 0.6, "{}", points[0].y);
        assert!((points[4].y - 10.5).abs() < 0.6, "{}", points[4].y);
        assert!((points.last().unwrap().y - 10.5).abs() < 0.6);
    }

    /// The circle selection marks the point things inside — a device
    /// included — and the bulk delete removes them all in one step, with
    /// the signal references cleaned up so the result still compiles.
    #[test]
    fn circle_selection_marks_and_deletes() {
        let source = content::musterbahn();
        let net = source.compile().unwrap().net;
        let mut doc = Line {
            source,
            net,
            path: None,
            dirty: false,
            needs_rebuild: false,
            terrain_change: Default::default(),
            recenter: false,
            issues: Vec::new(),
        };
        let mut state = EditorState::default();
        let center = device_pos(&doc.net, &doc.source.devices[0]).unwrap();
        mark_circle(&mut state, &doc, &Marks::default(), center, 15.0);
        assert!(state.marked.contains(&Mark::Device(0)), "device 0 caught");

        let devices = doc.source.devices.len();
        let caught = state
            .marked
            .iter()
            .filter(|m| matches!(m, Mark::Device(_)))
            .count();
        delete_marked(&mut doc, &mut state);
        assert!(state.marked.is_empty());
        assert_eq!(doc.source.devices.len(), devices - caught);
        assert!(doc.source.compile().is_ok(), "signal refs cleaned up");
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
            terrain_change: Default::default(),
            recenter: false,
            issues: Vec::new(),
        };

        // Branch off edge 0 at km 1.5, curving away to the left.
        let pose = doc.net.edges()[0].eval(1500.0);
        let mut drawing = Drawing::branch_at(&pose, doc.source.geoid_offset, 0, 1500.0, false);
        let frame = EnuFrame::at(pose.pos);
        let tangent = frame.dir_to_local(pose.tangent);
        let left = DVec3::new(-tangent.y, tangent.x, 0.0);
        drawing.click(Target::free(frame.to_ecef(tangent * 400.0 + left * 60.0)));
        assert_eq!(drawing.segments.len(), 1);
        let mut state = EditorState {
            drawing: Some(drawing),
            ..Default::default()
        };
        assert!(finish_drawing(&mut doc, &mut state, None));

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
            terrain_change: Default::default(),
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
        drawing.click(Target::free(frame.to_ecef(-tangent * 400.0 + left * 60.0)));
        let mut state = EditorState {
            drawing: Some(drawing),
            ..Default::default()
        };
        assert!(finish_drawing(&mut doc, &mut state, None));

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

    /// A click on open water picks the body, a click on its island misses,
    /// and a click on dry land finds nothing — the last body in the file
    /// wins where two lie on top of each other.
    #[test]
    fn a_click_picks_the_water_it_lands_on() {
        let point = |lat: f64, lon: f64| content::route::WaterPoint { lat, lon };
        let lake = content::route::WaterSource {
            name: "See".into(),
            polygon: vec![
                point(52.000, 10.000),
                point(52.000, 10.020),
                point(52.020, 10.020),
                point(52.020, 10.000),
            ],
            holes: vec![vec![
                point(52.008, 10.008),
                point(52.008, 10.012),
                point(52.012, 10.012),
                point(52.012, 10.008),
            ]],
            tags: vec!["water".into()],
        };
        let pond = content::route::WaterSource {
            name: "Teich".into(),
            polygon: vec![
                point(52.004, 10.014),
                point(52.004, 10.018),
                point(52.008, 10.018),
                point(52.008, 10.014),
            ],
            holes: Vec::new(),
            tags: vec!["water".into()],
        };
        let mut source = straight_east(1000.0);
        source.waters = vec![lake, pond];
        let line = line_of(source);

        // Open water of the lake, clear of pond and island.
        assert!(matches!(
            pick_water_at(&line, 52.018, 10.004),
            Some(Selection::Water(0))
        ));
        // The island is the lake's hole: no water.
        assert!(pick_water_at(&line, 52.010, 10.010).is_none());
        // The pond lies over the lake's corner and wins the click.
        assert!(matches!(
            pick_water_at(&line, 52.006, 10.016),
            Some(Selection::Water(1))
        ));
        // Dry land beside everything.
        assert!(pick_water_at(&line, 52.030, 10.030).is_none());
    }

    fn line_of(source: content::LineSource) -> Line {
        let net = source.compile().unwrap().net;
        Line {
            source,
            net,
            path: None,
            dirty: false,
            needs_rebuild: false,
            terrain_change: Default::default(),
            recenter: false,
            issues: Vec::new(),
        }
    }

    /// One straight edge running east — the bench for the track tools.
    fn straight_east(length: f64) -> content::LineSource {
        content::LineSource {
            name: "bench".into(),
            nodes: vec![NodeSource::Buffer, NodeSource::Buffer],
            edges: vec![EdgeSource {
                from: 0,
                to: 1,
                start: EdgeStart::Geo {
                    point: GeoPoint {
                        lat: 52.0,
                        lon: 10.0,
                        height: 100.0,
                    },
                    heading_deg: 90.0,
                },
                segments: vec![Segment::straight(length)],
                grade: vec![],
                cant: vec![],
                speed: vec![],
                track_type: vec![],
                electrification: vec![],
                formation: true,
            }],
            ..Default::default()
        }
    }

    /// The biarc lands on the far end with the asked-for heading — tangent
    /// at both sides, which is what makes it a join and not a kink.
    #[test]
    fn a_biarc_arrives_tangentially() {
        let to = DVec2::new(200.0, 60.0);
        let [(a, mid), (b, end)] = biarc(DVec2::ZERO, 0.0, to, 0.0).expect("solvable");
        let (p1, h1) = advance(DVec2::ZERO, 0.0, &a, a.len);
        assert!((h1 - mid).abs() < 1e-9);
        let (p2, h2) = advance(p1, h1, &b, b.len);
        assert!(p2.distance(to) < 1e-6, "missed by {}", p2.distance(to));
        assert!(h2.abs() < 1e-9, "arrived at {} rad", h2);
        assert!((end - h2).abs() < 1e-9);
    }

    /// Joining two open ends with a gap lays a connecting piece between the
    /// nodes; collinear ends get what amounts to a straight.
    #[test]
    fn joining_distant_ends_lays_a_connecting_piece() {
        let mut source = straight_east(300.0);
        // A second straight further east, leaving a 100 m gap.
        let start = source.compile().unwrap().net.edges()[0].end_pose();
        let ahead = EcefPos(start.pos.0 + start.tangent * 100.0);
        let (lat, lon, height) = geo::from_ecef(ahead);
        source.nodes.push(NodeSource::Buffer);
        source.nodes.push(NodeSource::Buffer);
        source.edges.push(EdgeSource {
            from: 2,
            to: 3,
            start: EdgeStart::Geo {
                point: GeoPoint {
                    lat: lat.to_degrees(),
                    lon: lon.to_degrees(),
                    height: height - source.geoid_offset,
                },
                heading_deg: 90.0,
            },
            segments: vec![Segment::straight(300.0)],
            grade: vec![],
            cant: vec![],
            speed: vec![],
            track_type: vec![],
            electrification: vec![],
            formation: true,
        });
        let mut doc = line_of(source);
        let ends = open_ends(&doc);
        assert_eq!(ends.len(), 4);
        let a = *ends.iter().find(|e| e.edge == 0 && e.at_end).unwrap();
        let b = *ends.iter().find(|e| e.edge == 1 && !e.at_end).unwrap();
        join_ends(
            &mut doc,
            &LayOptions::default(),
            &crate::stake::StakeOptions::default(),
            a,
            b,
        )
        .expect("joins");
        let compiled = doc.source.compile().expect("still compiles");
        assert_eq!(doc.source.edges.len(), 3);
        // The connecting piece runs from A's end to B's start, gap-free.
        let joint = compiled.net.edges()[2].end_pose();
        let b_start = compiled.net.edges()[1].eval(0.0);
        assert!(joint.pos.distance(b_start.pos) < 0.05);
        assert!(matches!(doc.source.nodes[1], NodeSource::Joint));
        assert!(matches!(doc.source.nodes[2], NodeSource::Joint));
    }

    /// Joining offset parallel ends runs the stake-out calculator: the edge
    /// it lays is a double arc with its intermediate straight, carries the
    /// cant band of both hands, and compiles.
    #[test]
    fn joining_offset_ends_stakes_out_a_double_arc() {
        let mut source = straight_east(300.0);
        // A parallel track further east and 60 m north, running the same way:
        // its start faces A's end across a 600 m gap with a 60 m offset.
        let start = source.compile().unwrap().net.edges()[0].eval(0.0);
        let left = start.up.cross(start.tangent).normalize();
        let shifted = EcefPos(start.pos.0 + start.tangent * 900.0 + left * 60.0);
        let (lat, lon, height) = geo::from_ecef(shifted);
        source.nodes.push(NodeSource::Buffer);
        source.nodes.push(NodeSource::Buffer);
        source.edges.push(EdgeSource {
            from: 2,
            to: 3,
            start: EdgeStart::Geo {
                point: GeoPoint {
                    lat: lat.to_degrees(),
                    lon: lon.to_degrees(),
                    height: height - source.geoid_offset,
                },
                heading_deg: 90.0,
            },
            segments: vec![Segment::straight(300.0)],
            grade: vec![],
            cant: vec![],
            speed: vec![],
            track_type: vec![],
            electrification: vec![],
            formation: true,
        });
        let mut doc = line_of(source);
        let ends = open_ends(&doc);
        let a = *ends.iter().find(|e| e.edge == 0 && e.at_end).unwrap();
        let b = *ends.iter().find(|e| e.edge == 1 && !e.at_end).unwrap();
        let stake = crate::stake::StakeOptions {
            speed: 60.0,
            ..Default::default()
        };
        join_ends(&mut doc, &LayOptions::default(), &stake, a, b).expect("stakes out");
        let compiled = doc.source.compile().expect("still compiles");
        let connector = compiled.net.edges().last().unwrap();
        // It lands on B's start, tangentially. The chain is planned in A's
        // chord plane while `eval` runs on the curved frame — over 900 m that
        // is a few centimetres of height, the documented plane approximation.
        let b_start = compiled.net.edges()[1].eval(0.0);
        assert!(connector.end_pose().pos.distance(b_start.pos) < 0.25);
        assert!(connector.end_pose().tangent.dot(b_start.tangent) > 0.999_9);
        // An S of two hands, with the cant band signed to match.
        let cant = &doc.source.edges.last().unwrap().cant;
        assert!(!cant.is_empty(), "the calculator writes the cant band");
        let peak = cant.iter().map(|(_, c)| *c).fold(0.0, f64::max);
        let low = cant.iter().map(|(_, c)| *c).fold(0.0, f64::min);
        assert!(peak > 0.0 && low < 0.0, "{peak} / {low}");
        assert!(matches!(doc.source.nodes[1], NodeSource::Joint));
        assert!(matches!(doc.source.nodes[2], NodeSource::Joint));
    }

    /// Ends already on one point are welded: one node from then on, and the
    /// dropped node's index is remapped everywhere.
    #[test]
    fn joining_touching_ends_welds_the_nodes() {
        let mut source = straight_east(300.0);
        let end = source.compile().unwrap().net.edges()[0].end_pose();
        let (lat, lon, height) = geo::from_ecef(end.pos);
        source.nodes.push(NodeSource::Buffer);
        source.nodes.push(NodeSource::Buffer);
        source.edges.push(EdgeSource {
            from: 2,
            to: 3,
            start: EdgeStart::Geo {
                point: GeoPoint {
                    lat: lat.to_degrees(),
                    lon: lon.to_degrees(),
                    height: height - source.geoid_offset,
                },
                heading_deg: 90.0,
            },
            segments: vec![Segment::straight(300.0)],
            grade: vec![],
            cant: vec![],
            speed: vec![],
            track_type: vec![],
            electrification: vec![],
            formation: true,
        });
        let mut doc = line_of(source);
        let ends = open_ends(&doc);
        let a = *ends.iter().find(|e| e.edge == 0 && e.at_end).unwrap();
        let b = *ends.iter().find(|e| e.edge == 1 && !e.at_end).unwrap();
        join_ends(
            &mut doc,
            &LayOptions::default(),
            &crate::stake::StakeOptions::default(),
            a,
            b,
        )
        .expect("welds");
        assert_eq!(doc.source.nodes.len(), 3, "one node gone");
        assert_eq!(doc.source.edges.len(), 2, "no piece needed");
        assert_eq!(doc.source.edges[1].from, doc.source.edges[0].to);
        assert!(matches!(
            doc.source.nodes[doc.source.edges[0].to as usize],
            NodeSource::Joint
        ));
        doc.source.compile().expect("still compiles");
    }

    /// The offset tool lays a parallel: same heading, the asked-for distance,
    /// carried over the whole length.
    #[test]
    fn an_offset_track_runs_parallel() {
        let mut doc = line_of(content::musterbahn());
        let new = offset_edge(&mut doc, 0, 4.0).expect("offsets");
        let compiled = doc.source.compile().expect("still compiles");
        let (base, parallel) = (&compiled.net.edges()[0], &compiled.net.edges()[new]);
        for fraction in [0.0, 0.3, 0.7, 1.0] {
            let p = parallel.eval(parallel.length() * fraction).pos;
            let (_, d) = nearest_on_edge(base, p);
            assert!(
                (d - 4.0).abs() < 0.15,
                "spacing {d} m at fraction {fraction}"
            );
        }
    }

    /// A crossover cuts both tracks and wires two turnouts with the S of
    /// their arcs between them — and the diagonal lands tangentially.
    #[test]
    fn a_crossover_connects_two_parallel_tracks() {
        let mut doc = line_of(straight_east(600.0));
        offset_edge(&mut doc, 0, -4.0).expect("parallel");
        doc.net = doc.source.compile().unwrap().net;
        crossover(&mut doc, &LayOptions::default(), 0, 200.0, 1, 190.0).expect("builds");
        let compiled = doc.source.compile().expect("still compiles");
        let switches = doc
            .source
            .nodes
            .iter()
            .filter(|n| matches!(n, NodeSource::Switch { .. }))
            .count();
        assert_eq!(switches, 2);
        // The diagonal is the last edge; it leaves track A tangentially and
        // arrives on track B parallel to it.
        let diagonal = compiled.net.edges().last().unwrap();
        let a_dir = compiled.net.edges()[0].end_pose().tangent;
        assert!(diagonal.eval(0.0).tangent.dot(a_dir) > 0.999_9);
        assert!(diagonal.end_pose().tangent.dot(a_dir) > 0.999_9);
        // It actually reaches the other track.
        let b_first = &compiled.net.edges()[1];
        let (_, miss) = nearest_on_edge(b_first, diagonal.end_pose().pos);
        assert!(miss < 0.5, "diagonal misses track B by {miss} m");
    }

    /// A drawing bench: one straight click east fixes the heading, so the
    /// next click is the curve under test.
    fn eased_drawing(speed: f64) -> (Drawing, EnuFrame) {
        let start = world_coords::geo::to_ecef_deg(52.0, 10.0, 100.0);
        let mut drawing = Drawing::start_at(start, 46.0);
        drawing.easements = Some(Easements {
            rules: CantRules::default(),
            speed,
        });
        let frame = EnuFrame::at(start);
        drawing.click(Target::free(frame.to_ecef(DVec3::new(500.0, 0.0, 0.0))));
        (drawing, frame)
    }

    /// With easements on, a clicked curve comes out as clothoid – arc –
    /// clothoid: curvature-continuous, the ramps at the rulebook length, and
    /// the chain still ends on the clicked point.
    #[test]
    fn an_eased_curve_is_clothoid_arc_clothoid_on_the_click() {
        let (mut drawing, frame) = eased_drawing(160.0);
        let target = frame.to_ecef(DVec3::new(1500.0, 300.0, 0.0));
        drawing.click(Target::free(target));

        assert_eq!(drawing.segments.len(), 4, "straight + in + arc + out");
        let [_, t_in, arc, t_out] = drawing.segments[..] else {
            panic!("shape");
        };
        assert!(t_in.dk != 0.0 && t_out.dk != 0.0 && arc.dk == 0.0);
        // Curvature runs 0 → k → k → 0 without a jump.
        assert!(t_in.k0.abs() < 1e-12);
        assert!((t_in.end_curvature() - arc.k0).abs() < 1e-9);
        assert!((t_out.k0 - arc.k0).abs() < 1e-9);
        assert!(t_out.end_curvature().abs() < 1e-9);
        // Left-hand curve, positive cant at the rulebook value, ramps to match.
        let e = drawing.easements.unwrap();
        let cant = signed_cant(arc.k0, e);
        assert!(
            arc.k0 > 0.0 && cant > 0.0,
            "left curve carries positive cant"
        );
        assert!((t_in.len - e.rules.ramp_length(cant, e.speed)).abs() < 1e-6);
        // The chain still passes through the click.
        let mut p = DVec2::ZERO;
        let mut h = 0.0;
        for seg in &drawing.segments {
            let (q, g) = advance(p, h, seg, seg.len);
            p = q;
            h = g;
        }
        let local = frame.to_local(target);
        assert!(
            p.distance(DVec2::new(local.x, local.y)) < 0.05,
            "missed the click by {} m",
            p.distance(DVec2::new(local.x, local.y))
        );
        // The cant band ramps up under the transitions and back to zero.
        let peak = drawing
            .cant_steps
            .iter()
            .map(|(_, c)| *c)
            .fold(0.0, f64::max);
        assert!((peak - cant).abs() < 1e-9, "peak {peak} vs {cant}");
        assert_eq!(drawing.cant_steps.first().unwrap().1, 0.0);
        assert_eq!(drawing.cant_steps.last().unwrap().1, 0.0);
    }

    /// A right-hand curve carries its cant as a negative number — that is
    /// what rolls the track toward the inside in `TrackEdge::eval`.
    #[test]
    fn a_right_hand_eased_curve_carries_negative_cant() {
        let (mut drawing, frame) = eased_drawing(160.0);
        drawing.click(Target::free(frame.to_ecef(DVec3::new(1500.0, -300.0, 0.0))));
        let low = drawing
            .cant_steps
            .iter()
            .map(|(_, c)| *c)
            .fold(0.0, f64::min);
        assert!(low < -40.0, "right-hand cant must be negative: {low}");
    }

    /// Where the click leaves no room for the ramps, the piece falls back to
    /// the bare arc instead of inventing a distorted chain.
    #[test]
    fn a_curve_too_short_for_ramps_stays_a_bare_arc() {
        let (mut drawing, frame) = eased_drawing(160.0);
        drawing.click(Target::free(frame.to_ecef(DVec3::new(530.0, 3.0, 0.0))));
        assert_eq!(drawing.segments.len(), 2, "straight + bare arc");
        assert!(drawing.segments.iter().all(|s| s.dk == 0.0));
        assert!(drawing.cant_steps.is_empty());
    }

    /// Easements and the radius snap together: the arc lands on the standard
    /// series and the running end slides along instead of holding the click.
    #[test]
    fn an_eased_arc_snaps_to_the_standard_series() {
        let (mut drawing, frame) = eased_drawing(160.0);
        drawing.radii = content::import::alignment::preferred_radii();
        drawing.click(Target::free(frame.to_ecef(DVec3::new(1500.0, 300.0, 0.0))));
        let arc = drawing
            .segments
            .iter()
            .find(|s| s.dk == 0.0 && s.k0.abs() > 1e-9)
            .expect("an arc");
        let radius = 1.0 / arc.k0.abs();
        assert!(
            drawing.radii.iter().any(|r| (r - radius).abs() < 1e-6),
            "radius {radius} not on the series"
        );
    }

    /// Finishing an eased drawing writes the cant into the edge, and the
    /// compiled track rolls by it mid-curve while the straight stays level.
    #[test]
    fn an_eased_finish_writes_the_cant_profile() {
        let (mut drawing, frame) = eased_drawing(160.0);
        drawing.click(Target::free(frame.to_ecef(DVec3::new(1500.0, 300.0, 0.0))));
        let expected = drawing
            .cant_steps
            .iter()
            .map(|(_, c)| *c)
            .fold(0.0, f64::max);
        let ramp_start = drawing.segments[0].len;
        let arc_mid = ramp_start + drawing.segments[1].len + drawing.segments[2].len / 2.0;

        let mut doc = line_of(content::LineSource {
            name: "eased".into(),
            ..Default::default()
        });
        let mut state = EditorState {
            drawing: Some(drawing),
            ..Default::default()
        };
        assert!(finish_drawing(&mut doc, &mut state, None));
        let edge = &doc.source.edges[0];
        assert!(!edge.cant.is_empty(), "the cant band is in the file");
        let compiled = doc.source.compile().expect("compiles");
        let track = &compiled.net.edges()[0];
        assert!((track.eval(arc_mid).cant - expected).abs() < 1e-6);
        assert_eq!(track.eval(100.0).cant, 0.0, "the straight stays level");
    }

    /// The radius snap rounds an arc onto the standard series and keeps its
    /// change of heading, so the alignment stays tangent-continuous.
    #[test]
    fn arcs_snap_to_standard_radii() {
        let arc = Segment::arc(400.0, 823.0);
        let snapped = snap_radius(arc, &content::import::alignment::preferred_radii());
        assert!((1.0 / snapped.k0 - 800.0).abs() < 1e-9);
        assert!(
            (snapped.heading_delta(snapped.len) - arc.heading_delta(arc.len)).abs() < 1e-9,
            "the turn must survive the snap"
        );
        // Straights pass unchanged.
        let straight = Segment::straight(100.0);
        assert_eq!(
            snap_radius(straight, &content::import::alignment::preferred_radii()),
            straight
        );
    }

    /// Continuing from an open end and landing on another one shares their
    /// nodes: the ends close into joints and the geometry chains exactly.
    #[test]
    fn laying_between_open_ends_closes_them() {
        let mut source = straight_east(300.0);
        // A parallel track 40 m north, running the same way.
        let start = source.compile().unwrap().net.edges()[0].eval(0.0);
        let left = start.up.cross(start.tangent).normalize();
        let shifted = EcefPos(start.pos.0 + left * 40.0);
        let (lat, lon, height) = geo::from_ecef(shifted);
        source.nodes.push(NodeSource::Buffer);
        source.nodes.push(NodeSource::Buffer);
        source.edges.push(EdgeSource {
            from: 2,
            to: 3,
            start: EdgeStart::Geo {
                point: GeoPoint {
                    lat: lat.to_degrees(),
                    lon: lon.to_degrees(),
                    height: height - source.geoid_offset,
                },
                heading_deg: 90.0,
            },
            segments: vec![Segment::straight(300.0)],
            grade: vec![],
            cant: vec![],
            speed: vec![],
            track_type: vec![],
            electrification: vec![],
            formation: true,
        });
        let mut doc = line_of(source);
        let ends = open_ends(&doc);
        let from = *ends.iter().find(|e| e.edge == 0 && e.at_end).unwrap();
        let to = *ends.iter().find(|e| e.edge == 1 && e.at_end).unwrap();
        let mut drawing = Drawing::continue_from(from, doc.source.geoid_offset);
        drawing.click(Target {
            pos: to.pos,
            end: Some(to),
        });
        assert!(drawing.to_end.is_some(), "the click lands on the end");
        let mut state = EditorState {
            drawing: Some(drawing),
            ..Default::default()
        };
        assert!(finish_drawing(&mut doc, &mut state, None));
        let compiled = doc.source.compile().expect("still compiles");
        // Both former buffers are joints now, and the new edge closes the gap.
        assert!(matches!(doc.source.nodes[1], NodeSource::Joint));
        assert!(matches!(doc.source.nodes[3], NodeSource::Joint));
        let new = compiled.net.edges().last().unwrap();
        assert!(new.end_pose().pos.distance(to.pos) < 0.05);
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
            terrain_change: Default::default(),
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
            terrain_change: Default::default(),
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

#[cfg(test)]
mod brush_tests {
    use super::*;
    use content::route::{AreaSpan, TrackAreaSource};

    fn line() -> Line {
        let source = content::musterbahn();
        Line {
            net: source.compile().expect("compiles").net,
            source,
            path: None,
            dirty: false,
            needs_rebuild: false,
            terrain_change: Default::default(),
            recenter: false,
            issues: Vec::new(),
        }
    }

    #[test]
    fn a_stroke_painted_backwards_is_the_same_stroke() {
        let back = AreaStroke {
            edge: 2,
            from: 900.0,
            to: 300.0,
        };
        assert_eq!(back.length(), 600.0);
        let span = back.span();
        assert_eq!(span.edge, 2);
        assert_eq!(span.from, 300.0);
        assert_eq!(span.to, 900.0);
    }

    #[test]
    fn laying_a_stroke_down_opens_an_area_and_selects_it() {
        let mut line = line();
        let mut state = EditorState::default();
        let stroke = AreaStroke {
            edge: 0,
            from: 200.0,
            to: 800.0,
        };
        assert!(commit_stroke(&mut line, &mut state, stroke).is_none());
        assert_eq!(line.source.areas.len(), 1);
        assert_eq!(
            line.source.areas[0].spans,
            vec![AreaSpan::new(0, 200.0, 800.0)]
        );
        assert_eq!(state.selection, Selection::TrackArea(0));
        // It is painted at the width the brush is set to.
        assert_eq!(line.source.areas[0].width, AREA_WIDTH);
    }

    #[test]
    fn the_next_stroke_joins_the_selected_area() {
        let mut line = line();
        let mut state = EditorState::default();
        commit_stroke(
            &mut line,
            &mut state,
            AreaStroke {
                edge: 0,
                from: 0.0,
                to: 500.0,
            },
        );
        // Still selected, so the second stroke belongs to the same area — which is how
        // one area comes to cover a whole station, one track at a time.
        commit_stroke(
            &mut line,
            &mut state,
            AreaStroke {
                edge: 1,
                from: 100.0,
                to: 400.0,
            },
        );
        assert_eq!(line.source.areas.len(), 1);
        assert_eq!(line.source.areas[0].spans.len(), 2);
        assert_eq!(line.source.areas[0].spans[1].edge, 1);

        // With nothing selected the next one opens an area of its own.
        state.selection = Selection::None;
        commit_stroke(
            &mut line,
            &mut state,
            AreaStroke {
                edge: 2,
                from: 0.0,
                to: 300.0,
            },
        );
        assert_eq!(line.source.areas.len(), 2);
    }

    #[test]
    fn a_click_without_a_drag_paints_nothing() {
        let mut line = line();
        let mut state = EditorState::default();
        let dot = AreaStroke {
            edge: 0,
            from: 500.0,
            to: 500.2,
        };
        assert!(commit_stroke(&mut line, &mut state, dot).is_some());
        assert!(line.source.areas.is_empty());
    }

    #[test]
    fn a_new_area_wears_the_brush_width_and_keeps_it() {
        let mut line = line();
        let mut state = EditorState {
            area_width: Some(6.0),
            ..Default::default()
        };
        commit_stroke(
            &mut line,
            &mut state,
            AreaStroke {
                edge: 0,
                from: 0.0,
                to: 100.0,
            },
        );
        assert_eq!(line.source.areas[0].width, 6.0);
        // The width belongs to the area, not to the brush: a file written now reads back
        // the same width whatever the brush is set to afterwards.
        let text = ron::to_string(&line.source.areas[0]).expect("serialises");
        let back: TrackAreaSource = ron::from_str(&text).expect("parses");
        assert_eq!(back.width, 6.0);
    }
}

#[cfg(test)]
mod palette_tests {
    use super::*;

    /// The number keys count down the toolbox's active box: `1` is always
    /// the select tool, the category's own tools follow from `2`. Both sides
    /// read `TOOL_GROUPS`, and this is what says so.
    #[test]
    fn the_digits_follow_the_toolbox() {
        // Every tool sits in exactly one category — and select in none.
        let all: Vec<Tool> = TOOL_GROUPS
            .iter()
            .flat_map(|(_, _, tools)| tools.iter().map(|(tool, _, _)| *tool))
            .collect();
        for tool in &all {
            assert_eq!(all.iter().filter(|t| *t == tool).count(), 1, "{tool:?}");
        }
        assert!(!all.contains(&Tool::Select), "select belongs to every box");
        // Digits: select first, then the box's own tools.
        assert_eq!(tool_digit(Tool::Select), Some(1));
        assert_eq!(tool_digit(Tool::DrawTrack), Some(2));
        assert_eq!(tool_digit(Tool::Split), Some(3));
        assert_eq!(tool_digit(Tool::PlaceDevice), Some(2), "first of its box");
        assert_eq!(tool_digit(Tool::PlaceTree), Some(2));
        // The entry lookup answers for every tool, select included.
        assert_eq!(tool_entry(Tool::Select).0, Tool::Select);
        for tool in &all {
            assert_eq!(tool_entry(*tool).0, *tool);
        }
    }

    /// `--tool <name>` names a tool by its i18n key without the prefix — the
    /// walkway tools included, and both in the people box.
    #[test]
    fn the_tool_flag_names_the_walkway_tools() {
        assert_eq!(Tool::parse("walk-path"), Some(Tool::PlaceWalkPath));
        assert_eq!(Tool::parse("walk-area"), Some(Tool::PlaceWalkArea));
        assert_eq!(Tool::parse("select"), Some(Tool::Select));
        assert_eq!(Tool::parse("walkway"), None);
        assert_eq!(
            category_of(Tool::PlaceWalkPath),
            category_of(Tool::PlaceWalkArea)
        );
        assert_eq!(
            TOOL_GROUPS[category_of(Tool::PlaceWalkPath)].0,
            "tool-group-people"
        );
        // Switching tools drops a half-drawn way, like a half-drawn forest.
        let mut state = EditorState::default();
        state.walk_points.push(geo::to_ecef_deg(52.0, 10.0, 0.0));
        select_tool(&mut state, Tool::Select);
        assert!(state.walk_points.is_empty());
    }
}
