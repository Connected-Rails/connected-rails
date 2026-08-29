//! The source note a line has to carry, and the one it must not ship without.
//!
//! The data is free, not unconditional. Most states ask under `dl-de/by-2-0` or
//! `CC BY 4.0` for the source to be named; Schleswig-Holstein asks for nothing;
//! and three states have not said at all — Mecklenburg-Vorpommern writes
//! "UrhG", Saxony-Anhalt writes nothing, Baden-Württemberg publishes no open
//! download. Those three are usable for building and looking at, and are not
//! something to put in a released module before somebody has the answer in
//! writing (plan ch. 2, ch. 9).
//!
//! So the import collects which states it drew on, and this turns that into the
//! two things that follow from it: a credit block to ship with the line, and a
//! warning for the states that have not granted anything. An hour of work that
//! covers the whole question.

use crate::cache::{OSM, origin_code};
use crate::land::{Land, Licence};
use std::collections::BTreeMap;

/// What a field taken from OpenStreetMap owes. A project name and a licence
/// identifier, so neither is translated.
const OSM_CREDIT: &str = "© OpenStreetMap contributors";

/// What a line owes for the field data in it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Attribution {
    /// The states used, and the application year taken from each. Ordered, so
    /// the credit block does not shuffle between builds.
    pub used: BTreeMap<&'static str, Option<u32>>,
}

impl Attribution {
    /// Notes that a field went into the line. `land` is `None` for one taken
    /// from OpenStreetMap outside the German registers.
    pub fn add(&mut self, land: Option<Land>, year: Option<u32>) {
        let entry = self.used.entry(origin_code(land)).or_insert(year);
        // The newest year seen wins: a module across a border shows the later
        // of the two registers, which is what "the state as of" means.
        if year > *entry {
            *entry = year;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.used.is_empty()
    }

    /// The states used, as [`Land`]s.
    pub fn lands(&self) -> Vec<Land> {
        self.used
            .keys()
            .filter_map(|c| Land::from_code(c))
            .collect()
    }

    /// The credit block, one line per state that asks for one. Empty when
    /// nothing used asks — Schleswig-Holstein alone owes nothing.
    ///
    /// Not translated: a source note under `dl-de/by-2-0` is the wording the
    /// licence asks for, and translating it would be answering a legal question
    /// with a Fluent file.
    pub fn credits(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for (code, year) in &self.used {
            // Abroad there is no state and no register — the fallback's own
            // note stands in its place.
            if *code == OSM {
                lines.push(format!("{OSM_CREDIT}, {}", Licence::Odbl.id()));
                continue;
            }
            let Some(land) = Land::from_code(code) else {
                continue;
            };
            let service = land.service();
            if !service.licence.needs_attribution() || service.credit.is_empty() {
                continue;
            }
            lines.push(match year {
                Some(year) => format!("{} ({year}), {}", service.credit, service.licence.id()),
                None => format!("{}, {}", service.credit, service.licence.id()),
            });
        }
        lines
    }

    /// The whole block as one string, ready to be written next to the line.
    pub fn block(&self) -> String {
        self.credits().join("\n")
    }

    /// The states whose licence nobody has established. A line that uses one of
    /// these can be built and driven; releasing it is a decision somebody has
    /// to make with the state's answer in hand.
    pub fn unclear(&self) -> Vec<Land> {
        self.lands()
            .into_iter()
            .filter(|l| l.service().licence == Licence::Unclear)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_used_owes_nothing() {
        let attribution = Attribution::default();
        assert!(attribution.is_empty());
        assert!(attribution.credits().is_empty());
        assert!(attribution.unclear().is_empty());
    }

    #[test]
    fn north_rhine_westphalia_is_named_with_its_year() {
        let mut attribution = Attribution::default();
        attribution.add(Some(Land::Nw), Some(2026));
        let credits = attribution.credits();
        assert_eq!(credits.len(), 1);
        assert!(credits[0].contains("Landwirtschaftskammer"), "{credits:?}");
        assert!(credits[0].contains("2026"), "{credits:?}");
        assert!(credits[0].contains("dl-de/by-2-0"), "{credits:?}");
    }

    #[test]
    fn schleswig_holstein_owes_no_credit() {
        let mut attribution = Attribution::default();
        attribution.add(Some(Land::Sh), Some(2026));
        assert!(!attribution.is_empty());
        assert!(attribution.credits().is_empty());
    }

    #[test]
    fn the_later_year_of_two_wins() {
        let mut attribution = Attribution::default();
        attribution.add(Some(Land::Nw), Some(2025));
        attribution.add(Some(Land::Nw), Some(2026));
        assert_eq!(attribution.used["NW"], Some(2026));
        attribution.add(Some(Land::Nw), Some(2024));
        assert_eq!(attribution.used["NW"], Some(2026));
    }

    #[test]
    fn a_state_without_a_licence_is_flagged() {
        let mut attribution = Attribution::default();
        attribution.add(Some(Land::Nw), Some(2026));
        attribution.add(Some(Land::Mv), Some(2026));
        assert_eq!(attribution.unclear(), vec![Land::Mv]);
    }

    #[test]
    fn a_field_from_abroad_credits_openstreetmap() {
        let mut attribution = Attribution::default();
        attribution.add(None, None);
        let credits = attribution.credits();
        assert_eq!(credits.len(), 1);
        assert!(credits[0].contains("OpenStreetMap"), "{credits:?}");
        assert!(credits[0].contains("ODbL"), "{credits:?}");
        // The fallback's licence is stated, so nothing about it is unclear.
        assert!(attribution.unclear().is_empty());
        // And it does not pretend to be a state.
        assert!(attribution.lands().is_empty());
    }

    #[test]
    fn the_block_is_stable_between_runs() {
        let mut a = Attribution::default();
        a.add(Some(Land::Th), None);
        a.add(Some(Land::Nw), Some(2026));
        let mut b = Attribution::default();
        b.add(Some(Land::Nw), Some(2026));
        b.add(Some(Land::Th), None);
        assert_eq!(a.block(), b.block());
        assert_eq!(a.block().lines().count(), 2);
    }
}
