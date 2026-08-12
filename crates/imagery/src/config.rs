//! Konfiguration des Luftbild-Overlays — vollständig aus einer RON-Datei steuerbar.

use crate::tiles::TileId;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Bildformat der Kacheln.
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

/// Wie die Kachel-URL gebildet wird.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TileUrl {
    /// Kachelschema `z/x/y` (Slippy Map, WMTS-RESTful).
    ///
    /// Platzhalter: `{z}` `{x}` `{y}` `{-y}` (TMS-Zählung), `{s}` (Subdomain),
    /// `{key}` (Zugangsschlüssel).
    Template(String),
    /// WMS: die Kachelgrenzen werden als `BBOX` in EPSG:3857 angehängt.
    Wms {
        endpoint: String,
        layers: String,
        #[serde(default = "wms_version")]
        version: String,
        #[serde(default)]
        styles: String,
        /// Zusätzliche Parameter, z. B. `("TRANSPARENT", "TRUE")`.
        #[serde(default)]
        extra: Vec<(String, String)>,
    },
}

fn wms_version() -> String {
    "1.3.0".into()
}

/// Ein Bildanbieter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Provider {
    /// Kurzname zur Auswahl (`active`).
    pub id: String,
    /// Anzeigename.
    pub name: String,
    pub url: TileUrl,
    /// Subdomains für `{s}`.
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
    /// Pflichtangabe des Anbieters — gehört sichtbar ins Bild.
    #[serde(default)]
    pub attribution: String,
    /// Zugangsschlüssel für `{key}`.
    #[serde(default)]
    pub api_key: Option<String>,
    /// Hinweis zu Nutzungsbedingungen.
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
    /// Baut die URL einer Kachel.
    pub fn tile_url(&self, tile: TileId) -> String {
        match &self.url {
            TileUrl::Template(template) => {
                let subdomain = if self.subdomains.is_empty() {
                    String::new()
                } else {
                    // Verteilung über die Subdomains, aber deterministisch — sonst
                    // landet dieselbe Kachel bei jedem Lauf in einem anderen Cache.
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

    /// Zoomstufe auf den Bereich des Anbieters begrenzen.
    pub fn clamp_zoom(&self, zoom: u8) -> u8 {
        zoom.clamp(self.min_zoom, self.max_zoom)
    }
}

/// Woher die Zoomstufe kommt.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ZoomMode {
    /// Feste Stufe.
    Fixed(u8),
    /// Aus der gewünschten Bodenauflösung [m/Pixel] bestimmt.
    Resolution(f64),
}

/// Einstellungen des Kachel-Caches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CacheConfig {
    /// Ablageort der Kacheln.
    pub directory: PathBuf,
    /// Obergrenze des Plattenplatzes [Byte]; 0 = unbegrenzt.
    pub max_bytes: u64,
    /// Wie viele Kacheln zusätzlich im Arbeitsspeicher gehalten werden.
    pub memory_tiles: usize,
    /// Nur aus dem Cache lesen, nichts nachladen.
    pub offline: bool,
    /// Kacheln nach dieser Zeit erneut laden [Tage]; 0 = nie.
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

/// Einstellungen der Abrufe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestConfig {
    /// `User-Agent` — viele Dienste verlangen einen aussagekräftigen Eintrag.
    pub user_agent: String,
    pub timeout_seconds: u64,
    /// Wie viele Kacheln gleichzeitig geladen werden.
    pub parallel: usize,
    pub retries: u32,
}

impl Default for RequestConfig {
    fn default() -> Self {
        Self {
            user_agent: format!("TrainSim-DE-Editor/{}", env!("CARGO_PKG_VERSION")),
            timeout_seconds: 15,
            parallel: 4,
            retries: 2,
        }
    }
}

/// Die vollständige Overlay-Konfiguration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageryConfig {
    /// Overlay überhaupt anzeigen.
    pub enabled: bool,
    /// `id` des aktiven Anbieters.
    pub active: String,
    /// Deckkraft 0…1.
    pub opacity: f32,
    pub zoom: ZoomMode,
    /// Umkreis um die Kamera, für den Kacheln geladen werden [m]. Zusammen mit
    /// `max_tiles` bestimmt er, wie viele Kacheln ein Bild kostet: bei 0,5 m/Pixel ist
    /// eine Kachel rund 90 m breit.
    pub radius: f64,
    /// Obergrenze gleichzeitig sichtbarer Kacheln.
    pub max_tiles: usize,
    /// Manuelle Verschiebung des Bildes gegen die Karte [m] (Ost/Nord) — Luftbilder
    /// sind gegenüber der Gleislage oft um Meter versetzt.
    pub offset: (f64, f64),
    /// Höhe des Overlays über der Schienenoberkante [m]; negativ = darunter.
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
            radius: 600.0,
            max_tiles: 256,
            offset: (0.0, 0.0),
            height_offset: -0.35,
            cache: CacheConfig::default(),
            request: RequestConfig::default(),
            providers: predefined_providers(),
        }
    }
}

