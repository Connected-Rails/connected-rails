// Foliage cards: the cut-out sheet every leaf quad of a tree samples.
//
// ez-tree puts one textured quad at the end of each twig. Drawing a *single*
// leaf on that quad would need one quad per leaf, and a beech carries a
// hundred thousand — so the card holds a whole spray: a shoot with five to
// nine leaves, or a fascicle of needles. Ten times fewer quads for the same
// canopy, which is where the triangle budget of a forest is won.
//
// The quad's own frame decides the layout: ez-tree emits `uv (0,0)` at the
// attachment point and `uv (0,1)` at the far end, and glTF puts `v = 0` on the
// image's top row. The shoot therefore enters the card at the top edge and the
// leaves hang downwards from it.
//
// Shapes are botanical, not decorative: an oak is lobed, an ash pinnate, a
// horse chestnut palmate, a willow lanceolate, a pine two long needles in a
// fascicle, a spruce a flat spray of short ones. That is what tells the
// species apart in a wood, far more than the crown outline does.

import { Rng, fbm, seedOf } from './rng.mjs';
import { Surface, mix, rgb } from './raster.mjs';
import { stamp } from './leaves.mjs';

/**
 * Paints one foliage card.
 *
 * @param {number} size edge length [texels]
 * @param {object} spec the catalogue's `foliage` block
 * @param {'summer'|'autumn'|'winter'} season
 * @param {string} seedText
 * @param {?object} scan a resolved `lib/leaves.mjs` scan — the cut-out leaves
 *   this species is built from. `null` falls back to the painted blades, which
 *   is what a species with no matching sheet still gets.
 * @returns {Surface}
 */
export function foliageCard(size, spec, season, seedText, scan = null) {
  const seed = seedOf(`${seedText}/${season}`);
  const rng = new Rng(seed);
  const surface = new Surface(size, size);

  const palette = paletteFor(spec, season);
  const shape = spec.shape ?? 'ovate';
  const twigColor = rgb(spec.twig ?? '#5b4630');

  // A scan of whole sprays — a fir twig, a rowan's compound leaf — is already
  // the arrangement, so it is stamped as it is; anything else is composed out
  // of single leaves the way the painted cards are.
  if (scan?.whole) {
    sprays(surface, size, spec, palette, twigColor, rng, scan);
  } else if (shape.startsWith('needle') || shape === 'scale') {
    conifer(surface, size, shape, spec, palette, twigColor, rng, seed);
  } else if (shape === 'pinnate' || shape === 'palmate-compound') {
    compound(surface, size, shape, spec, palette, twigColor, rng, seed, scan);
  } else {
    simple(surface, size, shape, spec, palette, twigColor, rng, seed, scan);
  }

  if (season === 'winter' && spec.snow !== false) {
    snow(surface, size, seed);
  }
  // A cut-out has to carry colour into its transparent texels, or the mip
  // chain pulls the canopy towards black as soon as it is a few pixels wide.
  surface.dilateAlpha(10);
  return surface;
}

/** The two ends of the season's colour ramp plus the vein tint. */
function paletteFor(spec, season) {
  const summer = {
    base: rgb(spec.base ?? '#4b7a2c'),
    tip: rgb(spec.tip ?? '#6d9a3a'),
    vein: rgb(spec.vein ?? '#93b45c'),
    spread: spec.spread ?? 0.12,
  };
  if (season === 'autumn') {
    return {
      base: rgb(spec.autumnBase ?? '#a5731f'),
      tip: rgb(spec.autumnTip ?? '#c8a03a'),
      vein: rgb(spec.autumnVein ?? '#8a5a1c'),
      // Autumn is never uniform — half the leaf variance is what sells it.
      spread: (spec.autumnSpread ?? 0.34),
      mottle: mix(summer.base, [0.35, 0.22, 0.10], 0.5),
    };
  }
  if (season === 'winter') {
    // Only evergreens keep a card in winter; they go duller and bluer.
    return {
      base: mix(summer.base, [0.16, 0.26, 0.24], 0.35),
      tip: mix(summer.tip, [0.20, 0.30, 0.28], 0.35),
      vein: mix(summer.vein, [0.25, 0.34, 0.32], 0.35),
      spread: summer.spread * 0.6,
      // A scanned needle already has its colour; winter only takes some of the
      // life out of it.
      scanTint: [0.82, 0.88, 0.94],
    };
  }
  return summer;
}

