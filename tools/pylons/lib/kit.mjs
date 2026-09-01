// The kit: the parts a mast is put together from, and the assembly that reads
// an atlas entry and returns one.
//
// Everything a German overhead line stands on is the same five pieces in
// different proportions — a body, some crossarms, a fitting per conductor, an
// earth peak and a foundation — so the kit has five builders and the atlas
// (`pylons.json`) has the numbers. A Donaumast and a Bahnstrommast differ in
// this file by nothing at all; they differ in the catalogue by a crossarm.
//
// Every builder takes a `detail` from 0 (finest) to 3 (coarsest) and answers
// with less geometry as it rises: bracing panels are halved, then dropped,
// round sections lose facets, fittings disappear. The level that survives to
// the end is the silhouette, because a mast at eight hundred metres is a line
// of grey pixels and nothing else.

import { empty, member, tube, box, merge, append, mirrorX, translate, weather } from './geom.mjs';

/**
 * Which material a part belongs to. Three at most, and the third exists for one
 * reason: a lattice mast stands on **concrete**, and a galvanised-steel
 * material on its foundation stubs is the sort of thing nobody notices until
 * they walk up to one, at which point the mast is standing on four metal
 * blocks.
 */
export const STRUCTURE = 0;
export const FITTINGS = 1;
export const CONCRETE = 2;

/**
 * The bracing density of each level: how many of the panels are drawn.
 *
 * **`LOD2` keeps a quarter of them, and that is not a rounding of zero.** A
 * lattice mast at a kilometre is not two legs and a crossarm — it is a grey
 * haze with structure in it, and the structure is the difference between a mast
 * and a pylon-shaped clothes prop. An earlier cut of this dropped the bracing
 * here, matched the *amount* of grey by making the legs thick, and produced a
 * bare A-frame: the right ink in the wrong place. Ink is necessary and not
 * sufficient — `--ink` says how much, the eye says where.
 *
 * The bracing goes at `LOD3`, which starts where the mast's own body is under
 * two pixels wide and there is genuinely nothing inside the outline left to
 * draw.
 */
const BRACE_SHARE = [1, 0.5, 0.25, 0];
/** Facets on a round section per level. */
const TUBE_SIDES = [10, 8, 6, 4];

/**
 * A steel member whose ends are **closed at the finest level**.
 *
 * `geom::member` leaves them open by default, and for the whole life of the kit
 * every one of the 276 pieces of a Donaumast was an open box. Nothing showed,
 * because the sides were wound inside out and the far one's interior stood in
 * for the near one's exterior — closing the winding opened every end at once,
 * and a joint became a bar you could see down.
 *
 * Only at `LOD0`: two quads an end is four triangles, and 276 members of them
 * is half the structure again. `LOD1` starts at 278 m on a Donaumast, where a
 * 15 cm bar is half a pixel and the hole in it a good deal less.
 */
function bar(out, a, b, wu, wv, detail) {
  // **And it runs a little past both ends.** A member is drawn from node to
  // node, so where two of them meet at an angle their square ends leave a wedge
  // between them — shallow at a right angle, deep at the twenty degrees a
  // diagonal makes with a chord. Riveted steel does not have those: the bars
  // lap and are bolted through. Running each one past its node by half its own
  // width buries the end in whatever it meets and costs no triangles; the cap
  // on 20 % of the length keeps a short stub from doubling.
  const d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
  const len = Math.hypot(d[0], d[1], d[2]);
  if (len < 1e-6) return out;
  const over = Math.min(Math.max(wu, wv) * 0.5, len * 0.2) / len;
  const from = [a[0] - d[0] * over, a[1] - d[1] * over, a[2] - d[2] * over];
  const to = [b[0] + d[0] * over, b[1] + d[1] * over, b[2] + d[2] * over];
  return member(out, from, to, wu, wv, detail === 0);
}

