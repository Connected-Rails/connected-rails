//! Overhead lines: the masts on the ground and the conductors between them.
//!
//! A [`crate::route::PowerLineSource`] is what OSM maps a power line as — the
//! masts in order — plus the numbers that turn the chain of points into
//! something to look at: which mast object stands at each point, how high its
//! crossarms are and where the conductors sit on them.
//!
//! Two things come out of it, and they take different routes through the
//! pipeline because they are different kinds of thing:
//!
//! - **The masts** are geo-positioned instances of a mod object, which is
//!   exactly what a tree is, so they travel as [`crate::route::TreeSource`] and
//!   the tile pipeline streams and batches them without knowing they are masts
//!   ([`crate::terrain::Vegetation`]). Three hundred Donaumasten along a line
//!   cost what three hundred spruces cost.
//! - **The conductors** are geometry, cut to the terrain tiles the way the
//!   roads are, and hung as catenaries between the mast tops.
//!
//! The mast tops are fixed **once**, in [`PowerLines::prepare`], against the
//! same elevation data the terrain is built from — a span crosses tile
//! boundaries and the two masts it hangs between are rarely on one tile, so a
//! per-tile height grid cannot answer where a conductor starts. That is the
//! same trade [`crate::water`] makes for its shorelines, and it has the same
//! small cost: the foot height is the raw ground, not the ground after the
//! track's cutting blend, so a mast standing inside the corridor may sit a few
//! centimetres off. Power lines cross a railway; they do not run in it.
//!
//! Nothing here is simulated. A power line is scenery: no state, nothing to
//! replicate, and both clients of a multiplayer run build the same conductors
//! out of the same line file without a byte crossing the network.

use std::sync::Arc;

use glam::{DVec2, DVec3};
use world_coords::{EnuFrame, geo};

use crate::import::dgm::TerrainSource;
use crate::route::{LineSource, PowerArm, PowerLineSource, TreeSource};
use crate::terrain::{CellMap, Sampler, TileKey, to_render, to_render_dir};

/// How wide a conductor is [m] — its **true** width, not a drawing width.
///
/// A 110 kV single conductor is about 2 cm and a 380 kV quad bundle spreads
/// four of them over 40 cm, so 11 cm is the honest middle of what a German line
/// carries. It is honest because nothing has to lie here any more: the width
/// travels to the GPU as a number and `world_render::conductors` draws the wire
/// at whatever it takes to stay a pixel and a half wide, fading its coverage as
/// it goes. A conductor drawn at a fixed 11 cm is under a pixel past 190 m and
/// crawls as a dotted line; a conductor drawn fat enough to survive that is a
/// rope up close.
const CONDUCTOR_WIDTH: f64 = 0.11;

/// How finely a span is broken into straight pieces [m].
///
/// The sag over a 400 m span is twelve metres, so the curve has to be sampled
/// or it is a straight line with a kink in the middle. Twenty metres puts about
/// twenty pieces in a span and keeps the chord error under a centimetre.
const CONDUCTOR_STEP: f64 = 20.0;

/// What a mast type is, as far as a line file needs to know.
///
/// The numbers are the atlas's (`tools/pylons/pylons.json`) — this table is
/// what the OSM import stamps into a [`PowerLineSource`], the same way
/// [`crate::roads::PRESETS`] stamps a carriageway width. A test below reads the
/// atlas and fails if the two have drifted apart.
#[derive(Debug, Clone, Copy)]
pub struct PowerPreset {
    /// The atlas id (`donaumast-380`).
    pub id: &'static str,
    /// The suspension mast object (`"<mod>:<name>"`).
    pub object: &'static str,
    /// The tension mast object; empty where the type is not built as one.
    pub tension_object: &'static str,
    /// Mast height over ground [m] — the mean of the atlas's band.
    pub height: f64,
    /// Half the body width where the crossarms leave it [m].
    pub root: f64,
    /// Nominal span to the next mast [m] — the mean of the atlas's band.
    pub span: f64,
    /// Crossarms, top first.
    pub arms: &'static [PowerArm],
}

