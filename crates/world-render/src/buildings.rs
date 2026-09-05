//! Baked parametric building meshes and the shared German-building PBR library.
//!
//! Every distinct recipe is meshed once and cached. Placements only add transforms;
//! all facade, roof, glass, trim and balcony materials are global handles, so Bevy can
//! batch equal parts across buildings and streamed terrain tiles.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::VisibilityRange;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use content::{BakedBuilding, BuildingSpec, BuildingUse, RoofStyle};

use crate::{NIGHT_SUFFIX, Scattered};

pub const BUILDING_LOD0_END: f32 = 180.0;
pub const BUILDING_LOD1_END: f32 = 650.0;
pub const BUILDING_CULL: f32 = 2_500.0;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildingIndex(pub u32);

#[derive(Clone)]
struct MaterialSet {
    facade: [Handle<StandardMaterial>; 5],
    roof: [Handle<StandardMaterial>; 4],
    glass: Handle<StandardMaterial>,
    lit_glass: Handle<StandardMaterial>,
    trim: Handle<StandardMaterial>,
    balcony: Handle<StandardMaterial>,
    detail: Handle<StandardMaterial>,
    door: [Handle<StandardMaterial>; 3],
}

#[derive(Clone, Default)]
struct LodMeshes {
    facade: Option<Handle<Mesh>>,
    roof: Option<Handle<Mesh>>,
    glass: Option<Handle<Mesh>>,
    lit: Option<Handle<Mesh>>,
    trim: Option<Handle<Mesh>>,
    balcony: Option<Handle<Mesh>>,
    detail: Option<Handle<Mesh>>,
    door: Option<Handle<Mesh>>,
}

#[derive(Clone)]
struct BuildingMeshes([LodMeshes; 3]);

/// Global material library and geometry cache shared by editor and simulator.
#[derive(Resource)]
pub struct BuildingAssets {
    materials: MaterialSet,
    meshes: HashMap<u64, BuildingMeshes>,
}

impl FromWorld for BuildingAssets {
    fn from_world(world: &mut World) -> Self {
        let textures = {
            let mut images = world.resource_mut::<Assets<Image>>();
            (0..9)
                .map(|kind| {
                    let (albedo, normal, metal_rough) = surface_textures(kind);
                    (
                        images.add(albedo),
                        images.add(normal),
                        images.add(metal_rough),
                    )
                })
                .collect::<Vec<_>>()
        };
        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
        let textured = |texture: &(Handle<Image>, Handle<Image>, Handle<Image>)| StandardMaterial {
            base_color_texture: Some(texture.0.clone()),
            normal_map_texture: Some(texture.1.clone()),
            metallic_roughness_texture: Some(texture.2.clone()),
            // The packed map already contains physically meaningful values;
            // unit factors preserve them instead of multiplying them twice.
            perceptual_roughness: 1.0,
            metallic: 1.0,
            ..default()
        };
        let facade = [
            materials.add(textured(&textures[0])),
            materials.add(textured(&textures[1])),
            materials.add(textured(&textures[2])),
            materials.add(textured(&textures[3])),
            materials.add(textured(&textures[4])),
        ];
        let roof = [
            materials.add(textured(&textures[5])),
            materials.add(textured(&textures[6])),
            materials.add(textured(&textures[7])),
            materials.add(textured(&textures[8])),
        ];
        let glass = materials.add(StandardMaterial {
            base_color: Color::srgb(0.045, 0.075, 0.105),
            metallic: 0.0,
            perceptual_roughness: 0.11,
            reflectance: 0.9,
            ..default()
        });
        let lit_glass = materials.add(StandardMaterial {
            base_color: Color::srgb(0.82, 0.52, 0.20),
            emissive: LinearRgba::rgb(2.2, 1.05, 0.30),
            perceptual_roughness: 0.34,
            reflectance: 0.35,
            ..default()
        });
        let trim = materials.add(StandardMaterial {
            base_color: Color::srgb(0.82, 0.84, 0.82),
            perceptual_roughness: 0.44,
            reflectance: 0.42,
            ..default()
        });
        let balcony = materials.add(StandardMaterial {
            base_color: Color::srgb(0.24, 0.27, 0.29),
            metallic: 0.62,
            perceptual_roughness: 0.42,
            ..default()
        });
        let detail = materials.add(StandardMaterial {
            base_color: Color::srgb(0.55, 0.59, 0.61),
            metallic: 0.72,
            perceptual_roughness: 0.38,
            reflectance: 0.48,
            ..default()
        });
        let door = [
            materials.add(StandardMaterial {
                base_color: Color::srgb(0.22, 0.105, 0.045),
                perceptual_roughness: 0.64,
                reflectance: 0.32,
                ..default()
            }),
            materials.add(StandardMaterial {
                base_color: Color::srgb(0.075, 0.10, 0.12),
                metallic: 0.18,
                perceptual_roughness: 0.3,
                ..default()
            }),
            materials.add(StandardMaterial {
                base_color: Color::srgb(0.34, 0.37, 0.39),
                metallic: 0.68,
                perceptual_roughness: 0.42,
                ..default()
            }),
        ];
        Self {
            materials: MaterialSet {
                facade,
                roof,
                glass,
                lit_glass,
                trim,
                balcony,
                detail,
                door,
            },
            meshes: HashMap::new(),
        }
    }
}

/// Adds every baked building as a child of its terrain tile.
pub fn spawn_buildings(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    assets: &mut BuildingAssets,
    tile: Entity,
    buildings: &[BakedBuilding],
) {
    if buildings.is_empty() || commands.get_entity(tile).is_err() {
        return;
    }
    for building in buildings {
        let spec = building.spec.normalised();
        let key = spec.mesh_key();
        let mesh_set = assets
            .meshes
            .entry(key)
            .or_insert_with(|| build_meshes(meshes, &spec))
            .clone();
        let root = commands
            .spawn((
                Transform::from_translation(Vec3::from(building.pos))
                    .with_rotation(Quat::from_array(building.rotation)),
                Visibility::default(),
                BuildingIndex(building.source_index),
                Scattered,
            ))
            .id();
        commands.entity(tile).add_child(root);
        let facade = assets.materials.facade[spec.facade as usize].clone();
        let roof = assets.materials.roof[spec.roof as usize].clone();
        let door = assets.materials.door[spec.use_kind as usize].clone();
        for (level, lod) in mesh_set.0.iter().enumerate() {
            let range = match level {
                0 => VisibilityRange::abrupt(0.0, BUILDING_LOD0_END),
                1 => VisibilityRange::abrupt(BUILDING_LOD0_END, BUILDING_LOD1_END),
                _ => VisibilityRange::abrupt(BUILDING_LOD1_END, BUILDING_CULL),
            };
            let parts = [
                (&lod.facade, &facade, "facade"),
                (&lod.roof, &roof, "roof"),
                (&lod.glass, &assets.materials.glass, "windows"),
                (&lod.trim, &assets.materials.trim, "trim"),
                (&lod.balcony, &assets.materials.balcony, "balconies"),
                (&lod.detail, &assets.materials.detail, "details"),
                (&lod.door, &door, "doors"),
            ];
            for (mesh, material, name) in parts {
                if let Some(mesh) = mesh {
                    let child = commands
                        .spawn((
                            Name::new(format!("building_{name}_LOD{level}")),
                            Mesh3d(mesh.clone()),
                            MeshMaterial3d(material.clone()),
                            Transform::IDENTITY,
                            range.clone(),
                            Scattered,
                        ))
                        .id();
                    commands.entity(root).add_child(child);
                }
            }
            if let Some(mesh) = &lod.lit {
                let child = commands
                    .spawn((
                        Name::new(format!("building_windows_LOD{level}{NIGHT_SUFFIX}")),
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(assets.materials.lit_glass.clone()),
                        Transform::IDENTITY,
                        range,
                        Scattered,
                    ))
                    .id();
                commands.entity(root).add_child(child);
            }
        }
    }
}

fn build_meshes(meshes: &mut Assets<Mesh>, spec: &BuildingSpec) -> BuildingMeshes {
    BuildingMeshes(std::array::from_fn(|lod| {
        let built = geometry(spec, lod);
        LodMeshes {
            facade: built.facade.finish().map(|m| meshes.add(m)),
            roof: built.roof.finish().map(|m| meshes.add(m)),
            glass: built.glass.finish().map(|m| meshes.add(m)),
            lit: built.lit.finish().map(|m| meshes.add(m)),
            trim: built.trim.finish().map(|m| meshes.add(m)),
            balcony: built.balcony.finish().map(|m| meshes.add(m)),
            detail: built.detail.finish().map(|m| meshes.add(m)),
            door: built.door.finish().map(|m| meshes.add(m)),
        }
    }))
}

#[derive(Default)]
struct Geometry {
    facade: MeshData,
    roof: MeshData,
    glass: MeshData,
    lit: MeshData,
    trim: MeshData,
    balcony: MeshData,
    detail: MeshData,
    door: MeshData,
}

#[derive(Default)]
struct MeshData {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl MeshData {
    fn quad(&mut self, p: [[f32; 3]; 4], normal: [f32; 3], uv: [[f32; 2]; 4], color: [f32; 4]) {
        let start = self.positions.len() as u32;
        self.positions.extend(p);
        self.normals.extend([normal; 4]);
        self.uvs.extend(uv);
        self.colors.extend([color; 4]);
        self.indices
            .extend_from_slice(&[start, start + 1, start + 2, start, start + 2, start + 3]);
    }

    fn box_(&mut self, center: Vec3, size: Vec3, color: [f32; 4]) {
        self.box_uv(center, size, color, None);
    }

    fn closed_box(&mut self, center: Vec3, size: Vec3, color: [f32; 4]) {
        self.closed_box_uv(center, size, color, None);
    }

    /// Closed low-poly vertical cylinder for flues and roof ventilators.
    /// It lives in the shared detail mesh, so even a busy factory adds no
    /// entities or material switches per vent.
    fn vertical_prism(
        &mut self,
        center: Vec3,
        radius: f32,
        height: f32,
        sides: usize,
        color: [f32; 4],
    ) {
        self.prism_between(
            center - Vec3::Y * height * 0.5,
            center + Vec3::Y * height * 0.5,
            radius,
            sides,
            color,
        );
    }

    /// Closed round profile along an arbitrary line. Used for both gutters
    /// and their vertical/offset drain pipes, keeping the same cross-section
    /// through every connector.
    fn prism_between(
        &mut self,
        start: Vec3,
        end: Vec3,
        radius: f32,
        sides: usize,
        color: [f32; 4],
    ) {
        let sides = sides.max(3);
        let axis = (end - start).normalize_or_zero();
        if axis.length_squared() < 0.5 {
            return;
        }
        let radial_a = if axis.y.abs() < 0.9 {
            axis.cross(Vec3::Y).normalize_or_zero()
        } else {
            Vec3::X
        };
        let radial_b = radial_a.cross(axis).normalize_or_zero();
        for side in 0..sides {
            let a = side as f32 * std::f32::consts::TAU / sides as f32;
            let b = (side + 1) as f32 * std::f32::consts::TAU / sides as f32;
            let direction_a = radial_a * a.cos() + radial_b * a.sin();
            let direction_b = radial_a * b.cos() + radial_b * b.sin();
            let start_a = start + direction_a * radius;
            let start_b = start + direction_b * radius;
            let end_a = end + direction_a * radius;
            let end_b = end + direction_b * radius;
            let normal = (direction_a + direction_b).normalize_or_zero();
            self.quad(
                [
                    start_a.to_array(),
                    end_a.to_array(),
                    end_b.to_array(),
                    start_b.to_array(),
                ],
                normal.to_array(),
                [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]],
                color,
            );
            self.triangle(
                [end.to_array(), end_b.to_array(), end_a.to_array()],
                axis.to_array(),
                [[0.5, 0.5], [0.0, 0.0], [1.0, 0.0]],
                color,
            );
            self.triangle(
                [start.to_array(), start_a.to_array(), start_b.to_array()],
                (-axis).to_array(),
                [[0.5, 0.5], [1.0, 0.0], [0.0, 0.0]],
                color,
            );
        }
    }

