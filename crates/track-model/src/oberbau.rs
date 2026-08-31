//! The Oberbau — what the track is physically built of, in the dimensions the
//! DB InfraGO drawings give (plan ch. 15).
//!
//! This is the one place the real measurements live. The renderer extrudes
//! what [`RailProfile::contour`] hands it and stacks what [`Oberbau`] says;
//! it invents no millimetre of its own. That split is deliberate: a rail
//! section is a property of the rail, not of a graphics backend, and the
//! route editor's rule check reads the same numbers the simulator draws.
//!
//! The vertical datum for everything here is **SO** (Schienenoberkante, the
//! top of rail); depths grow downwards. Stacked up, the Regeloberbau reads:
//!
//! ```text
//!  0 mm   SO — top of the rail head
//! 14 mm   gauge measuring plane: 1435 mm between the inner head faces
//! 172 mm  rail foot (60E1)
//! 182 mm  sleeper top — the rail pad (Zwischenlage) is 10 mm
//! 396 mm  sleeper underside (B 70: 214 mm at the rail seat)
//! 696 mm  Planum — 300 mm of ballast under the sleeper (Hauptbahn)
//! ```
//!
//! Sources: EN 13674-1 for the rolled rail sections, DB Ril 800.0130 /
//! 820.2010 for the Bettungsquerschnitt, the B 70 / B 90 sleeper drawings for
//! the concrete sleepers and the DB Regelschwelle for the timber ones.

use serde::{Deserialize, Serialize};

/// Track gauge \[m\]: 1435 mm between the inner head faces, measured
/// [`GAUGE_MEASURE`] below the running surface.
pub const GAUGE: f64 = 1.435;

/// Depth below the top of rail where the gauge is measured \[m\]. It is also
/// where a Vignol head is at its widest — the head width of a profile is the
/// width in this plane, which is what makes the two numbers one measurement
/// and lets [`RailProfile::contour`] solve the head arcs from them.
pub const GAUGE_MEASURE: f64 = 0.014;

/// Rails stand inclined 1:40 towards the gauge — the head leans in. On
/// concrete sleepers the inclination is cast into the rail seat, on timber
/// ones the ribbed baseplate carries it; either way the whole rail is
/// *rotated*, running surface included, not sheared.
pub const RAIL_CANT: f64 = 1.0 / 40.0;

/// Density of rail steel R260 \[kg/m³\] — what turns a section area into the
/// kilograms per metre the profile is named after.
pub const RAIL_STEEL_DENSITY: f64 = 7850.0;

/// A rail section (Vignol profile) — what the track's rails are rolled as.
/// The renderer extrudes the real cross-section, so the dimensions here are
/// the ones the drawing shows: EN 13674-1 / DB Ril 821.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RailProfile {
    /// 49E1 (DIN S 49): 149 mm high, 67 mm head, 125 mm foot, 49.43 kg/m.
    /// The Reichsbahn standard, still on lightly loaded branch lines.
    R49,
    /// 54E3 (DIN S 54): 154 mm high, 67 mm head, 125 mm foot, 54.54 kg/m.
    /// DB standard from 1963, main lines and station tracks.
    R54,
    /// 60E1 (UIC 60): 172 mm high, 72 mm head, 150 mm foot, 60.21 kg/m.
    /// Heavy and fast lines since 1970 — the current main-line standard, and
    /// what a type is laid with when it says nothing about its rail.
    #[default]
    R60,
}

/// The rolled dimensions of one profile \[m\], all depths from the top of
/// rail. The three head radii are the same on every Vignol profile the DB
/// lays (R 300 running surface, R 80 shoulders, R 13 gauge corner), so they
/// are constants of the section rather than fields.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RailSection {
    /// Overall height, running surface to foot underside.
    pub height: f64,
    /// Head width in the gauge measuring plane — the head's widest point.
    pub head_width: f64,
    /// Foot width.
    pub foot_width: f64,
    /// Web thickness.
    pub web_thickness: f64,
    /// Foot thickness at its outer edge.
    pub foot_edge_thickness: f64,
    /// Depth of the lower end of the head's side faces — below this the head
    /// flares in towards the web.
    pub head_side_depth: f64,
    /// Depth where the head's underside has run into the web.
    pub head_depth: f64,
    /// Depth where the foot's top surface leaves the web. This is the one
    /// ordinate the summary tables of the standards do not print, so it is
    /// set to the value at which the polygon, times the density of rail
    /// steel, weighs the kilograms per metre the profile is named after —
    /// which is what `the_section_weighs_what_the_profile_is_called` checks.
    pub foot_top_depth: f64,
    /// Nominal mass \[kg/m\] — the number the profile is named after, and
    /// what the section area is checked against.
    pub mass: f64,
}

