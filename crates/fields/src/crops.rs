//! What a crop code means, in the only resolution that shows.
//!
//! Every state numbers its crops itself and numbers a lot of them: North
//! Rhine-Westphalia alone has 222 codes in use, down to *Arnica montana* on
//! four hectares. Drawing 222 kinds of field would be a year of texture work
//! for something nobody can tell apart at 140 km/h, so the import maps them
//! onto a dozen groups that genuinely look different from a train window —
//! winter cereal, maize, rape, beet, grass — and keeps the original code and
//! text on the field for the property panel and for anybody who wants to argue
//! about a row (plan ch. 5, "zweistufige interne Taxonomie").
//!
//! The mapping is a CSV per state, not code. It is a judgement call which
//! bucket *Kohlrübe* falls in, the answer changes with the region, and a
//! builder who disagrees should be able to fix it in a text editor. The files
//! ship inside the binary and a directory of overrides is read over the top.

use crate::land::Land;
use std::collections::HashMap;

/// What the simulator draws. The plan's stage one: twelve groups plus the
/// bucket an unknown code falls into.
//
// ponytail: sunflowers, hemp and flax end up in `Other` — each is under half a
// per cent of the arable land and each would want its own model. When a line
// through the Uckermark makes sunflowers worth it, they become a variant of
// their own rather than a finer split of everything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CropClass {
    /// Wheat, rye, barley, triticale — sown in autumn, green over winter,
    /// gold in July.
    WinterCereal,
    /// The same species sown in spring, and everything else that ripens with
    /// them: oats, millet, buckwheat.
    SummerCereal,
    Maize,
    Rapeseed,
    SugarBeet,
    Potato,
    Legume,
    Grassland,
    Vegetable,
    Orchard,
    Vineyard,
    /// Set aside, flowering strips, field margins — bare in spring, rough and
    /// weedy by midsummer.
    Fallow,
    /// A code the table does not know, or a crop with no group of its own.
    Other,
}

impl CropClass {
    pub const ALL: [CropClass; 13] = [
        CropClass::WinterCereal,
        CropClass::SummerCereal,
        CropClass::Maize,
        CropClass::Rapeseed,
        CropClass::SugarBeet,
        CropClass::Potato,
        CropClass::Legume,
        CropClass::Grassland,
        CropClass::Vegetable,
        CropClass::Orchard,
        CropClass::Vineyard,
        CropClass::Fallow,
        CropClass::Other,
    ];

    /// The identifier used in the CSVs and in a line file. Kebab-case like
    /// every other id in the project.
    pub fn id(self) -> &'static str {
        match self {
            CropClass::WinterCereal => "winter-cereal",
            CropClass::SummerCereal => "summer-cereal",
            CropClass::Maize => "maize",
            CropClass::Rapeseed => "rapeseed",
            CropClass::SugarBeet => "sugar-beet",
            CropClass::Potato => "potato",
            CropClass::Legume => "legume",
            CropClass::Grassland => "grassland",
            CropClass::Vegetable => "vegetable",
            CropClass::Orchard => "orchard",
            CropClass::Vineyard => "vineyard",
            CropClass::Fallow => "fallow",
            CropClass::Other => "other",
        }
    }

    pub fn from_id(id: &str) -> Option<CropClass> {
        CropClass::ALL.into_iter().find(|c| c.id() == id)
    }

    /// The translation key of the name shown in the editor.
    pub fn key(self) -> String {
        format!("crop-{}", self.id())
    }
}

/// One row of a state's code list.
#[derive(Debug, Clone, PartialEq)]
pub struct CropEntry {
    pub class: CropClass,
    /// The crop as the service writes it — `Winterweichweizen`. Kept for the
    /// property panel, and only ever shown, never matched on.
    pub label: String,
}

