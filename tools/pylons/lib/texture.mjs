// What the steel, the concrete and the wood are made of, painted rather than
// photographed.
//
// A flat base colour plus a metallic and a roughness number is a *correct* PBR
// material and it still reads as plastic, because nothing real is uniform. What
// makes hot-dip galvanising look like galvanising is the **spangle** — the zinc
// freezes into crystal facets a centimetre or two across, each catching the
// light at its own angle — and what makes it look weathered is that some of
// those facets have gone dull while others still shine. None of that is colour;
// almost all of it is *roughness*, which is why a base-colour-only material
// cannot get there however carefully the grey is chosen.
//
// So each material ships three maps, and they tile **once per metre** (the UVs
// are in metres, see `geom.mjs`): a base colour, an ORM (occlusion, roughness,
// metallic — the glTF packing), and a normal map derived from the same height
// field the colour came from. Everything is generated here from a seeded hash,
// so the files are reproducible and nothing is downloaded.
//
// The maps are small on purpose. 256 px per metre is a millimetre per texel,
// which is finer than anyone gets to stand to a mast that is behind a fence.

/** A deterministic hash in `0…1` — the whole random source of this file. */
function hash(x, y, seed) {
  let h = (x | 0) * 374761393 + (y | 0) * 668265263 + seed * 1442695040888963407;
  h = Math.imul(h ^ (h >>> 13), 1274126177);
  return ((h ^ (h >>> 16)) >>> 0) / 4294967296;
}

/** Value noise on a grid of `period` cells, wrapping — so the tile is seamless. */
function noise(x, y, period, seed) {
  const xi = Math.floor(x);
  const yi = Math.floor(y);
  const xf = x - xi;
  const yf = y - yi;
  const u = xf * xf * (3 - 2 * xf);
  const v = yf * yf * (3 - 2 * yf);
  const at = (i, j) => hash(((i % period) + period) % period, ((j % period) + period) % period, seed);
  const a = at(xi, yi);
  const b = at(xi + 1, yi);
  const c = at(xi, yi + 1);
  const d = at(xi + 1, yi + 1);
  return a + (b - a) * u + (c - a) * v + (a - b - c + d) * u * v;
}

/** Several octaves of it. */
function fbm(x, y, period, seed, octaves = 4, gain = 0.5) {
  let sum = 0;
  let amp = 1;
  let norm = 0;
  for (let o = 0; o < octaves; o++) {
    const p = period << o;
    sum += amp * noise(x * (1 << o), y * (1 << o), p, seed + o * 17);
    norm += amp;
    amp *= gain;
  }
  return sum / norm;
}

/**
 * The zinc spangle: which crystal facet a point belongs to, and how far it is
 * from the facet's edge.
 *
 * Wrapped in both directions — the seeds of the neighbouring copies of the tile
 * are searched too, or the crystals would be cut in half at the seam and a
 * lattice tiled every metre would show a grid.
 */
function spangle(x, y, cells, seed) {
  const cx = Math.floor(x * cells);
  const cy = Math.floor(y * cells);
  let best = Infinity;
  let second = Infinity;
  let id = 0;
  for (let j = -1; j <= 1; j++) {
    for (let i = -1; i <= 1; i++) {
      const gx = cx + i;
      const gy = cy + j;
      const wx = ((gx % cells) + cells) % cells;
      const wy = ((gy % cells) + cells) % cells;
      const px = (gx + hash(wx, wy, seed)) / cells;
      const py = (gy + hash(wx, wy, seed + 991)) / cells;
      const d = (px - x) * (px - x) + (py - y) * (py - y);
      if (d < best) {
        second = best;
        best = d;
        id = wx * 977 + wy;
      } else if (d < second) {
        second = d;
      }
    }
  }
  // The gap between the nearest two seeds is small on a facet edge and large in
  // the middle of one — the standard way to get a crystal boundary out of a
  // Voronoi cell without tracing it.
  return { id, edge: Math.sqrt(second) - Math.sqrt(best) };
}

const clamp01 = (v) => (v < 0 ? 0 : v > 1 ? 1 : v);

/**
 * Turns a height field into a tangent-space normal map, wrapping at the edges
 * so it tiles with the colour it came from.
 */
