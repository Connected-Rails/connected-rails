//! Configuration of the aerial imagery overlay — fully driven by a RON file.

use crate::tiles::TileId;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Image format of the tiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ImageFormat {
    #[default]
    Png,
    Jpeg,
}

impl ImageFormat {
    pub fn extension(self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpg",
        }
    }

    pub fn mime(self) -> &'static str {
        match self {
            ImageFormat::Png => "image/png",
            ImageFormat::Jpeg => "image/jpeg",
        }
    }
}

/// How the tile URL is built.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TileUrl {
    /// Tile scheme `z/x/y` (Slippy Map, WMTS RESTful).
    ///
    /// Placeholders: `{z}` `{x}` `{y}` `{-y}` (TMS counting), `{s}` (subdomain),
    /// `{key}` (access key).
    Template(String),
    /// WMS: the tile bounds are appended as `BBOX` in EPSG:3857.
    Wms {
        endpoint: String,
        layers: String,
        #[serde(default = "wms_version")]
        version: String,
        #[serde(default)]
        styles: String,
        /// Additional parameters, e.g. `("TRANSPARENT", "TRUE")`.
        #[serde(default)]
        extra: Vec<(String, String)>,
    },
}

fn wms_version() -> String {
    "1.3.0".into()
}

/// An imagery provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Provider {
    /// Short name used for selection (`active`).
    pub id: String,
    /// Display name.
    pub name: String,
    pub url: TileUrl,
    /// Subdomains for `{s}`.
    #[serde(default)]
    pub subdomains: Vec<String>,
    #[serde(default)]
    pub min_zoom: u8,
    #[serde(default = "default_max_zoom")]
    pub max_zoom: u8,
    #[serde(default = "default_tile_size")]
    pub tile_size: u32,
    #[serde(default)]
    pub format: ImageFormat,
    /// Mandatory credit of the provider — belongs visibly in the image.
    #[serde(default)]
    pub attribution: String,
    /// Licence or copyright page the credit links to. OpenStreetMap's
    /// attribution guidelines ask for the link, not just the name.
    #[serde(default)]
    pub attribution_url: Option<String>,
    /// Access key for `{key}`.
    #[serde(default)]
    pub api_key: Option<String>,
    /// Note about the terms of use.
    #[serde(default)]
    pub note: String,
}

fn default_max_zoom() -> u8 {
    19
}

fn default_tile_size() -> u32 {
    256
}

impl Provider {
    /// Builds the URL of a tile.
    pub fn tile_url(&self, tile: TileId) -> String {
        match &self.url {
            TileUrl::Template(template) => {
                let subdomain = if self.subdomains.is_empty() {
                    String::new()
                } else {
                    // Spread across the subdomains, but deterministically — otherwise
                    // the same tile ends up in a different cache on every run.
                    let index = (tile.x as usize + tile.y as usize) % self.subdomains.len();
                    self.subdomains[index].clone()
                };
                template
                    .replace("{z}", &tile.z.to_string())
                    .replace("{x}", &tile.x.to_string())
                    .replace("{-y}", &tile.tms_y().to_string())
                    .replace("{y}", &tile.y.to_string())
                    .replace("{s}", &subdomain)
                    .replace("{key}", self.api_key.as_deref().unwrap_or(""))
            }
            TileUrl::Wms {
                endpoint,
                layers,
                version,
                styles,
                extra,
            } => {
                let (west, south, east, north) = tile.bounds_meters();
                let separator = if endpoint.contains('?') { '&' } else { '?' };
                let mut url = format!(
                    "{endpoint}{separator}SERVICE=WMS&REQUEST=GetMap&VERSION={version}\
                     &LAYERS={layers}&STYLES={styles}&FORMAT={}&WIDTH={}&HEIGHT={}\
                     &CRS=EPSG:3857&BBOX={west},{south},{east},{north}",
                    self.format.mime(),
                    self.tile_size,
                    self.tile_size
                );
                for (key, value) in extra {
                    url.push('&');
                    url.push_str(key);
                    url.push('=');
                    url.push_str(value);
                }
                url
            }
        }
    }

    /// Clamp the zoom level to the provider's range.
    pub fn clamp_zoom(&self, zoom: u8) -> u8 {
        zoom.clamp(self.min_zoom, self.max_zoom)
    }
}

/// Where the zoom level comes from.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ZoomMode {
    /// Fixed level.
    Fixed(u8),
    /// Derived from the target ground resolution [m/pixel].
    Resolution(f64),
}