/**
 * How much thicker a member is drawn at each level.
 *
 * A coarse level is not the same mast with fewer triangles — it is a **paler**
 * mast, because the members it dropped were carrying ink. A lattice at six
 * hundred metres is a grey haze made of a hundred sub-pixel bars, and a level
 * that throws half of them away and draws the rest at their true width halves
 * the grey. The mast then visibly thickens as the train approaches it, which is
 * the one thing a level of detail must not do.
 *
 * So what is left is drawn thicker, by as much as it takes to put the coverage
 * back. The numbers are not guessed: `build_pylons.mjs --ink` supersamples each
 * level at the distance it hands over at and reports the ratio to the level
 * before it, and these are what brings every type inside a few per cent.
 *
 * It is a lie about the steel and an honest picture of the mast, which is the
 * right way round — nobody at six hundred metres can tell a 16 cm angle from a
 * 30 cm one, and everybody can tell a grey mast from a white one.
 */
const MEMBER_SCALE = [1.0, 1.22, 1.15, 2.0];

/**
 * The same for the crossarms, which lose their geometry on a different
 * schedule: the body drops half its bracing at `LOD1` and all of it at `LOD2`,
 * while an arm keeps both its chords until `LOD3` and only then falls to two
 * bars. One multiplier for both parts overshoots the body or starves the arms,
 * depending on which type it was fitted to — a Donaumast with a 24 m crossarm
 * and a medium-voltage lattice with a 2.6 m bracket are not the same problem.
 */
const ARM_SCALE = [1.0, 1.15, 1.7, 1.7];

const lerp = (a, b, t) => a + (b - a) * t;

/**
 * The four legs of a lattice body and the bracing between them.
 *
 * A German lattice mast is a square in plan: it stands on a wide base, the legs
 * batter inwards to a waist and run parallel from there to the top. The waist is
 * where the crossarms start, and the reason the shape exists — a mast is a
 * cantilever against the wind, so it needs its width at the bottom and nowhere
 * else.
 */
function latticeBody(out, { base, shaft, waist, top, panels, shaftPanels, leg, brace, detail }) {
  // Half-width of the square at height y.
  const half = (y) => (y >= waist ? shaft / 2 : lerp(base / 2, shaft / 2, y / waist) );
  const corners = (y) => {
    const h = half(y);
    return [
      [-h, y, -h],
      [h, y, -h],
      [h, y, h],
      [-h, y, h],
    ];
  };

  // The nodes the bracing meets the legs at: the battered part first, then the
  // parallel shaft. The coarse levels keep the *kink* at the waist and throw
  // the rest away — the batter is the silhouette, the panels are the detail.
  const levels = [];
  const n = Math.max(1, Math.round(panels / (detail + 1)));
  const m = Math.max(1, Math.round(shaftPanels / (detail + 1)));
  for (let i = 0; i <= n; i++) levels.push((i / n) * waist);
  for (let i = 1; i <= m; i++) levels.push(waist + (i / m) * (top - waist));

  // Legs, one member per panel so the batter is a polyline rather than a bend.
  for (let i = 0; i < levels.length - 1; i++) {
    const a = corners(levels[i]);
    const b = corners(levels[i + 1]);
    const width = lerp(leg, leg * 0.6, levels[i] / top);
    for (let c = 0; c < 4; c++) bar(out, a[c], b[c], width, width, detail);
  }

  const share = BRACE_SHARE[detail];
  if (share <= 0) return out;
  const step = share >= 1 ? 1 : Math.max(1, Math.round(1 / share));
  for (let i = 0; i < levels.length - 1; i += step) {
    const y0 = levels[i];
    const y1 = levels[Math.min(i + step, levels.length - 1)];
    if (y1 <= y0) break;
    const a = corners(y0);
    const b = corners(y1);
    for (let c = 0; c < 4; c++) {
      const d = (c + 1) % 4;
      // The X of the panel, plus the horizontal tie that closes its top. Real
      // masts alternate the diagonal's direction face to face; at the size a
      // panel covers on screen the X reads as both.
      bar(out, a[c], b[d], brace, brace, detail);
      bar(out, a[d], b[c], brace, brace, detail);
      bar(out, b[c], b[d], brace * 0.8, brace * 0.8, detail);
    }
  }
  return out;
}

