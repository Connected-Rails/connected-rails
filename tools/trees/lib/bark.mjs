// Bark, painted per species.
//
// Nothing here is photographed. A birch has to be white with black lenticels,
// a Scots pine orange-red in flat plates and a beech smooth grey, and no
// library of eleven generic bark scans gives that — so the pipeline paints the
// bark from the same catalogue entry that shapes the tree. The result tiles in
// both axes: the branch UVs wrap once around the trunk (`wrapsX = 1`, see
// build_trees.mjs) and repeat every two rings along it.
//
// There is no normal map. The relief is shaded into the colour instead —
// furrows dark, ridges light — which costs one texture instead of two per
// species and is what a trunk read at twenty metres from a moving train shows
// anyway.

import { Rng, fbm, seedOf } from './rng.mjs';
import { Surface, mix, rgb } from './raster.mjs';

/**
 * @param {number} size edge length [texels]
 * @param {object} spec the catalogue's `bark` block
 * @param {string} seedText what the noise is derived from
 * @returns {Surface}
 */
export function barkTexture(size, spec, seedText) {
  const seed = seedOf(seedText);
  const rng = new Rng(seed);
  const surface = new Surface(size, size);
  const base = rgb(spec.base);
  const dark = rgb(spec.dark);
  const light = rgb(spec.light);
  const style = spec.style ?? 'furrowed';
  const fissures = spec.fissures ?? 7;
  const relief = spec.relief ?? 0.7;

  for (let y = 0; y < size; y++) {
    const v = y / size;
    for (let x = 0; x < size; x++) {
      const u = x / size;
      // Warping the vertical furrows with low-frequency noise is what keeps
      // them from reading as corduroy.
      const warp = (fbm(u, v, 3, 3, seed + 11) - 0.5) * 1.6;
      const grain = fbm(u, v, 8, 4, seed + 23);
      const fine = fbm(u, v, 32, 3, seed + 37);
      let f = fissureField(style, u, v, fissures, warp, grain, seed);
      // Fine grain rides on top of every style.
      f = clamp01(f + (fine - 0.5) * 0.35);

      let color = f < 0.5 ? mix(dark, base, f * 2) : mix(base, light, (f - 0.5) * 2);
      color = mix(base, color, relief);
      // One slow octave over the whole sheet: what keeps a trunk from showing
      // its texture's period as a stack of identical courses.
      const mottle = fbm(u, v, 2, 2, seed + 617) - 0.5;
      color = [
        color[0] * (1 + mottle * 0.26),
        color[1] * (1 + mottle * 0.24),
        color[2] * (1 + mottle * 0.22),
      ];

      // Species-specific marks on top of the field.
      color = marks(style, color, u, v, seed, grain, fine, dark, light);

      // A trunk is darker where moss and damp sit, towards the foot of the
      // texture and in the deep furrows.
      const moss = spec.moss ?? 0;
      if (moss > 0) {
        const patch = fbm(u, v, 4, 3, seed + 71);
        const amount = Math.max(0, patch - 0.55) * 2.2 * moss * (1 - f * 0.6);
        color = mix(color, rgb(spec.mossColor ?? '#4a5a34'), Math.min(0.85, amount));
      }
      const shade = 1 + (rng.next() - 0.5) * 0.02;
      surface.blend(x, y, color[0] * shade, color[1] * shade, color[2] * shade, 1);
    }
  }
  return surface;
}

function clamp01(v) {
  return v < 0 ? 0 : v > 1 ? 1 : v;
}

/** The 0…1 relief field a style makes out of the noise. */
function fissureField(style, u, v, fissures, warp, grain, seed) {
  const ridged = (phase) => {
    // |sin| with the peaks flattened: ridges are broad, furrows narrow.
    const s = Math.abs(Math.sin(Math.PI * phase));
    return Math.pow(s, 0.55);
  };
  switch (style) {
    case 'smooth':
      // Beech and hornbeam: almost featureless, a slow mottle and the faint
      // vertical sinews of the trunk.
      return 0.45 + (grain - 0.5) * 0.5 + Math.sin(Math.PI * (u * 2 + warp * 0.3)) * 0.06;
    case 'birch':
      // Papery white with the horizontal grain of the peeling layers.
      return 0.72 + (fbm(u, v * 4, 6, 3, seed + 53) - 0.5) * 0.3;
    case 'plated': {
      // Scots pine: irregular flat plates with thin dark gaps between them.
      const cell = plates(u, v, 5, 7, seed + 101);
      return clamp01(0.35 + cell.edge * 0.75 + (grain - 0.5) * 0.3);
    }
    case 'scaly': {
      // Spruce: many small scales.
      const cell = plates(u, v, 11, 16, seed + 131);
      return clamp01(0.3 + cell.edge * 0.7 + (grain - 0.5) * 0.4);
    }
    case 'blistered':
      // Silver fir: smooth grey with resin blisters, added in `marks`.
      return 0.55 + (grain - 0.5) * 0.35;
    case 'cherry':
      // Glossy bands, the lenticels come later.
      return 0.5 + Math.sin(Math.PI * v * 6) * 0.05 + (grain - 0.5) * 0.3;
    case 'flaky': {
      // Sycamore and plane: big flakes that expose a paler layer.
      const cell = plates(u, v, 4, 4, seed + 149);
      return clamp01(0.45 + cell.edge * 0.4 + (grain - 0.5) * 0.5);
    }
    case 'fine':
      return clamp01(ridged(u * fissures + warp * 0.8) * 0.6 + 0.25 + (grain - 0.5) * 0.4);
    case 'diamond': {
      // Poplar and aspen: diamond-shaped lenticel scars over a smooth trunk.
      const d = Math.abs(Math.sin(Math.PI * (u * fissures + v * 3 + warp * 0.4))) *
        Math.abs(Math.sin(Math.PI * (u * fissures - v * 3 - warp * 0.4)));
      return clamp01(0.62 - Math.pow(d, 3) * 0.5 + (grain - 0.5) * 0.3);
    }
    case 'furrowed':
    default:
      // Oak, ash, elm, robinia: deep vertical furrows in long ridges.
      return clamp01(ridged(u * fissures + warp) * (0.75 + grain * 0.3) - 0.05);
  }
}