    fn closed_box_uv(&mut self, center: Vec3, size: Vec3, color: [f32; 4], density: Option<f32>) {
        self.box_uv(center, size, color, density);
        let lo = center - size * 0.5;
        let hi = center + size * 0.5;
        let uv = match density {
            Some(density) => [
                [0.0, 0.0],
                [0.0, size.z * density],
                [size.x * density, size.z * density],
                [size.x * density, 0.0],
            ],
            None => [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]],
        };
        self.quad(
            [
                [lo.x, lo.y, hi.z],
                [lo.x, lo.y, lo.z],
                [hi.x, lo.y, lo.z],
                [hi.x, lo.y, hi.z],
            ],
            [0.0, -1.0, 0.0],
            uv,
            color,
        );
    }

    fn box_uv(&mut self, center: Vec3, size: Vec3, color: [f32; 4], density: Option<f32>) {
        let lo = center - size * 0.5;
        let hi = center + size * 0.5;
        let uv = |u: f32, v: f32| match density {
            Some(density) => [
                [0.0, 0.0],
                [0.0, v * density],
                [u * density, v * density],
                [u * density, 0.0],
            ],
            None => [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        };
        self.quad(
            [
                [lo.x, lo.y, lo.z],
                [lo.x, hi.y, lo.z],
                [hi.x, hi.y, lo.z],
                [hi.x, lo.y, lo.z],
            ],
            [0.0, 0.0, -1.0],
            uv(size.x, size.y),
            color,
        );
        self.quad(
            [
                [hi.x, lo.y, hi.z],
                [hi.x, hi.y, hi.z],
                [lo.x, hi.y, hi.z],
                [lo.x, lo.y, hi.z],
            ],
            [0.0, 0.0, 1.0],
            uv(size.x, size.y),
            color,
        );
        self.quad(
            [
                [lo.x, lo.y, hi.z],
                [lo.x, hi.y, hi.z],
                [lo.x, hi.y, lo.z],
                [lo.x, lo.y, lo.z],
            ],
            [-1.0, 0.0, 0.0],
            uv(size.z, size.y),
            color,
        );
        self.quad(
            [
                [hi.x, lo.y, lo.z],
                [hi.x, hi.y, lo.z],
                [hi.x, hi.y, hi.z],
                [hi.x, lo.y, hi.z],
            ],
            [1.0, 0.0, 0.0],
            uv(size.z, size.y),
            color,
        );
        self.quad(
            [
                [lo.x, hi.y, lo.z],
                [lo.x, hi.y, hi.z],
                [hi.x, hi.y, hi.z],
                [hi.x, hi.y, lo.z],
            ],
            [0.0, 1.0, 0.0],
            uv(size.x, size.z),
            color,
        );
    }

    fn triangle(&mut self, p: [[f32; 3]; 3], normal: [f32; 3], uv: [[f32; 2]; 3], color: [f32; 4]) {
        let start = self.positions.len() as u32;
        self.positions.extend(p);
        self.normals.extend([normal; 3]);
        self.uvs.extend(uv);
        self.colors.extend([color; 3]);
        self.indices
            .extend_from_slice(&[start, start + 1, start + 2]);
    }

    fn quad_auto(&mut self, p: [[f32; 3]; 4], uv: [[f32; 2]; 4], color: [f32; 4]) {
        let a = Vec3::from_array(p[1]) - Vec3::from_array(p[0]);
        let b = Vec3::from_array(p[2]) - Vec3::from_array(p[0]);
        self.quad(p, a.cross(b).normalize_or_zero().to_array(), uv, color);
    }

    fn triangle_auto(&mut self, p: [[f32; 3]; 3], uv: [[f32; 2]; 3], color: [f32; 4]) {
        let a = Vec3::from_array(p[1]) - Vec3::from_array(p[0]);
        let b = Vec3::from_array(p[2]) - Vec3::from_array(p[0]);
        self.triangle(p, a.cross(b).normalize_or_zero().to_array(), uv, color);
    }

    /// A thin roof panel with a visible underside. `outer_edges` identifies
    /// the perimeter edges that need fascia; shared ridges and hips remain
    /// open inside the joined shell and cannot produce overlapping faces.
    fn roof_quad(
        &mut self,
        top: [[f32; 3]; 4],
        uv: [[f32; 2]; 4],
        outer_edges: [bool; 4],
        color: [f32; 4],
    ) {
        const THICKNESS: f32 = 0.12;
        self.quad_auto(top, uv, color);
        let bottom = top.map(|point| [point[0], point[1] - THICKNESS, point[2]]);
        self.quad_auto(
            [bottom[3], bottom[2], bottom[1], bottom[0]],
            [uv[3], uv[2], uv[1], uv[0]],
            color,
        );
        for edge in 0..4 {
            if !outer_edges[edge] {
                continue;
            }
            let next = (edge + 1) % 4;
            let length = Vec3::from_array(top[next]).distance(Vec3::from_array(top[edge]))
                * SURFACE_UV_DENSITY;
            self.quad_auto(
                [top[edge], top[next], bottom[next], bottom[edge]],
                [
                    [0.0, 0.0],
                    [length, 0.0],
                    [length, THICKNESS * SURFACE_UV_DENSITY],
                    [0.0, THICKNESS * SURFACE_UV_DENSITY],
                ],
                color,
            );
        }
    }

    fn roof_triangle(
        &mut self,
        top: [[f32; 3]; 3],
        uv: [[f32; 2]; 3],
        outer_edges: [bool; 3],
        color: [f32; 4],
    ) {
        const THICKNESS: f32 = 0.12;
        self.triangle_auto(top, uv, color);
        let bottom = top.map(|point| [point[0], point[1] - THICKNESS, point[2]]);
        self.triangle_auto(
            [bottom[2], bottom[1], bottom[0]],
            [uv[2], uv[1], uv[0]],
            color,
        );
        for edge in 0..3 {
            if !outer_edges[edge] {
                continue;
            }
            let next = (edge + 1) % 3;
            let length = Vec3::from_array(top[next]).distance(Vec3::from_array(top[edge]))
                * SURFACE_UV_DENSITY;
            self.quad_auto(
                [top[edge], top[next], bottom[next], bottom[edge]],
                [
                    [0.0, 0.0],
                    [length, 0.0],
                    [length, THICKNESS * SURFACE_UV_DENSITY],
                    [0.0, THICKNESS * SURFACE_UV_DENSITY],
                ],
                color,
            );
        }
    }

    /// Two thin flashing wings that each lie on one of the adjoining roof
    /// planes. `inside_a` and `inside_b` identify those planes and keep the
    /// construction valid for ridges as well as diagonal hip joints.
    fn roof_cap(
        &mut self,
        start: Vec3,
        end: Vec3,
        inside_a: Vec3,
        inside_b: Vec3,
        width: f32,
        color: [f32; 4],
    ) {
        let axis = (end - start).normalize_or_zero();
        let middle = (start + end) * 0.5;
        let into_plane = |inside: Vec3| {
            let offset = inside - middle;
            (offset - axis * offset.dot(axis)).normalize_or_zero()
        };
        let direction_a = into_plane(inside_a);
        let direction_b = into_plane(inside_b);
        let upward_normal = |direction: Vec3| {
            let mut normal = axis.cross(direction).normalize_or_zero();
            if normal.y < 0.0 {
                normal = -normal;
            }
            normal
        };
        let normal_a = upward_normal(direction_a);
        let normal_b = upward_normal(direction_b);
        let crest = (normal_a + normal_b).normalize_or(Vec3::Y) * 0.018;
        let along = (end - start).length() * SURFACE_UV_DENSITY;
        for (direction, normal) in [(direction_a, normal_a), (direction_b, normal_b)] {
            let mut points = [
                (start + crest).to_array(),
                (end + crest).to_array(),
                (end + direction * width + normal * 0.008).to_array(),
                (start + direction * width + normal * 0.008).to_array(),
            ];
            let mut uv = [
                [0.0, 0.0],
                [along, 0.0],
                [along, width * SURFACE_UV_DENSITY],
                [0.0, width * SURFACE_UV_DENSITY],
            ];
            let face = (Vec3::from_array(points[1]) - Vec3::from_array(points[0]))
                .cross(Vec3::from_array(points[2]) - Vec3::from_array(points[0]));
            if face.dot(normal) < 0.0 {
                points.swap(1, 3);
                uv.swap(1, 3);
            }
            self.quad_auto(points, uv, color);
        }
    }

    fn finish(self) -> Option<Mesh> {
        if self.positions.is_empty() {
            return None;
        }
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.colors);
        mesh.insert_indices(Indices::U32(self.indices));
        // Shared facade and roof materials carry normal maps, which require a
        // tangent basis. Every generated quad has non-degenerate UVs.
        let _ = mesh.generate_tangents();
        Some(mesh)
    }
}

fn geometry(spec: &BuildingSpec, lod: usize) -> Geometry {
    let mut g = Geometry::default();
    let (w, l, h) = (spec.width, spec.length, spec.wall_height());
    let tint = [
        spec.facade_color[0],
        spec.facade_color[1],
        spec.facade_color[2],
        1.0,
    ];
    add_walls(&mut g.facade, w, l, h, tint);
    add_roof_end_walls(&mut g.facade, spec, w, l, h, tint);
    add_roof(&mut g.roof, spec, w, l, h);
    if lod < 2 {
        add_roof_caps(&mut g.roof, spec, w, l, h);
        if spec.roof_style == RoofStyle::Sawtooth {
            add_sawtooth_glazing(&mut g.glass, spec, w, l, h);
        }
        add_openings(&mut g, spec, lod);
        add_roof_details(&mut g, spec, lod, tint);
    }
    if lod == 0 && spec.balconies {
        add_balconies(&mut g.balcony, spec);
    }
    g
}