/** A tapered pole — spun concrete, wood, or the tube of a compact mast. */
function poleBody(out, { baseDiameter, topDiameter, top, detail }) {
  tube(out, [0, 0, 0], [0, top, 0], baseDiameter / 2, topDiameter / 2, TUBE_SIDES[detail]);
  return out;
}

/**
 * One crossarm, from the body out to one tip, as two parallel trusses joined
 * across. Mirrored for the other side by the caller.
 *
 * The top chord is horizontal at the conductor level and the bottom chord runs
 * up to meet it at the tip, which is what makes the arm a triangle in
 * elevation — the shape that carries a downward load out on a cantilever.
 */
function crossarmHalf(out, { y, root, tip, depth, zRoot, zTip, size, detail }) {
  if (tip <= root + 0.05) return out;
  if (detail >= 3) {
    // The coarsest level is the arm's outline: one chord out, one back up.
    member(out, [root, y, 0], [tip, y, 0], size * 1.6, size * 1.6);
    member(out, [root, y - depth, 0], [tip, y, 0], size * 1.3, size * 1.3);
    return out;
  }
  const truss = (z0, z1) => {
    const topRoot = [root, y, z0];
    const topTip = [tip, y, z1];
    const botRoot = [root, y - depth, z0];
    bar(out, topRoot, topTip, size, size, detail);
    bar(out, botRoot, topTip, size * 0.85, size * 0.85, detail);
    bar(out, topRoot, botRoot, size * 0.7, size * 0.7, detail);
    if (BRACE_SHARE[detail] > 0) {
      // Web members between the two chords, spaced so a long 380 kV arm gets
      // four or five and a 20 kV bracket gets one.
      const bays = Math.max(1, Math.round((tip - root) / 3.5 / (detail + 1)));
      for (let i = 1; i < bays; i++) {
        const t = i / bays;
        const upper = [lerp(root, tip, t), y, lerp(z0, z1, t)];
        const lower = [lerp(root, tip, t), lerp(y - depth, y, t), lerp(z0, z1, t)];
        bar(out, upper, lower, size * 0.55, size * 0.55, detail);
        const back = [lerp(root, tip, (i - 1) / bays), y - depth * (1 - (i - 1) / bays), lerp(z0, z1, (i - 1) / bays)];
        bar(out, back, upper, size * 0.5, size * 0.5, detail);
      }
    }
  };
  if (detail >= 2) {
    // One truss on the mast's centre plane instead of two: half the members,
    // and from two hundred metres the pair was one line of pixels anyway.
    truss(0, 0);
    return out;
  }
  truss(-zRoot, -zTip);
  truss(zRoot, zTip);
  // The horizontal lattice that ties the two trusses into one arm.
  const bays = Math.max(1, Math.round((tip - root) / 4));
  for (let i = 0; i <= bays; i++) {
    const t = i / bays;
    const x = lerp(root, tip, t);
    const z = lerp(zRoot, zTip, t);
    bar(out, [x, y, -z], [x, y, z], size * 0.5, size * 0.5, detail);
  }
  return out;
}

/** A flat bracket instead of a truss — what a concrete or wooden pole carries. */
function bracket(out, { y, root, tip, size, zRoot, depth, detail }) {
  bar(out, [root, y, 0], [tip, y, 0], size, size * 1.4, detail);
  if (depth > 0.05) bar(out, [root, y - depth, 0], [tip * 0.75, y, 0], size * 0.7, size * 0.7, detail);
  void zRoot;
  return out;
}

/**
 * A suspension insulator string: the cap-and-pin chain the conductor hangs on,
 * vertical under the arm. Below about 200 m it is three or four pixels wide and
 * the discs are what makes it read as an insulator rather than a rope.
 */
