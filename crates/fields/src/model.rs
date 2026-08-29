//! One field, in the shape everything downstream reads.
//!
//! Sixteen states, three schemas and a dozen attribute spellings meet here and
//! come out the same. Every state's reader fills a [`FieldFeature`] and nothing
//! after this point knows where the parcel came from — bar the two fields that
//! say so, which is what an attribution note and a reproducible import need.

use crate::crops::CropClass;
use crate::land::Land;
use glam::DVec2;

/// How much the source said about this parcel — carried through so the editor
/// can show that a field's crop was drawn rather than declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// The crop came out of the application: this is what the farmer declared.
    Declared,
    /// The service gave the InVeKoS group only (`GT`, cereals) and the crop was
    /// drawn from the group's weights.
    Group,
    /// The service gave the field block and no crop at all; the crop was drawn
    /// from the regional statistics (plan ch. 5).
    Drawn,
}

impl Level {
    pub fn key(self) -> &'static str {
        match self {
            Level::Declared => "field-level-declared",
            Level::Group => "field-level-group",
            Level::Drawn => "field-level-drawn",
        }
    }
}

/// A parcel as the import hands it over: the outline in the source's own UTM
/// zone, the crop resolved, and everything needed to explain the row.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldFeature {
    /// The outline, `(easting, northing)` [m] in `zone`, closed implicitly —
    /// the first point is not repeated at the end.
    pub polygon: Vec<DVec2>,
    /// UTM zone the polygon is in.
    pub zone: u8,
    /// The German state whose register the parcel came from, or `None` for a
    /// module outside them — abroad, where the fallback is OpenStreetMap and
    /// there is no state to name (see [`crate::osm`]).
    pub land: Option<Land>,
    /// Application year the parcel was declared for, where the service says.
    pub year: Option<u32>,
    /// The state's own code, verbatim — `115`, `GT`. Never matched on after
    /// the mapping; it is what a builder needs to correct a row.
    pub code_raw: String,
    /// The crop as the service spells it — `Winterweichweizen`. May be empty.
    pub code_text: String,
    pub crop: CropClass,
    pub level: Level,
    /// Which way the field was worked, as the angle of its long axis against
    /// grid east [rad] — furrows, tramlines and the combine's swath all run
    /// along it, and it is the single biggest thing the eye picks up about a
    /// field (plan ch. 7). Derived from the finished outline, so it is set
    /// when the import has cut the field to shape and not before.
    pub direction: f64,
    /// Declared area [ha] as the service states it. Not recomputed from the
    /// polygon: the two differ, and the service's number is the official one.
    pub area_ha: f64,
    /// Organic farming, where the service says. Kept because it is in the data
    /// and dropped from anything a player sees — see the note in plan ch. 9.
    pub organic: Option<bool>,
    /// The parcel's id at the source. The seed of every draw about this field,
    /// so the same parcel always comes out the same (plan ch. 5).
    pub id: String,
}

impl FieldFeature {
    /// Area of the outline [m²], from the polygon rather than the declaration.
    pub fn area(&self) -> f64 {
        crate::geometry::area(&self.polygon).abs()
    }

    /// Centre of the bounding box [m UTM] — where the editor's list jumps to.
    pub fn centre(&self) -> DVec2 {
        let (lo, hi) = self.bounds();
        (lo + hi) / 2.0
    }

    /// Bounding box `(min, max)` [m UTM].
    pub fn bounds(&self) -> (DVec2, DVec2) {
        let mut lo = DVec2::splat(f64::MAX);
        let mut hi = DVec2::splat(f64::MIN);
        for p in &self.polygon {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        (lo, hi)
    }

    /// The outline in degrees, `(lat, lon)` — what a line file stores.
    pub fn to_degrees(&self) -> Vec<(f64, f64)> {
        self.polygon
            .iter()
            .map(|p| {
                let (lat, lon) = world_coords::geo::from_utm(p.x, p.y, self.zone);
                (lat.to_degrees(), lon.to_degrees())
            })
            .collect()
    }

    /// A stable 64-bit seed for this parcel — the id if the source gave one,
    /// the outline's own geometry otherwise. Two runs of the import over the
    /// same data draw the same crops and the same furrow offsets from it.
    pub fn seed(&self) -> u64 {
        if !self.id.is_empty() {
            return hash(self.id.as_bytes());
        }
        // Rounded to the centimetre first: the services hand out coordinates
        // with a varying number of decimals between requests.
        let mut bytes = Vec::with_capacity(self.polygon.len() * 16);
        for p in &self.polygon {
            bytes.extend_from_slice(&((p.x * 100.0).round() as i64).to_le_bytes());
            bytes.extend_from_slice(&((p.y * 100.0).round() as i64).to_le_bytes());
        }
        hash(&bytes)
    }

    /// The label the editor puts on the row: the state's own crop name where
    /// there is one, the render group otherwise.
    pub fn label(&self) -> String {
        if self.code_text.is_empty() {
            i18n::t!(&self.crop.key())
        } else {
            self.code_text.clone()
        }
    }
}

/// FNV-1a. A hash that is the same on every machine and in every build — the
/// standard library's is deliberately neither.
pub fn hash(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(size: f64) -> FieldFeature {
        FieldFeature {
            polygon: vec![
                DVec2::new(0.0, 0.0),
                DVec2::new(size, 0.0),
                DVec2::new(size, size),
                DVec2::new(0.0, size),
            ],
            zone: 32,
            land: Some(Land::Nw),
            year: Some(2026),
            code_raw: "115".into(),
            code_text: "Winterweichweizen".into(),
            crop: CropClass::WinterCereal,
            level: Level::Declared,
            direction: 0.0,
            area_ha: 1.0,
            organic: Some(false),
            id: "test".into(),
        }
    }

    #[test]
    fn area_comes_off_the_outline() {
        assert!((square(100.0).area() - 10_000.0).abs() < 1e-6);
    }

    #[test]
    fn the_seed_follows_the_id_not_the_shape() {
        let a = square(100.0);
        let mut b = square(250.0);
        assert_eq!(a.seed(), b.seed());
        b.id = "other".into();
        assert_ne!(a.seed(), b.seed());
    }

    #[test]
    fn an_id_less_parcel_is_seeded_by_its_outline() {
        let mut a = square(100.0);
        a.id.clear();
        let mut b = square(100.0);
        b.id.clear();
        assert_eq!(a.seed(), b.seed());
        let mut c = square(101.0);
        c.id.clear();
        assert_ne!(a.seed(), c.seed());
    }

    #[test]
    fn degrees_land_where_the_utm_says() {
        let mut field = square(100.0);
        // A real place: the Soester Boerde, zone 32.
        field.polygon = field
            .polygon
            .iter()
            .map(|p| *p + DVec2::new(440_000.0, 5_715_000.0))
            .collect();
        let ring = field.to_degrees();
        assert_eq!(ring.len(), 4);
        for (lat, lon) in ring {
            assert!((51.0..52.0).contains(&lat), "{lat}");
            assert!((7.5..8.5).contains(&lon), "{lon}");
        }
    }
}