/// Running-surface radius of a Vignol head \[m\] (R 300).
const HEAD_CROWN_RADIUS: f64 = 0.300;
/// Shoulder radius between crown and gauge corner \[m\] (R 80).
const HEAD_FLANK_RADIUS: f64 = 0.080;
/// Gauge corner radius \[m\] (R 13) — the radius the wheel flange rides.
const HEAD_CORNER_RADIUS: f64 = 0.013;
/// How much narrower the head is at the bottom of its side faces than in the
/// gauge measuring plane \[m\], per side — the slight draft of the rolled head.
const HEAD_SIDE_DRAFT: f64 = 0.002;
/// Fillet where the head's underside runs into the web \[m\].
const HEAD_WEB_FILLET: f64 = 0.020;
/// Fillet where the web runs into the foot \[m\].
const WEB_FOOT_FILLET: f64 = 0.024;
/// Chamfer at the foot's outer edge \[m\].
const FOOT_EDGE_CHAMFER: f64 = 0.004;

impl RailProfile {
    /// The rolled dimensions of this profile.
    pub fn dimensions(&self) -> RailSection {
        match self {
            Self::R49 => RailSection {
                height: 0.149,
                head_width: 0.067,
                foot_width: 0.125,
                web_thickness: 0.014,
                foot_edge_thickness: 0.0105,
                head_side_depth: 0.028,
                head_depth: 0.044,
                foot_top_depth: 0.1153,
                mass: 49.43,
            },
            Self::R54 => RailSection {
                height: 0.154,
                head_width: 0.067,
                foot_width: 0.125,
                web_thickness: 0.016,
                foot_edge_thickness: 0.0105,
                head_side_depth: 0.028,
                head_depth: 0.044,
                foot_top_depth: 0.1119,
                mass: 54.54,
            },
            Self::R60 => RailSection {
                height: 0.172,
                head_width: 0.072,
                foot_width: 0.150,
                web_thickness: 0.0165,
                foot_edge_thickness: 0.0115,
                head_side_depth: 0.030,
                head_depth: 0.0495,
                foot_top_depth: 0.1425,
                mass: 60.21,
            },
        }
    }

    /// (height, head width, foot width) \[m\] — the three numbers most of the
    /// code only ever needs.
    pub fn section(&self) -> (f64, f64, f64) {
        let d = self.dimensions();
        (d.height, d.head_width, d.foot_width)
    }

    /// The closed cross-section of the rail, counter-clockwise from the crown
    /// centre over the field side and back over the gauge side.
    ///
    /// `steps` is the tessellation of the head's arcs — the running surface
    /// is what the light works on, so it gets its own knob; everything below
    /// is straights and fillets and needs far less. The points carry their
    /// own [`RailPoint::polish`], because where the wheels reach is a fact
    /// about the section, not something a shader should guess from a normal.
    pub fn contour(&self, steps: usize) -> Vec<RailPoint> {
        mirror(&self.half_contour(steps.max(3)))
    }

    /// The same section reduced to its envelope: eight points a side, no
    /// arcs and no fillets. For the level of detail past a few hundred
    /// metres, where a rail head is under a pixel wide and every fillet is
    /// a vertex spent on nothing.
    pub fn coarse_contour(&self) -> Vec<RailPoint> {
        let d = self.dimensions();
        let half = [
            (0.0, 0.0),
            (d.head_width / 2.0, GAUGE_MEASURE * 0.5),
            (d.head_width / 2.0 - HEAD_SIDE_DRAFT, d.head_side_depth),
            (d.web_thickness / 2.0, d.head_depth),
            (d.web_thickness / 2.0, d.foot_top_depth),
            (d.foot_width / 2.0, d.height - d.foot_edge_thickness),
            (d.foot_width / 2.0, d.height),
            (0.0, d.height),
        ];
        let half: Vec<RailPoint> = half
            .into_iter()
            .map(|(across, down)| RailPoint {
                across,
                down,
                polish: polish_at(across, down, &d),
                flank: flank_at(down, &d),
            })
            .collect();
        mirror(&half)
    }