/// Every type of `tools/pylons/pylons.json`, in the atlas's order.
pub const PRESETS: &[PowerPreset] = &[
    PowerPreset {
        id: "einebenenmast-380",
        object: "pylons:einebenenmast_380_trag",
        tension_object: "pylons:einebenenmast_380_abspann",
        height: 42.0,
        root: 1.60,
        span: 400.0,
        arms: &[PowerArm {
            at: 36.54,
            half_width: 20.00,
            conductors: 6,
        }],
    },
    PowerPreset {
        id: "donaumast-380",
        object: "pylons:donaumast_380_trag",
        tension_object: "pylons:donaumast_380_abspann",
        height: 60.0,
        root: 1.70,
        span: 400.0,
        arms: &[
            PowerArm {
                at: 54.60,
                half_width: 7.00,
                conductors: 2,
            },
            PowerArm {
                at: 46.20,
                half_width: 12.00,
                conductors: 4,
            },
        ],
    },
    PowerPreset {
        id: "tonnenmast-380",
        object: "pylons:tonnenmast_380_trag",
        tension_object: "pylons:tonnenmast_380_abspann",
        height: 71.0,
        root: 1.60,
        span: 400.0,
        arms: &[
            PowerArm {
                at: 66.03,
                half_width: 8.00,
                conductors: 2,
            },
            PowerArm {
                at: 58.93,
                half_width: 11.00,
                conductors: 2,
            },
            PowerArm {
                at: 50.41,
                half_width: 9.00,
                conductors: 2,
            },
        ],
    },
    PowerPreset {
        id: "donaumast-220",
        object: "pylons:donaumast_220_trag",
        tension_object: "pylons:donaumast_220_abspann",
        height: 45.0,
        root: 1.30,
        span: 375.0,
        arms: &[
            PowerArm {
                at: 40.50,
                half_width: 5.00,
                conductors: 2,
            },
            PowerArm {
                at: 34.20,
                half_width: 8.50,
                conductors: 4,
            },
        ],
    },
    PowerPreset {
        id: "tannenbaummast-220",
        object: "pylons:tannenbaummast_220_trag",
        tension_object: "pylons:tannenbaummast_220_abspann",
        height: 47.5,
        root: 1.30,
        span: 325.0,
        arms: &[
            PowerArm {
                at: 44.18,
                half_width: 5.50,
                conductors: 2,
            },
            PowerArm {
                at: 38.48,
                half_width: 7.00,
                conductors: 2,
            },
            PowerArm {
                at: 31.83,
                half_width: 8.50,
                conductors: 2,
            },
        ],
    },
    PowerPreset {
        id: "donaumast-110",
        object: "pylons:donaumast_110_trag",
        tension_object: "pylons:donaumast_110_abspann",
        height: 30.0,
        root: 1.00,
        span: 275.0,
        arms: &[
            PowerArm {
                at: 26.70,
                half_width: 3.00,
                conductors: 2,
            },
            PowerArm {
                at: 22.20,
                half_width: 5.50,
                conductors: 4,
            },
        ],
    },
    PowerPreset {
        id: "einebenenmast-110",
        object: "pylons:einebenenmast_110_trag",
        tension_object: "pylons:einebenenmast_110_abspann",
        height: 26.0,
        root: 1.10,
        span: 275.0,
        arms: &[PowerArm {
            at: 22.36,
            half_width: 9.00,
            conductors: 6,
        }],
    },
    PowerPreset {
        id: "kombimast-380-110",
        object: "pylons:kombimast_380_110_trag",
        tension_object: "pylons:kombimast_380_110_abspann",
        height: 72.5,
        root: 1.80,
        span: 375.0,
        arms: &[
            PowerArm {
                at: 68.15,
                half_width: 7.00,
                conductors: 2,
            },
            PowerArm {
                at: 60.17,
                half_width: 12.00,
                conductors: 4,
            },
            PowerArm {
                at: 48.58,
                half_width: 8.00,
                conductors: 6,
            },
        ],
    },
    PowerPreset {
        id: "portalmast-380",
        object: "pylons:portalmast_380_abspann",
        tension_object: "pylons:portalmast_380_abspann",
        height: 32.5,
        root: 1.10,
        span: 200.0,
        arms: &[PowerArm {
            at: 28.60,
            half_width: 15.00,
            conductors: 6,
        }],
    },
    PowerPreset {
        id: "kompaktmast-380",
        object: "pylons:kompaktmast_380_trag",
        tension_object: "pylons:kompaktmast_380_abspann",
        height: 52.5,
        root: 0.55,
        span: 325.0,
        arms: &[
            PowerArm {
                at: 48.83,
                half_width: 4.00,
                conductors: 2,
            },
            PowerArm {
                at: 43.05,
                half_width: 6.00,
                conductors: 2,
            },
            PowerArm {
                at: 37.27,
                half_width: 6.00,
                conductors: 2,
            },
        ],
    },
    PowerPreset {
        id: "bahnstrommast-110",
        object: "pylons:bahnstrommast_110_trag",
        tension_object: "pylons:bahnstrommast_110_abspann",
        height: 27.5,
        root: 0.90,
        span: 300.0,
        arms: &[PowerArm {
            at: 23.93,
            half_width: 4.50,
            conductors: 4,
        }],
    },
    PowerPreset {
        id: "bahnstrommast-110-zweiebenen",
        object: "pylons:bahnstrommast_110_zweiebenen_trag",
        tension_object: "pylons:bahnstrommast_110_zweiebenen_abspann",
        height: 32.0,
        root: 0.95,
        span: 300.0,
        arms: &[
            PowerArm {
                at: 28.48,
                half_width: 4.50,
                conductors: 4,
            },
            PowerArm {
                at: 24.00,
                half_width: 4.50,
                conductors: 4,
            },
        ],
    },
    PowerPreset {
        id: "betonmast-20kv-einebene",
        object: "pylons:betonmast_20kv_einebene_trag",
        tension_object: "pylons:betonmast_20kv_einebene_abspann",
        height: 12.0,
        root: 0.10,
        span: 95.0,
        arms: &[PowerArm {
            at: 11.16,
            half_width: 1.20,
            conductors: 3,
        }],
    },
    PowerPreset {
        id: "betonmast-20kv-dreieck",
        object: "pylons:betonmast_20kv_dreieck_trag",
        tension_object: "",
        height: 12.0,
        root: 0.10,
        span: 95.0,
        arms: &[
            PowerArm {
                at: 12.00,
                half_width: 0.00,
                conductors: 1,
            },
            PowerArm {
                at: 10.56,
                half_width: 0.90,
                conductors: 2,
            },
        ],
    },
    PowerPreset {
        id: "stahlgittermast-20kv",
        object: "pylons:stahlgittermast_20kv_trag",
        tension_object: "pylons:stahlgittermast_20kv_abspann",
        height: 15.0,
        root: 0.50,
        span: 115.0,
        arms: &[
            PowerArm {
                at: 13.80,
                half_width: 1.30,
                conductors: 3,
            },
            PowerArm {
                at: 12.00,
                half_width: 1.30,
                conductors: 3,
            },
        ],
    },
    PowerPreset {
        id: "masttrafo-20kv",
        object: "pylons:masttrafo_20kv",
        tension_object: "",
        height: 10.5,
        root: 0.11,
        span: 95.0,
        arms: &[PowerArm {
            at: 9.77,
            half_width: 1.20,
            conductors: 3,
        }],
    },
    PowerPreset {
        id: "holzmast-nsp",
        object: "pylons:holzmast_nsp_trag",
        tension_object: "pylons:holzmast_nsp_abspann",
        height: 9.0,
        root: 0.09,
        span: 40.0,
        arms: &[PowerArm {
            at: 8.10,
            half_width: 0.80,
            conductors: 4,
        }],
    },
    PowerPreset {
        id: "fernmeldemast-bahn",
        object: "pylons:fernmeldemast_bahn_trag",
        tension_object: "pylons:fernmeldemast_bahn_abspann",
        height: 7.5,
        root: 0.08,
        span: 60.0,
        arms: &[
            PowerArm {
                at: 7.12,
                half_width: 0.70,
                conductors: 0,
            },
            PowerArm {
                at: 6.30,
                half_width: 0.70,
                conductors: 0,
            },
        ],
    },
];