// ---------------------------------------------------------------------------
// Outlines. Every shape is generated in a local frame: the base sits at the
// origin, the tip at (0, 1), the width is `w` at its broadest.

/** Half width of a blade at `t` (0 = base, 1 = tip). */
function bladeWidth(t, w, broad, sharp) {
  if (t <= 0 || t >= 1) return 0;
  // Bell curve whose peak sits at `broad`; `sharp` draws the tip out.
  const p = Math.log(0.5) / Math.log(broad);
  const s = Math.sin(Math.PI * Math.pow(t, p));
  return w * Math.pow(s, sharp);
}

/**
 * One blade as a closed polygon of `steps * 2` points.
 * `edge(t)` may add teeth or lobes as a multiplier on the half width.
 */
function blade(w, broad, sharp, edge, steps = 26) {
  const right = [];
  const left = [];
  for (let i = 0; i <= steps; i++) {
    const t = i / steps;
    const hw = bladeWidth(t, w, broad, sharp) * (edge ? edge(t) : 1);
    right.push([hw, t]);
    left.push([-hw, t]);
  }
  return right.concat(left.reverse());
}

const OUTLINES = {
  ovate: (w) => blade(w, 0.42, 0.75, (t) => 1 + Math.sin(t * Math.PI * 14) * 0.05),
  // Beech and hornbeam: an even blade with a fine wavy edge.
  elliptic: (w) => blade(w, 0.5, 0.8, (t) => 1 + Math.sin(t * Math.PI * 18) * 0.035),
  // Willow: long and narrow.
  lanceolate: (w) => blade(w * 0.62, 0.32, 0.55, (t) => 1 + Math.sin(t * Math.PI * 22) * 0.03),
  // Lime: heart-shaped, widest close to the base.
  cordate: (w) => blade(w * 1.05, 0.3, 0.7, (t) => 1 + Math.sin(t * Math.PI * 16) * 0.06),
  // Poplar: a triangle with a drawn-out point.
  deltoid: (w) => blade(w * 1.1, 0.22, 0.85, (t) => 1 + Math.sin(t * Math.PI * 12) * 0.05),
  // Oak: five or six rounded lobes a side, the sinuses cutting deep.
  lobed: (w) =>
    blade(w * 1.05, 0.55, 0.6, (t) => 0.62 + 0.38 * Math.abs(Math.cos(Math.PI * t * 5.5)) ** 0.6, 46),
  // Alder and hazel: broad, blunt, the tip almost cut off.
  obovate: (w) => blade(w, 0.62, 0.55, (t) => 1 + Math.sin(t * Math.PI * 15) * 0.05),
  // Birch: a small triangle with a sharp double-toothed edge.
  rhombic: (w) => blade(w * 0.92, 0.3, 0.8, (t) => 1 + Math.sin(t * Math.PI * 20) * 0.09),
  // Maple and sycamore: five pointed lobes cut deep into the blade.
  maple: (w) => palmate(w, 5, 0.55),
};

/** Maple: a palm of pointed lobes, drawn as one closed outline. */
function palmate(w, lobes, depth) {
  const points = [];
  const steps = 240;
  for (let i = 0; i <= steps; i++) {
    const a = (i / steps) * Math.PI * 2;
    // Angle measured from the tip direction (+Y), symmetric left and right.
    const from = Math.atan2(Math.sin(a), Math.cos(a));
    const petal = Math.abs(Math.cos((from - Math.PI / 2) * lobes / 2));
    const r = (1 - depth) + depth * Math.pow(petal, 0.55);
    // The leaf is taller than wide and hangs below its stalk.
    const x = Math.cos(a) * r * w * 1.35;
    const y = 0.5 + Math.sin(a) * r * 0.5;
    if (y < 0.02) continue;
    points.push([x, y]);
  }
  return points;
}

// ---------------------------------------------------------------------------
// Cards.

/**
 * A branched spray of simple leaves. One shoot alone would fill a strip down
 * the middle of a square card and leave the corners empty, so the spray forks:
 * every five leaves get a shoot of their own, angled off the first.
 */
