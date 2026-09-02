//! What kind of tree a crown is, read off the crown itself.
//!
//! A model that finds tree crowns says *tree*. Every published one does: the
//! aerial sets they are trained on are labelled with one class, because
//! drawing a box round a crown is a job a student can do from the photograph
//! and naming the species is not. But a module built out of one class is a
//! module planted with one kind of tree, and a spruce slope done in lime is
//! the first thing anybody notices about a wood.
//!
//! So where the model cannot say, the pixels are asked. Two things about a
//! crown separate a fir from a lime, and both are **ratios**, which is what
//! makes them worth anything: the imagery of one province is a stop brighter
//! than the next, and a rule written in absolute brightness would hold on one
//! provider and be nonsense on the other.
//!
//! * **Contrast.** A conifer is a cone. It is lit on one side and shadowed on
//!   the other, and between the two there is a hard edge — inside one crown the
//!   brightness varies enormously. A broadleaf is a dome of small leaves that
//!   scatters light in every direction, and reads far flatter.
//! * **Warmth.** Needles are blue-green and dark. Broadleaf foliage is
//!   yellow-green, and in autumn frankly orange. The red channel against the
//!   blue says which, and says it without knowing how bright the day was.
//!
//! **It is a hint and not a finding, and the measurement says so.** Over a
//! mixed wood in the Sauerland on the imagery a provider actually gives —
//! 19 cm a pixel, enlarged to the 5 cm the model reads at — the two
//! populations overlap badly:
//!
//! | | contrast | warmth |
//! | --- | --- | --- |
//! | broadleaf crowns | median 0.10, p90 0.19 | median 0.01 |
//! | spruce under 2 m across | median 0.13 | median 0.00 |
//! | spruce of 3 m and more | median 0.22 | median 0.04 |
//!
//! The reason is resolution rather than the idea: a two-metre crown is ten
//! native pixels across, and the shadow that makes a cone a cone is not in
//! ten pixels. Where the crowns are big enough to resolve, the separation is
//! real (0.22 against 0.10); where they are not, this is close to a coin.
//! Warmth barely separates anything on summer imagery and is kept for what it
//! is genuinely good at: vetoing the orange of an autumn crown, which is
//! shadowed like a fir and is not one.
//!
//! So the crate offers it and never insists on it. A class with species of its
//! own ([`crate::ClassSpec::conifer`] left empty) never consults it, and the
//! editor lets a builder overrule the lot by naming the stand to plant from —
//! which is the honest division of labour, because at this resolution a person
//! looking at the photograph can see what the arithmetic cannot.

use crate::detect::Detection;

/// The share of the box that is sampled, as a radius.
///
/// Well inside it. A detector's box is drawn round the *whole* crown and
/// therefore contains its edge, and the edge of a crown is half ground: grass,
/// track ballast, the roof of the shed behind it. A third of the way out is
/// all crown whatever the box did, and a crown is uniform enough that the
/// middle is representative of it.
const CORE: f32 = 0.35;

/// Fewest pixels a verdict is given on. Below it, a crown a handful of pixels
/// across, where the contrast is the resampling and not the tree.
const ENOUGH: usize = 24;

/// Relative brightness spread above which a crown reads as a lit-and-shadowed
/// cone rather than a dome.
///
/// Measured rather than guessed, and the first version of this file guessed
/// 0.25 — which on real imagery put a plantation of spruce at seven per cent
/// conifer, because 0.25 is what a crown reads at when the photograph is sharp
/// enough to hold its shadow. Between the p90 of the broadleaves (0.19) and the
/// median of the spruce big enough to resolve (0.22) there is one number, and
/// this is it. The overlap either side of it is real; see the module head.
const SHADOWED: f64 = 0.20;

/// Warmth above which a crown cannot be needles.
///
/// A veto, not a test: on summer imagery both kinds sit within a few
/// hundredths of nought and this never fires. What it is for is the autumn
/// broadleaf, which is orange, is shadowed exactly like a fir, and would
/// otherwise be planted as one — and the dead spruce of a bark-beetle stand,
/// which is browner still.
const WARM: f64 = 0.10;

