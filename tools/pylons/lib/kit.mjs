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

import { empty, member, angleMember, quad, tri, tube, box, merge, append, mirrorX, translate, weather, normalise, cross } from './geom.mjs';

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
/**
 * Facets on a round section per level.
 *
 * Smooth normals fix the hard-shaded prism, but they cannot fix the change of
 * shading gradient at a ten-sided tube's mesh edges in a close camera. Sixteen
 * sides put a two-metre compact mast's silhouette error below two centimetres;
 * the lower levels retain only what their screen size can show.
 */
const TUBE_SIDES = [16, 12, 8, 6];

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
function profiledBar(out, a, b, wu, wv, detail, primitive) {
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
  return primitive(out, from, to, wu, wv, detail === 0);
}

function bar(out, a, b, wu, wv, detail) {
  return profiledBar(out, a, b, wu, wv, detail, member);
}

/** Real angle steel close up, the same silhouette-saving box farther away. */
function latticeBar(out, a, b, wu, wv, detail) {
  return profiledBar(out, a, b, wu, wv, detail, detail === 0 ? angleMember : member);
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
const MEMBER_SCALE = [1.0, 1.22, 1.04, 1.75];

/**
 * The same for the crossarms, which lose their geometry on a different
 * schedule: the body drops half its bracing at `LOD1` and all of it at `LOD2`,
 * while an arm keeps both its chords until `LOD3` and only then falls to two
 * bars. One multiplier for both parts overshoots the body or starves the arms,
 * depending on which type it was fitted to — a Donaumast with a 24 m crossarm
 * and a medium-voltage lattice with a 2.6 m bracket are not the same problem.
 */
const ARM_SCALE = [1.0, 1.15, 1.5, 1.58];

const lerp = (a, b, t) => a + (b - a) * t;

/**
 * The four legs of a lattice body and the bracing between them.
 *
 * A German lattice mast is a square in plan: it stands on a wide base, the legs
 * batter steeply inward to a knee, then continue on a shallow taper to the
 * head. A mast is a cantilever against the wind, so it needs most of its width
 * near the ground while the upper shaft still narrows instead of becoming a
 * long parallel box.
 */
function latticeBody(out, { base, shaft, waist, top, panels, shaftPanels, leg, brace, detail }) {
  // German tower shafts do not turn into a long parallel box above the lower
  // batter. At the knee the steep foot changes into a much gentler taper that
  // continues to the mast head. `shaft` is therefore the width at the head;
  // the knee remains broad enough to transfer the leg forces into the foot.
  const kneeHalf = Math.max((shaft / 2) * 1.35, (base / 2) * 0.42);
  const half = (y) => y <= waist
    ? lerp(base / 2, kneeHalf, y / waist)
    : lerp(kneeHalf, shaft / 2, (y - waist) / Math.max(0.001, top - waist));
  const corners = (y) => {
    const h = half(y);
    return [
      [-h, y, -h],
      [h, y, -h],
      [h, y, h],
      [-h, y, h],
    ];
  };

  // The nodes the bracing meets the legs at: the steep batter first, then the
  // shallow upper taper. The coarse levels keep the *kink* at the waist and throw
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
    for (let c = 0; c < 4; c++) latticeBar(out, a[c], b[c], width, width, detail);
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
      latticeBar(out, a[c], b[d], brace, brace, detail);
      latticeBar(out, a[d], b[c], brace, brace, detail);
      latticeBar(out, b[c], b[d], brace * 0.8, brace * 0.8, detail);
    }
  }
  if (detail === 0) {
    // Angle sections do not meet by magic. Each panel node has two thin
    // gusset plates, one on each adjacent face, and a visible bolt head in
    // each plate. They are deliberately confined to LOD0: at the first hand-
    // over a 40 mm bolt is far below a pixel, while close up these joints are
    // what makes the mast read as assembled steel rather than grey sticks.
    const plate = Math.max(0.16, leg * 1.35);
    const thick = Math.max(0.014, leg * 0.055);
    const bolt = Math.max(0.018, Math.min(0.032, leg * 0.09));
    for (let i = 1; i < levels.length - 1; i++) {
      for (const [x, y, z] of corners(levels[i])) {
        box(
          out,
          [x - plate * 0.55, y - plate * 0.72, z - thick],
          [x + plate * 0.55, y + plate * 0.72, z + thick],
        );
        box(
          out,
          [x - thick, y - plate * 0.72, z - plate * 0.55],
          [x + thick, y + plate * 0.72, z + plate * 0.55],
        );
        const sx = Math.sign(x) || 1;
        const sz = Math.sign(z) || 1;
        tube(
          out,
          [x, y + plate * 0.28, z + sz * thick],
          [x, y + plate * 0.28, z + sz * (thick + bolt * 0.65)],
          bolt,
          bolt,
          6,
        );
        tube(
          out,
          [x + sx * thick, y - plate * 0.28, z],
          [x + sx * (thick + bolt * 0.65), y - plate * 0.28, z],
          bolt,
          bolt,
          6,
        );
      }
    }
  }
  return out;
}

/** A tapered pole — spun concrete, wood, or the tube of a compact mast. */
function poleBody(out, { baseDiameter, topDiameter, bottom = 0, top, detail }) {
  tube(
    out,
    [0, bottom, 0],
    [0, top, 0],
    baseDiameter / 2,
    topDiameter / 2,
    TUBE_SIDES[detail],
  );
  return out;
}

