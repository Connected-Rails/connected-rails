//! Line source format (RON) and compiler into track network + interlocking (plan ch. 15).

use serde::{Deserialize, Serialize};
use sim_core::interlock::{
    BlockMarkerPayload, FlankGuard, Interlock, Route as IlRoute, RouteId, Signal, SignalId,
    SignalKind, SignalSystem,
};
use sim_core::safety::de::{MagnetFrequency, MagnetPayload};
use sim_core::yard::{Yard, YardKind};
use track_model::{
    DeviceKind, EdgeId, Facing, NodeId, NodeKind, Segment, StepProfile, Switch, SwitchPosition,
    TrackEdge, TrackNetwork, TrackObject, TrackType, TracksideDevice,
};
use track_model::{EdgeEnd, EdgeSide, TrackPosition};
use world_coords::geo::to_ecef_deg;

/// Georeferenced start of the line.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GeoPoint {
    pub lat: f64,
    pub lon: f64,
    /// Normal height [m] (DHHN2016).
    pub height: f64,
}

/// A corner of the module envelope.
///
/// The envelope (Zusi calls it *Hüllkurve*) is the closed polygon that bounds a
/// module. It is stored in degrees rather than metres so it survives a change of
/// UTM zone and reads as a place on a map, like every other geo-positioned entry
/// of a line.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EnvelopePoint {
    pub lat: f64,
    pub lon: f64,
}

/// A vertex of a walkway — a footpath or a place people wander about on (plan
/// ch. 12). Degrees like every other geo-positioned entry; the height is the
/// terrain's, plus what the walkway itself adds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WalkPoint {
    pub lat: f64,
    pub lon: f64,
}

/// A footpath people walk along: a polyline over the ground, walked up and down
/// by a handful of people at a time. A footbridge, the way from the forecourt to
/// the platform, the platform's own length where no platform model carries a
/// way of its own (see MODS.md, *People*).
///
/// Who walks where is never stored: the people are a function of the line, the
/// scenario clock and a seed, so every client of a run and every restart shows
/// the same people in the same places.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WalkPathSource {
    /// Free label for the editor and the rule check.
    #[serde(default)]
    pub name: String,
    /// The vertices in walking order — two at least.
    pub points: Vec<WalkPoint>,
    /// Width of the way [m]; the people spread across it.
    #[serde(default = "default_walk_width")]
    pub width: f64,
    /// How many people are on the way at a time.
    #[serde(default = "default_walk_people")]
    pub people: u32,
    /// Height of the way above the terrain [m] — a footbridge, a modelled platform.
    #[serde(default)]
    pub height: f64,
    /// Free-form tags, lower-case kebab like everywhere else.
    #[serde(default)]
    pub tags: Vec<String>,
}

/// A place people are about on: a polygon over the ground, some of its people
/// wandering between random spots inside it, the rest standing. A forecourt, a
/// waiting area, a platform.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WalkAreaSource {
    #[serde(default)]
    pub name: String,
    /// The corners in either winding — three at least.
    pub polygon: Vec<WalkPoint>,
    /// How many people are in the area.
    #[serde(default = "default_area_people")]
    pub people: u32,
    /// Share of them that wander instead of standing, 0 … 1.
    #[serde(default = "default_walking_share")]
    pub walking_share: f64,
    /// Height of the area above the terrain [m].
    #[serde(default)]
    pub height: f64,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// A corner of a field. Degrees like every other geo-positioned entry; the
/// height comes from the terrain the field is draped over.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FieldPoint {
    pub lat: f64,
    pub lon: f64,
}

/// A field: a polygon of farmland with a crop on it (plan ch. 14, and the
/// field plan in full).
///
/// Imported from the state agricultural registers by the route editor's field
/// import, or drawn by hand like a walk area. Either way it is an ordinary
/// entry of the line — one that can be moved, re-cropped and deleted, so an
/// import is a starting point rather than a black box.
///
/// What is *not* stored is what the field looks like today: the crop plus the
/// scenario's date gives the growth stage, the colour and the height through
/// `fields::phenology`, so the same module shows winter wheat green in April
/// and gold in July without holding either. That also makes it free over the
/// network — every client works the appearance out from the same three numbers
/// (see CLAUDE.md, *Multiplayer*).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldSource {
    /// The outline in either winding — three corners at least, and not closed
    /// (the last corner joins the first).
    pub polygon: Vec<FieldPoint>,
    /// What grows here, as a `fields::CropClass` id — `"winter-cereal"`,
    /// `"maize"`. An id the installed tables do not know is drawn as bare
    /// ground rather than refused.
    pub crop: String,
    /// The register's own crop code, kept verbatim. Nothing reads it; it is
    /// what a builder needs to correct a mapping, and what says where a wrong
    /// crop came from.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub code: String,
    /// The crop as the register spells it — `"Winterweichweizen"`. Shown in
    /// the editor, never matched on.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    /// How the crop came to be known: `"declared"` (the farmer applied for it),
    /// `"group"` (the register gave only the crop group and it was drawn from
    /// that group's weights) or `"drawn"` (the register gave no crop at all and
    /// it came from the regional statistics). Empty for a field drawn by hand.
    ///
    /// It is not decoration. Half the states publish the field block and no
    /// crop, so half a module's fields can be plausible guesses — and a builder
    /// correcting one by hand needs to see which those are (plan ch. 5).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub level: String,
    /// Which way the field was worked, against grid east [deg]. Furrows,
    /// tramlines and the combine's swath all run along it; the import takes it
    /// from the outline's long axis, and it can be turned by hand afterwards.
    #[serde(default)]
    pub direction_deg: f64,
    /// The state the parcel came from, as its code (`"NW"`). Empty for a field
    /// drawn by hand.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
    /// Application year the parcel was declared for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<u32>,
    /// Seed of everything that varies per field: how far through its season it
    /// is, where the rows start, how wide the working width is. Kept in the
    /// file rather than derived from the outline, so nudging a corner does not
    /// re-roll the whole field.
    #[serde(default)]
    pub seed: u64,
    /// Free-form tags, lower-case kebab like everywhere else.
    #[serde(default)]
    pub tags: Vec<String>,
}

impl FieldSource {
    /// One imported parcel as a line entry — what the field import writes,
    /// from the editor's dialog as well as from `import-module`.
    pub fn from_feature(field: &fields::FieldFeature) -> Self {
        Self {
            polygon: field
                .to_degrees()
                .into_iter()
                .map(|(lat, lon)| FieldPoint { lat, lon })
                .collect(),
            crop: field.crop.id().to_string(),
            code: field.code_raw.clone(),
            label: field.code_text.clone(),
            level: match field.level {
                fields::Level::Declared => "declared",
                fields::Level::Group => "group",
                fields::Level::Drawn => "drawn",
            }
            .to_string(),
            direction_deg: field.direction.to_degrees(),
            // `OSM` where there is no state: a module abroad still has to say
            // where its fields came from.
            source: fields::cache::origin_code(field.land).to_string(),
            year: field.year,
            seed: field.seed(),
            tags: Vec::new(),
        }
    }

    /// Middle of the outline [deg] — where the editor's list jumps to.
    pub fn centre(&self) -> (f64, f64) {
        if self.polygon.is_empty() {
            return (0.0, 0.0);
        }
        let n = self.polygon.len() as f64;
        (
            self.polygon.iter().map(|p| p.lat).sum::<f64>() / n,
            self.polygon.iter().map(|p| p.lon).sum::<f64>() / n,
        )
    }

    /// Area of the outline [m²], measured in the UTM zone `zone`.
    pub fn area(&self, zone: u8) -> f64 {
        let ring: Vec<glam::DVec2> = self
            .polygon
            .iter()
            .map(|p| {
                let (e, n) =
                    world_coords::geo::to_utm(p.lat.to_radians(), p.lon.to_radians(), zone);
                glam::DVec2::new(e, n)
            })
            .collect();
        if ring.len() < 3 {
            return 0.0;
        }
        let mut total = 0.0;
        let mut j = ring.len() - 1;
        for i in 0..ring.len() {
            total += (ring[j].x - ring[i].x) * (ring[j].y + ring[i].y);
            j = i;
        }
        (total / 2.0).abs()
    }
}

/// A corner of a water polygon. Degrees like every other geo-positioned
/// entry; the surface height comes from the terrain.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WaterPoint {
    pub lat: f64,
    pub lon: f64,
}

/// A body of water: a lake, a pond, a reservoir, a stretch of river between
/// its banks — the closed OSM polygons of the `natural=water` family, or one
/// drawn by hand.
///
/// The polygon is the waterline. Where the surface lies is decided at terrain
/// build time, from the elevation data around the outline — a lake settles
/// flat into its basin, a river follows its fall — see [`crate::water`].
/// Nothing is stored about the look of the water: the shader makes its waves
/// out of the wind and the weather, which the scenario already knows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WaterSource {
    /// Name from OSM (`name=*`), shown in the editor; empty is fine.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// The outline in either winding — three corners at least, and not closed
    /// (the last corner joins the first).
    pub polygon: Vec<WaterPoint>,
    /// Islands and other ground the water goes around — holes in the surface,
    /// each an outline like `polygon`. An outline that is not inside the
    /// water is ignored, so a hand-edited file cannot tear the surface.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holes: Vec<Vec<WaterPoint>>,
    /// Free-form tags, lower-case kebab like everywhere else. The OSM import
    /// records what it matched (`water`, `waterway`) so a hand-edited file can
    /// still tell a lake from a riverbank.
    #[serde(default)]
    pub tags: Vec<String>,
}

impl WaterSource {
    /// Middle of the outline [deg] — where the editor's list jumps to.
    pub fn centre(&self) -> (f64, f64) {
        if self.polygon.is_empty() {
            return (0.0, 0.0);
        }
        let n = self.polygon.len() as f64;
        (
            self.polygon.iter().map(|p| p.lat).sum::<f64>() / n,
            self.polygon.iter().map(|p| p.lon).sum::<f64>() / n,
        )
    }

    /// Area of the outline less its holes [m²], measured in the UTM zone
    /// `zone` — what the editor's panel reports, and how a click tells the
    /// big lake from the ditch beside it.
    pub fn area(&self, zone: u8) -> f64 {
        let utm = |p: &WaterPoint| {
            let (e, n) = world_coords::geo::to_utm(p.lat.to_radians(), p.lon.to_radians(), zone);
            glam::DVec2::new(e, n)
        };
        let ring_area = |ring: &[WaterPoint]| -> f64 {
            if ring.len() < 3 {
                return 0.0;
            }
            let ring: Vec<glam::DVec2> = ring.iter().map(utm).collect();
            let mut total = 0.0;
            let mut j = ring.len() - 1;
            for i in 0..ring.len() {
                total += (ring[j].x - ring[i].x) * (ring[j].y + ring[i].y);
                j = i;
            }
            (total / 2.0).abs()
        };
        let outer = ring_area(&self.polygon);
        let holes: f64 = self.holes.iter().map(|h| ring_area(h)).sum();
        (outer - holes).max(0.0)
    }

    /// Whether the point [deg] lies on the water — inside the outline and
    /// outside every hole. What a click in the editor asks.
    pub fn contains(&self, lat: f64, lon: f64) -> bool {
        use crate::terrain::point_in_polygon;
        let p = glam::DVec2::new(lat, lon);
        let ring = |points: &[WaterPoint]| {
            points
                .iter()
                .map(|q| glam::DVec2::new(q.lat, q.lon))
                .collect::<Vec<_>>()
        };
        point_in_polygon(p, &ring(&self.polygon))
            && !self.holes.iter().any(|h| point_in_polygon(p, &ring(h)))
    }
}

/// What a road's surface is made of. It decides the material the surface is
/// drawn with — asphalt everywhere, and the concrete of the motorway
/// carriageways and the farm roads' slabs. The markings are not part of it:
/// they travel on their own (see [`CenterLine`]), so every combination of
/// surface and markings is one road, not four kinds of road.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub enum RoadSurface {
    /// `surface=asphalt`, the default — the commonest German carriageway.
    #[default]
    Asphalt,
    /// `surface=concrete` and friends — motorway sections, farm slabs.
    Concrete,
}

impl RoadSurface {
    /// The id a line file stores (`"asphalt"` / `"concrete"`).
    pub fn id(self) -> &'static str {
        match self {
            RoadSurface::Asphalt => "asphalt",
            RoadSurface::Concrete => "concrete",
        }
    }

    /// The surface an id names; an unknown one reads as asphalt rather than
    /// dropping the road.
    pub fn from_id(id: &str) -> Self {
        match id {
            "concrete" => RoadSurface::Concrete,
            _ => RoadSurface::Asphalt,
        }
    }
}

/// What runs along the middle of a road. A centre line exists only on a
/// two-way road wide enough to stripe — a motorway carriageway carries none
/// (it is one direction; the next carriageway is its neighbour's business),
/// and a field track is nobody's to overtake on. The dashes follow the RMS:
/// the 6 m stroke with a 12 m gap outside built-up areas, the 3 m stroke
/// with a 6 m gap inside them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CenterLine {
    /// Nothing — one-way roads, field tracks, the narrowest streets.
    #[default]
    None,
    /// Gestrichelte Mittellinie, außerorts — overtaking allowed, the usual
    /// case on country roads: 6 m stroke, 12 m gap.
    Dashed,
    /// Gestrichelte Mittellinie, innerorts — the shorter stroke the RMS
    /// paints on town streets: 3 m stroke, 6 m gap.
    DashedUrban,
    /// Durchgezogene Mittellinie — the Überholverbot line.
    Solid,
}

impl CenterLine {
    /// The id a line file stores.
    pub fn id(self) -> &'static str {
        match self {
            CenterLine::None => "none",
            CenterLine::Dashed => "dashed",
            CenterLine::DashedUrban => "dashed-urban",
            CenterLine::Solid => "solid",
        }
    }

    /// The centre line an id names; unknown reads as none.
    pub fn from_id(id: &str) -> Self {
        match id {
            "dashed" => CenterLine::Dashed,
            "dashed-urban" => CenterLine::DashedUrban,
            "solid" => CenterLine::Solid,
            _ => CenterLine::None,
        }
    }
}

/// A corner of a road's centre line. Degrees like every other geo-positioned
/// entry; the surface height comes from the terrain.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RoadPoint {
    pub lat: f64,
    pub lon: f64,
}

/// A road: the centre line OSM maps a street with, and the width, surface and
/// markings that turn it into a carriageway.
///
/// The centre line is what the import gets (`highway=*` ways), so it is what
/// the file stores; the surface is cut to the terrain tiles at build time and
/// draped on the ground a road-width either side of the line (see
/// [`crate::roads`]). Roads do not carry their look: the textures are the
/// program's, the markings are drawn by the shader, so a module carries no
/// road bitmaps and a multiplayer run agrees on what a road looks like
/// without a byte crossing the network.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoadSource {
    /// Name from OSM (`name=*`, else `ref=*`), shown in the editor; empty is
    /// fine.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// The centre line, open — two points at least. A way OSM cut at the
    /// module boundary stays whole: the neighbour imports the same way and
    /// draws only what its own ground covers, like the waters.
    pub points: Vec<RoadPoint>,
    /// Carriageway width [m], kerb to kerb — what the ribbon is laid either
    /// side of the centre line.
    #[serde(default = "default_road_width")]
    pub width: f64,
    /// What the carriageway is made of.
    #[serde(default)]
    pub surface: RoadSurface,
    /// What runs along the middle.
    #[serde(default)]
    pub center_line: CenterLine,
    /// Whether the solid white edge lines (Seitenlinien) run along the kerbs.
    #[serde(default = "default_edge_lines")]
    pub edge_lines: bool,
    /// Whether the way flies (`bridge=*` in OSM). Where the ground dips
    /// below the line between the way's own ends — a valley, a river, a
    /// cutting — the carriageway holds that line instead of following the
    /// hollow (see [`crate::roads`]).
    #[serde(default)]
    pub bridge: bool,
    /// Free-form tags, lower-case kebab like everywhere else. The OSM import
    /// records the `highway=*` class it matched, so a hand-edited file can
    /// still tell a motorway from a field track.
    #[serde(default)]
    pub tags: Vec<String>,
}

fn default_road_width() -> f64 {
    6.0
}

fn default_edge_lines() -> bool {
    true
}

impl RoadSource {
    /// Middle of the centre line [deg] — where the editor's panel jumps to.
    pub fn centre(&self) -> (f64, f64) {
        if self.points.is_empty() {
            return (0.0, 0.0);
        }
        let n = self.points.len() as f64;
        (
            self.points.iter().map(|p| p.lat).sum::<f64>() / n,
            self.points.iter().map(|p| p.lon).sum::<f64>() / n,
        )
    }

    /// Length of the centre line [m], measured in the UTM zone `zone` — what
    /// the editor's panel reports.
    pub fn length(&self, zone: u8) -> f64 {
        let utm = |p: &RoadPoint| {
            let (e, n) = world_coords::geo::to_utm(p.lat.to_radians(), p.lon.to_radians(), zone);
            glam::DVec2::new(e, n)
        };
        let mut total = 0.0;
        if let Some(first) = self.points.first() {
            let mut prev = utm(first);
            for point in &self.points[1..] {
                let next = utm(point);
                total += prev.distance(next);
                prev = next;
            }
        }
        total
    }
}

/// What a field import drew on: one row per state it asked, with the state of
/// the register it got (plan ch. 4, ch. 9).
///
/// Two things need it. The licences of most states ask for a source note, and
/// the note has to name the year; and the registers move under the module —
/// North Rhine-Westphalia's daily, Brandenburg's yearly — so a line that does
/// not record what it was built against cannot be rebuilt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldSourceStamp {
    /// The state's code, `"NW"`.
    pub land: String,
    /// Application year the parcels were declared for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<u32>,
    /// When it was fetched, in seconds since the Unix epoch.
    #[serde(default)]
    pub fetched: u64,
}

fn default_walk_width() -> f64 {
    2.0
}

fn default_walk_people() -> u32 {
    4
}

fn default_area_people() -> u32 {
    6
}

fn default_walking_share() -> f64 {
    0.5
}

/// Node of the source file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NodeSource {
    Buffer,
    Joint,
    /// Switch: root/straight/diverging are resolved through the edge indices.
    Switch {
        root: (u32, bool),
        straight: (u32, bool),
        diverging: (u32, bool),
        #[serde(default = "default_throw_time")]
        throw_time: f64,
    },
}

