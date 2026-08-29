//! Guessing a crop, well.
//!
//! Half the states publish the field block and no more: this is farmland, it is
//! arable rather than grass, goodbye. Recognising the crop from an aerial photo
//! would be a research project; asking the farmer is not on offer. What is on
//! offer is the statistics — how much of a region's arable land carries wheat,
//! maize, rape, beet — and drawing from that (plan ch. 5).
//!
//! The single field is then wrong about as often as the statistics say it
//! should be, and the *landscape* is right: the right share of wheat, in fields
//! the right size, in the right places. From a train window that is the whole
//! of the effect, and it costs a lookup table rather than a model.
//!
//! The draw is seeded by the parcel's own id, so it is not random in any way
//! the user can see: the same field is the same crop in every import, on every
//! machine, in single player and on the server. Re-importing a line a year later
//! changes the fields the register changed and nothing else.

use crate::crops::CropClass;
use crate::model::hash;

/// Picks from weighted alternatives, deterministically from `seed`.
///
/// `weights` must be sorted and normalised — [`crate::crops::CropTable`] does
/// both when it reads a file, so a hand-edited CSV cannot change what a given
/// field draws just by listing its rows in another order.
pub fn draw(weights: &[(CropClass, f64)], seed: u64) -> Option<CropClass> {
    if weights.is_empty() {
        return None;
    }
    let total: f64 = weights.iter().map(|(_, w)| w).sum();
    if total <= 0.0 {
        return Some(weights[0].0);
    }
    // The top 53 bits of the hash as a fraction: f64 has exactly that many, so
    // the conversion neither rounds nor clumps.
    let unit = ((hash(&seed.to_le_bytes()) >> 11) as f64) / ((1u64 << 53) as f64);
    let mut at = unit * total;
    for (class, weight) in weights {
        at -= weight;
        if at < 0.0 {
            return Some(*class);
        }
    }
    Some(weights[weights.len() - 1].0)
}

/// A second, independent number from the same seed, in `0.0 ..= 1.0`.
///
/// Everything about a field that varies but must not change between runs comes
/// from here with a different `salt`: how far the sowing is offset across the
/// rows, how wide the working width is inside its range, how far the crop is
/// through its season on a given day.
pub fn vary(seed: u64, salt: u64) -> f64 {
    let h = hash(&(seed ^ salt.wrapping_mul(0x9e37_79b9_7f4a_7c15)).to_le_bytes());
    ((h >> 11) as f64) / ((1u64 << 53) as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crops::CropTable;
    use std::collections::HashMap;

    #[test]
    fn the_same_field_always_draws_the_same_crop() {
        let table = CropTable::built_in();
        let weights = table.group_weights("GT").expect("GT is in the table");
        let first = draw(weights, 1234);
        for _ in 0..10 {
            assert_eq!(draw(weights, 1234), first);
        }
    }

    #[test]
    fn the_draw_follows_the_weights() {
        let weights = [(CropClass::Maize, 0.25), (CropClass::WinterCereal, 0.75)];
        let mut counts: HashMap<CropClass, usize> = HashMap::new();
        for seed in 0..20_000u64 {
            *counts.entry(draw(&weights, seed).unwrap()).or_default() += 1;
        }
        let maize = counts[&CropClass::Maize] as f64 / 20_000.0;
        // Twenty thousand draws put the share within a percentage point of the
        // weight; a wider bound would let a broken hash through.
        assert!((maize - 0.25).abs() < 0.01, "{maize}");
    }

    #[test]
    fn no_weights_draws_nothing() {
        assert_eq!(draw(&[], 1), None);
    }

    #[test]
    fn a_single_alternative_always_wins() {
        let weights = [(CropClass::Grassland, 1.0)];
        for seed in 0..100 {
            assert_eq!(draw(&weights, seed), Some(CropClass::Grassland));
        }
    }

    #[test]
    fn variations_are_spread_and_repeatable() {
        assert_eq!(vary(7, 1), vary(7, 1));
        assert_ne!(vary(7, 1), vary(7, 2));
        let mean: f64 = (0..1000).map(|s| vary(s, 3)).sum::<f64>() / 1000.0;
        assert!((mean - 0.5).abs() < 0.05, "{mean}");
        assert!((0..1000).all(|s| (0.0..=1.0).contains(&vary(s, 4))));
    }
}