/// The preset of an atlas id.
pub fn preset(id: &str) -> Option<&'static PowerPreset> {
    PRESETS.iter().find(|p| p.id == id)
}

/// A line source stamped from a preset — what the OSM import and the editor's
/// power line tool both produce.
pub fn source_from(
    preset: &PowerPreset,
    name: String,
    points: Vec<crate::route::PowerPoint>,
    tags: Vec<String>,
) -> PowerLineSource {
    PowerLineSource {
        name,
        points,
        object: preset.object.to_string(),
        tension_object: preset.tension_object.to_string(),
        height: preset.height,
        root: preset.root,
        arms: preset.arms.to_vec(),
        sag: 0.03,
        tags,
    }
}

/// One mast, prepared: where it stands in the UTM grid and how high its foot
/// is. `foot` is filled by [`PowerLines::prepare`].
#[derive(Debug, Clone, Copy)]
struct Mast {
    pos: DVec2,
    foot: f64,
}

/// A line, prepared for tile builds.
#[derive(Debug, Clone)]
struct Prepared {
    masts: Vec<Mast>,
    arms: Vec<PowerArm>,
    root: f64,
    sag: f64,
}

/// One straight piece of one conductor: `x`/`y` in the UTM grid, `z` the
/// ellipsoidal height.
#[derive(Debug, Clone, Copy)]
struct Piece {
    a: DVec3,
    b: DVec3,
}

