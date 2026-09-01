// The glTF the game loads.
//
// One file per mast variant, one node per level of detail named the way the app
// reads levels off a model (`mast_LOD0` … `mast_LOD3`, see
// `sim_core::train::lod_level`, `world_render::scatter`). A level may hold more
// than one primitive, because a mast is two materials at most: the structure
// (galvanised steel, spun concrete or creosoted wood) and the fittings (the
// insulator strings). The renderer spawns one entity per primitive per level,
// so the coarse levels drop the fittings and are one primitive again.
//
// **Three maps per material, painted rather than photographed.** A flat base
// colour with a metallic and a roughness number is a correct PBR material and
// still reads as plastic, because what makes galvanising look like galvanising
// is the *spangle* — zinc crystals a couple of centimetres across, each at its
// own gloss — and that lives in the roughness, not in the colour and not in the
// relief. So a mast carries a base colour, an ORM and a normal map
// (`lib/texture.mjs`), all tiling once per metre: the UVs are in metres, so the
// grain is the same size on a 36 cm leg as on a 6 cm brace. Nine PNGs of 256 px
// serve all thirty-three models — under a megabyte for the lot, because they
// are generated and not scanned.
//
// On top of that every vertex carries a **colour**: per-member value jitter and
// the dirt that gathers towards a mast's foot. The texture says what the
// material is, the vertex colour what has happened to this particular bar.

import { writeFileSync } from 'node:fs';
import { join } from 'node:path';

const FLOAT = 5126;
const UNSIGNED_INT = 5125;
const UNSIGNED_SHORT = 5123;
const UNSIGNED_BYTE = 5121;
const ARRAY_BUFFER = 34962;
const ELEMENT_ARRAY_BUFFER = 34963;

/**
 * Packs geometries into one binary buffer and remembers where each one went, so
 * several glTF files can share one `.bin`.
 */
export class BufferBuilder {
  constructor() {
    this.chunks = [];
    this.length = 0;
  }

  #append(bytes) {
    while (this.length % 4 !== 0) {
      this.chunks.push(Buffer.alloc(1));
      this.length += 1;
    }
    const offset = this.length;
    this.chunks.push(bytes);
    this.length += bytes.length;
    return offset;
  }

  /** Writes a geometry and returns what the glTF needs to point at it. */
  add(geometry) {
    const count = geometry.positions.length / 3;
    const position = Buffer.from(new Float32Array(geometry.positions).buffer);
    const normal = Buffer.from(new Float32Array(geometry.normals).buffer);
    const uv = Buffer.from(new Float32Array(geometry.uvs).buffer);
    // Four bytes a vertex rather than sixteen: weathering is a shade, and a
    // shade has nowhere near eight bits of meaning in it, let alone twenty-four.
    const colorBytes = new Uint8Array(count * 4);
    for (let i = 0; i < count * 4; i++) {
      colorBytes[i] = Math.max(0, Math.min(255, Math.round(geometry.colors[i] * 255)));
    }
    const color = Buffer.from(colorBytes.buffer, colorBytes.byteOffset, colorBytes.length);
    const short = count <= 65535;
    const indexArray = short
      ? new Uint16Array(geometry.indices)
      : new Uint32Array(geometry.indices);
    const index = Buffer.from(indexArray.buffer);

    const min = [Infinity, Infinity, Infinity];
    const max = [-Infinity, -Infinity, -Infinity];
    for (let i = 0; i < geometry.positions.length; i += 3) {
      for (let c = 0; c < 3; c++) {
        const v = geometry.positions[i + c];
        if (v < min[c]) min[c] = v;
        if (v > max[c]) max[c] = v;
      }
    }
    return {
      count,
      indexCount: geometry.indices.length,
      short,
      min,
      max,
      offsets: {
        position: this.#append(position),
        normal: this.#append(normal),
        uv: this.#append(uv),
        color: this.#append(color),
        index: this.#append(index),
      },
      lengths: {
        position: position.length,
        normal: normal.length,
        uv: uv.length,
        color: color.length,
        index: index.length,
      },
    };
  }

  toBuffer() {
    return Buffer.concat(this.chunks, this.length);
  }
}

