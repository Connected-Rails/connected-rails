//! What has been fetched already, kept on disk.
//!
//! An import is a handful of requests to a public service, and repeating them
//! every time the editor opens a module is rude to the service and slow for the
//! user. Once fetched, a tile of parcels stays: the editor then re-imports
//! offline, on a train, and — the point of it — **reproducibly**. A line built
//! against the 2026 application year keeps being built against the 2026
//! application year even after North Rhine-Westphalia's daily update has moved
//! on (plan ch. 4).
//!
//! The stored form is the normalised [`FieldFeature`], not the service's own
//! answer: the mapping from a crop code to a render group is the expensive part
//! and the part most likely to be corrected, so it is done once at fetch time
//! and the correction is a re-import.
//!
//! Grid: two kilometres in the state's own UTM zone. A module is a few tiles, a
//! tile is some hundreds of parcels, and a tile boundary never cuts a field —
//! the parcels a tile holds are every parcel whose box *touches* it, so a field
//! on the seam comes back from both and is de-duplicated on its id.

use crate::crops::CropClass;
use crate::land::Land;
use crate::model::{FieldFeature, Level};
use glam::DVec2;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Edge length of a cache tile [m UTM].
pub const TILE: f64 = 2_000.0;

/// Where a tile sits in the grid.
pub type TileKey = (i64, i64);

/// The tile a point falls in.
pub fn tile_at(p: DVec2) -> TileKey {
    ((p.x / TILE).floor() as i64, (p.y / TILE).floor() as i64)
}

/// South-west corner of a tile [m UTM].
pub fn tile_min(key: TileKey) -> DVec2 {
    DVec2::new(key.0 as f64 * TILE, key.1 as f64 * TILE)
}

/// Every tile a box touches.
pub fn tiles_in(min: DVec2, max: DVec2) -> Vec<TileKey> {
    let (lo, hi) = (tile_at(min), tile_at(max));
    let mut keys = Vec::new();
    for y in lo.1..=hi.1 {
        for x in lo.0..=hi.0 {
            keys.push((x, y));
        }
    }
    keys
}

/// What a tile was fetched against — written into the line so a module says
/// which state of which register it portrays.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stamp {
    /// The state's code.
    pub land: String,
    /// Application year the parcels were declared for, where the service says.
    pub year: Option<u32>,
    /// When it was fetched, as seconds since the Unix epoch. Not a formatted
    /// date: a line file that is read on another machine should not depend on
    /// that machine's idea of a calendar.
    pub fetched: u64,
}

/// One tile's worth of parcels on disk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Entry {
    /// Format version. A file from an older layout is dropped and re-fetched
    /// rather than guessed at.
    version: u32,
    stamp: Stamp,
    fields: Vec<Record>,
}

const VERSION: u32 = 1;

/// What is written in a record's `land` column: the state's code, or `OSM` for
/// a field from outside the German registers.
pub const OSM: &str = "OSM";

/// The column for a field's origin.
pub fn origin_code(land: Option<Land>) -> &'static str {
    land.map_or(OSM, |l| l.code())
}

/// The origin a column names. The outer `Option` is "is this readable at all" —
/// an unknown code drops the row and the next import brings it back; the inner
/// one is the origin itself, which is `None` for OpenStreetMap.
pub fn origin_of(code: &str) -> Option<Option<Land>> {
    if code == OSM {
        return Some(None);
    }
    Land::from_code(code).map(Some)
}

/// A field as it is written out. Deliberately not [`FieldFeature`] itself:
/// the enums are stored as their ids, so renaming a Rust variant does not
/// invalidate everybody's cache, and a hand-read file says what it means.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Record {
    ring: Vec<(f64, f64)>,
    zone: u8,
    land: String,
    #[serde(default)]
    year: Option<u32>,
    code: String,
    #[serde(default)]
    text: String,
    crop: String,
    level: String,
    #[serde(default)]
    area: f64,
    #[serde(default)]
    organic: Option<bool>,
    #[serde(default)]
    id: String,
}

impl Record {
    fn of(field: &FieldFeature) -> Self {
        Self {
            ring: field.polygon.iter().map(|p| (p.x, p.y)).collect(),
            zone: field.zone,
            land: origin_code(field.land).to_string(),
            year: field.year,
            code: field.code_raw.clone(),
            text: field.code_text.clone(),
            crop: field.crop.id().to_string(),
            level: match field.level {
                Level::Declared => "declared",
                Level::Group => "group",
                Level::Drawn => "drawn",
            }
            .to_string(),
            area: field.area_ha,
            organic: field.organic,
            id: field.id.clone(),
        }
    }

    /// `None` for a row that no longer makes sense — an unknown state, an
    /// unknown crop id. The tile is then short a field, and the next import
    /// with a cleared cache brings it back.
    fn into_field(self) -> Option<FieldFeature> {
        Some(FieldFeature {
            polygon: self.ring.iter().map(|(x, y)| DVec2::new(*x, *y)).collect(),
            zone: self.zone,
            land: origin_of(&self.land)?,
            year: self.year,
            code_raw: self.code,
            code_text: self.text,
            crop: CropClass::from_id(&self.crop)?,
            // Derived from the outline, so it is worked out again rather than
            // stored: a cached field is re-shaped on every import anyway.
            direction: 0.0,
            level: match self.level.as_str() {
                "declared" => Level::Declared,
                "group" => Level::Group,
                _ => Level::Drawn,
            },
            area_ha: self.area,
            organic: self.organic,
            id: self.id,
        })
    }
}

/// The cache.
#[derive(Debug, Clone)]
pub struct FieldCache {
    directory: PathBuf,
    /// Never read from disk, only written — what "re-import against today's
    /// data" means.
    pub refresh: bool,
    /// Never write, and treat a miss as empty — for a machine with no line out.
    pub offline: bool,
}