function simple(surface, size, shape, spec, palette, twigColor, rng, seed, scan = null) {
  // A scanned leaf carries its own gaps, so a card built from them needs more
  // of them than a painted one to cover the same quad.
  const count = scan ? Math.round((spec.count ?? 10) * (spec.scanCount ?? 1.7)) : spec.count ?? 10;
  const main = shoot(size, rng, 0.88);
  const shoots = [{ path: main, share: 1 }];
  const forks = Math.min(4, Math.floor(count / 4));
  for (let f = 0; f < forks; f++) {
    const from = along(main, 0.22 + f * 0.26);
    const side = f % 2 === 0 ? 1 : -1;
    const a = side * (0.55 + rng.next() * 0.3);
    const l = size * (0.5 - f * 0.08);
    shoots.push({
      path: [from, [from[0] + Math.sin(a) * l, from[1] + Math.cos(a) * l]],
      share: 1,
    });
  }
  const twigWidth = scan ? 0.007 : 0.013;
  for (const s of shoots) {
    strokeTwig(surface, s.path, size * (s.path === main ? twigWidth : twigWidth * 0.6), twigColor);
  }

  const outline = OUTLINES[shape] ?? OUTLINES.ovate;
  for (let i = 0; i < count; i++) {
    const s = shoots[i % shoots.length];
    const k = Math.floor(i / shoots.length);
    const t = 0.12 + ((k + (i % 2) * 0.5) / Math.max(1, count / shoots.length)) * 0.8;
    const side = i % 2 === 0 ? 1 : -1;
    const at = along(s.path, Math.min(0.95, t));
    const length = size * (spec.length ?? 0.34) * (1 - t * 0.28) * rng.jitter(0.18);
    const width = (spec.width ?? 0.44) * rng.jitter(0.12);
    const angle = (side * (spec.angle ?? 55) + (rng.next() - 0.5) * 30) * (Math.PI / 180);
    if (scan) {
      // A scanned leaf keeps the size the shoot gives it; blowing it up piles
      // the card into one green lump instead of a spray of separate leaves.
      stampLeaf(surface, scan, palette, rng, { at, angle, length });
    } else {
      drawBlade(surface, outline(width), at, angle, length, palette, rng, seed, spec);
    }
    // The leaf stalk.
    const stalk = length * (spec.petiole ?? 0.12);
    if (stalk > 1) {
      surface.stroke(
        [at, [at[0] + Math.sin(angle) * stalk, at[1] + Math.cos(angle) * stalk]],
        size * 0.008,
        () => twigColor,
      );
    }
  }
}

/** Ash, rowan, robinia, elder, horse chestnut: two or three compound leaves. */
function compound(surface, size, shape, spec, palette, twigColor, rng, seed, scan = null) {
  const leaves = spec.count ?? 3;
  const stem = shoot(size, rng, 0.5);
  strokeTwig(surface, stem, size * 0.014, twigColor);
  for (let i = 0; i < leaves; i++) {
    const t = 0.15 + (i / Math.max(1, leaves - 1)) * 0.7;
    const at = along(stem, t);
    const side = i % 2 === 0 ? 1 : -1;
    const angle = (side * (spec.angle ?? 40) + (rng.next() - 0.5) * 20) * (Math.PI / 180);
    const length = size * (spec.length ?? 0.6) * rng.jitter(0.12);
    if (scan) {
      // Single leaflets off a sheet: the rachis is still the pipeline's, only
      // the blade on it comes out of the scan.
      if (shape === 'pinnate') {
        pinnateLeaf(surface, at, angle, length, spec, palette, twigColor, rng, seed, scan);
      } else {
        palmateLeaf(surface, at, angle, length, spec, palette, twigColor, rng, seed, scan);
      }
    } else if (shape === 'pinnate') {
      pinnateLeaf(surface, at, angle, length, spec, palette, twigColor, rng, seed);
    } else {
      palmateLeaf(surface, at, angle, length, spec, palette, twigColor, rng, seed);
    }
  }
}