    /// One half of the section, from the crown centre `(0, 0)` down the side
    /// to the foot's underside on the axis.
    fn half_contour(&self, steps: usize) -> Vec<RailPoint> {
        let d = self.dimensions();
        let mut pts: Vec<(f64, f64)> = head_top(d.head_width, steps);

        // Head side, underside, web, foot — a polyline whose corners are
        // rounded to the fillets of the rolled section.
        let head_bottom_half = d.head_width / 2.0 - HEAD_SIDE_DRAFT;
        let web_half = d.web_thickness / 2.0;
        let foot_half = d.foot_width / 2.0;
        let corners = [
            // Down the head's side face.
            ((head_bottom_half, d.head_side_depth), 0.0),
            // In under the head to the web.
            ((web_half, d.head_depth), HEAD_WEB_FILLET),
            // Down the web to where the foot starts.
            ((web_half, d.foot_top_depth), WEB_FOOT_FILLET),
            // Out over the foot's top surface to its edge.
            (
                (
                    foot_half - FOOT_EDGE_CHAMFER,
                    d.height - d.foot_edge_thickness,
                ),
                0.0,
            ),
            // The foot's edge and its chamfered underside corner.
            (
                (
                    foot_half,
                    d.height - d.foot_edge_thickness + FOOT_EDGE_CHAMFER,
                ),
                0.0,
            ),
            ((foot_half, d.height), 0.0),
            ((0.0, d.height), 0.0),
        ];
        let mut path: Vec<(f64, f64)> = vec![*pts.last().expect("head has points")];
        path.extend(corners.iter().map(|(p, _)| *p));
        let radii: Vec<f64> = corners.iter().map(|(_, r)| *r).collect();
        pts.extend(fillet_path(&path, &radii).into_iter().skip(1));

        pts.into_iter()
            .map(|(across, down)| RailPoint {
                across,
                down,
                polish: polish_at(across, down, &d),
                flank: flank_at(down, &d),
            })
            .collect()
    }
}

/// Closes a half section into the whole one: the given half over the field
/// side, then its mirror back over the gauge side. The two points on the
/// axis — the crown centre and the middle of the foot — are not repeated.
fn mirror(half: &[RailPoint]) -> Vec<RailPoint> {
    let mut points = Vec::with_capacity(half.len() * 2 - 2);
    points.extend(half.iter().copied());
    for p in half.iter().rev().skip(1).take(half.len() - 2) {
        points.push(RailPoint {
            across: -p.across,
            ..*p
        });
    }
    points
}

/// One point of a rail's cross-section, with what the surface there looks
/// like. `across` is the lateral offset from the rail's own axis \[m\] and
/// `down` the depth below the running surface \[m\], both before the 1:40
/// inclination is applied.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RailPoint {
    pub across: f64,
    pub down: f64,
    /// How far the wheels have polished the steel here, 0 … 1: 1 on the
    /// running band the tread rides, a dull sheen around the gauge corner a
    /// flange touches in curves, 0 everywhere the wheel never reaches — that
    /// is where a rail rusts.
    pub polish: f64,
    /// 1 on the head's side faces, 0 elsewhere. On the gauge side those are
    /// the faces the head shades itself; no shadow map resolves that at the
    /// scale of a rail head, so the shader bakes it and needs to be told
    /// which faces it is.
    pub flank: f64,
}

/// The head's running surface, from the crown centre out to the widest point
/// in the gauge measuring plane, as the three arcs of the rolled head:
/// R 300 crown, R 80 shoulder, R 13 gauge corner.
///
/// The arcs are fully determined by the head width and [`GAUGE_MEASURE`]:
/// the chain has to reach `(w/2, 14 mm)` with a vertical tangent, which fixes
/// the two turning angles. For a 72 mm head that puts the crown's end at
/// ±20 mm, which is the running band a wheel tread actually leaves.
fn head_top(head_width: f64, steps: usize) -> Vec<(f64, f64)> {
    let (r1, r2, r3) = (HEAD_CROWN_RADIUS, HEAD_FLANK_RADIUS, HEAD_CORNER_RADIUS);
    // The chain of arcs ends at (w/2, g) with a vertical tangent:
    //   (r1 - r2) sin a + (r2 - r3) sin b = w/2 - r3
    //   (r1 - r2) cos a + (r2 - r3) cos b = r1 - g
    // — two vectors of known length summing to a known one, so the triangle
    // gives the angles.
    let (a_len, b_len) = (r1 - r2, r2 - r3);
    let (sx, sy) = (head_width / 2.0 - r3, r1 - GAUGE_MEASURE);
    let s_len = sx.hypot(sy);
    let cos_beta =
        ((a_len * a_len + s_len * s_len - b_len * b_len) / (2.0 * a_len * s_len)).clamp(-1.0, 1.0);
    let crown_end = sx.atan2(sy) - cos_beta.acos();
    let flank_end = (sx - a_len * crown_end.sin()).atan2(sy - a_len * crown_end.cos());

    // Centres of the shoulder and corner arcs, offset from the crown's centre
    // along the normal at each tangency.
    let c2 = (a_len * crown_end.sin(), r1 - a_len * crown_end.cos());
    let c3 = (
        a_len * crown_end.sin() + b_len * flank_end.sin(),
        r1 - a_len * crown_end.cos() - b_len * flank_end.cos(),
    );
    let arcs = [
        ((0.0, r1), r1, 0.0, crown_end),
        (c2, r2, crown_end, flank_end),
        (c3, r3, flank_end, std::f64::consts::FRAC_PI_2),
    ];

    // Split the tessellation by arc *length*, not by turn angle. The crown
    // turns barely 4° but runs 20 mm — it is where the highlight lives and
    // needs the points; the R 13 corner turns most of the 90° in 19 mm and
    // needs about as many.
    let total: f64 = arcs.iter().map(|&(_, r, from, to)| r * (to - from)).sum();
    let mut pts = Vec::with_capacity(steps * 2 + 2);
    for (i, &((cx, cy), r, from, to)) in arcs.iter().enumerate() {
        let n = ((steps as f64 * r * (to - from) / total).round() as usize).max(1);
        // The crown starts on the axis; every later arc starts where the one
        // before it ended and must not repeat that point.
        let first = usize::from(i > 0);
        for k in first..=n {
            let t = from + (to - from) * k as f64 / n as f64;
            pts.push((cx + r * t.sin(), cy - r * t.cos()));
        }
    }
    pts
}

