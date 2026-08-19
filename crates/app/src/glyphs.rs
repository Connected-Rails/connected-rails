//! The pictograms and instrument faces of the HUD — drawn here, not fetched.
//!
//! # Why they are generated
//!
//! The ground textures are generated (`world-render`), the sounds are synthesised
//! (`sim-core::synth`), and the binary ships without an asset directory beside it. The
//! HUD's graphics follow the same rule: every mark on it is a few lines of geometry in
//! this file, rasterised into an [`Image`] when the run starts. That keeps the licence
//! situation trivial — the drawings are the project's own — and it means an icon is
//! *edited* rather than *replaced*: a pantograph that should read better at 16 px is a
//! coordinate here, not a trip to an icon set.
//!
//! An icon set would also have been the wrong shape. What this HUD needs is a pantograph,
//! a Federspeicher, a sanding funnel and a Doppelmanometer — a general-purpose icon
//! library has none of those, and filling the gaps with a generic "gear" and "power"
//! symbol is exactly what makes an interface look like it was assembled rather than
//! designed.
//!
//! # How they are drawn
//!
//! One small signed-distance rasteriser. Every primitive is a function that answers "how
//! far is this point from the shape", the canvas turns that distance into coverage over
//! one texel — which is the anti-aliasing — and takes the maximum over the shapes drawn
//! so far. Coordinates are in a unit square, `(0,0)` top left, so a drawing is
//! independent of the size it is rasterised at.
//!
//! Everything comes out white on transparent and is tinted where it is used
//! ([`ImageNode::color`]), so one drawing serves a lamp that is lit and one that is not.

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// A dial's sweep: 0 sits at seven o'clock, full scale at five, as on a cab instrument.
/// Angles are clockwise from twelve o'clock, which is the direction a needle turns.
pub const DIAL_START: f32 = -2.356_194_5; // −135°
pub const DIAL_SWEEP: f32 = 4.712_389; // 270°

/// Where a fraction of full scale sits on the dial [rad, clockwise from twelve].
pub fn dial_angle(fraction: f32) -> f32 {
    DIAL_START + fraction.clamp(0.0, 1.0) * DIAL_SWEEP
}

/// The point a fraction of full scale sits at, in the unit square, at radius `r`.
pub fn dial_point(fraction: f32, r: f32) -> Vec2 {
    let a = dial_angle(fraction);
    Vec2::new(0.5 + r * a.sin(), 0.5 - r * a.cos())
}

// ---------------------------------------------------------------------------------
// The rasteriser
// ---------------------------------------------------------------------------------

/// A square drawing surface in unit coordinates.
struct Canvas {
    size: u32,
    cover: Vec<f32>,
}

impl Canvas {
    fn new(size: u32) -> Self {
        Self {
            size,
            cover: vec![0.0; (size * size) as usize],
        }
    }

    /// Adds everything the distance function reports as inside (`d <= 0`), with one
    /// texel of anti-aliasing across the edge. Shapes union, they never cut each other —
    /// a drawing that needs a hole draws the outline instead of subtracting a disc, which
    /// is what an engraved mark is anyway.
    fn paint(&mut self, sdf: impl Fn(Vec2) -> f32) {
        let n = self.size as f32;
        for y in 0..self.size {
            for x in 0..self.size {
                let p = Vec2::new((x as f32 + 0.5) / n, (y as f32 + 0.5) / n);
                let a = (0.5 - sdf(p) * n).clamp(0.0, 1.0);
                let i = (y * self.size + x) as usize;
                self.cover[i] = self.cover[i].max(a);
            }
        }
    }

    /// A stroked line with round caps.
    fn line(&mut self, a: Vec2, b: Vec2, width: f32) {
        self.paint(|p| sd_segment(p, a, b) - width * 0.5);
    }

    /// A polyline through the points.
    fn path(&mut self, points: &[Vec2], width: f32) {
        for pair in points.windows(2) {
            self.line(pair[0], pair[1], width);
        }
    }

