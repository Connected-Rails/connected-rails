// Levels of detail, by collapsing edges.
//
// A kit model is one level of detail and a car park needs four. There is no
// Blender on the machines this repository is built on and no npm package in
// it, so the decimator is here: Garland and Heckbert's quadric error metric,
// which is the one every tool uses, in the length it takes when the input is a
// closed low-poly shell.
//
// Three things about *this* content shape the implementation:
//
// * **Colour is per face, not per vertex.** The kit paints its models from a
//   sheet of swatches, so a triangle has one colour and keeping it is a matter
//   of not merging triangles of different colours. A face carries its colour
//   through a collapse untouched, and an edge between two colours costs extra
//   to collapse — so a car loses the curve of its roof long before it loses
//   its windows, which is the right order to lose things in.
// * **Vertices stay where they were.** The optimal position of a collapse is
//   the solution of a 3×3 system, and it pulls vertices off the surface. For a
//   faceted shape at a hundred metres that buys nothing and costs the flat
//   panels their flatness, so a collapse moves one endpoint onto the other and
//   the shell keeps its own corners.
// * **A fold is refused outright.** An inverted face is a black hole in the
//   paint and no error metric sees it coming, so every collapse is tried
//   against the triangles that would move and taken back if one of them turns
//   over.

/**
 * Whether two faces count as the same colour.
 *
 * A payload may carry a fifth number — the palette cell the colour came out
 * of. Where it does, that is the identity: two shades of one swatch are one
 * paint, and comparing the shades would find a boundary inside every panel.
 */
function sameColour(a, b) {
  if (a === null || b === null) return false;
  if (a.length > 4 && b.length > 4) return a[4] === b[4];
  return a[0] === b[0] && a[1] === b[1] && a[2] === b[2];
}

/**
 * How hard a border pulls back, against the surface's own planes.
 *
 * A border is either the rim of an open shell or the line between two
 * colours, and both are decisions somebody made. Eight is enough that a
 * vertex on one slides *along* it for a long time before it steps off it.
 */
const BORDER = 8;
/**
 * How hard a crease pulls back, and how sharp an edge has to be to count.
 *
 * A crease is a border too — the line where two panels meet is as much a
 * decision as the line where two colours do. Without one, the corner of a box
 * body slides *along* its own top edge for nothing, because both planes it
 * sits on contain that edge, and a lorry's box collapses into a wedge while
 * every triangle in it stays perfectly flat.
 */
const CREASE = 4;
const CREASE_ANGLE = Math.cos((35 * Math.PI) / 180);
/** How far a face may swing before a collapse counts as a fold. */
const FOLD = 0.5;

/**
 * Welds triangles into a topological mesh.
 *
 * `positions` is flat xyz, `faces` are index triples into it, `colors` is one
 * rgba per face. Vertices within `tolerance` of each other **that also agree
 * about their texture coordinate** become one — a model is split at every hard
 * edge, and a decimator that could not see across those splits would collapse
 * nothing at all, but one that cannot see the texture either will tear the
 * atlas apart.
 */
export function weld(positions, faces, colors, tolerance = 1e-4, uvs = null) {
  const map = new Map();
  const out = [];
  const outUvs = uvs ? [] : null;
  // Which welded vertices came out of one position. A position that yielded
  // more than one is a place where the texture is cut open.
  const atPlace = new Map();
  const remap = new Array(positions.length / 3);
  for (let i = 0; i < positions.length / 3; i++) {
    const x = positions[i * 3];
    const y = positions[i * 3 + 1];
    const z = positions[i * 3 + 2];
    const place = `${Math.round(x / tolerance)},${Math.round(y / tolerance)},${Math.round(z / tolerance)}`;
    // Position *and* texture coordinate. Two corners that sit on the same
    // point but read different parts of the atlas are not the same vertex, and
    // merging them is not a rounding error: the survivor keeps one of the two
    // coordinates, and every triangle that used the other one is left reading
    // a piece of some entirely different panel.
    //
    // On a tidy atlas that is a handful of triangles along a seam. These are
    // photogrammetry atlases of several hundred small islands, so nearly every
    // shared corner disagrees, and welding by position alone smears the whole
    // sheet over the car — which it did, at every level below the finest.
    const key = uvs
      ? `${place}|${Math.round(uvs[i * 2] / 1e-4)},${Math.round(uvs[i * 2 + 1] / 1e-4)}`
      : place;
    let at = map.get(key);
    if (at === undefined) {
      at = out.length / 3;
      map.set(key, at);
      out.push(x, y, z);
      if (outUvs) outUvs.push(uvs[i * 2], uvs[i * 2 + 1]);
      if (!atPlace.has(place)) atPlace.set(place, []);
      atPlace.get(place).push(at);
    }
    remap[i] = at;
  }

  // A vertex that shares its position with another is on a cut. `simplify`
  // walls off the edges between two such vertices, so the island keeps its
  // shape instead of being pulled across the cut by its neighbour.
  const seams = new Set();
  for (const together of atPlace.values()) {
    if (together.length > 1) for (const v of together) seams.add(v);
  }

  const welded = [];
  const weldedColors = [];
  for (let f = 0; f < faces.length; f++) {
    const [a, b, c] = faces[f].map((i) => remap[i]);
    // A triangle whose corners welded together has no area left.
    if (a === b || b === c || a === c) continue;
    welded.push([a, b, c]);
    weldedColors.push(colors[f]);
  }
  return { positions: out, faces: welded, colors: weldedColors, uvs: outUvs, seams };
}