function normalsFrom(height, size, strength) {
  const rgba = new Uint8Array(size * size * 4);
  const at = (x, y) => height[(((y % size) + size) % size) * size + (((x % size) + size) % size)];
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const dx = (at(x + 1, y) - at(x - 1, y)) * strength;
      const dy = (at(x, y + 1) - at(x, y - 1)) * strength;
      const len = Math.hypot(dx, dy, 1);
      const o = (y * size + x) * 4;
      rgba[o] = Math.round(((-dx / len) * 0.5 + 0.5) * 255);
      rgba[o + 1] = Math.round(((-dy / len) * 0.5 + 0.5) * 255);
      rgba[o + 2] = Math.round(((1 / len) * 0.5 + 0.5) * 255);
      rgba[o + 3] = 255;
    }
  }
  return rgba;
}

/** Packs occlusion, roughness and metallic the way glTF wants them. */
function orm(size, fill) {
  const rgba = new Uint8Array(size * size * 4);
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const { occlusion, roughness, metallic } = fill(x, y);
      const o = (y * size + x) * 4;
      rgba[o] = Math.round(clamp01(occlusion) * 255);
      rgba[o + 1] = Math.round(clamp01(roughness) * 255);
      rgba[o + 2] = Math.round(clamp01(metallic) * 255);
      rgba[o + 3] = 255;
    }
  }
  return rgba;
}

function colour(size, fill) {
  const rgba = new Uint8Array(size * size * 4);
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const c = fill(x, y);
      const o = (y * size + x) * 4;
      rgba[o] = Math.round(clamp01(c[0]) * 255);
      rgba[o + 1] = Math.round(clamp01(c[1]) * 255);
      rgba[o + 2] = Math.round(clamp01(c[2]) * 255);
      rgba[o + 3] = 255;
    }
  }
  return rgba;
}

/**
 * **Hot-dip galvanised steel.** A metre of it holds a few dozen zinc crystals;
 * each is a shade of its own and, more to the point, a *gloss* of its own, and
 * a slow mottle of weathering runs over the lot.
 *
 * **Weathered zinc is not a mirror**, and this is where a first attempt went
 * wrong: fresh galvanising is a metal with a reflectance near 0.85, so
 * `metallic = 1` and a bright base colour gave a mast that reflected the whole
 * sky and blew out white against it. What a mast on a line actually wears is
 * zinc *carbonate* — the dull grey skin that forms in the first year or two —
 * and that is a **dielectric** layer over the metal. So the weathering field
 * drives the metallic as well: a facet that is still bright reads as metal, one
 * that has gone chalky reads as the mineral it is covered in, and the mast ends
 * up the mid grey a mast is.
 */
function galvanised(size) {
  const height = new Float32Array(size * size);
  const shade = new Float32Array(size * size);
  const gloss = new Float32Array(size * size);
  const metal = new Float32Array(size * size);
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const u = x / size;
      const v = y / size;
      const cell = spangle(u, v, 11, 7);
      const facet = hash(cell.id, 3, 41);
      const edge = clamp01(cell.edge * 26);
      // Weathering: a slow field that decides which facets have gone dull.
      const weather = fbm(u * 3, v * 3, 4, 13, 4);
      // Fine drawing marks along the rolled bar, and the pitting of a few years.
      const grain = fbm(u * 90, v * 6, 16, 29, 2);
      const pit = Math.max(0, fbm(u * 40, v * 40, 24, 61, 3) - 0.62) * 2.6;
      const i = y * size + x;
      height[i] = edge * 0.55 + grain * 0.12 - pit * 0.7;
      // How far this spot has gone over to the chalky carbonate skin.
      const dull = clamp01(weather * 1.15 + pit * 0.5 - 0.15);
      shade[i] = 0.78 + (facet - 0.5) * 0.1 - (1 - edge) * 0.05 - dull * 0.2;
      gloss[i] = 0.44 + (facet - 0.5) * 0.1 + dull * 0.34 + (1 - edge) * 0.04;
      metal[i] = 0.92 - dull * 0.55;
    }
  }
  return {
    colour: colour(size, (x, y) => {
      const s = shade[y * size + x];
      // Zinc is very slightly blue.
      return [s * 0.985, s * 0.995, s * 1.0];
    }),
    orm: orm(size, (x, y) => ({
      occlusion: 1,
      roughness: gloss[y * size + x],
      metallic: metal[y * size + x],
    })),
    normal: normalsFrom(height, size, 1.6),
  };
}

