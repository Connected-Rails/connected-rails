//! Tile fetching: cache first, network only when needed — and never on the main thread.

use crate::cache::{CacheKey, CacheStats, TileCache};
use crate::config::ImageryConfig;
use crate::tiles::TileId;
use std::collections::HashSet;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

/// A decoded tile, ready to be uploaded to the graphics card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedTile {
    pub tile: TileId,
    pub width: u32,
    pub height: u32,
    /// RGBA8, row by row from top to bottom.
    pub pixels: Vec<u8>,
}

/// State of a requested tile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TileState {
    /// Being fetched or decoded.
    Pending,
    /// Not present and (in offline mode) not obtainable either.
    Unavailable,
    /// Done — delivered via [`ImagerySource::drain`].
    Ready,
}

/// Result of a load job.
enum Loaded {
    Tile(Box<DecodedTile>),
    Failed(TileId, String),
}

/// Fetches tiles for the active provider.
pub struct ImagerySource {
    config: ImageryConfig,
    cache: Arc<Mutex<TileCache>>,
    jobs: Option<Sender<Job>>,
    /// Behind a mutex so the source works as a Bevy resource:
    /// `Receiver` is `Send`, but not `Sync`.
    results: Mutex<Receiver<Loaded>>,
    results_sender: Sender<Loaded>,
    pending: HashSet<TileId>,
    failed: HashSet<TileId>,
    workers: Vec<std::thread::JoinHandle<()>>,
    /// Error messages for display (newest last).
    pub errors: Vec<String>,
}

struct Job {
    url: String,
    key: CacheKey,
    extension: &'static str,
    tile: TileId,
    user_agent: String,
    timeout: std::time::Duration,
    retries: u32,
    cache: Arc<Mutex<TileCache>>,
    results: Sender<Loaded>,
}

impl ImagerySource {
    pub fn new(config: ImageryConfig) -> Self {
        let cache = Arc::new(Mutex::new(TileCache::new(&config.cache)));
        let (results_sender, results) = channel();
        let (job_sender, job_receiver) = channel::<Job>();
        let job_receiver = Arc::new(Mutex::new(job_receiver));

        // Fixed number of worker threads — more concurrency loads the services without
        // making the editor any faster.
        let mut workers = Vec::new();
        for _ in 0..config.request.parallel.clamp(1, 16) {
            let receiver = job_receiver.clone();
            workers.push(std::thread::spawn(move || {
                loop {
                    let job = {
                        let Ok(guard) = receiver.lock() else { break };
                        match guard.recv() {
                            Ok(job) => job,
                            Err(_) => break,
                        }
                    };
                    run_job(job);
                }
            }));
        }

        Self {
            config,
            cache,
            jobs: Some(job_sender),
            results: Mutex::new(results),
            results_sender,
            pending: HashSet::new(),
            failed: HashSet::new(),
            workers,
            errors: Vec::new(),
        }
    }

    pub fn config(&self) -> &ImageryConfig {
        &self.config
    }

    /// Replace the configuration (provider switch, zoom, offline …).
    ///
    /// If the provider changes, in-flight requests are discarded — their tiles
    /// belong to the old imagery.
    pub fn set_config(&mut self, config: ImageryConfig) {
        let provider_changed = config.active != self.config.active;
        let cache_changed = config.cache != self.config.cache;
        self.config = config;
        if provider_changed {
            self.pending.clear();
            self.failed.clear();
        }
        if cache_changed {
            self.cache = Arc::new(Mutex::new(TileCache::new(&self.config.cache)));
        }
    }

    pub fn cache_stats(&self) -> CacheStats {
        self.cache.lock().map(|c| c.stats()).unwrap_or_default()
    }

    pub fn disk_usage(&self) -> u64 {
        self.cache.lock().map(|mut c| c.disk_usage()).unwrap_or(0)
    }

    pub fn clear_cache(&mut self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
        self.pending.clear();
        self.failed.clear();
    }

