// Copies the render-resolution corner of the frame into exact-size textures for
// FSR (fsr.rs). The main target keeps the display resolution while the scene is
// drawn at render resolution into its lower-left corner; the FSR shaders sample
// their inputs by UV, so they need sources sized to exactly what they read.

@group(0) @binding(0) var src_color: texture_2d<f32>;
@group(0) @binding(1) var src_depth: texture_2d<f32>;
@group(0) @binding(2) var src_motion_vectors: texture_2d<f32>;
@group(0) @binding(3) var dst_color: texture_storage_2d<rgba16float, write>;
@group(0) @binding(4) var dst_depth: texture_storage_2d<r32float, write>;
@group(0) @binding(5) var dst_motion_vectors: texture_storage_2d<rg16float, write>;

@compute @workgroup_size(8, 8)
fn crop(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(dst_color);
    if (id.x >= size.x || id.y >= size.y) {
        return;
    }
    let pixel = vec2<i32>(id.xy);
    textureStore(dst_color, id.xy, textureLoad(src_color, pixel, 0));
    textureStore(dst_depth, id.xy, vec4<f32>(textureLoad(src_depth, pixel, 0).x, 0.0, 0.0, 0.0));
    textureStore(dst_motion_vectors, id.xy, textureLoad(src_motion_vectors, pixel, 0));
}