function insulatorString(out, { at, length, discs, direction, detail }) {
  const [x, y, z] = at;
  const dir = direction ?? [0, -1, 0];
  const end = [x + dir[0] * length, y + dir[1] * length, z + dir[2] * length];
  const rod = Math.max(0.05, length * 0.03);
  bar(out, [x, y, z], end, rod, rod, detail);
  if (detail >= 2) return out;
  // The discs are the expensive part of a mast, not the steel: sixteen of them
  // per string, six strings on a Donaumast, and a capped ten-sided tube each
  // came to more triangles than the whole lattice. They are drawn as open
  // six-sided rings — a disc is a centimetre of screen at the distance the
  // fine level survives to, and its silhouette is all that is left of it.
  // **Every cap at the finest level.** The atlas counts them off the real
  // 146 mm unit, and a constant thinning even at LOD0 spread sixteen of them
  // over three and a half metres — a 21 cm pitch, half again what a cap is, so
  // each one had to be drawn fat to close the gap and the string read as a
  // screw thread. From LOD1 the thinning is steep, because a 15 cm disc is a
  // pixel at forty metres and gone at eighty.
  const n = Math.max(4, Math.round(discs / (detail * 2 + 1)));
  const r = Math.max(0.08, length * 0.045);
  for (let i = 0; i < n; i++) {
    const t = (i + 0.5) / n;
    const c = [x + dir[0] * length * t, y + dir[1] * length * t, z + dir[2] * length * t];
    const thickness = length / n / 3;
    tube(
      out,
      [c[0] - dir[0] * thickness, c[1] - dir[1] * thickness, c[2] - dir[2] * thickness],
      [c[0] + dir[0] * thickness, c[1] + dir[1] * thickness, c[2] + dir[2] * thickness],
      r,
      r * 0.55,
      6,
      false,
    );
  }
  return out;
}

/** A standing pin insulator — medium and low voltage sit on these, not under. */
function pinInsulator(out, { at, length, detail }) {
  const [x, y, z] = at;
  const sides = TUBE_SIDES[detail];
  tube(out, [x, y, z], [x, y + length, z], length * 0.42, length * 0.3, sides);
  tube(out, [x, y + length * 0.55, z], [x, y + length * 0.75, z], length * 0.5, length * 0.34, sides, false);
  return out;
}

/**
 * What is left of a fitting at the coarsest level: one bar per conductor point,
 * as wide as the thing it stands in for looked.
 *
 * Not nothing, which is what it used to be. On a 60 m Donaumast the insulator
 * strings are a twentieth of the mast and dropping them is free; on a 15 m
 * medium-voltage lattice the six pin insulators are **most of what there is to
 * see**, and a level that throws them away loses seventy per cent of the mast's
 * ink and turns it into a bare pole two hundred metres before it should.
 * `--ink` is what found that; nothing about the triangle count says it.
 */
function fittingStub(out, { at, length, width, direction }) {
  const [x, y, z] = at;
  const dir = direction ?? [0, -1, 0];
  const end = [x + dir[0] * length, y + dir[1] * length, z + dir[2] * length];
  member(out, [x, y, z], end, width, width);
  return out;
}

/** The double-bell porcelain of a railway telegraph pole. */
function bellInsulator(out, { at, length, detail }) {
  const [x, y, z] = at;
  const sides = Math.max(5, TUBE_SIDES[detail] - 2);
  tube(out, [x, y, z], [x, y + length * 1.2, z], length * 0.16, length * 0.16, sides, false);
  tube(out, [x, y + length * 0.5, z], [x, y + length * 1.1, z], length * 0.5, length * 0.34, sides, false);
  return out;
}

/** The earth-wire peak: a small pyramid of the same lattice on the body top. */
function earthPeak(out, { top, half, height, brace, detail }) {
  if (height <= 0.05) return out;
  const apex = [0, top + height, 0];
  const feet = [
    [-half, top, -half],
    [half, top, -half],
    [half, top, half],
    [-half, top, half],
  ];
  for (const f of feet) member(out, f, apex, brace * 1.2, brace * 1.2);
  if (BRACE_SHARE[detail] > 0) {
    const mid = feet.map((f) => [f[0] * 0.45, top + height * 0.55, f[2] * 0.45]);
    for (let c = 0; c < 4; c++) member(out, mid[c], mid[(c + 1) % 4], brace * 0.7, brace * 0.7);
  }
  return out;
}

