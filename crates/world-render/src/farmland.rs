//! Drawing the fields (the field plan, ch. 6 and 7).
//!
//! One material per crop, not per field. A line through the Börde has some
//! thousands of fields and a dozen crops; what differs between two wheat fields
//! is a tint and a row phase, and both of those ride in the mesh's vertex
//! colours. So a tile costs one draw call per crop on it, and the whole line
//! costs thirteen materials.
//!
//! The date lives in the material, not in the mesh. When the scenario clock
//! passes midnight — or the editor's date slider is dragged — [`update`] writes
//! new uniforms and every field in the world turns with it, without a single
//! mesh being rebuilt. That is what makes it worth having the phenology at all:
//! the same module shows April and August, and shows them for free.

use crate::weather;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use content::FieldPatch;
use fields::CropClass;
use fields::phenology::{self, Growth};
use std::collections::HashMap;

/// The material a field's surface is drawn with.
pub type FieldMaterial = ExtendedMaterial<StandardMaterial, CropExt>;

/// What `fields.wgsl` needs to know about the crop it is drawing.
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct CropExt {
    #[uniform(100)]
    pub crop: CropParams,
    /// What the weather is doing to the ground — the same uniform the terrain
    /// and the objects carry, written by `weather::update`.
    #[uniform(101)]
    pub weather: weather::WeatherParams,
}

/// The crop's own numbers, in the shader's layout.
#[derive(Clone, Copy, Debug, ShaderType)]
pub struct CropParams {
    /// `rgb` the stand's colour, `a` how much of the ground it covers.
    pub color: Vec4,
    /// `x` how strongly the rows read, `y` row spacing [m], `z` tramline
    /// spacing [m], `w` the stand's height [m].
    pub rows: Vec4,
    /// `rgb` the soil, `a` the surface's roughness.
    pub soil: Vec4,
}

impl MaterialExtension for CropExt {
    fn fragment_shader() -> ShaderRef {
        "embedded://world_render/fields.wgsl".into()
    }
}

/// Bare soil. The same brown under every crop: what varies between a field in
/// Schleswig-Holstein and one in the Börde is the crop on it, not a colour the
/// import has no data for.
//
// ponytail: one soil colour for the whole country. The soil map (BÜK200, also
// open) would give a loess brown for the Börde and a sandy grey for the Geest;
// that is a second import and a second attribution, and it belongs after
// somebody has looked at the first one.
const SOIL: [f32; 3] = [0.29, 0.22, 0.16];

/// How far apart the drills run, per crop [m]. What the eye reads as "the rows"
/// — 12 cm for drilled cereal, 75 cm for maize and beet, two metres for vines.
fn row_spacing(crop: CropClass) -> f32 {
    match crop {
        CropClass::WinterCereal | CropClass::SummerCereal | CropClass::Legume => 0.14,
        CropClass::Rapeseed => 0.30,
        CropClass::Maize | CropClass::SugarBeet | CropClass::Potato => 0.75,
        CropClass::Vegetable => 0.45,
        CropClass::Orchard => 3.5,
        CropClass::Vineyard => 2.0,
        // Grass and set-aside are not drilled in rows anybody can see.
        CropClass::Grassland | CropClass::Fallow | CropClass::Other => 1.0,
    }
}

/// How far apart the sprayer's wheel tracks run [m]. A working width: 24 m is
/// the common one, 36 on the big eastern holdings. Permanent crops have none —
/// the rows themselves are the way through.
fn tramline_spacing(crop: CropClass) -> f32 {
    match crop {
        CropClass::Orchard | CropClass::Vineyard => 1.0e6,
        CropClass::Grassland => 1.0e6,
        _ => 24.0,
    }
}

/// One sRGB channel as the linear value a shader works in.
///
/// The phenology table is written in sRGB, because that is the space a colour
/// is picked in and read back in. A material's `base_color` is linear, and
/// handing it 0.28 where 0.06 was meant is why an early attempt drew the whole
/// countryside in pale sage.
fn linear(channel: f32) -> f32 {
    if channel <= 0.04045 {
        channel / 12.92
    } else {
        ((channel + 0.055) / 1.055).powf(2.4)
    }
}

/// The uniform for a crop on a day.
pub fn params(crop: CropClass, growth: Growth) -> CropParams {
    CropParams {
        color: Vec4::new(
            linear(growth.color[0]),
            linear(growth.color[1]),
            linear(growth.color[2]),
            growth.cover,
        ),
        rows: Vec4::new(
            growth.rows,
            row_spacing(crop),
            tramline_spacing(crop),
            growth.height,
        ),
        soil: Vec4::new(linear(SOIL[0]), linear(SOIL[1]), linear(SOIL[2]), 0.94),
    }
}

/// The materials of the line, one per crop, and the date they were written for.
#[derive(Resource, Default)]
pub struct FieldMaterials {
    by_crop: HashMap<CropClass, Handle<FieldMaterial>>,
    /// Day of the year the uniforms hold. `None` until the first write.
    day: Option<u16>,
}

impl FieldMaterials {
    /// The material for a crop, made on first use.
    pub fn get(
        &mut self,
        crop: CropClass,
        materials: &mut Assets<FieldMaterial>,
        month: u32,
        day: u32,
    ) -> Handle<FieldMaterial> {
        self.by_crop
            .entry(crop)
            .or_insert_with(|| {
                let growth = phenology::growth(crop, month, day, 0);
                materials.add(FieldMaterial {
                    base: StandardMaterial {
                        perceptual_roughness: 0.94,
                        ..default()
                    },
                    extension: CropExt {
                        crop: params(crop, growth),
                        weather: weather::WeatherParams::default(),
                    },
                })
            })
            .clone()
    }