    /// Request a tile. If cached, it arrives immediately via [`Self::drain`].
    pub fn request(&mut self, tile: TileId) -> TileState {
        let Some(provider) = self.config.provider() else {
            return TileState::Unavailable;
        };
        if self.pending.contains(&tile) {
            return TileState::Pending;
        }
        if self.failed.contains(&tile) {
            return TileState::Unavailable;
        }

        let key = CacheKey::new(&provider.id, tile);
        let extension = provider.format.extension();

        // 1. Cache.
        let cached = self
            .cache
            .lock()
            .ok()
            .and_then(|mut cache| cache.get(&key, extension));
        if let Some(bytes) = cached {
            self.pending.insert(tile);
            let sender = self.results_sender.clone();
            // Decoding costs milliseconds — that does not belong on the main thread either.
            std::thread::spawn(move || {
                let _ = sender.send(decode(tile, &bytes));
            });
            return TileState::Pending;
        }

        // 2. Network — except in offline mode.
        if self.config.cache.offline {
            self.failed.insert(tile);
            return TileState::Unavailable;
        }
        let Some(jobs) = &self.jobs else {
            return TileState::Unavailable;
        };
        let job = Job {
            url: provider.tile_url(tile),
            key,
            extension: match provider.format {
                crate::config::ImageFormat::Png => "png",
                crate::config::ImageFormat::Jpeg => "jpg",
            },
            tile,
            user_agent: self.config.request.user_agent.clone(),
            timeout: std::time::Duration::from_secs(self.config.request.timeout_seconds.max(1)),
            retries: self.config.request.retries,
            cache: self.cache.clone(),
            results: self.results_sender.clone(),
        };
        if jobs.send(job).is_ok() {
            self.pending.insert(tile);
            TileState::Pending
        } else {
            TileState::Unavailable
        }
    }

    /// Collect finished tiles (non-blocking).
    pub fn drain(&mut self) -> Vec<DecodedTile> {
        let mut ready = Vec::new();
        let Ok(results) = self.results.lock() else {
            return ready;
        };
        let mut incoming = Vec::new();
        while let Ok(result) = results.try_recv() {
            incoming.push(result);
        }
        drop(results);
        for result in incoming {
            match result {
                Loaded::Tile(tile) => {
                    self.pending.remove(&tile.tile);
                    ready.push(*tile);
                }
                Loaded::Failed(tile, error) => {
                    self.pending.remove(&tile);
                    self.failed.insert(tile);
                    if self.errors.last().map(String::as_str) != Some(error.as_str()) {
                        self.errors.push(error);
                    }
                    if self.errors.len() > 8 {
                        self.errors.remove(0);
                    }
                }
            }
        }
        ready
    }

    /// How many tiles are currently in flight.
    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    /// Allow failed tiles to be requested again.
    pub fn retry_failed(&mut self) {
        self.failed.clear();
        self.errors.clear();
    }
}