impl Default for FieldCache {
    fn default() -> Self {
        Self {
            directory: PathBuf::from("cache/fields"),
            refresh: false,
            offline: false,
        }
    }
}

impl FieldCache {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
            ..Default::default()
        }
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// `<cache>/<land>/<x>/<y>.json`, with `OSM` where there is no state.
    pub fn path_for(&self, land: Option<Land>, key: TileKey) -> PathBuf {
        self.directory
            .join(origin_code(land))
            .join(key.0.to_string())
            .join(format!("{}.json", key.1))
    }

    /// A tile as it was stored, if it was.
    pub fn get(&self, land: Option<Land>, key: TileKey) -> Option<(Stamp, Vec<FieldFeature>)> {
        if self.refresh {
            return None;
        }
        let text = std::fs::read_to_string(self.path_for(land, key)).ok()?;
        let entry: Entry = serde_json::from_str(&text).ok()?;
        if entry.version != VERSION {
            return None;
        }
        let fields = entry
            .fields
            .into_iter()
            .filter_map(Record::into_field)
            .collect();
        Some((entry.stamp, fields))
    }

    /// Writes a tile. A cache that cannot be written is not an error the user
    /// should be stopped by — the import simply stays slow.
    pub fn put(&self, land: Option<Land>, key: TileKey, stamp: &Stamp, fields: &[FieldFeature]) {
        if self.offline {
            return;
        }
        let path = self.path_for(land, key);
        let Some(parent) = path.parent() else { return };
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
        let entry = Entry {
            version: VERSION,
            stamp: stamp.clone(),
            fields: fields.iter().map(Record::of).collect(),
        };
        if let Ok(text) = serde_json::to_string(&entry) {
            let _ = std::fs::write(path, text);
        }
    }

    /// Throws the whole cache away — the editor's "fetch it again" button.
    pub fn clear(&self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

/// Seconds since the Unix epoch, or zero on a machine whose clock is before it.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(id: &str) -> FieldFeature {
        FieldFeature {
            polygon: vec![
                DVec2::new(440_000.0, 5_715_000.0),
                DVec2::new(440_100.0, 5_715_000.0),
                DVec2::new(440_100.0, 5_715_100.0),
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
            id: id.into(),
        }
    }

    #[test]
    fn tiles_line_up_with_their_corners() {
        assert_eq!(tile_at(DVec2::new(0.0, 0.0)), (0, 0));
        assert_eq!(tile_at(DVec2::new(1999.0, 1999.0)), (0, 0));
        assert_eq!(tile_at(DVec2::new(2001.0, 0.0)), (1, 0));
        assert_eq!(tile_at(DVec2::new(-1.0, 0.0)), (-1, 0));
        assert_eq!(tile_min((3, -2)), DVec2::new(6_000.0, -4_000.0));
    }

    #[test]
    fn a_box_covers_every_tile_it_touches() {
        let keys = tiles_in(DVec2::new(100.0, 100.0), DVec2::new(4_100.0, 2_100.0));
        assert_eq!(keys.len(), 6, "{keys:?}");
        assert!(keys.contains(&(0, 0)) && keys.contains(&(2, 1)));
    }

    #[test]
    fn a_tile_survives_the_round_trip() {
        let dir = std::env::temp_dir().join(format!("fields-cache-{}", std::process::id()));
        let cache = FieldCache::new(&dir);
        let stamp = Stamp {
            land: "NW".into(),
            year: Some(2026),
            fetched: 1_700_000_000,
        };
        let fields = vec![field("a"), field("b")];
        cache.put(Some(Land::Nw), (220, 2857), &stamp, &fields);

        let (read_stamp, read_fields) = cache.get(Some(Land::Nw), (220, 2857)).expect("stored");
        assert_eq!(read_stamp, stamp);
        assert_eq!(read_fields, fields);

        cache.clear();
        assert!(cache.get(Some(Land::Nw), (220, 2857)).is_none());
    }

    #[test]
    fn refresh_ignores_what_is_stored() {
        let dir = std::env::temp_dir().join(format!("fields-refresh-{}", std::process::id()));
        let mut cache = FieldCache::new(&dir);
        let stamp = Stamp {
            land: "NW".into(),
            year: None,
            fetched: 0,
        };
        cache.put(Some(Land::Nw), (1, 1), &stamp, &[field("a")]);
        assert!(cache.get(Some(Land::Nw), (1, 1)).is_some());
        cache.refresh = true;
        assert!(cache.get(Some(Land::Nw), (1, 1)).is_none());
        cache.clear();
    }

    #[test]
    fn a_file_from_another_version_is_not_guessed_at() {
        let dir = std::env::temp_dir().join(format!("fields-version-{}", std::process::id()));
        let cache = FieldCache::new(&dir);
        let path = cache.path_for(Some(Land::Nw), (0, 0));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{"version":99,"stamp":{"land":"NW","year":null,"fetched":0},"fields":[]}"#,
        )
        .unwrap();
        assert!(cache.get(Some(Land::Nw), (0, 0)).is_none());
        cache.clear();
    }

    #[test]
    fn an_offline_cache_reads_but_never_writes() {
        let dir = std::env::temp_dir().join(format!("fields-offline-{}", std::process::id()));
        let mut cache = FieldCache::new(&dir);
        cache.offline = true;
        let stamp = Stamp {
            land: "NW".into(),
            year: None,
            fetched: 0,
        };
        cache.put(Some(Land::Nw), (0, 0), &stamp, &[field("a")]);
        assert!(cache.get(Some(Land::Nw), (0, 0)).is_none());
        assert!(!cache.path_for(Some(Land::Nw), (0, 0)).exists());
    }
}
