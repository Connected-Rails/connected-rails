// The primitives everything in this kit is built from.
//
// A lattice mast is a few hundred straight steel members and nothing else, so
// the whole vocabulary is: a box between two points, a tapered tube between two
// points, and a way to glue the results together. Flat normals throughout —
// angle steel has edges, and smoothing them costs vertices to hide the one
// thing that makes a lattice read as steel.
//
// Axes are the game's (`MODS.md`, *Track objects*): +Y up, the model's front
// along −Z. A mast is built with its crossarms along ±X, so the line it carries
// runs along Z and the placement's yaw turns the mast to face along the line.

/**
 * An empty geometry in this pipeline's layout.
 *
 * `uvs` are in **metres**, not in `0…1`: the texture tiles once per metre
 * (`tools/pylons/lib/texture.mjs`), so the grain of the zinc is the same size
 * on a 36 cm leg as on a 6 cm brace, which is the one thing that stops a
 * tiled material from reading as wallpaper.
 *
 * `colors` carry the weathering — see `tint`.
 */
export function empty() {
  return { positions: [], normals: [], uvs: [], colors: [], indices: [], pieces: [], piece: 0 };
}

/**
 * The colour every following face is written with, until it is set again.
 *
 * The texture says what the material *is*; this says what has happened to this
 * particular member. A lattice whose four hundred bars are the exact same grey
 * reads as one extruded object rather than as four hundred bolted pieces, and
 * the foot of a mast is dirtier than its peak.
 */
export function tint(out, rgba) {
  out.tint = rgba;
  return out;
}

export function triangleCount(geometry) {
  return geometry.indices.length / 3;
}