fn add_roof_end_walls(
    mesh: &mut MeshData,
    spec: &BuildingSpec,
    w: f32,
    l: f32,
    h: f32,
    color: [f32; 4],
) {
    let r = spec.roof_height;
    if spec.roof_style == RoofStyle::Shed {
        // A shed roof ends one full roof-height above the ordinary wall on
        // its high side. Closing only the triangular end walls leaves this
        // whole long side open and exposes the culled wall backs from above.
        mesh.quad(
            [
                [w / 2.0, h, -l / 2.0],
                [w / 2.0, h + r, -l / 2.0],
                [w / 2.0, h + r, l / 2.0],
                [w / 2.0, h, l / 2.0],
            ],
            [1.0, 0.0, 0.0],
            [[0.0, h], [0.0, h + r], [l, h + r], [l, h]],
            color,
        );
    }
    let profile: Vec<[f32; 2]> = match spec.roof_style {
        RoofStyle::Gable => vec![[-w / 2.0, h], [0.0, h + r], [w / 2.0, h]],
        RoofStyle::Shed => vec![[-w / 2.0, h], [w / 2.0, h + r], [w / 2.0, h]],
        RoofStyle::Mansard => vec![
            [-w / 2.0, h],
            [-w * 0.32, h + r * 0.72],
            [0.0, h + r],
            [w * 0.32, h + r * 0.72],
            [w / 2.0, h],
        ],
        RoofStyle::Sawtooth => {
            add_sawtooth_end_walls(mesh, spec, w, l, h, color);
            return;
        }
        RoofStyle::Flat | RoofStyle::Hip => return,
    };
    for end in [-1.0_f32, 1.0] {
        let z = end * l / 2.0;
        for i in 1..profile.len() - 1 {
            let source = [profile[0], profile[i], profile[i + 1]];
            let mut p = [
                [source[0][0], source[0][1], z],
                [source[1][0], source[1][1], z],
                [source[2][0], source[2][1], z],
            ];
            // Planar projection shared by every triangle: brick and plaster
            // continue through the gable instead of restarting diagonally.
            let mut uv = source.map(|point| {
                [
                    (point[0] + w / 2.0) * SURFACE_UV_DENSITY,
                    point[1] * SURFACE_UV_DENSITY,
                ]
            });
            if end > 0.0 {
                p.swap(1, 2);
                uv.swap(1, 2);
            }
            mesh.triangle_auto(p, uv, color);
        }
    }
}

fn add_walls(mesh: &mut MeshData, w: f32, l: f32, h: f32, color: [f32; 4]) {
    let uv = |u: f32, v: f32| {
        let (u, v) = (u * SURFACE_UV_DENSITY, v * SURFACE_UV_DENSITY);
        [[0.0, 0.0], [0.0, v], [u, v], [u, 0.0]]
    };
    mesh.quad(
        [
            [-w / 2.0, 0.0, -l / 2.0],
            [-w / 2.0, h, -l / 2.0],
            [w / 2.0, h, -l / 2.0],
            [w / 2.0, 0.0, -l / 2.0],
        ],
        [0.0, 0.0, -1.0],
        uv(w, h),
        color,
    );
    mesh.quad(
        [
            [w / 2.0, 0.0, l / 2.0],
            [w / 2.0, h, l / 2.0],
            [-w / 2.0, h, l / 2.0],
            [-w / 2.0, 0.0, l / 2.0],
        ],
        [0.0, 0.0, 1.0],
        uv(w, h),
        color,
    );
    mesh.quad(
        [
            [-w / 2.0, 0.0, l / 2.0],
            [-w / 2.0, h, l / 2.0],
            [-w / 2.0, h, -l / 2.0],
            [-w / 2.0, 0.0, -l / 2.0],
        ],
        [-1.0, 0.0, 0.0],
        uv(l, h),
        color,
    );
    mesh.quad(
        [
            [w / 2.0, 0.0, -l / 2.0],
            [w / 2.0, h, -l / 2.0],
            [w / 2.0, h, l / 2.0],
            [w / 2.0, 0.0, l / 2.0],
        ],
        [1.0, 0.0, 0.0],
        uv(l, h),
        color,
    );
}

fn sawtooth_count(width: f32) -> usize {
    (width / 10.0).round().clamp(2.0, 8.0) as usize
}

fn sawtooth_is_glazed(spec: &BuildingSpec, tooth: usize, count: usize) -> bool {
    let glazed = (spec.skylights as usize).min(count);
    glazed == count || (tooth + spec.seed as usize % count) % count < glazed
}

fn add_sawtooth_end_walls(
    mesh: &mut MeshData,
    spec: &BuildingSpec,
    w: f32,
    l: f32,
    h: f32,
    color: [f32; 4],
) {
    let count = sawtooth_count(w);
    let run = w / count as f32;
    for end in [-1.0_f32, 1.0] {
        let z = end * l / 2.0;
        for tooth in 0..count {
            let x0 = -w / 2.0 + tooth as f32 * run;
            let x1 = x0 + run * 0.78;
            let x2 = x0 + run;
            for source in [
                [[x0, h, z], [x1, h + spec.roof_height, z], [x1, h, z]],
                [[x1, h, z], [x1, h + spec.roof_height, z], [x2, h, z]],
            ] {
                let mut p = source;
                let mut uv = source.map(|point| {
                    [
                        (point[0] + w / 2.0) * SURFACE_UV_DENSITY,
                        point[1] * SURFACE_UV_DENSITY,
                    ]
                });
                if end > 0.0 {
                    p.swap(1, 2);
                    uv.swap(1, 2);
                }
                mesh.triangle_auto(p, uv, color);
            }
        }
    }
}

fn add_sawtooth_glazing(mesh: &mut MeshData, spec: &BuildingSpec, w: f32, l: f32, h: f32) {
    let e = 0.28;
    let count = sawtooth_count(w);
    let run = (w + e * 2.0) / count as f32;
    for tooth in 0..count {
        if !sawtooth_is_glazed(spec, tooth, count) {
            continue;
        }
        let x0 = -w / 2.0 - e + tooth as f32 * run;
        let high = x0 + run * 0.78;
        let low = x0 + run;
        mesh.quad_auto(
            [
                [high, h + spec.roof_height, -l / 2.0 - e],
                [high, h + spec.roof_height, l / 2.0 + e],
                [low, h, l / 2.0 + e],
                [low, h, -l / 2.0 - e],
            ],
            [
                [0.0, 0.0],
                [(l + e * 2.0) * SURFACE_UV_DENSITY, 0.0],
                [
                    (l + e * 2.0) * SURFACE_UV_DENSITY,
                    run.hypot(spec.roof_height) * SURFACE_UV_DENSITY,
                ],
                [0.0, run.hypot(spec.roof_height) * SURFACE_UV_DENSITY],
            ],
            [0.72, 0.78, 0.84, 1.0],
        );
    }
}

fn add_roof(mesh: &mut MeshData, spec: &BuildingSpec, w: f32, l: f32, h: f32) {
    let c = [1.0; 4];
    let e = 0.28;
    // U always follows the eave/ridge and V always climbs the roof. Besides
    // making the scale independent of the building footprint, this is what
    // makes tile courses run parallel to the eave and standing seams drain
    // from ridge to gutter.
    let density = SURFACE_UV_DENSITY;
    let along = (l + e * 2.0) * density;
    match spec.roof_style {
        RoofStyle::Flat => mesh.closed_box_uv(
            Vec3::new(0.0, h + 0.18, 0.0),
            Vec3::new(w + e * 2.0, 0.36, l + e * 2.0),
            c,
            Some(density),
        ),
        RoofStyle::Gable => {
            let r = spec.roof_height;
            let eave = h - r * e / (w / 2.0).max(0.1);
            let slope = (w / 2.0 + e).hypot(h + r - eave) * density;
            mesh.roof_quad(
                [
                    [-w / 2.0 - e, eave, -l / 2.0 - e],
                    [-w / 2.0 - e, eave, l / 2.0 + e],
                    [0.0, h + r, l / 2.0 + e],
                    [0.0, h + r, -l / 2.0 - e],
                ],
                [[0.0, 0.0], [along, 0.0], [along, slope], [0.0, slope]],
                [true, true, false, true],
                c,
            );
            mesh.roof_quad(
                [
                    [0.0, h + r, -l / 2.0 - e],
                    [0.0, h + r, l / 2.0 + e],
                    [w / 2.0 + e, eave, l / 2.0 + e],
                    [w / 2.0 + e, eave, -l / 2.0 - e],
                ],
                [[0.0, slope], [along, slope], [along, 0.0], [0.0, 0.0]],
                [false, true, true, true],
                c,
            );
        }
        RoofStyle::Hip => {
            let r = spec.roof_height;
            let ridge = (l * 0.22).min(l / 2.0 - e);
            let eave = h - r * e / (w / 2.0).max(0.1);
            let slope = (w / 2.0 + e).hypot(h + r - eave) * density;
            let ridge_front = (l / 2.0 + e - ridge) * density;
            let ridge_back = (l / 2.0 + e + ridge) * density;
            mesh.roof_quad(
                [
                    [-w / 2.0 - e, eave, -l / 2.0 - e],
                    [-w / 2.0 - e, eave, l / 2.0 + e],
                    [0.0, h + r, ridge],
                    [0.0, h + r, -ridge],
                ],
                [
                    [0.0, 0.0],
                    [along, 0.0],
                    [ridge_back, slope],
                    [ridge_front, slope],
                ],
                [true, false, false, false],
                c,
            );
            mesh.roof_quad(
                [
                    [0.0, h + r, -ridge],
                    [0.0, h + r, ridge],
                    [w / 2.0 + e, eave, l / 2.0 + e],
                    [w / 2.0 + e, eave, -l / 2.0 - e],
                ],
                [
                    [ridge_front, slope],
                    [ridge_back, slope],
                    [along, 0.0],
                    [0.0, 0.0],
                ],
                [false, false, true, false],
                c,
            );
            let across = (w + e * 2.0) * density;
            let hip_slope = (l / 2.0 + e - ridge).hypot(h + r - eave) * density;
            mesh.roof_triangle(
                [
                    [-w / 2.0 - e, eave, -l / 2.0 - e],
                    [0.0, h + r, -ridge],
                    [w / 2.0 + e, eave, -l / 2.0 - e],
                ],
                [[0.0, 0.0], [across / 2.0, hip_slope], [across, 0.0]],
                [false, false, true],
                c,
            );
            mesh.roof_triangle(
                [
                    [w / 2.0 + e, eave, l / 2.0 + e],
                    [0.0, h + r, ridge],
                    [-w / 2.0 - e, eave, l / 2.0 + e],
                ],
                [[across, 0.0], [across / 2.0, hip_slope], [0.0, 0.0]],
                [false, false, true],
                c,
            );
        }
        RoofStyle::Shed => {
            let extension = spec.roof_height * e / w.max(0.1);
            let low = h - extension;
            let high = h + spec.roof_height + extension;
            let slope = (w + e * 2.0).hypot(high - low) * density;
            mesh.roof_quad(
                [
                    [-w / 2.0 - e, low, -l / 2.0 - e],
                    [-w / 2.0 - e, low, l / 2.0 + e],
                    [w / 2.0 + e, high, l / 2.0 + e],
                    [w / 2.0 + e, high, -l / 2.0 - e],
                ],
                [[0.0, 0.0], [along, 0.0], [along, slope], [0.0, slope]],
                [true; 4],
                c,
            );
        }
        RoofStyle::Mansard => {
            let r = spec.roof_height;
            let shoulder = w * 0.32;
            let lower_run = w / 2.0 - shoulder;
            let eave = h - r * 0.72 * e / lower_run.max(0.1);
            let lower = (w / 2.0 + e - shoulder).hypot(h + r * 0.72 - eave) * density;
            let upper = shoulder.hypot(r * 0.28) * density;
            let crown = lower + upper;
            for (panel, (a, b, va, vb)) in [
                ((-w / 2.0 - e, eave), (-shoulder, h + r * 0.72), 0.0, lower),
                ((-shoulder, h + r * 0.72), (0.0, h + r), lower, crown),
                ((0.0, h + r), (shoulder, h + r * 0.72), crown, lower),
                ((shoulder, h + r * 0.72), (w / 2.0 + e, eave), lower, 0.0),
            ]
            .into_iter()
            .enumerate()
            {
                mesh.roof_quad(
                    [
                        [a.0, a.1, -l / 2.0 - e],
                        [a.0, a.1, l / 2.0 + e],
                        [b.0, b.1, l / 2.0 + e],
                        [b.0, b.1, -l / 2.0 - e],
                    ],
                    [[0.0, va], [along, va], [along, vb], [0.0, vb]],
                    [panel == 0, true, panel == 3, true],
                    c,
                );
            }
        }
        RoofStyle::Sawtooth => {
            let count = sawtooth_count(w);
            let full_width = w + e * 2.0;
            let run = full_width / count as f32;
            for tooth in 0..count {
                let x0 = -w / 2.0 - e + tooth as f32 * run;
                let high = x0 + run * 0.78;
                let low = x0 + run;
                let rise = run.mul_add(0.78, 0.0).hypot(spec.roof_height) * density;
                mesh.roof_quad(
                    [
                        [x0, h, -l / 2.0 - e],
                        [x0, h, l / 2.0 + e],
                        [high, h + spec.roof_height, l / 2.0 + e],
                        [high, h + spec.roof_height, -l / 2.0 - e],
                    ],
                    [[0.0, 0.0], [along, 0.0], [along, rise], [0.0, rise]],
                    [true; 4],
                    c,
                );
                if !sawtooth_is_glazed(spec, tooth, count) {
                    let fall = (run * 0.22).hypot(spec.roof_height) * density;
                    mesh.roof_quad(
                        [
                            [high, h + spec.roof_height, -l / 2.0 - e],
                            [high, h + spec.roof_height, l / 2.0 + e],
                            [low, h, l / 2.0 + e],
                            [low, h, -l / 2.0 - e],
                        ],
                        [[0.0, 0.0], [along, 0.0], [along, fall], [0.0, fall]],
                        [true; 4],
                        c,
                    );
                }
            }
        }
    }
}

