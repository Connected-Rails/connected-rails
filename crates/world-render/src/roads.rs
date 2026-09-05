//! Drawing the roads: the line's carriageways as draped surfaces with a
//! shader of their own.
//!
//! One material per surface kind, not per road. What differs between two
//! asphalt roads is their width and their markings — and both of those ride
//! in the mesh's vertex colours and UVs, the way the fields' crops do. So a
//! tile costs one draw per surface kind on it, and the whole line costs three
//! — asphalt, concrete and the gravel of the field tracks.
//!
//! **Multiplayer.** Nothing here is state: the uniform is a function of the
//! weather, the mesh is a function of the line and the elevation data.

use crate::weather;
use bevy::asset::embedded_asset;
use bevy::image::{
    ImageAddressMode, ImageFilterMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor,
};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;
use content::RoadPatch;

/// The material a carriageway is drawn with.
pub type RoadMaterial = ExtendedMaterial<StandardMaterial, RoadExt>;

/// What `roads.wgsl` needs to know about the surface it is drawing.
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct RoadExt {
    #[uniform(100)]
    pub weather: weather::WeatherParams,
    /// The carriageway's own look — asphalt, concrete or gravel — and its normal
    /// map. Program assets, not the module's: nothing about a road varies
    /// from module to module but its geometry and its markings.
    #[texture(101)]
    #[sampler(102)]
    pub texture: Handle<Image>,
    #[texture(103)]
    #[sampler(104)]
    pub normal_map: Handle<Image>,
}

impl MaterialExtension for RoadExt {
    fn fragment_shader() -> ShaderRef {
        "embedded://world_render/roads.wgsl".into()
    }
}

/// Registers the road material. Part of
/// [`WorldRenderPlugin`](crate::WorldRenderPlugin) — both programs draw the
/// same roads.
pub(crate) fn plugin(app: &mut App) {
    embedded_asset!(app, "roads.wgsl");
    embedded_asset!(app, "roads/asphalt.jpg");
    embedded_asset!(app, "roads/asphalt_nor.jpg");
    embedded_asset!(app, "roads/concrete.jpg");
    embedded_asset!(app, "roads/concrete_nor.jpg");
    embedded_asset!(app, "roads/gravel.jpg");
    embedded_asset!(app, "roads/gravel_nor.jpg");
    app.add_plugins(MaterialPlugin::<RoadMaterial>::default())
        .init_resource::<RoadMaterials>()
        .add_systems(Update, mip_surfaces);
}

/// The sampler a carriageway's texture wants. Two settings, both of them the
/// difference between a road and a smear:
///
/// * **Repeat.** The shader samples in metres — a kilometre of road is 250
///   repeats — and Bevy's default sampler clamps to the edge. A clamped road
///   is one column of texels stretched from the horizon to the kerb.
/// * **Mipmapped and anisotropic.** A road runs to the horizon, which is the
///   one thing a texture without a mip chain cannot survive: it boils. JPEG
///   brings no chain, so [`mip_surfaces`] builds one once the image is there
///   and this sampler reads it.
fn road_sampler(srgb: bool) -> impl Fn(&mut ImageLoaderSettings) + Send + Sync + 'static {
    move |settings: &mut ImageLoaderSettings| {
        // A normal map is a direction, not a colour: decoded as sRGB, its
        // flat 128 would come out as a slope, and the whole road with it.
        settings.is_srgb = srgb;
        settings.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            address_mode_w: ImageAddressMode::Repeat,
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            mipmap_filter: ImageFilterMode::Linear,
            ..default()
        });
    }
}

