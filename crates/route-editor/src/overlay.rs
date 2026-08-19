//! Aerial imagery overlay: request tiles, place them in the world, clean up again.
//!
//! A tile is a grid draped over the ground the terrain builder reports, so the
//! photo follows the relief instead of cutting through it. The heights are
//! sampled on a worker thread — the builder's lock is held by a tile build for
//! tens of milliseconds, and a few hundred samples per tile is not work for the
//! frame.

use bevy::asset::RenderAssetUsages;
use bevy::image::Image;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use content::terrain::TerrainBuilder;
use glam::DVec3;
use imagery::{DecodedTile, ImageryConfig, ImagerySource, TileId, tiles};
use std::collections::HashMap;
use world_coords::{EcefPos, EnuFrame, RenderOrigin, geo};

/// Grid spacing the imagery is draped at [m]. Fine enough to follow the
/// embankment the terrain builder puts under the track, coarse enough that a
/// tile stays a few hundred height lookups rather than a few thousand.
const DRAPE_STEP: f64 = 8.0;
/// Height grids sampled at the same time. They queue behind the same builder
/// lock the terrain tiles use, so asking for more only starves those.
const MAX_DRAPING: usize = 8;

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
    /// Decoded tiles waiting for a height grid to be sampled for them.
    waiting: HashMap<TileId, DecodedTile>,
    /// Height grids being sampled, with the tile they belong to.
    draping: HashMap<TileId, (DecodedTile, Task<Vec<f64>>)>,
    /// Most recently used zoom level.
    pub zoom: u8,
    /// Message for the display.
    pub status: String,
}

