//! What a crop looks like on a given day.
//!
//! The register says "winter wheat". It does not say that on the 3rd of April
//! that is ankle-high and blue-green, on the 20th of June waist-high and still
//! green, on the 25th of July gold and about to be cut, and on the 1st of
//! August a stubble field with straw bales on it. Without that the whole import
//! buys nothing: a line driven in October would show summer everywhere.
//!
//! So each render group gets a handful of key dates with a cover, a height and
//! a colour, and the day of the run is interpolated between them (plan ch. 6).
//! The dates are central German lowlands — the DWD's phenological network
//! publishes the real regional ones, and a line that wants them can override
//! this table; a table filled in an hour gets 95 % of the impression.
//!
//! Everything is a function of `(crop, day of year, seed)` and nothing else, so
//! two clients of a multiplayer run draw the same field the same way without a
//! byte crossing the network.

use crate::crops::CropClass;
use crate::stats::vary;

/// What is happening on the field — for the editor's readout, and for the
/// renderer where a stage means more than its cover and colour (stubble has
/// bales on it, a ploughed field has furrows and nothing growing).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stage {
    /// Ploughed or cultivated, nothing up yet.
    Bare,
    /// Sown; the rows are just visible.
    Emerging,
    /// Growing, the stand closing over.
    Growing,
    /// In flower — rape in May is the one field anybody can name at 160 km/h.
    Flowering,
    /// Turning, from green to gold or brown.
    Ripening,
    /// Standing ripe, waiting for the combine.
    Ripe,
    /// Cut. Stubble, straw, the odd bale.
    Stubble,
}

impl Stage {
    /// The translation key of the name shown in the editor.
    pub fn key(self) -> &'static str {
        match self {
            Stage::Bare => "growth-bare",
            Stage::Emerging => "growth-emerging",
            Stage::Growing => "growth-growing",
            Stage::Flowering => "growth-flowering",
            Stage::Ripening => "growth-ripening",
            Stage::Ripe => "growth-ripe",
            Stage::Stubble => "growth-stubble",
        }
    }
}

/// What the renderer needs to draw one field on one day.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Growth {
    pub stage: Stage,
    /// How much of the ground the crop covers, 0 = bare soil … 1 = closed.
    pub cover: f32,
    /// Height of the stand [m]. Only the permanent crops are taller than a
    /// person, and only maize comes close.
    pub height: f32,
    /// Colour of the stand, sRGB. Bare soil shows through it by `cover`.
    pub color: [f32; 3],
    /// How strongly the working rows read — drilled cereal barely at all once
    /// the stand closes, maize and beet all summer, a ploughed field most of
    /// all.
    pub rows: f32,
}

/// One key date of a crop's year.
struct Key {
    /// Day of the year, 1 … 365.
    day: u16,
    stage: Stage,
    cover: f32,
    height: f32,
    color: [f32; 3],
    rows: f32,
}

const fn key(day: u16, stage: Stage, cover: f32, height: f32, color: [f32; 3], rows: f32) -> Key {
    Key {
        day,
        stage,
        cover,
        height,
        color,
        rows,
    }
}

// The palette. Named so the tables below read as what they are.
const SOIL: [f32; 3] = [0.30, 0.24, 0.18];
const SPROUT: [f32; 3] = [0.36, 0.52, 0.22];
const GREEN: [f32; 3] = [0.28, 0.45, 0.16];
const DEEP_GREEN: [f32; 3] = [0.20, 0.36, 0.13];
const BLUE_GREEN: [f32; 3] = [0.24, 0.40, 0.24];
const GOLD: [f32; 3] = [0.72, 0.62, 0.24];
const RIPE_GOLD: [f32; 3] = [0.80, 0.70, 0.30];
const STUBBLE: [f32; 3] = [0.68, 0.62, 0.38];
const RAPE_YELLOW: [f32; 3] = [0.86, 0.80, 0.16];
const BROWN: [f32; 3] = [0.45, 0.36, 0.22];
const MEADOW: [f32; 3] = [0.32, 0.50, 0.20];
const ROUGH: [f32; 3] = [0.42, 0.46, 0.24];