fn add_roof_caps(mesh: &mut MeshData, spec: &BuildingSpec, w: f32, l: f32, h: f32) {
    let r = spec.roof_height;
    let e = 0.28;
    // The cap shares the selected roof material. A slight vertex tint keeps
    // the joint readable without turning it into a white window-trim stripe.
    let color = [0.82, 0.82, 0.82, 1.0];
    let ridge_front = Vec3::new(0.0, h + r, -l / 2.0 - e);
    let ridge_back = Vec3::new(0.0, h + r, l / 2.0 + e);
    match spec.roof_style {
        RoofStyle::Gable => {
            mesh.roof_cap(
                ridge_front,
                ridge_back,
                Vec3::new(-w * 0.25, h + r * 0.5, 0.0),
                Vec3::new(w * 0.25, h + r * 0.5, 0.0),
                0.10,
                color,
            );
        }
        RoofStyle::Mansard => {
            mesh.roof_cap(
                ridge_front,
                ridge_back,
                Vec3::new(-w * 0.16, h + r * 0.86, 0.0),
                Vec3::new(w * 0.16, h + r * 0.86, 0.0),
                0.10,
                color,
            );
        }
        RoofStyle::Hip => {
            let ridge = (l * 0.22).min(l / 2.0 - e);
            let eave = h - r * e / (w / 2.0).max(0.1);
            let front = Vec3::new(0.0, h + r, -ridge);
            let back = Vec3::new(0.0, h + r, ridge);
            let left_front = Vec3::new(-w / 2.0 - e, eave, -l / 2.0 - e);
            let right_front = Vec3::new(w / 2.0 + e, eave, -l / 2.0 - e);
            let left_back = Vec3::new(-w / 2.0 - e, eave, l / 2.0 + e);
            let right_back = Vec3::new(w / 2.0 + e, eave, l / 2.0 + e);
            let left_face = (left_front + left_back + front + back) * 0.25;
            let right_face = (right_front + right_back + front + back) * 0.25;
            let front_face = (left_front + right_front + front) / 3.0;
            let back_face = (left_back + right_back + back) / 3.0;
            mesh.roof_cap(front, back, left_face, right_face, 0.10, color);
            mesh.roof_cap(front, left_front, left_face, front_face, 0.085, color);
            mesh.roof_cap(front, right_front, right_face, front_face, 0.085, color);
            mesh.roof_cap(back, left_back, left_face, back_face, 0.085, color);
            mesh.roof_cap(back, right_back, right_face, back_face, 0.085, color);
        }
        RoofStyle::Flat | RoofStyle::Shed | RoofStyle::Sawtooth => {}
    }
}

fn roof_surface_height(spec: &BuildingSpec, x: f32, z: f32) -> f32 {
    let w = spec.width.max(0.1);
    let l = spec.length.max(0.1);
    let h = spec.wall_height();
    let r = spec.roof_height;
    match spec.roof_style {
        RoofStyle::Flat => h + 0.36,
        RoofStyle::Gable => h + r * (1.0 - 2.0 * x.abs() / w).clamp(0.0, 1.0),
        RoofStyle::Hip => {
            let ridge = (l * 0.22).min(l / 2.0 - 0.01);
            let end = ((z.abs() - ridge) / (l / 2.0 - ridge).max(0.01)).max(0.0);
            let edge = (2.0 * x.abs() / w).max(end);
            h + r * (1.0 - edge).clamp(0.0, 1.0)
        }
        RoofStyle::Shed => h + r * (x / w + 0.5).clamp(0.0, 1.0),
        RoofStyle::Mansard => {
            let x = x.abs();
            let shoulder = w * 0.32;
            if x <= shoulder {
                h + r * (1.0 - x / shoulder.max(0.01) * 0.28)
            } else {
                h + r * 0.72 * (1.0 - (x - shoulder) / (w / 2.0 - shoulder).max(0.01))
            }
        }
        RoofStyle::Sawtooth => {
            let count = sawtooth_count(w);
            let run = w / count as f32;
            let local = (x + w / 2.0).rem_euclid(run);
            if local <= run * 0.78 {
                h + r * local / (run * 0.78).max(0.01)
            } else {
                h + r * (1.0 - (local - run * 0.78) / (run * 0.22).max(0.01))
            }
        }
    }
}

fn add_roof_details(g: &mut Geometry, spec: &BuildingSpec, lod: usize, facade_tint: [f32; 4]) {
    let count = spec.chimneys as usize;
    for chimney in 0..count {
        let side = if chimney % 2 == 0 { -1.0 } else { 1.0 };
        let x = if spec.roof_style == RoofStyle::Sawtooth {
            0.0
        } else {
            side * spec.width * 0.18
        };
        let z = -spec.length * 0.34
            + spec.length * 0.68 * (chimney as f32 + 1.0) / (count as f32 + 1.0);
        let visible_height = if spec.use_kind == BuildingUse::Industrial {
            1.8
        } else {
            1.05
        };
        let size = if spec.use_kind == BuildingUse::Industrial {
            0.88
        } else {
            0.54
        };
        let depth = size * 0.82;
        let apron = size + 0.20;
        let mut flashing = [
            [x - apron / 2.0, 0.0, z - apron / 2.0],
            [x - apron / 2.0, 0.0, z + apron / 2.0],
            [x + apron / 2.0, 0.0, z + apron / 2.0],
            [x + apron / 2.0, 0.0, z - apron / 2.0],
        ];
        for point in &mut flashing {
            point[1] = roof_surface_height(spec, point[0], point[2]) + 0.012;
        }
        g.detail.quad_auto(
            flashing,
            [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]],
            [0.42, 0.44, 0.44, 1.0],
        );
        let bottom = flashing
            .iter()
            .map(|point| point[1])
            .fold(f32::INFINITY, f32::min)
            - 0.08;
        let top = roof_surface_height(spec, x, z) + visible_height;
        let shaft_height = top - bottom;
        g.facade.closed_box_uv(
            Vec3::new(x, (bottom + top) * 0.5, z),
            Vec3::new(size, shaft_height, depth),
            facade_tint,
            Some(SURFACE_UV_DENSITY),
        );
        g.detail.closed_box(
            Vec3::new(x, top + 0.03, z),
            Vec3::new(size + 0.045, 0.06, depth + 0.045),
            [0.44, 0.45, 0.44, 1.0],
        );
        let flue_radius = size * 0.13;
        g.detail.vertical_prism(
            Vec3::new(x, top + 0.13, z),
            flue_radius,
            0.20,
            if lod == 0 { 10 } else { 6 },
            [0.30, 0.31, 0.31, 1.0],
        );
        g.detail.vertical_prism(
            Vec3::new(x, top + 0.245, z),
            flue_radius * 1.30,
            0.035,
            if lod == 0 { 10 } else { 6 },
            [0.26, 0.27, 0.27, 1.0],
        );
    }

    let count = spec.roof_vents as usize;
    for vent in 0..count {
        let row = vent % 2;
        let x = if spec.roof_style == RoofStyle::Sawtooth {
            -spec.width * 0.28 + spec.width * 0.56 * (vent as f32 + 0.5) / count.max(1) as f32
        } else {
            (row as f32 * 2.0 - 1.0) * spec.width * 0.22
        };
        let z =
            -spec.length * 0.38 + spec.length * 0.76 * (vent as f32 + 1.0) / (count as f32 + 1.0);
        let base = roof_surface_height(spec, x, z);
        let stem_height = if spec.use_kind == BuildingUse::Industrial {
            0.9
        } else {
            0.62
        };
        let radius = if spec.use_kind == BuildingUse::Industrial {
            0.24
        } else {
            0.17
        };
        g.detail.vertical_prism(
            Vec3::new(x, base + stem_height * 0.5, z),
            radius,
            stem_height,
            if lod == 0 { 10 } else { 6 },
            [0.82, 0.84, 0.84, 1.0],
        );
        g.detail.vertical_prism(
            Vec3::new(x, base + stem_height + 0.07, z),
            radius * 1.55,
            0.14,
            if lod == 0 { 10 } else { 6 },
            [0.68, 0.71, 0.72, 1.0],
        );
    }

    if lod == 0 {
        add_skylights(&mut g.glass, &mut g.detail, spec);
        if spec.rain_gutters {
            add_rain_gutters(&mut g.detail, spec);
        }
        if spec.entrance_canopy {
            add_entrance_canopy(&mut g.detail, spec);
        }
    }
}

fn add_skylights(glass: &mut MeshData, frame_mesh: &mut MeshData, spec: &BuildingSpec) {
    if spec.roof_style == RoofStyle::Sawtooth {
        return;
    }
    let count = spec.skylights as usize;
    let (max_w, max_l) = if spec.use_kind == BuildingUse::Industrial {
        (1.35, 1.8)
    } else {
        (0.95, 1.35)
    };
    let patch_w = (spec.width * 0.055).clamp(0.65, max_w);
    let patch_l = (spec.length / (count.max(1) as f32 + 1.0) * 0.40).clamp(0.85, max_l);
    for light in 0..count {
        let side = if light % 2 == 0 { -1.0 } else { 1.0 };
        let x = match spec.roof_style {
            RoofStyle::Gable | RoofStyle::Hip | RoofStyle::Mansard => side * spec.width * 0.26,
            RoofStyle::Flat | RoofStyle::Shed => side * spec.width * 0.16,
            RoofStyle::Sawtooth => unreachable!(),
        };
        let z =
            -spec.length * 0.38 + spec.length * 0.76 * (light as f32 + 1.0) / (count as f32 + 1.0);
        let mut p = [
            [x - patch_w / 2.0, 0.0, z - patch_l / 2.0],
            [x - patch_w / 2.0, 0.0, z + patch_l / 2.0],
            [x + patch_w / 2.0, 0.0, z + patch_l / 2.0],
            [x + patch_w / 2.0, 0.0, z - patch_l / 2.0],
        ];
        for point in &mut p {
            point[1] = roof_surface_height(spec, point[0], point[2]) + 0.012;
        }
        glass.quad_auto(
            p,
            [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]],
            [0.38, 0.52, 0.62, 1.0],
        );

        // Four strips follow the same bilinear roof patch. Unlike an
        // axis-aligned box they cannot lift off a pitched or hipped roof.
        let p = p.map(Vec3::from_array);
        let u = (0.055 / patch_w).clamp(0.03, 0.15);
        let v = (0.055 / patch_l).clamp(0.03, 0.15);
        let on_patch =
            |u: f32, v: f32| p[0].lerp(p[3], u).lerp(p[1].lerp(p[2], u), v) + Vec3::Y * 0.004;
        let i00 = on_patch(u, v);
        let i01 = on_patch(u, 1.0 - v);
        let i11 = on_patch(1.0 - u, 1.0 - v);
        let i10 = on_patch(1.0 - u, v);
        for strip in [
            [p[0], p[1], i01, i00],
            [i10, i11, p[2], p[3]],
            [p[0], i00, i10, p[3]],
            [i01, p[1], p[2], i11],
        ] {
            frame_mesh.quad_auto(
                strip.map(|point| point.to_array()),
                [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]],
                [0.78, 0.81, 0.82, 1.0],
            );
        }
    }
}

