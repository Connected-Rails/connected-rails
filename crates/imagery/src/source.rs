//! Kachelbeschaffung: Cache zuerst, Netz nur wenn nötig — und nie im Hauptthread.

use crate::cache::{CacheKey, CacheStats, TileCache};
use crate::config::ImageryConfig;
use crate::tiles::TileId;
use std::collections::HashSet;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

/// Eine entschlüsselte Kachel, fertig zum Hochladen in die Grafikkarte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedTile {
    pub tile: TileId,
    pub width: u32,
    pub height: u32,
    /// RGBA8, zeilenweise von oben nach unten.
    pub pixels: Vec<u8>,
}

/// Stand einer angefragten Kachel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TileState {
    /// Wird geladen oder entschlüsselt.
    Pending,
    /// Nicht vorhanden und (im Offlinebetrieb) auch nicht beschaffbar.
    Unavailable,
    /// Fertig — wird über [`ImagerySource::drain`] ausgeliefert.
    Ready,
}

/// Ergebnis eines Ladeauftrags.
enum Loaded {
    Tile(Box<DecodedTile>),
    Failed(TileId, String),
}

/// Beschafft Kacheln für den aktiven Anbieter.
pub struct ImagerySource {
    config: ImageryConfig,
    cache: Arc<Mutex<TileCache>>,
    jobs: Option<Sender<Job>>,
    /// Hinter einem Mutex, damit die Quelle als Bevy-Ressource taugt:
    /// `Receiver` ist `Send`, aber nicht `Sync`.
    results: Mutex<Receiver<Loaded>>,
    results_sender: Sender<Loaded>,
    pending: HashSet<TileId>,
    failed: HashSet<TileId>,
    workers: Vec<std::thread::JoinHandle<()>>,
    /// Fehlermeldungen für die Anzeige (jüngste zuletzt).
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

        // Feste Anzahl Arbeitsthreads — mehr Parallelität belastet die Dienste, ohne
        // dass der Editor schneller wird.
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

    /// Konfiguration austauschen (Anbieterwechsel, Zoom, Offline …).
    ///
    /// Wechselt der Anbieter, werden laufende Anfragen verworfen — ihre Kacheln
    /// gehören zum alten Bild.
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
        self.cache.lock().map(|c| c.disk_usage()).unwrap_or(0)
    }

    pub fn clear_cache(&mut self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
        self.pending.clear();
        self.failed.clear();
    }

    /// Kachel anfordern. Liegt sie im Cache, ist sie sofort über [`Self::drain`] da.
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
            // Entschlüsseln kostet Millisekunden — auch das gehört nicht in den Hauptthread.
            std::thread::spawn(move || {
                let _ = sender.send(decode(tile, &bytes));
            });
            return TileState::Pending;
        }

        // 2. Netz — außer im Offlinebetrieb.
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

    /// Fertige Kacheln abholen (nicht blockierend).
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

    /// Wie viele Kacheln gerade unterwegs sind.
    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    /// Fehlgeschlagene Kacheln erneut zulassen.
    pub fn retry_failed(&mut self) {
        self.failed.clear();
        self.errors.clear();
    }
}

impl Drop for ImagerySource {
    fn drop(&mut self) {
        // Sender schließen, damit die Arbeitsthreads ihre Schleife verlassen.
        self.jobs.take();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

/// Einen Ladeauftrag ausführen: herunterladen, ablegen, entschlüsseln.
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
            Ok(_) => last_error = "leere Antwort".into(),
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

/// Bilddaten nach RGBA8 entschlüsseln.
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
        Err(e) => Loaded::Failed(tile, format!("Bild nicht lesbar: {e}")),
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
                // Absichtlich unerreichbar: kein Test darf ins Netz gehen.
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

    /// Wartet, bis eine Kachel fertig ist (oder die Zeit abläuft).
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
    fn kachel_aus_dem_cache_wird_entschluesselt() {
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
    fn offline_geht_nicht_ins_netz() {
        let config = config("offline", true);
        let mut source = ImagerySource::new(config.clone());
        let tile = TileId::new(10, 1, 1);
        assert_eq!(source.request(tile), TileState::Unavailable);
        // Auch beim zweiten Mal — der Fehlschlag ist gemerkt.
        assert_eq!(source.request(tile), TileState::Unavailable);
        assert!(source.drain().is_empty());

        // Nach `retry_failed` wird es erneut versucht (bleibt offline erfolglos).
        source.retry_failed();
        assert_eq!(source.request(tile), TileState::Unavailable);
        std::fs::remove_dir_all(config.cache.directory).ok();
    }

    #[test]
    fn unerreichbarer_dienst_meldet_einen_fehler() {
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
        assert_eq!(source.pending(), 0, "Auftrag muss abgeschlossen werden");
        assert!(!source.errors.is_empty(), "Fehler wird gemeldet");
        assert!(
            source.errors[0].contains("example.invalid"),
            "{:?}",
            source.errors
        );
        // Danach wird die Kachel nicht erneut angefragt.
        assert_eq!(source.request(tile), TileState::Unavailable);
        std::fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn anbieterwechsel_verwirft_laufende_anfragen() {
        let mut config = config("switch", true);
        config.providers.push(Provider {
            id: "zweiter".into(),
            ..config.providers[0].clone()
        });
        let directory = config.cache.directory.clone();
        let mut source = ImagerySource::new(config.clone());
        let tile = TileId::new(9, 2, 3);
        source.request(tile);

        let mut next = config;
        next.active = "zweiter".into();
        source.set_config(next);
        assert_eq!(source.pending(), 0);
        assert_eq!(source.config().active, "zweiter");
        std::fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn cache_leeren_wirkt() {
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
            "offline und leer"
        );
        std::fs::remove_dir_all(config.cache.directory).ok();
    }
}
