//! Luftbild-Overlay: Kacheln anfordern, als Flächen in die Welt legen, wieder aufräumen.

use bevy::asset::RenderAssetUsages;
use bevy::image::Image;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use glam::DVec3;
use imagery::{DecodedTile, ImageryConfig, ImagerySource, TileId, tiles};
use std::collections::HashMap;
use world_coords::{EcefPos, EnuFrame, RenderOrigin, geo};

/// Eine im Bild liegende Kachel.
#[derive(Component)]
pub struct OverlayTile {
    /// Welche Kachel hier liegt — für Fehlersuche und spätere Neuzuordnung.
    #[allow(dead_code)]
    pub tile: TileId,
    /// Ankerpunkt des lokalen Frames — für das Nachführen beim Origin-Rebase.
    pub anchor: EcefPos,
}

/// Laufzeitzustand des Overlays.
#[derive(Resource)]
pub struct Overlay {
    pub source: ImagerySource,
    /// Welche Kachel hängt an welcher Entität.
    entities: HashMap<TileId, Entity>,
    /// Höhe, auf der das Bild liegt (ellipsoidisch) [m].
    pub base_height: f64,
    /// Zuletzt verwendete Zoomstufe.
    pub zoom: u8,
    /// Meldung für die Anzeige.
    pub status: String,
}

impl Overlay {
    pub fn new(config: ImageryConfig, base_height: f64, status: String) -> Self {
        Self {
            source: ImagerySource::new(config),
            entities: HashMap::new(),
            base_height,
            zoom: 0,
            status,
        }
    }

    pub fn config(&self) -> &ImageryConfig {
        self.source.config()
    }

    pub fn tiles_shown(&self) -> usize {
        self.entities.len()
    }

    /// Konfiguration ändern — alle Kacheln werden neu aufgebaut.
    pub fn apply(&mut self, commands: &mut Commands, config: ImageryConfig) {
        self.clear(commands);
        self.source.set_config(config);
    }

    /// Alle Kachelflächen entfernen.
    pub fn clear(&mut self, commands: &mut Commands) {
        for (_, entity) in self.entities.drain() {
            commands.entity(entity).despawn();
        }
    }
}

/// Ein Bildpunkt je Frame: Kacheln anfordern, fertige einhängen, ferne entfernen.
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    mut overlay: ResMut<Overlay>,
    origin: Res<crate::Origin>,
    focus: Res<crate::Focus>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    if !overlay.config().enabled {
        if overlay.tiles_shown() > 0 {
            overlay.clear(&mut commands);
        }
        return;
    }

    // Sichtbarer Ausschnitt um den Blickpunkt. `from_ecef` liefert Bogenmaß, das
    // Kachelraster rechnet in Grad.
    let (lat, lon) = crate::focus_degrees(focus.position);
    let zoom = overlay.config().zoom_for(lat);
    if zoom != overlay.zoom {
        // Zoomwechsel: die alten Kacheln passen nicht mehr.
        overlay.clear(&mut commands);
        overlay.zoom = zoom;
    }

    let radius = overlay.config().radius.max(50.0);
    let bounds = bounds_around(lat, lon, radius);
    let max_tiles = overlay.config().max_tiles.max(1);
    let wanted = tiles::covering(bounds, zoom, max_tiles);

    // Nicht mehr benötigte Kacheln abräumen.
    let keep: std::collections::HashSet<TileId> = wanted.iter().copied().collect();
    let stale: Vec<TileId> = overlay
        .entities
        .keys()
        .copied()
        .filter(|t| !keep.contains(t))
        .collect();
    for tile in stale {
        if let Some(entity) = overlay.entities.remove(&tile) {
            commands.entity(entity).despawn();
        }
    }

    // Fehlende anfordern.
    for tile in &wanted {
        if !overlay.entities.contains_key(tile) {
            overlay.source.request(*tile);
        }
    }

    // Fertige einhängen.
    let ready = overlay.source.drain();
    if ready.is_empty() {
        return;
    }
    let opacity = overlay.config().opacity.clamp(0.0, 1.0);
    let offset = overlay.config().offset;
    let height = overlay.base_height + overlay.config().height_offset;
    for decoded in ready {
        if !keep.contains(&decoded.tile) || overlay.entities.contains_key(&decoded.tile) {
            continue;
        }
        let entity = spawn_tile(
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut images,
            &origin.0,
            &decoded,
            height,
            offset,
            opacity,
        );
        overlay.entities.insert(decoded.tile, entity);
    }
}

/// Nach einem Origin-Rebase die Kachelflächen neu ausrichten.
pub fn resync<F: bevy::ecs::query::QueryFilter>(
    origin: &RenderOrigin,
    query: &mut Query<(&OverlayTile, &mut Transform), F>,
) {
    for (tile, mut transform) in query.iter_mut() {
        let frame = EnuFrame::at(tile.anchor);
        let (translation, rotation) = origin.frame_transform(&frame);
        transform.translation = translation;
        transform.rotation = rotation;
    }
}

/// Geografischer Ausschnitt um einen Punkt `(west, süd, ost, nord)` in Grad.
fn bounds_around(lat: f64, lon: f64, radius: f64) -> (f64, f64, f64, f64) {
    let d_lat = radius / 111_320.0;
    let d_lon = radius / (111_320.0 * lat.to_radians().cos().abs().max(1e-6));
    (lon - d_lon, lat - d_lat, lon + d_lon, lat + d_lat)
}

/// Legt eine Kachel als texturierte Fläche in die Welt.
#[allow(clippy::too_many_arguments)]
fn spawn_tile(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    origin: &RenderOrigin,
    decoded: &DecodedTile,
    height: f64,
    offset: (f64, f64),
    opacity: f32,
) -> Entity {
    let (west, south, east, north) = decoded.tile.bounds();
    let (clat, clon) = decoded.tile.center();
    let anchor = geo::to_ecef_deg(clat, clon, height);
    let frame = EnuFrame::at(anchor);

    // Eckpunkte im lokalen Frame der Kachel, inklusive der manuellen Verschiebung.
    let corner = |lat: f64, lon: f64| {
        let world = geo::to_ecef_deg(lat, lon, height);
        let local = frame.to_local(world) + DVec3::new(offset.0, offset.1, 0.0);
        [local.x as f32, local.z as f32, -local.y as f32]
    };
    let positions = vec![
        corner(north, west),
        corner(north, east),
        corner(south, east),
        corner(south, west),
    ];
    let uvs = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; 4]);
    mesh.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));

    let texture = images.add(Image::new(
        Extent3d {
            width: decoded.width,
            height: decoded.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        decoded.pixels.clone(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    ));

    let material = materials.add(StandardMaterial {
        base_color_texture: Some(texture),
        base_color: Color::srgba(1.0, 1.0, 1.0, opacity),
        alpha_mode: AlphaMode::Blend,
        // Luftbilder sollen so aussehen wie geliefert, nicht beleuchtet werden.
        unlit: true,
        ..default()
    });

    let (translation, rotation) = origin.frame_transform(&frame);
    commands
        .spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material),
            Transform::from_translation(translation).with_rotation(rotation),
            OverlayTile {
                tile: decoded.tile,
                anchor,
            },
        ))
        .id()
}