fn add_rain_gutters(mesh: &mut MeshData, spec: &BuildingSpec) {
    let h = spec.wall_height();
    let detail = [0.80, 0.83, 0.84, 1.0];
    let pipe_radius = 0.036;
    let sides = 10;
    let gutter_end = (spec.length / 2.0 - 0.12).max(0.3);
    let (left_y, right_y) = match spec.roof_style {
        RoofStyle::Shed => (h, h + spec.roof_height),
        RoofStyle::Flat => (h + 0.34, h + 0.34),
        _ => (h, h),
    };
    for (x, y) in [
        (-spec.width / 2.0 - 0.31, left_y),
        (spec.width / 2.0 + 0.31, right_y),
    ] {
        mesh.closed_box(
            Vec3::new(x, y - 0.055, 0.0),
            Vec3::new(0.075, 0.085, gutter_end * 2.0),
            detail,
        );
    }
    if matches!(spec.roof_style, RoofStyle::Hip | RoofStyle::Flat) {
        let y = if spec.roof_style == RoofStyle::Flat {
            h + 0.34
        } else {
            h
        };
        let gutter_end_x = (spec.width / 2.0 - 0.12).max(0.3);
        for z in [-spec.length / 2.0 - 0.31, spec.length / 2.0 + 0.31] {
            mesh.closed_box(
                Vec3::new(0.0, y - 0.055, z),
                Vec3::new(gutter_end_x * 2.0, 0.085, 0.075),
                detail,
            );
        }
    }
    if spec.roof_style == RoofStyle::Sawtooth {
        let teeth = sawtooth_count(spec.width);
        for valley in 1..teeth {
            let x = -spec.width / 2.0 + spec.width * valley as f32 / teeth as f32;
            mesh.closed_box(
                Vec3::new(x, h + 0.035, 0.0),
                Vec3::new(0.065, 0.065, gutter_end * 2.0),
                detail,
            );
        }
    }
    for sign_x in [-1.0_f32, 1.0] {
        let gutter_x = sign_x * (spec.width / 2.0 + 0.31);
        let pipe_x = sign_x * (spec.width / 2.0 + pipe_radius * 0.72);
        let top = if sign_x < 0.0 { left_y } else { right_y };
        for sign_z in [-1.0_f32, 1.0] {
            // The pipe sits on the side facade, slightly inboard of the
            // corner. A short diagonal elbow reaches the gutter above it.
            let pipe_z = sign_z * (spec.length / 2.0 - 0.26).max(0.1);
            let pipe_top = Vec3::new(pipe_x, top - 0.12, pipe_z);
            mesh.prism_between(
                Vec3::new(pipe_x, 0.06, pipe_z),
                pipe_top,
                pipe_radius,
                sides,
                detail,
            );
            mesh.prism_between(
                pipe_top,
                Vec3::new(gutter_x, top - 0.055, pipe_z),
                pipe_radius,
                sides,
                detail,
            );
        }
    }
}

fn add_entrance_canopy(mesh: &mut MeshData, spec: &BuildingSpec) {
    let width = if spec.use_kind == BuildingUse::Industrial {
        (spec.width * 0.72).min(28.0)
    } else {
        (spec.width * 0.58).min(10.0)
    };
    let depth = if spec.use_kind == BuildingUse::Industrial {
        2.4
    } else {
        1.55
    };
    let y = if spec.use_kind == BuildingUse::Industrial {
        (spec.floor_height * 0.82).min(spec.wall_height() - 0.2)
    } else {
        2.55_f32.min(spec.wall_height() - 0.1)
    };
    let z = -spec.length / 2.0 - depth / 2.0;
    mesh.closed_box(
        Vec3::new(0.0, y, z),
        Vec3::new(width, 0.16, depth),
        [0.72, 0.75, 0.76, 1.0],
    );
    for x in [-width / 2.0 + 0.18, width / 2.0 - 0.18] {
        if spec.use_kind == BuildingUse::Industrial {
            // Cantilever beams leave every loading lane unobstructed.
            mesh.closed_box(
                Vec3::new(x, y - 0.16, z + 0.18),
                Vec3::new(0.12, 0.12, depth - 0.30),
                [0.62, 0.65, 0.67, 1.0],
            );
        } else {
            mesh.closed_box(
                Vec3::new(x, y * 0.5, z - depth / 2.0 + 0.12),
                Vec3::new(0.12, y, 0.12),
                [0.62, 0.65, 0.67, 1.0],
            );
        }
    }
}

fn has_balcony(spec: &BuildingSpec, floor: usize) -> bool {
    floor > 0 && spec.balconies && floor.is_multiple_of(spec.balcony_every.max(1) as usize)
}

/// A centred balcony must end on bay boundaries, not through the windows at
/// arbitrary percentages of the facade. Matching the span parity to the total
/// bay count is what keeps both ends equally far from the nearest opening.
fn balcony_width(spec: &BuildingSpec) -> f32 {
    let bays = (spec.width / spec.window_spacing).floor().max(1.0) as usize;
    let desired = bays as f32 * 0.5;
    let span_bays = (1..=bays)
        .filter(|candidate| candidate % 2 == bays % 2)
        .min_by(|a, b| ((*a as f32 - desired).abs()).total_cmp(&(*b as f32 - desired).abs()))
        .unwrap_or(1);
    spec.width / bays as f32 * span_bays as f32
}

fn entrance_layout(spec: &BuildingSpec) -> (Vec<f32>, f32, f32) {
    let height = (if spec.use_kind == BuildingUse::Industrial {
        spec.floor_height * 0.78
    } else {
        2.15
    })
    .min(spec.wall_height() - 0.1);
    if spec.use_kind != BuildingUse::Industrial {
        return (vec![0.0], 1.25, height);
    }
    let count = spec.loading_doors.clamp(1, 10) as usize;
    let width = if count == 1 {
        (spec.width * 0.32).min(7.0)
    } else {
        (spec.width / (count as f32 + 1.0) * 0.62).clamp(2.6, 5.5)
    };
    let centers = (0..count)
        .map(|door| -spec.width / 2.0 + spec.width * (door as f32 + 1.0) / (count as f32 + 1.0))
        .collect();
    (centers, width, height)
}

fn add_openings(g: &mut Geometry, spec: &BuildingSpec, lod: usize) {
    let h = spec.wall_height();
    let inset = 0.025;
    let (door_centers, door_w, door_h) = entrance_layout(spec);
    let sides = [
        (spec.width, -spec.length / 2.0 - inset, false),
        (spec.length, -spec.width / 2.0 - inset, true),
    ];
    for (side_index, (span, plane, side)) in sides.into_iter().enumerate() {
        let bays = (span / spec.window_spacing).floor().max(1.0) as usize;
        for mirror in 0..2 {
            for floor in 0..spec.floors as usize {
                for bay in 0..bays {
                    let center = -span / 2.0 + span * (bay as f32 + 0.5) / bays as f32;
                    let y = floor as f32 * spec.floor_height + spec.floor_height * 0.56;
                    let (ww, wh) = if spec.use_kind == BuildingUse::Commercial && floor == 0 {
                        (spec.window_width * 1.35, spec.window_height * 1.25)
                    } else {
                        (spec.window_width, spec.window_height)
                    };
                    // The opening owns its whole rectangle, not just the bay
                    // containing its centre. Wide industrial doors commonly
                    // span two or three window bays.
                    if side_index == 0
                        && mirror == 0
                        && door_centers
                            .iter()
                            .any(|door| (center - door).abs() < door_w / 2.0 + ww / 2.0 + 0.18)
                        && y - wh / 2.0 < door_h + 0.12
                    {
                        continue;
                    }
                    if side_index == 0
                        && mirror == 0
                        && has_balcony(spec, floor)
                        && center.abs() < 0.5 + ww / 2.0 + 0.12
                    {
                        continue;
                    }
                    let signed_plane = if mirror == 0 { plane } else { -plane };
                    let p = window_quad(
                        center,
                        y,
                        ww.min(span / bays as f32 * 0.82),
                        wh.min(h * 0.8),
                        signed_plane,
                        side,
                        mirror == 0,
                    );
                    g.glass.quad(
                        p.0,
                        p.1,
                        [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]],
                        [1.0; 4],
                    );
                    if lit(
                        spec.seed,
                        side_index,
                        mirror,
                        floor,
                        bay,
                        spec.lit_window_share,
                    ) {
                        let mut lp = p.0;
                        for point in &mut lp {
                            point[0] += p.1[0] * inset;
                            point[1] += p.1[1] * inset;
                            point[2] += p.1[2] * inset;
                        }
                        g.lit.quad(
                            lp,
                            p.1,
                            [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]],
                            [1.0; 4],
                        );
                    }
                    if lod == 0 {
                        add_window_trim(
                            &mut g.trim,
                            center,
                            y,
                            ww,
                            wh,
                            signed_plane,
                            side,
                            mirror == 0,
                        );
                    }
                }
            }
        }
    }
    let door_plane = -spec.length / 2.0 - 0.055;
    for center in &door_centers {
        g.door.quad(
            [
                [center - door_w / 2.0, 0.02, door_plane],
                [center - door_w / 2.0, door_h, door_plane],
                [center + door_w / 2.0, door_h, door_plane],
                [center + door_w / 2.0, 0.02, door_plane],
            ],
            [0.0, 0.0, -1.0],
            [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]],
            [1.0; 4],
        );
    }
    if lod == 0 && spec.use_kind == BuildingUse::Industrial {
        let frame = 0.11;
        let frame_color = [0.42, 0.45, 0.47, 1.0];
        for center in &door_centers {
            for x in [center - door_w / 2.0, center + door_w / 2.0] {
                g.trim.box_(
                    Vec3::new(x, door_h / 2.0, door_plane - 0.015),
                    Vec3::new(frame, door_h, frame),
                    frame_color,
                );
            }
            g.trim.box_(
                Vec3::new(*center, door_h, door_plane - 0.015),
                Vec3::new(door_w + frame, frame, frame),
                frame_color,
            );
            let panels = (door_h / 0.9).floor().max(1.0) as usize;
            for panel in 1..panels {
                g.trim.box_(
                    Vec3::new(
                        *center,
                        door_h * panel as f32 / panels as f32,
                        door_plane - 0.015,
                    ),
                    Vec3::new(door_w, frame * 0.45, frame),
                    frame_color,
                );
            }
        }
    }
    if spec.balconies {
        let balcony_door_w = 1.0;
        let balcony_door_h = 2.15_f32.min(spec.floor_height - 0.25);
        for floor in 1..spec.floors as usize {
            if !has_balcony(spec, floor) {
                continue;
            }
            let bottom = floor as f32 * spec.floor_height + 0.13;
            g.glass.quad(
                [
                    [-balcony_door_w / 2.0, bottom, door_plane],
                    [-balcony_door_w / 2.0, bottom + balcony_door_h, door_plane],
                    [balcony_door_w / 2.0, bottom + balcony_door_h, door_plane],
                    [balcony_door_w / 2.0, bottom, door_plane],
                ],
                [0.0, 0.0, -1.0],
                [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]],
                [1.0; 4],
            );
            if lod == 0 {
                let frame = 0.075;
                let center_y = bottom + balcony_door_h / 2.0;
                for x in [-balcony_door_w / 2.0, balcony_door_w / 2.0] {
                    g.trim.box_(
                        Vec3::new(x, center_y, door_plane - 0.012),
                        Vec3::new(frame, balcony_door_h + frame, frame),
                        [1.0; 4],
                    );
                }
                g.trim.box_(
                    Vec3::new(0.0, bottom + balcony_door_h, door_plane - 0.012),
                    Vec3::new(balcony_door_w + frame, frame, frame),
                    [1.0; 4],
                );
                for y in [bottom, bottom + balcony_door_h * 0.46] {
                    g.trim.box_(
                        Vec3::new(0.0, y, door_plane - 0.012),
                        Vec3::new(balcony_door_w + frame, frame, frame),
                        [1.0; 4],
                    );
                }
            }
        }
    }
}