/** A rachis with paired leaflets and a terminal one. */
function pinnateLeaf(surface, at, angle, length, spec, palette, twigColor, rng, seed, scan = null) {
  const pairs = spec.leaflets ?? 4;
  const dir = [Math.sin(angle), Math.cos(angle)];
  const tip = [at[0] + dir[0] * length, at[1] + dir[1] * length];
  surface.stroke([at, tip], Math.max(1, length * 0.018), () => twigColor);
  const w = spec.leafletWidth ?? 0.34;
  for (let i = 0; i < pairs; i++) {
    const t = 0.22 + (i / pairs) * 0.72;
    const node = [at[0] + dir[0] * length * t, at[1] + dir[1] * length * t];
    for (const side of [1, -1]) {
      const a = angle + side * (spec.leafletAngle ?? 62) * (Math.PI / 180) + (rng.next() - 0.5) * 0.25;
      const l = length * (spec.leafletLength ?? 0.36) * rng.jitter(0.14);
      if (scan) stampLeaf(surface, scan, palette, rng, { at: node, angle: a, length: l });
      else drawBlade(surface, OUTLINES.lanceolate(w), node, a, l, palette, rng, seed, spec);
    }
  }
  const terminal = {
    at: tip,
    angle: angle + (rng.next() - 0.5) * 0.2,
    length: length * (spec.leafletLength ?? 0.36),
  };
  if (scan) stampLeaf(surface, scan, palette, rng, terminal);
  else {
    drawBlade(surface, OUTLINES.lanceolate(w), terminal.at, terminal.angle, terminal.length, palette, rng, seed, spec);
  }
}

/** Leaflets radiating from one point — horse chestnut. */
function palmateLeaf(surface, at, angle, length, spec, palette, twigColor, rng, seed, scan = null) {
  const n = spec.leaflets ?? 5;
  const spread = (spec.leafletSpread ?? 105) * (Math.PI / 180);
  for (let i = 0; i < n; i++) {
    const a = angle - spread / 2 + (i / (n - 1)) * spread + (rng.next() - 0.5) * 0.12;
    // The middle leaflets of a chestnut are the longest.
    const centre = 1 - Math.abs(i - (n - 1) / 2) / ((n - 1) / 2);
    const l = length * (0.62 + centre * 0.38) * rng.jitter(0.1);
    if (scan) stampLeaf(surface, scan, palette, rng, { at, angle: a, length: l });
    else drawBlade(surface, OUTLINES.obovate(spec.leafletWidth ?? 0.4), at, a, l, palette, rng, seed, spec);
  }
}