/// Walks a polyline and rounds every corner that has a radius, so the head's
/// underside runs into the web and the web into the foot the way the rolled
/// section does instead of meeting at a crease.
///
/// `radii[i]` belongs to `path[i + 1]`; a zero radius keeps the corner sharp.
/// A radius the neighbouring segments cannot fit is shortened to what fits —
/// a profile is data a mod may set, and a bad number must round badly, not
/// fold the section inside out.
fn fillet_path(path: &[(f64, f64)], radii: &[f64]) -> Vec<(f64, f64)> {
    /// Points a rounded corner is tessellated with.
    const CORNER_STEPS: usize = 5;

    let mut out = vec![path[0]];
    for i in 1..path.len() - 1 {
        let (prev, here, next) = (path[i - 1], path[i], path[i + 1]);
        let (in_dir, in_len) = direction(here, prev);
        let (out_dir, out_len) = direction(here, next);
        let radius = radii.get(i - 1).copied().unwrap_or(0.0);
        // The half angle between the two legs decides how far up each leg the
        // tangent point sits: t = r / tan(θ/2).
        let cos_theta = (in_dir.0 * out_dir.0 + in_dir.1 * out_dir.1).clamp(-1.0, 1.0);
        let half = cos_theta.acos() / 2.0;
        let tangent = if radius > 0.0 && half > 1e-4 && half < std::f64::consts::FRAC_PI_2 - 1e-4 {
            (radius / half.tan()).min(in_len * 0.5).min(out_len * 0.5)
        } else {
            0.0
        };
        if tangent <= 1e-6 {
            out.push(here);
            continue;
        }
        let start = (here.0 + in_dir.0 * tangent, here.1 + in_dir.1 * tangent);
        let end = (here.0 + out_dir.0 * tangent, here.1 + out_dir.1 * tangent);
        // The centre sits on the bisector: the tangent point is `tangent`
        // along a leg, the centre `tangent / cos(θ/2)` along the bisector.
        let bisector = normalize((in_dir.0 + out_dir.0, in_dir.1 + out_dir.1));
        let reach = tangent / half.cos();
        let centre = (here.0 + bisector.0 * reach, here.1 + bisector.1 * reach);
        let a0 = (start.1 - centre.1).atan2(start.0 - centre.0);
        let a1 = (end.1 - centre.1).atan2(end.0 - centre.0);
        // Take the short way round — the corner of a section never bends
        // more than half a turn.
        let mut sweep = a1 - a0;
        while sweep > std::f64::consts::PI {
            sweep -= std::f64::consts::TAU;
        }
        while sweep < -std::f64::consts::PI {
            sweep += std::f64::consts::TAU;
        }
        let r_actual = (start.0 - centre.0).hypot(start.1 - centre.1);
        for k in 0..=CORNER_STEPS {
            let a = a0 + sweep * k as f64 / CORNER_STEPS as f64;
            out.push((centre.0 + r_actual * a.cos(), centre.1 + r_actual * a.sin()));
        }
    }
    out.push(*path.last().expect("path has points"));
    out
}

/// Unit vector from `from` towards `to`, and how far it is.
fn direction(from: (f64, f64), to: (f64, f64)) -> ((f64, f64), f64) {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let len = dx.hypot(dy);
    if len < 1e-12 {
        return ((0.0, 0.0), 0.0);
    }
    ((dx / len, dy / len), len)
}

