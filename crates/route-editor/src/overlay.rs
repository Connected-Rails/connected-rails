//! Aerial imagery overlay: request tiles, place them as quads in the world, clean up again.

use bevy::asset::RenderAssetUsages;
use bevy::image::Image;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use glam::DVec3;
use imagery::{DecodedTile, ImageryConfig, ImagerySource, TileId, tiles};
use std::collections::HashMap;
use world_coords::{EcefPos, EnuFrame, RenderOrigin, geo};

/// A tile placed in the imagery.
#[derive(Component)]
pub struct OverlayTile {
    /// Which tile lies here — for debugging and later re-assignment.
    #[allow(dead_code)]
    pub tile: TileId,
    /// Anchor point of the local frame — for following up on an origin rebase.
    pub anchor: EcefPos,
}

/// Runtime state of the overlay.
#[derive(Resource)]
pub struct Overlay {
    pub source: ImagerySource,
    /// Which tile belongs to which entity.
    entities: HashMap<TileId, Entity>,
    /// Height at which the imagery lies (ellipsoidal) [m].
    pub base_height: f64,
    /// Most recently used zoom level.
    pub zoom: u8,
    /// Message for the display.
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

    /// Change the configuration — all tiles are rebuilt.
    pub fn apply(&mut self, commands: &mut Commands, config: ImageryConfig) {
        self.clear(commands);
        self.source.set_config(config);
    }

    /// Remove all tile quads.
    pub fn clear(&mut self, commands: &mut Commands) {
        for (_, entity) in self.entities.drain() {
            commands.entity(entity).despawn();
        }
    }
}

/// One imagery step per frame: request tiles, attach finished ones, remove distant ones.
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

    // Visible extent around the view point. `from_ecef` returns radians, the
    // tile grid works in degrees.
    let (lat, lon) = crate::focus_degrees(focus.position);
    let zoom = overlay.config().zoom_for(lat);
    if zoom != overlay.zoom {
        // Zoom change: the old tiles no longer fit.
        overlay.clear(&mut commands);
        overlay.zoom = zoom;
    }

    let radius = overlay.config().radius.max(50.0);
    let bounds = bounds_around(lat, lon, radius);
    let max_tiles = overlay.config().max_tiles.max(1);
    let wanted = tiles::covering(bounds, zoom, max_tiles);

    // Clear away tiles that are no longer needed.
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

    // Request the missing ones.
    for tile in &wanted {
        if !overlay.entities.contains_key(tile) {
            overlay.source.request(*tile);
        }
    }

    // Attach the finished ones.
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

/// Re-align the tile quads after an origin rebase.
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

/// Geographic extent around a point `(west, south, east, north)` in degrees.
fn bounds_around(lat: f64, lon: f64, radius: f64) -> (f64, f64, f64, f64) {
    let d_lat = radius / 111_320.0;
    let d_lon = radius / (111_320.0 * lat.to_radians().cos().abs().max(1e-6));
    (lon - d_lon, lat - d_lat, lon + d_lon, lat + d_lat)
}

/// Places a tile as a textured quad in the world.
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

    // Corner points in the local frame of the tile, including the manual offset.
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
    // Counter-clockwise seen from above — the editor camera looks straight
    // down, and a clockwise quad is a backface to it (culled: black viewport).
    mesh.insert_indices(Indices::U32(vec![0, 2, 1, 0, 3, 2]));

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
        // Aerial imagery should look as delivered, not be lit.
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
