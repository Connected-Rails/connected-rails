// Deterministic random numbers and the noise the textures are painted with.
//
// Everything in the pipeline draws from a seed that is derived from the
// species id and the variant letter, so a rebuild produces byte-identical
// files and a diff of `mods/trees` only ever shows what the catalogue changed.

/** SplitMix64-style scrambler on 32 bit — enough for texture noise. */
export function hash32(x) {
  x |= 0;
  x = Math.imul(x ^ (x >>> 16), 0x7feb352d);
  x = Math.imul(x ^ (x >>> 15), 0x846ca68b);
  return (x ^ (x >>> 16)) >>> 0;
}

/** Turns a string into the 32 bit seed the generators start from. */
export function seedOf(text) {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return hash32(h);
}

/** Mulberry32: small, fast, and the same stream on every machine. */
export class Rng {
  constructor(seed) {
    this.state = seed >>> 0;
  }

  /** @returns {number} in [0, 1) */
  next() {
    this.state = (this.state + 0x6d2b79f5) >>> 0;
    let t = this.state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  }

  /** @returns {number} in [lo, hi) */
  range(lo, hi) {
    return lo + this.next() * (hi - lo);
  }

  /** A number around 1 with the given spread, e.g. 0.2 for ±20 %. */
  jitter(spread) {
    return 1 + (this.next() * 2 - 1) * spread;
  }

  pick(items) {
    return items[Math.floor(this.next() * items.length) % items.length];
  }
}

/**
 * Value noise on an integer lattice, tiling with period `period` in both
 * axes — bark has to repeat around the trunk without a seam.
 */
export function tileNoise(x, y, period, seed) {
  const x0 = Math.floor(x);
  const y0 = Math.floor(y);
  const fx = x - x0;
  const fy = y - y0;
  const at = (ix, iy) => {
    const wx = ((ix % period) + period) % period;
    const wy = ((iy % period) + period) % period;
    return hash32(hash32(wx + 0x9e3779b9) ^ hash32(wy * 0x85ebca6b) ^ seed) / 4294967296;
  };
  // Quintic fade — smooth enough that the lattice does not show as a grid.
  const sx = fx * fx * fx * (fx * (fx * 6 - 15) + 10);
  const sy = fy * fy * fy * (fy * (fy * 6 - 15) + 10);
  const n00 = at(x0, y0);
  const n10 = at(x0 + 1, y0);
  const n01 = at(x0, y0 + 1);
  const n11 = at(x0 + 1, y0 + 1);
  return n00 + (n10 - n00) * sx + (n01 - n00) * sy + (n00 - n10 - n01 + n11) * sx * sy;
}

/**
 * Fractal sum of {@link tileNoise}. `cells` is the lattice size of the first
 * octave over the whole tile; every octave doubles it, so the result tiles
 * seamlessly over [0, 1)².
 */
export function fbm(u, v, cells, octaves, seed, gain = 0.5) {
  let sum = 0;
  let amp = 1;
  let norm = 0;
  let n = cells;
  for (let o = 0; o < octaves; o++) {
    sum += amp * tileNoise(u * n, v * n, n, seed + o * 0x2545f491);
    norm += amp;
    amp *= gain;
    n *= 2;
  }
  return sum / norm;
}