/// What one crown looks like, in the two numbers that survive the exposure.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crown {
    /// Standard deviation of the brightness over its mean — how hard the
    /// crown is lit on one side and shadowed on the other.
    pub contrast: f64,
    /// `(R − B)` over the sum of the channels: how far from blue-green towards
    /// yellow and orange the foliage is.
    pub warmth: f64,
    /// Pixels the two were measured on.
    pub samples: usize,
}

impl Crown {
    /// Whether this reads as a needle-leaf tree.
    ///
    /// Both, not either. The contrast is what carries the decision and the
    /// warmth is what stops it: a broadleaf standing alone over a mown field
    /// is high-contrast too, because its own shadow is in the box, and an
    /// autumn crown is high-contrast and orange.
    pub fn conifer(&self) -> bool {
        self.contrast > SHADOWED && self.warmth < WARM
    }
}

/// Reads the crown of `at` out of the window it was found in.
///
/// `pixels` is the RGB8 buffer the detector was given, so the box is in the
/// coordinates the detection already carries and nothing has to be mapped.
/// `None` where the box is too small to say anything about, which is the
/// honest answer for a crown eight pixels across.
pub fn read(pixels: &[u8], width: u32, height: u32, at: &Detection) -> Option<Crown> {
    let radius = (at.w.min(at.h) * CORE).max(1.0);
    let (left, right) = (at.cx - radius, at.cx + radius);
    let (top, bottom) = (at.cy - radius, at.cy + radius);
    let x0 = left.floor().max(0.0) as u32;
    let y0 = top.floor().max(0.0) as u32;
    let x1 = (right.ceil() as i64).clamp(0, width as i64) as u32;
    let y1 = (bottom.ceil() as i64).clamp(0, height as i64) as u32;

    let mut samples = 0usize;
    let mut sum = 0.0;
    let mut squares = 0.0;
    let mut warmth = 0.0;
    for y in y0..y1 {
        for x in x0..x1 {
            // The disc, not its bounding square: the corners of the square
            // are outside the crown by a fifth of its width.
            let (dx, dy) = (x as f32 + 0.5 - at.cx, y as f32 + 0.5 - at.cy);
            if dx * dx + dy * dy > radius * radius {
                continue;
            }
            let at = ((y as usize * width as usize) + x as usize) * 3;
            let (r, g, b) = (
                *pixels.get(at)? as f64,
                *pixels.get(at + 1)? as f64,
                *pixels.get(at + 2)? as f64,
            );
            // Rec. 709, the luminance every display works in.
            let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            sum += luma;
            squares += luma * luma;
            // The +1 is for the black pixel: a hole in the imagery is nought
            // over nought otherwise, and it would come out as warm.
            warmth += (r - b) / (r + g + b + 1.0);
            samples += 1;
        }
    }
    if samples < ENOUGH {
        return None;
    }
    let n = samples as f64;
    let mean = sum / n;
    if mean <= 1.0 {
        // Black. A hole in the coverage, or a shadow so deep there is nothing
        // in it to read — either way not a crown anybody can name.
        return None;
    }
    let variance = (squares / n - mean * mean).max(0.0);
    Some(Crown {
        contrast: variance.sqrt() / mean,
        warmth: warmth / n,
        samples,
    })
}