impl ImageryConfig {
    /// Lädt die Konfiguration; fehlt die Datei, wird die Vorgabe geschrieben.
    pub fn load_or_create(path: impl Into<PathBuf>) -> (Self, Option<String>) {
        let path = path.into();
        match std::fs::read_to_string(&path) {
            Ok(text) => match ron::from_str::<Self>(&text) {
                Ok(config) => (config, None),
                Err(e) => (
                    Self::default(),
                    Some(format!(
                        "{} nicht lesbar ({e}) — Vorgabe aktiv",
                        path.display()
                    )),
                ),
            },
            Err(_) => {
                let config = Self::default();
                let message = match config.save(&path) {
                    Ok(()) => format!("{} angelegt", path.display()),
                    Err(e) => format!("{} nicht schreibbar: {e}", path.display()),
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

    /// Der aktive Anbieter (oder der erste, falls die `id` unbekannt ist).
    pub fn provider(&self) -> Option<&Provider> {
        self.providers
            .iter()
            .find(|p| p.id == self.active)
            .or_else(|| self.providers.first())
    }

    /// Nächsten Anbieter aktivieren.
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

    /// Zoomstufe für eine Position, begrenzt auf den Bereich des Anbieters.
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

/// Mitgelieferte Anbieter.
///
/// Die URLs sind Ausgangspunkte, keine Zusicherung: Verfügbarkeit und
/// Nutzungsbedingungen jedes Dienstes sind vor dem Einsatz zu prüfen — insbesondere für
/// Massenabrufe. Eigene Anbieter kommen einfach in die Konfigurationsdatei.
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
            api_key: None,
            note: "Nutzungsbedingungen von Esri beachten.".into(),
        },
        Provider {
            id: "bkg_topplus_open".into(),
            name: "BKG TopPlusOpen (Karte)".into(),
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
            api_key: None,
            note: "Offene Daten des BKG; keine Luftbilder, aber gute Referenzkarte.".into(),
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
            attribution: "© OpenStreetMap-Mitwirkende".into(),
            api_key: None,
            note: "Nur für den Editorbetrieb, kein Massenabruf (Tile Usage Policy).".into(),
        },
        Provider {
            id: "dop_wms_vorlage".into(),
            name: "Digitales Orthophoto (WMS-Vorlage)".into(),
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
            api_key: None,
            note: "Vorlage: Endpunkt und Layer des gewünschten DOP-Dienstes eintragen. \
                   Die Länder liefern ihre Orthophotos meist als WMS."
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
            api_key: Some("geheim".into()),
            note: String::new(),
        }
    }

    #[test]
    fn platzhalter_werden_ersetzt() {
        let p = provider("https://x/{z}/{x}/{y}.png?key={key}");
        assert_eq!(
            p.tile_url(TileId::new(14, 8800, 5375)),
            "https://x/14/8800/5375.png?key=geheim"
        );
    }

    #[test]
    fn tms_und_subdomain() {
        let p = provider("https://{s}.x/{z}/{x}/{-y}.png");
        let url = p.tile_url(TileId::new(2, 1, 0));
        assert!(url.ends_with("/2/1/3.png"), "{url}");
        // Deterministische Verteilung: dieselbe Kachel, dieselbe Subdomain.
        assert_eq!(url, p.tile_url(TileId::new(2, 1, 0)));
        assert!(url.starts_with("https://b.x/"), "{url}");
    }

    #[test]
    fn wms_bekommt_bbox_in_mercator_metern() {
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
    fn zoomstufe_bleibt_im_bereich_des_anbieters() {
        let mut config = ImageryConfig {
            providers: vec![provider("{z}")],
            active: "test".into(),
            zoom: ZoomMode::Fixed(22),
            ..Default::default()
        };
        assert_eq!(config.zoom_for(52.0), 18, "auf max_zoom begrenzt");
        config.zoom = ZoomMode::Fixed(0);
        assert_eq!(config.zoom_for(52.0), 2, "auf min_zoom angehoben");
        config.zoom = ZoomMode::Resolution(0.5);
        assert_eq!(config.zoom_for(52.0), 18);
        config.zoom = ZoomMode::Resolution(50.0);
        assert!(config.zoom_for(52.0) < 18);
    }

    #[test]
    fn anbieter_durchschalten() {
        let mut config = ImageryConfig::default();
        let first = config.active.clone();
        assert!(config.provider().is_some());
        config.cycle_provider();
        assert_ne!(config.active, first);
        for _ in 0..config.providers.len() {
            config.cycle_provider();
        }
        assert_ne!(config.active, first, "Durchlauf endet nicht am Anfang");
    }

    #[test]
    fn unbekannter_anbieter_faellt_auf_den_ersten_zurueck() {
        let config = ImageryConfig {
            active: "gibtsnicht".into(),
            ..Default::default()
        };
        assert_eq!(config.provider().unwrap().id, "esri_world_imagery");
    }

    #[test]
    fn konfiguration_ueberlebt_ron() {
        let config = ImageryConfig::default();
        let text = ron::ser::to_string_pretty(&config, ron::ser::PrettyConfig::default()).unwrap();
        let back: ImageryConfig = ron::from_str(&text).expect("RON lesbar");
        assert_eq!(back, config);
        assert!(text.contains("esri_world_imagery"));
    }

    #[test]
    fn eigener_anbieter_aus_ron() {
        let text = r#"(
            enabled: true,
            active: "eigener",
            opacity: 0.5,
            zoom: Fixed(17),
            radius: 400.0,
            max_tiles: 64,
            offset: (2.5, -1.0),
            height_offset: 0.0,
            cache: (
                directory: "eigener/cache",
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
                id: "eigener",
                name: "Eigener Dienst",
                url: Template("https://intern/dop/{z}/{x}/{y}.jpg"),
                max_zoom: 20,
                tile_size: 512,
                format: Jpeg,
                attribution: "Eigene Befliegung",
            )],
        )"#;
        let config: ImageryConfig = ron::from_str(text).expect("RON lesbar");
        assert_eq!(config.provider().unwrap().name, "Eigener Dienst");
        assert_eq!(config.zoom_for(52.0), 17);
        assert!(config.cache.offline);
        assert_eq!(config.offset, (2.5, -1.0));
        assert_eq!(
            config.provider().unwrap().tile_url(TileId::new(17, 1, 2)),
            "https://intern/dop/17/1/2.jpg"
        );
    }

    #[test]
    fn fehlende_datei_wird_angelegt() {
        let dir = std::env::temp_dir().join("trainsim-imagery-config");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("imagery.ron");

        let (config, message) = ImageryConfig::load_or_create(&path);
        assert!(message.unwrap().contains("angelegt"));
        assert!(path.exists());

        // Beim zweiten Mal wird gelesen, nicht überschrieben.
        let (again, message) = ImageryConfig::load_or_create(&path);
        assert!(message.is_none());
        assert_eq!(again, config);
        std::fs::remove_dir_all(dir).ok();
    }
}
