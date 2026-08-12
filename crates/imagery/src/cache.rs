//! Kachel-Cache: Arbeitsspeicher davor, Platte dahinter.
//!
//! Einmal geladene Kacheln bleiben liegen — der Editor ist damit offline benutzbar, und
//! die Dienste werden nicht bei jedem Programmstart erneut belastet. Der Plattenplatz ist
//! gedeckelt; ist das Budget erschöpft, fliegen die ältesten Kacheln zuerst.

use crate::config::CacheConfig;
use crate::tiles::TileId;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// Kennzeichnung einer Kachel im Cache.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey {
    pub provider: String,
    pub tile: TileId,
}

impl CacheKey {
    pub fn new(provider: impl Into<String>, tile: TileId) -> Self {
        Self {
            provider: provider.into(),
            tile,
        }
    }
}

/// Kennzahlen für die Anzeige im Editor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub hits_memory: usize,
    pub hits_disk: usize,
    pub misses: usize,
    pub stored: usize,
    pub evicted: usize,
    /// Belegter Plattenplatz beim letzten Aufräumen [Byte].
    pub disk_bytes: u64,
}

/// Der Cache.
#[derive(Debug)]
pub struct TileCache {
    directory: PathBuf,
    max_bytes: u64,
    max_age: Option<Duration>,
    memory_limit: usize,
    /// Zuletzt benutzte Kacheln, vorne die jüngste.
    memory: VecDeque<(CacheKey, Arc<Vec<u8>>)>,
    stats: CacheStats,
}

impl TileCache {
    pub fn new(config: &CacheConfig) -> Self {
        Self {
            directory: config.directory.clone(),
            max_bytes: config.max_bytes,
            max_age: (config.max_age_days > 0)
                .then(|| Duration::from_secs(config.max_age_days * 24 * 3600)),
            memory_limit: config.memory_tiles.max(1),
            memory: VecDeque::new(),
            stats: CacheStats::default(),
        }
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn stats(&self) -> CacheStats {
        self.stats
    }

    /// Ablageort einer Kachel: `<cache>/<anbieter>/<z>/<x>/<y>.<ext>`.
    pub fn path_for(&self, key: &CacheKey, extension: &str) -> PathBuf {
        self.directory
            .join(sanitize(&key.provider))
            .join(key.tile.z.to_string())
            .join(key.tile.x.to_string())
            .join(format!("{}.{extension}", key.tile.y))
    }

    /// Kachel aus dem Cache holen.
    pub fn get(&mut self, key: &CacheKey, extension: &str) -> Option<Arc<Vec<u8>>> {
        if let Some(index) = self.memory.iter().position(|(k, _)| k == key) {
            let entry = self.memory.remove(index).expect("Index gerade gefunden");
            let data = entry.1.clone();
            self.memory.push_front(entry);
            self.stats.hits_memory += 1;
            return Some(data);
        }

        let path = self.path_for(key, extension);
        if self.is_stale(&path) {
            self.stats.misses += 1;
            return None;
        }
        match std::fs::read(&path) {
            Ok(bytes) if !bytes.is_empty() => {
                let data = Arc::new(bytes);
                self.remember(key.clone(), data.clone());
                self.stats.hits_disk += 1;
                Some(data)
            }
            _ => {
                self.stats.misses += 1;
                None
            }
        }
    }

    /// Ist eine Kachel vorhanden, ohne sie zu laden?
    pub fn contains(&self, key: &CacheKey, extension: &str) -> bool {
        self.memory.iter().any(|(k, _)| k == key) || {
            let path = self.path_for(key, extension);
            path.exists() && !self.is_stale(&path)
        }
    }

    /// Kachel ablegen.
    pub fn store(&mut self, key: CacheKey, extension: &str, bytes: Vec<u8>) -> std::io::Result<()> {
        let path = self.path_for(&key, extension);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, &bytes)?;
        self.stats.stored += 1;
        self.remember(key, Arc::new(bytes));
        self.prune();
        Ok(())
    }

    fn remember(&mut self, key: CacheKey, data: Arc<Vec<u8>>) {
        self.memory.retain(|(k, _)| *k != key);
        self.memory.push_front((key, data));
        while self.memory.len() > self.memory_limit {
            self.memory.pop_back();
        }
    }

    fn is_stale(&self, path: &Path) -> bool {
        let Some(max_age) = self.max_age else {
            return false;
        };
        let Ok(modified) = std::fs::metadata(path).and_then(|m| m.modified()) else {
            return false;
        };
        modified.elapsed().is_ok_and(|age| age > max_age)
    }

    /// Plattenplatz auf das Budget zurückführen — älteste Kacheln zuerst.
    pub fn prune(&mut self) {
        if self.max_bytes == 0 {
            return;
        }
        let mut files = Vec::new();
        collect(&self.directory, &mut files);
        let total: u64 = files.iter().map(|(_, size, _)| *size).sum();
        self.stats.disk_bytes = total;
        if total <= self.max_bytes {
            return;
        }

        files.sort_by_key(|(_, _, modified)| *modified);
        let mut remaining = total;
        for (path, size, _) in files {
            if remaining <= self.max_bytes {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                remaining -= size;
                self.stats.evicted += 1;
            }
        }
        self.stats.disk_bytes = remaining;
    }

    /// Alles verwerfen — Platte und Arbeitsspeicher.
    pub fn clear(&mut self) {
        self.memory.clear();
        let _ = std::fs::remove_dir_all(&self.directory);
        self.stats = CacheStats::default();
    }