// The year of each crop, as key dates. The last entry wraps round to the first.
static WINTER_CEREAL: [Key; 12] = [
    key(1, Stage::Growing, 0.55, 0.10, BLUE_GREEN, 0.45),
    key(75, Stage::Growing, 0.65, 0.15, BLUE_GREEN, 0.35),
    key(115, Stage::Growing, 0.85, 0.35, GREEN, 0.20),
    key(150, Stage::Growing, 1.00, 0.80, GREEN, 0.10),
    key(165, Stage::Flowering, 1.00, 0.95, GREEN, 0.08),
    key(185, Stage::Ripening, 1.00, 1.00, GOLD, 0.08),
    key(200, Stage::Ripe, 1.00, 1.00, RIPE_GOLD, 0.10),
    key(210, Stage::Stubble, 0.40, 0.15, STUBBLE, 0.55),
    key(245, Stage::Bare, 0.05, 0.00, SOIL, 0.85),
    key(285, Stage::Bare, 0.02, 0.00, SOIL, 0.95),
    key(300, Stage::Emerging, 0.20, 0.04, SPROUT, 0.75),
    key(330, Stage::Growing, 0.45, 0.08, BLUE_GREEN, 0.55),
];

static SUMMER_CEREAL: [Key; 10] = [
    key(1, Stage::Bare, 0.02, 0.00, SOIL, 0.90),
    key(75, Stage::Bare, 0.03, 0.00, SOIL, 0.95),
    key(95, Stage::Emerging, 0.20, 0.05, SPROUT, 0.80),
    key(130, Stage::Growing, 0.70, 0.30, GREEN, 0.30),
    key(165, Stage::Growing, 1.00, 0.75, GREEN, 0.10),
    key(180, Stage::Flowering, 1.00, 0.85, GREEN, 0.10),
    key(200, Stage::Ripening, 1.00, 0.90, GOLD, 0.10),
    key(215, Stage::Ripe, 1.00, 0.90, RIPE_GOLD, 0.12),
    key(225, Stage::Stubble, 0.40, 0.15, STUBBLE, 0.55),
    key(260, Stage::Bare, 0.05, 0.00, SOIL, 0.85),
];

static MAIZE: [Key; 10] = [
    key(1, Stage::Bare, 0.02, 0.00, SOIL, 0.90),
    key(115, Stage::Bare, 0.03, 0.00, SOIL, 1.00),
    key(140, Stage::Emerging, 0.10, 0.15, SPROUT, 1.00),
    key(165, Stage::Growing, 0.35, 0.70, GREEN, 0.90),
    key(190, Stage::Growing, 0.85, 1.90, DEEP_GREEN, 0.55),
    key(210, Stage::Flowering, 1.00, 2.60, DEEP_GREEN, 0.35),
    key(245, Stage::Ripening, 1.00, 2.60, GREEN, 0.35),
    key(265, Stage::Ripe, 1.00, 2.50, BROWN, 0.40),
    key(280, Stage::Stubble, 0.25, 0.25, STUBBLE, 0.80),
    key(310, Stage::Bare, 0.05, 0.00, SOIL, 0.90),
];

static RAPESEED: [Key; 12] = [
    key(1, Stage::Growing, 0.60, 0.15, DEEP_GREEN, 0.40),
    key(85, Stage::Growing, 0.75, 0.25, DEEP_GREEN, 0.30),
    key(110, Stage::Growing, 0.95, 0.80, GREEN, 0.12),
    key(122, Stage::Flowering, 1.00, 1.30, RAPE_YELLOW, 0.05),
    key(145, Stage::Flowering, 1.00, 1.45, RAPE_YELLOW, 0.05),
    key(160, Stage::Growing, 1.00, 1.50, GREEN, 0.06),
    key(185, Stage::Ripening, 1.00, 1.45, BROWN, 0.08),
    key(200, Stage::Ripe, 1.00, 1.40, BROWN, 0.10),
    key(210, Stage::Stubble, 0.30, 0.20, STUBBLE, 0.60),
    key(230, Stage::Bare, 0.05, 0.00, SOIL, 0.90),
    key(250, Stage::Emerging, 0.25, 0.06, SPROUT, 0.70),
    key(300, Stage::Growing, 0.55, 0.12, DEEP_GREEN, 0.45),
];

static SUGAR_BEET: [Key; 8] = [
    key(1, Stage::Bare, 0.02, 0.00, SOIL, 0.90),
    key(100, Stage::Bare, 0.03, 0.00, SOIL, 1.00),
    key(125, Stage::Emerging, 0.08, 0.05, SPROUT, 1.00),
    key(155, Stage::Growing, 0.45, 0.25, GREEN, 0.85),
    key(180, Stage::Growing, 0.95, 0.45, DEEP_GREEN, 0.40),
    key(250, Stage::Growing, 1.00, 0.50, DEEP_GREEN, 0.35),
    key(285, Stage::Ripe, 0.95, 0.45, GREEN, 0.40),
    key(300, Stage::Bare, 0.05, 0.00, SOIL, 0.95),
];

