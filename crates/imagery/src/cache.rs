//! Tile cache: memory in front, disk behind.
//!
//! Once fetched, tiles stay around — this makes the editor usable offline, and the
//! services are not hit again on every program start. The disk space is capped;
//! once the budget is exhausted, the oldest tiles go first.

use crate::config::CacheConfig;
use crate::tiles::TileId;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// Identification of a tile in the cache.
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

/// Metrics for display in the editor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub hits_memory: usize,
    pub hits_disk: usize,
    pub misses: usize,
    pub stored: usize,
    pub evicted: usize,
    /// Disk space used at the last cleanup [bytes].
    pub disk_bytes: u64,
}

/// The cache.
#[derive(Debug)]
pub struct TileCache {
    directory: PathBuf,
    max_bytes: u64,
    max_age: Option<Duration>,
    memory_limit: usize,
    /// Most recently used tiles, the newest at the front.
    memory: VecDeque<(CacheKey, Arc<Vec<u8>>)>,
    /// Has the cache directory been walked once in this session? A full walk
    /// is thousands of files, so `stats.disk_bytes` is carried forward from
    /// what is written and evicted instead, and the walk repeats only when the
    /// budget is actually in reach.
    scanned: bool,
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
            scanned: false,
            stats: CacheStats::default(),
        }
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn stats(&self) -> CacheStats {
        self.stats
    }

    /// Storage location of a tile: `<cache>/<provider>/<z>/<x>/<y>.<ext>`.
    pub fn path_for(&self, key: &CacheKey, extension: &str) -> PathBuf {
        self.directory
            .join(sanitize(&key.provider))
            .join(key.tile.z.to_string())
            .join(key.tile.x.to_string())
            .join(format!("{}.{extension}", key.tile.y))
    }

    /// Fetch a tile from the cache.
    pub fn get(&mut self, key: &CacheKey, extension: &str) -> Option<Arc<Vec<u8>>> {
        if let Some(index) = self.memory.iter().position(|(k, _)| k == key) {
            let entry = self.memory.remove(index).expect("index just found");
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

    /// Is a tile present, without loading it?
    pub fn contains(&self, key: &CacheKey, extension: &str) -> bool {
        self.memory.iter().any(|(k, _)| k == key) || {
            let path = self.path_for(key, extension);
            path.exists() && !self.is_stale(&path)
        }
    }

    /// Store a tile.
    pub fn store(&mut self, key: CacheKey, extension: &str, bytes: Vec<u8>) -> std::io::Result<()> {
        let path = self.path_for(&key, extension);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, &bytes)?;
        self.stats.stored += 1;
        self.stats.disk_bytes += bytes.len() as u64;
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

    /// Bring the disk space back down to the budget — oldest tiles first.
    ///
    /// Called after every stored tile, so the walk over the cache directory has
    /// to stay off the common path: while the running total says there is room,
    /// nothing can need evicting and nothing is walked.
    pub fn prune(&mut self) {
        if self.max_bytes == 0 {
            return;
        }
        if self.scanned && self.stats.disk_bytes <= self.max_bytes {
            return;
        }
        let mut files = self.rescan();
        if self.stats.disk_bytes <= self.max_bytes {
            return;
        }

        files.sort_by_key(|(_, _, modified)| *modified);
        let mut remaining = self.stats.disk_bytes;
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

    /// Discard everything — disk and memory.
    pub fn clear(&mut self) {
        self.memory.clear();
        let _ = std::fs::remove_dir_all(&self.directory);
        self.stats = CacheStats::default();
        self.scanned = true;
    }

    /// Disk space used [bytes]. The editor reads this every frame it draws the
    /// cache panel, so the directory is walked once and the figure is carried
    /// forward after that.
    pub fn disk_usage(&mut self) -> u64 {
        if !self.scanned {
            self.rescan();
        }
        self.stats.disk_bytes
    }

    /// Walk the cache directory and set `disk_bytes` to what is really there.
    fn rescan(&mut self) -> Vec<(PathBuf, u64, SystemTime)> {
        let mut files = Vec::new();
        collect(&self.directory, &mut files);
        self.stats.disk_bytes = files.iter().map(|(_, size, _)| *size).sum();
        self.scanned = true;
        files
    }
}

/// All files below a directory with size and modification time.
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

/// Turn a provider name into a safe directory name.
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
    fn store_and_find_again() {
        let config = temp_config("basic");
        let mut cache = TileCache::new(&config);
        let key = CacheKey::new("esri", TileId::new(14, 8800, 5375));

        assert!(cache.get(&key, "jpg").is_none());
        assert!(!cache.contains(&key, "jpg"));
        cache.store(key.clone(), "jpg", vec![1, 2, 3]).unwrap();

        assert!(cache.contains(&key, "jpg"));
        assert_eq!(*cache.get(&key, "jpg").unwrap(), vec![1, 2, 3]);
        assert_eq!(cache.stats().hits_memory, 1);

        // A fresh cache finds the file again too — that is the whole point.
        let mut reopened = TileCache::new(&config);
        assert_eq!(*reopened.get(&key, "jpg").unwrap(), vec![1, 2, 3]);
        assert_eq!(reopened.stats().hits_disk, 1);

        // Storage separated by provider and zoom level.
        let path = cache.path_for(&key, "jpg");
        assert!(
            path.ends_with("esri/14/8800/5375.jpg"),
            "{}",
            path.display()
        );
        std::fs::remove_dir_all(config.directory).ok();
    }

    #[test]
    fn memory_keeps_only_the_newest() {
        let config = temp_config("memory");
        let mut cache = TileCache::new(&config);
        for i in 0..4u32 {
            let key = CacheKey::new("p", TileId::new(10, i, 0));
            cache.store(key, "png", vec![i as u8; 10]).unwrap();
        }
        // memory_tiles = 2 → the first two come from disk.
        let old = CacheKey::new("p", TileId::new(10, 0, 0));
        assert!(cache.get(&old, "png").is_some());
        assert_eq!(cache.stats().hits_disk, 1);
        assert_eq!(cache.stats().hits_memory, 0);
        std::fs::remove_dir_all(config.directory).ok();
    }

    #[test]
    fn budget_evicts_the_oldest_tiles() {
        let mut config = temp_config("prune");
        config.max_bytes = 2_500;
        let mut cache = TileCache::new(&config);

        for i in 0..5u32 {
            let key = CacheKey::new("p", TileId::new(12, i, 0));
            cache.store(key, "png", vec![0u8; 1000]).unwrap();
            // Spread the modification times apart so the order is unambiguous.
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        assert!(
            cache.disk_usage() <= 2_500,
            "{} bytes over the budget",
            cache.disk_usage()
        );
        assert!(cache.stats().evicted >= 2, "{:?}", cache.stats());
        // The newest tile is still there.
        let newest = CacheKey::new("p", TileId::new(12, 4, 0));
        assert!(cache.contains(&newest, "png"));
        std::fs::remove_dir_all(config.directory).ok();
    }

    /// The walk is skipped while there is room — but a cache that was already
    /// over budget before this session started still has to be found and
    /// pruned, and that is the case the skip could swallow.
    #[test]
    fn a_cache_already_over_budget_is_still_pruned() {
        let mut config = temp_config("prefilled");
        config.max_bytes = 2_500;
        {
            let mut filling = TileCache::new(&config);
            filling.max_bytes = 0; // fill past the budget without evicting
            for i in 0..5u32 {
                let key = CacheKey::new("p", TileId::new(12, i, 0));
                filling.store(key, "png", vec![0u8; 1000]).unwrap();
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }

        // A fresh cache knows nothing about those 5000 bytes until it looks.
        let mut cache = TileCache::new(&config);
        assert_eq!(cache.disk_usage(), 5_000);
        cache
            .store(
                CacheKey::new("p", TileId::new(12, 9, 0)),
                "png",
                vec![0u8; 100],
            )
            .unwrap();
        assert!(cache.disk_usage() <= 2_500, "{} bytes", cache.disk_usage());
        std::fs::remove_dir_all(config.directory).ok();
    }

    #[test]
    fn stale_tiles_count_as_missing() {
        let mut config = temp_config("age");
        config.max_age_days = 0;
        let mut cache = TileCache::new(&config);
        let key = CacheKey::new("p", TileId::new(9, 1, 1));
        cache.store(key.clone(), "png", vec![7; 4]).unwrap();
        assert!(
            cache.contains(&key, "png"),
            "without a maximum age it stays valid"
        );

        // With a maximum age of "0 days" it stays valid (0 = never expire).
        // A real limit is checked via a cache with a tiny maximum age.
        let mut short_lived = TileCache::new(&config);
        short_lived.max_age = Some(Duration::from_millis(1));
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(!short_lived.contains(&key, "png"), "expired");
        assert!(short_lived.get(&key, "png").is_none());
        std::fs::remove_dir_all(config.directory).ok();
    }

    #[test]
    fn clearing_removes_everything() {
        let config = temp_config("clear");
        let mut cache = TileCache::new(&config);
        let key = CacheKey::new("p", TileId::new(8, 3, 3));
        cache.store(key.clone(), "png", vec![1; 100]).unwrap();
        cache.clear();
        assert!(!cache.contains(&key, "png"));
        assert_eq!(cache.disk_usage(), 0);
    }

    #[test]
    fn provider_name_is_sanitized() {
        assert_eq!(sanitize("a/b:c*d"), "a_b_c_d");
        let cache = TileCache::new(&temp_config("sanitize"));
        let key = CacheKey::new("../evil", TileId::new(1, 0, 0));
        let path = cache.path_for(&key, "png");
        assert!(!path.to_string_lossy().contains(".."), "{}", path.display());
    }
}