/// The tag a find of this class gets: the class's own, or its conifer tag
/// where the crown reads as needles.
///
/// The one entry point the walk uses, so the rule that a class with species of
/// its own is never second-guessed lives in one place: an empty
/// [`crate::ClassSpec::conifer`] returns `place` without ever looking at a
/// pixel.
pub fn tag_for(
    class: &crate::ClassSpec,
    pixels: &[u8],
    width: u32,
    height: u32,
    at: &Detection,
) -> String {
    if class.conifer.is_empty() {
        return class.place.clone();
    }
    match read(pixels, width, height, at) {
        Some(crown) if crown.conifer() => class.conifer.clone(),
        _ => class.place.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A window with one crown painted into it: `shade` is how much darker the
    /// shadowed half is, so a cone and a dome differ by that alone.
    fn window(colour: (u8, u8, u8), shade: f64) -> (Vec<u8>, u32) {
        const SIZE: u32 = 64;
        let mut pixels = vec![0u8; (SIZE * SIZE * 3) as usize];
        for y in 0..SIZE {
            for x in 0..SIZE {
                let lit = if x < SIZE / 2 { 1.0 } else { 1.0 - shade };
                let at = ((y * SIZE + x) * 3) as usize;
                pixels[at] = (colour.0 as f64 * lit) as u8;
                pixels[at + 1] = (colour.1 as f64 * lit) as u8;
                pixels[at + 2] = (colour.2 as f64 * lit) as u8;
            }
        }
        (pixels, SIZE)
    }

    fn found(size: f32) -> Detection {
        Detection {
            class: 0,
            score: 0.9,
            cx: 32.0,
            cy: 32.0,
            w: size,
            h: size,
            angle: 0.0,
        }
    }

    #[test]
    fn a_dark_blue_green_cone_reads_as_a_conifer() {
        let (pixels, size) = window((46, 62, 48), 0.55);
        let crown = read(&pixels, size, size, &found(40.0)).unwrap();
        assert!(crown.conifer(), "{crown:?}");
    }

    #[test]
    fn a_flat_yellow_green_dome_reads_as_a_broadleaf() {
        let (pixels, size) = window((86, 116, 54), 0.12);
        let crown = read(&pixels, size, size, &found(40.0)).unwrap();
        assert!(!crown.conifer(), "{crown:?}");
        assert!(crown.warmth > WARM, "yellow-green is warm: {crown:?}");
    }

    /// An oak in autumn is orange and strongly shadowed — it fails the
    /// contrast test and would be a fir on brightness alone. The colour is
    /// what saves it, which is why both tests have to agree.
    #[test]
    fn an_autumn_crown_stays_a_broadleaf_however_shadowed() {
        let (pixels, size) = window((168, 120, 58), 0.6);
        let crown = read(&pixels, size, size, &found(40.0)).unwrap();
        assert!(crown.contrast > SHADOWED, "shadowed: {crown:?}");
        assert!(!crown.conifer(), "but not a fir: {crown:?}");
    }

    /// The claim the whole module rests on: a stop of exposure either way is
    /// the same tree. Both numbers are ratios of the channels against each
    /// other, so neither moves with the brightness of the day.
    ///
    /// Near enough rather than exactly: the guard against a black pixel puts a
    /// one in the denominator of the warmth, which is a hundredth of a percent
    /// at these levels and the price of not dividing by nought in a hole in
    /// the imagery.
    #[test]
    fn a_brighter_photograph_is_the_same_tree() {
        let (pixels, size) = window((40, 54, 42), 0.5);
        let dim = read(&pixels, size, size, &found(40.0)).unwrap();
        let brighter: Vec<u8> = pixels.iter().map(|b| b.saturating_mul(2)).collect();
        let bright = read(&brighter, size, size, &found(40.0)).unwrap();
        assert!((dim.contrast - bright.contrast).abs() < 1e-6, "{dim:?}");
        assert!((dim.warmth - bright.warmth).abs() < 1e-3, "{dim:?}");
        assert_eq!(dim.conifer(), bright.conifer());
    }

    #[test]
    fn a_crown_of_a_handful_of_pixels_is_not_named() {
        let (pixels, size) = window((46, 62, 48), 0.55);
        assert!(read(&pixels, size, size, &found(4.0)).is_none());
        // Nor is a hole in the imagery.
        let black = vec![0u8; (size * size * 3) as usize];
        assert!(read(&black, size, size, &found(40.0)).is_none());
    }

    /// A class that names its own species is never second-guessed, and one
    /// that cannot be read falls back to the broadleaf tag rather than to
    /// nothing.
    #[test]
    fn a_model_that_knows_its_species_is_left_alone() {
        let (pixels, size) = window((46, 62, 48), 0.55);
        let told = crate::ClassSpec::tree("conifer", "nadelbaum", "", (2.0, 22.0));
        assert_eq!(
            tag_for(&told, &pixels, size, size, &found(40.0)),
            "nadelbaum"
        );

        let guessing = crate::ClassSpec::tree("tree", "laubbaum", "nadelbaum", (2.0, 26.0));
        assert_eq!(
            tag_for(&guessing, &pixels, size, size, &found(40.0)),
            "nadelbaum",
            "a dark cone is a fir"
        );
        assert_eq!(
            tag_for(&guessing, &pixels, size, size, &found(4.0)),
            "laubbaum",
            "and a crown too small to read is the class's own tag"
        );
    }
}