/// Gives the surface textures their mip chain once they have arrived. The
/// JPEG loader brings none, and the roads' textures are the program's own —
/// they never pass the dressing the models' materials get
/// ([`crate::mip_textures`]), which only walks a `StandardMaterial`.
fn mip_surfaces(
    mut roads: ResMut<RoadMaterials>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<RoadMaterial>>,
    mut events: MessageReader<AssetEvent<Image>>,
) {
    if roads.pending.is_empty() {
        events.clear();
        return;
    }
    let arrived: Vec<AssetId<Image>> = events
        .read()
        .filter_map(|event| match event {
            AssetEvent::LoadedWithDependencies { id } => Some(*id),
            _ => None,
        })
        .collect();
    if arrived.is_empty() {
        return;
    }
    let mut built = false;
    roads.pending.retain(|handle| {
        if !arrived.contains(&handle.id()) {
            return true;
        }
        if let Some(mut image) = images.get_mut(handle) {
            built |= crate::build_mip_chain(&mut image, None);
        }
        false
    });
    if built {
        // The bind groups hold the texture views the chain replaced; a touch
        // has them prepared anew.
        let made: Vec<AssetId<RoadMaterial>> = materials.iter().map(|(id, _)| id).collect();
        for id in made {
            materials.get_mut(id);
        }
    }
}

/// The three road materials, made on first use and shared by every tile.
#[derive(Resource, Default)]
pub struct RoadMaterials {
    by_surface: std::collections::HashMap<content::route::RoadSurface, Handle<RoadMaterial>>,
    /// Surface textures still waiting for their mip chain (see
    /// [`mip_surfaces`]).
    pending: Vec<Handle<Image>>,
}

impl RoadMaterials {
    /// The material for a surface, made on first use. The textures are the
    /// program's own — CC0 (ambientCG), like the train ground's.
    pub fn get(
        &mut self,
        surface: content::route::RoadSurface,
        materials: &mut Assets<RoadMaterial>,
        server: &AssetServer,
    ) -> Handle<RoadMaterial> {
        if let Some(made) = self.by_surface.get(&surface) {
            return made.clone();
        }
        let (color, normal) = match surface {
            content::route::RoadSurface::Asphalt => (
                "embedded://world_render/roads/asphalt.jpg",
                "embedded://world_render/roads/asphalt_nor.jpg",
            ),
            content::route::RoadSurface::Concrete => (
                "embedded://world_render/roads/concrete.jpg",
                "embedded://world_render/roads/concrete_nor.jpg",
            ),
            content::route::RoadSurface::Gravel => (
                "embedded://world_render/roads/gravel.jpg",
                "embedded://world_render/roads/gravel_nor.jpg",
            ),
        };
        let texture = server
            .load_builder()
            .with_settings(road_sampler(true))
            .load(color);
        let normal_map = server
            .load_builder()
            .with_settings(road_sampler(false))
            .load(normal);
        self.pending.push(texture.clone());
        self.pending.push(normal_map.clone());
        // A loose surface scatters what a bound one reflects: gravel keeps
        // no sheen of its own, and the weather's wet look has nothing to
        // gather on it.
        let roughness = match surface {
            content::route::RoadSurface::Gravel => 0.98,
            _ => 0.88,
        };
        let made = materials.add(RoadMaterial {
            base: StandardMaterial {
                base_color: Color::WHITE,
                perceptual_roughness: roughness,
                metallic: 0.0,
                ..default()
            },
            extension: RoadExt {
                weather: weather::WeatherParams::default(),
                texture,
                normal_map,
            },
        });
        self.by_surface.insert(surface, made.clone());
        made
    }

    pub fn is_empty(&self) -> bool {
        self.by_surface.is_empty()
    }
}

/// Marks a road surface — one surface kind's worth on one tile, a child of
/// the tile it lies on, so it streams in and out with the ground under it.
/// `sources` are the line's roads that went into it, in line order — what a
/// click on it would select.
#[derive(Component, Debug, Clone)]
pub struct RoadSurfaceMark {
    pub surface: content::route::RoadSurface,
    pub sources: Vec<u32>,
}

/// What [`spawn_roads`] needs besides the patches. Bundled because both
/// programs pass the same things from the same resources.
pub struct RoadDraw<'a> {
    pub materials: &'a mut RoadMaterials,
    pub assets: &'a mut Assets<RoadMaterial>,
    pub server: &'a AssetServer,
}