/**
 * Two peaks, one over each leg — what a portal carries, and what a mast with
 * two earth wires carries. `spacing` is where the apexes stand, `half` how wide
 * each peak's own foot is; a peak as wide as the beam would be a tent, not a
 * peak.
 */
function twinPeaks(out, { top, spacing, half, height, brace }) {
  if (height <= 0.05) return out;
  for (const s of [-1, 1]) {
    const apex = [s * spacing, top + height, 0];
    const feet = [
      [s * spacing - half, top, -half],
      [s * spacing + half, top, -half],
      [s * spacing + half, top, half],
      [s * spacing - half, top, half],
    ];
    for (const f of feet) member(out, f, apex, brace, brace);
  }
  return out;
}

/** Concrete stubs under the legs — the part of a mast that is actually visible
 * from a train, because the grass grows up to them. */
function foundations(out, { half, size }) {
  for (const sx of [-1, 1]) {
    for (const sz of [-1, 1]) {
      const x = sx * half;
      const z = sz * half;
      box(out, [x - size, -0.4, z - size], [x + size, size * 1.3, z + size]);
    }
  }
  return out;
}

/** The transformer platform of a Masttransformatorstation. */
function transformer(out, { y, diameter }) {
  const w = Math.max(0.7, diameter * 3.2);
  box(out, [-w / 2, y, -w / 2], [w / 2, y + w * 1.15, w / 2]);
  // The platform it stands on, wider than the tank.
  box(out, [-w * 0.75, y - 0.16, -w * 0.75], [w * 0.75, y, w * 0.75]);
  return out;
}

/**
 * Where the conductors sit on one arm, as **signed** x offsets from the mast
 * centre — the whole arm, both sides, and the middle one where there is one.
 *
 * The outermost sits at the tip and the rest come evenly in towards the body,
 * no closer to it than it is wide. Two things were wrong with the first cut and
 * both showed on the small masts, which is where an arm is short:
 *
 * **The inner limit was a fixed 90 cm**, and a low-voltage pole's arm is 1.6 m
 * across — so both points on a side were pushed to the same place and the four
 * conductors of a village line came out as two insulators.
 *
 * **An odd count was rounded up.** Three conductors on one level is the
 * commonest medium-voltage arrangement in Germany, and it was drawn as four.
 * The odd one belongs over the pole, which is where it stands on the prototype.
 */
export function conductorPoints(halfWidth, conductors, rootX) {
  // A telegraph pole carries wires without the atlas counting them as
  // conductors; one a side is what its crossarm is for.
  const total = conductors > 0 ? conductors : 2;
  if (halfWidth <= 0.02) {
    return [0];
  }
  // Never more than half way in: a clearance that does not scale with the arm
  // collapses every point on a short one into the same spot.
  const inner = Math.min(rootX + 0.9, halfWidth * 0.55);
  const perSide = Math.floor(total / 2);
  const xs = total % 2 === 1 ? [0] : [];
  for (let i = 0; i < perSide; i++) {
    const x = perSide === 1 ? halfWidth : halfWidth - ((halfWidth - inner) * i) / (perSide - 1);
    xs.push(-x, x);
  }
  return xs;
}

/**
 * Builds one mast.
 *
 * @param {object} type an entry of `pylons.json`
 * @param {object} options `{ role, height, detail }`
 * @returns {{ structure: object, fittings: object }} two geometries, one per material
 */