/**
 * Closes a planted pole against the terrain from above.
 *
 * `tube` quite correctly gives its bottom cap an outward, downward normal. A
 * terrain tile, however, is only a surface and supplies no solid soil inside
 * the pole footprint. At a grazing camera angle the downward cap is culled and
 * the ground shows through as a green semicircle. This independent upward fan
 * lives two millimetres above the placement plane, wholly inside the shaft, so
 * it closes that view without a collar or any below-ground side triangles.
 */
function poleFootDisc(out, { radius, detail }) {
  out.piece = (out.piece ?? 0) + 1;
  const sides = TUBE_SIDES[detail];
  const y = 0.002;
  const centre = [0, y, 0];
  const point = (i) => {
    const a = (i / sides) * Math.PI * 2;
    return [Math.cos(a) * radius, y, Math.sin(a) * radius];
  };
  for (let i = 0; i < sides; i++) {
    // next → current is +Y in the XZ plane.
    tri(out, centre, point(i + 1), point(i));
  }
  return out;
}

/**
 * One crossarm, from the body out to one tip, as two parallel trusses joined
 * across. Mirrored for the other side by the caller.
 *
 * The bottom chord is horizontal at the conductor level and the top chord
 * slopes down from the mast to meet it at the tip. This is the characteristic
 * German crossarm section in operator drawings: the deep root takes bending,
 * while the insulator hangs from the flat lower chord.
 */