fn normalize(v: (f64, f64)) -> (f64, f64) {
    let len = v.0.hypot(v.1);
    if len < 1e-12 {
        (0.0, 0.0)
    } else {
        (v.0 / len, v.1 / len)
    }
}

/// How polished the steel is at a point of the section. The wheel tread rides
/// the middle of the crown and leaves it mirror bright; the shoulders take a
/// flange only in curves and stay dull; below the gauge measuring plane
/// nothing touches the rail and it rusts.
fn polish_at(across: f64, down: f64, d: &RailSection) -> f64 {
    let band = 1.0 - smoothstep(0.28 * d.head_width, 0.46 * d.head_width, across.abs());
    let reach = 1.0 - smoothstep(0.006, GAUGE_MEASURE + 0.002, down);
    band * reach
}

/// 1 on the head's side faces — the band between the gauge measuring plane
/// and the underside of the head.
fn flank_at(down: f64, d: &RailSection) -> f64 {
    smoothstep(GAUGE_MEASURE - 0.002, GAUGE_MEASURE + 0.004, down)
        * (1.0 - smoothstep(d.head_side_depth, d.head_depth, down))
}

fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    if edge1 <= edge0 {
        return f64::from(x >= edge1);
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How the track is supported: concrete sleepers, wooden sleepers, or a slab
/// (Feste Fahrbahn) instead of a ballast bed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SleeperKind {
    /// Reinforced-concrete sleeper (B 70, B 90, …): trapezoid in section,
    /// thicker under the rail seats than in the middle.
    #[default]
    Concrete,
    /// Impregnated timber sleeper (DB Regelschwelle 2600 × 260 × 160 mm):
    /// a plain beam, and it sits lower than a concrete one.
    Wood,
    /// Feste Fahrbahn: a continuous concrete slab replaces sleepers and
    /// ballast. Of the sleeper fields only length (slab width), height (slab
    /// thickness) and the textures are used; spacing and ballast are not.
    Slab,
}

/// The rail fastening — what stands on the sleeper beside the rail foot.
/// Small, but it is the difference between track and a ladder: at the length
/// of a platform it is the only thing that gives a sleeper a top side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Fastening {
    /// System W 14 — the elastic fastening of the DB Regeloberbau: an angled
    /// guide plate each side of the foot and the Spannklemme Skl 14 over it.
    #[default]
    W14,
    /// Oberbau K — the ribbed baseplate of timber track, the rail clamped to
    /// it by two clamp plates and screwed through. The plate carries the 1:40.
    K,
    /// Nothing modelled — for a type whose sleepers are their own model.
    None,
}

impl Fastening {
    /// Whether anything is drawn at all.
    pub fn is_some(self) -> bool {
        !matches!(self, Self::None)
    }
}

