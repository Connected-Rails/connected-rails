// Close meadow foliage: Bevy's standard mesh vertex path with root-pinned
// wind deformation, followed by the same weather-aware PBR fragment path as
// the rest of the outdoor world.

#import bevy_pbr::{
    mesh_bindings::mesh,
    mesh_functions,
    forward_io::{VertexOutput, FragmentOutput},
    view_transformations::position_world_to_clip,
    mesh_view_bindings::{globals, view},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
}
#import world_render::weather::{Weather, weather_pbr}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> grass_weather: Weather;

struct GrassSettings {
    bands: vec4<f32>,
    fades: vec4<f32>,
    options: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(101) var<uniform> grass_settings: GrassSettings;

struct GrassVertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    // xy = compressed radial LOD/random; zw = local leaf coordinates.
    @location(3) data: vec4<f32>,
}

fn grass_coverage(lod: f32, distance_to_camera: f32) -> f32 {
    if grass_settings.options.x < 0.5 {
        return 0.0;
    }
    if lod < 0.5 {
        return 1.0 - smoothstep(
            grass_settings.bands.x - grass_settings.fades.x,
            grass_settings.bands.x + grass_settings.fades.x,
            distance_to_camera,
        );
    } else if lod < 1.5 {
        return smoothstep(
            grass_settings.bands.x - grass_settings.fades.x,
            grass_settings.bands.x + grass_settings.fades.x,
            distance_to_camera,
        ) * (1.0 - smoothstep(
            grass_settings.bands.y - grass_settings.fades.y,
            grass_settings.bands.y + grass_settings.fades.y,
            distance_to_camera,
        ));
    } else if lod < 2.5 {
        return smoothstep(
            grass_settings.bands.y - grass_settings.fades.y,
            grass_settings.bands.y + grass_settings.fades.y,
            distance_to_camera,
        ) * (1.0 - smoothstep(
            grass_settings.bands.z - grass_settings.fades.z,
            grass_settings.bands.z + grass_settings.fades.z,
            distance_to_camera,
        ));
    }
    return 1.0 - smoothstep(
        grass_settings.bands.w - grass_settings.fades.w,
        grass_settings.bands.w + grass_settings.fades.w,
        distance_to_camera,
    );
}

// A conservative two-metre guard around the visible interval. Entire blades
// outside it can be rejected in the vertex stage, before triangle setup and
// fragment shading. The guard is wider than any blade, so the visible result
// remains byte-for-byte governed by grass_coverage in the fragment stage.
fn grass_can_be_visible(lod: f32, distance_to_camera: f32) -> bool {
    if grass_settings.options.x < 0.5 {
        return false;
    }
    let guard = 2.0;
    if lod < 0.5 {
        return distance_to_camera < grass_settings.bands.x + grass_settings.fades.x + guard;
    } else if lod < 1.5 {
        return distance_to_camera > grass_settings.bands.x - grass_settings.fades.x - guard
            && distance_to_camera < grass_settings.bands.y + grass_settings.fades.y + guard;
    } else if lod < 2.5 {
        return distance_to_camera > grass_settings.bands.y - grass_settings.fades.y - guard
            && distance_to_camera < grass_settings.bands.z + grass_settings.fades.z + guard;
    }
    return distance_to_camera < grass_settings.bands.w + grass_settings.fades.w + guard;
}

@vertex
fn vertex(vertex: GrassVertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);

#ifdef VERTEX_NORMALS
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        vertex.normal,
        vertex.instance_index,
    );
#endif

#ifdef VERTEX_POSITIONS
    var world_position = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(vertex.position, 1.0),
    );
#ifdef VERTEX_COLORS
    // Alpha carries normalized blade height: zero pins the root, one lets the
    // pointed tip take the full displacement. Two frequencies keep a whole
    // cell from moving as one rigid sheet.
    let weight = pow(clamp(vertex.color.a, 0.0, 1.0), 1.65);
    let speed = length(grass_weather.wind.xy);
    var direction = vec2(0.72, 0.69);
    if speed > 0.05 {
        direction = normalize(grass_weather.wind.xy);
    }
    let phase = dot(world_position.xz, vec2(0.19, 0.13))
        + globals.time * (1.25 + speed * 0.09);
    let gust = sin(phase) + sin(phase * 2.17 + world_position.x * 0.31) * 0.32;
    let amplitude = 0.012 + min(speed, 18.0) * 0.006;
    let offset = direction * (gust * amplitude * weight);
    world_position = vec4(
        world_position.x + offset.x,
        world_position.y,
        world_position.z + offset.y,
        world_position.w,
    );
#endif
    out.world_position = world_position;
    out.position = position_world_to_clip(world_position.xyz);
#ifdef VERTEX_UVS_A
    let distance_to_camera = distance(world_position.xyz, view.world_position.xyz);
    if !grass_can_be_visible(vertex.data.x * 3.0, distance_to_camera) {
        // z > w is beyond the far clip plane. All vertices of an invisible
        // blade take this path, so the rasterizer receives no triangle.
        out.position = vec4(0.0, 0.0, 2.0, 1.0);
    }
#endif
#endif

#ifdef VERTEX_UVS_A
    out.uv = vertex.data.xy;
#endif
#ifdef VERTEX_UVS_B
    out.uv_b = vertex.data.zw;
#endif
#ifdef VERTEX_TANGENTS
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(
        world_from_local,
        vertex.tangent,
        vertex.instance_index,
    );
#endif
#ifdef VERTEX_COLORS
    // StandardMaterial must see opaque vertex colour; alpha has already done
    // its private job as the bend weight above.
    // RGB was normalized into an 8-bit 0..1.5 range in the mesh. Expanding
    // restores the authored HDR tint while cutting twelve bytes per vertex.
    out.color = vec4(vertex.color.rgb * 1.5, 1.0);
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex.instance_index;
#endif
#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex.instance_index,
        world_from_local[3],
    );
#endif
    return out;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    // Mesh residency is cell based, but visible detail must never be. UV.x
    // identifies close / near / far / hero-detail geometry and UV.y is one
    // stable random number for the complete blade. Selecting here makes the
    // bands perfectly radial around the current camera every frame; no 32 m
    // entity AABB can leak into the picture as a square patch.
    let distance_to_camera = distance(in.world_position.xyz, view.world_position.xyz);
    let coverage = grass_coverage(in.uv.x * 3.0, distance_to_camera);
    if in.uv.y > coverage {
        discard;
    }

    var pbr_input = pbr_input_from_standard_material(in, is_front);
    // UV-B is local to the leaf. A soft central vein, slightly darker edges
    // and a lengthwise chlorophyll gradient keep nearby blades from reading
    // as flat, uniformly green polygons. UV-A.y adds stable plant-to-plant
    // variation rather than animated noise.
    let across = abs(in.uv_b.x - 0.5) * 2.0;
    let along = clamp(in.uv_b.y, 0.0, 1.0);
    let vein = 1.0 - smoothstep(0.035, 0.16, abs(in.uv_b.x - 0.5));
    let edge_shade = 1.0 - 0.13 * smoothstep(0.62, 1.0, across);
    let length_shade = mix(0.82, 1.08, along);
    let plant_variation = 0.94 + in.uv.y * 0.12;
    let blade_tint = edge_shade * length_shade * plant_variation * (1.0 + vein * 0.055);
    pbr_input.material.base_color = vec4(
        pbr_input.material.base_color.rgb * blade_tint,
        pbr_input.material.base_color.a,
    );
    pbr_input.material.perceptual_roughness = mix(0.90, 0.76, along);
    pbr_input = weather_pbr(grass_weather, globals.time, pbr_input);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