export function buildMast(type, { role = 'suspension', height, detail = 0 } = {}) {
  const base = type.build;
  // What the coarse levels dropped is given back as width (see MEMBER_SCALE).
  const b = {
    ...base,
    leg_m: base.leg_m * MEMBER_SCALE[detail],
    brace_m: base.brace_m * MEMBER_SCALE[detail],
    arm_m: base.arm_m * ARM_SCALE[detail],
  };
  const H = height ?? (type.height_m[0] + type.height_m[1]) / 2;
  const structure = empty();
  const fittings = empty();
  // The concrete a lattice mast stands on. A pole *is* concrete, so its own
  // collar stays in the structure.
  const concrete = empty();

  const peakHeight = b.peaks > 0 ? b.peak_m : 0;
  const bodyTop = H - peakHeight;
  const waist = b.waist >= 1 ? bodyTop : b.waist * H;
  const shaftHalf = b.shaft_m / 2;

  // A tension mast carries the pull of the conductors, so it is stouter: a
  // wider base and heavier members. That is the whole visible difference, and
  // it is enough — from a train you read an Abspannmast off its horizontal
  // insulator strings long before you read it off the steel.
  const stout = role === 'tension' ? 1.18 : 1;

  if (b.kind === 'lattice') {
    latticeBody(structure, {
      base: b.base_m * stout,
      shaft: b.shaft_m * stout,
      waist,
      top: bodyTop,
      panels: b.panels,
      shaftPanels: Math.max(1, b.shaft_panels),
      leg: b.leg_m * stout,
      brace: b.brace_m,
      detail,
    });
    if (detail <= 1) foundations(concrete, { half: (b.base_m * stout) / 2, size: b.leg_m * 1.6 });
  } else if (b.kind === 'portal') {
    // Two legs under one beam: the body is a pair of lattice shafts standing
    // where the beam ends, and the beam is the crossarm that spans them.
    const legHalf = type.crossarms[0].width_m / 2 - b.base_m / 2;
    for (const s of [-1, 1]) {
      const leg = empty();
      latticeBody(leg, {
        base: b.base_m,
        shaft: b.shaft_m,
        waist: bodyTop,
        top: bodyTop,
        panels: b.panels,
        shaftPanels: 1,
        leg: b.leg_m,
        brace: b.brace_m,
        detail,
      });
      append(structure, translate(leg, [s * legHalf, 0, 0]));
      if (detail <= 1) {
        const foot = empty();
        foundations(foot, { half: b.base_m / 2, size: b.leg_m * 1.6 });
        append(concrete, translate(foot, [s * legHalf, 0, 0]));
      }
    }
  } else {
    poleBody(structure, {
      baseDiameter: b.base_m,
      topDiameter: b.shaft_m,
      top: bodyTop,
      detail,
    });
    if (b.kind === 'pole' && detail <= 1) {
      // The collar of earth the pole is rammed into.
      tube(structure, [0, -0.15, 0], [0, 0.25, 0], b.base_m * 0.85, b.base_m * 0.72, TUBE_SIDES[detail]);
    }
  }

  // The crossarms, and the fitting that hangs at every conductor point.
  const flat = b.kind === 'pole';
  for (const arm of type.crossarms) {
    const y = arm.at_frac * H;
    const halfWidth = arm.width_m / 2;
    const rootX = b.kind === 'portal' ? 0 : shaftHalf;
    const depth = arm.depth_m;

    if (halfWidth > 0.02) {
      const half = empty();
      if (flat) {
        bracket(half, { y, root: rootX * 0.6, tip: halfWidth, size: b.arm_m, zRoot: 0, depth, detail });
      } else if (b.kind === 'portal') {
        // The beam is one piece across both legs, so it is built whole here.
        bar(structure, [-halfWidth, y, 0], [halfWidth, y, 0], b.arm_m, b.arm_m * 1.6, detail);
        bar(structure, [-halfWidth, y - depth, 0], [halfWidth, y - depth, 0], b.arm_m * 0.8, b.arm_m * 0.8, detail);
        const bays = 8;
        for (let i = 0; i <= bays; i++) {
          const x = lerp(-halfWidth, halfWidth, i / bays);
          bar(structure, [x, y - depth, 0], [x, y, 0], b.arm_m * 0.5, b.arm_m * 0.5, detail);
        }
      } else {
        crossarmHalf(half, {
          y,
          root: rootX,
          tip: halfWidth,
          depth,
          zRoot: Math.max(0.35, shaftHalf * 0.9),
          zTip: Math.max(0.18, b.arm_m * 1.2),
          size: b.arm_m,
          detail,
        });
      }
      if (half.positions.length) append(structure, merge([half, mirrorX(half)]));
    }

    const xs = conductorPoints(halfWidth, arm.conductors, Math.max(rootX, b.base_m * 0.3));
    for (const x of xs) {
      {
        const at = [x, y, 0];
        if (detail >= 3) {
          // A pin stands on the arm and a string hangs under it, and at this
          // distance the difference is which side of the arm the bar is on.
          const standing = b.fitting === 'pin' || b.fitting === 'bell';
          fittingStub(fittings, {
            at: standing ? at : [at[0], at[1] - 0.2, at[2]],
            length: b.insulator_m,
            width: b.insulator_m * (standing ? 0.62 : 0.1),
            direction: standing ? [0, 1, 0] : [0, -1, 0],
          });
        } else if (b.fitting === 'pin') {
          pinInsulator(fittings, { at, length: b.insulator_m, detail });
        } else if (b.fitting === 'bell') {
          for (const z of [-0.13, 0.13]) {
            bellInsulator(fittings, { at: [at[0], at[1], z], length: b.insulator_m, detail });
          }
        } else if (role === 'tension') {
          // Horizontal, pulling inwards along the line: the strings of an
          // Abspannmast lie in the direction of the conductors, one each side.
          //
          // They start **on the arm**. Starting them 40 cm out along the line
          // left each one hanging in the air with a visible gap between it and
          // the steel it is supposed to be shackled to; 12 cm is enough to keep
          // the two apart where they meet and close enough to read as attached.
          for (const zDir of [-1, 1]) {
            insulatorString(fittings, {
              at: [at[0], at[1] - 0.25, zDir * 0.12],
              length: b.insulator_m,
              discs: b.insulator_discs,
              direction: [0, 0, zDir],
              detail,
            });
          }
        } else {
          insulatorString(fittings, {
            at: [at[0], at[1] - 0.2, 0],
            length: b.insulator_m,
            discs: b.insulator_discs,
            detail,
          });
        }
      }
    }
  }

  if (b.peaks === 1) {
    earthPeak(structure, {
      top: bodyTop,
      half: shaftHalf,
      height: peakHeight,
      brace: Math.max(b.brace_m, 0.08),
      detail,
    });
  } else if (b.peaks === 2) {
    twinPeaks(structure, {
      top: bodyTop,
      spacing: type.crossarms[0].width_m / 2 - b.base_m / 2,
      half: shaftHalf,
      height: peakHeight,
      brace: Math.max(b.brace_m, 0.08),
    });
  }

  if (type.id === 'masttrafo-20kv' && detail <= 2) {
    transformer(structure, { y: H * 0.46, diameter: b.base_m });
  }

  // The weathering: per-member shade and the dirt that gathers at the foot.
  // The insulators get less of it — porcelain sheds rain, which is its job.
  weather(structure, { ground: Math.max(4, H * 0.12), seed: 1 });
  weather(fittings, { ground: Math.max(3, H * 0.08), floor: 0.86, jitter: 0.03, seed: 2 });
  weather(concrete, { ground: 1.2, floor: 0.8, jitter: 0.04, seed: 3 });

  return { structure, fittings, concrete };
}

/**
 * The thinnest member the type is built from [m] — the brace of a lattice, the
 * crossarm of a pole. It is what decides where the fine levels stop paying:
 * once it is under a pixel it has stopped being drawn and started shimmering.
 */
export function finestMember(type) {
  const b = type.build;
  const candidates = [b.brace_m, b.arm_m, b.leg_m].filter((v) => v > 0.001);
  return Math.min(...candidates);
}

/**
 * The member that carries the mast's outline [m] — a leg of the lattice, or the
 * shaft of a pole. When *this* stops resolving there is nothing left to draw
 * but a silhouette, which is what the coarsest level is.
 */
export function bodyMember(type) {
  const b = type.build;
  return b.leg_m > 0.001 ? b.leg_m : b.base_m;
}

export { lerp, MEMBER_SCALE, ARM_SCALE };