const sub = (a, b) => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const add = (a, b) => [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
const scale = (a, s) => [a[0] * s, a[1] * s, a[2] * s];
const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const cross = (a, b) => [
  a[1] * b[2] - a[2] * b[1],
  a[2] * b[0] - a[0] * b[2],
  a[0] * b[1] - a[1] * b[0],
];
const length = (a) => Math.hypot(a[0], a[1], a[2]);

function normalise(a) {
  const l = length(a);
  return l < 1e-9 ? [0, 1, 0] : [a[0] / l, a[1] / l, a[2] / l];
}

/**
 * An orthonormal frame with `w` along `axis`. `u` is kept as horizontal as the
 * axis allows, which is what makes a diagonal brace sit flat rather than
 * rolled: two braces meeting at a node then share a face instead of crossing
 * at a random angle.
 */
function frame(axis) {
  const w = normalise(axis);
  const up = Math.abs(w[1]) > 0.99 ? [0, 0, 1] : [0, 1, 0];
  const u = normalise(cross(up, w));
  const v = cross(w, u);
  return { u, v, w };
}

const WHITE = [1, 1, 1, 1];

/**
 * Adds one quad, `a b c d` in order round the face, with a flat normal.
 *
 * `uv` is the four corners' texture coordinates in metres; without it they are
 * measured off the quad's own edges, which is right for anything rectangular.
 */
export function quad(out, a, b, c, d, uv) {
  const n = normalise(cross(sub(b, a), sub(c, a)));
  const base = out.positions.length / 3;
  const t = out.tint ?? WHITE;
  const corners = [a, b, c, d];
  const map = uv ?? [
    [0, 0],
    [length(sub(b, a)), 0],
    [length(sub(b, a)), length(sub(c, b))],
    [0, length(sub(d, a))],
  ];
  for (let i = 0; i < 4; i++) {
    const p = corners[i];
    out.positions.push(p[0], p[1], p[2]);
    out.normals.push(n[0], n[1], n[2]);
    out.uvs.push(map[i][0], map[i][1]);
    out.colors.push(t[0], t[1], t[2], t[3]);
    out.pieces.push(out.piece ?? 0);
  }
  out.indices.push(base, base + 1, base + 2, base, base + 2, base + 3);
  return out;
}

/** Adds one triangle with a flat normal. */
export function tri(out, a, b, c, uv) {
  const n = normalise(cross(sub(b, a), sub(c, a)));
  const base = out.positions.length / 3;
  const t = out.tint ?? WHITE;
  const corners = [a, b, c];
  const map = uv ?? [
    [0, 0],
    [length(sub(b, a)), 0],
    [0, length(sub(c, a))],
  ];
  for (let i = 0; i < 3; i++) {
    const p = corners[i];
    out.positions.push(p[0], p[1], p[2]);
    out.normals.push(n[0], n[1], n[2]);
    out.uvs.push(map[i][0], map[i][1]);
    out.colors.push(t[0], t[1], t[2], t[3]);
    out.pieces.push(out.piece ?? 0);
  }
  out.indices.push(base, base + 1, base + 2);
  return out;
}

/**
 * A steel member: a box of cross-section `wu` x `wv` running from `a` to `b`.
 * Twelve triangles. This is the single most-used call in the kit — a 380 kV
 * Donaumast is about four hundred of them at the finest level.
 *
 * `capped` closes the ends; leave it off for members that die inside another
 * one, which is most of them, and the two triangles per end are saved.
 */
export function member(out, a, b, wu, wv = wu, capped = false) {
  if (length(sub(b, a)) < 1e-6) return out;
  out.piece = (out.piece ?? 0) + 1;
  const { u, v } = frame(sub(b, a));
  const hu = wu / 2;
  const hv = wv / 2;
  const corner = (p, su, sv) => add(add(p, scale(u, su * hu)), scale(v, sv * hv));
  const a00 = corner(a, -1, -1);
  const a10 = corner(a, 1, -1);
  const a11 = corner(a, 1, 1);
  const a01 = corner(a, -1, 1);
  const b00 = corner(b, -1, -1);
  const b10 = corner(b, 1, -1);
  const b11 = corner(b, 1, 1);
  const b01 = corner(b, -1, 1);
  // `u` runs along the bar in metres and `v` across it, so the grain follows
  // the steel rather than the triangle.
  const len = length(sub(b, a));
  const face = (w) => [
    [0, 0],
    [len, 0],
    [len, w],
    [0, w],
  ];
  quad(out, a00, b00, b10, a10, face(wu));
  quad(out, a10, b10, b11, a11, face(wv));
  quad(out, a11, b11, b01, a01, face(wu));
  quad(out, a01, b01, b00, a00, face(wv));
  if (capped) {
    quad(out, a00, a10, a11, a01, face(wv));
    quad(out, b01, b11, b10, b00, face(wv));
  }
  return out;
}

/**
 * A tapered tube from `a` (radius `r0`) to `b` (radius `r1`), `sides` facets.
 * Concrete and wooden poles, insulator sheds and the steel tube of a compact
 * mast — everything round in the kit.
 */
export function tube(out, a, b, r0, r1, sides = 8, capped = true) {
  out.piece = (out.piece ?? 0) + 1;
  const { u, v } = frame(sub(b, a));
  const ring = (p, r) => {
    const pts = [];
    for (let i = 0; i < sides; i++) {
      const t = (i / sides) * Math.PI * 2;
      pts.push(add(add(p, scale(u, Math.cos(t) * r)), scale(v, Math.sin(t) * r)));
    }
    return pts;
  };
  const lo = ring(a, r0);
  const hi = ring(b, r1);
  const len = length(sub(b, a));
  const step = (2 * Math.PI * Math.max(r0, r1)) / sides;
  for (let i = 0; i < sides; i++) {
    const j = (i + 1) % sides;
    quad(out, lo[i], hi[i], hi[j], lo[j], [
      [i * step, 0],
      [i * step, len],
      [(i + 1) * step, len],
      [(i + 1) * step, 0],
    ]);
  }
  if (capped) {
    for (let i = 1; i < sides - 1; i++) {
      tri(out, lo[0], lo[i + 1], lo[i]);
      tri(out, hi[0], hi[i], hi[i + 1]);
    }
  }
  return out;
}

/** An axis-aligned box between two opposite corners. */
export function box(out, min, max) {
  out.piece = (out.piece ?? 0) + 1;
  const [x0, y0, z0] = min;
  const [x1, y1, z1] = max;
  const p = (x, y, z) => [x, y, z];
  quad(out, p(x0, y0, z1), p(x1, y0, z1), p(x1, y1, z1), p(x0, y1, z1));
  quad(out, p(x1, y0, z0), p(x0, y0, z0), p(x0, y1, z0), p(x1, y1, z0));
  quad(out, p(x1, y0, z1), p(x1, y0, z0), p(x1, y1, z0), p(x1, y1, z1));
  quad(out, p(x0, y0, z0), p(x0, y0, z1), p(x0, y1, z1), p(x0, y1, z0));
  quad(out, p(x0, y1, z1), p(x1, y1, z1), p(x1, y1, z0), p(x0, y1, z0));
  quad(out, p(x0, y0, z0), p(x1, y0, z0), p(x1, y0, z1), p(x0, y0, z1));
  return out;
}

/** Concatenates geometries into one, offsetting the indices. */
export function merge(parts) {
  const out = empty();
  for (const part of parts) {
    if (!part) continue;
    const offset = out.positions.length / 3;
    // Element by element: spreading tens of thousands of floats into `push`
    // overflows the argument stack.
    for (const v of part.positions) out.positions.push(v);
    for (const v of part.normals) out.normals.push(v);
    for (const v of part.uvs) out.uvs.push(v);
    for (const v of part.colors) out.colors.push(v);
    // The pieces of two geometries must stay distinct, or the weathering would
    // give a mirrored crossarm the same shade as the one it mirrors.
    const base = out.piece ?? 0;
    for (const v of part.pieces) out.pieces.push(base + v);
    out.piece = base + (part.piece ?? 0);
    for (const i of part.indices) out.indices.push(i + offset);
  }
  return out;
}

/** Merges `part` into `target` in place. */
export function append(target, part) {
  if (!part || part.positions.length === 0) return target;
  const offset = target.positions.length / 3;
  for (const v of part.positions) target.positions.push(v);
  for (const v of part.normals) target.normals.push(v);
  for (const v of part.uvs) target.uvs.push(v);
  for (const v of part.colors) target.colors.push(v);
  const base = target.piece ?? 0;
  for (const v of part.pieces) target.pieces.push(base + v);
  target.piece = base + (part.piece ?? 0);
  for (const i of part.indices) target.indices.push(i + offset);
  return target;
}

/** Mirrors a geometry through the YZ plane, flipping the winding with it. */
export function mirrorX(geometry) {
  const out = empty();
  for (let i = 0; i < geometry.positions.length; i += 3) {
    out.positions.push(-geometry.positions[i], geometry.positions[i + 1], geometry.positions[i + 2]);
    out.normals.push(-geometry.normals[i], geometry.normals[i + 1], geometry.normals[i + 2]);
  }
  for (const v of geometry.uvs) out.uvs.push(v);
  for (const v of geometry.colors) out.colors.push(v);
  // Mirrored, but a *different* piece: the two halves of a crossarm were
  // galvanised in the same bath and still weather differently.
  for (const v of geometry.pieces) out.pieces.push(v + 100000);
  out.piece = (geometry.piece ?? 0) + 100000;
  for (let i = 0; i < geometry.indices.length; i += 3) {
    out.indices.push(geometry.indices[i], geometry.indices[i + 2], geometry.indices[i + 1]);
  }
  return out;
}

/** Moves every vertex by `delta`. */
export function translate(geometry, delta) {
  for (let i = 0; i < geometry.positions.length; i += 3) {
    geometry.positions[i] += delta[0];
    geometry.positions[i + 1] += delta[1];
    geometry.positions[i + 2] += delta[2];
  }
  return geometry;
}

/** Axis-aligned bounds as `{ min, max }`. */
export function bounds(geometry) {
  const min = [Infinity, Infinity, Infinity];
  const max = [-Infinity, -Infinity, -Infinity];
  for (let i = 0; i < geometry.positions.length; i += 3) {
    for (let c = 0; c < 3; c++) {
      const v = geometry.positions[i + c];
      if (v < min[c]) min[c] = v;
      if (v > max[c]) max[c] = v;
    }
  }
  return { min, max };
}

/**
 * The sag of a conductor hanging between two points of equal height, as the
 * offset below the chord at the fraction `t` of the span.
 *
 * A catenary and a parabola differ by less than a centimetre over the spans a
 * line uses, and the parabola costs one multiplication — but the *shape* is the
 * whole point. A conductor drawn as a straight line between two masts is the
 * single most obvious way to make a power line look wrong, and the sag is a
 * tenth of nothing: about 3 % of the span at 380 kV, more in summer heat.
 */
export function sag(t, span, ratio = 0.03) {
  return 4 * ratio * span * t * (1 - t);
}

export { normalise, cross, sub, add, scale, dot, length };

/**
 * Writes the weathering into the vertex colours.
 *
 * Two things, and both are what stops a lattice from reading as one extruded
 * object: **per-member shade**, because four hundred bars galvanised in the
 * same bath still come out a few per cent apart and pick up dirt at their own
 * rate, and the **dirt at the foot**, because rain runs down a mast and mud
 * comes up it. Above about six metres a mast is clean; at the concrete it is
 * brown-grey and a fifth darker.
 *
 * It goes in the vertex colour and not in the texture because it is a property
 * of the *mast*, not of the material: the texture tiles every metre and knows
 * nothing about which end of the leg it is on.
 */
export function weather(geometry, { ground = 6.0, floor = 0.74, jitter = 0.05, seed = 1 } = {}) {
  const shadeOf = (piece) => {
    // A cheap integer hash — the same one the textures use, and deterministic,
    // so a rebuild produces the same mast.
    let h = Math.imul(piece + seed * 7919, 374761393);
    h = Math.imul(h ^ (h >>> 13), 1274126177);
    return (((h ^ (h >>> 16)) >>> 0) / 4294967296 - 0.5) * 2;
  };
  for (let i = 0; i < geometry.positions.length / 3; i++) {
    const y = geometry.positions[i * 3 + 1];
    const dirt = Math.max(0, 1 - Math.max(0, y) / ground);
    const value = (1 - dirt * (1 - floor)) * (1 + shadeOf(geometry.pieces[i]) * jitter);
    geometry.colors[i * 4] = value;
    // The dirt is warm; the clean steel is not.
    geometry.colors[i * 4 + 1] = value * (1 - dirt * 0.05);
    geometry.colors[i * 4 + 2] = value * (1 - dirt * 0.12);
    geometry.colors[i * 4 + 3] = 1;
  }
  return geometry;
}