/// Hangs a tile's roads under the tile.
///
/// Children rather than entities of their own: the terrain streaming despawns
/// a tile with everything below it, so a carriageway never outlives the
/// ground it was draped on, and nothing has to be tracked twice — the same
/// arrangement the fields and the waters hang by.
pub fn spawn_roads(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    draw: &mut RoadDraw,
    tile: Entity,
    patches: &[RoadPatch],
) {
    if patches.is_empty() {
        return;
    }
    let Ok(mut entity) = commands.get_entity(tile) else {
        return;
    };
    let RoadDraw {
        materials,
        assets,
        server,
    } = draw;
    entity.with_children(|parent| {
        for patch in patches {
            let material = materials.get(patch.surface, assets, server);
            parent.spawn((
                Mesh3d(meshes.add(mesh(patch))),
                MeshMaterial3d(material),
                // The patch is already in the tile's own frame.
                Transform::IDENTITY,
                // No meadow grass grows through it: the patch cuts a hole into
                // the grass ground cache (`crate::grass`).
                crate::grass::GroundSurface::Excluded,
                RoadSurfaceMark {
                    surface: patch.surface,
                    sources: patch.sources.clone(),
                },
            ));
        }
    });
}

/// A road's surface as a Bevy mesh, in the tile's own frame.
pub fn mesh(patch: &RoadPatch) -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::mesh::{Indices, PrimitiveTopology};

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, patch.positions.clone());
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, patch.normals.clone());
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, patch.uvs.clone());
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, patch.colors.clone());
    mesh.insert_indices(Indices::U32(patch.indices.clone()));
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mesh carries the attribute set the shader reads, and the vertex
    /// colour is where the markings live.
    #[test]
    fn a_patch_is_a_marked_surface() {
        let patch = RoadPatch {
            surface: content::route::RoadSurface::Asphalt,
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            normals: vec![[0.0, 1.0, 0.0]; 3],
            uvs: vec![[0.5, 0.0]; 3],
            colors: vec![[1.0, 1.0, 3.0, 1.0]; 3],
            indices: vec![0, 1, 2],
            sources: vec![0],
        };
        let mesh = mesh(&patch);
        assert_eq!(mesh.count_vertices(), 3);
        for attribute in [Mesh::ATTRIBUTE_UV_0, Mesh::ATTRIBUTE_COLOR] {
            assert!(mesh.attribute(attribute).is_some(), "{attribute:?}");
        }
        let Some(bevy::mesh::VertexAttributeValues::Float32x4(colors)) =
            mesh.attribute(Mesh::ATTRIBUTE_COLOR)
        else {
            panic!("colors");
        };
        assert!((colors[0][2] - 3.0).abs() < 1e-6, "the half-width");
    }

    /// The shader samples the surface in metres, so a kilometre of road is
    /// hundreds of repeats: a texture that clamps to its edge instead of
    /// repeating draws one column of texels the length of the carriageway.
    /// And a normal map is a direction — read back through sRGB, its flat
    /// 128 comes out as a slope and tilts the whole road.
    #[test]
    fn the_surface_repeats_and_the_normal_map_stays_linear() {
        let mut settings = ImageLoaderSettings::default();
        road_sampler(true)(&mut settings);
        assert!(settings.is_srgb, "the colour is a colour");
        let ImageSampler::Descriptor(sampler) = &settings.sampler else {
            panic!("the road sets its own sampler");
        };
        assert_eq!(sampler.address_mode_u, ImageAddressMode::Repeat);
        assert_eq!(sampler.address_mode_v, ImageAddressMode::Repeat);
        assert_eq!(sampler.mipmap_filter, ImageFilterMode::Linear);

        let mut settings = ImageLoaderSettings::default();
        road_sampler(false)(&mut settings);
        assert!(!settings.is_srgb, "the normal map is a direction");
    }
}