/// The code lists, and the weights the guesswork needs.
#[derive(Debug, Clone, Default)]
pub struct CropTable {
    /// Per state, the detailed code list. States that publish only a group
    /// code have none.
    by_land: HashMap<&'static str, HashMap<String, CropEntry>>,
    /// The InVeKoS group code (`GT`, `OE`, …) and the render groups it can
    /// stand for, with weights that sum to one.
    groups: HashMap<String, Vec<(CropClass, f64)>>,
    /// What grows on arable land where nothing but "arable" is known, per
    /// region (`*` = anywhere).
    arable: HashMap<String, Vec<(CropClass, f64)>>,
}

impl CropTable {
    /// The tables that ship with the program.
    pub fn built_in() -> Self {
        let mut table = CropTable::default();
        table.load_codes("NW", include_str!("crops/nw.csv"));
        table.load_groups(include_str!("crops/groups.csv"));
        table.load_arable(include_str!("crops/arable.csv"));
        table
    }

    /// Reads `<dir>/<land>.csv`, `<dir>/groups.csv` and `<dir>/arable.csv` over
    /// the built-in tables — a mod or a builder correcting a row. A file that
    /// is not there changes nothing; that is the normal case.
    pub fn load_overrides(&mut self, dir: &std::path::Path) {
        for land in Land::ALL {
            let path = dir.join(format!("{}.csv", land.code().to_lowercase()));
            if let Ok(text) = std::fs::read_to_string(&path) {
                self.load_codes_owned(land.code(), &text);
            }
        }
        if let Ok(text) = std::fs::read_to_string(dir.join("groups.csv")) {
            self.load_groups(&text);
        }
        if let Ok(text) = std::fs::read_to_string(dir.join("arable.csv")) {
            self.load_arable(&text);
        }
    }

    fn load_codes(&mut self, land: &'static str, text: &str) {
        let entries = self.by_land.entry(land).or_default();
        for (code, class, label) in parse_codes(text) {
            entries.insert(code, CropEntry { class, label });
        }
    }

    /// The same for a state whose code is not a literal in this binary.
    fn load_codes_owned(&mut self, land: &str, text: &str) {
        let Some(land) = Land::ALL.into_iter().find(|l| l.code() == land) else {
            return;
        };
        self.load_codes(land.code(), text);
    }

    fn load_groups(&mut self, text: &str) {
        for (key, class, weight) in parse_weights(text) {
            self.groups.entry(key).or_default().push((class, weight));
        }
        normalise(&mut self.groups);
    }

    fn load_arable(&mut self, text: &str) {
        for (key, class, weight) in parse_weights(text) {
            self.arable.entry(key).or_default().push((class, weight));
        }
        normalise(&mut self.arable);
    }

    /// What a state's own code means. `None` when the state has no list, or
    /// the code is not in it — the caller then falls back to the group.
    pub fn lookup(&self, land: Land, code: &str) -> Option<&CropEntry> {
        self.by_land.get(land.code())?.get(code)
    }

    /// The render groups an InVeKoS group code can stand for, with weights.
    pub fn group_weights(&self, group: &str) -> Option<&[(CropClass, f64)]> {
        self.groups.get(group).map(|v| v.as_slice())
    }

    /// The weights for a parcel that is only known to be arable. The region's
    /// own row if there is one, the general one otherwise.
    pub fn arable_weights(&self, region: &str) -> Option<&[(CropClass, f64)]> {
        self.arable
            .get(region)
            .or_else(|| self.arable.get("*"))
            .map(|v| v.as_slice())
    }

    /// How many detail codes are known for a state — what the editor shows to
    /// say whether an override took.
    pub fn code_count(&self, land: Land) -> usize {
        self.by_land.get(land.code()).map_or(0, HashMap::len)
    }
}