/** Conifers: fascicles, sprays, rosettes and scales. */
function conifer(surface, size, shape, spec, palette, twigColor, rng, seed) {
  const stem = shoot(size, rng, 0.9);
  strokeTwig(surface, stem, size * (shape === 'needle-pair' ? 0.016 : 0.011), twigColor);
  const needle = (from, angle, length, width) => {
    const dir = [Math.sin(angle), Math.cos(angle)];
    const nx = dir[1];
    const ny = -dir[0];
    const tipW = width * 0.25;
    surface.fill(
      [
        [from[0] + nx * width, from[1] + ny * width],
        [from[0] + dir[0] * length + nx * tipW, from[1] + dir[1] * length + ny * tipW],
        [from[0] + dir[0] * length - nx * tipW, from[1] + dir[1] * length - ny * tipW],
        [from[0] - nx * width, from[1] - ny * width],
      ],
      (x, y) => {
        const t = Math.min(1, Math.hypot(x - from[0], y - from[1]) / length);
        const c = mix(palette.base, palette.tip, t * 0.8 + fbm(x / size, y / size, 24, 2, seed) * 0.2);
        return c;
      },
    );
  };

  switch (shape) {
    case 'needle-pair': {
      // Scots pine: fascicles of two long needles, packed towards the end of
      // the shoot the way a pine carries them.
      const bundles = spec.count ?? 16;
      for (let i = 0; i < bundles; i++) {
        const t = 0.2 + (i / bundles) * 0.76;
        const at = along(stem, t);
        const side = i % 2 === 0 ? 1 : -1;
        const base = (side * (spec.angle ?? 46) + (rng.next() - 0.5) * 30) * (Math.PI / 180);
        const l = size * (spec.length ?? 0.4) * rng.jitter(0.2);
        for (const d of [-0.16, 0, 0.16]) {
          needle(at, base + d, l * rng.jitter(0.12), size * (spec.needleWidth ?? 0.011));
        }
      }
      break;
    }
    case 'needle-tuft': {
      // Larch: rosettes of twenty short needles on short spurs.
      const tufts = spec.count ?? 5;
      for (let i = 0; i < tufts; i++) {
        const t = 0.12 + (i / Math.max(1, tufts - 1)) * 0.78;
        const at = along(stem, t);
        const n = spec.needles ?? 18;
        for (let k = 0; k < n; k++) {
          const a = (k / n) * Math.PI * 1.5 - Math.PI * 0.75 + (rng.next() - 0.5) * 0.2;
          needle(at, a, size * (spec.length ?? 0.17) * rng.jitter(0.25), size * 0.007);
        }
      }
      break;
    }
    case 'scale': {
      // Juniper and thuja: overlapping scales along a fine shoot.
      const n = spec.count ?? 26;
      for (let i = 0; i < n; i++) {
        const t = 0.05 + (i / n) * 0.92;
        const at = along(stem, t);
        const side = i % 2 === 0 ? 1 : -1;
        const a = (side * 30 + (rng.next() - 0.5) * 16) * (Math.PI / 180);
        needle(at, a, size * (spec.length ?? 0.08) * rng.jitter(0.2), size * 0.014);
      }
      break;
    }
    case 'needle-spray':
    default: {
      // Spruce, fir, Douglas fir: short needles combed off a flat spray. Four
      // side shoots and a needle every few texels — a conifer card drawn as a
      // thin comb makes a crown you can see the sky through, which is what a
      // wood of them looks like from a train and is not what a spruce is.
      const shoots = [stem];
      for (let k = 0; k < 4; k++) {
        const from = along(stem, 0.22 + k * 0.18 + rng.next() * 0.08);
        const a = (k % 2 === 0 ? 1 : -1) * (0.55 + rng.next() * 0.3);
        const l = size * (0.46 - k * 0.06);
        shoots.push([from, [from[0] + Math.sin(a) * l, from[1] + Math.cos(a) * l]]);
      }
      for (const s of shoots) {
        if (s !== stem) strokeTwig(surface, s, size * 0.007, twigColor);
        const n = spec.count ?? 30;
        for (let i = 0; i < n; i++) {
          const t = 0.04 + (i / n) * 0.94;
          const at = along(s, t);
          const side = i % 2 === 0 ? 1 : -1;
          const a =
            Math.atan2(s[s.length - 1][0] - s[0][0], s[s.length - 1][1] - s[0][1]) +
            side * (spec.angle ?? 68) * (Math.PI / 180) +
            (rng.next() - 0.5) * 0.4;
          needle(
            at,
            a,
            size * (spec.length ?? 0.15) * rng.jitter(0.25),
            size * (spec.needleWidth ?? 0.013),
          );
        }
      }
      break;
    }
  }
}

/**
 * A shoot carrying whole scanned sprays — a fir twig, a rowan's compound leaf.
 * The sheet is already the arrangement, so the card only has to place a few of
 * them and let them overlap.
 */
function sprays(surface, size, spec, palette, twigColor, rng, scan) {
  const count = spec.sprays ?? 8;
  // No drawn shoot: a scanned spray is photographed with its own stem, and a
  // painted line under it reads as a second twig. The sprays hang off an
  // invisible shoot running down the card, alternating sides, which is how a
  // conifer branch carries them and what fills the quad top to bottom.
  const stem = shoot(size, rng, 0.86);
  for (let i = 0; i < count; i++) {
    const t = 0.04 + (i / Math.max(1, count - 1)) * 0.88;
    const at = along(stem, t);
    const side = i % 2 === 0 ? 1 : -1;
    const angle = (side * (spec.sprayAngle ?? 48) + (rng.next() - 0.5) * 30) * (Math.PI / 180);
    // The sprays shorten towards the tip of the shoot, as they do on a branch.
    const length = size * (spec.sprayLength ?? 0.6) * (1 - t * 0.3) * rng.jitter(0.2);
    stampLeaf(surface, scan, palette, rng, { at, angle, length });
  }
}

/**
 * Puts one cut-out leaf on the card. Which of the sheet's leaves it is comes
 * out of the card's own random stream, so two cards of a species differ; the
 * tint is the species' own correction on the scan plus a little per-leaf
 * brightness, which is what keeps six photographed leaves from reading as six
 * stamps of one.
 */
function stampLeaf(surface, scan, palette, rng, { at, angle, length }) {
  const image = scan.images[Math.floor(rng.next() * scan.images.length) % scan.images.length];
  const jitter = 1 + (rng.next() * 2 - 1) * (palette.spread ?? 0.12) * 0.7;
  const species = scan.tint ?? [1, 1, 1];
  const season = palette.scanTint ?? [1, 1, 1];
  stamp(surface, image, {
    at,
    angle,
    length,
    tint: [
      species[0] * season[0] * jitter,
      species[1] * season[1] * jitter,
      species[2] * season[2] * jitter,
    ],
    flip: scan.flip,
  });
}