/** The plane of a triangle as `[a, b, c, d]`, normalised, or null if degenerate. */
function plane(p, [i, j, k]) {
  const ax = p[j * 3] - p[i * 3];
  const ay = p[j * 3 + 1] - p[i * 3 + 1];
  const az = p[j * 3 + 2] - p[i * 3 + 2];
  const bx = p[k * 3] - p[i * 3];
  const by = p[k * 3 + 1] - p[i * 3 + 1];
  const bz = p[k * 3 + 2] - p[i * 3 + 2];
  let nx = ay * bz - az * by;
  let ny = az * bx - ax * bz;
  let nz = ax * by - ay * bx;
  const length = Math.hypot(nx, ny, nz);
  if (length < 1e-12) return null;
  nx /= length;
  ny /= length;
  nz /= length;
  return [nx, ny, nz, -(nx * p[i * 3] + ny * p[i * 3 + 1] + nz * p[i * 3 + 2])];
}

/** Adds the outer product of a plane with itself into a quadric. */
function addPlane(q, [a, b, c, d], weight = 1) {
  q[0] += weight * a * a; q[1] += weight * a * b; q[2] += weight * a * c; q[3] += weight * a * d;
  q[4] += weight * b * b; q[5] += weight * b * c; q[6] += weight * b * d;
  q[7] += weight * c * c; q[8] += weight * c * d;
  q[9] += weight * d * d;
}

function addQuadric(into, from) {
  for (let i = 0; i < 10; i++) into[i] += from[i];
}

/** vᵀQv — how far a point is from all the planes the quadric remembers. */
function error(q, x, y, z) {
  return (
    q[0] * x * x + 2 * q[1] * x * y + 2 * q[2] * x * z + 2 * q[3] * x +
    q[4] * y * y + 2 * q[5] * y * z + 2 * q[6] * y +
    q[7] * z * z + 2 * q[8] * z +
    q[9]
  );
}

/** A binary heap of collapses, cheapest first. */
class Heap {
  constructor() {
    this.items = [];
  }

  get size() {
    return this.items.length;
  }

  push(item) {
    const items = this.items;
    items.push(item);
    let i = items.length - 1;
    while (i > 0) {
      const parent = (i - 1) >> 1;
      if (items[parent].cost <= items[i].cost) break;
      [items[parent], items[i]] = [items[i], items[parent]];
      i = parent;
    }
  }

  pop() {
    const items = this.items;
    const top = items[0];
    const last = items.pop();
    if (items.length) {
      items[0] = last;
      let i = 0;
      for (;;) {
        const l = i * 2 + 1;
        const r = l + 1;
        let small = i;
        if (l < items.length && items[l].cost < items[small].cost) small = l;
        if (r < items.length && items[r].cost < items[small].cost) small = r;
        if (small === i) break;
        [items[small], items[i]] = [items[i], items[small]];
        i = small;
      }
    }
    return top;
  }
}

/**
 * Collapses `mesh` down to `target` triangles, or as near as it can get
 * without folding anything over.
 *
 * Returns a fresh mesh — the input is untouched, so one source yields every
 * level of the chain.
 */
