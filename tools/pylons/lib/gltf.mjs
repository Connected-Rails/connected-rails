// The glTF the game loads.
//
// One file per mast variant, one node per level of detail named the way the app
// reads levels off a model (`mast_LOD0` … `mast_LOD3`, see
// `sim_core::train::lod_level`, `world_render::scatter`). A level may hold more
// than one primitive: the structure (and metal fitting hardware), the
// insulators, and on lattice masts the concrete foundations. The renderer
// spawns one entity per primitive per level; a missing part simply leaves that
// level with one primitive fewer.
//
// **Three maps per material, painted rather than photographed.** A flat base
// colour with a metallic and a roughness number is a correct PBR material and
// still reads as plastic, because what makes galvanising look like galvanising
// is the *spangle* — zinc crystals a couple of centimetres across, each at its
// own gloss — and that lives in the roughness, not in the colour and not in the
// relief. So a mast carries a base colour, an ORM and a normal map
// (`lib/texture.mjs`), all tiling once per metre: the UVs are in metres, so the
// grain is the same size on a 36 cm leg as on a 6 cm brace. Nine tiling PNGs of
// 1024 px serve every structural surface; the printed warning sign contributes
// its own colour/ORM/normal triplet.
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
    // A geometry with no texture coordinates leaves them out rather than
    // writing zeroes: eight bytes a vertex is a third of a mesh that carries
    // its colour in its vertices and samples nothing (`tools/cars`).
    const uv = Buffer.from(new Float32Array(geometry.uvs ?? []).buffer);
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
        uv: uv.length ? this.#append(uv) : 0,
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

/** One glTF material out of the pipeline's description of it. */
function materialOf(m) {
  const material = {
    name: m.name,
    pbrMetallicRoughness: {
      baseColorFactor: m.color,
      metallicFactor: m.metallic ?? 0,
      roughnessFactor: m.roughness ?? 0.8,
    },
    doubleSided: m.doubleSided ?? false,
  };
  if (m.maps) {
    // The base colour is sRGB and the other two are linear; in glTF that
    // is implied by the slot, not declared, so the same image must never
    // be used for both kinds.
    if (m.maps.colour !== undefined) {
      material.pbrMetallicRoughness.baseColorTexture = { index: m.maps.colour };
    }
    // A material may carry a base colour and nothing else — a baked atlas
    // out of a photogrammetry tool has no separate roughness or normal map
    // to give, and writing slots that point nowhere is worse than not
    // writing them.
    if (m.maps.orm !== undefined) {
      material.pbrMetallicRoughness.metallicRoughnessTexture = { index: m.maps.orm };
      material.occlusionTexture = { index: m.maps.orm };
    }
    // `null` explicitly disables a tangent-space map. Smooth low-sided
    // tubes need that: their tangent changes at each mesh segment and even
    // a nearly flat map otherwise reveals every segment as a vertical seam.
    if (m.normalScale !== null && m.maps.normal !== undefined) {
      material.normalTexture = { index: m.maps.normal, scale: m.normalScale ?? 1 };
    }
  }
  // A lamp: the factor is the colour, the strength how bright it is against a
  // physically exposed sky (`KHR_materials_emissive_strength`, which the
  // game's loader reads). A signal lens without the strength is a red dot at
  // noon and nothing at all beside a sunset.
  if (m.emissive) {
    material.emissiveFactor = m.emissive;
    if (m.emissiveStrength !== undefined && m.emissiveStrength !== 1) {
      material.extensions = {
        KHR_materials_emissive_strength: { emissiveStrength: m.emissiveStrength },
      };
    }
  }
  return material;
}

/**
 * The accessors and primitives of one packed geometry, appended to the file's
 * tables. Shared by the flat and the hierarchical writer.
 */
function primitiveOf(p, material, tables) {
  const { bufferViews, accessors } = tables;
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
  return {
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
      ...(p.lengths.uv
        ? {
            TEXCOORD_0: accessor(
              view(p.offsets.uv, p.lengths.uv, ARRAY_BUFFER, 8),
              FLOAT,
              p.count,
              'VEC2',
            ),
          }
        : {}),
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
  };
}

