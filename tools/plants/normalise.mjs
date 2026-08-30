// Normalises the Quaternius crop models (CC0, via Poly Pizza) for the
// simulator's standing crop: bakes the node transforms in, puts the origin at
// the foot, scales every model to exactly one metre tall, re-indexes the
// vertices and writes one compact GLB per crop.
//
//   node tools/plants/normalise.mjs <in.glb> <out.glb>
//
// The sources and their licences are recorded in THIRD_PARTY_LICENSES.md and
// tools/plants/README.md; re-download with fetch.mjs if they ever move.

import { readFileSync, writeFileSync } from "node:fs";

const [inPath, outPath] = process.argv.slice(2);

// ---- GLB in ----------------------------------------------------------------

function parseGlb(buf) {
  if (buf.readUInt32LE(0) !== 0x46546c67) throw new Error("not a GLB");
  const jsonLen = buf.readUInt32LE(12);
  const json = JSON.parse(buf.subarray(20, 20 + jsonLen).toString());
  let at = 20 + jsonLen;
  let bin = new DataView(new ArrayBuffer(0));
  if (at + 8 <= buf.length && buf.readUInt32LE(at + 4) === 0x004e4942) {
    const binLen = buf.readUInt32LE(at);
    bin = new DataView(buf.buffer, buf.byteOffset + at + 8, binLen);
  }
  return { json, bin };
}

const COMPONENTS = { SCALAR: 1, VEC2: 2, VEC3: 3, VEC4: 4 };
const FLOAT = 5126, U16 = 5123, U32 = 5125, U8 = 5121;

function readAccessor(gltf, bin, index) {
  const acc = gltf.accessors[index];
  const bv = gltf.bufferViews[acc.bufferView ?? 0];
  const comps = COMPONENTS[acc.type];
  const width = { [FLOAT]: 4, [U16]: 2, [U32]: 4, [U8]: 1 }[acc.componentType];
  const stride = bv.byteStride ?? comps * width;
  const out = [];
  for (let i = 0; i < acc.count; i++) {
    const at = (bv.byteOffset ?? 0) + (acc.byteOffset ?? 0) + i * stride;
    const v = [];
    for (let c = 0; c < comps; c++) {
      const q = at + c * width;
      let x;
      switch (acc.componentType) {
        case FLOAT: x = bin.getFloat32(q, true); break;
        case U16: x = bin.getUint16(q, true); break;
        case U32: x = bin.getUint32(q, true); break;
        default: x = bin.getUint8(q);
      }
      // Normalised integers come in as 0 … 1.
      if (acc.normalized) x /= acc.componentType === U16 ? 65535 : 255;
      v.push(x);
    }
    out.push(v);
  }
  return out;
}

// ---- Transform bake --------------------------------------------------------

function compose(n) {
  if (n.matrix) return n.matrix;
  const [x = 0, y = 0, z = 0] = n.translation ?? [];
  const [rx = 0, ry = 0, rz = 0] = n.rotation ?? [];
  const [sx = 1, sy = 1, sz = 1] = n.scale ?? [];
  const cx = Math.cos(rx), sxr = Math.sin(rx);
  const cy = Math.cos(ry), syr = Math.sin(ry);
  const cz = Math.cos(rz), szr = Math.sin(rz);
  return [
    cy * cz * sx, (cx * szr + sxr * sy * cz) * sx, (sxr * szr - cx * sy * cz) * sx, 0,
    -cy * szr * sy, (cx * cz - sxr * sy * szr) * sy, (sxr * cz + cx * sy * szr) * sy, 0,
    sy * sz, -sxr * cy * sz, cx * cy * sz, 0,
    x, y, z, 1,
  ];
}
function mul(a, b) {
  const o = new Array(16).fill(0);
  for (let c = 0; c < 4; c++)
    for (let r = 0; r < 4; r++)
      for (let k = 0; k < 4; k++) o[c * 4 + r] += a[k * 4 + r] * b[c * 4 + k];
  return o;
}
function apply(m, v) {
  return [
    m[0] * v[0] + m[4] * v[1] + m[8] * v[2] + m[12],
    m[1] * v[0] + m[5] * v[1] + m[9] * v[2] + m[13],
    m[2] * v[0] + m[6] * v[1] + m[10] * v[2] + m[14],
  ];
}
function rotApply(m, v) {
  return [
    m[0] * v[0] + m[4] * v[1] + m[8] * v[2],
    m[1] * v[0] + m[5] * v[1] + m[9] * v[2],
    m[2] * v[0] + m[6] * v[1] + m[10] * v[2],
  ];
}