/**
 * Writes one glTF file.
 *
 * @param {object} options
 * @param {string} options.path where to write
 * @param {string} options.name scene name
 * @param {string} options.buffer file name of the shared `.bin`
 * @param {number} options.bufferLength its byte length
 * @param {Array} options.levels `[{ name, parts: [{ packed, material }] }]`, finest first
 * @param {Array} options.materials `[{ name, color, metallic, roughness, maps }]`,
 *   where `maps` is `{ colour, orm, normal }` of image file names — a material
 *   without it is a plain constant, which is what the insulators are.
 * @param {Array} options.images image file names, in the order the materials
 *   index them
 * @param {string} options.copyright
 * @param {string} options.generator
 */
export function writeGltf(options) {
  const bufferViews = [];
  const accessors = [];
  const meshes = [];
  const nodes = [];

  const view = (offset, length, target, stride) => {
    bufferViews.push({
      buffer: 0,
      byteOffset: offset,
      byteLength: length,
      target,
      ...(stride ? { byteStride: stride } : {}),
    });
    return bufferViews.length - 1;
  };
  const accessor = (bufferView, componentType, count, type, extra = {}) => {
    accessors.push({ bufferView, componentType, count, type, ...extra });
    return accessors.length - 1;
  };

  for (const level of options.levels) {
    const primitives = level.parts.map(({ packed: p, material }) => ({
      attributes: {
        POSITION: accessor(
          view(p.offsets.position, p.lengths.position, ARRAY_BUFFER, 12),
          FLOAT,
          p.count,
          'VEC3',
          { min: p.min, max: p.max },
        ),
        NORMAL: accessor(
          view(p.offsets.normal, p.lengths.normal, ARRAY_BUFFER, 12),
          FLOAT,
          p.count,
          'VEC3',
        ),
        TEXCOORD_0: accessor(
          view(p.offsets.uv, p.lengths.uv, ARRAY_BUFFER, 8),
          FLOAT,
          p.count,
          'VEC2',
        ),
        COLOR_0: accessor(
          view(p.offsets.color, p.lengths.color, ARRAY_BUFFER, 4),
          UNSIGNED_BYTE,
          p.count,
          'VEC4',
          { normalized: true },
        ),
      },
      indices: accessor(
        view(p.offsets.index, p.lengths.index, ELEMENT_ARRAY_BUFFER),
        p.short ? UNSIGNED_SHORT : UNSIGNED_INT,
        p.indexCount,
        'SCALAR',
      ),
      material,
    }));
    meshes.push({ name: level.name, primitives });
    nodes.push({ name: level.name, mesh: meshes.length - 1 });
  }

  const gltf = {
    asset: {
      version: '2.0',
      generator: options.generator ?? 'tools/pylons/build_pylons.mjs (Connected Rails)',
      copyright: options.copyright,
    },
    scene: 0,
    scenes: [{ name: options.name, nodes: nodes.map((_, i) => i) }],
    nodes,
    meshes,
    materials: options.materials.map((m) => {
      const material = {
        name: m.name,
        pbrMetallicRoughness: {
          baseColorFactor: m.color,
          metallicFactor: m.metallic ?? 0,
          roughnessFactor: m.roughness ?? 0.8,
        },
        doubleSided: false,
      };
      if (m.maps) {
        // The base colour is sRGB and the other two are linear; in glTF that
        // is implied by the slot, not declared, so the same image must never
        // be used for both kinds.
        material.pbrMetallicRoughness.baseColorTexture = { index: m.maps.colour };
        material.pbrMetallicRoughness.metallicRoughnessTexture = { index: m.maps.orm };
        material.occlusionTexture = { index: m.maps.orm };
        material.normalTexture = { index: m.maps.normal, scale: m.normalScale ?? 1 };
      }
      return material;
    }),
    ...(options.images?.length
      ? {
          images: options.images.map((uri) => ({ uri })),
          textures: options.images.map((_, i) => ({ source: i, sampler: 0 })),
          samplers: [{ magFilter: 9729, minFilter: 9987, wrapS: 10497, wrapT: 10497 }],
        }
      : {}),
    accessors,
    bufferViews,
    buffers: [{ uri: options.buffer, byteLength: options.bufferLength }],
  };
  writeFileSync(options.path, `${JSON.stringify(gltf, null, 1)}\n`, 'utf8');
}

/** Writes the shared binary next to the glTF files that name it. */
export function writeBuffer(directory, name, buffer) {
  writeFileSync(join(directory, name), buffer);
}