fn window_quad(
    c: f32,
    y: f32,
    w: f32,
    h: f32,
    plane: f32,
    side: bool,
    near: bool,
) -> ([[f32; 3]; 4], [f32; 3]) {
    let sign = if near { -1.0 } else { 1.0 };
    let (mut points, normal) = if side {
        (
            [
                [plane, y - h / 2.0, c + w / 2.0],
                [plane, y + h / 2.0, c + w / 2.0],
                [plane, y + h / 2.0, c - w / 2.0],
                [plane, y - h / 2.0, c - w / 2.0],
            ],
            [sign, 0.0, 0.0],
        )
    } else {
        (
            [
                [c - w / 2.0, y - h / 2.0, plane],
                [c - w / 2.0, y + h / 2.0, plane],
                [c + w / 2.0, y + h / 2.0, plane],
                [c + w / 2.0, y - h / 2.0, plane],
            ],
            [0.0, 0.0, sign],
        )
    };
    // The base winding faces the negative side on both axes. Reverse the
    // opposite wall so back-face culling and the authored normal agree.
    if !near {
        points.swap(1, 3);
    }
    (points, normal)
}

#[allow(clippy::too_many_arguments)]
fn add_window_trim(
    mesh: &mut MeshData,
    c: f32,
    y: f32,
    w: f32,
    h: f32,
    plane: f32,
    side: bool,
    near: bool,
) {
    let bar = 0.075;
    if side {
        let x = plane;
        mesh.box_(
            Vec3::new(x, y, c - w / 2.0),
            Vec3::new(bar, h + bar, bar),
            [1.0; 4],
        );
        mesh.box_(
            Vec3::new(x, y, c + w / 2.0),
            Vec3::new(bar, h + bar, bar),
            [1.0; 4],
        );
        mesh.box_(
            Vec3::new(x, y - h / 2.0, c),
            Vec3::new(bar, bar, w),
            [1.0; 4],
        );
        mesh.box_(
            Vec3::new(x, y + h / 2.0, c),
            Vec3::new(bar, bar, w),
            [1.0; 4],
        );
        mesh.box_(
            Vec3::new(x, y, c),
            Vec3::new(bar, h + bar, bar * 0.72),
            [1.0; 4],
        );
        mesh.box_(Vec3::new(x, y, c), Vec3::new(bar, bar * 0.72, w), [1.0; 4]);
    } else {
        let z = plane;
        mesh.box_(
            Vec3::new(c - w / 2.0, y, z),
            Vec3::new(bar, h + bar, bar),
            [1.0; 4],
        );
        mesh.box_(
            Vec3::new(c + w / 2.0, y, z),
            Vec3::new(bar, h + bar, bar),
            [1.0; 4],
        );
        mesh.box_(
            Vec3::new(c, y - h / 2.0, z),
            Vec3::new(w, bar, bar),
            [1.0; 4],
        );
        mesh.box_(
            Vec3::new(c, y + h / 2.0, z),
            Vec3::new(w, bar, bar),
            [1.0; 4],
        );
        mesh.box_(
            Vec3::new(c, y, z),
            Vec3::new(bar * 0.72, h + bar, bar),
            [1.0; 4],
        );
        mesh.box_(Vec3::new(c, y, z), Vec3::new(w, bar * 0.72, bar), [1.0; 4]);
    }
    let _ = near;
}

fn add_balconies(mesh: &mut MeshData, spec: &BuildingSpec) {
    let front = -spec.length / 2.0 - spec.balcony_depth / 2.0;
    for floor in 1..spec.floors {
        if !has_balcony(spec, floor as usize) {
            continue;
        }
        let y = floor as f32 * spec.floor_height + 0.12;
        let width = balcony_width(spec);
        mesh.closed_box(
            Vec3::new(0.0, y, front),
            Vec3::new(width, 0.18, spec.balcony_depth),
            [1.0; 4],
        );
        mesh.closed_box(
            Vec3::new(0.0, y + 1.0, front - spec.balcony_depth / 2.0),
            Vec3::new(width, 0.075, 0.075),
            [1.0; 4],
        );
        // Use an odd number of intervals so no upright stands on the centre
        // line directly in front of the balcony door.
        let mut intervals = (width / 1.2).ceil().max(1.0) as usize;
        if intervals.is_multiple_of(2) {
            intervals += 1;
        }
        for post in 0..=intervals {
            let x = -width / 2.0 + width * post as f32 / intervals as f32;
            mesh.closed_box(
                Vec3::new(x, y + 0.55, front - spec.balcony_depth / 2.0),
                Vec3::new(0.055, 0.95, 0.055),
                [1.0; 4],
            );
        }
        // Close both short sides. The front-only railing looks plausible
        // head-on but becomes an unsafe floating comb from every side view.
        for x in [-width / 2.0, width / 2.0] {
            mesh.closed_box(
                Vec3::new(x, y + 1.0, front),
                Vec3::new(0.075, 0.075, spec.balcony_depth),
                [1.0; 4],
            );
            for z in [front, front + spec.balcony_depth / 2.0] {
                mesh.closed_box(
                    Vec3::new(x, y + 0.55, z),
                    Vec3::new(0.055, 0.95, 0.055),
                    [1.0; 4],
                );
            }
        }
    }
}

fn lit(seed: u64, side: usize, mirror: usize, floor: usize, bay: usize, share: f32) -> bool {
    let mut x = seed
        ^ (side as u64).wrapping_mul(0x9e3779b97f4a7c15)
        ^ (mirror as u64).wrapping_mul(0xbf58476d1ce4e5b9)
        ^ (floor as u64).wrapping_mul(0x94d049bb133111eb)
        ^ bay as u64;
    x ^= x >> 30;
    x = x.wrapping_mul(0xbf58476d1ce4e5b9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94d049bb133111eb);
    ((x ^ (x >> 31)) as u32) as f32 / (u32::MAX as f32) < share
}

#[derive(Clone, Copy)]
struct SurfaceSample {
    albedo: [u8; 3],
    height: f32,
    roughness: u8,
    metallic: u8,
}

const SURFACE_SIZE: i32 = 512;
const SURFACE_WORLD_SIZE: f32 = 8.0;
const SURFACE_UV_DENSITY: f32 = 1.0 / SURFACE_WORLD_SIZE;