/// The line's overhead lines, prepared for tile builds.
#[derive(Debug, Clone, Default)]
pub struct PowerLines {
    lines: Vec<Prepared>,
    /// Conductor pieces by tile, keyed on the tile the piece's middle falls in.
    /// A piece is twenty metres long, so cutting on the middle is exact to
    /// within half of that and two neighbouring tiles never draw the same wire
    /// twice — which duplicating whole spans across tiles would.
    by_tile: CellMap<Vec<Piece>>,
    prepared: bool,
    tile_size: f64,
}

impl PowerLines {
    pub fn from_line(line: &LineSource, zone: u8, tile_size: f64) -> Self {
        Self::from_parts(&line.power_lines, zone, tile_size)
    }

    pub fn from_parts(sources: &[PowerLineSource], zone: u8, tile_size: f64) -> Self {
        let lines = sources
            .iter()
            .filter(|l| l.points.len() >= 2)
            .map(|l| Prepared {
                masts: l
                    .points
                    .iter()
                    .map(|p| {
                        let (e, n) = geo::to_utm(p.lat.to_radians(), p.lon.to_radians(), zone);
                        Mast {
                            pos: DVec2::new(e, n),
                            foot: 0.0,
                        }
                    })
                    .collect(),
                arms: l.arms.clone(),
                root: l.root,
                sag: l.sag,
            })
            .collect();
        Self {
            lines,
            by_tile: CellMap::default(),
            prepared: false,
            tile_size,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Whether this tile carries any conductor at all.
    pub fn touches(&self, k: TileKey) -> bool {
        self.by_tile.contains_key(&k)
    }

    /// Fixes every mast's foot on the elevation data and strings the
    /// conductors. Called once, by the terrain builder; an already prepared set
    /// is left alone so the same lines can be handed from one builder
    /// generation to the next.
    pub(crate) fn prepare(
        &mut self,
        sources: &[Arc<TerrainSource>],
        zone: u8,
        geoid_offset: f64,
        fallback_height: f64,
    ) {
        if self.prepared {
            return;
        }
        self.prepared = true;
        let fallback = fallback_height + geoid_offset;
        let mut sampler = Sampler::new(sources.iter().map(Arc::as_ref), zone);
        for line in &mut self.lines {
            for mast in &mut line.masts {
                let (lat, lon) = geo::from_utm(mast.pos.x, mast.pos.y, zone);
                mast.foot = sampler.height(mast.pos, lat, lon).unwrap_or(fallback);
            }
        }
        self.string();
    }

    /// Hangs every conductor of every span and buckets the pieces by tile.
    fn string(&mut self) {
        let mut pieces = Vec::new();
        for line in &self.lines {
            for arm in &line.arms {
                for offset in arm.offsets(line.root) {
                    for pair in line.masts.windows(2) {
                        let (m0, m1) = (pair[0], pair[1]);
                        let along = m1.pos - m0.pos;
                        let span = along.length();
                        if span < 1.0 {
                            continue;
                        }
                        // Across the line, so the conductor leaves the mast at
                        // the insulator it hangs on rather than at its centre.
                        let across = DVec2::new(-along.y, along.x) / span;
                        let a = m0.pos + across * offset;
                        let b = m1.pos + across * offset;
                        let ha = m0.foot + arm.at;
                        let hb = m1.foot + arm.at;

                        let steps = ((span / CONDUCTOR_STEP).ceil() as usize).max(2);
                        let mut previous = DVec3::new(a.x, a.y, ha);
                        for i in 1..=steps {
                            let t = i as f64 / steps as f64;
                            let p = a.lerp(b, t);
                            // A parabola is the catenary to within a centimetre
                            // over the spans a line uses, and the shape is what
                            // matters: a conductor drawn straight is the one
                            // thing that says "not a power line".
                            let dip = 4.0 * line.sag * span * t * (1.0 - t);
                            let next = DVec3::new(p.x, p.y, ha + (hb - ha) * t - dip);
                            pieces.push(Piece {
                                a: previous,
                                b: next,
                            });
                            previous = next;
                        }
                    }
                }
            }
        }
        for piece in pieces {
            self.push(piece);
        }
    }

    fn push(&mut self, piece: Piece) {
        let mid = (piece.a + piece.b) * 0.5;
        let k = (
            (mid.x / self.tile_size).floor() as i64,
            (mid.y / self.tile_size).floor() as i64,
        );
        self.by_tile.entry(k).or_default().push(piece);
    }
}

/// The conductors of one tile, in the tile's own frame — every wire of the tile
/// in one patch, so a tile of overhead line costs one draw call.
///
/// What is stored is the wire's **centre line**, not a ribbon around it: the
/// vertex shader spreads each piece into a band facing the camera and as wide
/// as it has to be to stay visible (`world_render::conductors`). That is two
/// triangles per piece where a cross of two fixed quads was four, it never
/// vanishes edge-on the way a fixed quad does, and the width is free to depend
/// on the distance — which is the whole point.
#[derive(Debug, Clone, PartialEq)]
pub struct ConductorPatch {
    /// The centre line, in render axes (x = east, y = up, z = −north) relative
    /// to the tile anchor. Both vertices of a ribbon edge sit on it; the shader
    /// moves them apart.
    pub positions: Vec<[f32; 3]>,
    /// The wire's own direction at this vertex (`w` unused), which is the axis
    /// the ribbon is spread across.
    pub tangents: Vec<[f32; 4]>,
    /// `x` = which side of the centre line this vertex belongs to (−1 or +1),
    /// `y` = the wire's true half-width \[m\].
    pub across: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

impl ConductorPatch {
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }

    pub fn vertices(&self) -> usize {
        self.positions.len()
    }
}

/// Builds the conductor geometry of one tile: two triangles per piece, on the
/// wire's centre line.
pub(crate) fn patches(
    k: TileKey,
    frame: &EnuFrame,
    zone: u8,
    lines: &PowerLines,
) -> Vec<ConductorPatch> {
    let Some(pieces) = lines.by_tile.get(&k) else {
        return Vec::new();
    };
    let mut patch = ConductorPatch {
        positions: Vec::with_capacity(pieces.len() * 4),
        tangents: Vec::with_capacity(pieces.len() * 4),
        across: Vec::with_capacity(pieces.len() * 4),
        indices: Vec::with_capacity(pieces.len() * 6),
    };
    let local = |p: DVec3| -> DVec3 {
        let (lat, lon) = geo::from_utm(p.x, p.y, zone);
        frame.to_local(geo::to_ecef(lat, lon, p.z))
    };
    let half = (CONDUCTOR_WIDTH / 2.0) as f32;
    for piece in pieces {
        let a = local(piece.a);
        let b = local(piece.b);
        let along = b - a;
        if along.length_squared() < 1e-9 {
            continue;
        }
        let tangent = to_render_dir(along.normalize());
        let base = patch.positions.len() as u32;
        // a and b twice each, once for either side of the centre line.
        for (point, side) in [(a, -1.0), (b, -1.0), (b, 1.0), (a, 1.0)] {
            patch.positions.push(to_render(point));
            patch.tangents.push([tangent.x, tangent.y, tangent.z, 1.0]);
            patch.across.push([side, half]);
        }
        patch
            .indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    if patch.indices.is_empty() {
        return Vec::new();
    }
    vec![patch]
}

/// The masts of the line's overhead lines, as geo-positioned instances.
///
/// A mast is turned so that its crossarms stand across the line: the model's
/// front (−Z) runs along the conductors, and at a corner it bisects the two
/// directions, which is what a real mast does. The ends of a line and any mast
/// the line turns hard at carry the tension variant.
pub fn masts(lines: &[PowerLineSource]) -> Vec<TreeSource> {
    let mut out = Vec::new();
    for line in lines {
        if line.points.len() < 2 || line.object.is_empty() {
            continue;
        }
        for (i, point) in line.points.iter().enumerate() {
            let before = i.checked_sub(1).map(|j| bearing(&line.points[j], point));
            let after = line.points.get(i + 1).map(|next| bearing(point, next));
            let yaw = match (before, after) {
                (Some(a), Some(b)) => bisect(a, b),
                (Some(a), None) => a,
                (None, Some(b)) => b,
                (None, None) => 0.0,
            };
            let object = if point.tension && !line.tension_object.is_empty() {
                &line.tension_object
            } else {
                &line.object
            };
            out.push(TreeSource {
                object: object.clone(),
                lat: point.lat,
                lon: point.lon,
                yaw_deg: yaw,
                scale: 1.0,
            });
        }
    }
    out
}

/// The bearing from `a` to `b` [deg, clockwise from north]. Flat-earth over the
/// two or three hundred metres between two masts, which is exact to well under
/// a degree — and a degree of mast rotation is not a thing anybody sees.
pub(crate) fn bearing(a: &crate::route::PowerPoint, b: &crate::route::PowerPoint) -> f64 {
    let mean = ((a.lat + b.lat) / 2.0).to_radians();
    let east = (b.lon - a.lon) * mean.cos();
    let north = b.lat - a.lat;
    east.atan2(north).to_degrees().rem_euclid(360.0)
}

/// The direction that bisects two bearings, the short way round.
fn bisect(a: f64, b: f64) -> f64 {
    let delta = (b - a + 540.0).rem_euclid(360.0) - 180.0;
    (a + delta / 2.0).rem_euclid(360.0)
}

/// Whether the line turns hard enough at `i` for the mast to be a tension mast.
///
/// A suspension mast can take a few degrees of corner; beyond that the sideways
/// pull of the conductors needs a mast built to be pulled on. Fifteen degrees is
/// the usual limit in the German drawings, and it also picks out exactly the
/// masts a viewer reads as "the line turns here".
pub fn turns_hard(before: f64, after: f64) -> bool {
    let delta = (after - before + 540.0).rem_euclid(360.0) - 180.0;
    delta.abs() > 15.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::PowerPoint;

    fn point(lat: f64, lon: f64) -> PowerPoint {
        PowerPoint {
            lat,
            lon,
            tension: false,
        }
    }

    /// [`PRESETS`] is a copy of the atlas, and a copy drifts. This reads the
    /// atlas the models are generated from and fails the moment a crossarm in
    /// `tools/pylons/pylons.json` stops agreeing with the table the import
    /// stamps into a line — which would put the conductors beside the insulator
    /// strings instead of on them.
    #[test]
    fn the_table_matches_the_atlas() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tools/pylons/pylons.json"
        );
        let text = std::fs::read_to_string(path).expect("the atlas is in the repository");
        let atlas: serde_json::Value = serde_json::from_str(&text).expect("the atlas parses");
        let types = atlas["types"].as_array().expect("types");
        assert_eq!(types.len(), PRESETS.len(), "one preset per atlas entry");

        for (entry, preset) in types.iter().zip(PRESETS) {
            let id = entry["id"].as_str().unwrap();
            assert_eq!(id, preset.id);
            let band = entry["height_m"].as_array().unwrap();
            let height = (band[0].as_f64().unwrap() + band[1].as_f64().unwrap()) / 2.0;
            assert!(
                (height - preset.height).abs() < 0.05,
                "{id}: height {height} vs {}",
                preset.height
            );
            let root = entry["build"]["shaft_m"].as_f64().unwrap() / 2.0;
            assert!((root - preset.root).abs() < 0.005, "{id}: root");

            let arms = entry["crossarms"].as_array().unwrap();
            assert_eq!(arms.len(), preset.arms.len(), "{id}: crossarm count");
            for (arm, built) in arms.iter().zip(preset.arms) {
                let at = arm["at_frac"].as_f64().unwrap() * height;
                assert!((at - built.at).abs() < 0.05, "{id}: arm height");
                let half = arm["width_m"].as_f64().unwrap() / 2.0;
                assert!((half - built.half_width).abs() < 0.005, "{id}: arm width");
                assert_eq!(
                    arm["conductors"].as_u64().unwrap() as u8,
                    built.conductors,
                    "{id}: conductors"
                );
            }
        }
    }

    /// Every preset names objects the generated mod actually ships. A typo here
    /// is a line of masts that silently draws the placeholder.
    #[test]
    fn every_preset_names_a_built_object() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods/pylons/objects");
        for preset in PRESETS {
            for name in [preset.object, preset.tension_object] {
                if name.is_empty() {
                    continue;
                }
                let stem = name.strip_prefix("pylons:").expect("mod-qualified");
                let path = std::path::Path::new(root).join(format!("{stem}.ron"));
                assert!(path.exists(), "{name}: {} is missing", path.display());
            }
        }
    }