export function simplify(mesh, target) {
  const positions = mesh.positions.slice();
  const faces = mesh.faces.map((f) => f.slice());
  const colors = mesh.colors.slice();
  const uvs = mesh.uvs ? mesh.uvs.slice() : null;
  const seams = mesh.seams ?? new Set();
  const alive = new Array(faces.length).fill(true);
  const count = positions.length / 3;

  const quadrics = Array.from({ length: count }, () => new Array(10).fill(0));
  const around = Array.from({ length: count }, () => new Set());
  let living = 0;
  for (let f = 0; f < faces.length; f++) {
    const pl = plane(positions, faces[f]);
    if (!pl) {
      alive[f] = false;
      continue;
    }
    living++;
    for (const v of faces[f]) {
      addPlane(quadrics[v], pl);
      around[v].add(f);
    }
  }

  // Borders. Three kinds, and they are all the same thing: a line somebody
  // decided on. The rim of an open shell, the line where one colour meets
  // another, and the crease where two panels meet.
  //
  // Each gets a plane standing *on* the surface along the line, which is what
  // a border really is, and both its ends are held to it. A vertex then slides
  // freely along the border and pays to step away from it.
  //
  // Without the colour one, a tail lamp — a small patch lying in the plane of
  // the panel behind it — has no quadric but that one plane, so collapsing a
  // corner of it anywhere else in the same plane is free, and the lamp
  // stretches across the whole back of the car. Without the crease one, the
  // top rear corner of a lorry's box slides along its own roof edge for
  // nothing, because both planes it sits on contain that edge, and the box
  // collapses into a wedge while every triangle in it stays perfectly flat.
  const edgeFaces = new Map();
  const key = (a, b) => (a < b ? `${a},${b}` : `${b},${a}`);
  for (let f = 0; f < faces.length; f++) {
    if (!alive[f]) continue;
    const [a, b, c] = faces[f];
    for (const [i, j] of [[a, b], [b, c], [c, a]]) {
      const k = key(i, j);
      if (!edgeFaces.has(k)) edgeFaces.set(k, []);
      edgeFaces.get(k).push(f);
    }
  }
  for (const [k, on] of edgeFaces) {
    const [ka, kb] = k.split(',').map(Number);
    let weight = 0;
    if (on.length === 1) {
      weight = BORDER;
    } else if (seams.has(ka) && seams.has(kb)) {
      // Both ends sit where the texture is cut open: the edge is the cut.
      weight = BORDER;
    } else if (on.length === 2) {
      if (!sameColour(colors[on[0]], colors[on[1]])) {
        weight = BORDER;
      } else {
        const first = plane(positions, faces[on[0]]);
        const second = plane(positions, faces[on[1]]);
        const flat =
          first && second && first[0] * second[0] + first[1] * second[1] + first[2] * second[2];
        if (flat !== null && flat !== false && flat < CREASE_ANGLE) weight = CREASE;
      }
    }
    if (!weight) continue;
    const [a, b] = [ka, kb];
    const pl = plane(positions, faces[on[0]]);
    if (!pl) continue;
    const ex = positions[b * 3] - positions[a * 3];
    const ey = positions[b * 3 + 1] - positions[a * 3 + 1];
    const ez = positions[b * 3 + 2] - positions[a * 3 + 2];
    let nx = ey * pl[2] - ez * pl[1];
    let ny = ez * pl[0] - ex * pl[2];
    let nz = ex * pl[1] - ey * pl[0];
    const length = Math.hypot(nx, ny, nz);
    if (length < 1e-12) continue;
    nx /= length; ny /= length; nz /= length;
    const wall = [
      nx, ny, nz,
      -(nx * positions[a * 3] + ny * positions[a * 3 + 1] + nz * positions[a * 3 + 2]),
    ];
    for (const v of [a, b]) addPlane(quadrics[v], wall, weight);
  }

  const removed = new Array(count).fill(false);
  const version = new Array(count).fill(0);
  const refused = new Set();

  const costOf = (from, to) => {
    const q = quadrics[from].slice();
    addQuadric(q, quadrics[to]);
    return error(q, positions[to * 3], positions[to * 3 + 1], positions[to * 3 + 2]);
  };

  const heap = new Heap();
  // A refusal is remembered against the versions it was refused at: once the
  // neighbourhood has changed the collapse may well be fine, and a refusal
  // that stuck forever is what leaves a coarse level twice the size it was
  // asked for.
  const refusalKey = (from, to) => `${from}>${to}@${version[from]},${version[to]}`;
  const offer = (from, to) => {
    if (removed[from] || removed[to] || from === to) return;
    if (refused.has(refusalKey(from, to))) return;
    heap.push({ from, to, cost: costOf(from, to), va: version[from], vb: version[to] });
  };
  const offerAround = (v) => {
    for (const f of around[v]) {
      if (!alive[f]) continue;
      for (const other of faces[f]) {
        if (other === v) continue;
        offer(v, other);
        offer(other, v);
      }
    }
  };
  for (let f = 0; f < faces.length; f++) {
    if (!alive[f]) continue;
    const [a, b, c] = faces[f];
    for (const [i, j] of [[a, b], [b, c], [c, a]]) {
      offer(i, j);
      offer(j, i);
    }
  }

  /**
   * Moves `from` onto `to`, or refuses because something would fold over.
   *
   * Returns how many faces died and which vertices need re-pricing.
   */
  const collapse = (from, to) => {
    const moving = [...around[from]].filter((f) => alive[f]);
    const before = new Map();
    for (const f of moving) {
      if (faces[f].includes(to)) continue;
      const pl = plane(positions, faces[f]);
      if (pl) before.set(f, pl);
    }
    const saved = [positions[from * 3], positions[from * 3 + 1], positions[from * 3 + 2]];
    positions[from * 3] = positions[to * 3];
    positions[from * 3 + 1] = positions[to * 3 + 1];
    positions[from * 3 + 2] = positions[to * 3 + 2];
    let folded = false;
    for (const [f, was] of before) {
      const now = plane(positions, faces[f]);
      if (!now || was[0] * now[0] + was[1] * now[1] + was[2] * now[2] < FOLD) {
        folded = true;
        break;
      }
    }
    if (folded) {
      positions[from * 3] = saved[0];
      positions[from * 3 + 1] = saved[1];
      positions[from * 3 + 2] = saved[2];
      return { died: 0, touched: null };
    }

    let died = 0;
    for (const f of moving) {
      if (!faces[f].includes(to)) continue;
      alive[f] = false;
      died++;
      for (const v of faces[f]) around[v].delete(f);
    }
    for (const f of [...around[from]]) {
      if (!alive[f]) continue;
      faces[f] = faces[f].map((v) => (v === from ? to : v));
      around[to].add(f);
      around[from].delete(f);
    }
    addQuadric(quadrics[to], quadrics[from]);
    removed[from] = true;
    // Everything that touched either end has to be priced again.
    const touched = new Set([to]);
    for (const f of around[to]) {
      if (alive[f]) for (const v of faces[f]) touched.add(v);
    }
    for (const v of touched) version[v]++;
    return { died, touched };
  };

  while (living > target && heap.size > 0) {
    const top = heap.pop();
    if (removed[top.from] || removed[top.to]) continue;
    if (version[top.from] !== top.va || version[top.to] !== top.vb) continue;
    const { died, touched } = collapse(top.from, top.to);
    if (died === 0) {
      refused.add(refusalKey(top.from, top.to));
      continue;
    }
    living -= died;
    // Every vertex whose version was just bumped, not only the survivor. A
    // bumped version silently voids that vertex's entries in the heap, so
    // re-offering the survivor alone throws away every edge *between* its
    // neighbours — the queue runs dry with thousands of perfectly good
    // collapses never reconsidered, and the level stops far above the size it
    // was asked for. Two of the four levels came out byte for byte identical
    // because both were stopped by this rather than by their target.
    for (const v of touched) offerAround(v);
  }

  const keep = [];
  const keepColors = [];
  for (let f = 0; f < faces.length; f++) {
    if (!alive[f]) continue;
    const [a, b, c] = faces[f];
    if (a === b || b === c || a === c) continue;
    keep.push([a, b, c]);
    keepColors.push(colors[f]);
  }
  return compact({ positions, faces: keep, colors: keepColors, uvs });
}

/** Drops the vertices nothing points at any more. */
function compact(mesh) {
  const used = new Map();
  const positions = [];
  const uvs = mesh.uvs ? [] : null;
  const faces = mesh.faces.map((face) =>
    face.map((v) => {
      let at = used.get(v);
      if (at === undefined) {
        at = positions.length / 3;
        used.set(v, at);
        positions.push(
          mesh.positions[v * 3],
          mesh.positions[v * 3 + 1],
          mesh.positions[v * 3 + 2],
        );
        if (uvs) uvs.push(mesh.uvs[v * 2], mesh.uvs[v * 2 + 1]);
      }
      return at;
    }),
  );
  return { positions, faces, colors: mesh.colors, uvs };
}