    /// A ring of `width`, or a filled disc where `width` is zero.
    fn circle(&mut self, centre: Vec2, radius: f32, width: f32) {
        if width <= 0.0 {
            self.paint(|p| p.distance(centre) - radius);
        } else {
            self.paint(|p| (p.distance(centre) - radius).abs() - width * 0.5);
        }
    }

    /// A rounded rectangle, outlined or filled.
    fn rect(&mut self, min: Vec2, max: Vec2, round: f32, width: f32) {
        let centre = (min + max) * 0.5;
        let half = (max - min) * 0.5 - Vec2::splat(round);
        let box_sd = move |p: Vec2| {
            let d = (p - centre).abs() - half;
            d.max(Vec2::ZERO).length() + d.x.max(d.y).min(0.0) - round
        };
        if width <= 0.0 {
            self.paint(box_sd);
        } else {
            self.paint(move |p| box_sd(p).abs() - width * 0.5);
        }
    }

    /// A filled triangle.
    fn triangle(&mut self, a: Vec2, b: Vec2, c: Vec2) {
        self.paint(|p| sd_triangle(p, a, b, c));
    }

    /// White where it was drawn, transparent where it was not — the tint happens in the
    /// UI, so one drawing serves every state a lamp can be in.
    fn into_image(self) -> Image {
        let mut data = Vec::with_capacity(self.cover.len() * 4);
        for a in &self.cover {
            let a = (a.clamp(0.0, 1.0) * 255.0) as u8;
            data.extend_from_slice(&[255, 255, 255, a]);
        }
        let mut image = Image::new(
            Extent3d {
                width: self.size,
                height: self.size,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            data,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        );
        // Drawn at twice the size it is shown at; without linear filtering the downscale
        // would sparkle along every diagonal.
        image.sampler = ImageSampler::linear();
        image
    }
}

fn sd_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = (pa.dot(ba) / ba.length_squared()).clamp(0.0, 1.0);
    (pa - ba * h).length()
}

/// Inigo Quilez's triangle distance: negative inside, positive outside.
fn sd_triangle(p: Vec2, a: Vec2, b: Vec2, c: Vec2) -> f32 {
    let (e0, e1, e2) = (b - a, c - b, a - c);
    let (v0, v1, v2) = (p - a, p - b, p - c);
    let edge = |e: Vec2, v: Vec2| v - e * (v.dot(e) / e.length_squared()).clamp(0.0, 1.0);
    let (p0, p1, p2) = (edge(e0, v0), edge(e1, v1), edge(e2, v2));
    let s = (e0.x * e2.y - e0.y * e2.x).signum();
    let d = Vec2::new(p0.length_squared(), s * (v0.x * e0.y - v0.y * e0.x))
        .min(Vec2::new(
            p1.length_squared(),
            s * (v1.x * e1.y - v1.y * e1.x),
        ))
        .min(Vec2::new(
            p2.length_squared(),
            s * (v2.x * e2.y - v2.y * e2.x),
        ));
    -d.x.sqrt() * d.y.signum()
}

// ---------------------------------------------------------------------------------
// Instruments
// ---------------------------------------------------------------------------------

/// The face of a round instrument: the bezel ring, and the tick marks of the scale.
///
/// `majors` is the number of labelled intervals — a 0…10 bar manometer has ten — and
/// `minors` how many unlabelled ticks fall between two of them.
pub fn dial_face(size: u32, majors: u32, minors: u32) -> Image {
    let mut canvas = Canvas::new(size);
    canvas.circle(Vec2::splat(0.5), 0.470, 0.014);
    let steps = majors * minors.max(1);
    for i in 0..=steps {
        let f = i as f32 / steps as f32;
        let major = i % minors.max(1) == 0;
        let (len, width) = if major {
            (0.085, 0.022)
        } else {
            (0.048, 0.012)
        };
        let outer = dial_point(f, 0.440);
        let inner = dial_point(f, 0.440 - len);
        canvas.line(outer, inner, width);
    }
    canvas.into_image()
}