/// `code,class,…` — everything after the second column is documentation (the
/// German label carries commas of its own, so it is never split on).
fn parse_codes(text: &str) -> Vec<(String, CropClass, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(3, ',');
        let (Some(code), Some(class)) = (parts.next(), parts.next()) else {
            continue;
        };
        let Some(class) = CropClass::from_id(class.trim()) else {
            continue;
        };
        // What is left is `label,group,share`. The label is taken by peeling
        // the last two columns off the *end* — a German crop name carries
        // commas of its own ("Salat (Garten, Lollo Rosso.)") and splitting
        // forwards would cut it in half.
        let label = parts
            .next()
            .map(|rest| {
                let mut back = rest.rsplitn(3, ',');
                match (back.next(), back.next(), back.next()) {
                    (Some(_), Some(_), Some(label)) => label,
                    _ => rest,
                }
            })
            .unwrap_or("")
            .trim()
            .to_string();
        out.push((code.trim().to_string(), class, label));
    }
    out
}

/// `key,class,weight` — the two weight tables share a shape.
fn parse_weights(text: &str) -> Vec<(String, CropClass, f64)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split(',');
        let (Some(key), Some(class), Some(weight)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let (Some(class), Ok(weight)) = (CropClass::from_id(class.trim()), weight.trim().parse())
        else {
            continue;
        };
        if weight > 0.0 {
            out.push((key.trim().to_string(), class, weight));
        }
    }
    out
}

/// Weights are read as they are written and normalised afterwards, so a hand-
/// edited file may hold percentages, shares or plain counts.
fn normalise(table: &mut HashMap<String, Vec<(CropClass, f64)>>) {
    for weights in table.values_mut() {
        let total: f64 = weights.iter().map(|(_, w)| w).sum();
        if total > 0.0 {
            for (_, w) in weights.iter_mut() {
                *w /= total;
            }
        }
        // A stable order, so the draw is reproducible whatever the file's was.
        weights.sort_by_key(|(class, _)| *class);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        for class in CropClass::ALL {
            assert_eq!(CropClass::from_id(class.id()), Some(class));
        }
    }

    #[test]
    fn the_built_in_table_knows_north_rhine_westphalia() {
        let table = CropTable::built_in();
        // 115 is winter wheat, the state's single biggest crop.
        let entry = table.lookup(Land::Nw, "115").expect("115 is in the list");
        assert_eq!(entry.class, CropClass::WinterCereal);
        assert_eq!(entry.label, "Winterweichweizen");
        // Silage maize sits in the fodder group and still has to come out maize.
        assert_eq!(
            table.lookup(Land::Nw, "411").map(|e| e.class),
            Some(CropClass::Maize)
        );
        assert_eq!(
            table.lookup(Land::Nw, "459").map(|e| e.class),
            Some(CropClass::Grassland)
        );
        assert!(table.code_count(Land::Nw) > 200);
    }

    #[test]
    fn labels_with_commas_survive() {
        let table = CropTable::built_in();
        // "Salat (Garten, Lollo Rosso.)" would be cut in half by a naive split.
        let entry = table.lookup(Land::Nw, "637").expect("637 is in the list");
        assert!(entry.label.contains("Lollo"), "{}", entry.label);
    }

    #[test]
    fn group_weights_sum_to_one() {
        let table = CropTable::built_in();
        for group in ["GT", "OE", "HF", "GL", "AF"] {
            let weights = table.group_weights(group).expect("group is in the list");
            let total: f64 = weights.iter().map(|(_, w)| w).sum();
            assert!((total - 1.0).abs() < 1e-9, "{group}: {total}");
        }
    }

    #[test]
    fn cereal_group_is_mostly_winter_cereal() {
        let table = CropTable::built_in();
        let weights = table.group_weights("GT").expect("GT is in the list");
        let winter = weights
            .iter()
            .find(|(c, _)| *c == CropClass::WinterCereal)
            .expect("winter cereal is in the group");
        assert!(winter.1 > 0.5, "{winter:?}");
    }

    #[test]
    fn arable_falls_back_to_the_general_row() {
        let table = CropTable::built_in();
        let weights = table.arable_weights("HE").expect("there is a general row");
        assert!(!weights.is_empty());
        let total: f64 = weights.iter().map(|(_, w)| w).sum();
        assert!((total - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_comment_only_file_changes_nothing() {
        assert!(parse_codes("# nothing here\n\n").is_empty());
        assert!(parse_weights("# nothing here\n\n").is_empty());
    }
}