fn default_throw_time() -> f64 {
    6.0
}

/// Where an edge begins.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum EdgeStart {
    /// Georeferenced with heading (0° = north, clockwise).
    Geo { point: GeoPoint, heading_deg: f64 },
    /// Joins the end of an earlier edge.
    Continue { edge: u32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EdgeSource {
    pub from: u32,
    pub to: u32,
    pub start: EdgeStart,
    pub segments: Vec<Segment>,
    /// Gradient [‰] as steps `(s, value)`.
    #[serde(default)]
    pub grade: Vec<(f64, f64)>,
    /// Cant [mm].
    #[serde(default)]
    pub cant: Vec<(f64, f64)>,
    /// Permitted speed [km/h].
    #[serde(default)]
    pub speed: Vec<(f64, f64)>,
    /// Track type (`"<mod>:<name>"`, see `track_types/*.ron`) as steps
    /// `(s, name)` — one edge changes its superstructure section by section.
    /// Empty = the default type.
    #[serde(default)]
    pub track_type: Vec<(f64, String)>,
    /// What hangs over this track as steps `(s, system)` — `"ac-15kv"`,
    /// `"ac-25kv"`, `"dc-3kv"`, `"dc-1.5kv"`, `"third-rail"` or `"none"`.
    /// Empty = no wire, unless the file still carries the legacy
    /// [`LineSource::electrification`].
    #[serde(default)]
    pub electrification: Vec<(f64, String)>,
    /// Whether this edge carries a formation — ballast bed, and the embankment
    /// or cutting the terrain builds under it (`true` unless the file says
    /// otherwise). `false` for track on the builder's own constructions:
    /// bridges, platforms, ground they shaped themselves.
    #[serde(default = "default_formation")]
    pub formation: bool,
}

fn default_formation() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceSource {
    pub kind: DeviceKind,
    pub edge: u32,
    pub s: f64,
    #[serde(default)]
    pub facing: Facing,
    #[serde(default)]
    pub lateral_offset: f64,
    /// Country-specific payload as RON text.
    #[serde(default)]
    pub payload: String,
}

/// A scenery object placed relative to the track: a mod's `objects/*.ron`
/// (`"<mod>:<name>"`) at `(edge, s)`. The editor stamps the object's own
/// default offset/rotation/height on placement; the values here are what
/// stands, so a single instance can deviate from its kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObjectSource {
    pub object: String,
    pub edge: u32,
    pub s: f64,
    /// Lateral offset [m], positive = right of increasing arc length.
    #[serde(default)]
    pub lateral_offset: f64,
    /// Rotation about the up axis [deg], clockwise seen from above;
    /// 0 = the model's front points along increasing arc length.
    #[serde(default)]
    pub yaw_deg: f64,
    /// Height above the railhead [m] — above the *terrain* instead when
    /// `snap_to_terrain` is set.
    #[serde(default)]
    pub height: f64,
    /// Put the object's base on the terrain surface instead of the rail plane;
    /// `height` then measures from the ground. Resolved by the terrain tile
    /// the object stands on, in the editor as in the run.
    #[serde(default)]
    pub snap_to_terrain: bool,
}

/// A place for stock: a stabling road on the line, or a portal at the edge of it
/// (plan ch. 11, "v1 trains spawn/despawn at fiddle yards").
///
/// Shunting needs somewhere to shunt *to*, and an operating day needs somewhere to
/// leave a unit between two workings. A yard is a mark on the track like a device: an
/// `(edge, s)` with the direction a train standing here faces, and the length of the
/// road behind it. The simulation reads it as [`sim_core::yard::Yard`] — the AI's shunt
/// jobs address one by name, [`sim_core::Sim::place_at`] puts a train on it, and
/// [`sim_core::Sim::withdraw`] takes one off the line at a portal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct YardSource {
    /// What a shunt job, a timetable or an operating day calls it. Content, not
    /// translated — it is a place on this line, like a station name.
    pub name: String,
    #[serde(default)]
    pub kind: YardKind,
    /// Where the head of a standing train comes to.
    pub edge: u32,
    pub s: f64,
    /// The way the train faces: `Forward` along increasing arc length, `Backward`
    /// against it — the same two directions a device is read in.
    #[serde(default)]
    pub facing: Facing,
    /// Usable length [m] — what fits on the road. `0` = not stated, and then nothing is
    /// refused for being long.
    #[serde(default)]
    pub length: f64,
}

impl YardSource {
    /// The mark on the track graph, as the simulation holds it.
    pub fn compile(&self) -> Yard {
        Yard {
            name: self.name.clone(),
            kind: self.kind,
            at: TrackPosition::new(
                EdgeId(self.edge),
                self.s,
                if self.facing == Facing::Backward {
                    -1
                } else {
                    1
                },
            ),
            length: self.length,
        }
    }
}

/// A single tree — geo-positioned, standing on the terrain (plan ch. 14
/// "vegetation as streamed instances"). Placed one by one with the tree tool,
/// or baked in rows by the forest brush and the forest import.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TreeSource {
    /// 3D object from a mod (`objects/*.ron`, `"<mod>:<name>"`); empty means
    /// the app's built-in placeholder tree.
    #[serde(default)]
    pub object: String,
    /// Position [deg]; the height comes from the terrain.
    pub lat: f64,
    pub lon: f64,
    /// Rotation about the up axis [deg], clockwise seen from above.
    #[serde(default)]
    pub yaw_deg: f64,
    /// Uniform scale on the object's own size.
    #[serde(default = "default_scale")]
    pub scale: f64,
}

fn default_scale() -> f64 {
    1.0
}

/// Height data a module ships with itself: a directory of ESRI ASCII grids,
/// one per terrain tile, cut out of the state survey office's DGM by the route
/// editor's DGM import.
///
/// A module that names its own heights is self-contained — no `--dgm` on the
/// command line, and the tiles that were imported are exactly the ground the
/// module needs. `path` is mod-qualified like everything else
/// (`"<mod>:heights/<line>"`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeightSource {
    pub path: String,
    /// UTM zone of the grids (32 west, 33 east of 12° E).
    pub zone: u8,
}

/// What one terrain brush stroke does to the ground.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum TerrainEdit {
    /// Raise (+) or lower (−) the ground by this much at the centre [m].
    Raise(f64),
    /// Pull the ground to this ellipsoidal height [m] — the editor fills it in
    /// from the nearest rail, which is what levelling a station forecourt or a
    /// depot means.
    Level(f64),
}

/// One terrain brush stroke: a round stamp on the elevation data, falling off
/// to nothing at its edge. Strokes are applied in file order, so a later one
/// paints over an earlier one exactly as it was drawn.
///
/// The DGM stays untouched — a line stores the strokes, not a heightfield, so
/// every stroke can be picked, re-dialled or deleted afterwards, the file stays
/// small, and re-importing better elevation data does not throw the shaping
/// away. The track is never moved: strokes act on the ground *before* the
/// cutting/embankment blend, so the strip along the rails keeps rail height.
// ponytail: no smoothing brush — smoothing needs the neighbourhood, which a
// stamp does not have. A large, gentle `Level` is the same thing by hand; a
// real smooth brush needs the tile grid as its working set.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TerrainEditSource {
    /// Centre [deg].
    pub lat: f64,
    pub lon: f64,
    /// Radius of the stroke [m]; beyond it nothing changes.
    pub radius: f64,
    pub edit: TerrainEdit,
}

/// A reference marker: a labelled point on the map that says *where* something
/// belongs while the track is drawn by hand — a level crossing, a platform, a
/// kilometre mark. Nothing in the simulation or the compilation reads them;
/// they are drawing aids and travel with the line so that the next session
/// still has them.
///
/// The `layer` is a free name, and everything sharing it is one layer: the
/// editor shows, hides and deletes markers by that string. The OSM import
/// fills it with the tag it matched (`level-crossing`, `platform`, …).
// ponytail: no layer registry — a layer is the set of markers that name it.
// Nothing else needs a home for per-layer settings; colour and order would be
// the first reason to add one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarkerSource {
    pub layer: String,
    /// Free text shown next to the marker; empty is fine.
    #[serde(default)]
    pub label: String,
    /// Position [deg]; markers lie on the map plane, not on the terrain.
    pub lat: f64,
    pub lon: f64,
}

/// One stretch of track a marked area covers: `[from, to)` along one edge.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AreaSpan {
    pub edge: u32,
    pub from: f64,
    pub to: f64,
}

impl AreaSpan {
    pub fn new(edge: u32, a: f64, b: f64) -> Self {
        Self {
            edge,
            from: a.min(b),
            to: a.max(b),
        }
    }

    pub fn length(&self) -> f64 {
        (self.to - self.from).max(0.0)
    }

    pub fn covers(&self, edge: u32, s: f64) -> bool {
        self.edge == edge && s >= self.from && s < self.to
    }
}

/// A **marked stretch of track** that carries properties.
///
/// The per-edge step profiles say what a track is like metre by metre, which is exactly
/// right for a compiler and exactly wrong for a person: laying a 40 km/h restriction
/// through a station means editing the speed steps of every track it touches, and
/// changing it again means finding them all a second time.
///
/// An area is that job the other way round: mark the stretch once, give it a name and a
/// colour, and set the properties on it. What it does not set, it does not touch — so a
/// speed restriction laid over an electrification boundary leaves the wire alone. Areas
/// are laid over the edges' own profiles in order, so a later one wins where two overlap,
/// which is what "drawn on top" means on the map.
///
/// Nothing in the simulation knows about them: [`LineSource::compile`] bakes them down
/// into the same step profiles the edges have always carried.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackAreaSource {
    /// What it is for, in the author's words — "Bahnhof Musterstadt", "La 40".
    pub name: String,
    /// Colour on the map (sRGB 0..1).
    #[serde(default = "default_area_color")]
    pub color: (f32, f32, f32),
    /// Half-width of the stroke it is painted with [m]. A display property — the data is
    /// the stretch, not the paint — but it belongs to the area rather than to the editor,
    /// so a wide marking stays wide when the file is opened again.
    #[serde(default = "default_area_width")]
    pub width: f64,
    /// The stretches it covers. Several, because a station is several tracks.
    #[serde(default)]
    pub spans: Vec<AreaSpan>,
    /// Permitted speed [km/h].
    #[serde(default)]
    pub speed: Option<f64>,
    /// Cant [mm].
    #[serde(default)]
    pub cant: Option<f64>,
    /// Longitudinal gradient [‰].
    #[serde(default)]
    pub grade: Option<f64>,
    /// Track type (`"<mod>:<name>"`, or `"default"`) — model and texture of the
    /// superstructure.
    #[serde(default)]
    pub track_type: Option<String>,
    /// Electrification (an id of [`track_model::PowerSystem`], or `"none"`).
    #[serde(default)]
    pub electrification: Option<String>,
}

/// Permitted speed a track without a profile of its own carries [km/h] — the figure
/// `TrackEdge::new` starts from, repeated here because an area laid over such a track has
/// to know what it is laying over.
pub const DEFAULT_SPEED: f64 = 160.0;

/// The reserved name of the built-in track type.
pub const DEFAULT_TRACK_TYPE: &str = "default";

/// Half-width a marked area is painted with by default [m] — comfortably wider than the
/// 1.5 m of the track ribbon, so a painted stretch reads as laid over the track.
pub const DEFAULT_AREA_WIDTH: f64 = 2.5;

fn default_area_width() -> f64 {
    DEFAULT_AREA_WIDTH
}

fn default_area_color() -> (f32, f32, f32) {
    // The editor's accent, so a fresh area is visible before anybody colours it.
    (0.35, 0.72, 0.95)
}

impl Default for TrackAreaSource {
    fn default() -> Self {
        Self {
            name: String::new(),
            color: default_area_color(),
            width: default_area_width(),
            spans: Vec::new(),
            speed: None,
            cant: None,
            grade: None,
            track_type: None,
            electrification: None,
        }
    }
}

impl TrackAreaSource {
    /// Does the area set anything at all? One that does not is a marking and nothing more
    /// — useful while working, worth saying out loud in the editor.
    pub fn sets_anything(&self) -> bool {
        self.speed.is_some()
            || self.cant.is_some()
            || self.grade.is_some()
            || self.track_type.is_some()
            || self.electrification.is_some()
    }

    /// Total length of track it covers [m].
    pub fn length(&self) -> f64 {
        self.spans.iter().map(AreaSpan::length).sum()
    }
}

/// Lays area values over a base step profile.
///
/// `base` is what the edge itself says (empty = nothing but `base_default`), `spans` are
/// the `(from, to, value)` of every area covering this edge, in the order they are to be
/// applied. The result is the same kind of step list, with equal neighbours collapsed.
fn overlay_steps<T: Clone + PartialEq>(
    base: &[(f64, T)],
    base_default: T,
    spans: &[(f64, f64, T)],
    length: f64,
) -> Vec<(f64, T)> {
    if spans.is_empty() {
        return base.to_vec();
    }
    // The value the edge's own profile has at `s` — the first entry also applies before
    // its own `s`, as `StepProfile` reads it.
    let from_base = |s: f64| -> T {
        let mut value = base
            .first()
            .map_or(base_default.clone(), |(_, v)| v.clone());
        for (at, v) in base {
            if *at <= s {
                value = v.clone();
            } else {
                break;
            }
        }
        value
    };
    let value_at = |s: f64| -> T {
        // Later areas are drawn on top of earlier ones.
        spans
            .iter()
            .rev()
            .find(|(from, to, _)| s >= *from && s < *to)
            .map(|(_, _, v)| v.clone())
            .unwrap_or_else(|| from_base(s))
    };

    let mut breaks: Vec<f64> = vec![0.0];
    breaks.extend(base.iter().map(|(s, _)| *s));
    for (from, to, _) in spans {
        breaks.push(*from);
        breaks.push(*to);
    }
    breaks.retain(|s| *s >= 0.0 && *s < length);
    breaks.sort_by(|a, b| a.total_cmp(b));
    breaks.dedup_by(|a, b| (*a - *b).abs() < 1e-6);

    let mut steps: Vec<(f64, T)> = Vec::with_capacity(breaks.len());
    for s in breaks {
        let value = value_at(s);
        if steps.last().is_none_or(|(_, last)| *last != value) {
            steps.push((s, value));
        }
    }
    steps
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SectionSource {
    pub edges: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignalSource {
    pub kind: SignalKind,
    #[serde(default = "default_system")]
    pub system: SignalSystem,
    /// Index into `devices`.
    pub device: u32,
    #[serde(default)]
    pub next: Option<u32>,
    #[serde(default)]
    pub guarded: Vec<u32>,
    #[serde(default)]
    pub requires_route: bool,
    #[serde(default)]
    pub diverging_speed: Option<f64>,
    /// Signal type from a mod (`"<mod>:<name>"`) — the aspect then comes from that rule
    /// table instead of the built-in logic. Resolved by the mod runtime (plan ch. 19).
    #[serde(default)]
    pub signal_type: Option<String>,
    /// 3D model override (`"<mod>:<name>"` below `signal_models/`) — wins over the
    /// signal type's default model.
    #[serde(default)]
    pub model: Option<String>,
}

fn default_system() -> SignalSystem {
    SignalSystem::Ks
}

/// Named connection point of a module: a `Buffer` node at which another module may attach
/// (plan ch. 15; the composition is in [`crate::compose`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoundarySource {
    pub name: String,
    /// Index into `nodes`; must be a `Buffer` at the open end of an edge.
    pub node: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteSource {
    pub entry: u32,
    pub exit: u32,
    /// Train route or shunting route (Ril 408 / 301). A shunting route clears **Sh 1** at
    /// its entry signal instead of the main aspect and may be set into an occupied track.
    #[serde(default)]
    pub kind: sim_core::interlock::RouteKind,
    #[serde(default)]
    pub switches: Vec<(u32, SwitchPosition)>,
    #[serde(default)]
    pub sections: Vec<u32>,
    #[serde(default)]
    pub overlap: Vec<u32>,
    /// Flank protection of the route (Ril 819) — see [`FlankSource`].
    #[serde(default)]
    pub flank: Vec<FlankSource>,
    #[serde(default)]
    pub diverging: bool,
}

/// One flank protection measure in source form: a node index for a protecting
/// turnout with the position it has to lie in, a signal index for a signal
/// that has to stay at stop while the route is set.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum FlankSource {
    Switch(u32, SwitchPosition),
    Signal(u32),
}

/// A complete line in source form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LineSource {
    pub name: String,
    /// The year the module portrays — the state of the line a driver is meant
    /// to find. Nothing in the simulation reads it yet; it is what a module
    /// says about itself. A module that does not care simply has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<u32>,
    /// Whether the module is invented rather than a rebuild of a real place.
    #[serde(default)]
    pub fictional: bool,
    /// Geoid undulation for the height conversion [m] (plan 4.2).
    #[serde(default = "default_geoid")]
    pub geoid_offset: f64,
    /// Legacy: what the whole line is electrified with where an edge says
    /// nothing. The wire belongs to the track, not to the line — the editor
    /// sets it per edge and writes nothing here. Files from before that keep
    /// their value and keep working; empty means the edges decide alone.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub electrification: String,
    pub nodes: Vec<NodeSource>,
    pub edges: Vec<EdgeSource>,
    #[serde(default)]
    pub devices: Vec<DeviceSource>,
    /// Scenery objects linked to the track; nothing in the simulation reads
    /// them — they are the line's furniture.
    #[serde(default)]
    pub objects: Vec<ObjectSource>,
    /// Stabling roads and portals — where stock may be put, and where trains appear and
    /// disappear (see [`YardSource`]). A line without them can be driven but not
    /// shunted, because there is nowhere to shunt to.
    #[serde(default)]
    pub yards: Vec<YardSource>,
    /// Trees (geo-positioned, height from the terrain) — placed one by one, or
    /// baked in rows of thousands by the editor's forest brush and forest
    /// import. Every tree is an ordinary entry, so every tree can be moved or
    /// deleted on its own.
    #[serde(default)]
    pub trees: Vec<TreeSource>,
    /// Footpaths people walk along (see [`WalkPathSource`]) — geo-positioned like
    /// the trees, height from the terrain.
    #[serde(default)]
    pub walk_paths: Vec<WalkPathSource>,
    /// Places people are about on (see [`WalkAreaSource`]).
    #[serde(default)]
    pub walk_areas: Vec<WalkAreaSource>,
    /// Fields: farmland with a crop on it (see [`FieldSource`]), imported from
    /// the agricultural registers or drawn by hand.
    #[serde(default)]
    pub fields: Vec<FieldSource>,
    /// What the field import drew on (see [`FieldSourceStamp`]) — one row per
    /// state, so the module says which register it portrays and the licences
    /// can be honoured.
    #[serde(default)]
    pub field_sources: Vec<FieldSourceStamp>,
    /// Reference markers — editor aids in named layers, ignored by everything
    /// that drives a train (see [`MarkerSource`]).
    #[serde(default)]
    pub markers: Vec<MarkerSource>,
    /// Bodies of water (see [`WaterSource`]) — imported from OSM or drawn by
    /// hand. The surfaces are laid over the terrain when the tiles are built,
    /// like the fields are draped on it.
    #[serde(default)]
    pub waters: Vec<WaterSource>,
    /// Roads (see [`RoadSource`]) — imported from OSM or drawn by hand. The
    /// carriageways are draped on the terrain when the tiles are built, like
    /// the fields are.
    #[serde(default)]
    pub roads: Vec<RoadSource>,
    /// Terrain brush strokes on top of the elevation data (see
    /// [`TerrainEditSource`]).
    #[serde(default)]
    pub terrain: Vec<TerrainEditSource>,
    /// Height data shipped with the module (see [`HeightSource`]). A list
    /// because a composition merges the modules' entries into one line; a
    /// module edited on its own has exactly one.
    #[serde(default)]
    pub heights: Vec<HeightSource>,
    #[serde(default)]
    pub sections: Vec<SectionSource>,
    /// Marked stretches of track with properties (see [`TrackAreaSource`]). They are laid
    /// over the edges' own profiles on compile, in order, so a later one wins.
    #[serde(default)]
    pub areas: Vec<TrackAreaSource>,
    #[serde(default)]
    pub signals: Vec<SignalSource>,
    #[serde(default)]
    pub routes: Vec<RouteSource>,
    /// Connection points for the module composition; a line that is never composed
    /// simply has none.
    #[serde(default)]
    pub boundaries: Vec<BoundarySource>,
    /// Where the module sits: the point the editor centres on when the module is
    /// opened and the envelope is built around when it is created. A line that
    /// predates the anchor simply has none — the editor then falls back to the
    /// middle of the track it finds.
    #[serde(default)]
    pub anchor: Option<GeoPoint>,
    /// The module's envelope as a closed polygon (see [`EnvelopePoint`]): what
    /// the module may cover. Terrain strokes, trees, objects and markers have to
    /// lie inside it, so that two neighbouring modules never shape the same
    /// ground twice. **Empty means unbounded** — that is what every line written
    /// before envelopes existed reads as, and it keeps working.
    #[serde(default)]
    pub envelope: Vec<EnvelopePoint>,
    /// Optional Lua script hook (plan 19.7), named `"<mod>:<file stem>"`.
    #[serde(default)]
    pub script: Option<String>,
}

