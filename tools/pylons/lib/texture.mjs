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
 * How many residual zinc crystals fit across a metre.
 *
 * An in-service mast no longer presents the high-contrast flowers of a fresh
 * bath: weathering turns the coating into a soft, fairly uniform matte grey.
 * The 10 mm cells below survive only as a very low-contrast roughness change.
 * They must disappear from the colour within a few metres and never alter the
 * normal. The old 24 mm, high-contrast cells were the polygonal camouflage in
 * close photographs of the model.
 */
const SPANGLE_CELLS = 96;

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
      // Keep the same physical slope when a map's resolution changes. The
      // unscaled finite difference halves when 512 becomes 1024 even though
      // the represented one-metre surface did not become flatter.
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
 * A metre of it holds thousands of zinc crystals, but after years outdoors
 * their visible contrast is almost gone beneath a zinc-carbonate patina. A
 * slow mottle of weathering remains; individual crystals only whisper in the
 * roughness at inspection distance.
 *
 * **Weathered zinc is not a mirror.** Fresh galvanising is a metal with a
 * reflectance near 0.85, and `metallic` near one over a bright base colour
 * gives a mast that reflects the whole sky — under an image-based light off
 * Bevy's atmosphere that is not a subtle error, it is a blue-white mast that
 * blows out against the very sky it is mirroring. What a mast on a line
 * actually wears is zinc *carbonate*, the chalky grey skin of its first years,
 * and that is a **dielectric** over the metal. So the weathering field drives
 * the metallic, and it drives it into a narrow band close to dielectric. What
 * is left is a broad dim sheen rather than a white reflection of the sky.
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
  const brown = new Float32Array(size * size);
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const u = x / size;
      const v = y / size;
      const cell = spangle(u, v, SPANGLE_CELLS, 7);
      const facet = hash(cell.id, 3, 41);
      // Zero on a narrow crystal boundary, one in the facet. This only reaches
      // colour and roughness: crystal boundaries have no macroscopic relief.
      const edge = clamp01(cell.edge * SPANGLE_CELLS * 5.5);
      // Weathering: a slow field that decides which facets have gone dull.
      const weather = fbm(u, v, 3, 3, 13, 4);
      // The mill's rolling marks run along the bar (`v`). They are fine enough
      // not to read as bands but still four texels wide at the top octave.
      const grain = fbm(u, v, 12, 112, 29, 2);
      // The waviness a coating freezes into as it drains off the section.
      const peel = fbm(u, v, 40, 40, 83, 3);
      // Pitting: where the zinc has gone and the steel underneath is working.
      const pit = Math.max(0, fbm(u, v, 56, 56, 61, 3) - 0.68) * 2.8;
      const i = y * size + x;
      height[i] = peel * 0.12 + grain * 0.055 - pit * 0.18;
      // How far this spot has gone over to the chalky carbonate skin.
      const dull = clamp01(weather * 1.10 + pit * 0.35 - 0.06);
      // The simulator's open-sky lighting lifts a neutral mid-grey strongly.
      // Keep the stored surface in the sRGB 82–101 band so the lit mast still
      // separates from a bright sky instead of becoming nearly white.
      shade[i] = 0.398 - dull * 0.074 + (facet - 0.5) * 0.014 - (1 - edge) * 0.007;
      // Patina is broad and matte; remnants of younger zinc retain a dim,
      // rough metal response. Neither end behaves like polished aluminium.
      gloss[i] = 0.67 + dull * 0.17 + (facet - 0.5) * 0.040 + (1 - edge) * 0.018;
      metal[i] = 0.20 - dull * 0.145;
      chalk[i] = dull;
      // Decades-old galvanising develops occasional light-brown fields without
      // exposing red structural steel. Keep it sparse and low in saturation.
      brown[i] = clamp01((weather - 0.69) * 2.2 + pit * 0.30);
    }
  }
  return {
    colour: colour(size, (x, y) => {
      const i = y * size + x;
      const s = shade[i];
      const d = chalk[i];
      const b = brown[i];
      // Bare zinc is slightly cool. The patina neutralises it rather than
      // turning it beige; only old, damaged fields acquire a restrained warm
      // cast, while stronger soil colour still comes from vertex dirt.
      return [s * (0.965 + d * 0.020 + b * 0.080), s * (0.985 - b * 0.025), s * (1.02 - d * 0.010 - b * 0.105)];
    }),
    orm: orm(size, (x, y) => ({
      occlusion: 1,
      roughness: gloss[y * size + x],
      metallic: metal[y * size + x],
    })),
    // Gently: the height field is under a millimetre from end to end, and a
    // strength that suited an embossed spangle turns a millimetre into a dent.
    normal: normalsFrom(height, size, 0.72),
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
 * it are the size concrete's things are. The first correction still left the
 * aggregate as broad six-centimetre clouds; this pass moves the visible skin
 * into the 5–25 mm range and reserves large scales for dampness only.
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
      const mould = fbm(u, v, 28, 3, 97, 2);
      // Aggregate showing through the skin, and the blow-holes beside it.
      const stone = fbm(u, v, 40, 40, 71, 3);
      const fines = fbm(u, v, 96, 96, 173, 2);
      const pore = Math.max(0, fbm(u, v, 64, 64, 5, 2) - 0.66) * 2.9;
      const i = y * size + x;
      height[i] = stone * 0.17 + fines * 0.055 + mould * 0.07 - pore * 0.34;
      damp[i] = clamp01(weather * 1.2 - 0.1);
      shade[i] =
        0.625 + (stone - 0.5) * 0.055 + (fines - 0.5) * 0.018 +
        (mould - 0.5) * 0.035 - damp[i] * 0.105 - pore * 0.07;
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
    normal: normalsFrom(height, size, 0.72),
  };
}

