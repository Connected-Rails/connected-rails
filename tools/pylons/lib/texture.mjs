// What the steel, the concrete and the wood are made of, painted rather than
// photographed.
//
// A flat base colour plus a metallic and a roughness number is a *correct* PBR
// material and it still reads as plastic, because nothing real is uniform. What
// makes hot-dip galvanising look like galvanising is the **spangle** — the zinc
// freezes into crystal facets a couple of centimetres across, each catching the
// light at its own angle — and what makes it look weathered is that some of
// those facets have gone dull while others still shine. None of that is colour;
// almost all of it is *roughness*, which is why a base-colour-only material
// cannot get there however carefully the grey is chosen.
//
// It is also why the spangle has to stay out of the **normal** map. A crystal
// differs from its neighbour in gloss, not in height — the facets freeze level
// with one another — and a map that embosses them turns a bar of angle iron
// into beaten metal at the first raking light.
//
// So each material ships three maps, and they tile **once per metre** (the UVs
// are in metres, see `geom.mjs`): a base colour, an ORM (occlusion, roughness,
// metallic — the glTF packing), and a normal map derived from the same height
// field the colour came from. Everything is generated here from a seeded hash,
// so the files are reproducible and nothing is downloaded.
//
// **Nothing is drawn finer than the map can hold, and nothing finer than the eye
// can keep.** Both halves of that were wrong to begin with. The concrete drew
// its sand at 120 cycles a metre over three octaves — 480 cycles at the top,
// half a texel each on a 256 px map — so what came out was not sand but
// aliasing, and a pole read as sandpaper close up. The wood drew its fibre the
// same way and lost it the other way round: the detail was so fine that the
// first mip level averaged it flat, and a creosoted pole ten metres off was a
// smooth pink stick.
//
// So there is a budget, and every painter keeps to it: **the highest octave of
// any term stays at or below a quarter of [`SIZE`] cycles per metre**, which is
// four texels a cycle. What is left over goes into the roughness, where a
// material's microstructure belongs. And the features that carry the material
// are the ones that *survive*: a pole is looked at from ten metres far more
// often than from one, so the things worth drawing are the checks, the knots,
// the mould streaks and the aggregate — centimetres, not millimetres.

/** A deterministic hash in `0…1` — the whole random source of this file. */
function hash(x, y, seed) {
  let h = (x | 0) * 374761393 + (y | 0) * 668265263 + seed * 1442695040888963407;
  h = Math.imul(h ^ (h >>> 13), 1274126177);
  return ((h ^ (h >>> 16)) >>> 0) / 4294967296;
}

/**
 * Value noise on a grid that wraps every `px` by `py` cells.
 *
 * Two periods, not one, because almost everything on a mast is anisotropic —
 * the grain of the steel, the mould's seam down a concrete pole, the fibre of a
 * creosoted one all run one way and not the other.
 */
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

/**
 * Several octaves of it over the unit tile, `fu` cells across and `fv` down.
 *
 * **The wrap period is the frequency**, and that is the whole point of this
 * signature. The first version took the two separately — `fbm(u * 3, v * 3, 4,
 * …)`, three cycles over a tile that wrapped every four — and of the twelve
 * terms the three materials are made of, exactly one happened to match. The
 * other eleven were discontinuous at the tile boundary, so every surface in the
 * catalogue carried a **grid of seams a metre apart**. On a lattice nothing
 * shows, because no member is a metre wide in both directions; on the compact
 * mast's two-metre tube it was a brick wall. Passing the frequency and deriving
 * the period from it makes a mismatch impossible to write.
 */
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

/**
 * How many zinc crystals fit across a metre.
 *
 * A **24 mm** facet, which is the middle of the band a hot-dip bath throws on
 * structural steel (5 mm to about 50 mm on thick sections). This number is the
 * one thing that decides whether the material reads as galvanising or as
 * crumpled foil: at the 11 it started on, a crystal was 90 mm — wider than most
 * braces on the mast, so a bar carried one or two of them and the spangle
 * stopped being a surface finish and became the shape of the bar. It is also
 * why the pattern stayed visible at a hundred metres, where real spangle is
 * long gone.
 */
const SPANGLE_CELLS = 42;

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
 * **Hot-dip galvanised steel**, twenty years into its service life.
 *
 * A metre of it holds a couple of thousand zinc crystals; each is a shade of
 * its own and, more to the point, a *gloss* of its own, and a slow mottle of
 * weathering runs over the lot.
 *
 * **Weathered zinc is not a mirror.** Fresh galvanising is a metal with a
 * reflectance near 0.85, and `metallic` near one over a bright base colour
 * gives a mast that reflects the whole sky — under an image-based light off
 * Bevy's atmosphere that is not a subtle error, it is a blue-white mast that
 * blows out against the very sky it is mirroring. What a mast on a line
 * actually wears is zinc *carbonate*, the chalky grey skin of its first years,
 * and that is a **dielectric** over the metal. So the weathering field drives
 * the metallic, and it drives it into a narrow band low down: `0.08` where the
 * skin is thickest to `0.36` on a facet still bright. What is left is a broad
 * dim sheen rather than a reflection, which is what galvanising has.
 *
 * **And it is flat.** The spangle is a pattern in *reflectance*, not in relief:
 * the crystals freeze level with each other and you cannot feel the boundaries
 * with a hand. Putting them in the height field — where they were, at four
 * fifths of it — embossed every crystal, and a bar lit from the side then read
 * as beaten metal. What relief the surface does have is the gentle orange peel
 * a dipped coating freezes into, the mill's rolling marks, and pitting where
 * the zinc has finally failed; all three are under a millimetre.
 */
