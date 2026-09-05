// What a wind turbine is made of, painted rather than photographed.
//
// One tiling material carries almost the whole machine: the **coating** — a
// two-pack polyurethane over epoxy on the steel tower, a gelcoat over the glass
// fibre of the nacelle and the blades. Both are the same light grey (RAL 7035
// on nearly every German machine; Enercon's is a touch warmer), both are
// semi-gloss, and both weather the same way: rain runs down them and leaves
// its streaks, the gloss goes chalky in the sun over ten years, and the
// leading edge of a blade is sand-blasted by a hundred kilometres an hour of
// rain until it is matte. None of that is *colour* — a turbine is white from a
// train — and almost all of it is roughness, which is why a flat factor
// material cannot get there however carefully the grey is chosen.
//
// The map tiles once per metre like the masts' (`tools/pylons/lib/texture.mjs`,
// whose helpers this file borrows): the UVs are in metres, `v` runs **along**
// a piece — up the tower, out along a blade — and `u` around it, so the rain
// streaks run down the tower whichever way the mesh was built. What is a
// property of the *machine* rather than of the coating — the dirt at the foot,
// the red marking bands, Enercon's green rings — goes into the vertex colour
// (`kit.mjs`), the same split the masts make.
//
// The frequency budget is the masts': nothing above a quarter of the map's
// size in cycles per metre, four texels a cycle, so the top mip levels average
// what they are meant to average instead of aliasing.

import { paint as paintPylon } from '../../pylons/lib/texture.mjs';

/** A deterministic hash in `0…1` — the same one the mast painters use. */
function hash(x, y, seed) {
  let h = (x | 0) * 374761393 + (y | 0) * 668265263 + seed * 1442695040888963407;
  h = Math.imul(h ^ (h >>> 13), 1274126177);
  return ((h ^ (h >>> 16)) >>> 0) / 4294967296;
}

function noise(x, y, px, py, seed) {
  const xi = Math.floor(x);
  const yi = Math.floor(y);
  const xf = x - xi;
  const yf = y - yi;
  const u = xf * xf * (3 - 2 * xf);
  const v = yf * yf * (3 - 2 * yf);
  const at = (i, j) => hash(((i % px) + px) % px, ((j % py) + py) % py, seed);
  const a = at(xi, yi);
  const b = at(xi + 1, yi);
  const c = at(xi, yi + 1);
  const d = at(xi + 1, yi + 1);
  return a + (b - a) * u + (c - a) * v + (a - b - c + d) * u * v;
}

/** Octaves over the unit tile; the wrap period is the frequency (see pylons). */
function fbm(u, v, fu, fv, seed, octaves = 4, gain = 0.5) {
  let sum = 0;
  let amp = 1;
  let norm = 0;
  for (let o = 0; o < octaves; o++) {
    const px = fu << o;
    const py = fv << o;
    sum += amp * noise(u * px, v * py, px, py, seed + o * 17);
    norm += amp;
    amp *= gain;
  }
  return sum / norm;
}

const clamp01 = (v) => (v < 0 ? 0 : v > 1 ? 1 : v);

function normalsFrom(height, size, strength) {
  const rgba = new Uint8Array(size * size * 4);
  const at = (x, y) => height[(((y % size) + size) % size) * size + (((x % size) + size) % size)];
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const physicalStrength = strength * (size / 512);
      const dx = (at(x + 1, y) - at(x - 1, y)) * physicalStrength;
      const dy = (at(x, y + 1) - at(x, y - 1)) * physicalStrength;
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
 * **The coating**, ten years into its service life.
 *
 * RAL 7035 light grey is sRGB 215 — under the sky's image-based light that is
 * a surface that comes close to blowing out against the very sky behind it,
 * which is also exactly what a turbine does on a bright day, so the base sits
 * at sRGB 200–210 rather than being pulled down the way the galvanising was.
 * A turbine that reads as mid-grey is a turbine that has been painted wrong.
 *
 * What is painted on top of it, all of it in the roughness far more than in
 * the colour:
 *
 * - **Orange peel.** A sprayed two-pack coating freezes into a fine waviness a
 *   few millimetres across; it is the only relief a tower has, and it is what
 *   keeps a specular highlight from being a hard mirror line down the tube.
 * - **Rain streaks.** Long along `v`, narrow across `u`: the dust the rain
 *   picks up on its way down the tower and the trails it leaves under every
 *   flange and every rivet. Faint in colour — a few per cent darker and a
 *   touch warmer — and *matte*, because dirt is.
 * - **Chalking.** The slow field of a gloss going dull as the binder is
 *   weathered out of the surface: a coating is glossier in the lee than on
 *   the weather side, and the difference is a broad roughness mottle.
 *
 * Metallic is zero throughout: paint over steel is a dielectric, and a
 * gelcoat over glass fibre is one too.
 */
function coating(size) {
  const height = new Float32Array(size * size);
  const shade = new Float32Array(size * size);
  const gloss = new Float32Array(size * size);
  const dirt = new Float32Array(size * size);
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const u = x / size;
      const v = y / size;
      const peel = fbm(u, v, 48, 48, 211, 3);
      const chalk = fbm(u, v, 3, 3, 313, 4);
      // Streaks: narrow across, very long along — the period along `v` is one
      // tile, so a streak runs unbroken down a tower forty metres tall.
      const streak = fbm(u, v, 24, 1, 419, 3);
      const trail = Math.max(0, streak - 0.58) * 2.4;
      const i = y * size + x;
      height[i] = (peel - 0.5) * 0.09;
      dirt[i] = clamp01(trail * (0.6 + chalk * 0.6));
      shade[i] = 0.80 - dirt[i] * 0.045 + (peel - 0.5) * 0.008;
      // Semi-gloss where the coating is fresh, matte where it has chalked and
      // where the dirt sits; the orange peel adds a fine sparkle either way.
      gloss[i] = 0.34 + chalk * 0.22 + dirt[i] * 0.28 + (peel - 0.5) * 0.06;
    }
  }
  return {
    colour: colour(size, (x, y) => {
      const i = y * size + x;
      const s = shade[i];
      const d = dirt[i];
      // A clean coating is a hair cool; the dust in the streaks is warm.
      return [s * (0.985 + d * 0.05), s * (0.99 + d * 0.015), s * (1.0 - d * 0.04)];
    }),
    orm: orm(size, (x, y) => ({
      occlusion: 1,
      roughness: gloss[y * size + x],
      metallic: 0,
    })),
    normal: normalsFrom(height, size, 0.55),
  };
}

/** The painters, by what a part is made of. */
export const PAINTERS = {
  coating: { id: 'lack', paint: coating },
};

/**
 * Paints one material's three maps at `size` x `size`. The concrete of the
 * foundation and the zinc of a lattice tower are the masts' own painters — a
 * foundation is a foundation, whatever stands on it.
 */
export function paint(structure, size = 1024) {
  const painter = PAINTERS[structure];
  if (painter) return { id: painter.id, ...painter.paint(size) };
  return paintPylon(structure, size);
}
