// The glTF the game loads.
//
// One file per tree, holding one node per level of detail named the way the
// app reads levels off a model (`crown_LOD0`, `crown_LOD1`, …, see
// `sim_core::train::lod_level`). Every level is a single primitive with a
// single material, because `world_render::scatter` spawns one entity per
// primitive per level: bark and foliage in one atlas means four entities per
// tree instead of eight, and one instanced draw per level instead of two.
//
// Buffer and texture live beside the file, not inside it. A species' three
// shape variants share nothing, but its summer, autumn and winter files point
// at the *same* `.bin` — the autumn tree is the same wood with another sheet
// of leaves, and the winter one is that wood with the leaf primitive left out.
// Only the season in play is ever loaded, so the duplication costs disk and
// nothing else.

import { writeFileSync } from 'node:fs';
import { join } from 'node:path';

const FLOAT = 5126;
const UNSIGNED_INT = 5125;
const UNSIGNED_SHORT = 5123;
const ARRAY_BUFFER = 34962;
const ELEMENT_ARRAY_BUFFER = 34963;

/**
 * Packs geometries into one binary buffer and remembers where each one went,
 * so several glTF files can reference the same `.bin`.
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

  /**
   * Writes a geometry and returns the accessor descriptions the glTF needs.
   * Indices are 16 bit wherever the mesh fits, which is most levels.
   */
  add(geometry) {
    const count = geometry.positions.length / 3;
    const position = Buffer.from(new Float32Array(geometry.positions).buffer);
    const normal = Buffer.from(new Float32Array(geometry.normals).buffer);
    const uv = Buffer.from(new Float32Array(geometry.uvs).buffer);
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
        index: this.#append(index),
      },
      lengths: {
        position: position.length,
        normal: normal.length,
        uv: uv.length,
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
 * @param {Array} options.levels `[{ name, packed, material }]`, finest first
 * @param {Array} options.materials `[{ name, texture, alphaCutoff, doubleSided, baseColorFactor }]`
 * @param {Array} options.textures image file names, indexed by material
 * @param {string} options.copyright
 */
export function writeGltf(options) {
  const bufferViews = [];
  const accessors = [];
  const meshes = [];
  const nodes = [];

  const view = (offset, length, target, stride) => {
    bufferViews.push({ buffer: 0, byteOffset: offset, byteLength: length, target, ...(stride ? { byteStride: stride } : {}) });
    return bufferViews.length - 1;
  };
  const accessor = (bufferView, componentType, count, type, extra = {}) => {
    accessors.push({ bufferView, componentType, count, type, ...extra });
    return accessors.length - 1;
  };

  for (const level of options.levels) {
    const p = level.packed;
    const primitive = {
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
      },
      indices: accessor(
        view(p.offsets.index, p.lengths.index, ELEMENT_ARRAY_BUFFER),
        p.short ? UNSIGNED_SHORT : UNSIGNED_INT,
        p.indexCount,
        'SCALAR',
      ),
      material: level.material,
    };
    meshes.push({ name: level.name, primitives: [primitive] });
    nodes.push({ name: level.name, mesh: meshes.length - 1 });
  }

  const gltf = {
    asset: {
      version: '2.0',
      generator: 'tools/trees/build_trees.mjs (Connected Rails, geometry by ez-tree)',
      copyright: options.copyright,
    },
    scene: 0,
    scenes: [{ name: options.name, nodes: nodes.map((_, i) => i) }],
    nodes,
    meshes,
    materials: options.materials.map((m) => ({
      name: m.name,
      pbrMetallicRoughness: {
        baseColorTexture: { index: m.texture },
        baseColorFactor: m.baseColorFactor ?? [1, 1, 1, 1],
        metallicFactor: 0,
        roughnessFactor: m.roughnessFactor ?? 0.85,
      },
      // Foliage is a cut-out, and the trunk shares the sheet — one masked,
      // double-sided material for the whole tree.
      alphaMode: 'MASK',
      alphaCutoff: m.alphaCutoff ?? 0.5,
      doubleSided: true,
    })),
    textures: options.textures.map((_, i) => ({ source: i, sampler: 0 })),
    images: options.textures.map((uri) => ({ uri })),
    samplers: [{ magFilter: 9729, minFilter: 9987, wrapS: 33071, wrapT: 33071 }],
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