/**
 * **Spun concrete.** A centrifuged pole is dense and smooth on the outside —
 * the aggregate is thrown to the wall, so what shows is a fine speckle of sand
 * and the odd larger stone, plus the faint long streaks the mould leaves. Grey
 * with a warm cast, and rough everywhere.
 */
function concrete(size) {
  const height = new Float32Array(size * size);
  const shade = new Float32Array(size * size);
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const u = x / size;
      const v = y / size;
      const sand = fbm(u * 120, v * 120, 32, 5, 3);
      const stones = Math.max(0, fbm(u * 22, v * 22, 16, 71, 2) - 0.58) * 2.4;
      const mould = fbm(u * 2, v * 40, 8, 97, 2);
      const dirt = fbm(u * 4, v * 4, 4, 131, 4);
      const i = y * size + x;
      height[i] = sand * 0.35 + stones * 0.6 + mould * 0.1;
      shade[i] = 0.66 + (sand - 0.5) * 0.16 + stones * 0.14 - dirt * 0.14 + (mould - 0.5) * 0.05;
    }
  }
  return {
    colour: colour(size, (x, y) => {
      const s = shade[y * size + x];
      return [s * 1.02, s * 1.0, s * 0.95];
    }),
    orm: orm(size, (x, y) => ({
      occlusion: 1 - clamp01(0.35 - height[y * size + x] * 0.35),
      roughness: 0.88 + (hash(x, y, 3) - 0.5) * 0.06,
      metallic: 0,
    })),
    normal: normalsFrom(height, size, 2.2),
  };
}

/**
 * **Creosoted pine.** The grain runs the length of the pole — which is `v` in
 * this pipeline, because a tube's `u` goes round it — and the creosote makes it
 * near black in the grooves and a dull brown on the raised grain. Checks
 * (the long splits a pole dries into) are the deepest thing on it.
 */
function wood(size) {
  const height = new Float32Array(size * size);
  const shade = new Float32Array(size * size);
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const u = x / size;
      const v = y / size;
      // Rings drawn out along the pole: a ramp across, wobbled along.
      const wobble = fbm(u * 3, v * 12, 8, 17, 3) * 0.16;
      const rings = (u * 9 + wobble) % 1;
      const grain = Math.abs(rings - 0.5) * 2;
      const fibre = fbm(u * 60, v * 8, 24, 53, 2);
      const check = Math.max(0, fbm(u * 8, v * 2, 8, 211, 2) - 0.72) * 3.4;
      const i = y * size + x;
      height[i] = grain * 0.3 + fibre * 0.15 - check * 0.9;
      shade[i] = 0.3 + grain * 0.16 + (fibre - 0.5) * 0.1 - check * 0.22;
    }
  }
  return {
    colour: colour(size, (x, y) => {
      const s = shade[y * size + x];
      return [s * 1.0, s * 0.78, s * 0.58];
    }),
    orm: orm(size, (x, y) => ({
      occlusion: 1 - clamp01(0.3 - height[y * size + x] * 0.3),
      roughness: 0.78 + (1 - clamp01(height[y * size + x] + 0.3)) * 0.14,
      metallic: 0,
    })),
    normal: normalsFrom(height, size, 2.6),
  };
}

/** The painters, by the atlas's `structure`. */
export const PAINTERS = {
  'steel-lattice': { id: 'verzinkt', paint: galvanised },
  // A compact mast's tube is the same hot-dip zinc as a lattice; one set of
  // maps serves both, and `build_pylons.mjs` keys on the painter's id to avoid
  // shipping it twice.
  'steel-tube': { id: 'verzinkt', paint: galvanised },
  'spun-concrete': { id: 'beton', paint: concrete },
  wood: { id: 'holz', paint: wood },
};

/** Paints one material's three maps at `size` x `size`. */
export function paint(structure, size = 256) {
  const painter = PAINTERS[structure];
  if (!painter) throw new Error(`no painter for ${structure}`);
  return { id: painter.id, ...painter.paint(size) };
}