/// The physical build of the track — the Oberbau: rail section, sleepers and
/// ballast bed, in the real dimensions. Defaults are the DB Regeloberbau
/// (60E1 on B 70 at 60 cm, 30 cm ballast); a type that says nothing about its
/// build is laid like that.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Oberbau {
    /// Rail section the two running rails are extruded from.
    #[serde(default)]
    pub rail: RailProfile,
    /// What supports the rails — and whether there is a ballast bed at all.
    #[serde(default)]
    pub sleeper: SleeperKind,
    /// Sleeper length \[m\] across the track (2.6 m DB standard); on a slab,
    /// the slab width.
    #[serde(default = "default_sleeper_length")]
    pub sleeper_length: f64,
    /// Sleeper width at the underside \[m\], measured along the track
    /// (B 70: 0.30, B 90: 0.32, timber: 0.26); on a slab, unused.
    #[serde(default = "default_sleeper_width")]
    pub sleeper_width: f64,
    /// Sleeper width at the top \[m\]. A concrete sleeper is cast with draft,
    /// so its top is narrower than its base (B 70: 0.22 over 0.30); a timber
    /// sleeper is sawn square and has the same width top and bottom. `None`
    /// takes whichever of the two the sleeper kind implies — see
    /// [`Oberbau::top_width`].
    #[serde(default)]
    pub sleeper_top_width: Option<f64>,
    /// Sleeper height under the rail seat \[m\] (B 70/B 90: 0.214, timber:
    /// 0.16); on a slab, the slab thickness — see [`SleeperKind::Slab`].
    #[serde(default = "default_sleeper_height")]
    pub sleeper_height: f64,
    /// Sleeper height in the middle \[m\] (B 70: 0.175). A concrete sleeper
    /// is a beam on two supports and is cast thinner where it carries least;
    /// a timber one is the same depth end to end. `None` follows the sleeper
    /// kind — see [`Oberbau::mid_height`].
    #[serde(default)]
    pub sleeper_mid_height: Option<f64>,
    /// Distance between sleeper centres \[m\]; 0.60 m = 1667 per km, what
    /// almost every DB track is laid at.
    #[serde(default = "default_sleeper_spacing")]
    pub sleeper_spacing: f64,
    /// Rail pad (Zwischenlage) between rail foot and sleeper \[m\] — 10 mm
    /// of elastic pad on the Regeloberbau. It is what sets the sleeper top
    /// below the rail foot.
    #[serde(default = "default_rail_pad")]
    pub rail_pad: f64,
    /// What holds the rail down (see [`Fastening`]). `None` takes the system
    /// the sleeper kind is laid with: W 14 on concrete, Oberbau K on timber.
    #[serde(default)]
    pub fastening: Option<Fastening>,
    /// Ballast shoulder beyond the sleeper end \[m\] each side — the
    /// Schotterschulter, level with the sleeper top before the slope starts.
    /// DB Ril 800.0130 asks 0.40 m, and 0.50 m above 160 km/h.
    #[serde(default = "default_ballast_shoulder")]
    pub ballast_overhang: f64,
    /// Ballast under the sleeper \[m\] (30 cm Hauptbahn, 20 cm Nebenbahn) —
    /// the bed's depth from sleeper underside to Planum.
    #[serde(default = "default_ballast_depth")]
    pub ballast_depth: f64,
    /// Slope of the ballast shoulder, run per unit of fall — 1:1.5 is the DB
    /// standard section; a steeper bed will not stay put.
    #[serde(default = "default_ballast_slope")]
    pub ballast_slope: f64,
    /// How far below the sleeper top the crib ballast between two sleepers
    /// lies \[m\]. Freshly tamped DB track is filled flush with the sleeper
    /// top; a few centimetres down is what a bed looks like after some
    /// traffic, and it is what makes a sleeper sit *in* the bed rather than
    /// on it.
    #[serde(default = "default_crib_drop")]
    pub crib_drop: f64,
    /// Sleeper texture (`mods://<mod>/assets/…`); on a slab, the slab's.
    #[serde(default)]
    pub sleeper_texture: Option<String>,
    /// Sleeper normal map, same tiling as the texture.
    #[serde(default)]
    pub sleeper_normal_map: Option<String>,
    /// How many metres one repeat of those two covers \[m\] — the size of
    /// the patch the scan was taken from. The sleeper is mapped
    /// isotropically, `u` along its length and `v` around its section, so
    /// this is the one number that decides whether a sleeper shows the
    /// aggregate of its concrete or one smooth blown-up smear of it. A
    /// timber sleeper wants the whole plank on it (2.6 m); cast concrete
    /// wants the scan's own metre or so. `None` follows the sleeper kind.
    #[serde(default)]
    pub sleeper_texture_scale: Option<f64>,
}

fn default_sleeper_length() -> f64 {
    2.6
}

fn default_sleeper_width() -> f64 {
    0.30
}

fn default_sleeper_height() -> f64 {
    0.214
}

fn default_sleeper_spacing() -> f64 {
    0.60
}

fn default_rail_pad() -> f64 {
    0.010
}

fn default_ballast_shoulder() -> f64 {
    0.40
}

fn default_ballast_depth() -> f64 {
    0.30
}

fn default_ballast_slope() -> f64 {
    1.5
}

fn default_crib_drop() -> f64 {
    0.04
}

impl Default for Oberbau {
    fn default() -> Self {
        Self {
            rail: RailProfile::default(),
            sleeper: SleeperKind::default(),
            sleeper_length: default_sleeper_length(),
            sleeper_width: default_sleeper_width(),
            sleeper_top_width: None,
            sleeper_height: default_sleeper_height(),
            sleeper_mid_height: None,
            sleeper_spacing: default_sleeper_spacing(),
            rail_pad: default_rail_pad(),
            fastening: None,
            ballast_overhang: default_ballast_shoulder(),
            ballast_depth: default_ballast_depth(),
            ballast_slope: default_ballast_slope(),
            crib_drop: default_crib_drop(),
            sleeper_texture: None,
            sleeper_normal_map: None,
            sleeper_texture_scale: None,
        }
    }
}

impl Oberbau {
    /// Width of the sleeper's top face \[m\]. A concrete sleeper is cast in
    /// a mould and comes out with draft — 80 mm narrower on top than at the
    /// base on a B 70; a timber sleeper is sawn and is a beam.
    pub fn top_width(&self) -> f64 {
        self.sleeper_top_width.unwrap_or(match self.sleeper {
            SleeperKind::Concrete => (self.sleeper_width - 0.08).max(0.05),
            SleeperKind::Wood | SleeperKind::Slab => self.sleeper_width,
        })
    }