    /// A Donaumast carries two conductors on the upper arm and four on the
    /// lower — one and two a side, outermost at the tip.
    #[test]
    fn a_donaumast_hangs_its_conductors_where_the_arms_are() {
        let preset = preset("donaumast-380").expect("in the table");
        let upper = preset.arms[0].offsets(preset.root);
        assert_eq!(upper.len(), 2);
        assert!(
            (upper[1] - preset.arms[0].half_width).abs() < 1e-9,
            "at the tip"
        );
        assert!((upper[0] + upper[1]).abs() < 1e-9, "symmetric");

        let lower = preset.arms[1].offsets(preset.root);
        assert_eq!(lower.len(), 4);
        let outer = lower.iter().cloned().fold(f64::MIN, f64::max);
        assert!((outer - preset.arms[1].half_width).abs() < 1e-9);
        // The inner conductor is inside the outer one and outside the body.
        let inner = lower
            .iter()
            .filter(|x| **x > 0.0)
            .fold(f64::MAX, |a, b| a.min(*b));
        assert!(
            inner > preset.root && inner < outer,
            "{inner} in ({}, {outer})",
            preset.root
        );
    }

    /// The mast turns so that its crossarms stand across the line, and it
    /// bisects the corner where the line bends.
    #[test]
    fn masts_face_along_the_line() {
        let preset = preset("donaumast-110").expect("in the table");
        // Due east, then due north.
        let points = vec![point(52.0, 10.0), point(52.0, 10.01), point(52.01, 10.01)];
        let line = source_from(preset, "Test".into(), points, vec![]);
        let placed = masts(std::slice::from_ref(&line));
        assert_eq!(placed.len(), 3);
        assert!(
            (placed[0].yaw_deg - 90.0).abs() < 1.0,
            "{}",
            placed[0].yaw_deg
        );
        // The corner bisects east (90°) and north (0°).
        assert!(
            (placed[1].yaw_deg - 45.0).abs() < 1.0,
            "{}",
            placed[1].yaw_deg
        );
        assert!(placed[2].yaw_deg.abs() < 1.0, "{}", placed[2].yaw_deg);
    }