/// The needle of an instrument, pointing at twelve o'clock so that a rotation of the node
/// is the reading. `taper` is the half width at the hub; the tip is always a point.
pub fn needle(size: u32, taper: f32, length: f32) -> Image {
    let mut canvas = Canvas::new(size);
    let hub = Vec2::splat(0.5);
    canvas.triangle(
        hub + Vec2::new(-taper, 0.06),
        hub + Vec2::new(taper, 0.06),
        Vec2::new(0.5, 0.5 - length),
    );
    // The counterweight behind the spindle — what stops a needle from looking like an
    // arrow that happens to sit on a circle.
    canvas.triangle(
        hub + Vec2::new(-taper * 0.8, 0.0),
        hub + Vec2::new(taper * 0.8, 0.0),
        hub + Vec2::new(0.0, 0.11),
    );
    canvas.into_image()
}

/// A short radial marker at the rim, for the speed the line permits and the one the train
/// protection supervises. Points at twelve like the needle and is placed by rotation.
pub fn marker(size: u32) -> Image {
    let mut canvas = Canvas::new(size);
    canvas.triangle(
        Vec2::new(0.5 - 0.030, 0.5 - 0.395),
        Vec2::new(0.5 + 0.030, 0.5 - 0.395),
        Vec2::new(0.5, 0.5 - 0.310),
    );
    canvas.line(
        Vec2::new(0.5, 0.5 - 0.400),
        Vec2::new(0.5, 0.5 - 0.470),
        0.030,
    );
    canvas.into_image()
}

/// The Lf 7 board of the line: the triangle a temporary speed restriction is signed with.
/// Outlined rather than filled, so the figure inside it stays the brightest thing.
pub fn speed_board(size: u32) -> Image {
    let mut canvas = Canvas::new(size);
    let (a, b, c) = (
        Vec2::new(0.5, 0.055),
        Vec2::new(0.955, 0.875),
        Vec2::new(0.045, 0.875),
    );
    canvas.path(&[a, b, c, a], 0.075);
    canvas.into_image()
}

/// Hp 0: the disc that means stop. Filled — a stop signal is not an outline.
pub fn stop_disc(size: u32) -> Image {
    let mut canvas = Canvas::new(size);
    canvas.circle(Vec2::splat(0.5), 0.42, 0.0);
    canvas.into_image()
}

/// Where the train stands on the line diagram of the timetable: a wedge on the rail,
/// pointing the way the run goes. Small enough that the stop it points at stays the
/// brightest thing on the ribbon.
pub fn here(size: u32) -> Image {
    let mut canvas = Canvas::new(size);
    canvas.triangle(
        Vec2::new(0.06, 0.16),
        Vec2::new(0.94, 0.16),
        Vec2::new(0.50, 0.90),
    );
    canvas.into_image()
}

/// The glass of an indicator lamp: a filled disc with a hairline rim, so a lamp reads as
/// a lamp whether it is lit or dark.
pub fn lamp_glass(size: u32) -> Image {
    let mut canvas = Canvas::new(size);
    canvas.circle(Vec2::splat(0.5), 0.40, 0.0);
    canvas.into_image()
}

// ---------------------------------------------------------------------------------
// The pictograms of the desk
// ---------------------------------------------------------------------------------

/// Which drawing an annunciator shows. The order is the order they are generated in;
/// `Icon::ALL` is what the HUD walks to build its texture table.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Battery,
    Pantograph,
    MainSwitch,
    Compressor,
    Parking,
    Sanding,
    Doors,
    Lights,
    Slip,
    Heat,
}

impl Icon {
    pub const ALL: [Icon; 10] = [
        Icon::Battery,
        Icon::Pantograph,
        Icon::MainSwitch,
        Icon::Compressor,
        Icon::Parking,
        Icon::Sanding,
        Icon::Doors,
        Icon::Lights,
        Icon::Slip,
        Icon::Heat,
    ];
}