/** Extra marks a style paints over the relief. */
function marks(style, color, u, v, seed, grain, fine, dark, light) {
  switch (style) {
    case 'birch': {
      // Lenticels: short horizontal dashes, and a few peeling curls.
      const rowCount = 26;
      const row = Math.floor(v * rowCount);
      const jitter = fbm(row / rowCount, 0.31, 8, 2, seed + 211);
      const inRow = Math.abs(v * rowCount - (row + 0.5 + (jitter - 0.5) * 0.6));
      const density = fbm(u, row / rowCount, 10, 2, seed + 223);
      const width = 0.012 + density * 0.05;
      const centre = fbm(u * 0.5, row / rowCount, 6, 2, seed + 227);
      const near = Math.abs(((u * 7 + centre * 3) % 1) - 0.5);
      if (inRow < 0.34 && near < width * 6 && density > 0.42) {
        const t = 1 - inRow / 0.34;
        return mix(color, dark, Math.min(0.95, t * (0.5 + density * 0.6)));
      }
      // The foot of a birch is dark and cracked; that is baked into the model
      // by a second material band, not here — keep the sheet clean.
      return mix(color, light, Math.max(0, fine - 0.6) * 0.5);
    }
    case 'cherry': {
      // Horizontal lenticel bands, the mark of every Prunus.
      const rows = 9;
      const row = Math.floor(v * rows);
      const inRow = Math.abs(v * rows - (row + 0.5));
      if (inRow < 0.16) {
        const t = 1 - inRow / 0.16;
        const dashes = fbm(u, row / rows, 14, 2, seed + 307);
        if (dashes > 0.4) return mix(color, dark, t * 0.7);
      }
      return color;
    }
    case 'blistered': {
      // Resin blisters: small bright bumps.
      const b = fbm(u, v, 14, 2, seed + 401);
      if (b > 0.74) return mix(color, light, (b - 0.74) * 3.2);
      return color;
    }
    case 'plated': {
      // The gaps between the plates are near black, the plate faces flake to
      // a lighter orange in the middle.
      const inner = fbm(u, v, 20, 3, seed + 419);
      return mix(color, light, Math.max(0, inner - 0.6) * 0.7);
    }
    default:
      return color;
  }
}

/**
 * Worley-ish cell field on a jittered lattice that tiles: returns how far a
 * point is from the nearest cell border (`edge`, 0 at the border).
 */
function plates(u, v, cols, rows, seed) {
  let best = 1e9;
  let second = 1e9;
  const cu = u * cols;
  const cv = v * rows;
  const ix = Math.floor(cu);
  const iy = Math.floor(cv);
  for (let dy = -1; dy <= 1; dy++) {
    for (let dx = -1; dx <= 1; dx++) {
      const gx = ix + dx;
      const gy = iy + dy;
      const wx = ((gx % cols) + cols) % cols;
      const wy = ((gy % rows) + rows) % rows;
      const jx = fbm((wx + 0.5) / cols, (wy + 0.5) / rows, cols, 1, seed);
      const jy = fbm((wx + 0.5) / cols, (wy + 0.5) / rows, rows, 1, seed + 7);
      const px = gx + 0.04 + jx * 0.92;
      const py = gy + 0.04 + jy * 0.92;
      const d = Math.hypot(cu - px, cv - py);
      if (d < best) {
        second = best;
        best = d;
      } else if (d < second) {
        second = d;
      }
    }
  }
  // A wide falloff and the jitter above make the cells irregular; a tight one
  // draws a wall of identical bricks.
  const edge = Math.min(1, (second - best) * 1.05);
  return { edge, distance: best };
}