function galvanised(size) {
  const height = new Float32Array(size * size);
  const shade = new Float32Array(size * size);
  const gloss = new Float32Array(size * size);
  const metal = new Float32Array(size * size);
  const chalk = new Float32Array(size * size);
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const u = x / size;
      const v = y / size;
      const cell = spangle(u, v, SPANGLE_CELLS, 7);
      const facet = hash(cell.id, 3, 41);
      // The facet boundary is a couple of texels wide however many cells there
      // are — the width is a property of the crystal, not of the tile.
      const edge = clamp01(cell.edge * SPANGLE_CELLS * 2.4);
      // Weathering: a slow field that decides which facets have gone dull.
      const weather = fbm(u, v, 3, 3, 13, 4);
      // The mill's rolling marks, which run *along* the bar — `v` in this
      // pipeline, on a member as on a tube (`geom.mjs`). 60 cycles over two
      // octaves is 120 at the top, which is the budget at 512 px.
      const grain = fbm(u, v, 6, 60, 29, 2);
      // The waviness a coating freezes into as it drains off the section.
      const peel = fbm(u, v, 14, 14, 83, 2);
      // Pitting: where the zinc has gone and the steel underneath is working.
      // 30 over three octaves tops out at 120 — 8 mm pits, which is what they
      // are; at 40 the top octave was finer than the map and came out as noise.
      const pit = Math.max(0, fbm(u, v, 30, 30, 61, 3) - 0.66) * 2.4;
      const i = y * size + x;
      height[i] = edge * 0.06 + peel * 0.22 + grain * 0.1 - pit * 0.35;
      // How far this spot has gone over to the chalky carbonate skin.
      const dull = clamp01(weather * 1.15 + pit * 0.5 - 0.1);
      shade[i] = 0.56 + (facet - 0.5) * 0.07 - (1 - edge) * 0.02 - dull * 0.07;
      gloss[i] = 0.6 + (facet - 0.5) * 0.08 + dull * 0.26 + (1 - edge) * 0.03;
      metal[i] = 0.36 - dull * 0.28;
      chalk[i] = dull;
    }
  }
  return {
    colour: colour(size, (x, y) => {
      const i = y * size + x;
      const s = shade[i];
      const d = chalk[i];
      // Bright zinc is very slightly blue; the carbonate over it is very
      // slightly warm, which is the only colour in the material.
      return [s * (0.978 + d * 0.035), s * (0.993 + d * 0.007), s * (1.0 - d * 0.03)];
    }),
    orm: orm(size, (x, y) => ({
      occlusion: 1,
      roughness: gloss[y * size + x],
      metallic: metal[y * size + x],
    })),
    // Gently: the height field is under a millimetre from end to end, and a
    // strength that suited an embossed spangle turns a millimetre into a dent.
    normal: normalsFrom(height, size, 1.0),
  };
}

/**
 * **Spun concrete**, twenty years in a field.
 *
 * A centrifuged pole is dense and smooth on the outside — the aggregate is
 * thrown to the wall by the spinning, so what shows is not gravel but a fine
 * even skin over it, with the odd stone close enough to read and the blow-holes
 * the mould leaves. Down the pole run the faint long lines of the mould seam and,
 * over everything, the slow grey-green of a surface that is wet more often than
 * it is dry.
 *
 * **What it is not is sandpaper**, and that is how it looked: the sand was
 * drawn at 120 cycles a metre over three octaves, so the top octave was half a
 * texel wide and the map filled up with the aliasing of it. The aggregate now
 * sits at 16 cycles over three — 62 mm down to 8 mm, every step of it wider
 * than four texels — and the surface reads as concrete because the things on
 * it are the size concrete's things are.
 */