impl Default for LineSource {
    fn default() -> Self {
        Self {
            name: String::new(),
            year: None,
            fictional: false,
            geoid_offset: default_geoid(),
            electrification: String::new(),
            nodes: Vec::new(),
            edges: Vec::new(),
            devices: Vec::new(),
            objects: Vec::new(),
            yards: Vec::new(),
            trees: Vec::new(),
            walk_paths: Vec::new(),
            walk_areas: Vec::new(),
            fields: Vec::new(),
            field_sources: Vec::new(),
            markers: Vec::new(),
            waters: Vec::new(),
            roads: Vec::new(),
            terrain: Vec::new(),
            heights: Vec::new(),
            sections: Vec::new(),
            areas: Vec::new(),
            signals: Vec::new(),
            routes: Vec::new(),
            boundaries: Vec::new(),
            anchor: None,
            envelope: Vec::new(),
            script: None,
        }
    }
}

/// How far a field's corner may sit outside the envelope before the rule check
/// calls it out [m]. A cut field ends *on* the boundary, and a metre is well
/// under what a builder could drag a corner by without meaning to.
pub const FIELD_MARGIN: f64 = 1.0;

/// Half the edge length of the envelope a new module starts with [m].
///
/// Zusi's rule of thumb puts a module boundary about a kilometre ahead of a
/// distant signal, which makes a module a few kilometres across; 2 km to each
/// side is a line of that order that still fits on the editor's first screen.
pub const DEFAULT_ENVELOPE_HALF_SIZE: f64 = 2000.0;

/// The square envelope a new module starts with: `half_size` metres to each side
/// of `anchor`, counter-clockwise from the south-west corner.
pub fn default_envelope(anchor: GeoPoint, half_size: f64) -> Vec<EnvelopePoint> {
    // Degrees per metre at this latitude — good to a few metres over a couple of
    // kilometres, which is well inside what the user drags the corners by anyway.
    let dlat = half_size / 111_320.0;
    let dlon = half_size / (111_320.0 * anchor.lat.to_radians().cos().abs().max(1e-6));
    [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
        .into_iter()
        .map(|(sx, sy)| EnvelopePoint {
            lat: anchor.lat + sy * dlat,
            lon: anchor.lon + sx * dlon,
        })
        .collect()
}

fn default_geoid() -> f64 {
    46.0
}

/// Shortest distance from a point to the outline of a polygon, both in degrees,
/// answered in metres.
///
/// ponytail: the degrees are scaled to metres at the point's own latitude —
/// over the few kilometres a module spans that is metre-true, and this is used
/// for a click tolerance, not for a measurement.
fn distance_to_polygon(point: glam::DVec2, polygon: &[glam::DVec2]) -> f64 {
    let scale = glam::DVec2::new(111_320.0 * point.y.to_radians().cos().abs(), 111_320.0);
    let p = point * scale;
    (0..polygon.len())
        .map(|i| {
            let a = polygon[i] * scale;
            let b = polygon[(i + 1) % polygon.len()] * scale;
            let along = b - a;
            let t = ((p - a).dot(along) / along.length_squared().max(1e-9)).clamp(0.0, 1.0);
            (a + along * t).distance(p)
        })
        .fold(f64::INFINITY, f64::min)
}

/// Do two sides of the envelope cross?
///
/// A polygon that crosses itself has no inside — ray casting answers "in" for
/// the same place a human would call out, so everything built on the envelope
/// (what may be placed, what the terrain covers, where the neighbour begins)
/// silently means something else. Zusi requires a simple closed polygon for the
/// same reason.
///
/// ponytail: every pair of sides, which is O(n²) — an envelope has a handful of
/// corners, and a sweep line would be more code than the check it replaces.
pub fn envelope_self_intersects(corners: &[EnvelopePoint]) -> bool {
    let n = corners.len();
    if n < 4 {
        return false;
    }
    let at = |i: usize| glam::DVec2::new(corners[i % n].lon, corners[i % n].lat);
    for i in 0..n {
        for j in (i + 1)..n {
            // Neighbouring sides share a corner and always "touch" there.
            if j == i + 1 || (i == 0 && j == n - 1) {
                continue;
            }
            if segments_cross(at(i), at(i + 1), at(j), at(j + 1)) {
                return true;
            }
        }
    }
    false
}

/// Do the segments `a1a2` and `b1b2` cross? Touching counts — a corner laid
/// exactly onto another side is the same mistake as one dragged past it.
fn segments_cross(a1: glam::DVec2, a2: glam::DVec2, b1: glam::DVec2, b2: glam::DVec2) -> bool {
    let side = |p: glam::DVec2, q: glam::DVec2, r: glam::DVec2| {
        let v = (q - p).perp_dot(r - p);
        if v > 1e-12 {
            1
        } else if v < -1e-12 {
            -1
        } else {
            0
        }
    };
    let (d1, d2) = (side(a1, a2, b1), side(a1, a2, b2));
    let (d3, d4) = (side(b1, b2, a1), side(b1, b2, a2));
    if d1 != d2 && d3 != d4 {
        return true;
    }
    // Collinear and overlapping: the segments lie on one line and share a span.
    let on = |p: glam::DVec2, q: glam::DVec2, r: glam::DVec2| {
        side(p, q, r) == 0
            && r.x >= p.x.min(q.x) - 1e-12
            && r.x <= p.x.max(q.x) + 1e-12
            && r.y >= p.y.min(q.y) - 1e-12
            && r.y <= p.y.max(q.y) + 1e-12
    };
    on(a1, a2, b1) || on(a1, a2, b2) || on(b1, b2, a1) || on(b1, b2, a2)
}

/// Result of the compilation.
pub struct CompiledLine {
    pub net: TrackNetwork,
    pub interlock: Interlock,
    /// Stabling roads and portals of the line, resolved onto the graph (plan ch. 11).
    /// Goes into [`sim_core::Sim::yards`] when the world is built.
    pub yards: Vec<Yard>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    UnknownEdge(u32),
    UnknownNode(u32),
    UnknownDevice(u32),
    /// An edge refers to an edge that has not been compiled yet.
    ForwardReference(u32),
}

/// A finding of [`LineSource::check`] — wiring that compiles but fails on the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleIssue {
    /// Device edge index out of range or `s` beyond the edge length.
    DeviceOffEdge { device: u32 },
    /// Magnet payload does not parse or names a signal that does not exist.
    MagnetPayloadInvalid { device: u32 },
    /// Block marker payload does not parse or names a section that does not exist.
    BlockMarkerPayloadInvalid { device: u32 },
    /// Distant (or combination) signal without a 1000 Hz magnet linked to it.
    DistantWithout1000Hz { signal: u32 },
    /// Main (or combination) signal without a 2000 Hz magnet linked to it.
    MainWithout2000Hz { signal: u32 },
    /// Distant signal that announces no signal (`next` missing).
    DistantWithoutNext { signal: u32 },
    /// Signal whose device is missing or not a `Signal` device.
    SignalDeviceMismatch { signal: u32 },
    /// Boundary whose node is missing or not a `Buffer`.
    BoundaryInvalid { boundary: u32 },
    /// Edge names a track type the registry does not know.
    UnknownTrackType { edge: u32 },
    /// A marked area covers a track that does not exist, or a stretch beyond its end.
    AreaOffTrack { area: u32 },
    /// A marked area with no stretch, or one that sets nothing — a marking that does not
    /// reach the line.
    AreaWithoutEffect { area: u32 },
    /// A marked area names a track type no installed mod defines.
    AreaUnknownTrackType { area: u32 },
    /// Edge uses an LZB track type, but the line places no line conductor.
    LzbTypeWithoutConductor { edge: u32 },
    /// Stabling road or portal outside its track (bad edge index or `s` beyond the
    /// length).
    YardOffEdge { yard: u32 },
    /// A portal that is not at the edge of the line: the track behind it does not run
    /// straight out to a buffer stop or a module boundary, so a train appearing there
    /// would appear in the middle of the railway (plan ch. 11).
    PortalNotAtTheEdge { yard: u32 },
    /// Two yards of the same name — a shunt job that names it would always get the first.
    DuplicateYardName { yard: u32 },
    /// Scenery object outside its track (bad edge index or `s` beyond the length).
    ObjectOffEdge { object: u32 },
    /// Scenery object names an `objects/*.ron` no installed mod has.
    UnknownObject { object: u32 },
    /// Flank protection of a route names a node that is no switch, or a
    /// signal that does not exist.
    FlankGuardInvalid { route: u32 },
    /// A footpath with fewer than two vertices — nothing to walk along.
    WalkPathTooShort { path: u32 },
    /// A walk area with fewer than three corners — no area at all.
    WalkAreaTooSmall { area: u32 },
    /// A field with fewer than three corners — no area at all.
    FieldTooSmall { field: u32 },
    /// A field whose crop is not one the installed tables know. It is drawn as
    /// bare ground; the row says which id to correct.
    FieldUnknownCrop { field: u32 },
    /// A road with fewer than two points on its centre line — nothing to
    /// pave.
    RoadTooShort { road: u32 },
    /// The envelope crosses itself — see [`envelope_self_intersects`].
    EnvelopeSelfIntersects,
    /// Landscape outside the module envelope. Placing it is refused by the
    /// editor, so this is what dragging a corner inwards afterwards leaves
    /// behind — the ground would then be shaped by two modules at once.
    OutsideEnvelope {
        trees: u32,
        terrain: u32,
        markers: u32,
        /// Vertices of footpaths and corners of walk areas — counted one by
        /// one, so the figure says how much has to move back in.
        walkways: u32,
        /// Corners of fields, counted the same way.
        fields: u32,
    },
}

/// Splits a segment chain at arc length `s`: the segment containing `s` is cut
/// in two (curvature continues through the cut), whole segments stay whole.
fn split_segments(segments: &[Segment], s: f64) -> (Vec<Segment>, Vec<Segment>) {
    let mut first = Vec::new();
    let mut second = Vec::new();
    let mut acc = 0.0;
    for seg in segments {
        if acc >= s {
            second.push(*seg);
        } else if acc + seg.len <= s + 1e-9 {
            first.push(*seg);
        } else {
            let local = s - acc;
            first.push(Segment {
                len: local,
                k0: seg.k0,
                dk: seg.dk,
            });
            second.push(Segment {
                len: seg.len - local,
                k0: seg.k0 + seg.dk * local,
                dk: seg.dk,
            });
        }
        acc += seg.len;
    }
    (first, second)
}

/// Step profile entries of a source edge (`(s, value)`).
type Steps<T> = Vec<(f64, T)>;

/// One way onwards from a node: the edge end the path continues over, and the
/// switch position that continuation requires (`None` at a joint).
type Continuation = ((u32, bool), Option<(u32, SwitchPosition)>);

/// Regular overlap (Durchrutschweg) behind an exit signal [m], after the DB
/// staircase: the length follows the speed at the end of the route, because
/// the overlap is what a train overrunning at that speed needs to stop in.
/// 200 m is the regular case of an entry route; shorter overlaps are what
/// buys the lower entry speeds, and beyond 100 km/h it grows again.
///
/// ponytail: the four steps as the literature on German signalling states
/// them (Ril 819 "Durchrutschwege bemessen" itself is not a public document,
/// like the LZB brake tables). A line that knows better sets the length by
/// hand in the editor; a full table would replace this one function.
pub fn regular_overlap(speed_kmh: f64) -> f64 {
    match speed_kmh {
        v if v <= 30.0 => 50.0,
        v if v <= 60.0 => 100.0,
        v if v <= 100.0 => 200.0,
        _ => 300.0,
    }
}

/// Splits step profile entries at `s`; the second half starts with the value
/// in force at the cut. Empty stays empty — the edge default applies as before.
fn split_steps<T: Clone>(steps: &[(f64, T)], s: f64) -> (Steps<T>, Steps<T>) {
    if steps.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let value_at = |q: f64| {
        steps
            .iter()
            .filter(|(x, _)| *x <= q)
            .max_by(|a, b| a.0.total_cmp(&b.0))
            // The first entry also applies before its own `s` (StepProfile::new).
            .or_else(|| steps.iter().min_by(|a, b| a.0.total_cmp(&b.0)))
            .map(|(_, v)| v.clone())
            .expect("steps are non-empty")
    };
    let mut first: Vec<(f64, T)> = steps.iter().filter(|(x, _)| *x < s).cloned().collect();
    if first.is_empty() {
        first.push((0.0, value_at(0.0)));
    }
    let mut second = vec![(0.0, value_at(s))];
    second.extend(
        steps
            .iter()
            .filter(|(x, _)| *x > s)
            .map(|(x, v)| (*x - s, v.clone())),
    );
    (first, second)
}