// ---------------------------------------------------------------------------
// Helpers.

/** The shoot the card hangs from: enters at the top edge, curves a little. */
function shoot(size, rng, length) {
  const points = [];
  const bend = (rng.next() - 0.5) * 0.18;
  const steps = 8;
  for (let i = 0; i <= steps; i++) {
    const t = i / steps;
    points.push([size * (0.5 + bend * t * t), size * (0.02 + length * t)]);
  }
  return points;
}

function strokeTwig(surface, points, width, color) {
  surface.stroke(points, (t) => width * (1 - t * 0.5), () => color);
}

/** Point at parameter `t` along a polyline. */
function along(points, t) {
  const k = Math.min(points.length - 1, Math.max(0, t * (points.length - 1)));
  const i = Math.floor(k);
  const f = k - i;
  const a = points[i];
  const b = points[Math.min(points.length - 1, i + 1)];
  return [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f];
}

/**
 * Draws one blade: the outline placed at `at`, rotated by `angle` (0 = down
 * the card), scaled to `length`, filled with a base-to-tip ramp and veined.
 */
function drawBlade(surface, outline, at, angle, length, palette, rng, seed, spec) {
  const cos = Math.cos(angle);
  const sin = Math.sin(angle);
  // Local (x = across, y = along) into card coordinates, +y downwards.
  const place = ([x, y]) => [
    at[0] + (x * cos + y * sin) * length,
    at[1] + (-x * sin + y * cos) * length,
  ];
  const points = outline.map(place);

  // Per-leaf variation: hue, brightness, and in autumn whether this one has
  // turned at all.
  const shift = (rng.next() * 2 - 1) * palette.spread;
  const turned = palette.mottle ? rng.next() : 1;
  const shade = (x, y) => {
    // Where along the blade this texel sits, for the base-to-tip ramp.
    const dx = x - at[0];
    const dy = y - at[1];
    const t = Math.min(1, Math.max(0, (dx * sin + dy * cos) / length));
    let c = mix(palette.base, palette.tip, t * 0.75 + 0.15);
    if (palette.mottle && turned < 0.28) c = mix(c, palette.mottle, 0.75);
    const n = fbm(x / 256, y / 256, 18, 3, seed + 991);
    const k = 1 + shift + (n - 0.5) * 0.16;
    return [c[0] * k, c[1] * k, c[2] * k];
  };
  surface.fill(points, shade);

  // Midrib and a few side veins, a shade lighter than the blade.
  const veinColor = (x, y) => {
    const c = shade(x, y);
    return mix(c, palette.vein, 0.55);
  };
  const tip = place([0, 1]);
  surface.stroke([at, tip], Math.max(0.8, length * 0.014), veinColor);
  if (spec.veins !== false && length > 14) {
    const pairs = spec.veinPairs ?? 4;
    for (let i = 1; i <= pairs; i++) {
      const t = i / (pairs + 1);
      const node = place([0, t]);
      const w = 0;
      for (const side of [1, -1]) {
        const end = place([side * (0.42 * Math.sin(Math.PI * t) + w), t + 0.16]);
        surface.stroke([node, end], Math.max(0.6, length * 0.008), veinColor);
      }
    }
  }
}

/** A dusting of snow on the upper faces of a winter card. */
function snow(surface, size, seed) {
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const [r, g, b, a] = surface.at(x, y);
      if (a < 0.02) continue;
      // Snow settles where the card faces up — the top of every needle run.
      const above = surface.at(x, y - 3)[3];
      const n = fbm(x / size, y / size, 12, 3, seed + 7717);
      const amount = Math.max(0, 0.55 - above) * (0.5 + n * 0.9);
      if (amount <= 0) continue;
      const t = Math.min(0.85, amount * 1.4);
      surface.data[(y * size + x) * 4] = r + (0.92 - r) * t;
      surface.data[(y * size + x) * 4 + 1] = g + (0.94 - g) * t;
      surface.data[(y * size + x) * 4 + 2] = b + (0.98 - b) * t;
    }
  }
}

export { OUTLINES, palmate };
