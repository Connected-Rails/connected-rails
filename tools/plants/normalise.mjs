// Normalises the Quaternius crop models (CC0, via Poly Pizza) for the
// simulator's standing crop: bakes the node transforms in, splits the scene
// into one *variant* per placed plant, stands each variant on its own foot
// point, scales the set so the tallest variant is exactly one metre,
// re-indexes the vertices and writes one compact GLB per crop.
//
//   node tools/plants/normalise.mjs <in.glb> <out.glb>
//
// A Poly Pizza pack is a *scene*: the flowers file is seven clumps laid out
// in a row, the grass file two tufts sixty metres apart. Merged into one
// mesh that is a flowerbed, not a flower — so every placed node becomes a
// variant of its own, and the renderer picks one per plant. The variants
// keep their relative heights (a small tuft stays smaller than a large one);
// only the tallest is a metre.
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
const FLOAT = 5126, U16 = 5123, U32 = 5125, U8 = 5121, I16 = 5122, I8 = 5120;
const WIDTH = { [FLOAT]: 4, [U16]: 2, [U32]: 4, [U8]: 1, [I16]: 2, [I8]: 1 };

function readAccessor(gltf, bin, index) {
  const acc = gltf.accessors[index];
  const comps = COMPONENTS[acc.type];
  // A bufferView-less accessor is all zeroes by the spec.
  if (acc.bufferView === undefined) {
    return Array.from({ length: acc.count }, () => new Array(comps).fill(0));
  }
  const bv = gltf.bufferViews[acc.bufferView];
  const width = WIDTH[acc.componentType];
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
        case I16: x = bin.getInt16(q, true); break;
        case I8: x = bin.getInt8(q); break;
        default: x = bin.getUint8(q);
      }
      // Normalised integers come in as 0 … 1.
      if (acc.normalized) {
        x /= { [U16]: 65535, [U8]: 255, [I16]: 32767, [I8]: 127 }[acc.componentType];
      }
      v.push(x);
    }
    out.push(v);
  }
  return out;
}

// ---- Transform bake --------------------------------------------------------
//
// Column-major 4x4, the way glTF stores `node.matrix`: `m[c * 4 + r]`.

/// A glTF rotation is a **quaternion** `[x, y, z, w]` — the Poly Pizza models
/// all carry the −90° about X that turns a Z-up export into glTF's Y-up, and
/// reading those four numbers as Euler angles is what laid the crop on its
/// side and blew its footprint up to five metres.
function quatToMat(q) {
  const [x = 0, y = 0, z = 0, w = 1] = q;
  return [
    1 - 2 * (y * y + z * z), 2 * (x * y + z * w), 2 * (x * z - y * w), 0,
    2 * (x * y - z * w), 1 - 2 * (x * x + z * z), 2 * (y * z + x * w), 0,
    2 * (x * z + y * w), 2 * (y * z - x * w), 1 - 2 * (x * x + y * y), 0,
    0, 0, 0, 1,
  ];
}