/** A few explicit, seamless knots; random noise almost never makes a knot. */
function woodKnots(u, v, seed) {
  const cellsU = 6;
  const cellsV = 4;
  const ix = Math.floor(u * cellsU);
  const iy = Math.floor(v * cellsV);
  let core = 0;
  let rim = 0;
  for (let j = -1; j <= 1; j++) {
    for (let i = -1; i <= 1; i++) {
      const gx = ix + i;
      const gy = iy + j;
      const wx = ((gx % cellsU) + cellsU) % cellsU;
      const wy = ((gy % cellsV) + cellsV) % cellsV;
      if (hash(wx, wy, seed) < 0.72) continue;
      const cx = (gx + 0.18 + hash(wx, wy, seed + 11) * 0.64) / cellsU;
      const cy = (gy + 0.18 + hash(wx, wy, seed + 29) * 0.64) / cellsV;
      const ru = 0.026 + hash(wx, wy, seed + 47) * 0.025;
      const rv = 0.040 + hash(wx, wy, seed + 71) * 0.040;
      const du = (u - cx) / ru;
      const dv = (v - cy) / rv;
      const angle = Math.atan2(dv, du);
      const phase = hash(wx, wy, seed + 101) * Math.PI * 2;
      // A branch scar follows torn fibres; it is approximately elliptical but
      // never the perfect stamped oval that a plain radial falloff produces.
      const torn =
        1 + Math.sin(angle * 3 + phase) * 0.11 + Math.sin(angle * 7 - phase * 0.7) * 0.055;
      const d = Math.hypot(du, dv) * torn;
      core = Math.max(core, clamp01(1 - d));
      rim = Math.max(rim, Math.exp(-Math.pow((d - 0.82) / 0.18, 2)));
    }
  }
  return { core, rim };
}

