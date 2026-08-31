//! The ballast bed and the slab of the Feste Fahrbahn.
//!
//! The bed is the DB Regelquerschnitt (Ril 800.0130), and the one thing about
//! it that matters most for the look is where its top is: **level with the
//! sleepers, not under them**. A bed whose surface lies at the sleeper
//! underside leaves the sleepers standing on a plate like rungs on a ladder,
//! which is exactly what track built that way reads as. Real ballast is
//! packed into the cribs up to the sleeper top and heaped out to a shoulder
//! at the same height; a sleeper sits *in* the bed with a few centimetres
//! showing.
//!
//! Across the track, from the centre outwards:
//!
//! ```text
//! ├── sleeper_length/2 ──┼─ shoulder ─┤        crib level, 3 cm under the sleeper top
//!                                      \
//!                                       \  1:1.5
//!                                        \
//!                                         └──  Planum, 696 mm under the top of rail
//! ```

use glam::DVec3;
use track_model::{Oberbau, SleeperKind, TrackEdge};
use world_coords::EnuFrame;

use super::mesh::{SectionBuilder, to_render, wobble};
use super::{SAMPLE_BED, cross_section};

/// Where the bed's crest and its toe sit, laterally and in depth below the
/// top of rail: `(laterals, depths)`, left to right.
///
/// Six columns, not four: the shoulder is flat out to the ballast edge and
/// only then falls away, and the break between the two is where the light
/// catches a bed. The outer columns lie on the Planum, which is what the
/// terrain pulls the ground down to beside the track.
pub(super) fn bed_section(ob: &Oberbau) -> ([f64; 6], [f64; 6]) {
    let crest = ob.sleeper_top() + ob.crib_drop;
    let planum = ob.planum();
    let crest_half = ob.sleeper_length / 2.0 + ob.ballast_overhang;
    // The shoulder falls 1:1.5 — steeper and the stones roll off.
    let toe_half = crest_half + (planum - crest) * ob.ballast_slope;
    // A hand's width of Planum outside the toe, so the bed ends on flat
    // ground rather than in a crease with the terrain.
    let edge_half = toe_half + 0.35;
    (
        [
            -edge_half,
            -toe_half,
            -crest_half,
            crest_half,
            toe_half,
            edge_half,
        ],
        [-planum, -planum, -crest, -crest, -planum, -planum],
    )
}

/// The ballast bed between `s0` and `s1`.
///
/// The crest carries a slow wobble of a couple of centimetres — a tamped bed
/// is level to the tamping machine, not to the millimetre, and a perfectly
/// straight ballast edge running to the horizon is the single clearest tell
/// that a track was extruded. It is hashed from the arc length, so every
/// machine builds the same bed (plan ch. 20).
pub(super) fn build(
    e: &TrackEdge,
    frame: &EnuFrame,
    s0: f64,
    s1: f64,
    ob: &Oberbau,
    scale: f64,
) -> bevy::prelude::Mesh {
    let (laterals, depths) = bed_section(ob);
    let steps = (((s1 - s0) / SAMPLE_BED).ceil() as usize).max(1);
    let mut strip = SectionBuilder::default();
    for i in 0..=steps {
        let s = s0 + (s1 - s0) * i as f64 / steps as f64;
        let (center, right, _, up) = cross_section(e, frame, s);
        // Two wavelengths: the long one is the bed settling over tens of
        // metres, the short one the tamping passes.
        let lift = 0.022 * wobble(s, 19.0, 0x5B) + 0.011 * wobble(s, 5.5, 0xA3);
        let mut positions = Vec::with_capacity(laterals.len());
        let mut uvs = Vec::with_capacity(laterals.len());
        // `u` follows the true distance across the section, not its lateral
        // projection: on the 1:1.5 shoulder the two differ by a fifth, and a
        // texture that ignores it is visibly squeezed down the slope.
        let mut across = 0.0;
        for (col, (&l, &y)) in laterals.iter().zip(depths.iter()).enumerate() {
            // The crest and the shoulder edge move with the wobble; the toe
            // on the Planum stays where the terrain put it.
            let on_crest = (2..=3).contains(&col);
            let height = y + if on_crest { lift } else { 0.0 };
            if col > 0 {
                let (dl, dy) = (l - laterals[col - 1], height - depths[col - 1]);
                across += dl.hypot(dy);
            }
            positions.push(to_render(center + right * l + up * height));
            uvs.push([(across / scale) as f32, (s / scale) as f32]);
        }
        strip.push_row(positions, uvs);
    }
    strip.build()
}

/// The Feste Fahrbahn between `s0` and `s1`: a slab of the type's width and
/// thickness, its surface just under the rail fastenings, sides straight down
/// into the formation. No wobble — a cast slab is level, and that is half of
/// what makes slab track look like slab track next to a ballasted bed.
pub(super) fn build_slab(
    e: &TrackEdge,
    frame: &EnuFrame,
    s0: f64,
    s1: f64,
    ob: &Oberbau,
    scale: f64,
) -> bevy::prelude::Mesh {
    debug_assert_eq!(ob.sleeper, SleeperKind::Slab);
    let top = ob.sleeper_top();
    let bottom = top + ob.sleeper_height;
    let half = ob.sleeper_length / 2.0;
    let laterals = [-half, -half, half, half];
    let depths = [-bottom, -top, -top, -bottom];

    let steps = (((s1 - s0) / SAMPLE_BED).ceil() as usize).max(1);
    let mut strip = SectionBuilder::default();
    for i in 0..=steps {
        let s = s0 + (s1 - s0) * i as f64 / steps as f64;
        let (center, right, _, up) = cross_section(e, frame, s);
        let mut positions = Vec::with_capacity(laterals.len());
        let mut uvs = Vec::with_capacity(laterals.len());
        // `u` follows the section round the corner onto the slab's sides,
        // which are a vertical drop at the same lateral — projecting instead
        // would smear one column of texels down the whole face.
        let mut across = 0.0;
        for (col, (&l, &y)) in laterals.iter().zip(depths.iter()).enumerate() {
            if col > 0 {
                across += (l - laterals[col - 1]).hypot(y - depths[col - 1]);
            }
            positions.push(to_render(center + right * l + up * y));
            uvs.push([(across / scale) as f32, (s / scale) as f32]);
        }
        strip.push_row(positions, uvs);
    }
    strip.build()
}

/// ENU position of the cross-section midway between `s0` and `s1` — the point
/// a chunk of bed or sleepers is hung on.
pub(super) fn mid_section(e: &TrackEdge, frame: &EnuFrame, s0: f64, s1: f64) -> DVec3 {
    cross_section(e, frame, (s0 + s1) / 2.0).0
}