function compose(n) {
  if (n.matrix) return n.matrix.slice();
  const [tx = 0, ty = 0, tz = 0] = n.translation ?? [];
  const [sx = 1, sy = 1, sz = 1] = n.scale ?? [];
  const rot = quatToMat(n.rotation ?? [0, 0, 0, 1]);
  const scale = [sx, sy, sz];
  const m = new Array(16).fill(0);
  // T · R · S: each rotation column scaled by its own axis, translation last.
  for (let c = 0; c < 3; c++) {
    for (let r = 0; r < 3; r++) m[c * 4 + r] = rot[c * 4 + r] * scale[c];
  }
  m[12] = tx;
  m[13] = ty;
  m[14] = tz;
  m[15] = 1;
  return m;
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
/// Whether the transform mirrors — a mirrored node draws inside out unless
/// its triangles are wound the other way round.
function mirrors(m) {
  const d =
    m[0] * (m[5] * m[10] - m[6] * m[9]) -
    m[4] * (m[1] * m[10] - m[2] * m[9]) +
    m[8] * (m[1] * m[6] - m[2] * m[5]);
  return d < 0;
}

const buf = readFileSync(inPath);
const { json, bin } = parseGlb(buf);
const scene = json.scenes[json.scene ?? 0];

// ---- Variants --------------------------------------------------------------
//
// One variant per node that carries a mesh: a pack's seven flower clumps are
// seven plants a field can stand, not one three-metre flowerbed.

const variants = [];
function walk(node, parent) {
  const m = parent ? mul(parent, compose(node)) : compose(node);
  if (node.mesh !== undefined) {
    const flip = mirrors(m);
    const parts = new Map(); // material -> geometry
    for (const prim of json.meshes[node.mesh].primitives) {
      // Only triangle lists; a pack that ever ships strips can be converted
      // here, but none of them do.
      if ((prim.mode ?? 4) !== 4) continue;
      const mat = prim.material ?? 0;
      const part = parts.get(mat) ??
        { pos: [], nrm: [], col: [], uv: [], idx: [], hasCol: false, hasUv: false };
      parts.set(mat, part);
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
      // Every vertex gets a colour and a texture coordinate whether its
      // primitive shipped them or not: two primitives of one material that
      // disagree — one textured, one not — would otherwise leave the
      // per-vertex arrays a different length from the positions, and the
      // weld below reads them in step.
      part.hasCol ||= !!col;
      part.hasUv ||= !!uv;
      const base = part.pos.length;
      for (let i = 0; i < pos.length; i++) {
        part.pos.push(apply(m, pos[i]));
        part.nrm.push(nrm ? rotApply(m, nrm[i]) : [0, 1, 0]);
        const c = col?.[i];
        part.col.push(c ? [c[0] ?? 1, c[1] ?? 1, c[2] ?? 1, c[3] ?? 1] : [1, 1, 1, 1]);
        part.uv.push(uv?.[i] ?? [0, 0]);
      }
      const tri = prim.indices !== undefined
        ? readAccessor(json, bin, prim.indices).map(i => i[0])
        : [...pos.keys()];
      for (let i = 0; i + 2 < tri.length; i += 3) {
        part.idx.push(
          base + tri[i],
          base + tri[flip ? i + 2 : i + 1],
          base + tri[flip ? i + 1 : i + 2],
        );
      }
    }
    if (parts.size) variants.push({ name: node.name ?? `variant${variants.length}`, parts });
  }
  for (const child of node.children ?? []) walk(json.nodes[child], m);
}
for (const node of scene.nodes) walk(json.nodes[node], null);
if (!variants.length) throw new Error(`${inPath}: no mesh in the scene`);

// Each variant stands on its own foot: centred on its footprint in X and Z,
// its lowest point on y = 0. One scale for the set, so a small tuft stays
// smaller than a large one and only the tallest is a metre.
for (const variant of variants) {
  const lo = [1e30, 1e30, 1e30], hi = [-1e30, -1e30, -1e30];
  for (const part of variant.parts.values())
    for (const p of part.pos)
      for (let i = 0; i < 3; i++) {
        lo[i] = Math.min(lo[i], p[i]);
        hi[i] = Math.max(hi[i], p[i]);
      }
  variant.height = hi[1] - lo[1];
  variant.foot = [(lo[0] + hi[0]) / 2, lo[1], (lo[2] + hi[2]) / 2];
}
const tallest = Math.max(...variants.map(v => v.height));
const s = tallest > 1e-9 ? 1 / tallest : 1;

// Dedupe by every attribute, not by position alone: two faces of an atlas
// card share a corner and not its texture coordinate, and merging those two
// vertices smears one card's picture across the other.
function compact(part, foot) {
  const map = new Map();
  const pos = [], nrm = [], col = [], uv = [], idx = [];
  const at = i => {
    const p = part.pos[i];
    const n = part.nrm[i];
    const key = [
      Math.round((p[0] - foot[0]) * s * 1e4),
      Math.round((p[1] - foot[1]) * s * 1e4),
      Math.round((p[2] - foot[2]) * s * 1e4),
      Math.round(n[0] * 1e3), Math.round(n[1] * 1e3), Math.round(n[2] * 1e3),
      ...(part.hasUv ? part.uv[i].map(v => Math.round(v * 1e5)) : []),
      ...(part.hasCol ? part.col[i].map(v => Math.round(v * 1e3)) : []),
    ].join(",");
    let k = map.get(key);
    if (k === undefined) {
      k = pos.length;
      map.set(key, k);
      pos.push(p.map((v, c) => +((v - foot[c]) * s).toFixed(5)));
      const l = Math.hypot(n[0], n[1], n[2]) || 1;
      nrm.push([+(n[0] / l).toFixed(4), +(n[1] / l).toFixed(4), +(n[2] / l).toFixed(4)]);
      if (part.hasCol) col.push(part.col[i].map(x => +x.toFixed(4)));
      if (part.hasUv) uv.push(part.uv[i].map(x => +x.toFixed(6)));
    }
    return k;
  };
  for (let i = 0; i + 2 < part.idx.length; i += 3) {
    const t = [at(part.idx[i]), at(part.idx[i + 1]), at(part.idx[i + 2])];
    // A triangle the weld collapsed draws nothing but costs a vertex fetch.
    if (t[0] === t[1] || t[1] === t[2] || t[0] === t[2]) continue;
    idx.push(...t);
  }
  return { pos, nrm, col, uv, idx };
}
for (const variant of variants) {
  variant.compact = [...variant.parts.entries()]
    .map(([mat, part]) => ({ mat, ...compact(part, variant.foot) }))
    .filter(part => part.idx.length);
}

// ---- GLB out ---------------------------------------------------------------

const chunks = [];
let byteLength = 0;
function align() {
  while (byteLength % 4) { chunks.push(0); byteLength++; }
}
function pushF32(values) {
  align();
  const view = { buffer: 0, byteOffset: byteLength, byteLength: values.length * 4 };
  const b = new Uint8Array(4);
  const dv = new DataView(b.buffer);
  for (const v of values) {
    dv.setFloat32(0, v, true);
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

const accessors = [], views = [], meshes = [], nodes = [];
function accessor(view, componentType, count, type, min, max) {
  views.push(view);
  accessors.push({
    bufferView: views.length - 1,
    componentType,
    count,
    type,
    ...(min ? { min, max } : {}),
  });
  return accessors.length - 1;
}

/// Whether a PNG carries transparency — an alpha channel in the IHDR colour
/// type (4 = grey+alpha, 6 = RGBA), or a `tRNS` chunk, which is how a palette
/// or a colour-keyed image carries it. A leaf sheet whose transparency is
/// missed draws as a solid rectangle of leaf soup, which is the hedge this
/// whole pass exists to avoid; anything that is not a PNG is assumed to have
/// it rather than risk that.
function hasAlpha(bytes) {
  if (bytes.length < 26) return false;
  const png = [0x89, 0x50, 0x4e, 0x47];
  if (!png.every((b, i) => bytes[i] === b)) return true;
  const colourType = bytes[25];
  if (colourType === 4 || colourType === 6) return true;
  // Walk the chunk list for tRNS. The chunks start after the 8-byte
  // signature; each is a 4-byte length, a 4-byte type and its payload.
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  for (let at = 8; at + 8 <= bytes.length; ) {
    const length = view.getUint32(at);
    const type = String.fromCharCode(...bytes.subarray(at + 4, at + 8));
    if (type === "tRNS") return true;
    if (type === "IDAT" || type === "IEND") return false;
    at += 12 + length;
  }
  return false;
}

// Only the images a material actually samples: the tree pack ships a one
// megabyte normal map nothing here reads.
const out_images = [], out_textures = [], out_samplers = [];
const textureOf = new Map(); // source texture index -> output texture index
const alphaOf = new Map(); // output texture index -> carries alpha
function texture(index) {
  if (textureOf.has(index)) return textureOf.get(index);
  const src = json.textures?.[index];
  const img = json.images?.[src?.source ?? -1];
  if (!img || img.bufferView === undefined) return undefined;
  const bv = json.bufferViews[img.bufferView];
  const bytes = new Uint8Array(
    bin.buffer.slice(
      bin.byteOffset + (bv.byteOffset ?? 0),
      bin.byteOffset + (bv.byteOffset ?? 0) + bv.byteLength,
    ),
  );
  const view = pushBytes(bytes);
  const image = out_images.length;
  out_images.push({ bufferView: views.length, mimeType: img.mimeType ?? "image/png", name: img.name });
  views.push(view);
  const out = out_textures.length;
  out_textures.push({ source: image, sampler: 0 });
  textureOf.set(index, out);
  alphaOf.set(out, hasAlpha(bytes));
  return out;
}
// One sampler for the lot: repeat, trilinear — the mip chain the renderer
// builds is what keeps a stand from shimmering at distance.
out_samplers.push({ magFilter: 9729, minFilter: 9987, wrapS: 10497, wrapT: 10497 });

// The materials, in source order, so every variant's primitives share them.
const materials = [];
const materialOf = new Map();
function material(index) {
  if (materialOf.has(index)) return materialOf.get(index);
  const src = json.materials?.[index] ?? {};
  const tex = src.pbrMetallicRoughness?.baseColorTexture;
  const out = tex !== undefined ? texture(tex.index) : undefined;
  const pbr = {
    baseColorFactor: src.pbrMetallicRoughness?.baseColorFactor ?? [1, 1, 1, 1],
    metallicFactor: 0.0,
    roughnessFactor: 0.9,
  };
  if (out !== undefined) pbr.baseColorTexture = { index: out };
  const material = {
    name: src.name ?? `mat${index}`,
    pbrMetallicRoughness: pbr,
    // A plant is a shell: every leaf is seen from both sides.
    doubleSided: true,
  };
  // Vegetation is cut out, never blended: a blended leaf sheet has to be
  // sorted against every other leaf in the field, writes no depth, and
  // costs a full-rate pass over a stand that is mostly holes. The packs
  // ship BLEND because a modelling viewport does not care.
  //
  // Transparency the pack *declared* always counts — a factor alpha under
  // one says the same thing MASK and BLEND do. Transparency only the
  // picture carries counts unless the pack said OPAQUE outright: a bark
  // sheet that happens to be RGBA with every texel solid is not a cut-out,
  // and testing it would cost the whole trunk its early depth.
  const declared = src.alphaMode === "MASK" || src.alphaMode === "BLEND" ||
    (pbr.baseColorFactor[3] ?? 1) < 1.0;
  const pictured = out !== undefined && alphaOf.get(out) && src.alphaMode === undefined;
  if (declared || pictured) {
    material.alphaMode = "MASK";
    material.alphaCutoff = src.alphaCutoff ?? 0.5;
  }
  materialOf.set(index, materials.length);
  materials.push(material);
  return materials.length - 1;
}

for (const variant of variants) {
  const primitives = [];
  for (const part of variant.compact) {
    const lo = [1e30, 1e30, 1e30], hi = [-1e30, -1e30, -1e30];
    for (const p of part.pos)
      for (let i = 0; i < 3; i++) {
        lo[i] = Math.min(lo[i], p[i]);
        hi[i] = Math.max(hi[i], p[i]);
      }
    const prim = {
      attributes: {
        POSITION: accessor(pushF32(part.pos.flat()), FLOAT, part.pos.length, "VEC3", lo, hi),
        NORMAL: accessor(pushF32(part.nrm.flat()), FLOAT, part.nrm.length, "VEC3"),
      },
      indices: accessor(pushU32(part.idx), U32, part.idx.length, "SCALAR"),
      material: material(part.mat),
      mode: 4,
    };
    if (part.col.length) {
      prim.attributes.COLOR_0 = accessor(pushF32(part.col.flat()), FLOAT, part.col.length, "VEC4");
    }
    if (part.uv.length) {
      prim.attributes.TEXCOORD_0 = accessor(pushF32(part.uv.flat()), FLOAT, part.uv.length, "VEC2");
    }
    primitives.push(prim);
  }
  nodes.push({ name: variant.name, mesh: meshes.length });
  meshes.push({ name: variant.name, primitives });
}

const outJson = {
  asset: {
    version: "2.0",
    generator: "connected-rails tools/plants/normalise.mjs (source: Quaternius via Poly Pizza, CC0)",
  },
  scene: 0,
  scenes: [{ nodes: nodes.map((_, i) => i) }],
  nodes,
  meshes,
  materials,
  accessors,
  bufferViews: views,
  buffers: [{ byteLength }],
};
if (out_images.length) {
  outJson.images = out_images;
  outJson.textures = out_textures;
  outJson.samplers = out_samplers;
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

const tris = variants.reduce(
  (n, v) => n + v.compact.reduce((m, p) => m + p.idx.length / 3, 0),
  0,
);
const spread = variants
  .map(v => {
    const lo = [1e30, 1e30], hi = [-1e30, -1e30];
    for (const part of v.compact)
      for (const p of part.pos) {
        lo[0] = Math.min(lo[0], p[0]); hi[0] = Math.max(hi[0], p[0]);
        lo[1] = Math.min(lo[1], p[2]); hi[1] = Math.max(hi[1], p[2]);
      }
    return Math.max(hi[0] - lo[0], hi[1] - lo[1]);
  })
  .reduce((a, b) => Math.max(a, b), 0);
console.log(
  outPath, `${(total / 1024).toFixed(0)} kB,`,
  `${variants.length} variant(s), ${tris} tris,`,
  `tallest ${tallest.toFixed(2)} -> 1.00 m, widest ${spread.toFixed(2)} m`,
);