const buf = readFileSync(inPath);
const { json, bin } = parseGlb(buf);
const scene = json.scenes[json.scene ?? 0];

// World-space primitives per material.
const parts = new Map();
function walk(node, parent) {
  const m = parent ? mul(parent, compose(node)) : compose(node);
  if (node.mesh !== undefined) {
    for (const prim of json.meshes[node.mesh].primitives) {
      const part = parts.get(prim.material ?? 0) ??
        { pos: [], nrm: [], col: [], uv: [], idx: [] };
      parts.set(prim.material ?? 0, part);
      const pos = readAccessor(json, bin, prim.attributes.POSITION);
      const nrm = prim.attributes.NORMAL !== undefined
        ? readAccessor(json, bin, prim.attributes.NORMAL)
        : null;
      const col = prim.attributes.COLOR_0 !== undefined
        ? readAccessor(json, bin, prim.attributes.COLOR_0)
        : null;
      const uv = prim.attributes.TEXCOORD_0 !== undefined
        ? readAccessor(json, bin, prim.attributes.TEXCOORD_0)
        : null;
      const base = part.pos.length;
      for (let i = 0; i < pos.length; i++) {
        part.pos.push(apply(m, pos[i]));
        part.nrm.push(nrm ? rotApply(m, nrm[i]) : [0, 1, 0]);
        if (col) {
          const c = col[i];
          part.col.push([c[0] ?? 1, c[1] ?? 1, c[2] ?? 1, c[3] ?? 1]);
        }
        if (uv) part.uv.push(uv[i]);
      }
      if (prim.indices !== undefined) {
        for (const i of readAccessor(json, bin, prim.indices)) part.idx.push(base + i[0]);
      } else {
        // Non-indexed source: floor to whole triangles.
        for (let i = 0; i + 2 < pos.length; i += 3) part.idx.push(base + i, base + i + 1, base + i + 2);
      }
    }
  }
  for (const child of node.children ?? []) walk(json.nodes[child], m);
}
for (const node of scene.nodes) walk(json.nodes[node], null);

// Every model becomes exactly one metre tall, foot at the origin.
let lo = [1e9, 1e9, 1e9], hi = [-1e9, -1e9, -1e9];
for (const part of parts.values())
  for (const p of part.pos)
    for (let i = 0; i < 3; i++) {
      lo[i] = Math.min(lo[i], p[i]);
      hi[i] = Math.max(hi[i], p[i]);
    }
const height = hi[1] - lo[1];
const s = height > 1e-9 ? 1 / height : 1;

// Dedupe by position and remap the triangle list.
function compact(part) {
  const map = new Map();
  const pos = [], nrm = [], col = [], uv = [], idx = [];
  const at = i => {
    const key = part.pos[i].map(v => Math.round(v * s * 1e4)).join(",");
    let k = map.get(key);
    if (k === undefined) {
      k = pos.length;
      map.set(key, k);
      pos.push(part.pos[i].map((v, c) => +((v - lo[c]) * s).toFixed(5)));
      const n = part.nrm[i];
      const l = Math.hypot(n[0], n[1], n[2]) || 1;
      nrm.push([+(n[0] / l).toFixed(4), +(n[1] / l).toFixed(4), +(n[2] / l).toFixed(4)]);
      if (part.col.length) col.push(part.col[i].map(x => +x.toFixed(4)));
      if (part.uv.length) uv.push(part.uv[i]);
    }
    return k;
  };
  for (const i of part.idx) idx.push(at(i));
  return { pos, nrm, col, uv, idx };
}
const outParts = [...parts.entries()].map(([mat, part]) => ({ mat, ...compact(part) }));

// ---- GLB out ---------------------------------------------------------------

const chunks = [];
let byteLength = 0;
function align() {
  while (byteLength % 4) { chunks.push(0); byteLength++; }
}
function pushF32(values) {
  align();
  const view = { buffer: 0, byteOffset: byteLength, byteLength: values.length * 4 };
  for (const v of values) {
    const b = new Uint8Array(4);
    new DataView(b.buffer).setFloat32(0, v, true);
    chunks.push(b[0], b[1], b[2], b[3]);
    byteLength += 4;
  }
  return view;
}
function pushU32(values) {
  align();
  const view = { buffer: 0, byteOffset: byteLength, byteLength: values.length * 4 };
  for (const v of values) {
    chunks.push(v & 255, (v >> 8) & 255, (v >> 16) & 255, (v >> 24) & 255);
    byteLength += 4;
  }
  return view;
}
function pushBytes(bytes) {
  align();
  const view = { buffer: 0, byteOffset: byteLength, byteLength: bytes.length };
  for (const b of bytes) chunks.push(b);
  byteLength += bytes.length;
  return view;
}