static POTATO: [Key; 8] = [
    key(1, Stage::Bare, 0.02, 0.00, SOIL, 0.90),
    key(100, Stage::Bare, 0.03, 0.00, SOIL, 1.00),
    key(128, Stage::Emerging, 0.10, 0.08, SPROUT, 1.00),
    key(155, Stage::Growing, 0.55, 0.35, GREEN, 0.85),
    key(175, Stage::Flowering, 0.90, 0.55, GREEN, 0.65),
    key(210, Stage::Growing, 0.90, 0.55, DEEP_GREEN, 0.60),
    key(235, Stage::Ripening, 0.60, 0.40, BROWN, 0.70),
    key(255, Stage::Bare, 0.05, 0.00, SOIL, 0.95),
];

static LEGUME: [Key; 8] = [
    key(1, Stage::Bare, 0.02, 0.00, SOIL, 0.90),
    key(85, Stage::Bare, 0.03, 0.00, SOIL, 0.95),
    key(105, Stage::Emerging, 0.20, 0.08, SPROUT, 0.75),
    key(140, Stage::Growing, 0.80, 0.35, BLUE_GREEN, 0.30),
    key(160, Stage::Flowering, 1.00, 0.60, BLUE_GREEN, 0.15),
    key(195, Stage::Ripening, 0.95, 0.60, BROWN, 0.20),
    key(215, Stage::Stubble, 0.35, 0.15, STUBBLE, 0.55),
    key(250, Stage::Bare, 0.05, 0.00, SOIL, 0.85),
];

static GRASSLAND: [Key; 10] = [
    key(1, Stage::Growing, 0.95, 0.05, MEADOW, 0.0),
    key(100, Stage::Growing, 1.00, 0.15, MEADOW, 0.0),
    key(140, Stage::Growing, 1.00, 0.35, MEADOW, 0.0),
    key(150, Stage::Stubble, 1.00, 0.06, MEADOW, 0.05),
    key(185, Stage::Growing, 1.00, 0.28, MEADOW, 0.0),
    key(195, Stage::Stubble, 1.00, 0.06, MEADOW, 0.05),
    key(230, Stage::Growing, 1.00, 0.24, MEADOW, 0.0),
    key(240, Stage::Stubble, 1.00, 0.06, MEADOW, 0.05),
    key(285, Stage::Growing, 0.98, 0.14, MEADOW, 0.0),
    key(330, Stage::Growing, 0.95, 0.08, MEADOW, 0.0),
];

static VEGETABLE: [Key; 7] = [
    key(1, Stage::Bare, 0.05, 0.00, SOIL, 0.90),
    key(95, Stage::Bare, 0.05, 0.00, SOIL, 1.00),
    key(120, Stage::Emerging, 0.25, 0.08, SPROUT, 1.00),
    key(160, Stage::Growing, 0.70, 0.30, GREEN, 0.85),
    key(240, Stage::Growing, 0.70, 0.30, GREEN, 0.85),
    key(285, Stage::Ripe, 0.45, 0.20, GREEN, 0.90),
    key(305, Stage::Bare, 0.08, 0.00, SOIL, 0.95),
];

static ORCHARD: [Key; 7] = [
    key(1, Stage::Bare, 0.85, 2.60, BROWN, 0.75),
    key(100, Stage::Emerging, 0.88, 2.70, SPROUT, 0.70),
    key(112, Stage::Flowering, 0.95, 2.80, [0.86, 0.82, 0.80], 0.60),
    key(135, Stage::Growing, 1.00, 3.00, GREEN, 0.55),
    key(250, Stage::Ripe, 1.00, 3.00, DEEP_GREEN, 0.55),
    key(290, Stage::Ripening, 0.95, 2.90, GOLD, 0.65),
    key(320, Stage::Bare, 0.85, 2.70, BROWN, 0.75),
];

static VINEYARD: [Key; 6] = [
    key(1, Stage::Bare, 0.55, 1.40, BROWN, 0.95),
    key(110, Stage::Emerging, 0.60, 1.45, SPROUT, 0.92),
    key(150, Stage::Growing, 0.85, 1.70, GREEN, 0.85),
    key(255, Stage::Ripe, 0.85, 1.75, DEEP_GREEN, 0.85),
    key(290, Stage::Ripening, 0.75, 1.70, GOLD, 0.90),
    key(320, Stage::Bare, 0.55, 1.45, BROWN, 0.95),
];