impl Drop for ImagerySource {
    fn drop(&mut self) {
        // Close the sender so the worker threads leave their loop.
        self.jobs.take();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

/// Run a load job: download, store, decode.
fn run_job(job: Job) {
    let mut last_error = String::new();
    for attempt in 0..=job.retries {
        match download(&job.url, &job.user_agent, job.timeout) {
            Ok(bytes) if !bytes.is_empty() => {
                if let Ok(mut cache) = job.cache.lock() {
                    let _ = cache.store(job.key.clone(), job.extension, bytes.clone());
                }
                let _ = job.results.send(decode(job.tile, &bytes));
                return;
            }
            Ok(_) => last_error = "empty response".into(),
            Err(e) => last_error = e,
        }
        if attempt < job.retries {
            std::thread::sleep(std::time::Duration::from_millis(200 * (attempt as u64 + 1)));
        }
    }
    let _ = job.results.send(Loaded::Failed(
        job.tile,
        format!("{}: {last_error}", job.url),
    ));
}

fn download(url: &str, user_agent: &str, timeout: std::time::Duration) -> Result<Vec<u8>, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .user_agent(user_agent)
        .build()
        .into();
    let mut response = agent.get(url).call().map_err(|e| e.to_string())?;
    let mut body = Vec::new();
    std::io::copy(&mut response.body_mut().as_reader(), &mut body).map_err(|e| e.to_string())?;
    Ok(body)
}

/// Decode image data to RGBA8.
fn decode(tile: TileId, bytes: &[u8]) -> Loaded {
    match image::load_from_memory(bytes) {
        Ok(image) => {
            let rgba = image.to_rgba8();
            Loaded::Tile(Box::new(DecodedTile {
                tile,
                width: rgba.width(),
                height: rgba.height(),
                pixels: rgba.into_raw(),
            }))
        }
        Err(e) => Loaded::Failed(tile, format!("image not readable: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CacheConfig, ImageFormat, Provider, TileUrl};

    fn test_png(color: [u8; 4]) -> Vec<u8> {
        let mut image = image::RgbaImage::new(4, 4);
        for pixel in image.pixels_mut() {
            *pixel = image::Rgba(color);
        }
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }

    fn config(name: &str, offline: bool) -> ImageryConfig {
        let dir = std::env::temp_dir().join(format!("trainsim-source-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        ImageryConfig {
            active: "test".into(),
            providers: vec![Provider {
                id: "test".into(),
                name: "Test".into(),
                // Deliberately unreachable: no test may go to the network.
                url: TileUrl::Template("https://example.invalid/{z}/{x}/{y}.png".into()),
                subdomains: Vec::new(),
                min_zoom: 0,
                max_zoom: 20,
                tile_size: 256,
                format: ImageFormat::Png,
                attribution: String::new(),
                api_key: None,
                note: String::new(),
            }],
            cache: CacheConfig {
                directory: dir,
                max_bytes: 0,
                memory_tiles: 8,
                offline,
                max_age_days: 0,
            },
            ..Default::default()
        }
    }

    /// Waits until a tile is ready (or the time runs out).
    fn wait(source: &mut ImagerySource) -> Vec<DecodedTile> {
        for _ in 0..200 {
            let ready = source.drain();
            if !ready.is_empty() || source.pending() == 0 {
                return ready;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        Vec::new()
    }

    #[test]
    fn tile_from_the_cache_is_decoded() {
        let config = config("cached", true);
        let tile = TileId::new(14, 100, 200);
        {
            let mut cache = TileCache::new(&config.cache);
            cache
                .store(
                    CacheKey::new("test", tile),
                    "png",
                    test_png([10, 20, 30, 255]),
                )
                .unwrap();
        }

        let mut source = ImagerySource::new(config.clone());
        assert_eq!(source.request(tile), TileState::Pending);
        let ready = wait(&mut source);
        assert_eq!(ready.len(), 1);
        assert_eq!((ready[0].width, ready[0].height), (4, 4));
        assert_eq!(&ready[0].pixels[..4], &[10, 20, 30, 255]);
        assert_eq!(source.pending(), 0);
        std::fs::remove_dir_all(config.cache.directory).ok();
    }

    #[test]
    fn offline_does_not_go_to_the_network() {
        let config = config("offline", true);
        let mut source = ImagerySource::new(config.clone());
        let tile = TileId::new(10, 1, 1);
        assert_eq!(source.request(tile), TileState::Unavailable);
        // The second time as well — the failed attempt is remembered.
        assert_eq!(source.request(tile), TileState::Unavailable);
        assert!(source.drain().is_empty());

        // After `retry_failed` it is tried again (stays unsuccessful while offline).
        source.retry_failed();
        assert_eq!(source.request(tile), TileState::Unavailable);
        std::fs::remove_dir_all(config.cache.directory).ok();
    }

    #[test]
    fn unreachable_service_reports_an_error() {
        let mut config = config("unreachable", false);
        config.request.retries = 0;
        config.request.timeout_seconds = 1;
        let directory = config.cache.directory.clone();
        let mut source = ImagerySource::new(config);

        let tile = TileId::new(12, 5, 5);
        assert_eq!(source.request(tile), TileState::Pending);
        for _ in 0..300 {
            source.drain();
            if source.pending() == 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(source.pending(), 0, "job must be completed");
        assert!(!source.errors.is_empty(), "error is reported");
        assert!(
            source.errors[0].contains("example.invalid"),
            "{:?}",
            source.errors
        );
        // Afterwards the tile is not requested again.
        assert_eq!(source.request(tile), TileState::Unavailable);
        std::fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn provider_switch_discards_in_flight_requests() {
        let mut config = config("switch", true);
        config.providers.push(Provider {
            id: "second".into(),
            ..config.providers[0].clone()
        });
        let directory = config.cache.directory.clone();
        let mut source = ImagerySource::new(config.clone());
        let tile = TileId::new(9, 2, 3);
        source.request(tile);

        let mut next = config;
        next.active = "second".into();
        source.set_config(next);
        assert_eq!(source.pending(), 0);
        assert_eq!(source.config().active, "second");
        std::fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn clearing_the_cache_takes_effect() {
        let config = config("clear", true);
        let tile = TileId::new(11, 4, 4);
        {
            let mut cache = TileCache::new(&config.cache);
            cache
                .store(CacheKey::new("test", tile), "png", test_png([1, 2, 3, 255]))
                .unwrap();
        }
        let mut source = ImagerySource::new(config.clone());
        assert!(source.disk_usage() > 0);
        source.clear_cache();
        assert_eq!(source.disk_usage(), 0);
        assert_eq!(
            source.request(tile),
            TileState::Unavailable,
            "offline and empty"
        );
        std::fs::remove_dir_all(config.cache.directory).ok();
    }
}
