// Reading a binary glTF, far enough to get the triangles out of it.
//
// The kit this pipeline is built on ships `.glb`, and there is no Blender on
// the machines this repository is built on — so the reader is here, in the
// forty lines it actually takes for the subset a game asset uses: one buffer,
// float positions, normals and texture coordinates, unsigned indices, and a
// node tree of translations. Anything outside that subset throws rather than
// guessing, because a silently misread mesh is a car that looks nearly right.
//
// What comes out is this pipeline's own geometry layout (`tools/pylons/lib/
// geom.mjs`): flat arrays of positions, normals, uvs, colors and indices, with
// the node transforms already baked in, so the rest of the tool never sees a
// matrix.

import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';

const MAGIC = 0x46546c67; // "glTF"
const JSON_CHUNK = 0x4e4f534a;
const BIN_CHUNK = 0x004e4942;

/** Component types, as `[bytes, reader]`. */
const COMPONENTS = {
  5120: [1, (view, at) => view.getInt8(at)],
  5121: [1, (view, at) => view.getUint8(at)],
  5122: [2, (view, at) => view.getInt16(at, true)],
  5123: [2, (view, at) => view.getUint16(at, true)],
  5125: [4, (view, at) => view.getUint32(at, true)],
  5126: [4, (view, at) => view.getFloat32(at, true)],
};

const COUNTS = { SCALAR: 1, VEC2: 2, VEC3: 3, VEC4: 4, MAT4: 16 };

/**
 * Opens a glTF file, binary or not.
 *
 * A `.glb` carries its buffer inside it; a `.gltf` — which is what comes out
 * of an asset site's converter — names a `.bin` and a directory of images
 * beside itself. Both end up as one JSON and one buffer here, so nothing
 * downstream has to know which it was handed.
 */
export function readGltf(path) {
  if (path.toLowerCase().endsWith('.glb')) return { ...readGlb(path), base: dirname(path) };
  const json = JSON.parse(readFileSync(path, 'utf8'));
  const base = dirname(path);
  const buffers = (json.buffers ?? []).map((buffer) => {
    if (!buffer.uri) throw new Error(`${path}: a buffer without a uri outside a glb`);
    if (buffer.uri.startsWith('data:')) {
      return Buffer.from(buffer.uri.slice(buffer.uri.indexOf(',') + 1), 'base64');
    }
    return readFileSync(join(base, decodeURIComponent(buffer.uri)));
  });
  // Only one buffer is ever used by an exporter in practice; more than one
  // would need the bufferView's `buffer` index carried through every read, and
  // guessing wrong there is silently the wrong mesh.
  if (buffers.length > 1) throw new Error(`${path}: ${buffers.length} buffers, only one is read`);
  return { json, bin: buffers[0] ?? null, base };
}

/** Splits a `.glb` into its JSON and its binary chunk. */
export function readGlb(path) {
  const bytes = readFileSync(path);
  if (bytes.readUInt32LE(0) !== MAGIC) throw new Error(`${path}: not a glb`);
  const length = bytes.readUInt32LE(8);
  let offset = 12;
  let json = null;
  let bin = null;
  while (offset + 8 <= length) {
    const size = bytes.readUInt32LE(offset);
    const kind = bytes.readUInt32LE(offset + 4);
    const chunk = bytes.subarray(offset + 8, offset + 8 + size);
    if (kind === JSON_CHUNK) json = JSON.parse(chunk.toString('utf8'));
    else if (kind === BIN_CHUNK) bin = chunk;
    // Chunks are four-byte aligned; the length field is not padded.
    offset += 8 + size + ((4 - (size % 4)) % 4);
  }
  if (!json) throw new Error(`${path}: no json chunk`);
  return { json, bin };
}

/** One accessor as a flat array of numbers. */
export function readAccessor(json, bin, index) {
  const accessor = json.accessors[index];
  if (accessor.sparse) throw new Error('sparse accessors are not read');
  const per = COUNTS[accessor.type];
  const [size, read] = COMPONENTS[accessor.componentType];
  const out = new Array(accessor.count * per);
  if (accessor.bufferView === undefined) return out.fill(0);
  const bufferView = json.bufferViews[accessor.bufferView];
  const base = (bufferView.byteOffset ?? 0) + (accessor.byteOffset ?? 0);
  // A byte stride of zero means tightly packed, which is what an exporter
  // writes for anything but an interleaved buffer.
  const stride = bufferView.byteStride || per * size;
  const view = new DataView(bin.buffer, bin.byteOffset, bin.byteLength);
  for (let i = 0; i < accessor.count; i++) {
    for (let c = 0; c < per; c++) {
      out[i * per + c] = read(view, base + i * stride + c * size);
    }
  }
  return out;
}