    /// A tension point takes the tension object, a plain one the suspension
    /// object — and a line whose type is not built as a tension mast takes the
    /// suspension object everywhere rather than nothing.
    #[test]
    fn tension_points_take_the_tension_mast() {
        let preset = preset("donaumast-380").expect("in the table");
        let mut points = vec![point(52.0, 10.0), point(52.0, 10.01), point(52.0, 10.02)];
        points[0].tension = true;
        let line = source_from(preset, String::new(), points, vec![]);
        let placed = masts(std::slice::from_ref(&line));
        assert_eq!(placed[0].object, "pylons:donaumast_380_abspann");
        assert_eq!(placed[1].object, "pylons:donaumast_380_trag");

        let mut plain = line.clone();
        plain.tension_object = String::new();
        let placed = masts(std::slice::from_ref(&plain));
        assert_eq!(placed[0].object, "pylons:donaumast_380_trag");
    }

    /// The conductor hangs. The middle of a span is below the straight line
    /// between its ends by four times the sag share times the span, divided by
    /// four — the parabola's rise — and that is what makes a power line read as
    /// a power line rather than as a fence in the sky.
    #[test]
    fn a_span_sags_in_the_middle() {
        // One crossarm, so what the heights span is the sag and nothing else.
        let preset = preset("bahnstrommast-110").expect("in the table");
        // Two masts about 400 m apart, along a meridian.
        let points = vec![point(52.0, 10.0), point(52.0036, 10.0)];
        let line = source_from(preset, String::new(), points, vec![]);
        let mut lines = PowerLines::from_parts(std::slice::from_ref(&line), 32, 512.0);
        // No elevation data: every foot lands on the fallback, which is what a
        // test scene has.
        lines.prepare(&[], 32, 46.0, 100.0);
        assert!(!lines.is_empty());

        let mut heights: Vec<f64> = Vec::new();
        for pieces in lines.by_tile.values() {
            for piece in pieces {
                heights.push(piece.a.z);
                heights.push(piece.b.z);
            }
        }
        assert!(!heights.is_empty(), "the span was strung");
        let top = heights.iter().cloned().fold(f64::MIN, f64::max);
        let bottom = heights.iter().cloned().fold(f64::MAX, f64::min);
        let span = line.length(32);
        let expected = line.sag * span;
        assert!(
            ((top - bottom) - expected).abs() < 0.5,
            "sag {} m over {span:.0} m, expected {expected:.1} m",
            top - bottom
        );
        // And the top is the mast's own crossarm, not something invented: the
        // fallback ground (100 m) plus the geoid offset (46 m) plus the arm.
        let arm = preset.arms[0].at;
        assert!((top - (146.0 + arm)).abs() < 0.5, "top {top}");
    }

    /// A line with fewer than two masts is not a line, and a hand-edited file
    /// that says so gets nothing rather than a panic.
    #[test]
    fn a_single_point_is_not_a_line() {
        let preset = preset("donaumast-110").expect("in the table");
        let line = source_from(preset, String::new(), vec![point(52.0, 10.0)], vec![]);
        assert!(masts(std::slice::from_ref(&line)).is_empty());
        assert!(PowerLines::from_parts(std::slice::from_ref(&line), 32, 512.0).is_empty());
    }
}