static FALLOW: [Key; 6] = [
    key(1, Stage::Bare, 0.25, 0.06, ROUGH, 0.15),
    key(90, Stage::Emerging, 0.45, 0.10, SPROUT, 0.10),
    key(140, Stage::Growing, 0.85, 0.45, ROUGH, 0.05),
    key(190, Stage::Flowering, 0.95, 0.70, [0.52, 0.52, 0.28], 0.05),
    key(250, Stage::Ripening, 0.90, 0.65, STUBBLE, 0.08),
    key(310, Stage::Bare, 0.55, 0.30, BROWN, 0.12),
];

static OTHER: [Key; 6] = [
    key(1, Stage::Bare, 0.10, 0.00, SOIL, 0.85),
    key(100, Stage::Emerging, 0.30, 0.10, SPROUT, 0.70),
    key(160, Stage::Growing, 0.85, 0.45, GREEN, 0.35),
    key(230, Stage::Ripening, 0.80, 0.45, GOLD, 0.40),
    key(265, Stage::Stubble, 0.30, 0.12, STUBBLE, 0.65),
    key(310, Stage::Bare, 0.10, 0.00, SOIL, 0.85),
];

/// The year of a crop, as key dates. The last entry wraps round to the first.
fn calendar(crop: CropClass) -> &'static [Key] {
    match crop {
        CropClass::WinterCereal => &WINTER_CEREAL,
        CropClass::SummerCereal => &SUMMER_CEREAL,
        CropClass::Maize => &MAIZE,
        CropClass::Rapeseed => &RAPESEED,
        CropClass::SugarBeet => &SUGAR_BEET,
        CropClass::Potato => &POTATO,
        CropClass::Legume => &LEGUME,
        CropClass::Grassland => &GRASSLAND,
        CropClass::Vegetable => &VEGETABLE,
        CropClass::Orchard => &ORCHARD,
        CropClass::Vineyard => &VINEYARD,
        CropClass::Fallow => &FALLOW,
        CropClass::Other => &OTHER,
    }
}