function concrete(size) {
  const height = new Float32Array(size * size);
  const shade = new Float32Array(size * size);
  const damp = new Float32Array(size * size);
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const u = x / size;
      const v = y / size;
      // The slow mottle of a surface that dries unevenly — the largest thing on
      // the pole and the only one still there at fifty metres.
      const weather = fbm(u, v, 2, 2, 131, 4);
      // The mould's seam and the drips down it: narrow across, long along.
      const mould = fbm(u, v, 20, 2, 97, 2);
      // Aggregate showing through the skin, and the blow-holes beside it.
      const stone = fbm(u, v, 16, 16, 71, 3);
      const pore = Math.max(0, fbm(u, v, 24, 24, 5, 2) - 0.62) * 2.6;
      const i = y * size + x;
      height[i] = stone * 0.30 + mould * 0.12 - pore * 0.55;
      damp[i] = clamp01(weather * 1.2 - 0.1);
      shade[i] =
        0.63 + (stone - 0.5) * 0.09 + (mould - 0.5) * 0.05 - damp[i] * 0.12 - pore * 0.10;
    }
  }
  return {
    colour: colour(size, (x, y) => {
      const i = y * size + x;
      const s = shade[i];
      // Cement is warm and the damp on it is not: a dry pole is a light
      // buff-grey, a wet one goes green-grey, and the two together are what
      // stops a concrete pole from reading as a painted tube.
      const d = damp[i];
      return [s * (1.05 - d * 0.07), s * (1.0 - d * 0.01), s * (0.93 + d * 0.05)];
    }),
    orm: orm(size, (x, y) => {
      const i = y * size + x;
      return {
        // The blow-holes are the only shadow the surface has of its own.
        occlusion: 1 - clamp01(-height[i] * 0.8),
        roughness: 0.86 + (hash(x, y, 3) - 0.5) * 0.04 + damp[i] * 0.04,
        metallic: 0,
      };
    }),
    normal: normalsFrom(height, size, 1.0),
  };
}

/**
 * **Creosoted pine**, the low-voltage pole of a village street.
 *
 * A pole is a debarked trunk, so what shows is the grain running its whole
 * length — `v` in this pipeline, because a tube's `u` goes round it — the
 * fibre along it, the **checks** it dries into, and the **knots** where the
 * branches were. Creosote makes it near black in the grooves and a dull, dark,
 * hardly-brown on the raised grain; a pole that reads warm pine has not been
 * treated, and the ones beside a railway all have.
 *
 * The knots are new and they are what the material was missing: they are the
 * one feature of a pole big enough (a hand's width) to still be there at
 * twenty metres, and without them the fibre — drawn far too fine, so the first
 * mip average flattened it — left nothing at all and the pole was a smooth
 * stick.
 */
function wood(size) {
  const height = new Float32Array(size * size);
  const shade = new Float32Array(size * size);
  const dark = new Float32Array(size * size);
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const u = x / size;
      const v = y / size;
      // Growth rings drawn out along the pole: a lathe cuts them into bands
      // that run its whole length, wobbling as the trunk did.
      //
      // **Sharpened, and this is what the material was missing.** Value noise
      // interpolates, so everything built out of it is blobs with no edges, and
      // a cosine is smooth by definition — the pole came out as a soft brown
      // gradient that read as blurred however many texels it had. Wood is the
      // opposite: late wood meets early wood at a hard line. The power curve
      // widens the pale band and keeps the dark edge crisp, which is what a
      // lathe leaves.
      const wobble = fbm(u, v, 2, 3, 17, 3) * 0.4;
      const band = 0.5 - 0.5 * Math.cos(2 * Math.PI * (u * 14 + wobble));
      const ring = Math.pow(band, 0.45);
      // The fibre along the grain, **ridged** so it makes lines rather than
      // clouds: the fold at the middle of the noise is a crease, and raising it
      // to a power narrows the crease into a fibre.
      const noiseF = fbm(u, v, 24, 3, 53, 3);
      const fibre = Math.pow(1 - Math.abs(2 * noiseF - 1), 2.2);
      // Checks: the long splits a pole dries into, narrow across and running
      // most of its length.
      const check = Math.max(0, fbm(u, v, 10, 1, 211, 2) - 0.66) * 3.0;
      // Knots: where a branch was, a hand's width across and darker than
      // anything else on the pole.
      const knot = Math.max(0, fbm(u, v, 5, 2, 311, 2) - 0.66) * 3.2;
      const i = y * size + x;
      height[i] = fibre * 0.16 + ring * 0.10 - check * 0.85 - knot * 0.25;
      dark[i] = clamp01(check * 0.8 + knot * 0.9);
      // The rings carry more than twice what they did. At seven metres a pole
      // is forty pixels wide and the third mip level is what gets sampled, so
      // the only thing left to see is the fourteen bands round it — and at four
      // and a half per cent of the shade there was nothing left to see.
      shade[i] = 0.29 + ring * 0.10 + fibre * 0.05 - dark[i] * 0.14;
    }
  }
  return {
    colour: colour(size, (x, y) => {
      const i = y * size + x;
      const s = shade[i];
      // Creosote: dark, warm and *desaturated*. The first cut multiplied a
      // brighter shade by (1, 0.78, 0.58), which is the orange of fresh sawn
      // pine — a fifty-year-old pole beside a railway is nearly black.
      const d = dark[i];
      return [s * (1.0 - d * 0.05), s * (0.84 - d * 0.04), s * (0.71 - d * 0.06)];
    }),
    orm: orm(size, (x, y) => {
      const i = y * size + x;
      return {
        occlusion: 1 - clamp01(-height[i] * 0.7),
        // Tar is not gloss: a treated pole is matte all over, a shade less so
        // where the weather has washed the creosote out of the raised grain.
        roughness: 0.88 - clamp01(height[i]) * 0.06 + dark[i] * 0.04,
        metallic: 0,
      };
    }),
    normal: normalsFrom(height, size, 1.6),
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