fn surface_hash(kind: usize, x: i32, y: i32, salt: u64) -> u64 {
    let x = x as u64;
    let y = y as u64;
    let mut hash = x.wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ y.wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ (kind as u64).wrapping_mul(0x1656_67B1_9E37_79F9)
        ^ salt.wrapping_mul(0xD6E8_FEB8_6659_FD93);
    hash = (hash ^ (hash >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    hash = (hash ^ (hash >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    hash ^ (hash >> 31)
}

fn random_signed(kind: usize, x: i32, y: i32, salt: u64) -> f32 {
    let bits = (surface_hash(kind, x, y, salt) >> 40) as u32;
    bits as f32 / 0x00ff_ffff as f32 * 2.0 - 1.0
}

/// Seamless interpolated noise. Every selected cell size divides the texture,
/// so both value and gradient agree across the repeated border.
fn smooth_noise(kind: usize, x: i32, y: i32, cell: i32, salt: u64) -> f32 {
    let cells = SURFACE_SIZE / cell;
    let x = x.rem_euclid(SURFACE_SIZE);
    let y = y.rem_euclid(SURFACE_SIZE);
    let gx = x / cell;
    let gy = y / cell;
    let tx = (x % cell) as f32 / cell as f32;
    let ty = (y % cell) as f32 / cell as f32;
    let sx = tx * tx * (3.0 - 2.0 * tx);
    let sy = ty * ty * (3.0 - 2.0 * ty);
    let sample = |dx: i32, dy: i32| {
        random_signed(
            kind,
            (gx + dx).rem_euclid(cells),
            (gy + dy).rem_euclid(cells),
            salt,
        )
    };
    let top = sample(0, 0) + (sample(1, 0) - sample(0, 0)) * sx;
    let bottom = sample(0, 1) + (sample(1, 1) - sample(0, 1)) * sx;
    top + (bottom - top) * sy
}

fn surface_color(base: (i16, i16, i16), light: f32, warm: f32) -> [u8; 3] {
    let channel = |base: i16, offset: f32| (base as f32 + offset).round().clamp(0.0, 255.0) as u8;
    [
        channel(base.0, light + warm),
        channel(base.1, light + warm * 0.25),
        channel(base.2, light - warm * 0.75),
    ]
}

/// One texel of a seamless technical surface definition. Colour, relief and
/// material response all use the same joint mask, so mortar can never drift
/// away from the normal map again.
fn surface_sample(kind: usize, x: i32, y: i32) -> SurfaceSample {
    let x = x.rem_euclid(SURFACE_SIZE);
    let y = y.rem_euclid(SURFACE_SIZE);
    let grain = random_signed(kind, x, y, 1);
    let broad = smooth_noise(kind, x, y, 256, 2);
    let medium = smooth_noise(kind, x, y, 64, 3);
    match kind {
        0 => SurfaceSample {
            albedo: surface_color(
                (226, 222, 211),
                broad * 5.0 + medium * 2.2 + grain * 1.1,
                broad * 1.2,
            ),
            height: 128.0 + smooth_noise(kind, x, y, 16, 4) * 1.5 + grain * 0.35,
            roughness: (226.0 + medium * 5.0).clamp(205.0, 240.0) as u8,
            metallic: 0,
        },
        1 | 2 => {
            const BRICK_W: i32 = 16;
            const BRICK_H: i32 = 8;
            let row = y / BRICK_H;
            let shifted = x + (row & 1) * (BRICK_W / 2);
            let local_x = shifted.rem_euclid(BRICK_W);
            let local_y = y.rem_euclid(BRICK_H);
            let brick = (shifted / BRICK_W).rem_euclid(SURFACE_SIZE / BRICK_W);
            let mortar = local_y == 0 || local_x == 0;
            let unit = random_signed(kind, brick, row, 20);
            let edge = local_x
                .min(BRICK_W - local_x)
                .min(local_y.min(BRICK_H - local_y)) as f32;
            let (mortar_color, brick_color) = if kind == 1 {
                ((166, 158, 146), (174, 72, 48))
            } else {
                ((181, 174, 157), (202, 158, 83))
            };
            SurfaceSample {
                albedo: if mortar {
                    surface_color(mortar_color, broad * 4.0 + grain * 0.8, 0.0)
                } else {
                    surface_color(
                        brick_color,
                        unit * 11.0 + broad * 5.0 + medium * 2.0 + grain * 1.2,
                        unit * 3.0,
                    )
                },
                height: if mortar {
                    112.0 + medium
                } else {
                    128.0 + edge.min(2.0) * 2.1 + grain * 0.25
                },
                roughness: if mortar {
                    238
                } else {
                    (204.0 + unit * 8.0).clamp(188.0, 220.0) as u8
                },
                metallic: 0,
            }
        }
        3 => {
            let pore = random_signed(kind, x, y, 31) > 0.975;
            SurfaceSample {
                albedo: surface_color(
                    (183, 186, 185),
                    broad * 7.0 + medium * 3.0 + grain * 1.4 - if pore { 9.0 } else { 0.0 },
                    broad * 0.6,
                ),
                height: 128.0 + smooth_noise(kind, x, y, 16, 32) * 1.4 + grain * 0.45
                    - if pore { 4.0 } else { 0.0 },
                roughness: (232.0 + medium * 7.0).clamp(214.0, 246.0) as u8,
                metallic: 0,
            }
        }
        4 => {
            const PANEL: i32 = 32;
            let local = x.rem_euclid(PANEL);
            let distance = local.min(PANEL - local);
            let seam = distance < 2;
            let panel = x / PANEL;
            let unit = random_signed(kind, panel, 0, 40);
            SurfaceSample {
                albedo: surface_color(
                    if seam {
                        (119, 126, 130)
                    } else {
                        (148, 154, 157)
                    },
                    unit * 4.0 + broad * 3.5 + medium * 1.5 + grain * 0.7,
                    broad * 0.5,
                ),
                height: if seam {
                    141.0 - distance as f32 * 2.0
                } else {
                    128.0 + medium * 0.6 + grain * 0.15
                },
                roughness: (158.0 + unit * 10.0).clamp(135.0, 180.0) as u8,
                metallic: 176,
            }
        }
        5 => {
            const TILE: i32 = 16;
            let row = y / TILE;
            let shifted = x + (row & 1) * (TILE / 2);
            let local_x = shifted.rem_euclid(TILE);
            let local_y = y.rem_euclid(TILE);
            let tile = (shifted / TILE).rem_euclid(SURFACE_SIZE / TILE);
            let joint = local_y < 2 || local_x == 0;
            let unit = random_signed(kind, tile, row, 50);
            let crown = 1.0 - ((local_x as f32 + 0.5) / TILE as f32 * 2.0 - 1.0).abs();
            SurfaceSample {
                albedo: surface_color(
                    if joint { (105, 50, 38) } else { (164, 77, 52) },
                    unit * 10.0 + broad * 5.0 + medium * 2.0 + grain * 1.0,
                    unit * 2.5,
                ),
                height: if joint {
                    114.0
                } else {
                    128.0 + crown * 5.5 + (local_y as f32 / TILE as f32) * 1.2
                },
                roughness: (192.0 + unit * 10.0).clamp(170.0, 212.0) as u8,
                metallic: 0,
            }
        }
        6 => {
            const SLATE_W: i32 = 32;
            const SLATE_H: i32 = 16;
            let row = y / SLATE_H;
            let shifted = x + (row & 1) * (SLATE_W / 2);
            let local_x = shifted.rem_euclid(SLATE_W);
            let local_y = y.rem_euclid(SLATE_H);
            let tile = (shifted / SLATE_W).rem_euclid(SURFACE_SIZE / SLATE_W);
            let joint = local_y < 2 || local_x == 0;
            let unit = random_signed(kind, tile, row, 60);
            SurfaceSample {
                albedo: surface_color(
                    if joint { (54, 59, 63) } else { (79, 86, 91) },
                    unit * 8.0 + broad * 4.0 + medium * 1.5 + grain * 0.8,
                    -unit,
                ),
                height: if joint {
                    116.0
                } else {
                    128.0 + (local_y as f32 / SLATE_H as f32) * 2.5 + grain * 0.2
                },
                roughness: (211.0 + unit * 9.0).clamp(190.0, 230.0) as u8,
                metallic: 0,
            }
        }
        7 => {
            const PANEL: i32 = 32;
            let local = x.rem_euclid(PANEL);
            let distance = local.min(PANEL - local);
            let seam = distance < 2;
            let panel = x / PANEL;
            let unit = random_signed(kind, panel, 0, 70);
            SurfaceSample {
                albedo: surface_color(
                    if seam {
                        (122, 130, 134)
                    } else {
                        (146, 154, 158)
                    },
                    unit * 4.0 + broad * 4.0 + medium * 1.5 + grain * 0.6,
                    broad * 0.4,
                ),
                height: if seam {
                    142.0 - distance as f32 * 2.5
                } else {
                    128.0 + medium * 0.45 + grain * 0.12
                },
                roughness: (142.0 + unit * 10.0).clamp(120.0, 165.0) as u8,
                metallic: 218,
            }
        }
        _ => {
            let roll_seam = x.rem_euclid(64) < 2;
            let lap = y.rem_euclid(128) < 2;
            SurfaceSample {
                albedo: surface_color(
                    (68, 70, 68),
                    broad * 7.0 + medium * 3.0 + grain * 1.8
                        - if roll_seam || lap { 3.0 } else { 0.0 },
                    broad * 0.3,
                ),
                height: 128.0 + grain * 0.55 + if roll_seam || lap { 1.8 } else { 0.0 },
                roughness: (244.0 + medium * 5.0).clamp(232.0, 252.0) as u8,
                metallic: 0,
            }
        }
    }
}

/// Builds the registered PBR channels from one shared sample field. Besides
/// being substantially faster than evaluating the procedural material three
/// times, this makes channel drift structurally impossible.
fn surface_textures(kind: usize) -> (Image, Image, Image) {
    const S: u32 = SURFACE_SIZE as u32;
    let samples = (0..S)
        .flat_map(|y| (0..S).map(move |x| surface_sample(kind, x as i32, y as i32)))
        .collect::<Vec<_>>();
    let at = |x: i32, y: i32| {
        samples[(y.rem_euclid(SURFACE_SIZE) * SURFACE_SIZE + x.rem_euclid(SURFACE_SIZE)) as usize]
    };
    let capacity = (S * S * 4) as usize;
    let mut albedo = Vec::with_capacity(capacity);
    let mut normal = Vec::with_capacity(capacity);
    let mut metal_rough = Vec::with_capacity(capacity);
    for y in 0..S {
        for x in 0..S {
            let sample = at(x as i32, y as i32);
            albedo.extend_from_slice(&[sample.albedo[0], sample.albedo[1], sample.albedo[2], 255]);
            let dx = at(x as i32 + 1, y as i32).height - at(x as i32 - 1, y as i32).height;
            let dy = at(x as i32, y as i32 + 1).height - at(x as i32, y as i32 - 1).height;
            let direction = Vec3::new(-dx * 0.025, -dy * 0.025, 1.0).normalize();
            let encode = |channel: f32| ((channel * 0.5 + 0.5) * 255.0) as u8;
            normal.extend_from_slice(&[
                encode(direction.x),
                encode(direction.y),
                encode(direction.z),
                255,
            ]);
            // glTF convention: G = roughness, B = metallic.
            metal_rough.extend_from_slice(&[255, sample.roughness, sample.metallic, 255]);
        }
    }
    (
        surface_image(albedo, true),
        surface_image(normal, false),
        surface_image(metal_rough, false),
    )
}

fn surface_image(data: Vec<u8>, srgb: bool) -> Image {
    const S: u32 = SURFACE_SIZE as u32;
    let mut image = Image::new(
        Extent3d {
            width: S,
            height: S,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        if srgb {
            TextureFormat::Rgba8UnormSrgb
        } else {
            TextureFormat::Rgba8Unorm
        },
        RenderAssetUsages::default(),
    );
    let base = image.data.take().unwrap_or_default();
    let (data, mip_level_count) = crate::with_mipmaps(base, S, S, None);
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = mip_level_count;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 16,
        ..default()
    });
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_winding_matches_normals(mesh: &MeshData) {
        for triangle in mesh.indices.as_chunks::<3>().0 {
            let a = Vec3::from_array(mesh.positions[triangle[0] as usize]);
            let b = Vec3::from_array(mesh.positions[triangle[1] as usize]);
            let c = Vec3::from_array(mesh.positions[triangle[2] as usize]);
            let face = (b - a).cross(c - a).normalize_or_zero();
            let normal = Vec3::from_array(mesh.normals[triangle[0] as usize]);
            assert!(
                face.dot(normal) > 0.999,
                "triangle winding and vertex normal disagree: {face:?} / {normal:?}"
            );
        }
    }

    #[test]
    fn all_roofs_and_lods_make_meshes() {
        for roof_style in [
            RoofStyle::Gable,
            RoofStyle::Hip,
            RoofStyle::Flat,
            RoofStyle::Shed,
            RoofStyle::Mansard,
            RoofStyle::Sawtooth,
        ] {
            let spec = BuildingSpec {
                roof_style,
                ..Default::default()
            };
            for lod in 0..3 {
                let geometry = geometry(&spec, lod);
                assert!(!geometry.facade.positions.is_empty());
                assert!(!geometry.roof.positions.is_empty());
            }
        }
    }

    #[test]
    fn generated_faces_have_matching_winding_and_normals() {
        for roof_style in [
            RoofStyle::Gable,
            RoofStyle::Hip,
            RoofStyle::Flat,
            RoofStyle::Shed,
            RoofStyle::Mansard,
            RoofStyle::Sawtooth,
        ] {
            let geometry = geometry(
                &BuildingSpec {
                    roof_style,
                    balconies: true,
                    ..Default::default()
                },
                0,
            );
            for mesh in [
                &geometry.facade,
                &geometry.roof,
                &geometry.glass,
                &geometry.lit,
                &geometry.trim,
                &geometry.balcony,
                &geometry.detail,
                &geometry.door,
            ] {
                assert_winding_matches_normals(mesh);
            }
        }
    }

    #[test]
    fn pitched_roof_uvs_run_along_eave_then_up_slope() {
        let geometry = geometry(
            &BuildingSpec {
                roof_style: RoofStyle::Gable,
                ..Default::default()
            },
            0,
        );
        let uv = &geometry.roof.uvs[..4];
        assert_eq!(uv[0][1], uv[1][1]);
        assert!(uv[1][0] > uv[0][0]);
        assert_eq!(uv[1][0], uv[2][0]);
        assert!(uv[2][1] > uv[1][1]);
    }

    #[test]
    fn pitched_roofs_have_undersides_and_outer_fascia() {
        for roof_style in [
            RoofStyle::Gable,
            RoofStyle::Hip,
            RoofStyle::Shed,
            RoofStyle::Mansard,
            RoofStyle::Sawtooth,
        ] {
            let roof = geometry(
                &BuildingSpec {
                    roof_style,
                    ..Default::default()
                },
                0,
            )
            .roof;
            assert!(roof.normals.iter().any(|normal| normal[1] < -0.2));
            assert!(roof.normals.iter().any(|normal| normal[1].abs() < 0.001));
            assert_winding_matches_normals(&roof);
        }
    }

    #[test]
    fn roof_overhangs_continue_through_the_wall_crown() {
        let at_x =
            |a: [f32; 3], b: [f32; 3], x: f32| a[1] + (b[1] - a[1]) * (x - a[0]) / (b[0] - a[0]);
        for roof_style in [RoofStyle::Gable, RoofStyle::Mansard] {
            let spec = BuildingSpec {
                roof_style,
                ..Default::default()
            };
            let roof = geometry(&spec, 0).roof;
            assert!(
                (at_x(roof.positions[0], roof.positions[3], -spec.width / 2.0)
                    - spec.wall_height())
                .abs()
                    < 0.0001
            );
        }

        let spec = BuildingSpec {
            roof_style: RoofStyle::Shed,
            ..Default::default()
        };
        let roof = geometry(&spec, 0).roof;
        let low = at_x(roof.positions[0], roof.positions[3], -spec.width / 2.0);
        let high = at_x(roof.positions[0], roof.positions[3], spec.width / 2.0);
        assert!((low - spec.wall_height()).abs() < 0.0001);
        assert!((high - spec.wall_height() - spec.roof_height).abs() < 0.0001);
    }

    #[test]
    fn flat_roof_texture_is_not_stretched_over_the_whole_building() {
        let spec = BuildingSpec {
            roof_style: RoofStyle::Flat,
            ..Default::default()
        };
        let roof = geometry(&spec, 0).roof;
        let top = &roof.uvs[16..20];
        assert!(top.iter().map(|uv| uv[0]).fold(0.0_f32, f32::max) > 1.0);
        assert!(top.iter().map(|uv| uv[1]).fold(0.0_f32, f32::max) > 1.0);
        assert!(
            roof.normals
                .iter()
                .any(|normal| *normal == [0.0, -1.0, 0.0])
        );
        assert!(roof.positions.iter().any(|point| {
            (point[1] - spec.wall_height()).abs() < 0.0001 && point[0].abs() > spec.width / 2.0
        }));
    }

    #[test]
    fn shed_roof_high_side_is_closed_to_the_roofline() {
        let spec = BuildingSpec {
            roof_style: RoofStyle::Shed,
            ..Default::default()
        };
        let facade = geometry(&spec, 0).facade;
        let roofline = spec.wall_height() + spec.roof_height;
        let high_corners = facade
            .positions
            .iter()
            .filter(|p| {
                (p[0] - spec.width / 2.0).abs() < 0.0001
                    && (p[1] - roofline).abs() < 0.0001
                    && (p[2].abs() - spec.length / 2.0).abs() < 0.0001
            })
            .count();
        assert!(high_corners >= 2);
    }

    #[test]
    fn hip_roof_has_a_ridge_and_four_grat_caps() {
        let spec = BuildingSpec {
            roof_style: RoofStyle::Hip,
            ..Default::default()
        };
        let mut caps = MeshData::default();
        add_roof_caps(
            &mut caps,
            &spec,
            spec.width,
            spec.length,
            spec.wall_height(),
        );
        assert_eq!(caps.indices.len() / 6, 5 * 2);
        assert!(caps.normals.iter().any(|normal| normal[1] > 0.5));
        assert_winding_matches_normals(&caps);
    }

    #[test]
    fn industrial_gate_reserves_every_window_bay_it_covers() {
        let spec = BuildingSpec {
            use_kind: BuildingUse::Industrial,
            width: 48.0,
            length: 27.0,
            floors: 1,
            floor_height: 6.2,
            window_spacing: 6.0,
            window_width: 2.4,
            window_height: 2.0,
            loading_doors: 4,
            ..Default::default()
        };
        let geometry = geometry(&spec, 0);
        let (door_centers, door_w, door_h) = entrance_layout(&spec);
        assert_eq!(geometry.door.positions.len(), 4 * door_centers.len());
        for window in geometry.glass.positions.as_chunks::<4>().0 {
            if window.iter().all(|p| p[2] < -spec.length / 2.0) {
                let left = window.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min);
                let right = window
                    .iter()
                    .map(|p| p[0])
                    .fold(f32::NEG_INFINITY, f32::max);
                let bottom = window.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
                let overlaps_gate = door_centers
                    .iter()
                    .any(|center| right > center - door_w / 2.0 && left < center + door_w / 2.0)
                    && bottom < door_h;
                assert!(!overlaps_gate);
            }
        }
    }

    #[test]
    fn sawtooth_factory_has_closed_roof_and_north_lights() {
        let spec = BuildingSpec {
            use_kind: BuildingUse::Industrial,
            width: 64.0,
            length: 38.0,
            floors: 1,
            floor_height: 7.5,
            roof_style: RoofStyle::Sawtooth,
            roof_height: 3.2,
            skylights: 4,
            ..Default::default()
        };
        let geometry = geometry(&spec, 0);
        assert!(!geometry.roof.positions.is_empty());
        assert!(!geometry.glass.positions.is_empty());
        assert!(
            geometry
                .facade
                .positions
                .iter()
                .any(|point| point[1] > spec.wall_height() + 3.0)
        );
        assert_winding_matches_normals(&geometry.roof);
        assert_winding_matches_normals(&geometry.glass);
    }

    #[test]
    fn roof_accessories_are_detailed_and_lod_bounded() {
        let spec = BuildingSpec {
            chimneys: 2,
            roof_vents: 3,
            skylights: 4,
            rain_gutters: true,
            entrance_canopy: true,
            ..Default::default()
        };
        let near = geometry(&spec, 0);
        assert!(!near.detail.positions.is_empty());
        assert!(
            near.facade
                .positions
                .iter()
                .any(|point| point[1] > spec.wall_height())
        );
        assert_winding_matches_normals(&near.detail);
        let far = geometry(&spec, 2);
        assert!(far.detail.positions.is_empty());
    }

    #[test]
    fn balcony_railings_close_the_front_and_both_sides() {
        let spec = BuildingSpec {
            floors: 2,
            width: 12.0,
            length: 9.0,
            balcony_every: 1,
            balcony_depth: 1.2,
            ..Default::default()
        };
        let mut balcony = MeshData::default();
        add_balconies(&mut balcony, &spec);
        let width = balcony_width(&spec);
        let mut intervals = (width / 1.2).ceil().max(1.0) as usize;
        if intervals.is_multiple_of(2) {
            intervals += 1;
        }
        // Slab, front rail, adaptive front posts, two side rails and four
        // side posts. Every exposed balcony component is a closed six-face box.
        let boxes = 1 + 1 + intervals + 1 + 2 + 4;
        assert_eq!(balcony.positions.len(), boxes * 6 * 4);
        assert!(
            balcony
                .normals
                .iter()
                .any(|normal| *normal == [0.0, -1.0, 0.0])
        );
        assert_winding_matches_normals(&balcony);
    }

    #[test]
    fn explicit_balconies_are_not_limited_to_residential_buildings() {
        let geometry = geometry(
            &BuildingSpec {
                use_kind: BuildingUse::Commercial,
                floors: 2,
                balconies: true,
                balcony_every: 1,
                ..Default::default()
            },
            0,
        );
        assert!(!geometry.balcony.positions.is_empty());
    }

    #[test]
    fn balcony_edges_land_between_window_bays() {
        let spec = BuildingSpec {
            width: 15.0,
            window_spacing: 2.6,
            window_width: 1.25,
            ..Default::default()
        };
        let bays = (spec.width / spec.window_spacing).floor() as usize;
        let width = balcony_width(&spec);
        for bay in 0..bays {
            let center = -spec.width / 2.0 + spec.width * (bay as f32 + 0.5) / bays as f32;
            for edge in [-width / 2.0, width / 2.0] {
                assert!(
                    (center - edge).abs() > spec.window_width / 2.0 + 0.1,
                    "balcony edge {edge} intersects window bay at {center}"
                );
            }
        }
    }

    #[test]
    fn residential_balconies_replace_central_windows_with_doors() {
        let spec = BuildingSpec {
            use_kind: BuildingUse::Residential,
            floors: 3,
            width: 15.0,
            length: 11.0,
            window_spacing: 2.6,
            balconies: true,
            balcony_every: 1,
            ..Default::default()
        };
        let geometry = geometry(&spec, 0);

        assert_eq!(geometry.door.positions.len(), 4);
        let balcony_doors = geometry
            .glass
            .positions
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pane| {
                let is_front = pane.iter().all(|point| point[2] < -spec.length / 2.0);
                let center_x = pane.iter().map(|point| point[0]).sum::<f32>() / 4.0;
                let bottom = pane
                    .iter()
                    .map(|point| point[1])
                    .fold(f32::INFINITY, f32::min);
                let top = pane
                    .iter()
                    .map(|point| point[1])
                    .fold(f32::NEG_INFINITY, f32::max);
                is_front
                    && center_x.abs() < 0.1
                    && bottom > spec.floor_height
                    && top - bottom > spec.window_height
            })
            .count();
        assert_eq!(balcony_doors, 2);
    }

    #[test]
    fn all_surface_channels_wrap_on_the_same_seam() {
        for kind in 0..9 {
            for y in 0..SURFACE_SIZE {
                let before = surface_sample(kind, -1, y);
                let end = surface_sample(kind, SURFACE_SIZE - 1, y);
                assert_eq!(before.albedo, end.albedo);
                assert_eq!(before.height, end.height);
                assert_eq!(before.roughness, end.roughness);
                assert_eq!(before.metallic, end.metallic);

                let start = surface_sample(kind, 0, y);
                let after = surface_sample(kind, SURFACE_SIZE, y);
                assert_eq!(start.albedo, after.albedo);
                assert_eq!(start.height, after.height);
                assert_eq!(start.roughness, after.roughness);
                assert_eq!(start.metallic, after.metallic);
            }
        }
    }

    #[test]
    fn procedural_surfaces_have_mips_and_anisotropic_filtering() {
        let image = surface_textures(7).0;
        assert_eq!(image.texture_descriptor.size.width, 512);
        assert_eq!(image.texture_descriptor.size.height, 512);
        assert_eq!(image.texture_descriptor.mip_level_count, 10);
        let ImageSampler::Descriptor(sampler) = image.sampler else {
            panic!("procedural surface needs an explicit tiling sampler");
        };
        assert_eq!(sampler.anisotropy_clamp, 16);
        assert_eq!(sampler.mipmap_filter, ImageFilterMode::Linear);
    }

    #[test]
    fn old_one_metre_period_does_not_repeat() {
        for kind in 0..9 {
            let changed = (0..32)
                .filter(|sample| {
                    let x = sample * 13 + 3;
                    let y = sample * 7 + 5;
                    surface_sample(kind, x, y).albedo != surface_sample(kind, x + 64, y).albedo
                })
                .count();
            assert!(
                changed >= 20,
                "surface {kind} still repeats every 64 texels"
            );
        }
    }

    #[test]
    fn night_pattern_is_stable_and_not_uniform() {
        let pattern: Vec<bool> = (0..32).map(|bay| lit(42, 0, 0, 1, bay, 0.4)).collect();
        assert_eq!(
            pattern,
            (0..32)
                .map(|bay| lit(42, 0, 0, 1, bay, 0.4))
                .collect::<Vec<_>>()
        );
        assert!(pattern.iter().any(|v| *v) && pattern.iter().any(|v| !*v));
    }
}