/// Day of the year, 1 … 365, from a calendar date. February has 28 days here:
/// a leap day would move every crop by one, which no eye has ever caught.
pub fn day_of_year(month: u32, day: u32) -> u16 {
    const CUMULATIVE: [u16; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let month = month.clamp(1, 12) as usize - 1;
    (CUMULATIVE[month] + day.clamp(1, 31) as u16).min(365)
}

/// Where a crop is on a given day.
///
/// `seed` shifts the whole year by up to a week either way, per field: two
/// neighbouring wheat fields are never cut on the same afternoon, and a line
/// driven at the end of July shows both stubble and standing corn — which is
/// exactly what the real thing looks like.
pub fn growth(crop: CropClass, month: u32, day: u32, seed: u64) -> Growth {
    growth_on(crop, day_of_year(month, day), seed)
}

/// The day offset a field's seed gives, in days: −7 … 7.
///
/// [`growth`] applies it to the day before it reads the calendar. The offset
/// is published on its own because the renderer needs it twice: the material
/// uniforms and the standing crop both have to see the same shifted year, and
/// the second one reads it back out of the field's own vertex colour rather
/// than out of the seed it does not know.
pub fn offset_of(seed: u64) -> f64 {
    const SPREAD: f64 = 7.0;
    (vary(seed, 0x5EED) * 2.0 - 1.0) * SPREAD
}

/// The same, from a day of the year.
pub fn growth_on(crop: CropClass, day: u16, seed: u64) -> Growth {
    growth_offset(crop, day, offset_of(seed) as f32)
}

/// Growth at a day offset the caller already has — a field whose offset rides
/// in its vertex colour instead of in a seed the renderer never sees. The
/// offset is in days, −7 … 7, as [`offset_of`] gives it.
pub fn growth_offset(crop: CropClass, day: u16, offset_days: f32) -> Growth {
    let day = (day as f64 - offset_days as f64).rem_euclid(365.0) as u16;
    growth_on_day(crop, day)
}

/// The calendar itself, on a plain day of the year.
fn growth_on_day(crop: CropClass, day: u16) -> Growth {
    let day = day as f64;
    let keys = calendar(crop);

    // The span the day falls in. The table is short enough that a scan beats
    // anything cleverer, and it has to wrap round the new year anyway.
    let mut at = keys.len() - 1;
    for (i, k) in keys.iter().enumerate() {
        if (k.day as f64) <= day {
            at = i;
        }
    }
    let next = (at + 1) % keys.len();
    let (a, b) = (&keys[at], &keys[next]);
    let span = wrap(b.day as f64 - a.day as f64 - 1.0) + 1.0;
    let along = if span <= 0.0 {
        0.0
    } else {
        (wrap(day - a.day as f64) / span).clamp(0.0, 1.0)
    } as f32;

    Growth {
        // The stage is the one that has started, not the one being blended
        // towards: a field is stubble the day it is cut.
        stage: a.stage,
        cover: mix(a.cover, b.cover, along),
        height: mix(a.height, b.height, along),
        color: std::array::from_fn(|i| mix(a.color[i], b.color[i], along)),
        rows: mix(a.rows, b.rows, along),
    }
}

/// Folds a day into `0.0 .. 365.0`.
fn wrap(day: f64) -> f64 {
    day.rem_euclid(365.0)
}

fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_year_starts_where_it_should() {
        assert_eq!(day_of_year(1, 1), 1);
        assert_eq!(day_of_year(3, 1), 60);
        assert_eq!(day_of_year(12, 31), 365);
    }

    #[test]
    fn winter_wheat_is_gold_in_july_and_bare_in_september() {
        let july = growth(CropClass::WinterCereal, 7, 15, 1);
        assert_eq!(july.stage, Stage::Ripening);
        assert!(july.color[0] > july.color[2], "{:?}", july.color);
        assert!(july.height > 0.8, "{}", july.height);

        let september = growth(CropClass::WinterCereal, 9, 20, 1);
        assert!(september.cover < 0.2, "{}", september.cover);
        assert_eq!(september.stage, Stage::Bare);
    }

    #[test]
    fn rape_is_yellow_in_may_and_nothing_else_is() {
        // Mid-May: far enough into the flowering span that the week of jitter
        // a field's seed adds cannot carry it out the other side.
        let may = growth(CropClass::Rapeseed, 5, 15, 1);
        assert_eq!(may.stage, Stage::Flowering);
        assert!(may.color[0] > 0.7 && may.color[1] > 0.7 && may.color[2] < 0.3);
        let wheat = growth(CropClass::WinterCereal, 5, 15, 1);
        assert!(wheat.color[2] < 0.3 && wheat.color[0] < 0.5);
    }

    #[test]
    fn maize_is_the_tallest_thing_in_august() {
        let maize = growth(CropClass::Maize, 8, 15, 1);
        assert!(maize.height > 2.0, "{}", maize.height);
        for crop in [
            CropClass::WinterCereal,
            CropClass::SugarBeet,
            CropClass::Grassland,
        ] {
            assert!(growth(crop, 8, 15, 1).height < maize.height);
        }
    }

    #[test]
    fn grassland_is_never_bare_and_never_in_rows() {
        for day in 1..=365u16 {
            let g = growth_on(CropClass::Grassland, day, 1);
            assert!(g.cover > 0.9, "day {day}: {}", g.cover);
            assert!(g.rows < 0.1, "day {day}: {}", g.rows);
            assert_ne!(g.stage, Stage::Bare);
        }
    }

    #[test]
    fn every_crop_is_defined_on_every_day() {
        for crop in CropClass::ALL {
            for day in 1..=365u16 {
                let g = growth_on(crop, day, 42);
                assert!((0.0..=1.0).contains(&g.cover), "{crop:?} {day}");
                assert!((0.0..=1.0).contains(&g.rows), "{crop:?} {day}");
                assert!((0.0..4.0).contains(&g.height), "{crop:?} {day}");
                assert!(
                    g.color.iter().all(|c| (0.0..=1.0).contains(c)),
                    "{crop:?} {day}"
                );
            }
        }
    }

    #[test]
    fn the_year_is_continuous_across_the_new_year() {
        // No jump between 31 December and 1 January — the table wraps.
        for crop in CropClass::ALL {
            let last = growth_on(crop, 365, 0);
            let first = growth_on(crop, 1, 0);
            assert!(
                (last.height - first.height).abs() < 0.15,
                "{crop:?}: {last:?} {first:?}"
            );
            assert!((last.cover - first.cover).abs() < 0.15, "{crop:?}");
        }
    }

    #[test]
    fn neighbouring_fields_are_not_cut_on_the_same_day() {
        // Two fields of the same crop, in the week the combines come out.
        let mut stages = std::collections::HashSet::new();
        for seed in 0..40u64 {
            stages.insert(growth(CropClass::WinterCereal, 7, 28, seed).stage);
        }
        assert!(stages.len() > 1, "every field ripens on the same day");
    }

    #[test]
    fn an_explicit_offset_is_the_seed_its_own() {
        // The standing crop reads a field's offset back out of its vertex
        // colour and lands in `growth_offset`; it must not be able to tell
        // the difference from the seed itself.
        for seed in 0..20u64 {
            let direct = growth_on(CropClass::WinterCereal, 200, seed);
            let via_colour = growth_offset(CropClass::WinterCereal, 200, offset_of(seed) as f32);
            assert_eq!(direct, via_colour, "seed {seed}");
        }
    }
}