/// Draws one pictogram. Stroke widths are held near 0.08 of the square throughout, so
/// the ten of them look like one set rather than ten drawings.
pub fn icon(which: Icon, size: u32) -> Image {
    let mut c = Canvas::new(size);
    match which {
        // A cell with its terminal, filled to two thirds.
        Icon::Battery => {
            c.rect(Vec2::new(0.05, 0.26), Vec2::new(0.81, 0.74), 0.06, 0.07);
            c.rect(Vec2::new(0.83, 0.40), Vec2::new(0.96, 0.60), 0.03, 0.0);
            c.rect(Vec2::new(0.14, 0.36), Vec2::new(0.52, 0.64), 0.02, 0.0);
        }
        // A pantograph seen from the side: the frame it stands on, the scissor arms,
        // and the bow that touches the wire. Drawn as a scissor rather than a single arm
        // — one arm reads as a Z at this size, two read as a pantograph.
        Icon::Pantograph => {
            c.line(Vec2::new(0.10, 0.90), Vec2::new(0.90, 0.90), 0.07);
            c.line(Vec2::new(0.21, 0.90), Vec2::new(0.50, 0.44), 0.07);
            c.line(Vec2::new(0.79, 0.90), Vec2::new(0.50, 0.44), 0.07);
            c.line(Vec2::new(0.50, 0.44), Vec2::new(0.50, 0.28), 0.06);
            c.line(Vec2::new(0.15, 0.22), Vec2::new(0.85, 0.22), 0.085);
        }
        // The open contact of a circuit breaker, hung between its two terminals.
        Icon::MainSwitch => {
            c.circle(Vec2::new(0.5, 0.20), 0.075, 0.0);
            c.circle(Vec2::new(0.5, 0.80), 0.075, 0.0);
            c.line(Vec2::new(0.5, 0.06), Vec2::new(0.5, 0.20), 0.07);
            c.line(Vec2::new(0.5, 0.80), Vec2::new(0.5, 0.94), 0.07);
            c.line(Vec2::new(0.5, 0.20), Vec2::new(0.86, 0.72), 0.08);
        }
        // Pump and receiver: the cylinder beside the wheel that drives it.
        Icon::Compressor => {
            c.circle(Vec2::new(0.31, 0.60), 0.235, 0.075);
            c.rect(Vec2::new(0.60, 0.14), Vec2::new(0.94, 0.56), 0.05, 0.075);
            c.line(Vec2::new(0.31, 0.60), Vec2::new(0.63, 0.44), 0.06);
            c.line(Vec2::new(0.66, 0.06), Vec2::new(0.88, 0.06), 0.06);
        }
        // Federspeicher: the spring on the left, the block it presses onto the wheel.
        Icon::Parking => {
            c.circle(Vec2::new(0.66, 0.52), 0.255, 0.075);
            c.rect(Vec2::new(0.28, 0.34), Vec2::new(0.38, 0.70), 0.02, 0.0);
            c.path(
                &[
                    Vec2::new(0.28, 0.52),
                    Vec2::new(0.21, 0.30),
                    Vec2::new(0.14, 0.74),
                    Vec2::new(0.07, 0.38),
                    Vec2::new(0.03, 0.52),
                ],
                0.065,
            );
        }
        // The funnel over the rail, and what comes out of it.
        Icon::Sanding => {
            c.path(
                &[
                    Vec2::new(0.14, 0.08),
                    Vec2::new(0.86, 0.08),
                    Vec2::new(0.57, 0.50),
                    Vec2::new(0.43, 0.50),
                    Vec2::new(0.14, 0.08),
                ],
                0.075,
            );
            c.circle(Vec2::new(0.50, 0.66), 0.06, 0.0);
            c.circle(Vec2::new(0.37, 0.85), 0.055, 0.0);
            c.circle(Vec2::new(0.64, 0.88), 0.055, 0.0);
        }
        // Two leaves and the gap between them, with the handles that open them.
        Icon::Doors => {
            c.rect(Vec2::new(0.06, 0.10), Vec2::new(0.44, 0.90), 0.04, 0.075);
            c.rect(Vec2::new(0.56, 0.10), Vec2::new(0.94, 0.90), 0.04, 0.075);
            c.circle(Vec2::new(0.34, 0.50), 0.05, 0.0);
            c.circle(Vec2::new(0.66, 0.50), 0.05, 0.0);
        }
        // A lamp throwing three beams — the Spitzensignal, not a light bulb.
        Icon::Lights => {
            c.circle(Vec2::new(0.28, 0.50), 0.215, 0.0);
            c.line(Vec2::new(0.60, 0.50), Vec2::new(0.95, 0.50), 0.08);
            c.line(Vec2::new(0.56, 0.28), Vec2::new(0.87, 0.12), 0.08);
            c.line(Vec2::new(0.56, 0.72), Vec2::new(0.87, 0.88), 0.08);
        }
        // A wheel turning against a rail it has lost, with the slip lines behind it.
        Icon::Slip => {
            c.circle(Vec2::new(0.60, 0.44), 0.28, 0.08);
            c.circle(Vec2::new(0.60, 0.44), 0.065, 0.0);
            c.line(Vec2::new(0.02, 0.28), Vec2::new(0.22, 0.28), 0.065);
            c.line(Vec2::new(0.02, 0.60), Vec2::new(0.22, 0.60), 0.065);
            c.line(Vec2::new(0.16, 0.88), Vec2::new(0.98, 0.88), 0.07);
        }
        // A thermometer, with the two marks that make it one.
        Icon::Heat => {
            c.circle(Vec2::new(0.40, 0.78), 0.175, 0.0);
            c.rect(Vec2::new(0.32, 0.06), Vec2::new(0.48, 0.78), 0.08, 0.065);
            c.line(Vec2::new(0.58, 0.26), Vec2::new(0.80, 0.26), 0.06);
            c.line(Vec2::new(0.58, 0.46), Vec2::new(0.80, 0.46), 0.06);
        }
    }
    c.into_image()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rasteriser has to put ink where the shape is and leave the rest alone — a
    /// drawing that came out empty (or solid) would only show up as a blank HUD.
    #[test]
    fn shapes_are_drawn_where_they_belong() {
        let mut canvas = Canvas::new(32);
        canvas.circle(Vec2::splat(0.5), 0.3, 0.0);
        let at = |x: u32, y: u32| canvas.cover[(y * 32 + x) as usize];
        assert!(at(16, 16) > 0.99, "the centre of a filled disc is opaque");
        assert!(at(1, 1) < 0.01, "the corner outside it is clear");

        // Every drawing covers something, and none of them floods its square. A glyph
        // that came out empty would show up as a HUD with a hole in it and nothing else.
        let ink = |image: &Image| {
            image
                .data
                .as_ref()
                .expect("the drawing carries its pixels")
                .chunks(4)
                .map(|p| p[3] as f32 / 255.0)
                .sum::<f32>()
                / (image.width() * image.height()) as f32
        };
        for which in Icon::ALL {
            let covered = ink(&icon(which, 48));
            assert!(
                (0.04..0.72).contains(&covered),
                "{covered} of the icon is ink"
            );
        }
        for (name, image) in [
            ("dial face", dial_face(96, 10, 2)),
            ("needle", needle(96, 0.02, 0.34)),
            ("marker", marker(96)),
            ("Lf 7 board", speed_board(96)),
            ("Hp 0 disc", stop_disc(96)),
            ("position wedge", here(64)),
            ("lamp glass", lamp_glass(64)),
        ] {
            let covered = ink(&image);
            assert!((0.005..0.80).contains(&covered), "{name}: {covered} is ink");
        }
    }

    #[test]
    fn the_dial_runs_from_seven_oclock_to_five() {
        assert!(dial_point(0.0, 0.4).x < 0.5, "zero sits to the left");
        assert!(dial_point(0.0, 0.4).y > 0.5, "and low");
        assert!(dial_point(1.0, 0.4).x > 0.5, "full scale sits to the right");
        assert!(
            (dial_point(0.5, 0.4).x - 0.5).abs() < 1e-6,
            "half is up top"
        );
        assert!(dial_point(0.5, 0.4).y < 0.5);
    }
}