    /// Writes the day into every material, if the day has moved.
    ///
    /// Called every frame and cheap on all but one of them: the comparison is
    /// against the day of the year, so the clock ticking through an afternoon
    /// costs nothing and midnight costs thirteen uniform writes.
    pub fn set_date(
        &mut self,
        materials: &mut Assets<FieldMaterial>,
        month: u32,
        day: u32,
    ) -> bool {
        let today = phenology::day_of_year(month, day);
        if self.day == Some(today) {
            return false;
        }
        self.day = Some(today);
        for (crop, handle) in &self.by_crop {
            if let Some(mut material) = materials.get_mut(handle) {
                material.extension.crop = params(*crop, phenology::growth(*crop, month, day, 0));
            }
        }
        true
    }

    /// The weather uniform, written by `weather::update` alongside the terrain's.
    pub fn set_weather(
        &self,
        materials: &mut Assets<FieldMaterial>,
        weather: weather::WeatherParams,
    ) {
        for handle in self.by_crop.values() {
            if let Some(mut material) = materials.get_mut(handle) {
                material.extension.weather = weather;
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.by_crop.is_empty()
    }
}

/// Turns the fields with the calendar.
///
/// The date is the scenario clock in the simulator and the date slider in the
/// editor; both live in [`crate::sky::Sky`], so both programs get this for
/// nothing. It runs every frame and does nothing on all but one of them — the
/// day of the year is what it compares.
pub fn follow_date(
    sky: Res<crate::sky::Sky>,
    mut materials: ResMut<FieldMaterials>,
    mut assets: ResMut<Assets<FieldMaterial>>,
) {
    if materials.is_empty() {
        return;
    }
    materials.set_date(&mut assets, sky.month, sky.day);
}

/// Marks a field surface — one crop's worth on one tile, a child of the tile
/// it lies on, so it streams in and out with the ground under it.
#[derive(Component, Debug, Clone)]
pub struct FieldSurface {
    pub crop: CropClass,
    /// The fields of the line that went into it — what a click selects.
    pub sources: Vec<u32>,
}

/// What [`spawn_fields`] needs besides the patches: the materials it picks
/// from, and the day they are written for. Bundled because both programs pass
/// the same four things from the same three resources.
pub struct FieldDraw<'a> {
    pub materials: &'a mut FieldMaterials,
    pub assets: &'a mut Assets<FieldMaterial>,
    pub month: u32,
    pub day: u32,
}

/// Hangs a tile's farmland under the tile.
///
/// Children rather than entities of their own: the terrain streaming despawns
/// a tile with everything below it, so a field never outlives the ground it was
/// draped on, and nothing has to be tracked twice.
pub fn spawn_fields(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    draw: &mut FieldDraw,
    tile: Entity,
    patches: &[FieldPatch],
) {
    if patches.is_empty() {
        return;
    }
    let Ok(mut entity) = commands.get_entity(tile) else {
        return;
    };
    let (materials, assets, month, day) = (
        &mut *draw.materials,
        &mut *draw.assets,
        draw.month,
        draw.day,
    );
    entity.with_children(|parent| {
        for patch in patches {
            let material = materials.get(patch.crop, assets, month, day);
            parent.spawn((
                Mesh3d(meshes.add(mesh(patch))),
                MeshMaterial3d(material),
                // The patch is already in the tile's own frame.
                Transform::IDENTITY,
                FieldSurface {
                    crop: patch.crop,
                    sources: patch.sources.clone(),
                },
            ));
        }
    });
}

/// A field's surface as a Bevy mesh, in the tile's own frame.
pub fn mesh(patch: &FieldPatch) -> Mesh {
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

    #[test]
    fn drilled_cereal_is_in_tighter_rows_than_maize() {
        assert!(row_spacing(CropClass::WinterCereal) < row_spacing(CropClass::Maize));
        assert!(row_spacing(CropClass::Maize) < row_spacing(CropClass::Vineyard));
    }

    #[test]
    fn grass_and_orchards_have_no_tramlines() {
        // A spacing this large puts the next rut a hundred kilometres away.
        assert!(tramline_spacing(CropClass::Grassland) > 1000.0);
        assert!(tramline_spacing(CropClass::Orchard) > 1000.0);
        assert_eq!(tramline_spacing(CropClass::WinterCereal), 24.0);
    }

    #[test]
    fn the_uniform_carries_the_day() {
        // Wheat in July is gold and closed; in September it is bare soil.
        let july = params(
            CropClass::WinterCereal,
            phenology::growth(CropClass::WinterCereal, 7, 15, 0),
        );
        let september = params(
            CropClass::WinterCereal,
            phenology::growth(CropClass::WinterCereal, 9, 20, 0),
        );
        assert!(july.color.w > 0.9, "{}", july.color.w);
        assert!(september.color.w < 0.2, "{}", september.color.w);
        assert!(july.color.x > july.color.z, "not gold: {:?}", july.color);
    }

    #[test]
    fn colours_reach_the_shader_in_linear_space() {
        // The table is sRGB; the uniform is what the material blends in.
        assert!((linear(0.0) - 0.0).abs() < 1e-6);
        assert!((linear(1.0) - 1.0).abs() < 1e-6);
        // Mid grey: 0.5 sRGB is about 0.214 linear, and getting this wrong is
        // what washed the first version of the fields out.
        assert!((linear(0.5) - 0.2140).abs() < 1e-3, "{}", linear(0.5));
        let wheat = params(
            CropClass::WinterCereal,
            phenology::growth(CropClass::WinterCereal, 5, 1, 0),
        );
        assert!(wheat.color.y < 0.25, "still washed out: {:?}", wheat.color);
    }
}