/** Sparse, tapered drying checks instead of a dark noise cloud. */
function woodChecks(u, v, seed) {
  const cellsU = 11;
  const cellsV = 2;
  const ix = Math.floor(u * cellsU);
  const iy = Math.floor(v * cellsV);
  let cleft = 0;
  let lip = 0;
  for (let j = -1; j <= 1; j++) {
    for (let i = -1; i <= 1; i++) {
      const gx = ix + i;
      const gy = iy + j;
      const wx = ((gx % cellsU) + cellsU) % cellsU;
      const wy = ((gy % cellsV) + cellsV) % cellsV;
      if (hash(wx, wy, seed) < 0.78) continue;
      const cx = (gx + 0.15 + hash(wx, wy, seed + 13) * 0.70) / cellsU;
      const cy = (gy + 0.18 + hash(wx, wy, seed + 31) * 0.64) / cellsV;
      const halfLength = 0.14 + hash(wx, wy, seed + 47) * 0.18;
      const halfWidth = 0.0035 + hash(wx, wy, seed + 59) * 0.0065;
      const along = (v - cy) / halfLength;
      if (Math.abs(along) >= 1) continue;
      const phase = hash(wx, wy, seed + 79) * Math.PI * 2;
      // A check follows a fibre but never a ruler: it wanders by a few
      // millimetres, forks in the middle, and tapers away at both ends.
      const wander =
        Math.sin(along * Math.PI * 1.7 + phase) * halfWidth * 0.55 +
        Math.sin(along * Math.PI * 4.1 - phase * 0.6) * halfWidth * 0.22;
      const across = Math.abs((u - cx - wander) / halfWidth);
      const taper = Math.pow(1 - along * along, 0.65);
      const centre = Math.exp(-across * across * 2.6) * taper;
      const edge = Math.exp(-Math.pow((across - 1.35) / 0.52, 2)) * taper;
      cleft = Math.max(cleft, centre);
      lip = Math.max(lip, edge);
    }
  }
  return { cleft, lip };
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
  const wash = new Float32Array(size * size);
  const ridge = new Float32Array(size * size);
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const u = x / size;
      const v = y / size;
      // Two bands of grain. The medium one carries the material through the
      // mip chain; the fine one prevents a close pole or crossarm from looking
      // as though its colour was stretched over ten-pixel-wide stripes. Their
      // top octaves are 112 and 224 cycles/m respectively, both inside the
      // four-texel frequency budget of a 1024 map.
      const mediumNoise = fbm(u, v, 28, 4, 17, 3);
      const fineNoise = fbm(u, v, 112, 7, 137, 2);
      const medium = Math.pow(1 - Math.abs(2 * mediumNoise - 1), 3.6);
      const fine = Math.pow(1 - Math.abs(2 * fineNoise - 1), 2.4);
      const fibre = medium * 0.58 + fine * 0.42;
      // Slow stain and vertical wash marks belong to the treatment, not to the
      // wood grain. Keeping them separate lets dark creosote be a little less
      // rough while the sun-bleached raised fibres go grey and chalky.
      const stain = fbm(u, v, 3, 2, 229, 4);
      const drip = fbm(u, v, 13, 2, 251, 3);
      const treatment = clamp01(stain * 0.72 + drip * 0.28);
      const check = woodChecks(u, v, 211);
      // Noise made broad dark clouds but essentially no actual knots. Place a
      // handful of 5–10 cm elliptical branch scars, with a dark core and rim.
      const knot = woodKnots(u, v, 311);
      const i = y * size + x;
      ridge[i] = fibre;
      wash[i] = treatment;
      height[i] =
        (medium - 0.5) * 0.11 + (fine - 0.5) * 0.060 -
        check.cleft * 0.72 + check.lip * 0.13 - knot.core * 0.34 + knot.rim * 0.12;
      dark[i] = clamp01(check.cleft * 0.92 + knot.core + knot.rim * 0.34);
      // A narrow 45–90 sRGB range: enough local contrast to survive filtering,
      // without turning the pole into fresh orange construction timber.
      shade[i] =
        0.255 + (medium - 0.5) * 0.075 + (fine - 0.5) * 0.055 +
        (treatment - 0.5) * 0.060 - dark[i] * 0.145;
    }
  }
  return {
    colour: colour(size, (x, y) => {
      const i = y * size + x;
      const s = shade[i];
      const d = dark[i];
      const bleached = 1 - wash[i];
      // Old creosote is a neutral charcoal-brown in its wet streaks and a
      // slightly warmer, greyer brown where weather has lifted it from the
      // raised fibres. Reducing saturation as it bleaches avoids the former
      // pink/orange cast under a bright sky.
      return [
        s * (0.98 - d * 0.05),
        s * (0.87 + bleached * 0.025 - d * 0.04),
        s * (0.76 + bleached * 0.045 - d * 0.05),
      ];
    }),
    orm: orm(size, (x, y) => {
      const i = y * size + x;
      return {
        occlusion: 1 - clamp01(-height[i] * 0.7),
        // Intact treatment keeps a very broad, dim sheen; exposed fibres and
        // the lips of cracks are dry. Nothing approaches polished timber.
        roughness:
          0.90 - wash[i] * 0.10 - ridge[i] * 0.025 + dark[i] * 0.055,
        metallic: 0,
      };
    }),
    normal: normalsFrom(height, size, 1.22),
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
export function paint(structure, size = 1024) {
  const painter = PAINTERS[structure];
  if (!painter) throw new Error(`no painter for ${structure}`);
  return { id: painter.id, ...painter.paint(size) };
}