/// Settings of the tile cache.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CacheConfig {
    /// Storage location of the tiles.
    pub directory: PathBuf,
    /// Upper limit of the disk space [bytes]; 0 = unlimited.
    pub max_bytes: u64,
    /// How many tiles are additionally kept in memory.
    pub memory_tiles: usize,
    /// Only read from the cache, fetch nothing.
    pub offline: bool,
    /// Reload tiles after this time [days]; 0 = never.
    pub max_age_days: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            directory: PathBuf::from("cache/imagery"),
            max_bytes: 2 * 1024 * 1024 * 1024,
            memory_tiles: 256,
            offline: false,
            max_age_days: 0,
        }
    }
}

/// Settings of the fetches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestConfig {
    /// `User-Agent` — many services require a meaningful entry.
    pub user_agent: String,
    pub timeout_seconds: u64,
    /// How many tiles are fetched concurrently.
    pub parallel: usize,
    pub retries: u32,
}

impl Default for RequestConfig {
    fn default() -> Self {
        Self {
            user_agent: format!("ConnectedRails-Editor/{}", env!("CARGO_PKG_VERSION")),
            timeout_seconds: 15,
            parallel: 4,
            retries: 2,
        }
    }
}

/// The complete overlay configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageryConfig {
    /// Show the overlay at all.
    pub enabled: bool,
    /// `id` of the active provider.
    pub active: String,
    /// Opacity 0…1.
    pub opacity: f32,
    pub zoom: ZoomMode,
    /// Load radius around the camera for which tiles are fetched [m]. Together with
    /// `max_tiles` it determines how many tiles an image costs: at 0.5 m/pixel a
    /// tile is about 90 m wide.
    pub radius: f64,
    /// Upper limit of simultaneously visible tiles — the hard stop that keeps a
    /// large radius at a high zoom level from piling up thousands of textures.
    /// `covering` cuts north to south, so a limit below what the radius asks for
    /// leaves the southern rows empty rather than shrinking the circle.
    pub max_tiles: usize,
    /// Manual image offset against the map [m] (east/north) — aerial imagery is
    /// often off by metres relative to the track position.
    pub offset: (f64, f64),
    /// Lift of the overlay above the terrain surface [m]. The drape grid is
    /// coarser than the terrain mesh, so without a little air its secants cut
    /// through the ridges in between.
    pub height_offset: f64,
    pub cache: CacheConfig,
    pub request: RequestConfig,
    pub providers: Vec<Provider>,
}

impl Default for ImageryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            active: "esri_world_imagery".into(),
            opacity: 0.8,
            zoom: ZoomMode::Resolution(0.5),
            radius: 1_200.0,
            max_tiles: 1_024,
            offset: (0.0, 0.0),
            height_offset: 1.0,
            cache: CacheConfig::default(),
            request: RequestConfig::default(),
            providers: predefined_providers(),
        }
    }
}

impl ImageryConfig {
    /// Loads the configuration; if the file is missing, the default is written.
    pub fn load_or_create(path: impl Into<PathBuf>) -> (Self, Option<String>) {
        let path = path.into();
        match std::fs::read_to_string(&path) {
            Ok(text) => match ron::from_str::<Self>(&text) {
                Ok(config) => (config, None),
                Err(e) => (
                    Self::default(),
                    Some(i18n::t!(
                        "status-config-unreadable",
                        file = path.display(),
                        error = e
                    )),
                ),
            },
            Err(_) => {
                let config = Self::default();
                let message = match config.save(&path) {
                    Ok(()) => i18n::t!("status-config-created", file = path.display()),
                    Err(e) => {
                        i18n::t!(
                            "status-config-not-writable",
                            file = path.display(),
                            error = e
                        )
                    }
                };
                (config, Some(message))
            }
        }
    }

    pub fn save(&self, path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        std::fs::write(path, text)
    }

    /// The active provider (or the first one, if the `id` is unknown).
    pub fn provider(&self) -> Option<&Provider> {
        self.providers
            .iter()
            .find(|p| p.id == self.active)
            .or_else(|| self.providers.first())
    }

    /// Activate the next provider.
    pub fn cycle_provider(&mut self) {
        if self.providers.is_empty() {
            return;
        }
        let current = self
            .providers
            .iter()
            .position(|p| p.id == self.active)
            .unwrap_or(0);
        self.active = self.providers[(current + 1) % self.providers.len()]
            .id
            .clone();
    }