/** The file's tail: materials, images, samplers, the buffer. */
function assemble(options, tables, nodes, meshes, roots) {
  const extensionsUsed = new Set();
  const materials = options.materials.map((m) => {
    const material = materialOf(m);
    for (const name of Object.keys(material.extensions ?? {})) extensionsUsed.add(name);
    return material;
  });
  return {
    asset: {
      version: '2.0',
      generator: options.generator ?? 'tools/pylons/build_pylons.mjs (Connected Rails)',
      copyright: options.copyright,
    },
    ...(extensionsUsed.size ? { extensionsUsed: [...extensionsUsed].sort() } : {}),
    scene: 0,
    scenes: [{ name: options.name, nodes: roots }],
    nodes,
    meshes,
    materials,
    ...(options.images?.length
      ? {
          images: options.images.map((uri) => ({ uri })),
          textures: options.images.map((_, i) => ({ source: i, sampler: 0 })),
          samplers: [{ magFilter: 9729, minFilter: 9987, wrapS: 10497, wrapT: 10497 }],
        }
      : {}),
    accessors: tables.accessors,
    bufferViews: tables.bufferViews,
    buffers: [{ uri: options.buffer, byteLength: options.bufferLength }],
  };
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
 *   without it is a plain constant, which is what the insulators are. An
 *   `emissive` colour with an `emissiveStrength` makes a lamp.
 * @param {Array} options.images image file names, in the order the materials
 *   index them
 * @param {string} options.copyright
 * @param {string} options.generator
 */
export function writeGltf(options) {
  const tables = { bufferViews: [], accessors: [] };
  const meshes = [];
  const nodes = [];
  for (const level of options.levels) {
    const primitives = level.parts.map(({ packed, material }) =>
      primitiveOf(packed, material, tables),
    );
    meshes.push({ name: level.name, primitives });
    nodes.push({ name: level.name, mesh: meshes.length - 1 });
  }
  const gltf = assemble(options, tables, nodes, meshes, nodes.map((_, i) => i));
  writeFileSync(options.path, `${JSON.stringify(gltf, null, 1)}\n`, 'utf8');
}

/**
 * Writes one glTF file with a **node tree** — for a model whose parts move
 * against each other. A wind turbine's nacelle yaws on its tower and its rotor
 * turns on the nacelle, and the game moves them by node name, so the nodes have
 * to be real nodes with their own transforms rather than one flat list.
 *
 * `options.roots` is a list of nodes, each
 * `{ name, translation?, rotation?, scale?, extras?, parts?, children? }`:
 * `parts` are `[{ packed, material }]` and make the node a mesh node, `extras`
 * is any JSON the game may read off the node (the loader hands it over as the
 * node's `GltfExtras`), and `children` nest. Everything else is as
 * [`writeGltf`].
 */
export function writeGltfScene(options) {
  const tables = { bufferViews: [], accessors: [] };
  const meshes = [];
  const nodes = [];
  const emit = (node) => {
    const index = nodes.length;
    const out = { name: node.name };
    nodes.push(out);
    if (node.translation) out.translation = node.translation;
    if (node.rotation) out.rotation = node.rotation;
    if (node.scale) out.scale = node.scale;
    if (node.extras) out.extras = node.extras;
    if (node.parts?.length) {
      meshes.push({
        name: node.name,
        primitives: node.parts.map(({ packed, material }) => primitiveOf(packed, material, tables)),
      });
      out.mesh = meshes.length - 1;
    }
    if (node.children?.length) out.children = node.children.map(emit);
    return index;
  };
  const roots = options.roots.map(emit);
  const gltf = assemble(options, tables, nodes, meshes, roots);
  writeFileSync(options.path, `${JSON.stringify(gltf, null, 1)}\n`, 'utf8');
}

/** Writes the shared binary next to the glTF files that name it. */
export function writeBuffer(directory, name, buffer) {
  writeFileSync(join(directory, name), buffer);
}