    /// Height of the sleeper in the middle \[m\]. A concrete sleeper is a
    /// beam carried at its two rail seats and is cast shallower between them
    /// (B 70: 175 mm against 214 mm); a timber one is the same all along.
    pub fn mid_height(&self) -> f64 {
        self.sleeper_mid_height.unwrap_or(match self.sleeper {
            SleeperKind::Concrete => (self.sleeper_height - 0.039).max(0.05),
            SleeperKind::Wood | SleeperKind::Slab => self.sleeper_height,
        })
    }

    /// How many metres one repeat of the sleeper's texture covers \[m\].
    /// Timber shows one plank over the whole sleeper; concrete shows the
    /// patch of a scan, and blowing that up to 2.6 m is what turns a
    /// sleeper into a smooth pale slab.
    pub fn texture_scale(&self) -> f64 {
        self.sleeper_texture_scale.unwrap_or(match self.sleeper {
            SleeperKind::Wood => self.sleeper_length,
            SleeperKind::Concrete | SleeperKind::Slab => 1.0,
        })
    }

    /// The fastening system on this sleeper: W 14 on concrete, Oberbau K on
    /// timber, nothing on a slab unless the type says otherwise.
    pub fn fastening(&self) -> Fastening {
        self.fastening.unwrap_or(match self.sleeper {
            SleeperKind::Concrete | SleeperKind::Slab => Fastening::W14,
            SleeperKind::Wood => Fastening::K,
        })
    }

    /// Depth of the sleeper's top below the top of rail \[m\] — the rail
    /// section plus its pad. On a slab, the depth of the slab surface.
    pub fn sleeper_top(&self) -> f64 {
        self.rail.dimensions().height + self.rail_pad
    }

    /// Depth of the sleeper's underside below the top of rail \[m\], under
    /// the rail seat where the sleeper is deepest.
    pub fn sleeper_base(&self) -> f64 {
        self.sleeper_top() + self.sleeper_height
    }

    /// Depth of the Planum below the top of rail \[m\] — the formation the
    /// ballast bed stands on. 0.696 m on the Regeloberbau, which is what the
    /// terrain has to pull the ground down to beside the track.
    pub fn planum(&self) -> f64 {
        self.sleeper_base() + self.ballast_depth
    }

    /// Lateral distance of one rail's axis from the track centre \[m\]: the
    /// gauge is measured between the inner head faces, so the axis sits half
    /// a head width beyond the half gauge.
    pub fn rail_axis(&self) -> f64 {
        GAUGE / 2.0 + self.rail.dimensions().head_width / 2.0
    }
}

/// The Planum of the DB Regeloberbau below the top of rail \[m\] — what the
/// terrain pulls the ground down to beside the track when nothing more
/// specific is known. Kept as a constant so `content::terrain` does not have
/// to carry an [`Oberbau`] into every height query.
pub const REGEL_PLANUM: f64 = 0.696;

#[cfg(test)]
mod tests {
    use super::*;

    /// The rolled sections, as the drawings give them.
    #[test]
    fn rail_sections_match_the_rolled_profiles() {
        assert_eq!(RailProfile::R49.section(), (0.149, 0.067, 0.125));
        assert_eq!(RailProfile::R54.section(), (0.154, 0.067, 0.125));
        assert_eq!(RailProfile::R60.section(), (0.172, 0.072, 0.150));
    }

    /// Shoelace area of the contour.
    fn area(points: &[RailPoint]) -> f64 {
        let mut sum = 0.0;
        for i in 0..points.len() {
            let a = points[i];
            let b = points[(i + 1) % points.len()];
            sum += a.across * b.down - b.across * a.down;
        }
        sum.abs() / 2.0
    }

    /// The strongest check there is on a rolled section: the area of the
    /// polygon, times the density of rail steel, is the kilograms per metre
    /// the profile is named after. Getting the envelope right by eye and the
    /// mass wrong by ten per cent means the section is wrong.
    #[test]
    fn the_section_weighs_what_the_profile_is_called() {
        for profile in [RailProfile::R49, RailProfile::R54, RailProfile::R60] {
            let d = profile.dimensions();
            let mass = area(&profile.contour(12)) * RAIL_STEEL_DENSITY;
            let error = (mass - d.mass).abs() / d.mass;
            assert!(
                error < 0.005,
                "{profile:?}: section weighs {mass:.2} kg/m, profile is {:.2}",
                d.mass
            );
        }
    }