impl LineSource {
    pub fn from_ron(text: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(text)
    }

    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).expect("serializable")
    }

    /// Writes a field import into the line — one call, so the editor makes it
    /// one undo step. `replace_imported` (a whole-module import) removes what
    /// an earlier import put here and leaves hand-drawn fields alone; without
    /// it (one re-fetched parcel) the report's fields are simply added.
    /// `field_sources` gets one row per state, replacing that state's earlier
    /// row, so the module keeps saying which register it portrays.
    pub fn apply_field_import(&mut self, report: &fields::ImportReport, replace_imported: bool) {
        if replace_imported {
            self.fields.retain(|f| f.source.is_empty());
        }
        self.fields
            .extend(report.fields.iter().map(FieldSource::from_feature));
        for stamp in &report.stamps {
            let row = FieldSourceStamp {
                land: stamp.land.clone(),
                year: stamp.year,
                fetched: stamp.fetched,
            };
            // One row per state; a second import of the same state replaces it.
            self.field_sources
                .retain(|existing| existing.land != row.land);
            self.field_sources.push(row);
        }
        self.field_sources.sort_by(|a, b| a.land.cmp(&b.land));
    }

    /// Does this position lie inside the module's envelope?
    ///
    /// A module without an envelope bounds nothing, so everything is inside —
    /// that is what keeps lines from before envelopes editable.
    ///
    /// ponytail: ray casting straight on `(lon, lat)`. Over the few kilometres a
    /// module spans, the difference between a straight line in degrees and one on
    /// the ellipsoid is far below the width of the drawn boundary.
    pub fn envelope_contains(&self, lat: f64, lon: f64) -> bool {
        self.envelope_contains_within(lat, lon, 0.0)
    }

    /// The same, with the boundary itself counting as inside up to `margin`
    /// metres out.
    ///
    /// The track needs that margin: a module boundary is exactly where a rail
    /// has to meet its neighbour's, so the last metre of track sits *on* the
    /// polygon — where ray casting is undefined and a snapped click would be
    /// refused for being a millimetre on the wrong side. Landscape passes 0 and
    /// stays strictly inside.
    pub fn envelope_contains_within(&self, lat: f64, lon: f64, margin: f64) -> bool {
        if self.envelope.len() < 3 {
            return true;
        }
        let polygon: Vec<glam::DVec2> = self
            .envelope
            .iter()
            .map(|p| glam::DVec2::new(p.lon, p.lat))
            .collect();
        let point = glam::DVec2::new(lon, lat);
        if crate::terrain::point_in_polygon(point, &polygon) {
            return true;
        }
        margin > 0.0 && distance_to_polygon(point, &polygon) <= margin
    }

    /// Removes device `index`. Signals on it disappear with it; every other
    /// device and signal index in the file is remapped.
    pub fn remove_device(&mut self, index: usize) {
        if index >= self.devices.len() {
            return;
        }
        self.devices.remove(index);
        let removed = index as u32;
        let removed_signals: Vec<u32> = self
            .signals
            .iter()
            .enumerate()
            .filter(|(_, s)| s.device == removed)
            .map(|(n, _)| n as u32)
            .collect();
        self.signals.retain(|s| s.device != removed);
        for s in &mut self.signals {
            if s.device > removed {
                s.device -= 1;
            }
        }
        self.drop_signal_refs(&removed_signals);
    }

    /// Removes edge `index` together with the devices on it. An edge that
    /// continued from the removed one is anchored geographically first, so its
    /// geometry stays where it was; a switch that loses a leg degrades to a
    /// joint. Sections keep their (possibly empty) slot — section ids stay
    /// valid that way.
    pub fn remove_edge(&mut self, index: usize) {
        if index >= self.edges.len() {
            return;
        }
        let removed = index as u32;
        let mut removed_edges = vec![removed];

        // Re-anchor followers while the end pose still exists. A source that
        // does not compile has no pose to give — the followers go as well.
        let followers: Vec<usize> = self
            .edges
            .iter()
            .enumerate()
            .filter(|(_, e)| matches!(e.start, EdgeStart::Continue { edge } if edge == removed))
            .map(|(n, _)| n)
            .collect();
        if !followers.is_empty() {
            match self.compile() {
                Ok(compiled) => {
                    let (point, heading_deg) = self.end_anchor(&compiled, index);
                    for n in followers {
                        self.edges[n].start = EdgeStart::Geo { point, heading_deg };
                    }
                }
                Err(_) => {
                    let mut grew = true;
                    while grew {
                        grew = false;
                        for (n, e) in self.edges.iter().enumerate() {
                            if !removed_edges.contains(&(n as u32))
                                && matches!(e.start, EdgeStart::Continue { edge }
                                    if removed_edges.contains(&edge))
                            {
                                removed_edges.push(n as u32);
                                grew = true;
                            }
                        }
                    }
                }
            }
        }
        removed_edges.sort_unstable();
        let edge_map = |old: u32| -> Option<u32> {
            (!removed_edges.contains(&old))
                .then(|| old - removed_edges.iter().filter(|&&r| r < old).count() as u32)
        };

        // Devices on removed edges go, the rest move down — and the signals
        // and routes that referenced them follow.
        let removed_devices: Vec<u32> = self
            .devices
            .iter()
            .enumerate()
            .filter(|(_, d)| edge_map(d.edge).is_none())
            .map(|(n, _)| n as u32)
            .collect();
        self.devices.retain(|d| edge_map(d.edge).is_some());
        for d in &mut self.devices {
            d.edge = edge_map(d.edge).expect("kept devices sit on kept edges");
        }
        // Marked areas follow their edges: spans on a removed edge go with it, the rest
        // move down. An area that loses every span stays — it is a named thing the author
        // made, and an empty one is visibly empty in the panel rather than silently gone.
        for area in &mut self.areas {
            area.spans.retain(|span| edge_map(span.edge).is_some());
            for span in &mut area.spans {
                span.edge = edge_map(span.edge).expect("kept spans sit on kept edges");
            }
        }
        // Scenery objects follow their edge; nothing references them by index.
        self.objects.retain(|o| edge_map(o.edge).is_some());
        for o in &mut self.objects {
            o.edge = edge_map(o.edge).expect("kept objects sit on kept edges");
        }
        // A road whose track is gone is gone with it; the rest follow their edge.
        self.yards.retain(|y| edge_map(y.edge).is_some());
        for y in &mut self.yards {
            y.edge = edge_map(y.edge).expect("kept yards sit on kept edges");
        }
        let removed_signals: Vec<u32> = self
            .signals
            .iter()
            .enumerate()
            .filter(|(_, s)| removed_devices.contains(&s.device))
            .map(|(n, _)| n as u32)
            .collect();
        self.signals
            .retain(|s| !removed_devices.contains(&s.device));
        for s in &mut self.signals {
            s.device -= removed_devices.iter().filter(|&&r| r < s.device).count() as u32;
        }
        self.drop_signal_refs(&removed_signals);

        for section in &mut self.sections {
            section.edges.retain(|&e| edge_map(e).is_some());
            for e in &mut section.edges {
                *e = edge_map(*e).expect("kept section edges are kept edges");
            }
        }
        for node in &mut self.nodes {
            if let NodeSource::Switch {
                root,
                straight,
                diverging,
                ..
            } = node
            {
                match (
                    edge_map(root.0),
                    edge_map(straight.0),
                    edge_map(diverging.0),
                ) {
                    (Some(r), Some(s), Some(d)) => {
                        root.0 = r;
                        straight.0 = s;
                        diverging.0 = d;
                    }
                    _ => *node = NodeSource::Joint,
                }
            }
        }

        let mut n = 0u32;
        self.edges.retain(|_| {
            let keep = !removed_edges.contains(&n);
            n += 1;
            keep
        });
        for e in &mut self.edges {
            if let EdgeStart::Continue { edge } = &mut e.start {
                *edge = edge_map(*edge).expect("followers were re-anchored or removed");
            }
        }
        // A joint that lost its other side is an open end now — a buffer, so
        // the lay and join tools find it again and a boundary may sit on it.
        for n in 0..self.nodes.len() {
            if matches!(self.nodes[n], NodeSource::Joint) && self.node_ends(n as u32).len() < 2 {
                self.nodes[n] = NodeSource::Buffer;
            }
        }
    }

    /// Removes signal table entry `index`. The device stays where it is — it
    /// simply carries no signal any more.
    pub fn remove_signal(&mut self, index: usize) {
        if index >= self.signals.len() {
            return;
        }
        self.signals.remove(index);
        self.drop_signal_refs(&[index as u32]);
    }

    /// Removes the given signal indices from every cross reference: routes on
    /// them disappear, `next` links onto them are cleared, magnets lose the
    /// signal they were linked to, the rest are remapped. `self.signals`
    /// itself must already be filtered.
    fn drop_signal_refs(&mut self, removed: &[u32]) {
        if removed.is_empty() {
            return;
        }
        let map = |old: u32| -> Option<u32> {
            (!removed.contains(&old))
                .then(|| old - removed.iter().filter(|&&r| r < old).count() as u32)
        };
        for s in &mut self.signals {
            s.next = s.next.and_then(map);
        }
        // A magnet names its signal inside the payload text; left alone it
        // would point at whatever moved into that slot.
        for d in &mut self.devices {
            if d.kind == DeviceKind::Magnet
                && let Ok(mut p) = ron::from_str::<MagnetPayload>(&d.payload)
                && let Some(signal) = p.signal
            {
                p.signal = map(signal);
                d.payload = ron::to_string(&p).expect("serializable");
            }
        }
        self.routes
            .retain(|r| map(r.entry).is_some() && map(r.exit).is_some());
        for r in &mut self.routes {
            r.entry = map(r.entry).expect("kept routes reference kept signals");
            r.exit = map(r.exit).expect("kept routes reference kept signals");
            // A protecting signal that is gone protects nothing; the rest move
            // down with the table.
            r.flank = r
                .flank
                .iter()
                .filter_map(|g| match g {
                    FlankSource::Signal(signal) => map(*signal).map(FlankSource::Signal),
                    guard => Some(*guard),
                })
                .collect();
        }
    }

    /// Removes section `index`. Everything that addresses a section by index
    /// follows: guarded lists, route sections and overlaps, and the block
    /// marker payloads. A marker that pointed at the removed section loses its
    /// payload rather than inheriting the next one — the rule check then says
    /// so instead of the line silently marking the wrong block.
    pub fn remove_section(&mut self, index: usize) {
        if index >= self.sections.len() {
            return;
        }
        self.sections.remove(index);
        let removed = index as u32;
        let map =
            |old: u32| -> Option<u32> { (old != removed).then(|| old - u32::from(old > removed)) };
        for s in &mut self.signals {
            s.guarded = s.guarded.iter().filter_map(|g| map(*g)).collect();
        }
        for r in &mut self.routes {
            r.sections = r.sections.iter().filter_map(|s| map(*s)).collect();
            r.overlap = r.overlap.iter().filter_map(|s| map(*s)).collect();
        }
        for d in &mut self.devices {
            if d.kind == DeviceKind::BlockMarker
                && let Ok(p) = ron::from_str::<BlockMarkerPayload>(&d.payload)
            {
                d.payload = match map(p.section) {
                    Some(section) => {
                        ron::to_string(&BlockMarkerPayload { section }).expect("serializable")
                    }
                    None => String::new(),
                };
            }
        }
    }

    /// Welds node `drop` into node `keep`: every edge end on `drop` moves over,
    /// `drop` leaves the node list and every node index above it moves down —
    /// edges, boundaries, route switches and flank guards follow. `keep`
    /// becomes a joint once it holds two ends; a switch on either node stays
    /// what it is. Nothing changes geometrically: the editor only welds ends
    /// that already lie on the same point.
    pub fn merge_nodes(&mut self, keep: u32, drop: u32) {
        if keep == drop || keep as usize >= self.nodes.len() || drop as usize >= self.nodes.len() {
            return;
        }
        let map = |node: u32| -> u32 {
            let node = if node == drop { keep } else { node };
            if node > drop { node - 1 } else { node }
        };
        for e in &mut self.edges {
            e.from = map(e.from);
            e.to = map(e.to);
        }
        for b in &mut self.boundaries {
            b.node = map(b.node);
        }
        for r in &mut self.routes {
            for (node, _) in &mut r.switches {
                *node = map(*node);
            }
            for guard in &mut r.flank {
                if let FlankSource::Switch(node, _) = guard {
                    *node = map(*node);
                }
            }
        }
        let kept = map(keep) as usize;
        self.nodes.remove(drop as usize);
        if matches!(self.nodes[kept], NodeSource::Buffer) && self.node_ends(kept as u32).len() >= 2
        {
            self.nodes[kept] = NodeSource::Joint;
        }
    }

    /// Ends of node `node`, in the `(edge, at end)` form the switch fields use.
    pub fn node_ends(&self, node: u32) -> Vec<(u32, bool)> {
        let mut ends = Vec::new();
        for (i, e) in self.edges.iter().enumerate() {
            if e.from == node {
                ends.push((i as u32, false));
            }
            if e.to == node {
                ends.push((i as u32, true));
            }
        }
        ends
    }

    /// Where a path continues beyond `node` when it arrives over `incoming`,
    /// with the switch position each continuation requires.
    ///
    /// Unlike [`TrackNetwork::continuation`] this ignores the position the
    /// switch happens to lie in: a route is what *makes* it lie somewhere.
    fn continuations(&self, node: u32, incoming: (u32, bool)) -> Vec<Continuation> {
        match self.nodes.get(node as usize) {
            Some(NodeSource::Joint) => self
                .node_ends(node)
                .into_iter()
                .filter(|end| *end != incoming)
                .map(|end| (end, None))
                .collect(),
            Some(NodeSource::Switch {
                root,
                straight,
                diverging,
                ..
            }) => {
                if incoming == *root {
                    // Facing move: either leg, each over its own position.
                    vec![
                        (*straight, Some((node, SwitchPosition::Straight))),
                        (*diverging, Some((node, SwitchPosition::Diverging))),
                    ]
                } else if incoming == *straight {
                    vec![(*root, Some((node, SwitchPosition::Straight)))]
                } else if incoming == *diverging {
                    vec![(*root, Some((node, SwitchPosition::Diverging)))]
                } else {
                    Vec::new()
                }
            }
            // A buffer ends the path; an index out of range is no node at all.
            _ => Vec::new(),
        }
    }

    /// Length of an edge [m] — the source carries it as its segment chain.
    fn edge_length(&self, edge: u32) -> f64 {
        self.edges
            .get(edge as usize)
            .map(|e| e.segments.iter().map(|g| g.len).sum())
            .unwrap_or(0.0)
    }

    /// Permitted speed at `(edge, s)` [km/h] out of the edge's step profile;
    /// `None` where the edge states none (same rule as `StepProfile::new` —
    /// the first entry also applies before its own `s`).
    fn speed_at(&self, edge: u32, s: f64) -> Option<f64> {
        let steps = &self.edges.get(edge as usize)?.speed;
        steps
            .iter()
            .filter(|(x, _)| *x <= s)
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .or_else(|| steps.iter().min_by(|a, b| a.0.total_cmp(&b.0)))
            .map(|(_, v)| *v)
    }

    /// The signal that would hold a flank movement coming over `end` — the
    /// last one such a vehicle would have to pass before it reaches the node
    /// that `end` hangs on.
    fn guarding_signal(&self, end: (u32, bool)) -> Option<u32> {
        let (edge, at_end) = end;
        // A vehicle running towards that node runs towards this end.
        let dir: i8 = if at_end { 1 } else { -1 };
        self.signals
            .iter()
            .enumerate()
            // Everything that can show stop by itself holds a flank — a
            // track lock above all, which is what it is there for.
            .filter(|(_, s)| s.kind.holds_a_flank())
            .filter_map(|(i, s)| {
                let device = self.devices.get(s.device as usize)?;
                (device.edge == edge && device.facing.applies(dir))
                    .then_some((i as u32, device.s * dir as f64))
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// What protects a route against a movement coming out of `branch` — the
    /// leg of a trailing turnout the route does not use (Ril 819): the first
    /// signal that would hold such a movement, or the first turnout that can
    /// be laid so it leads the movement away.
    ///
    /// `None` where nothing is needed or nothing is there: a leg that ends in
    /// a buffer stop needs no guard, and one that opens into a turnout at its
    /// root would need every leg beyond it protected — which is a station
    /// layout the builder has to answer for, not the search.
    fn flank_guard(&self, branch: (u32, bool)) -> Option<FlankSource> {
        let mut current = branch;
        // Eight edges is a long throat; beyond it the guard belongs elsewhere.
        for _ in 0..8 {
            if let Some(signal) = self.guarding_signal(current) {
                return Some(FlankSource::Signal(signal));
            }
            let (edge, near) = current;
            // The far end of that edge, where the next node sits.
            let far = !near;
            let source = self.edges.get(edge as usize)?;
            let node = if far { source.to } else { source.from };
            match self.nodes.get(node as usize)? {
                NodeSource::Buffer => return None,
                NodeSource::Joint => {
                    current = *self.node_ends(node).iter().find(|e| **e != (edge, far))?
                }
                NodeSource::Switch {
                    straight,
                    diverging,
                    ..
                } => {
                    let incoming = (edge, far);
                    return if incoming == *straight {
                        Some(FlankSource::Switch(node, SwitchPosition::Diverging))
                    } else if incoming == *diverging {
                        Some(FlankSource::Switch(node, SwitchPosition::Straight))
                    } else {
                        None // reached at the root — it leads into both legs
                    };
                }
            }
        }
        None
    }

    /// Flank protection needed where a path passes `node` coming over
    /// `incoming`: only a turnout run into trailing exposes the route, because
    /// the leg it does not use joins the path there. A facing turnout guards
    /// the route itself by lying in the position the route needs.
    fn flank_at(&self, node: u32, incoming: (u32, bool), flank: &mut Vec<FlankSource>) {
        let Some(NodeSource::Switch {
            straight,
            diverging,
            ..
        }) = self.nodes.get(node as usize)
        else {
            return;
        };
        let other = if incoming == *straight {
            *diverging
        } else if incoming == *diverging {
            *straight
        } else {
            return; // facing move
        };
        if let Some(guard) = self.flank_guard(other)
            && !flank.contains(&guard)
        {
            flank.push(guard);
        }
    }

    /// The sections an overlap covers: on from `(edge, s)` in direction `dir`
    /// for `length` metres, straight ahead wherever the track forks. Switches
    /// on the way are appended to `switches`, because a route has to lock the
    /// overlap's turnouts as well as its own.
    ///
    /// The walk stops at a buffer, at a switch the route already needs the
    /// other way, and after 32 edges — an overlap that long is a wiring
    /// mistake, not a D-way.
    fn overlap_after(
        &self,
        edge: u32,
        s: f64,
        dir: i8,
        length: f64,
        switches: &mut Vec<(u32, SwitchPosition)>,
        flank: &mut Vec<FlankSource>,
    ) -> Vec<u32> {
        // What is left of the exit signal's own edge behind it.
        let mut remaining = length
            - if dir > 0 {
                self.edge_length(edge) - s
            } else {
                s
            };
        let mut current = (edge, dir);
        let mut path = Vec::new();
        for _ in 0..32 {
            if remaining <= 0.0 {
                break;
            }
            let (e, d) = current;
            let Some(source) = self.edges.get(e as usize) else {
                break;
            };
            let (node, at_end) = if d > 0 {
                (source.to, true)
            } else {
                (source.from, false)
            };
            // The overlap is part of what the route protects, so a turnout it
            // trails needs its flank guard just as one in the path does.
            self.flank_at(node, (e, at_end), flank);
            let mut options = self.continuations(node, (e, at_end));
            // Straight on where there is a choice: an overlap is the plain
            // continuation of the route, not a second diverging move.
            let chosen = if options.len() == 1 {
                options.pop()
            } else {
                options
                    .into_iter()
                    .find(|(_, sw)| matches!(sw, Some((_, SwitchPosition::Straight))))
            };
            let Some((next, switch)) = chosen else {
                break;
            };
            if let Some((n, position)) = switch {
                if switches.iter().any(|(m, p)| *m == n && *p != position) {
                    break;
                }
                if !switches.contains(&(n, position)) {
                    switches.push((n, position));
                }
            }
            path.push(next.0);
            remaining -= self.edge_length(next.0);
            current = (next.0, if next.1 { -1 } else { 1 });
        }
        self.sections_over(&path)
    }

    /// The sections a path runs through, in the order it enters them.
    fn sections_over(&self, path: &[u32]) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        for edge in path {
            for (i, section) in self.sections.iter().enumerate() {
                if section.edges.contains(edge) && !out.contains(&(i as u32)) {
                    out.push(i as u32);
                }
            }
        }
        out
    }

    /// The first signal on `edge` beyond `from_s` in direction `dir` that a
    /// route can end at. A distant signal announces, it does not end a route.
    fn next_target_signal(&self, edge: u32, from_s: f64, dir: i8) -> Option<u32> {
        self.signals
            .iter()
            .enumerate()
            .filter(|(_, s)| s.kind.ends_a_route())
            .filter_map(|(i, s)| {
                let device = self.devices.get(s.device as usize)?;
                let ahead = (device.s - from_s) * dir as f64;
                (device.edge == edge && device.facing.applies(dir) && ahead > 0.0)
                    .then_some((i as u32, ahead))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// Every route that can start at signal `entry`: the search runs out over
    /// the track graph and ends each branch at the next signal it can end at,
    /// so a turnout ahead of the signal yields one route per leg. This is the
    /// list a signal carries in the Zusi editor — what leaves here, and where
    /// each of them ends.
    ///
    /// The routes themselves come out of [`route_between`], so sections,
    /// switch positions and overlap follow the same rules.
    pub fn routes_from(&self, entry: u32, overlap: Option<f64>) -> Vec<RouteSource> {
        let Some(entry_signal) = self.signals.get(entry as usize) else {
            return Vec::new();
        };
        // Routes start where a train move is authorised — not at a distant
        // signal, which announces, nor at a track lock, which secures.
        if !entry_signal.kind.ends_a_route() {
            return Vec::new();
        }
        let Some(entry_device) = self.devices.get(entry_signal.device as usize) else {
            return Vec::new();
        };
        let mut queue = std::collections::VecDeque::new();
        let mut seen = std::collections::HashSet::new();
        // A set: two legs may run into the same signal, and that is one route.
        let mut targets = std::collections::BTreeSet::new();
        for dir in [1i8, -1] {
            if !entry_device.facing.applies(dir) || !seen.insert((entry_device.edge, dir)) {
                continue;
            }
            match self.next_target_signal(entry_device.edge, entry_device.s, dir) {
                Some(target) => {
                    targets.insert(target);
                }
                None => queue.push_back((entry_device.edge, dir)),
            }
        }
        while let Some((edge, dir)) = queue.pop_front() {
            let Some(source) = self.edges.get(edge as usize) else {
                continue;
            };
            let (node, at_end) = if dir > 0 {
                (source.to, true)
            } else {
                (source.from, false)
            };
            for (next, _) in self.continuations(node, (edge, at_end)) {
                let dir = if next.1 { -1 } else { 1 };
                if !seen.insert((next.0, dir)) {
                    continue;
                }
                // Entered at its near end, so the whole edge lies ahead.
                let from = if dir > 0 {
                    f64::NEG_INFINITY
                } else {
                    f64::INFINITY
                };
                match self.next_target_signal(next.0, from, dir) {
                    Some(target) => {
                        targets.insert(target);
                    }
                    None => queue.push_back((next.0, dir)),
                }
            }
        }
        targets
            .into_iter()
            .filter_map(|target| self.route_between(entry, target, overlap))
            .collect()
    }

    /// Fills a route in from the geometry: the shortest path across the track
    /// graph from the entry signal to the exit signal, and out of it the
    /// sections it runs through, the position every turnout on the way needs
    /// and whether it leads over a diverging leg.
    ///
    /// The entry signal's own edge stays out of the sections — a route starts
    /// at the signal, and the section in front of it is where the train
    /// stands, which `Interlock::request_route` would find occupied.
    ///
    /// The **overlap** is walked on behind the exit signal, for `overlap`
    /// metres or, with `None`, for the [`regular_overlap`] of the speed the
    /// route ends at — the diverging speed of the entry signal where it leads
    /// over a diverging leg, otherwise the permitted speed at the exit signal.
    /// Its sections are the ones the route does not already lock, and the
    /// turnouts inside it join `switches`.
    ///
    /// `None` when nothing connects the two — no path, or only paths that
    /// would need one switch in both positions at once.
    ///
    /// ponytail: breadth-first over `(edge, direction)` with one visit per
    /// state, so it returns the path over the fewest edges and nothing else.
    /// Where a builder wants the long way round, the fields stay editable.
    pub fn route_between(
        &self,
        entry: u32,
        exit: u32,
        overlap: Option<f64>,
    ) -> Option<RouteSource> {
        if entry == exit {
            return None;
        }
        let entry_signal = self.signals.get(entry as usize)?;
        let entry_device = self.devices.get(entry_signal.device as usize)?;
        let exit_device = self
            .devices
            .get(self.signals.get(exit as usize)?.device as usize)?;

        struct Step {
            edge: u32,
            dir: i8,
            path: Vec<u32>,
            switches: Vec<(u32, SwitchPosition)>,
            flank: Vec<FlankSource>,
        }
        let mut queue = std::collections::VecDeque::new();
        let mut seen = std::collections::HashSet::new();
        for dir in [1i8, -1] {
            if entry_device.facing.applies(dir) && seen.insert((entry_device.edge, dir)) {
                queue.push_back(Step {
                    edge: entry_device.edge,
                    dir,
                    path: Vec::new(),
                    switches: Vec::new(),
                    flank: Vec::new(),
                });
            }
        }

        while let Some(step) = queue.pop_front() {
            // On the entry signal's own edge the exit has to lie ahead of it;
            // every other edge is entered at its near end, so anything on it
            // is ahead by construction.
            if step.edge == exit_device.edge
                && (!step.path.is_empty()
                    || (exit_device.s - entry_device.s) * step.dir as f64 > 0.0)
            {
                let mut switches = step.switches;
                let mut flank = step.flank;
                let diverging = switches
                    .iter()
                    .any(|(_, p)| *p == SwitchPosition::Diverging);
                let sections = self.sections_over(&step.path);
                // The speed the route ends at decides the regular overlap: a
                // diverging route is entered at the entry signal's Zs3 speed,
                // a straight one at what the line permits at the exit signal.
                let speed = if diverging {
                    entry_signal.diverging_speed.unwrap_or(40.0)
                } else {
                    self.speed_at(exit_device.edge, exit_device.s)
                        .unwrap_or(100.0)
                };
                let length = overlap.unwrap_or_else(|| regular_overlap(speed));
                let overlap = self
                    .overlap_after(
                        exit_device.edge,
                        exit_device.s,
                        step.dir,
                        length,
                        &mut switches,
                        &mut flank,
                    )
                    .into_iter()
                    // What the route locks anyway needs no second entry.
                    .filter(|s| !sections.contains(s))
                    .collect();
                return Some(RouteSource {
                    entry,
                    exit,
                    // *Find routes* looks for train routes; a shunting route is a
                    // decision about how a place is worked, not something to be found on
                    // the track, and the editor writes it by hand.
                    kind: sim_core::interlock::RouteKind::Train,
                    diverging,
                    switches,
                    sections,
                    overlap,
                    flank,
                });
            }
            let Some(edge) = self.edges.get(step.edge as usize) else {
                continue;
            };
            let (node, at_end) = if step.dir > 0 {
                (edge.to, true)
            } else {
                (edge.from, false)
            };
            let mut flank = step.flank.clone();
            self.flank_at(node, (step.edge, at_end), &mut flank);
            for (next, switch) in self.continuations(node, (step.edge, at_end)) {
                if let Some((n, position)) = switch
                    && step.switches.iter().any(|(m, p)| *m == n && *p != position)
                {
                    continue; // this path already fixed that switch the other way
                }
                let dir = if next.1 { -1 } else { 1 };
                if !seen.insert((next.0, dir)) {
                    continue;
                }
                let mut path = step.path.clone();
                path.push(next.0);
                let mut switches = step.switches.clone();
                switches.extend(switch);
                queue.push_back(Step {
                    edge: next.0,
                    dir,
                    path,
                    switches,
                    flank: flank.clone(),
                });
            }
        }
        None
    }

    /// Geo anchor (point + compass heading) of the end of compiled edge `index` —
    /// what an edge needs to stand on its own where a `Continue` no longer holds.
    fn end_anchor(&self, compiled: &CompiledLine, index: usize) -> (GeoPoint, f64) {
        let edge = &compiled.net.edges()[index];
        let end = edge.end_pose().pos;
        let heading: f64 = edge.heading0
            + edge
                .segments
                .iter()
                .map(|s| s.heading_delta(s.len))
                .sum::<f64>();
        let (lat, lon, height) = world_coords::geo::from_ecef(end);
        let point = GeoPoint {
            lat: lat.to_degrees(),
            lon: lon.to_degrees(),
            height: height - self.geoid_offset,
        };
        (point, (90.0 - heading.to_degrees()).rem_euclid(360.0))
    }

    /// Splits edge `index` at arc length `s` into two edges joined by a new
    /// `Joint` node. The second half is appended at the end of the edge list,
    /// so no other edge index moves; devices beyond the cut, step profiles,
    /// switch legs and sections follow, and edges that continued from the old
    /// end are anchored geographically where that end was. Returns
    /// `(joint node, second-half edge index)`.
    ///
    /// Refuses a cut closer than 1 m to either end (a zero-length stub is no
    /// track) and a source that does not compile (nothing to re-anchor against).
    pub fn split_edge(&mut self, index: usize, s: f64) -> Option<(u32, u32)> {
        let length: f64 = self.edges.get(index)?.segments.iter().map(|g| g.len).sum();
        if s < 1.0 || s > length - 1.0 {
            return None;
        }
        // Validity gate only — a source that does not compile cannot re-anchor
        // its followers below.
        self.compile().ok()?;

        let new_index = self.edges.len() as u32;
        let (first, second) = split_segments(&self.edges[index].segments, s);
        let (grade_a, grade_b) = split_steps(&self.edges[index].grade, s);
        let (cant_a, cant_b) = split_steps(&self.edges[index].cant, s);
        let (speed_a, speed_b) = split_steps(&self.edges[index].speed, s);
        let (type_a, type_b) = split_steps(&self.edges[index].track_type, s);
        let (power_a, power_b) = split_steps(&self.edges[index].electrification, s);

        for d in &mut self.devices {
            if d.edge as usize == index && d.s >= s {
                d.edge = new_index;
                d.s -= s;
            }
        }
        for o in &mut self.objects {
            if o.edge as usize == index && o.s >= s {
                o.edge = new_index;
                o.s -= s;
            }
        }
        for y in &mut self.yards {
            if y.edge as usize == index && y.s >= s {
                y.edge = new_index;
                y.s -= s;
            }
        }
        // A switch leg attached to the old end now hangs on the second half.
        for node in &mut self.nodes {
            if let NodeSource::Switch {
                root,
                straight,
                diverging,
                ..
            } = node
            {
                for leg in [root, straight, diverging] {
                    if leg.0 as usize == index && leg.1 {
                        leg.0 = new_index;
                    }
                }
            }
        }
        // A marked area follows the cut: what lay beyond it moves to the second half, and
        // a span straddling the cut becomes two — the marking on the map does not move.
        for area in &mut self.areas {
            let mut added = Vec::new();
            for span in &mut area.spans {
                if span.edge as usize != index {
                    continue;
                }
                if span.from >= s {
                    span.edge = new_index;
                    span.from -= s;
                    span.to -= s;
                } else if span.to > s {
                    added.push(AreaSpan {
                        edge: new_index,
                        from: 0.0,
                        to: span.to - s,
                    });
                    span.to = s;
                }
            }
            area.spans.extend(added);
        }
        // Both halves stay one occupancy unit — section ids keep their meaning.
        for section in &mut self.sections {
            if section.edges.contains(&(index as u32)) {
                section.edges.push(new_index);
            }
        }

        let joint = self.nodes.len() as u32;
        self.nodes.push(NodeSource::Joint);
        let old_to = self.edges[index].to;
        let formation = self.edges[index].formation;
        self.edges[index].to = joint;
        self.edges[index].segments = first;
        self.edges[index].grade = grade_a;
        self.edges[index].cant = cant_a;
        self.edges[index].speed = speed_a;
        self.edges[index].track_type = type_a;
        self.edges[index].electrification = power_a;
        self.edges.push(EdgeSource {
            from: joint,
            to: old_to,
            start: EdgeStart::Continue { edge: index as u32 },
            segments: second,
            grade: grade_b,
            cant: cant_b,
            speed: speed_b,
            track_type: type_b,
            electrification: power_b,
            formation,
        });

        // Followers continued from the old end, which now belongs to the second
        // half. `Continue { new_index }` would be a forward reference, so they
        // are anchored geographically — at the end the second half has *now*:
        // the cut re-levels the tangent planes, which shifts long edges' ends
        // by the removed curvature-approximation error (sub-metre per km).
        let followers: Vec<usize> = self
            .edges
            .iter()
            .enumerate()
            .filter(|(n, e)| {
                *n != new_index as usize
                    && matches!(e.start, EdgeStart::Continue { edge } if edge as usize == index)
            })
            .map(|(n, _)| n)
            .collect();
        if !followers.is_empty()
            && let Ok(compiled) = self.compile()
        {
            let (point, heading_deg) = self.end_anchor(&compiled, new_index as usize);
            for n in followers {
                self.edges[n].start = EdgeStart::Geo { point, heading_deg };
            }
        }
        Some((joint, new_index))
    }

    /// Rule check of the source file: the wiring mistakes that compile fine
    /// but fail on the line — a distant signal without its 1000 Hz magnet, a
    /// device beyond its track, a boundary on a node that is no buffer, a
    /// track type or scenery object no installed mod has (the registries map
    /// `"<mod>:<name>"` → spec).
    pub fn check(
        &self,
        types: &std::collections::BTreeMap<String, TrackType>,
        objects: &std::collections::BTreeMap<String, TrackObject>,
    ) -> Vec<RuleIssue> {
        let mut issues = Vec::new();

        if envelope_self_intersects(&self.envelope) {
            issues.push(RuleIssue::EnvelopeSelfIntersects);
        }
        // Landscape that the envelope no longer covers — see `OutsideEnvelope`.
        if self.envelope.len() >= 3 {
            let trees = self
                .trees
                .iter()
                .filter(|t| !self.envelope_contains(t.lat, t.lon))
                .count() as u32;
            let terrain = self
                .terrain
                .iter()
                .filter(|t| !self.envelope_contains(t.lat, t.lon))
                .count() as u32;
            let markers = self
                .markers
                .iter()
                .filter(|m| !self.envelope_contains(m.lat, m.lon))
                .count() as u32;
            // Walkways vertex by vertex, not way by way: a corner of the
            // envelope dragged inwards leaves a path's far end on the
            // neighbour's ground, and it is the vertices that have to move.
            let walkways = self
                .walk_paths
                .iter()
                .flat_map(|p| p.points.iter())
                .chain(self.walk_areas.iter().flat_map(|a| a.polygon.iter()))
                .filter(|v| !self.envelope_contains(v.lat, v.lon))
                .count() as u32;
            // Fields corner by corner for the same reason. With a margin,
            // unlike the trees: the import cuts a field *to* the boundary, so
            // its outermost corners lie exactly on the polygon, where ray
            // casting is undefined — the same reason the track has one.
            let fields = self
                .fields
                .iter()
                .flat_map(|f| f.polygon.iter())
                .filter(|v| !self.envelope_contains_within(v.lat, v.lon, FIELD_MARGIN))
                .count() as u32;
            if trees + terrain + markers + walkways + fields > 0 {
                issues.push(RuleIssue::OutsideEnvelope {
                    trees,
                    terrain,
                    markers,
                    walkways,
                    fields,
                });
            }
        }

        // Fields: three corners at least, and a crop the tables know. A crop
        // that is not known is not fatal — the field is drawn as bare ground —
        // but it is always either a typo or a mod that has not been installed.
        for (i, field) in self.fields.iter().enumerate() {
            if field.polygon.len() < 3 {
                issues.push(RuleIssue::FieldTooSmall { field: i as u32 });
            }
            if fields::CropClass::from_id(&field.crop).is_none() {
                issues.push(RuleIssue::FieldUnknownCrop { field: i as u32 });
            }
        }

        // Walkways: a path needs a start and an end, an area three corners. Nobody on
        // them is not an error — a way may be laid out before it is peopled.
        for (i, path) in self.walk_paths.iter().enumerate() {
            if path.points.len() < 2 {
                issues.push(RuleIssue::WalkPathTooShort { path: i as u32 });
            }
        }
        for (i, area) in self.walk_areas.iter().enumerate() {
            if area.polygon.len() < 3 {
                issues.push(RuleIssue::WalkAreaTooSmall { area: i as u32 });
            }
        }

        // Roads: a centre line of two points at least. Width and markings are
        // clamped by the builder rather than checked — a road of odd width is
        // odd-looking, not broken.
        for (i, road) in self.roads.iter().enumerate() {
            if road.points.len() < 2 {
                issues.push(RuleIssue::RoadTooShort { road: i as u32 });
            }
        }

        // Scenery objects: on their track, and of a kind some mod defines.
        let lengths_of = |edge: u32| -> Option<f64> {
            self.edges
                .get(edge as usize)
                .map(|e| e.segments.iter().map(|g| g.len).sum())
        };
        for (i, o) in self.objects.iter().enumerate() {
            let object = i as u32;
            match lengths_of(o.edge) {
                Some(len) if (0.0..=len).contains(&o.s) => {}
                _ => issues.push(RuleIssue::ObjectOffEdge { object }),
            }
            if !objects.contains_key(&o.object) {
                issues.push(RuleIssue::UnknownObject { object });
            }
        }

        // Stabling roads and portals: on their track, uniquely named, and — for a portal —
        // at the edge of the line, which is where trains are allowed to appear from.
        for (i, y) in self.yards.iter().enumerate() {
            let yard = i as u32;
            match lengths_of(y.edge) {
                Some(len) if (0.0..=len).contains(&y.s) => {}
                _ => {
                    issues.push(RuleIssue::YardOffEdge { yard });
                    continue;
                }
            }
            if self.yards[..i].iter().any(|other| other.name == y.name) {
                issues.push(RuleIssue::DuplicateYardName { yard });
            }
            if y.kind == YardKind::Portal && !self.portal_reaches_the_edge(y) {
                issues.push(RuleIssue::PortalNotAtTheEdge { yard });
            }
        }

        // Track types: unknown names, and LZB superstructure whose line
        // conductor was never placed — the type says what belongs on the
        // track, the device is what the LZB actually reads.
        let has_conductor = self
            .devices
            .iter()
            .any(|d| d.kind == DeviceKind::LineConductor);
        for (i, e) in self.edges.iter().enumerate() {
            let edge = i as u32;
            if e.track_type
                .iter()
                .any(|(_, name)| name != "default" && !types.contains_key(name))
            {
                issues.push(RuleIssue::UnknownTrackType { edge });
            }
            if !has_conductor
                && e.track_type
                    .iter()
                    .any(|(_, name)| types.get(name).is_some_and(|t| t.lzb))
            {
                issues.push(RuleIssue::LzbTypeWithoutConductor { edge });
            }
        }
        let lengths: Vec<f64> = self
            .edges
            .iter()
            .map(|e| e.segments.iter().map(|g| g.len).sum())
            .collect();

        // Marked areas: on their track, reaching the line, and naming a type that exists.
        for (i, area) in self.areas.iter().enumerate() {
            let index = i as u32;
            let off = area.spans.iter().any(|span| {
                match lengths.get(span.edge as usize) {
                    None => true,
                    // A stretch that starts past the end of its track marks nothing; one
                    // that runs past it is simply clamped and is not worth a finding.
                    Some(length) => span.from >= *length || span.to <= 0.0 || span.to <= span.from,
                }
            });
            if off {
                issues.push(RuleIssue::AreaOffTrack { area: index });
            }
            if area.spans.is_empty() || !area.sets_anything() {
                issues.push(RuleIssue::AreaWithoutEffect { area: index });
            }
            if area
                .track_type
                .as_ref()
                .is_some_and(|name| name != DEFAULT_TRACK_TYPE && !types.contains_key(name))
            {
                issues.push(RuleIssue::AreaUnknownTrackType { area: index });
            }
        }

        let mut magnets: Vec<MagnetPayload> = Vec::new();
        for (i, d) in self.devices.iter().enumerate() {
            let device = i as u32;
            match lengths.get(d.edge as usize) {
                Some(len) if (0.0..=*len).contains(&d.s) => {}
                _ => issues.push(RuleIssue::DeviceOffEdge { device }),
            }
            match d.kind {
                DeviceKind::Magnet => match ron::from_str::<MagnetPayload>(&d.payload) {
                    Ok(p) if p.signal.is_none_or(|g| (g as usize) < self.signals.len()) => {
                        magnets.push(p);
                    }
                    _ => issues.push(RuleIssue::MagnetPayloadInvalid { device }),
                },
                DeviceKind::BlockMarker => match ron::from_str::<BlockMarkerPayload>(&d.payload) {
                    Ok(p) if (p.section as usize) < self.sections.len() => {}
                    _ => issues.push(RuleIssue::BlockMarkerPayloadInvalid { device }),
                },
                _ => {}
            }
        }

        let linked = |signal: u32, frequency: MagnetFrequency| {
            magnets
                .iter()
                .any(|p| p.frequency == frequency && p.signal == Some(signal))
        };
        for (j, sig) in self.signals.iter().enumerate() {
            let signal = j as u32;
            match self.devices.get(sig.device as usize) {
                Some(d) if d.kind == DeviceKind::Signal => {}
                _ => issues.push(RuleIssue::SignalDeviceMismatch { signal }),
            }
            // A Ks combination signal carries both functions, so both magnets.
            if matches!(sig.kind, SignalKind::Distant | SignalKind::Combined)
                && !linked(signal, MagnetFrequency::Hz1000)
            {
                issues.push(RuleIssue::DistantWithout1000Hz { signal });
            }
            if matches!(sig.kind, SignalKind::Main | SignalKind::Combined)
                && !linked(signal, MagnetFrequency::Hz2000)
            {
                issues.push(RuleIssue::MainWithout2000Hz { signal });
            }
            if sig.kind == SignalKind::Distant && sig.next.is_none() {
                issues.push(RuleIssue::DistantWithoutNext { signal });
            }
        }

        // Flank protection: a guard that addresses nothing protects nothing.
        for (i, r) in self.routes.iter().enumerate() {
            let broken = r.flank.iter().any(|g| match g {
                FlankSource::Switch(node, _) => !matches!(
                    self.nodes.get(*node as usize),
                    Some(NodeSource::Switch { .. })
                ),
                // A distant signal announces; it holds nothing at stop.
                FlankSource::Signal(signal) => self
                    .signals
                    .get(*signal as usize)
                    .is_none_or(|s| !s.kind.holds_a_flank()),
            });
            if broken {
                issues.push(RuleIssue::FlankGuardInvalid { route: i as u32 });
            }
        }

        for (b, boundary) in self.boundaries.iter().enumerate() {
            match self.nodes.get(boundary.node as usize) {
                Some(NodeSource::Buffer) => {}
                _ => issues.push(RuleIssue::BoundaryInvalid { boundary: b as u32 }),
            }
        }
        issues
    }

    /// Compiles the source file into track network and interlocking.
    pub fn compile(&self) -> Result<CompiledLine, CompileError> {
        let mut net = TrackNetwork::new();

        // Nodes first (switches get their edge ends later).
        let node_ids: Vec<NodeId> = self
            .nodes
            .iter()
            .map(|n| {
                net.add_node(match n {
                    NodeSource::Buffer => NodeKind::Buffer,
                    NodeSource::Joint | NodeSource::Switch { .. } => NodeKind::Joint,
                })
            })
            .collect();

        // Edges in source order; `Continue` may only refer backwards.
        // Track-type names are interned per line: index 0 stays the default
        // type — the reserved name `"default"` addresses it, so a section can
        // return to it mid-edge — and the specs behind the other names come
        // from the mod runtime later (`TrackNetwork::apply_track_types`),
        // like signal types.
        let mut type_names: Vec<String> = Vec::new();
        let intern = |names: &mut Vec<String>, name: &str| -> u32 {
            if name == "default" {
                return 0;
            }
            match names.iter().position(|n| n == name) {
                Some(i) => i as u32 + 1,
                None => {
                    names.push(name.to_string());
                    names.len() as u32
                }
            }
        };
        let mut edge_ids: Vec<EdgeId> = Vec::new();
        for (i, e) in self.edges.iter().enumerate() {
            let (anchor, heading) = match e.start {
                EdgeStart::Geo { point, heading_deg } => (
                    to_ecef_deg(
                        point.lat,
                        point.lon,
                        world_coords::geo::ellipsoidal_height(point.height, self.geoid_offset),
                    ),
                    // Source data gives the heading as a compass bearing, internally
                    // 0 = east and mathematically positive.
                    (90.0 - heading_deg).to_radians(),
                ),
                EdgeStart::Continue { edge } => {
                    let prev = *edge_ids
                        .get(edge as usize)
                        .ok_or(CompileError::ForwardReference(edge))?;
                    if edge as usize >= i {
                        return Err(CompileError::ForwardReference(edge));
                    }
                    let prev_edge = net.edge(prev);
                    let end = prev_edge.end_pose();
                    let heading: f64 = prev_edge.heading0
                        + prev_edge
                            .segments
                            .iter()
                            .map(|s| s.heading_delta(s.len))
                            .sum::<f64>();
                    // The joint gets its own ENU frame; the heading is the same in the
                    // new frame, because ENU frames are only rotated against each other
                    // over long distances (meridian convergence, negligible here).
                    (end.pos, heading)
                }
            };

            let from = *node_ids
                .get(e.from as usize)
                .ok_or(CompileError::UnknownNode(e.from))?;
            let to = *node_ids
                .get(e.to as usize)
                .ok_or(CompileError::UnknownNode(e.to))?;
            let mut edge = TrackEdge::new(EdgeId(0), from, to, anchor, heading, e.segments.clone())
                .with_formation(e.formation);
            // Marked areas are laid over the edge's own profiles, in file order, so a
            // later area wins where two of them overlap.
            let length: f64 = e.segments.iter().map(|g| g.len).sum();
            let spans = |pick: &dyn Fn(&TrackAreaSource) -> Option<f64>| -> Vec<(f64, f64, f64)> {
                self.areas
                    .iter()
                    .flat_map(|area| {
                        let value = pick(area);
                        area.spans
                            .iter()
                            .filter(move |span| span.edge as usize == i)
                            .filter_map(move |span| Some((span.from, span.to, value?)))
                    })
                    .collect()
            };

            let grade = overlay_steps(&e.grade, 0.0, &spans(&|a| a.grade), length);
            if !grade.is_empty() {
                edge = edge.with_grade(StepProfile::new(grade));
            }
            let cant = overlay_steps(&e.cant, 0.0, &spans(&|a| a.cant), length);
            if !cant.is_empty() {
                edge = edge.with_cant(StepProfile::new(cant));
            }
            let speed = overlay_steps(&e.speed, DEFAULT_SPEED, &spans(&|a| a.speed), length);
            if !speed.is_empty() {
                edge = edge.with_speed(StepProfile::new(speed));
            }

            let type_spans: Vec<(f64, f64, String)> = self
                .areas
                .iter()
                .flat_map(|area| {
                    let name = area.track_type.clone();
                    area.spans
                        .iter()
                        .filter(move |span| span.edge as usize == i)
                        .filter_map(move |span| Some((span.from, span.to, name.clone()?)))
                })
                .collect();
            let types = overlay_steps(
                &e.track_type,
                DEFAULT_TRACK_TYPE.to_string(),
                &type_spans,
                length,
            );
            if !types.is_empty() {
                let steps = types
                    .iter()
                    .map(|(s, name)| (*s, intern(&mut type_names, name)))
                    .collect();
                edge = edge.with_track_type(StepProfile::new(steps));
            }

            let power_spans: Vec<(f64, f64, String)> = self
                .areas
                .iter()
                .flat_map(|area| {
                    let id = area.electrification.clone();
                    area.spans
                        .iter()
                        .filter(move |span| span.edge as usize == i)
                        .filter_map(move |span| Some((span.from, span.to, id.clone()?)))
                })
                .collect();
            let power = overlay_steps(
                &e.electrification,
                self.electrification.clone(),
                &power_spans,
                length,
            );
            if !power.is_empty() {
                let steps = power
                    .iter()
                    .map(|(s, id)| (*s, track_model::electrification_from_id(id)))
                    .collect();
                edge = edge.with_electrification(StepProfile::new(steps));
            }
            edge_ids.push(net.add_edge(edge));
        }
        if !type_names.is_empty() {
            let mut types = vec![TrackType::default()];
            types.extend(type_names.iter().map(|n| TrackType::placeholder(n)));
            net.set_types(types);
        }
        net.set_default_electrification(track_model::electrification_from_id(
            &self.electrification,
        ));

        // Wire up the switches.
        for (i, n) in self.nodes.iter().enumerate() {
            if let NodeSource::Switch {
                root,
                straight,
                diverging,
                throw_time,
            } = n
            {
                let resolve = |(edge, at_end): (u32, bool)| -> Result<EdgeEnd, CompileError> {
                    let id = *edge_ids
                        .get(edge as usize)
                        .ok_or(CompileError::UnknownEdge(edge))?;
                    Ok(EdgeEnd::new(
                        id,
                        if at_end {
                            EdgeSide::End
                        } else {
                            EdgeSide::Start
                        },
                    ))
                };
                let mut sw =
                    Switch::new(resolve(*root)?, resolve(*straight)?, resolve(*diverging)?);
                sw.throw_time = *throw_time;
                net.node_mut(node_ids[i]).kind = NodeKind::Switch(sw);
            }
        }

        // Scenery objects are not compiled into the network — the app places
        // them straight from the source — but a dangling edge index is still
        // a broken file.
        for o in &self.objects {
            if o.edge as usize >= edge_ids.len() {
                return Err(CompileError::UnknownEdge(o.edge));
            }
        }

        // Stabling roads and portals: marks on the graph, read by the simulation rather
        // than drawn. The edge ids run in source order, so a yard resolves straight.
        let mut yards = Vec::new();
        for y in &self.yards {
            let edge = *edge_ids
                .get(y.edge as usize)
                .ok_or(CompileError::UnknownEdge(y.edge))?;
            let mut yard = y.compile();
            yard.at.edge = edge;
            yards.push(yard);
        }

        // Trackside devices.
        let mut device_ids = Vec::new();
        for d in &self.devices {
            let edge = *edge_ids
                .get(d.edge as usize)
                .ok_or(CompileError::UnknownEdge(d.edge))?;
            let mut device = TracksideDevice::new(d.kind.clone(), edge, d.s);
            device.facing = d.facing;
            device.lateral_offset = d.lateral_offset;
            if !d.payload.is_empty() {
                device.payload = d.payload.clone();
            }
            device_ids.push(net.add_device(device));
        }

        // Interlocking.
        let mut interlock = Interlock::new();
        for s in &self.sections {
            let edges = s
                .edges
                .iter()
                .map(|e| {
                    edge_ids
                        .get(*e as usize)
                        .copied()
                        .ok_or(CompileError::UnknownEdge(*e))
                })
                .collect::<Result<Vec<_>, _>>()?;
            interlock.add_section(edges);
        }
        for s in &self.signals {
            let device = *device_ids
                .get(s.device as usize)
                .ok_or(CompileError::UnknownDevice(s.device))?;
            let mut signal = Signal::new(SignalId(0), s.kind, device);
            signal.system = s.system;
            signal.next = s.next.map(SignalId);
            signal.guarded = s
                .guarded
                .iter()
                .map(|g| sim_core::interlock::SectionId(*g))
                .collect();
            signal.requires_route = s.requires_route;
            signal.diverging_speed = s.diverging_speed;
            interlock.add_signal(signal);
        }
        for r in &self.routes {
            let mut route = IlRoute::new(RouteId(0), SignalId(r.entry), SignalId(r.exit));
            route.kind = r.kind;
            route.switches = r
                .switches
                .iter()
                .map(|(n, p)| {
                    node_ids
                        .get(*n as usize)
                        .copied()
                        .map(|id| (id, *p))
                        .ok_or(CompileError::UnknownNode(*n))
                })
                .collect::<Result<Vec<_>, _>>()?;
            route.sections = r
                .sections
                .iter()
                .map(|s| sim_core::interlock::SectionId(*s))
                .collect();
            route.overlap = r
                .overlap
                .iter()
                .map(|s| sim_core::interlock::SectionId(*s))
                .collect();
            route.flank = r
                .flank
                .iter()
                .map(|g| match g {
                    FlankSource::Switch(node, position) => node_ids
                        .get(*node as usize)
                        .copied()
                        .map(|id| FlankGuard::Switch(id, *position))
                        .ok_or(CompileError::UnknownNode(*node)),
                    FlankSource::Signal(signal) => Ok(FlankGuard::Signal(SignalId(*signal))),
                })
                .collect::<Result<Vec<_>, _>>()?;
            route.diverging = r.diverging;
            interlock.add_route(route);
        }

        Ok(CompiledLine {
            net,
            interlock,
            yards,
        })
    }

    /// Does the track *behind* a portal run out to the edge of the line?
    ///
    /// A train standing at a portal has its head on the mark, facing into the line, so its
    /// body lies towards the other end of the edge. That end has to be a buffer stop or a
    /// module boundary — the edge of the modelled world. A portal in the middle of a plain
    /// line would put a train on the running road out of nothing.
    fn portal_reaches_the_edge(&self, yard: &YardSource) -> bool {
        let Some(edge) = self.edges.get(yard.edge as usize) else {
            return false;
        };
        let outer = if yard.facing == Facing::Backward {
            edge.to
        } else {
            edge.from
        };
        matches!(self.nodes.get(outer as usize), Some(NodeSource::Buffer))
            || self.boundaries.iter().any(|b| b.node == outer)
    }

    /// The line's stabling roads and portals on the track graph, without compiling the
    /// rest of it.
    ///
    /// The run builds the network and the yards in one go ([`LineSource::compile`]); this
    /// is for the callers that hold the source and want only the marks — the app filling
    /// [`sim_core::Sim::yards`] after the world is built, and the editor drawing them.
    pub fn compiled_yards(&self) -> Vec<Yard> {
        self.yards
            .iter()
            .filter(|y| (y.edge as usize) < self.edges.len())
            .map(YardSource::compile)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Walkways round-trip through RON with their defaults, and the rule check calls
    /// out a path without a second point and an area without a third corner.
    #[test]
    fn walkways_round_trip_and_are_checked() {
        let text = r#"(
            name: "Weg",
            nodes: [Buffer, Buffer],
            edges: [],
            walk_paths: [(points: [(lat: 52.0, lon: 10.0), (lat: 52.0, lon: 10.001)])],
            walk_areas: [(polygon: [(lat: 52.0, lon: 10.0), (lat: 52.0, lon: 10.001), (lat: 52.001, lon: 10.001)], people: 9)],
        )"#;
        let line: LineSource = ron::from_str(text).unwrap();
        assert_eq!(line.walk_paths[0].width, 2.0);
        assert_eq!(line.walk_paths[0].people, 4);
        assert_eq!(line.walk_areas[0].people, 9);
        assert_eq!(line.walk_areas[0].walking_share, 0.5);
        let back: LineSource = ron::from_str(&line.to_ron()).unwrap();
        assert_eq!(back.walk_paths, line.walk_paths);
        assert_eq!(back.walk_areas, line.walk_areas);
        let types = std::collections::BTreeMap::new();
        let objects = std::collections::BTreeMap::new();
        assert!(line.check(&types, &objects).is_empty());

        let mut broken = line.clone();
        broken.walk_paths[0].points.pop();
        broken.walk_areas[0].polygon.pop();
        let issues = broken.check(&types, &objects);
        assert!(
            issues.contains(&RuleIssue::WalkPathTooShort { path: 0 }),
            "{issues:?}"
        );
        assert!(
            issues.contains(&RuleIssue::WalkAreaTooSmall { area: 0 }),
            "{issues:?}"
        );
    }
    use crate::musterbahn;

    /// Removing the curve must not move the climb behind it: the follower is
    /// anchored at the exact coordinates the removed edge ended at.
    #[test]
    fn removing_a_middle_edge_keeps_the_follower_in_place() {
        let mut line = musterbahn();
        let before = line.compile().unwrap();
        let expected = before.net.edges()[2].eval(0.0).pos;
        let expected_dir = before.net.edges()[2].eval(0.0).tangent;

        line.remove_edge(1);
        let after = line.compile().expect("still compiles");
        assert_eq!(line.edges.len(), 2);
        assert!(matches!(line.edges[1].start, EdgeStart::Geo { .. }));
        let start = after.net.edges()[1].eval(0.0);
        assert!(
            start.pos.distance(expected) < 0.01,
            "follower moved by {} m",
            start.pos.distance(expected)
        );
        assert!(start.tangent.dot(expected_dir) > 0.999_999);

        // The curve's devices (line conductor, block marker) went with it;
        // everything else moved down one edge index.
        assert_eq!(line.devices.len(), 7);
        assert!(line.devices.iter().all(|d| d.edge <= 1));
        assert_eq!(line.signals.len(), 2);
    }

    /// Devices carry signals, signals carry links — the whole chain follows.
    #[test]
    fn removing_the_first_edge_drops_its_signals() {
        let mut line = musterbahn();
        line.remove_edge(0);
        line.compile().expect("still compiles");
        assert_eq!(line.edges.len(), 2);
        assert!(line.signals.is_empty());
        assert_eq!(line.devices.len(), 4);
        assert!(matches!(line.edges[0].start, EdgeStart::Geo { .. }));
    }

    #[test]
    fn removing_a_device_remaps_the_signal_table() {
        let mut line = musterbahn();
        line.remove_device(0); // the distant signal's device
        line.compile().expect("still compiles");
        assert_eq!(line.signals.len(), 1);
        assert_eq!(line.signals[0].kind, SignalKind::Main);
        assert_eq!(line.signals[0].device, 1);
    }

    /// The distant signal announced the main one; when the main goes, the
    /// `next` link must not dangle.
    #[test]
    fn removing_the_main_signal_clears_the_distant_link() {
        let mut line = musterbahn();
        line.remove_device(2);
        line.compile().expect("still compiles");
        assert_eq!(line.signals.len(), 1);
        assert_eq!(line.signals[0].kind, SignalKind::Distant);
        assert_eq!(line.signals[0].next, None);
    }

    /// Dropping a signal table entry leaves the mast standing and takes every
    /// reference with it — the `next` link, the route, and the magnets that
    /// named it in their payload text.
    #[test]
    fn removing_a_signal_entry_remaps_the_magnets() {
        let mut line = musterbahn();
        line.remove_signal(0); // the distant signal
        line.compile().expect("still compiles");
        assert_eq!(line.signals.len(), 1);
        assert_eq!(line.signals[0].kind, SignalKind::Main);
        assert_eq!(line.devices.len(), 9, "the device stays");

        let signal_of = |device: usize| {
            ron::from_str::<MagnetPayload>(&line.devices[device].payload)
                .expect("magnet payload")
                .signal
        };
        assert_eq!(signal_of(1), None, "1000 Hz lost its signal");
        assert_eq!(signal_of(3), Some(0), "500 Hz follows the main signal down");
        assert_eq!(signal_of(4), Some(0));
        assert!(
            line.check(&Default::default(), &Default::default())
                .iter()
                .all(|i| !matches!(i, RuleIssue::MagnetPayloadInvalid { .. }))
        );
    }

    /// A section is addressed by index from three places at once; removing one
    /// has to move all three, and a block marker left without its section says
    /// so rather than marking the next one.
    #[test]
    fn removing_a_section_remaps_every_reference() {
        let mut line = musterbahn();
        line.remove_section(1);
        line.compile().expect("still compiles");
        assert_eq!(line.sections.len(), 2);
        assert_eq!(line.signals[1].guarded, vec![1], "was [1, 2]");
        assert_eq!(line.devices[7].payload, "", "marker was on the removed one");
        assert_eq!(
            ron::from_str::<BlockMarkerPayload>(&line.devices[8].payload)
                .unwrap()
                .section,
            1,
            "was section 2"
        );
    }

    /// A straight run: the route from the main signal to a signal three
    /// tracks along picks up the sections behind it — but not the one it
    /// stands in, which is where the train waiting for the route is.
    #[test]
    fn a_route_collects_the_sections_behind_the_entry_signal() {
        let mut line = musterbahn();
        line.devices.push(DeviceSource {
            kind: DeviceKind::Signal,
            edge: 2,
            s: 2800.0,
            facing: Facing::Forward,
            lateral_offset: 3.5,
            payload: String::new(),
        });
        line.signals.push(SignalSource {
            kind: SignalKind::Main,
            system: SignalSystem::HV,
            device: line.devices.len() as u32 - 1,
            next: None,
            guarded: vec![],
            requires_route: false,
            diverging_speed: None,
            signal_type: None,
            model: None,
        });

        // Signal 1 is the main signal at km 2.0 on edge 0, signal 2 the new one.
        let route = line.route_between(1, 2, None).expect("a path exists");
        assert_eq!(route.sections, vec![1, 2], "edge 0 stays out");
        assert!(route.switches.is_empty());
        assert!(!route.diverging);
        // 160 km/h at the exit → a 300 m overlap, and 200 m of edge 2 are
        // left behind the signal, so it stays inside the route's own section.
        assert!(route.overlap.is_empty(), "the overlap runs on in section 2");

        // Backwards there is no path: the signals only act forwards.
        assert!(line.route_between(2, 1, None).is_none());
    }

    /// The overlap follows the rulebook staircase by default and can be
    /// overridden; where it runs past the exit signal's own section, that
    /// section joins the route as an overlap.
    #[test]
    fn the_overlap_follows_the_rulebook_and_the_override() {
        assert_eq!(regular_overlap(25.0), 50.0);
        assert_eq!(regular_overlap(60.0), 100.0);
        assert_eq!(regular_overlap(100.0), 200.0);
        assert_eq!(regular_overlap(160.0), 300.0);

        let mut line = musterbahn();
        // Exit signal 100 m before the end of edge 1 (the curve, 130 km/h),
        // so the regular 300 m overlap runs on into edge 2.
        line.devices.push(DeviceSource {
            kind: DeviceKind::Signal,
            edge: 1,
            s: 900.0,
            facing: Facing::Forward,
            lateral_offset: 3.5,
            payload: String::new(),
        });
        line.signals.push(SignalSource {
            kind: SignalKind::Main,
            system: SignalSystem::HV,
            device: line.devices.len() as u32 - 1,
            next: None,
            guarded: vec![],
            requires_route: false,
            diverging_speed: None,
            signal_type: None,
            model: None,
        });

        let route = line.route_between(1, 2, None).expect("a path exists");
        assert_eq!(route.sections, vec![1], "up to the exit signal");
        assert_eq!(route.overlap, vec![2], "300 m reach into the next section");

        // A shorter overlap by hand stays inside the exit signal's own edge.
        let short = line.route_between(1, 2, Some(50.0)).expect("a path exists");
        assert!(short.overlap.is_empty());
    }

    /// Over a turnout the search reports the position the leg needs, and a
    /// route over the diverging leg marks itself as one.
    #[test]
    fn a_route_over_a_turnout_reports_its_position() {
        let mut line = musterbahn();
        let (joint, straight) = line.split_edge(0, 2500.0).expect("splits");
        let buffer = line.nodes.len() as u32;
        line.nodes.push(NodeSource::Buffer);
        let branch = line.edges.len() as u32;
        line.edges.push(EdgeSource {
            from: joint,
            to: buffer,
            start: EdgeStart::Continue { edge: 0 },
            segments: vec![Segment::straight(800.0)],
            grade: vec![],
            cant: vec![],
            speed: vec![],
            track_type: vec![],
            electrification: Vec::new(),
            formation: true,
        });
        line.nodes[joint as usize] = NodeSource::Switch {
            root: (0, true),
            straight: (straight, false),
            diverging: (branch, false),
            throw_time: 6.0,
        };
        line.sections.push(SectionSource {
            edges: vec![branch],
        });
        let branch_section = line.sections.len() as u32 - 1;
        // A signal on each leg: 2 on the branch, 3 on the straight one.
        for (edge, s) in [(branch, 600.0), (straight, 400.0)] {
            line.devices.push(DeviceSource {
                kind: DeviceKind::Signal,
                edge,
                s,
                facing: Facing::Forward,
                lateral_offset: 3.5,
                payload: String::new(),
            });
            line.signals.push(SignalSource {
                kind: SignalKind::Main,
                system: SignalSystem::HV,
                device: line.devices.len() as u32 - 1,
                next: None,
                guarded: vec![],
                requires_route: false,
                diverging_speed: None,
                signal_type: None,
                model: None,
            });
        }
        line.compile().expect("still compiles");

        // Main signal (1) over the diverging leg to the branch signal (2).
        // A diverging route without a Zs3 speed is entered at 40 km/h, so its
        // overlap is the 100 m step — and 200 m of the branch are left.
        let over_branch = line.route_between(1, 2, None).expect("a path exists");
        assert_eq!(
            over_branch.switches,
            vec![(joint, SwitchPosition::Diverging)]
        );
        assert!(over_branch.diverging);
        assert_eq!(over_branch.sections, vec![branch_section]);
        assert!(over_branch.overlap.is_empty());

        // The same entry signal to the signal on the straight leg: same
        // turnout, other position, and no longer a diverging route. The
        // second half stayed in section 0 when the edge was split.
        let through = line.route_between(1, 3, None).expect("a path exists");
        assert_eq!(through.switches, vec![(joint, SwitchPosition::Straight)]);
        assert!(!through.diverging);
        assert_eq!(through.sections, vec![0]);

        // Both run into the turnout facing, so it guards them itself: the
        // position the route needs is the one that leads a flank movement
        // away (see `flank_protection_...` for the trailing case).
        assert!(over_branch.flank.is_empty());
        assert!(through.flank.is_empty());

        // What the signal itself offers: one route per leg of the turnout,
        // each ending at the next signal on that leg. The distant signal (0)
        // is no target, and nothing runs from it either.
        let offered = line.routes_from(1, None);
        let mut exits: Vec<u32> = offered.iter().map(|r| r.exit).collect();
        exits.sort_unstable();
        assert_eq!(exits, vec![2, 3], "one route per leg");
        assert_eq!(offered.iter().filter(|r| r.diverging).count(), 1);
        assert!(line.routes_from(0, None).is_empty(), "distant signal");
    }

    /// Adds a signal on `(edge, s)` and returns its index in the table.
    fn add_signal(line: &mut LineSource, edge: u32, s: f64, facing: Facing) -> u32 {
        line.devices.push(DeviceSource {
            kind: DeviceKind::Signal,
            edge,
            s,
            facing,
            lateral_offset: 3.5,
            payload: String::new(),
        });
        line.signals.push(SignalSource {
            kind: SignalKind::Main,
            system: SignalSystem::HV,
            device: line.devices.len() as u32 - 1,
            next: None,
            guarded: vec![],
            requires_route: false,
            diverging_speed: None,
            signal_type: None,
            model: None,
        });
        line.signals.len() as u32 - 1
    }

    /// Flank protection (Ril 819): where a route trails a turnout, the leg it
    /// does not use joins the path there, and the search reports what holds a
    /// movement off it — the signal covering that leg, or, without one, the
    /// next turnout laid so it leads the movement away.
    #[test]
    fn flank_protection_comes_from_the_signal_or_the_turnout_beyond() {
        let mut line = musterbahn();
        let (joint, straight) = line.split_edge(0, 2500.0).expect("splits");
        let buffer = line.nodes.len() as u32;
        line.nodes.push(NodeSource::Buffer);
        let branch = line.edges.len() as u32;
        line.edges.push(EdgeSource {
            from: joint,
            to: buffer,
            start: EdgeStart::Continue { edge: 0 },
            segments: vec![Segment::straight(800.0)],
            grade: vec![],
            cant: vec![],
            speed: vec![],
            track_type: vec![],
            electrification: Vec::new(),
            formation: true,
        });
        line.nodes[joint as usize] = NodeSource::Switch {
            root: (0, true),
            straight: (straight, false),
            diverging: (branch, false),
            throw_time: 6.0,
        };

        // The route runs back towards the line: out of the straight leg, over
        // the turnout it trails. The branch joins it there.
        let entry = add_signal(&mut line, straight, 400.0, Facing::Backward);
        let exit = add_signal(&mut line, 0, 1500.0, Facing::Backward);
        // A signal covering the branch in the direction of the turnout.
        let guard = add_signal(&mut line, branch, 200.0, Facing::Backward);
        line.compile().expect("compiles");

        let route = line
            .route_between(entry, exit, Some(0.0))
            .expect("a path exists");
        assert_eq!(route.switches, vec![(joint, SwitchPosition::Straight)]);
        assert_eq!(
            route.flank,
            vec![FlankSource::Signal(guard)],
            "the signal on the branch holds the flank"
        );

        // Without that signal, the turnout the branch runs into takes over —
        // laid to diverging it leads a movement away from the route.
        line.remove_signal(guard as usize);
        let far_root = line.edges.len() as u32;
        for _ in 0..2 {
            let end = line.nodes.len() as u32;
            line.nodes.push(NodeSource::Buffer);
            line.edges.push(EdgeSource {
                from: buffer,
                to: end,
                start: EdgeStart::Continue { edge: branch },
                segments: vec![Segment::straight(300.0)],
                grade: vec![],
                cant: vec![],
                speed: vec![],
                track_type: vec![],
                electrification: Vec::new(),
                formation: true,
            });
        }
        line.nodes[buffer as usize] = NodeSource::Switch {
            root: (far_root, false),
            straight: (branch, true),
            diverging: (far_root + 1, false),
            throw_time: 6.0,
        };
        line.compile().expect("still compiles");

        let route = line
            .route_between(entry, exit, Some(0.0))
            .expect("a path exists");
        assert_eq!(
            route.flank,
            vec![FlankSource::Switch(buffer, SwitchPosition::Diverging)]
        );
        assert!(
            line.check(&Default::default(), &Default::default())
                .iter()
                .all(|i| !matches!(i, RuleIssue::FlankGuardInvalid { .. })),
            "a derived guard is a valid one"
        );

        // A guard that names a node which is no switch is a finding.
        line.routes.push(RouteSource {
            kind: sim_core::interlock::RouteKind::Train,
            entry,
            exit,
            switches: vec![],
            sections: vec![],
            overlap: vec![],
            flank: vec![FlankSource::Switch(1, SwitchPosition::Straight)],
            diverging: false,
        });
        assert!(
            line.check(&Default::default(), &Default::default())
                .contains(&RuleIssue::FlankGuardInvalid { route: 0 })
        );
    }

    /// A track lock is a signal with two states, so it falls out of the same
    /// tables: it holds a flank, but no route ends at it and none starts
    /// there — a train move is never authorised to a track lock.
    #[test]
    fn a_track_lock_guards_the_flank_but_ends_no_route() {
        let mut line = musterbahn();
        let (joint, straight) = line.split_edge(0, 2500.0).expect("splits");
        let buffer = line.nodes.len() as u32;
        line.nodes.push(NodeSource::Buffer);
        let branch = line.edges.len() as u32;
        line.edges.push(EdgeSource {
            from: joint,
            to: buffer,
            start: EdgeStart::Continue { edge: 0 },
            segments: vec![Segment::straight(800.0)],
            grade: vec![],
            cant: vec![],
            speed: vec![],
            track_type: vec![],
            electrification: Vec::new(),
            formation: true,
        });
        line.nodes[joint as usize] = NodeSource::Switch {
            root: (0, true),
            straight: (straight, false),
            diverging: (branch, false),
            throw_time: 6.0,
        };
        let entry = add_signal(&mut line, straight, 400.0, Facing::Backward);
        let exit = add_signal(&mut line, 0, 1500.0, Facing::Backward);
        // On the branch, close to the turnout, where a track lock belongs.
        let lock = add_signal(&mut line, branch, 50.0, Facing::Backward);
        line.signals[lock as usize].kind = SignalKind::TrackLock;
        line.compile().expect("compiles");

        let route = line
            .route_between(entry, exit, Some(0.0))
            .expect("a path exists");
        assert_eq!(
            route.flank,
            vec![FlankSource::Signal(lock)],
            "the track lock holds the flank"
        );
        // And it starts none of its own.
        assert!(line.routes_from(lock, Some(0.0)).is_empty());

        // Nor does a route end at one: a lock on the branch, in the running
        // direction of the main signal (1), is no target — as a main signal
        // in the same spot would be.
        let ahead = add_signal(&mut line, branch, 500.0, Facing::Forward);
        line.signals[ahead as usize].kind = SignalKind::TrackLock;
        assert!(
            line.routes_from(1, Some(0.0))
                .iter()
                .all(|r| r.exit != ahead),
            "no route is offered to a track lock"
        );
        line.signals[ahead as usize].kind = SignalKind::Main;
        assert!(
            line.routes_from(1, Some(0.0))
                .iter()
                .any(|r| r.exit == ahead),
            "the same signal as a main signal ends one"
        );
    }

    /// Splitting must be invisible to the geometry: the cut is continuous,
    /// the far end and the follower stay put, devices and sections follow.
    #[test]
    fn splitting_an_edge_keeps_geometry_and_devices() {
        let mut line = musterbahn();
        let before = line.compile().unwrap();
        let cut_pose = before.net.edges()[0].eval(1500.0);
        let end_before = before.net.edges()[0].end_pose().pos;

        let (node, second) = line.split_edge(0, 1500.0).expect("splits");
        assert_eq!(second, 3);
        assert!(matches!(line.nodes[node as usize], NodeSource::Joint));
        let after = line.compile().expect("still compiles");

        let cut = after.net.edges()[3].eval(0.0);
        assert!(cut.pos.distance(cut_pose.pos) < 0.01);
        assert!(cut.tangent.dot(cut_pose.tangent) > 0.999_999);
        // The cut re-levels the tangent planes, so the far end may shift by the
        // curvature-approximation error it removes — sub-metre, nothing more.
        let end_after = after.net.edges()[3].end_pose().pos;
        assert!(end_after.distance(end_before) < 0.5);

        // Devices beyond the cut moved onto the second half, shifted by the cut.
        assert_eq!(line.devices[2].edge, 3, "main signal at km 2.0");
        assert!((line.devices[2].s - 500.0).abs() < 1e-9);
        assert_eq!(line.devices[0].edge, 0, "distant signal at km 1.0 stays");
        // Both halves stay in section 0.
        assert!(line.sections[0].edges.contains(&0) && line.sections[0].edges.contains(&3));
        // The curve continued from edge 0's old end — re-anchored onto the
        // second half's end, so the line stays gapless.
        assert!(matches!(line.edges[1].start, EdgeStart::Geo { .. }));
        assert!(after.net.edges()[1].eval(0.0).pos.distance(end_after) < 0.01);
    }

    /// A cut inside a transition curve keeps the curvature continuous, and the
    /// cant/grade profiles carry the value in force at the cut across it.
    #[test]
    fn splitting_inside_a_clothoid_keeps_curvature_and_profiles() {
        let mut line = musterbahn();
        let before = line.compile().unwrap();
        let cut_pose = before.net.edges()[1].eval(100.0); // mid-transition
        line.split_edge(1, 100.0).expect("splits");
        let after = line.compile().expect("still compiles");
        let cut = after.net.edges()[3].eval(0.0);
        assert!((cut.curvature - cut_pose.curvature).abs() < 1e-12);
        assert!((cut.cant - cut_pose.cant).abs() < 1e-9);
        // The cant ramp's later steps follow, shifted by the cut.
        assert_eq!(line.edges[3].cant[0].0, 0.0);
        assert_eq!(line.edges[3].cant[1], (100.0, 80.0));
        assert_eq!(line.edges[3].cant[2], (700.0, 0.0));
    }

    /// A switch leg that hung on the old edge end follows the second half.
    #[test]
    fn splitting_the_root_edge_rewires_the_switch() {
        let start = GeoPoint {
            lat: 52.0,
            lon: 10.0,
            height: 100.0,
        };
        let mut line = LineSource {
            name: "turnout".into(),
            geoid_offset: 46.0,
            electrification: track_model::PowerSystem::Ac15kv.id().to_string(),
            nodes: vec![
                NodeSource::Buffer,
                NodeSource::Switch {
                    root: (0, true),
                    straight: (1, false),
                    diverging: (2, false),
                    throw_time: 6.0,
                },
                NodeSource::Buffer,
                NodeSource::Buffer,
            ],
            edges: vec![
                EdgeSource {
                    from: 0,
                    to: 1,
                    start: EdgeStart::Geo {
                        point: start,
                        heading_deg: 90.0,
                    },
                    segments: vec![Segment::straight(1000.0)],
                    grade: vec![],
                    cant: vec![],
                    speed: vec![],
                    track_type: vec![],
                    electrification: Vec::new(),
                    formation: true,
                },
                EdgeSource {
                    from: 1,
                    to: 2,
                    start: EdgeStart::Continue { edge: 0 },
                    segments: vec![Segment::straight(500.0)],
                    grade: vec![],
                    cant: vec![],
                    speed: vec![],
                    track_type: vec![],
                    electrification: Vec::new(),
                    formation: true,
                },
                EdgeSource {
                    from: 1,
                    to: 3,
                    start: EdgeStart::Continue { edge: 0 },
                    segments: vec![Segment::arc(300.0, 190.0)],
                    grade: vec![],
                    cant: vec![],
                    speed: vec![],
                    track_type: vec![],
                    electrification: Vec::new(),
                    formation: true,
                },
            ],
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
        line.compile().expect("compiles before the split");

        let (_, second) = line.split_edge(0, 400.0).expect("splits");
        assert!(matches!(
            line.nodes[1],
            NodeSource::Switch { root, .. } if root == (second, true)
        ));
        line.compile().expect("still compiles");
    }

    #[test]
    fn split_refuses_the_edge_ends() {
        let mut line = musterbahn();
        assert!(line.split_edge(0, 0.5).is_none());
        assert!(line.split_edge(0, 2999.5).is_none());
        assert!(line.split_edge(7, 10.0).is_none());
    }

    /// Stabling roads and portals follow their edge through splits and removals the way
    /// devices and objects do, and compile onto the graph facing the way the file says.
    #[test]
    fn yards_follow_split_and_removal() {
        let mut line = musterbahn();
        line.yards.push(YardSource {
            name: "Ausweichgleis".into(),
            kind: YardKind::Stabling,
            edge: 1,
            s: 100.0,
            facing: Facing::Backward,
            length: 200.0,
        });
        let compiled = line.compile().expect("compiles");
        assert_eq!(compiled.yards.len(), 3);
        assert_eq!(
            compiled.yards[0].at,
            TrackPosition::new(EdgeId(0), 300.0, 1)
        );
        assert_eq!(compiled.yards[1].at.dir, -1, "facing back into the line");

        // The west portal sits before the cut and stays; the east one is on another edge.
        line.split_edge(0, 200.0).expect("splits");
        assert_eq!(line.yards[0].edge, 3, "beyond the cut, on the new half");
        assert!((line.yards[0].s - 100.0).abs() < 1e-9);
        assert_eq!(line.yards[1].edge, 2);

        // Removing the curve takes the road on it along and remaps the rest.
        line.remove_edge(1);
        assert_eq!(line.yards.len(), 2);
        assert!(line.yards.iter().all(|y| y.name.starts_with("Portal")));
        line.compile().expect("still compiles");
    }

    /// The check knows a road off its track, a portal that is not at the edge of the
    /// line, and two roads of the same name.
    #[test]
    fn check_flags_a_portal_in_the_middle_of_the_line() {
        let types = std::collections::BTreeMap::new();
        let objects = std::collections::BTreeMap::new();
        let mut line = musterbahn();
        assert!(line.check(&types, &objects).is_empty());

        // A portal on the middle edge has a joint behind it, not the edge of the world.
        line.yards.push(YardSource {
            name: "Portal Mitte".into(),
            kind: YardKind::Portal,
            edge: 1,
            s: 100.0,
            facing: Facing::Forward,
            length: 0.0,
        });
        // A second "Portal West" would shadow the first one for every shunt job.
        line.yards.push(YardSource {
            name: "Portal West".into(),
            kind: YardKind::Stabling,
            edge: 0,
            s: 9_999.0,
            facing: Facing::Forward,
            length: 0.0,
        });
        let issues = line.check(&types, &objects);
        assert!(issues.contains(&RuleIssue::PortalNotAtTheEdge { yard: 2 }));
        assert!(issues.contains(&RuleIssue::YardOffEdge { yard: 3 }));
        // A road that is off its track is reported once, not twice.
        assert!(!issues.contains(&RuleIssue::DuplicateYardName { yard: 3 }));

        // A portal on a module boundary is at the edge of the line just as a buffer is.
        line.yards.truncate(2);
        line.yards.push(YardSource {
            name: "Portal Übergang".into(),
            kind: YardKind::Portal,
            edge: 2,
            s: 100.0,
            facing: Facing::Forward,
            length: 0.0,
        });
        assert!(
            line.check(&types, &objects)
                .contains(&RuleIssue::PortalNotAtTheEdge { yard: 2 })
        );
        line.boundaries.push(BoundarySource {
            name: "nach_osten".into(),
            node: 2,
        });
        assert!(
            !line
                .check(&types, &objects)
                .iter()
                .any(|i| matches!(i, RuleIssue::PortalNotAtTheEdge { .. }))
        );
    }

    /// The example line is wired correctly; removing its 1000 Hz magnet is the
    /// textbook finding the check exists for.
    #[test]
    fn check_flags_the_missing_1000hz_magnet() {
        let types = std::collections::BTreeMap::new();
        let objects = std::collections::BTreeMap::new();
        let mut line = musterbahn();
        assert!(
            line.check(&types, &objects).is_empty(),
            "{:?}",
            line.check(&types, &objects)
        );
        line.remove_device(1); // the 1000 Hz magnet at the distant signal
        assert_eq!(
            line.check(&types, &objects),
            vec![RuleIssue::DistantWithout1000Hz { signal: 0 }]
        );
    }

    #[test]
    fn check_flags_bad_references() {
        let mut line = musterbahn();
        line.devices[6].s = 9000.0; // platform beyond its edge
        line.devices[1].payload = "(frequency:Hz1000,signal:Some(7))".into();
        line.boundaries.push(BoundarySource {
            name: "mitte".into(),
            node: 1, // a joint, not a buffer
        });
        let issues = line.check(
            &std::collections::BTreeMap::new(),
            &std::collections::BTreeMap::new(),
        );
        assert!(issues.contains(&RuleIssue::DeviceOffEdge { device: 6 }));
        assert!(issues.contains(&RuleIssue::MagnetPayloadInvalid { device: 1 }));
        assert!(issues.contains(&RuleIssue::BoundaryInvalid { boundary: 0 }));
        // The broken magnet no longer counts as the distant signal's 1000 Hz.
        assert!(issues.contains(&RuleIssue::DistantWithout1000Hz { signal: 0 }));
    }

    /// Track types compile into an interned table plus per-edge index
    /// profiles; the specs come from the registry later.
    #[test]
    fn track_types_intern_and_split() {
        let mut line = musterbahn();
        line.edges[0].track_type = vec![(0.0, "ex:hauptbahn".into()), (2500.0, "ex:alt".into())];
        line.edges[2].track_type = vec![(0.0, "ex:hauptbahn".into())];
        let compiled = line.compile().expect("compiles");
        let names: Vec<&str> = compiled
            .net
            .types()
            .iter()
            .map(|t| t.name.as_str())
            .collect();
        assert_eq!(names, ["default", "ex:hauptbahn", "ex:alt"]);
        assert_eq!(compiled.net.edges()[0].track_type.at(0.0), 1);
        assert_eq!(compiled.net.edges()[0].track_type.at(2600.0), 2);
        assert_eq!(compiled.net.edges()[1].track_type.at(0.0), 0);
        assert_eq!(compiled.net.edges()[2].track_type.at(0.0), 1);

        // Splitting carries the type sections across, shifted by the cut.
        line.split_edge(0, 1500.0).expect("splits");
        assert_eq!(line.edges[0].track_type, vec![(0.0, "ex:hauptbahn".into())]);
        assert_eq!(
            line.edges[3].track_type,
            vec![
                (0.0, "ex:hauptbahn".to_string()),
                (1000.0, "ex:alt".to_string())
            ]
        );
    }

    /// Scenery objects follow their edge through splits and removals like
    /// devices do — and the check knows a dangling or unknown one.
    #[test]
    fn objects_follow_split_and_removal() {
        let mast = |edge: u32, s: f64| ObjectSource {
            object: "ex:mast".into(),
            edge,
            s,
            lateral_offset: -3.5,
            yaw_deg: 0.0,
            height: 0.0,
            snap_to_terrain: false,
        };
        let mut line = musterbahn();
        line.objects = vec![mast(0, 500.0), mast(0, 2500.0), mast(1, 100.0)];
        line.compile().expect("compiles");

        line.split_edge(0, 1500.0).expect("splits");
        assert_eq!(line.objects[0].edge, 0, "before the cut");
        assert_eq!(line.objects[1].edge, 3, "beyond the cut");
        assert!((line.objects[1].s - 1000.0).abs() < 1e-9);

        // Removing the curve takes its object along; the rest is remapped.
        line.remove_edge(1);
        assert_eq!(line.objects.len(), 2);
        assert_eq!(line.objects[1].edge, 2);
        line.compile().expect("still compiles");

        let types = std::collections::BTreeMap::new();
        let mut objects = std::collections::BTreeMap::new();
        objects.insert(
            "ex:mast".to_string(),
            TrackObject {
                name: "Mast".into(),
                model: "x/assets/mast.gltf".into(),
                lateral_offset: -3.5,
                yaw_deg: 0.0,
                height: 0.0,
                autumn_model: None,
                winter_model: None,
                lod_distances: Vec::new(),
                tags: Vec::new(),
            },
        );
        assert!(line.check(&types, &objects).is_empty());
        line.objects[0].s = 99_999.0;
        line.objects[1].object = "ex:fehlt".into();
        let issues = line.check(&types, &objects);
        assert!(issues.contains(&RuleIssue::ObjectOffEdge { object: 0 }));
        assert!(issues.contains(&RuleIssue::UnknownObject { object: 1 }));
    }

    /// The registry-aware rules: unknown names and an LZB superstructure
    /// whose conductor was never placed.
    #[test]
    fn check_flags_track_type_wiring() {
        let mut types = std::collections::BTreeMap::new();
        types.insert(
            "ex:lzb".to_string(),
            TrackType {
                lzb: true,
                ..TrackType::default()
            },
        );
        let objects = std::collections::BTreeMap::new();
        let mut line = musterbahn();
        // The Musterbahn has a line conductor — an LZB type raises nothing.
        line.edges[2].track_type = vec![(0.0, "ex:lzb".into())];
        assert!(line.check(&types, &objects).is_empty());

        line.edges[0].track_type = vec![(0.0, "ex:tippfehler".into())];
        let issues = line.check(&types, &objects);
        assert_eq!(issues, vec![RuleIssue::UnknownTrackType { edge: 0 }]);

        // Without the conductor the LZB type is a promise nothing keeps.
        line.devices.retain(|d| d.kind != DeviceKind::LineConductor);
        let issues = line.check(&types, &objects);
        assert!(issues.contains(&RuleIssue::LzbTypeWithoutConductor { edge: 2 }));
    }

    #[test]
    fn envelope_bounds_the_module() {
        let anchor = GeoPoint {
            lat: 52.0,
            lon: 10.0,
            height: 100.0,
        };
        let mut line = LineSource {
            anchor: Some(anchor),
            envelope: default_envelope(anchor, DEFAULT_ENVELOPE_HALF_SIZE),
            ..Default::default()
        };
        assert_eq!(line.envelope.len(), 4);
        assert!(line.envelope_contains(anchor.lat, anchor.lon));
        // A kilometre out is still inside a 2 km half-size square, ten are not.
        assert!(line.envelope_contains(anchor.lat + 0.009, anchor.lon));
        assert!(!line.envelope_contains(anchor.lat + 0.09, anchor.lon));
        assert!(!line.envelope_contains(anchor.lat, anchor.lon + 0.09));

        // No envelope bounds nothing — lines from before envelopes stay editable.
        line.envelope.clear();
        assert!(line.envelope_contains(0.0, 0.0));
    }

    #[test]
    fn landscape_pulled_outside_the_envelope_is_reported() {
        let anchor = GeoPoint {
            lat: 52.0,
            lon: 10.0,
            height: 100.0,
        };
        let mut line = LineSource {
            anchor: Some(anchor),
            envelope: default_envelope(anchor, DEFAULT_ENVELOPE_HALF_SIZE),
            trees: vec![
                TreeSource {
                    object: String::new(),
                    lat: 52.0,
                    lon: 10.0,
                    yaw_deg: 0.0,
                    scale: 1.0,
                },
                TreeSource {
                    object: String::new(),
                    lat: 52.5,
                    lon: 10.0,
                    yaw_deg: 0.0,
                    scale: 1.0,
                },
            ],
            // A footpath with two of its three vertices past the boundary, and
            // an area wholly inside: the count is of vertices, not of ways.
            walk_paths: vec![WalkPathSource {
                name: String::new(),
                points: vec![
                    WalkPoint {
                        lat: 52.0,
                        lon: 10.0,
                    },
                    WalkPoint {
                        lat: 52.5,
                        lon: 10.0,
                    },
                    WalkPoint {
                        lat: 52.5,
                        lon: 10.1,
                    },
                ],
                width: 2.0,
                people: 4,
                height: 0.0,
                tags: Vec::new(),
            }],
            walk_areas: vec![WalkAreaSource {
                name: String::new(),
                polygon: vec![
                    WalkPoint {
                        lat: 52.0,
                        lon: 10.0,
                    },
                    WalkPoint {
                        lat: 52.0,
                        lon: 10.001,
                    },
                    WalkPoint {
                        lat: 52.001,
                        lon: 10.001,
                    },
                ],
                people: 6,
                walking_share: 0.5,
                height: 0.0,
                tags: Vec::new(),
            }],
            ..Default::default()
        };
        let types = std::collections::BTreeMap::new();
        let objects = std::collections::BTreeMap::new();
        assert!(
            line.check(&types, &objects)
                .contains(&RuleIssue::OutsideEnvelope {
                    trees: 1,
                    terrain: 0,
                    markers: 0,
                    walkways: 2,
                    fields: 0,
                })
        );
        // Without an envelope there is nothing to be outside of.
        line.envelope.clear();
        assert!(
            !line
                .check(&types, &objects)
                .iter()
                .any(|i| matches!(i, RuleIssue::OutsideEnvelope { .. }))
        );
    }

    #[test]
    fn the_boundary_itself_is_inside_for_the_track() {
        let anchor = GeoPoint {
            lat: 52.0,
            lon: 10.0,
            height: 0.0,
        };
        let line = LineSource {
            envelope: default_envelope(anchor, DEFAULT_ENVELOPE_HALF_SIZE),
            ..Default::default()
        };
        // The eastern side, and a few metres past it.
        let east = line.envelope[1].lon;
        assert!(!line.envelope_contains(anchor.lat, east + 0.00005));
        assert!(line.envelope_contains_within(anchor.lat, east + 0.00005, 10.0));
        // Fifty metres out is past any joining tolerance.
        assert!(!line.envelope_contains_within(anchor.lat, east + 0.0007, 10.0));
    }

    #[test]
    fn a_crossed_envelope_is_found() {
        let square = default_envelope(
            GeoPoint {
                lat: 52.0,
                lon: 10.0,
                height: 0.0,
            },
            DEFAULT_ENVELOPE_HALF_SIZE,
        );
        assert!(!envelope_self_intersects(&square));
        // Swapping two corners folds the square into a bow tie.
        let mut bow_tie = square.clone();
        bow_tie.swap(2, 3);
        assert!(envelope_self_intersects(&bow_tie));
        // A triangle cannot cross itself, and neither can two points.
        assert!(!envelope_self_intersects(&square[..3]));
        assert!(!envelope_self_intersects(&square[..2]));

        let mut line = LineSource {
            envelope: bow_tie,
            ..Default::default()
        };
        let types = std::collections::BTreeMap::new();
        let objects = std::collections::BTreeMap::new();
        assert!(
            line.check(&types, &objects)
                .contains(&RuleIssue::EnvelopeSelfIntersects)
        );
        line.envelope = square;
        assert!(
            !line
                .check(&types, &objects)
                .contains(&RuleIssue::EnvelopeSelfIntersects)
        );
    }
}