    /// Belegter Plattenplatz [Byte].
    pub fn disk_usage(&self) -> u64 {
        let mut files = Vec::new();
        collect(&self.directory, &mut files);
        files.iter().map(|(_, size, _)| *size).sum()
    }
}

/// Alle Dateien unterhalb eines Verzeichnisses mit Größe und Änderungszeit.
fn collect(dir: &Path, out: &mut Vec<(PathBuf, u64, SystemTime)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if let Ok(meta) = entry.metadata() {
            let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            out.push((path, meta.len(), modified));
        }
    }
}

/// Anbietername in einen sicheren Verzeichnisnamen überführen.
fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_config(name: &str) -> CacheConfig {
        let dir = std::env::temp_dir().join(format!("trainsim-tilecache-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        CacheConfig {
            directory: dir,
            max_bytes: 0,
            memory_tiles: 2,
            offline: false,
            max_age_days: 0,
        }
    }

    #[test]
    fn ablegen_und_wiederfinden() {
        let config = temp_config("basic");
        let mut cache = TileCache::new(&config);
        let key = CacheKey::new("esri", TileId::new(14, 8800, 5375));

        assert!(cache.get(&key, "jpg").is_none());
        assert!(!cache.contains(&key, "jpg"));
        cache.store(key.clone(), "jpg", vec![1, 2, 3]).unwrap();

        assert!(cache.contains(&key, "jpg"));
        assert_eq!(*cache.get(&key, "jpg").unwrap(), vec![1, 2, 3]);
        assert_eq!(cache.stats().hits_memory, 1);

        // Auch ein frischer Cache findet die Datei wieder — das ist der Sinn der Sache.
        let mut wiedergeoeffnet = TileCache::new(&config);
        assert_eq!(*wiedergeoeffnet.get(&key, "jpg").unwrap(), vec![1, 2, 3]);
        assert_eq!(wiedergeoeffnet.stats().hits_disk, 1);

        // Ablage nach Anbieter und Zoomstufe getrennt.
        let path = cache.path_for(&key, "jpg");
        assert!(
            path.ends_with("esri/14/8800/5375.jpg"),
            "{}",
            path.display()
        );
        std::fs::remove_dir_all(config.directory).ok();
    }

    #[test]
    fn arbeitsspeicher_haelt_nur_die_juengsten() {
        let config = temp_config("memory");
        let mut cache = TileCache::new(&config);
        for i in 0..4u32 {
            let key = CacheKey::new("p", TileId::new(10, i, 0));
            cache.store(key, "png", vec![i as u8; 10]).unwrap();
        }
        // memory_tiles = 2 → die ersten beiden kommen von der Platte.
        let old = CacheKey::new("p", TileId::new(10, 0, 0));
        assert!(cache.get(&old, "png").is_some());
        assert_eq!(cache.stats().hits_disk, 1);
        assert_eq!(cache.stats().hits_memory, 0);
        std::fs::remove_dir_all(config.directory).ok();
    }

    #[test]
    fn budget_wirft_die_aeltesten_kacheln_raus() {
        let mut config = temp_config("prune");
        config.max_bytes = 2_500;
        let mut cache = TileCache::new(&config);

        for i in 0..5u32 {
            let key = CacheKey::new("p", TileId::new(12, i, 0));
            cache.store(key, "png", vec![0u8; 1000]).unwrap();
            // Änderungszeiten auseinanderziehen, damit die Reihenfolge eindeutig ist.
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        assert!(
            cache.disk_usage() <= 2_500,
            "{} Byte über dem Budget",
            cache.disk_usage()
        );
        assert!(cache.stats().evicted >= 2, "{:?}", cache.stats());
        // Die neueste Kachel ist noch da.
        let newest = CacheKey::new("p", TileId::new(12, 4, 0));
        assert!(cache.contains(&newest, "png"));
        std::fs::remove_dir_all(config.directory).ok();
    }

    #[test]
    fn veraltete_kacheln_gelten_als_fehlend() {
        let mut config = temp_config("age");
        config.max_age_days = 0;
        let mut cache = TileCache::new(&config);
        let key = CacheKey::new("p", TileId::new(9, 1, 1));
        cache.store(key.clone(), "png", vec![7; 4]).unwrap();
        assert!(
            cache.contains(&key, "png"),
            "ohne Altersgrenze bleibt sie gültig"
        );

        // Mit einer Altersgrenze von „0 Tagen" bleibt sie gültig (0 = nie ablaufen).
        // Eine echte Grenze prüfen wir über einen Cache mit winziger Höchstdauer.
        let mut kurzlebig = TileCache::new(&config);
        kurzlebig.max_age = Some(Duration::from_millis(1));
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(!kurzlebig.contains(&key, "png"), "abgelaufen");
        assert!(kurzlebig.get(&key, "png").is_none());
        std::fs::remove_dir_all(config.directory).ok();
    }

    #[test]
    fn leeren_entfernt_alles() {
        let config = temp_config("clear");
        let mut cache = TileCache::new(&config);
        let key = CacheKey::new("p", TileId::new(8, 3, 3));
        cache.store(key.clone(), "png", vec![1; 100]).unwrap();
        cache.clear();
        assert!(!cache.contains(&key, "png"));
        assert_eq!(cache.disk_usage(), 0);
    }

    #[test]
    fn anbietername_wird_entschaerft() {
        assert_eq!(sanitize("a/b:c*d"), "a_b_c_d");
        let cache = TileCache::new(&temp_config("sanitize"));
        let key = CacheKey::new("../böse", TileId::new(1, 0, 0));
        let path = cache.path_for(&key, "png");
        assert!(!path.to_string_lossy().contains(".."), "{}", path.display());
    }
}