function crossarmHalf(out, { y, root, tip, depth, zRoot, zTip, size, detail }) {
  if (tip <= root + 0.05) return out;
  if (detail >= 3) {
    // The coarsest level is the arm's outline: one chord out, one back up.
    member(out, [root, y, 0], [tip, y, 0], size * 1.25, size * 1.25);
    member(out, [root, y + depth, 0], [tip, y, 0], size, size);
    return out;
  }
  const truss = (z0, z1) => {
    const botRoot = [root, y, z0];
    const tipNode = [tip, y, z1];
    const topRoot = [root, y + depth, z0];
    latticeBar(out, botRoot, tipNode, size, size, detail);
    latticeBar(out, topRoot, tipNode, size * 0.85, size * 0.85, detail);
    latticeBar(out, botRoot, topRoot, size * 0.7, size * 0.7, detail);
    if (BRACE_SHARE[detail] > 0) {
      // Web members between the two chords, spaced so a long 380 kV arm gets
      // four or five and a 20 kV bracket gets one.
      const bays = Math.max(1, Math.round((tip - root) / 3.5 / (detail + 1)));
      for (let i = 1; i < bays; i++) {
        const t = i / bays;
        const lower = [lerp(root, tip, t), y, lerp(z0, z1, t)];
        const upper = [lerp(root, tip, t), lerp(y + depth, y, t), lerp(z0, z1, t)];
        latticeBar(out, lower, upper, size * 0.55, size * 0.55, detail);
        const back = [lerp(root, tip, (i - 1) / bays), y, lerp(z0, z1, (i - 1) / bays)];
        latticeBar(out, back, upper, size * 0.5, size * 0.5, detail);
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
    latticeBar(out, [x, y, -z], [x, y, z], size * 0.5, size * 0.5, detail);
  }
  return out;
}

/**
 * A welded tubular cantilever for a compact monopole.
 *
 * Compact supports are chosen precisely to avoid a lattice silhouette. Their
 * arms are round hollow sections: a tapered horizontal boom, a slimmer knee
 * brace and a short collar at the shaft. Using `crossarmHalf` here previously
 * bolted two angle-section trusses to a smooth monopole, visually turning it
 * back into an ordinary lattice tower.
 */
function tubeCrossarmHalf(out, { y, root, tip, depth, size, detail }) {
  if (tip <= root + 0.05) return out;
  const sides = TUBE_SIDES[detail];
  const boom = size * 0.5;
  tube(out, [root * 0.78, y, 0], [tip, y, 0], boom, boom * 0.62, sides);
  if (depth > 0.05 && detail < 3) {
    tube(
      out,
      [root * 0.78, y + depth, 0],
      [lerp(root, tip, 0.82), y, 0],
      boom * 0.62,
      boom * 0.45,
      Math.max(6, sides - 2),
    );
  }
  return out;
}

/** A deep, three-dimensional lattice beam between the two portal legs. */
function portalBeam(out, { y, halfWidth, depth, width, size, detail }) {
  const zRoot = Math.max(width * 0.38, size * 1.8);
  if (detail >= 3) {
    member(out, [-halfWidth, y, 0], [halfWidth, y, 0], size * 1.25, size * 1.25);
    member(out, [-halfWidth, y + depth, 0], [halfWidth, y + depth, 0], size, size);
    return out;
  }
  const bays = Math.max(4, Math.round(10 / (detail + 1)));
  for (const z of [-zRoot, zRoot]) {
    latticeBar(out, [-halfWidth, y, z], [halfWidth, y, z], size, size * 1.45, detail);
    latticeBar(
      out,
      [-halfWidth, y + depth, z],
      [halfWidth, y + depth, z],
      size * 0.82,
      size * 0.82,
      detail,
    );
    for (let i = 0; i < bays; i++) {
      const x0 = lerp(-halfWidth, halfWidth, i / bays);
      const x1 = lerp(-halfWidth, halfWidth, (i + 1) / bays);
      const a = i % 2 === 0 ? [x0, y, z] : [x0, y + depth, z];
      const b = i % 2 === 0 ? [x1, y + depth, z] : [x1, y, z];
      latticeBar(out, a, b, size * 0.55, size * 0.55, detail);
    }
  }
  // Transverse ties make the two visible side trusses one torsionally stable
  // beam instead of two unrelated flat drawings.
  for (let i = 0; i <= bays; i++) {
    const x = lerp(-halfWidth, halfWidth, i / bays);
    for (const yy of [y, y + depth]) {
      latticeBar(out, [x, yy, -zRoot], [x, yy, zRoot], size * 0.48, size * 0.48, detail);
    }
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
function insulatorString(out, hardware, { at, length, discs, direction, detail }) {
  const [x, y, z] = at;
  const dir = direction ?? [0, -1, 0];
  const end = [x + dir[0] * length, y + dir[1] * length, z + dir[2] * length];
  // The cap and pin between the porcelain units is galvanised steel. Painting
  // this five-centimetre spine brown made the old chain read as one moulded
  // plastic screw instead of nine separate insulators.
  // At LOD2 the caps have merged below one pixel. Its single cylinder carries
  // their distant silhouette and therefore matches the square fallback used
  // at LOD3; at the close levels it returns to the physical pin diameter.
  const distantWidth = Math.max(0.05, Math.min(0.10, length * 0.03));
  const rod = detail >= 2
    ? distantWidth / 2
    : Math.max(0.025, Math.min(0.04, length * 0.022));
  tube(hardware, [x, y, z], end, rod, rod, Math.max(6, TUBE_SIDES[detail] - 4));
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
  // A standard 110 kV cap is roughly 25 cm across and only a few centimetres
  // deep. The previous 16 cm, 10 cm-long one-sided cone was both too narrow
  // and too deep; in a horizontal tension chain it became a row of arrowheads.
  const r = Math.max(0.105, Math.min(0.145, length * 0.098));
  const sides = Math.max(8, TUBE_SIDES[detail] - 4);
  for (let i = 0; i < n; i++) {
    const t = (i + 0.5) / n;
    const c = [x + dir[0] * length * t, y + dir[1] * length * t, z + dir[2] * length * t];
    // Two shallow frusta make the bevelled rim of one porcelain cap. Keeping
    // the profile symmetric avoids a false direction in tension strings while
    // the metal pin still makes the electrical chain legible.
    const half = Math.min(0.032, (length / n) * 0.22);
    const neck = r * 0.46;
    tube(
      out,
      [c[0] - dir[0] * half, c[1] - dir[1] * half, c[2] - dir[2] * half],
      c,
      neck,
      r,
      sides,
      false,
    );
    tube(
      out,
      c,
      [c[0] + dir[0] * half, c[1] + dir[1] * half, c[2] + dir[2] * half],
      r,
      neck,
      sides,
      false,
    );
  }
  return out;
}

/**
 * A silicone-composite long-rod insulator.
 *
 * This is deliberately not another skin on `insulatorString`: a cap-and-pin
 * string is a row of broad, heavy units with metal between them, while a
 * composite has one fibreglass core under a continuous silicone housing and
 * thin alternating sheds. Compact-line hardware depends on that narrow,
 * uninterrupted silhouette. At 420 kV the real assemblies are about four
 * metres long; the two shed sizes are the common alternating profile that
 * keeps rain from bridging adjacent sheds.
 */
function compositeInsulator(out, hardware, { at, length, sheds, direction, detail }) {
  const [x, y, z] = at;
  const dir = direction ?? [0, -1, 0];
  const point = (t) => [x + dir[0] * length * t, y + dir[1] * length * t, z + dir[2] * length * t];
  const core = Math.max(0.045, Math.min(0.075, length * 0.016));
  tube(out, point(0), point(1), core, core, TUBE_SIDES[detail]);
  if (detail >= 2) return out;

  // Every shed close up, every third one after the first hand-over. Unlike a
  // porcelain cap, the shed is only a thin silicone skirt around the shared
  // rod, so its axial thickness stays well below its pitch.
  const n = Math.max(6, Math.round(sheds / (detail * 2 + 1)));
  const pitch = length / (n + 1);
  for (let i = 0; i < n; i++) {
    const t = (i + 1) / (n + 1);
    const radius = i % 2 === 0 ? 0.105 : 0.085;
    const half = Math.min(0.018, pitch * 0.16) / length;
    tube(out, point(t - half), point(t + half), radius, radius * 0.72, 6, false);
  }

  if (detail <= 1) {
    // EHV composite insulators require field-grading/corona hardware at the
    // live end. A segmented steel ring is enough geometry to keep that very
    // characteristic halo in the close levels, and belongs to the galvanised
    // structure material rather than the silicone one.
    const centre = point(0.94);
    const seed = Math.abs(dir[1]) < 0.9 ? [0, 1, 0] : [1, 0, 0];
    const cross = (a, b) => [
      a[1] * b[2] - a[2] * b[1],
      a[2] * b[0] - a[0] * b[2],
      a[0] * b[1] - a[1] * b[0],
    ];
    const normalise = (v) => {
      const n = Math.hypot(...v);
      return v.map((c) => c / n);
    };
    const u = normalise(cross(dir, seed));
    const v = cross(dir, u);
    const segments = detail === 0 ? 16 : 12;
    const ringPoint = (i) => {
      const a = (i / segments) * Math.PI * 2;
      return [
        centre[0] + 0.38 * (u[0] * Math.cos(a) + v[0] * Math.sin(a)),
        centre[1] + 0.38 * (u[1] * Math.cos(a) + v[1] * Math.sin(a)),
        centre[2] + 0.38 * (u[2] * Math.cos(a) + v[2] * Math.sin(a)),
      ];
    };
    for (let i = 0; i < segments; i++) {
      bar(hardware, ringPoint(i), ringPoint(i + 1), 0.045, 0.045, detail);
    }
  }
  return out;
}

/** Dispatches the two hanging fitting families without confusing sheds with caps. */
function lineInsulator(out, hardware, build, options) {
  if (build.fitting === 'composite') {
    return compositeInsulator(out, hardware, {
      ...options,
      sheds: build.insulator_sheds,
    });
  }
  return insulatorString(out, hardware, {
    ...options,
    discs: build.insulator_discs,
  });
}

/** A standing pin insulator — medium and low voltage sit on these, not under. */
function pinInsulator(out, { at, length, discs = 3, detail }) {
  const [x, y, z] = at;
  const sides = TUBE_SIDES[detail];
  // The load-bearing porcelain core is narrow; electrical creepage distance
  // comes from several thin sheds around it. The former model made the whole
  // 35 cm fitting one broad taper plus an open collar — a terracotta lamp
  // shade, and from below another one-sided hole. Three closed skirts give a
  // 20 kV post insulator its recognisable stepped silhouette.
  const core = length * 0.13;
  tube(out, [x, y, z], [x, y + length, z], core, core * 0.82, sides, true);
  if (detail >= 2) {
    // At this distance the individual skirts merge, but retaining two changes
    // neither the height nor the characteristic width of the fitting.
    discs = Math.min(discs, 2);
  }
  const count = Math.max(2, discs);
  for (let i = 0; i < count; i++) {
    const t = count === 1 ? 0.58 : 0.28 + (0.52 * i) / (count - 1);
    const half = length * 0.045;
    const radius = length * (0.45 - i * 0.025);
    tube(
      out,
      [x, y + length * t - half, z],
      [x, y + length * t + half, z],
      radius,
      radius * 0.72,
      sides,
      true,
    );
  }
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
  // These are low enough to be seen from below. Leaving both frusta open made
  // their one-sided mantles turn into white hooks from that direction: the
  // camera looked straight through the porcelain and picked up only fragments
  // of the far wall. Close the pin and the bell. The two bodies overlap, so the
  // internal caps disappear while the exposed underside remains a solid piece
  // of porcelain instead of a paper-thin, open cone.
  tube(out, [x, y, z], [x, y + length * 1.2, z], length * 0.16, length * 0.16, sides, true);
  tube(out, [x, y + length * 0.5, z], [x, y + length * 1.1, z], length * 0.5, length * 0.34, sides, true);
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
  for (const f of feet) latticeBar(out, f, apex, brace * 1.2, brace * 1.2, detail);
  if (BRACE_SHARE[detail] > 0) {
    const mid = feet.map((f) => [f[0] * 0.45, top + height * 0.55, f[2] * 0.45]);
    for (let c = 0; c < 4; c++) {
      latticeBar(out, mid[c], mid[(c + 1) % 4], brace * 0.7, brace * 0.7, detail);
    }
  }
  return out;
}

/**
 * Two peaks, one over each leg — what a portal carries, and what a mast with
 * two earth wires carries. `spacing` is where the apexes stand, `half` how wide
 * each peak's own foot is; a peak as wide as the beam would be a tent, not a
 * peak.
 */
function twinPeaks(out, { top, spacing, half, height, brace, detail }) {
  if (height <= 0.05) return out;
  for (const s of [-1, 1]) {
    const apex = [s * spacing, top + height, 0];
    const feet = [
      [s * spacing - half, top, -half],
      [s * spacing + half, top, -half],
      [s * spacing + half, top, half],
      [s * spacing - half, top, half],
    ];
    for (const f of feet) latticeBar(out, f, apex, brace, brace, detail);
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

/** Galvanised climbing pegs set into a concrete or wooden pole. */
function climbingIrons(out, { height, baseRadius, topRadius, detail }) {
  if (detail > 1) return out;
  const step = detail === 0 ? 0.50 : 1.0;
  const last = Math.min(height - 1.0, 9.5);
  let n = 0;
  for (let y = 1.5; y <= last; y += step, n++) {
    const radius = lerp(baseRadius, topRadius, y / height);
    const side = n % 2 === 0 ? -1 : 1;
    const z = -radius * 0.70;
    bar(
      out,
      [side * radius * 0.72, y, z],
      [side * (radius + 0.18), y, z],
      0.018,
      0.018,
      detail,
    );
  }
  return out;
}

/** A thin, closed sheet-metal plate extruded towards +Z (back to the mast). */
function signPlate(out, points, zFront, zBack) {
  out.piece = (out.piece ?? 0) + 1;
  const front = points.map(([px, py]) => [px, py, zFront]);
  const back = points.map(([px, py]) => [px, py, zBack]);
  for (let i = 1; i < points.length - 1; i++) {
    tri(out, front[0], front[i], front[i + 1]);
    tri(out, back[0], back[i + 1], back[i]);
  }
  for (let i = 0; i < points.length; i++) {
    const j = (i + 1) % points.length;
    quad(out, front[i], back[i], back[j], front[j]);
  }
  return out;
}

/** German high-voltage pylon identification plate: an aluminium carrier,
 * W012 warning triangle, the two-line danger legend and a mast-number field.
 * Everything printed is one filtered texture on one quad: tiny text triangles
 * shimmer under MSAA and several coplanar bolt pieces fight the depth buffer.
 * The carrier and its brackets remain physical PBR geometry. */
function warningSign(face, hardware, {
  origin,
  right = [1, 0, 0],
  up = [0, 1, 0],
  inward = [0, 0, 1],
  clearance = 0.06,
}) {
  // Build the whole assembly in its own orthonormal frame, then place it on the
  // support. A lattice leg batters inward in both plan axes: keeping its sign
  // world-vertical made the lower edge cut into the leg while the upper rail
  // floated several decimetres away on broad combination towers.
  const place = (geometry) => {
    for (let i = 0; i < geometry.positions.length; i += 3) {
      const x = geometry.positions[i];
      const y = geometry.positions[i + 1];
      const z = geometry.positions[i + 2];
      geometry.positions[i] = origin[0] + right[0] * x + up[0] * y + inward[0] * z;
      geometry.positions[i + 1] = origin[1] + right[1] * x + up[1] * y + inward[1] * z;
      geometry.positions[i + 2] = origin[2] + right[2] * x + up[2] * y + inward[2] * z;

      const nx = geometry.normals[i];
      const ny = geometry.normals[i + 1];
      const nz = geometry.normals[i + 2];
      geometry.normals[i] = right[0] * nx + up[0] * ny + inward[0] * nz;
      geometry.normals[i + 1] = right[1] * nx + up[1] * ny + inward[1] * nz;
      geometry.normals[i + 2] = right[2] * nx + up[2] * ny + inward[2] * nz;
    }
    return geometry;
  };

  const plate = empty();
  const print = empty();
  const zBack = -clearance;
  const zFront = zBack - 0.016;
  const left = -0.23;
  const rightEdge = 0.23;
  const bottom = -0.18;
  const top = 0.52;

  // Clipped corners approximate the pressed, rounded aluminium carrier used by
  // German network operators without spending a curve tessellation on it.
  const carrier = [
    [left + 0.025, top], [rightEdge - 0.025, top],
    [rightEdge, top - 0.025], [rightEdge, bottom + 0.025],
    [rightEdge - 0.025, bottom], [left + 0.025, bottom],
    [left, bottom + 0.025], [left, top - 0.025],
  ];
  signPlate(plate, carrier, zFront, zBack);

  // One quad, one draw material and one mip chain. U is intentionally reversed:
  // the model's front faces -Z, whose screen-right is model -X.
  print.piece = (print.piece ?? 0) + 1;
  quad(
    print,
    [left, bottom, zFront - 0.006],
    [left, top, zFront - 0.006],
    [rightEdge, top, zFront - 0.006],
    [rightEdge, bottom, zFront - 0.006],
    [[1, 1], [1, 0], [0, 0], [0, 1]],
  );

  // Two horizontal mounting rails bridge the plate; centre standoffs terminate
  // on the mast member. The four visible screw heads live in the colour texture
  // so they cannot fight the face quad.
  for (const by of [bottom + 0.080, top - 0.080]) {
    bar(plate, [-0.185, by, zBack + 0.006], [0.185, by, zBack + 0.006], 0.022, 0.012, 0);
    // With a short offset the 12 mm-deep rail itself bridges from the tangent
    // point to the carrier. Adding the old round standoff on top made the sign
    // look as if it hovered in front of a concrete or tubular pole.
    if (clearance > 0.02) {
      tube(plate, [0, by, -0.003], [0, by, zBack + 0.010], 0.018, 0.018, 8);
    }
  }
  append(hardware, place(plate));
  append(face, place(print));
  return face;
}

/** The oil transformer and its steel console on a Masttransformatorstation. */
function transformer(out, porcelain, { y, diameter, detail }) {
  const radius = Math.max(0.34, diameter * 1.08);
  const tankHeight = Math.max(0.82, radius * 2.2);
  const near = diameter / 2 + 0.10;
  const centreZ = near + radius + 0.07;
  const far = centreZ + radius + 0.14;
  const railX = radius * 0.72;
  const rail = 0.085;

  // Two proper cantilever rails instead of a concrete-looking solid shelf.
  // They start inside the pole silhouette so the joint is visibly connected,
  // and diagonal knees transfer the load back into the shaft below.
  for (const x of [-railX, railX]) {
    bar(out, [x, y, diameter * 0.20], [x, y, far], rail, rail, detail);
    bar(
      out,
      [x, y - 0.72, diameter * 0.24],
      [x, y - 0.02, centreZ + radius * 0.48],
      rail * 0.82,
      rail * 0.82,
      detail,
    );
  }
  for (const z of [centreZ - radius * 0.72, centreZ + radius * 0.72]) {
    bar(out, [-radius * 1.12, y, z], [radius * 1.12, y, z], rail, rail, detail);
  }

  // A small rural distribution transformer is an oil-filled cylindrical tank,
  // not a metre-wide switchgear cabinet. Build its rolled shell, bottom flange,
  // rounded shoulder and bolted lid as separate silhouettes.
  const bottom = y + rail * 0.62;
  const shoulder = bottom + tankHeight * 0.82;
  const top = bottom + tankHeight;
  const sides = TUBE_SIDES[detail];
  tube(out, [0, bottom, centreZ], [0, shoulder, centreZ], radius, radius, sides, true);
  tube(
    out,
    [0, bottom - 0.025, centreZ],
    [0, bottom + 0.035, centreZ],
    radius * 1.06,
    radius * 1.06,
    sides,
    true,
  );
  tube(out, [0, shoulder, centreZ], [0, top - 0.045, centreZ], radius, radius * 0.82, sides, true);
  tube(
    out,
    [0, top - 0.045, centreZ],
    [0, top + 0.025, centreZ],
    radius * 0.86,
    radius * 0.86,
    sides,
    true,
  );

  // Thin vertical radiator plates. Close up there are enough to read as a
  // cooling surface; lower levels retain only the outer silhouette.
  const fins = detail === 0 ? 7 : detail === 1 ? 5 : detail === 2 ? 3 : 0;
  for (let i = 0; i < fins; i++) {
    const t = fins === 1 ? 0 : i / (fins - 1) - 0.5;
    const x = t * radius * 1.42;
    const z0 = centreZ + Math.sqrt(Math.max(0, radius * radius - x * x));
    box(
      out,
      [x - 0.014, bottom + tankHeight * 0.12, z0 - 0.015],
      [x + 0.014, shoulder - tankHeight * 0.08, z0 + 0.10],
    );
  }

  // Three medium-voltage porcelain bushings on the lid. They use the same
  // fired-brown fitting material as the line's post insulators, and are kept
  // separate from the painted steel tank for that reason.
  if (detail < 3) {
    for (const x of [-radius * 0.52, 0, radius * 0.52]) {
      pinInsulator(porcelain, {
        at: [x, top + 0.025, centreZ],
        length: 0.24,
        discs: 2,
        detail: Math.min(detail + 1, 2),
      });
    }
  }
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
  const perSide = Math.floor(total / 2);
  const xs = total % 2 === 1 ? [0] : [];
  for (let i = 0; i < perSide; i++) {
    // Equal electrical bays are the useful default: on a three-position half
    // arm this yields 1/3, 2/3 and the tip, and on a Donau lower arm the inner
    // phase is half way out. The clearance clamp matters only on tiny wooden
    // crossarms, where a fixed 90 cm would otherwise put the fitting beyond
    // the steel. This is deliberately byte-for-byte the rule in
    // `PowerArm::offsets`, because the rendered wire must meet this fitting.
    const equalBay = halfWidth * (perSide - i) / perSide;
    const clearance = Math.min(rootX + 0.9, halfWidth * 0.55);
    const x = Math.max(equalBay, clearance);
    xs.push(-x, x);
  }
  return xs;
}

/**
 * Builds one mast.
 *
 * @param {object} type an entry of `pylons.json`
 * @param {object} options `{ role, height, detail }`
 * @returns {{ structure: object, fittings: object, concrete: object, hardware: object, signage: object, markings: object }} one geometry per material
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
  // Material follows the part, not the mast: a concrete shaft still carries a
  // hot-dip-galvanised crossarm, brackets and transformer platform. Keeping
  // those in `structure` painted the whole assembly as cast concrete.
  const hardware = empty();
  const signage = empty();
  const markings = empty();

  const peakHeight = b.peaks > 0 ? b.peak_m : 0;
  const bodyTop = H - peakHeight;
  const waist = b.waist >= 1 ? bodyTop : b.waist * H;
  const shaftHalf = b.shaft_m / 2;

  // A tension mast carries the pull of the conductors, so it is stouter: a
  // wider base and heavier members. That is the whole visible difference, and
  // it is enough — from a train you read an Abspannmast off its horizontal
  // insulator strings long before you read it off the steel.
  const stout = role === 'tension' ? 1.18 : 1;
  const roundStout = role === 'tension' && (b.kind === 'pole' || b.kind === 'tube') ? 1.08 : 1;
  const effectiveShaftHalf = b.kind === 'lattice'
    ? shaftHalf * stout
    : shaftHalf * roundStout;
  const armSize = b.arm_m * (role === 'tension' ? 1.22 : 1);

  if (b.kind === 'lattice') {
    latticeBody(structure, {
      base: b.base_m * stout,
      shaft: b.shaft_m * stout,
      waist,
      top: bodyTop,
      panels: b.panels,
      shaftPanels: Math.max(1, b.shaft_panels),
      leg: b.leg_m * stout,
      brace: b.brace_m * (role === 'tension' ? 1.10 : 1),
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
      baseDiameter: b.base_m * roundStout,
      topDiameter: b.shaft_m * roundStout,
      // Terrain is a single surface, not a closed soil volume. Extending a pole
      // beneath it leaves the lower side triangles visible around the depth
      // intersection: two points with a green semicircle between them. End at
      // the placement plane instead; the bottom cap faces down and is culled
      // from every normal above-ground view.
      bottom: 0,
      top: bodyTop,
      detail,
    });
    if (b.kind === 'pole') {
      poleFootDisc(structure, { radius: b.base_m * roundStout / 2, detail });
      climbingIrons(hardware, {
        height: bodyTop,
        baseRadius: b.base_m * roundStout / 2,
        topRadius: b.shaft_m * roundStout / 2,
        detail,
      });
    }
    // A pole is embedded directly in soil or in a buried socket foundation.
    // There is no visible flared collar. The old extra tube was closed at both
    // ends: its top cap cut through the pole as a horizontal disc and its
    // buried bottom cap poked through the terrain as triangular fins.
  }

  // The crossarms, and the fitting that hangs at every conductor point.
  const flat = b.kind === 'pole';
  for (const arm of type.crossarms) {
    const y = arm.at_frac * H;
    const halfWidth = arm.width_m / 2;
    const rootX = b.kind === 'portal' ? 0 : effectiveShaftHalf;
    const depth = arm.depth_m;

    if (halfWidth > 0.02) {
      const half = empty();
      if (flat) {
        bracket(half, { y, root: rootX * 0.6, tip: halfWidth, size: armSize, zRoot: 0, depth, detail });
      } else if (b.kind === 'portal') {
        // The beam is one piece across both legs, so it is built whole here.
        portalBeam(structure, {
          y,
          halfWidth,
          depth,
          width: b.shaft_m,
          size: armSize,
          detail,
        });
      } else if (b.kind === 'tube') {
        tubeCrossarmHalf(half, {
          y,
          root: rootX,
          tip: halfWidth,
          depth,
          size: armSize,
          detail,
        });
        // One collar around the shaft, not one in each mirrored arm half: two
        // coincident cylinders flicker and fail the closed-surface audit.
        if (detail < 3) {
          tube(
            structure,
            [0, y - armSize * 0.16, 0],
            [0, y + armSize * 0.16, 0],
            rootX * 1.035,
            rootX * 1.035,
            TUBE_SIDES[detail],
          );
        }
      } else {
        crossarmHalf(half, {
          y,
          root: rootX,
          tip: halfWidth,
          depth,
          zRoot: Math.max(0.35, effectiveShaftHalf * 0.9),
          zTip: Math.max(0.18, armSize * 1.2),
          size: armSize,
          detail,
        });
      }
      if (half.positions.length) {
        const armMaterial = type.structure === 'spun-concrete' ? hardware : structure;
        append(armMaterial, merge([half, mirrorX(half)]));
      }
    }

    const xs = conductorPoints(halfWidth, arm.conductors, rootX);
    for (const x of xs) {
      {
        // A middle pin cannot occupy any mast's centreline below its top: that
        // puts the concrete shaft or lattice cage straight through porcelain.
        // A short galvanised bracket carries it along the line. From the front
        // it remains the middle conductor, but in depth there is real clearance.
        const blockedCentrePin =
          b.fitting === 'pin' && Math.abs(x) < 1e-6 && y < bodyTop - 0.02;
        const z = blockedCentrePin
          ? effectiveShaftHalf + b.insulator_m * 0.42 + 0.025
          : 0;
        const at = [x, y, z];
        if (blockedCentrePin) {
          bar(hardware, [0, y, effectiveShaftHalf * 0.75], at, armSize * 0.72, armSize * 0.72, detail);
        }
        if (detail >= 3) {
          // A pin stands on the arm and a string hangs under it, and at this
          // distance the difference is which side of the arm the bar is on.
          const standing = b.fitting === 'pin' || b.fitting === 'spool' || b.fitting === 'bell';
          fittingStub(fittings, {
            at: standing ? at : [at[0], at[1] - 0.2, at[2]],
            length: b.insulator_m,
            // A hanging string keeps the diameter of its load-bearing rod.
            // Scaling it with the *length* made a 3.4 m EHV string a 34 cm
            // beam at LOD3 — over three times its LOD2 diameter. Standing pin
            // and bell fittings really do carry their broad shed silhouette.
            width: standing
              ? b.insulator_m * 0.62
              : Math.max(0.05, Math.min(0.10, b.insulator_m * 0.03)),
            direction: standing ? [0, 1, 0] : [0, -1, 0],
          });
        } else if (b.fitting === 'pin' || b.fitting === 'spool') {
          pinInsulator(fittings, {
            at,
            length: b.insulator_m,
            discs: b.insulator_discs,
            detail,
          });
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
            lineInsulator(fittings, structure, b, {
              at: [at[0], at[1] - 0.25, zDir * 0.12],
              length: b.insulator_m,
              direction: [0, 0, zDir],
              detail,
            });
          }
        } else {
          lineInsulator(fittings, structure, b, {
            at: [at[0], at[1] - 0.2, 0],
            length: b.insulator_m,
            detail,
          });
        }
      }
    }
  }

  if (b.peaks === 1) {
    earthPeak(structure, {
      top: bodyTop,
      half: effectiveShaftHalf,
      height: peakHeight,
      brace: Math.max(b.brace_m * (role === 'tension' ? 1.10 : 1), 0.08),
      detail,
    });
  } else if (b.peaks === 2) {
    twinPeaks(structure, {
      top: bodyTop,
      spacing: type.crossarms[0].width_m / 2 - b.base_m / 2,
      half: effectiveShaftHalf,
      height: peakHeight,
      brace: Math.max(b.brace_m * (role === 'tension' ? 1.10 : 1), 0.08),
      detail,
    });
  }

  if (type.id === 'masttrafo-20kv') {
    // The transformer is the identity and most of the area of this object, not
    // fine detail. Dropping it at LOD3 changed a transformer station into an
    // ordinary pole and removed a quarter of its ink for 24 saved triangles.
    transformer(hardware, fittings, { y: H * 0.46, diameter: b.base_m, detail });
    // The 400 V side leaves as a black insulated bundle which is clipped down
    // the pole and disappears into the ground. Without it the tank is a loose
    // prop on a shelf rather than the endpoint of the village supply.
    if (detail <= 1) {
      const y = H * 0.46;
      const z = b.base_m / 2 + 0.025;
      tube(markings, [0.08, y + 0.42, 0.92], [0.08, y + 0.08, z], 0.026, 0.026, 8);
      tube(markings, [0.08, y + 0.08, z], [0.08, 0.10, z], 0.026, 0.026, 8);
    }
  }

  if (detail === 0 && Number(type.voltage_kv?.[0] ?? type.voltage_kv) >= 20 && b.kind !== 'portal') {
    const signY = Math.min(3.0, H * 0.28);
    let origin;
    let right;
    let up;
    let inward;
    if (b.kind === 'lattice') {
      const baseHalf = b.base_m * stout / 2;
      const kneeHalf = Math.max(effectiveShaftHalf * 1.35, baseHalf * 0.42);
      const waistY = b.waist >= 1 ? bodyTop : b.waist * H;
      const bodyHalf = signY <= waistY
        ? lerp(baseHalf, kneeHalf, signY / waistY)
        : lerp(kneeHalf, effectiveShaftHalf, (signY - waistY) / Math.max(0.001, bodyTop - waistY));
      const slope = signY <= waistY
        ? (kneeHalf - baseHalf) / waistY
        : (effectiveShaftHalf - kneeHalf) / Math.max(0.001, bodyTop - waistY);
      // Front-right leg: its centre follows (+half, y, -half). The plate's
      // horizontal direction is tangent to the square body at that corner;
      // its up direction is the actual battered leg axis. Their cross product
      // points back into the mast, giving a rigid carrier flush with the leg.
      right = normalise([1, 0, 1]);
      up = normalise([slope, 1, -slope]);
      inward = normalise(cross(right, up));
      const legHalf = b.leg_m * stout / 2;
      origin = [
        bodyHalf - inward[0] * legHalf,
        signY - inward[1] * legHalf,
        -bodyHalf - inward[2] * legHalf,
      ];
    } else {
      const baseRadius = b.base_m * roundStout / 2;
      const radius = lerp(baseRadius, effectiveShaftHalf, signY / bodyTop);
      const radiusSlope = (effectiveShaftHalf - baseRadius) / bodyTop;
      // A spun-concrete or tubular mast is a shallow cone. Following the front
      // generatrix keeps both mounting rails at the same distance instead of
      // touching at one edge and opening a visible wedge at the other.
      right = [1, 0, 0];
      up = normalise([0, 1, -radiusSlope]);
      inward = normalise(cross(right, up));
      origin = [0, signY, -radius];
    }
    warningSign(signage, hardware, {
      origin,
      right,
      up,
      inward,
      // The carrier is bolted to the leg itself, whose L-section projects
      // farther forward than the thinner face bracing. A short 35 mm spacer is
      // therefore enough to clear both. The old blanket 300 mm offset produced
      // conspicuous floating plates on broad-base combination masts.
      // A round shaft uses the two rear rails as tangent clamps: the carrier
      // itself sits directly on them, with no separate spacer in profile.
      clearance: b.kind === 'lattice' ? 0.025 : 0,
    });
  }

  // The weathering: per-member shade and the dirt that gathers at the foot.
  // The insulators get less of it — porcelain sheds rain, which is its job.
  weather(structure, { ground: Math.max(4, H * 0.12), seed: 1 });
  weather(fittings, { ground: Math.max(3, H * 0.08), floor: 0.86, jitter: 0.03, seed: 2 });
  weather(concrete, { ground: 1.2, floor: 0.8, jitter: 0.04, seed: 3 });
  weather(hardware, { ground: Math.max(4, H * 0.12), seed: 4 });

  return { structure, fittings, concrete, hardware, signage, markings };
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
  // A tapered pole is seen over almost all of its height at the shaft
  // diameter, not at the flared metre around its foot. Using `base_m` kept the
  // compact mast's silhouette level beyond the global 4 km cull, so LOD3 could
  // never be selected at all.
  return b.leg_m > 0.001 ? b.leg_m : b.shaft_m;
}

export { lerp, MEMBER_SCALE, ARM_SCALE };