    /// Zoom level for a position, clamped to the provider's range.
    pub fn zoom_for(&self, lat: f64) -> u8 {
        let Some(provider) = self.provider() else {
            return 0;
        };
        let zoom = match self.zoom {
            ZoomMode::Fixed(z) => z,
            ZoomMode::Resolution(meters) => crate::tiles::zoom_for_resolution(
                lat,
                meters.max(0.01),
                provider.tile_size,
                provider.max_zoom,
            ),
        };
        provider.clamp_zoom(zoom)
    }
}

/// Bundled providers.
///
/// The URLs are starting points, not a guarantee: availability and terms of use of
/// every service have to be checked before use — especially for bulk fetching.
/// Custom providers simply go into the configuration file.
pub fn predefined_providers() -> Vec<Provider> {
    vec![
        Provider {
            id: "esri_world_imagery".into(),
            name: "Esri World Imagery".into(),
            url: TileUrl::Template(
                "https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/\
                 MapServer/tile/{z}/{y}/{x}"
                    .into(),
            ),
            subdomains: Vec::new(),
            min_zoom: 0,
            max_zoom: 19,
            tile_size: 256,
            format: ImageFormat::Jpeg,
            attribution: "Esri, Maxar, Earthstar Geographics".into(),
            attribution_url: None,
            api_key: None,
            note: "Observe the Esri terms of use.".into(),
        },
        Provider {
            id: "bkg_topplus_open".into(),
            name: "BKG TopPlusOpen (map)".into(),
            url: TileUrl::Template(
                "https://sgx.geodatenzentrum.de/wmts_topplus_open/tile/1.0.0/web/\
                 default/WEBMERCATOR/{z}/{y}/{x}.png"
                    .into(),
            ),
            subdomains: Vec::new(),
            min_zoom: 0,
            max_zoom: 18,
            tile_size: 256,
            format: ImageFormat::Png,
            attribution: "© Bundesamt für Kartographie und Geodäsie".into(),
            attribution_url: None,
            api_key: None,
            note: "Open data from the BKG; no aerial imagery, but a good reference map.".into(),
        },
        Provider {
            id: "osm_standard".into(),
            name: "OpenStreetMap".into(),
            url: TileUrl::Template("https://tile.openstreetmap.org/{z}/{x}/{y}.png".into()),
            subdomains: Vec::new(),
            min_zoom: 0,
            max_zoom: 19,
            tile_size: 256,
            format: ImageFormat::Png,
            attribution: "© OpenStreetMap contributors".into(),
            attribution_url: Some("https://www.openstreetmap.org/copyright".into()),
            api_key: None,
            note: "Editor use only, no bulk fetching (Tile Usage Policy).".into(),
        },
        Provider {
            id: "dop_wms_vorlage".into(),
            name: "Digital orthophoto (WMS template)".into(),
            url: TileUrl::Wms {
                endpoint: "https://example.invalid/dop/wms".into(),
                layers: "dop".into(),
                version: wms_version(),
                styles: String::new(),
                extra: vec![("TRANSPARENT".into(), "FALSE".into())],
            },
            subdomains: Vec::new(),
            min_zoom: 10,
            max_zoom: 20,
            tile_size: 512,
            format: ImageFormat::Jpeg,
            attribution: "Landesvermessungsamt".into(),
            attribution_url: None,
            api_key: None,
            note: "Template: enter the endpoint and layer of the desired DOP service. \
                   The German states usually serve their orthophotos as WMS."
                .into(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(template: &str) -> Provider {
        Provider {
            id: "test".into(),
            name: "Test".into(),
            url: TileUrl::Template(template.into()),
            subdomains: vec!["a".into(), "b".into()],
            min_zoom: 2,
            max_zoom: 18,
            tile_size: 256,
            format: ImageFormat::Png,
            attribution: String::new(),
            attribution_url: None,
            api_key: Some("secret".into()),
            note: String::new(),
        }
    }

    #[test]
    fn placeholders_are_replaced() {
        let p = provider("https://x/{z}/{x}/{y}.png?key={key}");
        assert_eq!(
            p.tile_url(TileId::new(14, 8800, 5375)),
            "https://x/14/8800/5375.png?key=secret"
        );
    }

    #[test]
    fn tms_and_subdomain() {
        let p = provider("https://{s}.x/{z}/{x}/{-y}.png");
        let url = p.tile_url(TileId::new(2, 1, 0));
        assert!(url.ends_with("/2/1/3.png"), "{url}");
        // Deterministic distribution: same tile, same subdomain.
        assert_eq!(url, p.tile_url(TileId::new(2, 1, 0)));
        assert!(url.starts_with("https://b.x/"), "{url}");
    }

    #[test]
    fn wms_gets_bbox_in_mercator_metres() {
        let p = Provider {
            url: TileUrl::Wms {
                endpoint: "https://x/wms?token=1".into(),
                layers: "dop20".into(),
                version: "1.3.0".into(),
                styles: String::new(),
                extra: vec![("TRANSPARENT".into(), "TRUE".into())],
            },
            tile_size: 512,
            format: ImageFormat::Jpeg,
            ..provider("")
        };
        let url = p.tile_url(TileId::new(0, 0, 0));
        assert!(url.contains("&SERVICE=WMS"), "{url}");
        assert!(url.contains("LAYERS=dop20"));
        assert!(url.contains("WIDTH=512&HEIGHT=512"));
        assert!(url.contains("CRS=EPSG:3857"));
        assert!(url.contains("FORMAT=image/jpeg"));
        assert!(url.contains("TRANSPARENT=TRUE"));
        assert!(url.contains("BBOX=-20037508"), "{url}");
    }

    #[test]
    fn zoom_level_stays_within_the_provider_range() {
        let mut config = ImageryConfig {
            providers: vec![provider("{z}")],
            active: "test".into(),
            zoom: ZoomMode::Fixed(22),
            ..Default::default()
        };
        assert_eq!(config.zoom_for(52.0), 18, "clamped to max_zoom");
        config.zoom = ZoomMode::Fixed(0);
        assert_eq!(config.zoom_for(52.0), 2, "raised to min_zoom");
        config.zoom = ZoomMode::Resolution(0.5);
        assert_eq!(config.zoom_for(52.0), 18);
        config.zoom = ZoomMode::Resolution(50.0);
        assert!(config.zoom_for(52.0) < 18);
    }

    #[test]
    fn cycle_through_providers() {
        let mut config = ImageryConfig::default();
        let first = config.active.clone();
        assert!(config.provider().is_some());
        config.cycle_provider();
        assert_ne!(config.active, first);
        for _ in 0..config.providers.len() {
            config.cycle_provider();
        }
        assert_ne!(config.active, first, "cycle does not end at the beginning");
    }

    #[test]
    fn unknown_provider_falls_back_to_the_first_one() {
        let config = ImageryConfig {
            active: "does_not_exist".into(),
            ..Default::default()
        };
        assert_eq!(config.provider().unwrap().id, "esri_world_imagery");
    }

    #[test]
    fn configuration_survives_ron() {
        let config = ImageryConfig::default();
        let text = ron::ser::to_string_pretty(&config, ron::ser::PrettyConfig::default()).unwrap();
        let back: ImageryConfig = ron::from_str(&text).expect("RON readable");
        assert_eq!(back, config);
        assert!(text.contains("esri_world_imagery"));
    }

    #[test]
    fn custom_provider_from_ron() {
        let text = r#"(
            enabled: true,
            active: "custom",
            opacity: 0.5,
            zoom: Fixed(17),
            radius: 400.0,
            max_tiles: 64,
            offset: (2.5, -1.0),
            height_offset: 0.0,
            cache: (
                directory: "custom/cache",
                max_bytes: 1000000,
                memory_tiles: 32,
                offline: true,
                max_age_days: 30,
            ),
            request: (
                user_agent: "Test/1.0",
                timeout_seconds: 5,
                parallel: 2,
                retries: 0,
            ),
            providers: [(
                id: "custom",
                name: "Custom service",
                url: Template("https://intern/dop/{z}/{x}/{y}.jpg"),
                max_zoom: 20,
                tile_size: 512,
                format: Jpeg,
                attribution: "Own aerial survey",
            )],
        )"#;
        let config: ImageryConfig = ron::from_str(text).expect("RON readable");
        assert_eq!(config.provider().unwrap().name, "Custom service");
        assert_eq!(config.zoom_for(52.0), 17);
        assert!(config.cache.offline);
        assert_eq!(config.offset, (2.5, -1.0));
        assert_eq!(
            config.provider().unwrap().tile_url(TileId::new(17, 1, 2)),
            "https://intern/dop/17/1/2.jpg"
        );
    }

    #[test]
    fn missing_file_is_created() {
        let dir = std::env::temp_dir().join("trainsim-imagery-config");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("imagery.ron");

        let (config, message) = ImageryConfig::load_or_create(&path);
        assert!(message.unwrap().contains("imagery.ron"));
        assert!(path.exists());

        // The second time it is read, not overwritten.
        let (again, message) = ImageryConfig::load_or_create(&path);
        assert!(message.is_none());
        assert_eq!(again, config);
        std::fs::remove_dir_all(dir).ok();
    }
}