/** The 4×4 of a node, from its TRS. Column-major, like glTF's own. */
function nodeMatrix(node) {
  if (node.matrix) return node.matrix.slice();
  const [tx, ty, tz] = node.translation ?? [0, 0, 0];
  const [qx, qy, qz, qw] = node.rotation ?? [0, 0, 0, 1];
  const [sx, sy, sz] = node.scale ?? [1, 1, 1];
  // Rotation as a matrix, then scaled column by column.
  const r = [
    1 - 2 * (qy * qy + qz * qz),
    2 * (qx * qy + qz * qw),
    2 * (qx * qz - qy * qw),
    2 * (qx * qy - qz * qw),
    1 - 2 * (qx * qx + qz * qz),
    2 * (qy * qz + qx * qw),
    2 * (qx * qz + qy * qw),
    2 * (qy * qz - qx * qw),
    1 - 2 * (qx * qx + qy * qy),
  ];
  return [
    r[0] * sx, r[1] * sx, r[2] * sx, 0,
    r[3] * sy, r[4] * sy, r[5] * sy, 0,
    r[6] * sz, r[7] * sz, r[8] * sz, 0,
    tx, ty, tz, 1,
  ];
}

function multiply(a, b) {
  const out = new Array(16).fill(0);
  for (let c = 0; c < 4; c++) {
    for (let r = 0; r < 4; r++) {
      let sum = 0;
      for (let k = 0; k < 4; k++) sum += a[k * 4 + r] * b[c * 4 + k];
      out[c * 4 + r] = sum;
    }
  }
  return out;
}

function transformPoint(m, [x, y, z]) {
  return [
    m[0] * x + m[4] * y + m[8] * z + m[12],
    m[1] * x + m[5] * y + m[9] * z + m[13],
    m[2] * x + m[6] * y + m[10] * z + m[14],
  ];
}

/** A direction: the translation is left out and nothing is renormalised. */
function transformDirection(m, [x, y, z]) {
  return [
    m[0] * x + m[4] * y + m[8] * z,
    m[1] * x + m[5] * y + m[9] * z,
    m[2] * x + m[6] * y + m[10] * z,
  ];
}

/**
 * Every primitive of the file's default scene, with its node transform baked
 * in and its node's name attached.
 *
 * The name is what the rest of the pipeline works from: a kit calls its parts
 * `body`, `wheel-front-left`, and that is how a wheel is told from a car
 * without anyone having to look at the geometry.
 */
export function readParts(path) {
  const { json, bin } = readGltf(path);
  return partsOf(json, bin, path);
}

/** The same, when the file has already been opened. */
export function partsOf(json, bin, path = '<memory>') {
  const scene = json.scenes[json.scene ?? 0];
  const parts = [];

  const walk = (index, parent) => {
    const node = json.nodes[index];
    const matrix = multiply(parent, nodeMatrix(node));
    if (node.mesh !== undefined) {
      for (const primitive of json.meshes[node.mesh].primitives) {
        if (primitive.mode !== undefined && primitive.mode !== 4) {
          throw new Error(`${path}: primitive mode ${primitive.mode} is not triangles`);
        }
        const positions = readAccessor(json, bin, primitive.attributes.POSITION);
        const normals = primitive.attributes.NORMAL !== undefined
          ? readAccessor(json, bin, primitive.attributes.NORMAL)
          : null;
        const uvs = primitive.attributes.TEXCOORD_0 !== undefined
          ? readAccessor(json, bin, primitive.attributes.TEXCOORD_0)
          : null;
        const indices = primitive.indices !== undefined
          ? readAccessor(json, bin, primitive.indices)
          : [...positions.keys()].filter((i) => i % 3 === 0).map((i) => i / 3);

        const count = positions.length / 3;
        const out = {
          name: node.name ?? json.meshes[node.mesh].name ?? `part${parts.length}`,
          // Which material the primitive wears, for a pack whose colour is in
          // its textures rather than in a palette.
          material: primitive.material,
          positions: [],
          normals: [],
          uvs: [],
          colors: [],
          indices: [...indices],
          pieces: [],
        };
        for (let i = 0; i < count; i++) {
          const p = transformPoint(matrix, positions.slice(i * 3, i * 3 + 3));
          out.positions.push(p[0], p[1], p[2]);
          const n = normals
            ? transformDirection(matrix, normals.slice(i * 3, i * 3 + 3))
            : [0, 1, 0];
          const length = Math.hypot(n[0], n[1], n[2]) || 1;
          out.normals.push(n[0] / length, n[1] / length, n[2] / length);
          out.uvs.push(uvs ? uvs[i * 2] : 0, uvs ? uvs[i * 2 + 1] : 0);
          out.colors.push(1, 1, 1, 1);
          out.pieces.push(0);
        }
        parts.push(out);
      }
    }
    for (const child of node.children ?? []) walk(child, matrix);
  };

  const identity = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
  for (const root of scene.nodes) walk(root, identity);
  return parts;
}

/** Bounding box of a part or a list of them, as `{ min, max, size }`. */
export function boundsOf(parts) {
  const min = [Infinity, Infinity, Infinity];
  const max = [-Infinity, -Infinity, -Infinity];
  for (const part of [parts].flat()) {
    for (let i = 0; i < part.positions.length; i += 3) {
      for (let c = 0; c < 3; c++) {
        min[c] = Math.min(min[c], part.positions[i + c]);
        max[c] = Math.max(max[c], part.positions[i + c]);
      }
    }
  }
  return { min, max, size: [0, 1, 2].map((c) => max[c] - min[c]) };
}