impl Overlay {
    pub fn new(config: ImageryConfig, status: String) -> Self {
        Self {
            source: ImagerySource::new(config),
            entities: HashMap::new(),
            waiting: HashMap::new(),
            draping: HashMap::new(),
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

    /// Is this tile placed, or on its way there? The one question the request
    /// loop asks. A tile that has been decoded but still waits for its height
    /// grid is already gone from the source's own `pending`, so asking only
    /// about the placed ones re-fetches, re-decodes and re-queues it on every
    /// frame it waits — hundreds of them, every frame.
    fn has(&self, tile: TileId) -> bool {
        self.entities.contains_key(&tile)
            || self.waiting.contains_key(&tile)
            || self.draping.contains_key(&tile)
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
        self.waiting.clear();
        self.draping.clear();
    }
}

/// One imagery step per frame: request tiles, attach finished ones, remove distant ones.
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    mut overlay: ResMut<Overlay>,
    origin: Res<crate::Origin>,
    focus: Res<crate::Focus>,
    terrain: Res<crate::terrain::TerrainView>,
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

    // Visible extent around the camera, not around the pivot it looks at: in
    // the 3D view the pivot runs off towards the horizon, and the tiles would
    // load ahead of the camera while the ground under it stays empty.
    // `from_ecef` returns radians, the tile grid works in degrees.
    let (lat, lon) = crate::focus_degrees(focus.camera_pos());
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
    overlay.draping.retain(|tile, _| keep.contains(tile));

    // Request the missing ones.
    for tile in &wanted {
        if !overlay.has(*tile) {
            overlay.source.request(*tile);
        }
    }

    // Decoded tiles queue up for a height grid.
    let fresh = overlay.source.drain();
    let overlay = &mut *overlay;
    overlay
        .waiting
        .extend(fresh.into_iter().map(|d| (d.tile, d)));
    overlay.waiting.retain(|tile, _| keep.contains(tile));

    // Sample the grids on worker threads. The builder is only there once the
    // terrain has been set up; until then the tiles wait in the queue.
    let lift = overlay.config().height_offset;
    if let Some(builder) = terrain.builder_arc() {
        let pool = AsyncComputeTaskPool::get();
        let free = MAX_DRAPING.saturating_sub(overlay.draping.len());
        let next: Vec<TileId> = overlay.waiting.keys().copied().take(free).collect();
        for tile in next {
            let Some(decoded) = overlay.waiting.remove(&tile) else {
                continue;
            };
            let builder = builder.clone();
            let task = pool.spawn(async move {
                let mut builder = builder.lock().expect("terrain builder");
                height_grid(tile, drape_segments(tile), lift, &mut builder)
            });
            overlay.draping.insert(tile, (decoded, task));
        }
    }

    // Place what has its heights.
    let done: Vec<TileId> = overlay
        .draping
        .iter_mut()
        .filter(|(_, (_, task))| task.is_finished())
        .map(|(tile, _)| *tile)
        .collect();
    if done.is_empty() {
        return;
    }
    let opacity = overlay.config().opacity.clamp(0.0, 1.0);
    let offset = overlay.config().offset;
    for tile in done {
        let Some((decoded, mut task)) = overlay.draping.remove(&tile) else {
            continue;
        };
        let Some(heights) = block_on(poll_once(&mut task)) else {
            continue;
        };
        if !keep.contains(&tile) || overlay.entities.contains_key(&tile) {
            continue;
        }
        let entity = spawn_tile(
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut images,
            &origin.0,
            &decoded,
            &heights,
            offset,
            opacity,
        );
        overlay.entities.insert(tile, entity);
    }
}

/// Ellipsoidal heights of a tile's drape grid, row by row from the north edge —
/// the shape the terrain builder reports, lifted clear of it.
fn height_grid(tile: TileId, segments: usize, lift: f64, builder: &mut TerrainBuilder) -> Vec<f64> {
    let (west, south, east, north) = tile.bounds();
    let n = segments.max(1);
    let mut heights = Vec::with_capacity((n + 1) * (n + 1));
    for row in 0..=n {
        let lat = north + (south - north) * row as f64 / n as f64;
        for col in 0..=n {
            let lon = west + (east - west) * col as f64 / n as f64;
            heights.push(builder.surface_height(geo::to_ecef_deg(lat, lon, 0.0)) + lift);
        }
    }
    heights
}

/// Triangle indices of the drape grid, row-major from the north edge: two
/// triangles per cell, counter-clockwise seen from above. The editor camera
/// looks straight down, and a clockwise quad is a backface to it — wound the
/// other way every tile is culled and the viewport goes black.
fn grid_indices(n: usize) -> Vec<u32> {
    let mut indices = Vec::with_capacity(n * n * 6);
    for row in 0..n {
        for col in 0..n {
            let north_west = (row * (n + 1) + col) as u32;
            let north_east = north_west + 1;
            let south_west = north_west + (n + 1) as u32;
            let south_east = south_west + 1;
            indices.extend_from_slice(&[
                north_west, south_east, north_east, north_west, south_west, south_east,
            ]);
        }
    }
    indices
}

/// How many segments a tile is cut into per axis: about one vertex every
/// [`DRAPE_STEP`] metres. Capped — a low zoom level makes a tile kilometres
/// wide, and every vertex is a height lookup.
fn drape_segments(tile: TileId) -> usize {
    let (west, _, east, _) = tile.bounds();
    let (lat, _) = tile.center();
    let width = (east - west) * 111_320.0 * lat.to_radians().cos().abs();
    ((width / DRAPE_STEP).ceil() as usize).clamp(1, 32)
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

/// Places a tile in the world: a `segments`×`segments` grid whose vertices sit
/// at the height `height_at` gives for them. One segment is the flat quad the
/// map view has always drawn.
#[allow(clippy::too_many_arguments)]
fn spawn_tile(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    origin: &RenderOrigin,
    decoded: &DecodedTile,
    heights: &[f64],
    offset: (f64, f64),
    opacity: f32,
) -> Entity {
    let (west, south, east, north) = decoded.tile.bounds();
    let n = drape_segments(decoded.tile).max(1);
    let count = (n + 1) * (n + 1);
    let (clat, clon) = decoded.tile.center();
    // The anchor carries the tile's own frame, so it belongs at its middle —
    // the grid's centre vertex on an even count, its average otherwise.
    let middle = heights.get(count / 2).copied().unwrap_or_default();
    let anchor = geo::to_ecef_deg(clat, clon, middle);
    let frame = EnuFrame::at(anchor);

    // Grid points in the local frame of the tile, row by row from the north
    // edge southwards, including the manual offset.
    let mut positions = Vec::with_capacity(count);
    let mut uvs = Vec::with_capacity(count);
    for row in 0..=n {
        let v = row as f64 / n as f64;
        let lat = north + (south - north) * v;
        for col in 0..=n {
            let u = col as f64 / n as f64;
            let lon = west + (east - west) * u;
            let height = heights.get(row * (n + 1) + col).copied().unwrap_or(middle);
            let world = geo::to_ecef_deg(lat, lon, height);
            let local = frame.to_local(world) + DVec3::new(offset.0, offset.1, 0.0);
            positions.push([local.x as f32, local.z as f32, -local.y as f32]);
            uvs.push([u as f32, v as f32]);
        }
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    // The material is unlit, so the normal is never shaded — it only has to be
    // there for the vertex layout.
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; count]);
    mesh.insert_indices(Indices::U32(grid_indices(n)));

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

#[cfg(test)]
mod tests {
    use super::*;

    /// The bug this cost twice: a decoded tile waiting for its height grid is
    /// gone from the source's `pending`, so a request loop that only knows the
    /// placed tiles asks for it again — every frame, for hundreds of tiles.
    #[test]
    fn a_tile_waiting_for_its_heights_counts_as_taken_care_of() {
        let mut overlay = Overlay::new(ImageryConfig::default(), String::new());
        let tile = TileId::new(18, 1, 2);
        assert!(!overlay.has(tile), "nothing knows it yet");
        overlay.waiting.insert(
            tile,
            DecodedTile {
                tile,
                width: 1,
                height: 1,
                pixels: vec![0; 4],
            },
        );
        assert!(overlay.has(tile), "queued for its heights, not missing");
    }

    /// One segment has to stay exactly the quad the map has always drawn, or
    /// every tile turns its back to the camera and the viewport goes black.
    #[test]
    fn a_single_segment_is_the_old_quad() {
        // Row-major: 0 north-west, 1 north-east, 2 south-west, 3 south-east.
        assert_eq!(grid_indices(1), vec![0, 3, 1, 0, 2, 3]);
    }

    /// Every cell of a larger grid winds the same way as that quad, and no
    /// index points outside the grid.
    #[test]
    fn every_cell_of_the_grid_winds_alike() {
        let n = 3;
        let indices = grid_indices(n);
        assert_eq!(indices.len(), n * n * 6);
        assert!(indices.iter().all(|i| (*i as usize) < (n + 1) * (n + 1)));
        // Signed area over the (column, row) coordinates: same sign for all of
        // them means the same winding for all of them.
        for triangle in indices.chunks(3) {
            let at = |i: u32| ((i as usize % (n + 1)) as f64, (i as usize / (n + 1)) as f64);
            let (ax, ay) = at(triangle[0]);
            let (bx, by) = at(triangle[1]);
            let (cx, cy) = at(triangle[2]);
            let area = (bx - ax) * (cy - ay) - (by - ay) * (cx - ax);
            assert!(area < 0.0, "{triangle:?} winds the other way");
        }
    }
}