    /// The head is widest exactly in the gauge measuring plane — that is what
    /// makes "head width" and "gauge measured 14 mm down" one statement, and
    /// the head arcs are solved from it.
    #[test]
    fn the_head_is_widest_in_the_gauge_plane() {
        for profile in [RailProfile::R49, RailProfile::R54, RailProfile::R60] {
            let d = profile.dimensions();
            let head = profile.contour(24);
            let widest = head
                .iter()
                .filter(|p| p.down <= d.head_side_depth)
                .max_by(|a, b| a.across.total_cmp(&b.across))
                .expect("head points");
            assert!(
                (widest.across - d.head_width / 2.0).abs() < 1e-6,
                "{profile:?}: head {} mm wide",
                widest.across * 2000.0
            );
            assert!(
                (widest.down - GAUGE_MEASURE).abs() < 1e-6,
                "{profile:?}: widest at {} mm",
                widest.down * 1000.0
            );
        }
    }

    /// The crown is the real R 300: across a 72 mm head it drops about
    /// 2.2 mm, and that curvature is the whole reason a rail head carries a
    /// moving streak of sun instead of a flat band.
    #[test]
    fn the_running_surface_is_crowned() {
        let head = RailProfile::R60.contour(24);
        let crown = head
            .iter()
            .filter(|p| p.across.abs() < 0.020 && p.down < 0.005)
            .collect::<Vec<_>>();
        assert!(crown.len() > 4, "crown barely tessellated");
        for p in &crown {
            // Depth of the R 300 arc at this offset.
            let want = HEAD_CROWN_RADIUS
                - (HEAD_CROWN_RADIUS * HEAD_CROWN_RADIUS - p.across * p.across).sqrt();
            assert!(
                (p.down - want).abs() < 1e-6,
                "crown off the R 300 arc at {} mm",
                p.across * 1000.0
            );
        }
        let edge = head
            .iter()
            .filter(|p| p.down <= GAUGE_MEASURE)
            .map(|p| p.down)
            .fold(0.0f64, f64::max);
        assert!(
            (edge - GAUGE_MEASURE).abs() < 1e-9,
            "head top drops {} mm",
            edge * 1000.0
        );
    }

    /// The contour is closed, ordered, and stays inside the profile's own
    /// envelope — a point outside it would poke through the sleeper.
    #[test]
    fn the_contour_stays_in_the_envelope() {
        for profile in [RailProfile::R49, RailProfile::R54, RailProfile::R60] {
            let d = profile.dimensions();
            let points = profile.contour(8);
            assert!(points.len() > 24, "{profile:?}: {} points", points.len());
            for p in &points {
                assert!(p.down >= -1e-9 && p.down <= d.height + 1e-9, "{p:?}");
                assert!(p.across.abs() <= d.foot_width / 2.0 + 1e-9, "{p:?}");
                assert!((0.0..=1.0).contains(&p.polish), "{p:?}");
                assert!((0.0..=1.0).contains(&p.flank), "{p:?}");
            }
            // Starts on the crown centre and comes back to it.
            assert_eq!(points[0].across, 0.0);
            assert_eq!(points[0].down, 0.0);
            assert!(points.last().expect("closed").across < 0.0);
        }
    }

    /// The wheels polish the middle of the crown and nothing below the head:
    /// what the shader paints as steel and what it paints as rust.
    #[test]
    fn only_the_running_band_is_polished() {
        let points = RailProfile::R60.contour(24);
        let on_band = points
            .iter()
            .filter(|p| p.across.abs() < 0.015 && p.down < 0.002);
        for p in on_band {
            assert!(p.polish > 0.95, "running band not polished: {p:?}");
        }
        for p in points.iter().filter(|p| p.down > 0.020) {
            assert_eq!(p.polish, 0.0, "rust polished: {p:?}");
        }
        // The head's side faces are flagged, the web and the foot are not.
        assert!(
            points
                .iter()
                .any(|p| p.flank > 0.5 && p.down > GAUGE_MEASURE),
            "no head flank"
        );
        for p in points.iter().filter(|p| p.down > 0.060) {
            assert_eq!(p.flank, 0.0, "web or foot flagged as head flank: {p:?}");
        }
    }

    /// The build stacks up to the Regelquerschnitt: 182 mm to the sleeper
    /// top, 396 mm to its underside, 696 mm to the Planum.
    #[test]
    fn the_regeloberbau_stacks_up() {
        let ob = Oberbau::default();
        assert!((ob.sleeper_top() - 0.182).abs() < 1e-9);
        assert!((ob.sleeper_base() - 0.396).abs() < 1e-9);
        assert!((ob.planum() - REGEL_PLANUM).abs() < 1e-9);
        // The rail axis: half the gauge plus half a head width.
        assert!((ob.rail_axis() - (1.435 / 2.0 + 0.036)).abs() < 1e-9);
    }
}