const accessors = [], views = [], meshes = [], materials = [];
function accessor(view, componentType, count, type, min, max) {
  views.push(view);
  accessors.push({ bufferView: views.length - 1, componentType, count, type, ...(min ? { min, max } : {}) });
  return accessors.length - 1;
}

// Images first, so material texture indices can point at them.
let out_images = null;
let out_textures = null;
if ((json.images?.length ?? 0) > 0) {
  out_images = [];
  out_textures = [];
  json.images.forEach((img, i) => {
    const bv = json.bufferViews[img.bufferView];
    const bytes = bin.buffer.slice(
      bin.byteOffset + (bv.byteOffset ?? 0),
      bin.byteOffset + (bv.byteOffset ?? 0) + bv.byteLength,
    );
    const view = pushBytes(new Uint8Array(bytes));
    // The accessor-less buffer view of an image sits directly in the views
    // list; the texture's source index points at the image.
    out_images.push({ bufferView: views.length, mimeType: img.mimeType, name: img.name });
    views.push(view);
    out_textures.push({ source: i });
  });
}

for (const part of outParts) {
  const posMin = [0, 0, 0], posMax = [1, 1, 1];
  const prim = {
    attributes: {
      POSITION: accessor(pushF32(part.pos.flat()), FLOAT, part.pos.length, "VEC3", posMin, posMax),
      NORMAL: accessor(pushF32(part.nrm.flat()), FLOAT, part.nrm.length, "VEC3"),
    },
    indices: accessor(pushU32(part.idx), U32, part.idx.length, "SCALAR"),
    material: materials.length,
    mode: 4,
  };
  if (part.col.length) {
    prim.attributes.COLOR_0 = accessor(pushF32(part.col.flat()), FLOAT, part.col.length, "VEC4");
  }
  if (part.uv.length) {
    prim.attributes.TEXCOORD_0 = accessor(pushF32(part.uv.flat()), FLOAT, part.uv.length, "VEC2");
  }
  const src = json.materials?.[part.mat] ?? {};
  const material = {
    name: src.name ?? `mat${part.mat}`,
    pbrMetallicRoughness: {
      baseColorFactor: src.pbrMetallicRoughness?.baseColorFactor ?? [1, 1, 1, 1],
      metallicFactor: 0.0,
      roughnessFactor: 0.9,
    },
    doubleSided: true,
  };
  const tex = src.pbrMetallicRoughness?.baseColorTexture;
  if (tex && out_images) material.pbrMetallicRoughness.baseColorTexture = { index: tex.index };
  if (src.alphaMode) material.alphaMode = src.alphaMode;
  if (src.alphaCutoff) material.alphaCutoff = src.alphaCutoff;
  materials.push(material);
  meshes.push({ name: material.name, primitives: [prim] });
}

const outJson = {
  asset: {
    version: "2.0",
    generator: "connected-rails tools/plants/normalise.mjs (source: Quaternius, Ultimate Crops / Nature via Poly Pizza, CC0)",
  },
  scene: 0,
  scenes: [{ nodes: [0] }],
  nodes: [{ name: inPath.split("/").pop().replace(".glb", ""), mesh: 0 }],
  meshes,
  materials,
  accessors,
  bufferViews: views,
  buffers: [{ byteLength }],
};
if (out_images) {
  outJson.images = out_images;
  outJson.textures = out_textures;
}

let jsonStr = JSON.stringify(outJson);
while (jsonStr.length % 4) jsonStr += " ";
const jsonBytes = new TextEncoder().encode(jsonStr);
const binBytes = new Uint8Array(chunks);
const total = 12 + 8 + jsonBytes.length + 8 + binBytes.length;
const out = new Uint8Array(total);
const dv = new DataView(out.buffer);
dv.setUint32(0, 0x46546c67, true);
dv.setUint32(4, 2, true);
dv.setUint32(8, total, true);
dv.setUint32(12, jsonBytes.length, true);
dv.setUint32(16, 0x4e4f534a, true);
out.set(jsonBytes, 20);
dv.setUint32(20 + jsonBytes.length, binBytes.length, true);
dv.setUint32(24 + jsonBytes.length, 0x004e4942, true);
out.set(binBytes, 28 + jsonBytes.length);
writeFileSync(outPath, out);
const tris = outParts.reduce((n, p) => n + p.idx.length / 3, 0);
console.log(
  outPath, `${(total / 1024).toFixed(0)} kB,`,
  `${meshes.length} mesh(es), ${tris} tris, source height ${height.toFixed(2)} m -> 1.00 m`,
);
