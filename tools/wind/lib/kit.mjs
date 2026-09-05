// The kit: what a wind turbine is made of, and how each piece is built.
//
// A turbine is five things, and only the tower differs much between makers:
//
// - a **tower** — a tapered steel tube on nearly every machine since 2000,
//   a lattice on some of the 1990s ones;
// - a **nacelle** on top of it, the box that holds the drive train — a
//   rounded box on a Vestas, a Nordex, a Senvion or a GE, and Enercon's
//   drop-shaped shell, the one silhouette a viewer names from a train;
// - a **rotor** in front of the nacelle: three blades on a hub with a spinner
//   over it, the axis tilted up five degrees and the blades coned a few
//   degrees upwind so they clear the tower when they bend in a gust;
// - the **foundation**, a concrete disc the grass grows up to;
// - and, on anything whose tip reaches over a hundred metres, the aviation
//   **marking**: red bands on the blade tips and a red band round the tower by
//   day, two red lamps on the nacelle by night.
//
// Axes are the game's (`MODS.md`, *Track objects*): +Y up, the model's front
// along −Z. A turbine faces into the wind, so the rotor stands at −Z and the
// nacelle runs back along +Z; the placement's yaw turns the whole machine to
// where the wind comes from, and the nacelle node yaws on top of that as the
// weather turns (`world_render::wind`).
//
// The **moving parts are nodes**, not just meshes: `nacelle` sits at hub height
// and turns about Y; `rotor` sits at the hub, in front of the nacelle, and
// turns about its own Z. Everything that has to move hangs under them. The
// levels of detail are named the way every other model names them
// (`_LOD0` … `_LOD3`), and a level under the rotor turns with it.
//
// Every dimension comes from `wind.json`; this file knows shapes, not sizes.

import {
  append,
  empty,
  member,
  merge,
  quad,
  tint,
  tri,
  tube,
  weather,
} from '../../pylons/lib/geom.mjs';

// The materials a level's parts index, in the order `build_wind.mjs` writes
// them.
export const COATING = 0;
export const CONCRETE = 1;
export const GALVANISED = 2;
export const LAMP = 3;
export const DARK = 4;

/** Tilt of the rotor axis up out of the horizontal [deg] — five on every maker's sheet. */
export const TILT_DEG = 5;
/** Cone of the blades upwind, out of the rotor plane [deg]. */
export const CONE_DEG = 3;

/** The day marking: three six-metre bands from the tip inwards, red, white, red. */
const BAND_M = 6;
/** The tower band starts here [m] and is three metres tall (AVV Kennzeichnung). */
const TOWER_BAND_FROM = 40;
const TOWER_BAND_HEIGHT = 3;
const RED = [0.72, 0.06, 0.04, 1];
const WHITE = [1, 1, 1, 1];

/**
 * Enercon's tower foot: rings shaded from a deep green at the ground up to the
 * tower's own grey, so the machine grows out of the field instead of standing
 * on it. Seven bands on the prototype; the colours are linear RGB.
 */
const ENERCON_RINGS = [
  [0.10, 0.30, 0.16, 1],
  [0.16, 0.36, 0.20, 1],
  [0.24, 0.42, 0.26, 1],
  [0.36, 0.50, 0.36, 1],
  [0.52, 0.60, 0.50, 1],
  [0.70, 0.74, 0.66, 1],
  [0.88, 0.89, 0.86, 1],
];
const ENERCON_RING_HEIGHT = 1.6;

/** How many facets round a tube, per level of detail. */
const TOWER_SIDES = [28, 16, 10, 6];
/** Points round a blade section, per level. Four is a diamond: it still has a silhouette. */
const BLADE_POINTS = [20, 12, 8, 4];
/** Stations along a blade, per level — as a share of the blade from root to tip. */
const BLADE_STATIONS = [
  [0, 0.03, 0.06, 0.09, 0.12, 0.16, 0.2, 0.25, 0.31, 0.38, 0.46, 0.54, 0.62, 0.7, 0.78, 0.85, 0.91, 0.96, 0.99],
  [0, 0.06, 0.12, 0.2, 0.31, 0.46, 0.62, 0.78, 0.91, 0.99],
  [0, 0.12, 0.31, 0.62, 0.99],
  [0, 0.2, 0.99],
];
/** Facets on the spinner and the lamps. */
const ROUND_SIDES = [16, 10, 6, 6];
/** Lamp diameter per level [m] — bigger where the machine is far, so the light survives. */
const LAMP_M = [0.35, 0.5, 1.0, 1.6];

// ---------------------------------------------------------------------------
// Vector helpers
// ---------------------------------------------------------------------------

const add = (a, b) => [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
const sub = (a, b) => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const scale = (a, s) => [a[0] * s, a[1] * s, a[2] * s];
const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const cross = (a, b) => [
  a[1] * b[2] - a[2] * b[1],
  a[2] * b[0] - a[0] * b[2],
  a[0] * b[1] - a[1] * b[0],
];
const length = (a) => Math.hypot(a[0], a[1], a[2]);
const normalise = (a) => {
  const l = length(a);
  return l < 1e-9 ? [0, 1, 0] : [a[0] / l, a[1] / l, a[2] / l];
};
const lerp = (a, b, t) => a + (b - a) * t;
const smoothstep = (a, b, x) => {
  const t = Math.min(1, Math.max(0, (x - a) / (b - a)));
  return t * t * (3 - 2 * t);
};
const rad = (deg) => (deg * Math.PI) / 180;

/** A piecewise-linear table `[[x, y], …]` read at `x`. */
function table(points, x) {
  if (x <= points[0][0]) return points[0][1];
  for (let i = 1; i < points.length; i++) {
    if (x <= points[i][0]) {
      const [x0, y0] = points[i - 1];
      const [x1, y1] = points[i];
      return lerp(y0, y1, (x - x0) / (x1 - x0));
    }
  }
  return points[points.length - 1][1];
}

/** Rotates every position and normal of a geometry about the Z axis. */
export function rotateZ(geometry, angle) {
  const c = Math.cos(angle);
  const s = Math.sin(angle);
  for (const array of [geometry.positions, geometry.normals]) {
    for (let i = 0; i < array.length; i += 3) {
      const x = array[i];
      const y = array[i + 1];
      array[i] = x * c - y * s;
      array[i + 1] = x * s + y * c;
    }
  }
  return geometry;
}

/** Rotates every position and normal of a geometry about the Y axis. */
export function rotateY(geometry, angle) {
  const c = Math.cos(angle);
  const s = Math.sin(angle);
  for (const array of [geometry.positions, geometry.normals]) {
    for (let i = 0; i < array.length; i += 3) {
      const x = array[i];
      const z = array[i + 2];
      array[i] = x * c + z * s;
      array[i + 2] = -x * s + z * c;
    }
  }
  return geometry;
}

/** Moves every vertex by `delta`. */
export function translate(geometry, delta) {
  for (let i = 0; i < geometry.positions.length; i += 3) {
    geometry.positions[i] += delta[0];
    geometry.positions[i + 1] += delta[1];
    geometry.positions[i + 2] += delta[2];
  }
  return geometry;
}

// ---------------------------------------------------------------------------
// Surfaces
// ---------------------------------------------------------------------------

/**
 * A surface lofted through rings of points — the blade, the nacelle, the
 * spinner: everything here that is neither a straight tube nor a bar.
 *
 * Every ring has the same number of points and goes round the same way. The
 * normals are smooth in both directions, taken off the neighbouring points,
 * and pointed **outward** from each ring's own centre — so whichever way a
 * ring was listed, the faces are wound to match and the silhouette is not the
 * inside of the far side (the mistake the masts' `member` carried for a year).
 *
 * `v` runs along the loft in metres from the first ring, `u` round each ring
 * in metres, so the coating's rain streaks run *along* a blade and *down* a
 * nacelle. `colourOf(ring, point)` may shade single vertices — the dirt on a
 * blade's leading edge lives there.
 */
export function loft(out, rings, { closed = true, colourOf } = {}) {
  const n = rings[0].length;
  const m = rings.length;
  out.piece = (out.piece ?? 0) + 1;
  const centres = rings.map((ring) => {
    const c = [0, 0, 0];
    for (const p of ring) {
      c[0] += p[0] / n;
      c[1] += p[1] / n;
      c[2] += p[2] / n;
    }
    return c;
  });
  // Distance along the loft, ring by ring — the `v` of the map.
  const along = [0];
  for (let i = 1; i < m; i++) along.push(along[i - 1] + length(sub(centres[i], centres[i - 1])));

  const at = (i, j) => rings[i][((j % n) + n) % n];
  const normalAt = (i, j) => {
    const around = closed
      ? sub(at(i, j + 1), at(i, j - 1))
      : sub(at(i, Math.min(j + 1, n - 1)), at(i, Math.max(j - 1, 0)));
    const alongV = sub(at(Math.min(i + 1, m - 1), j), at(Math.max(i - 1, 0), j));
    let normal = normalise(cross(around, alongV));
    const outward = sub(at(i, j), centres[i]);
    // A ring collapsed to a point (a tip) has no outward; along is all there is.
    if (length(outward) > 1e-6 && dot(normal, outward) < 0) normal = scale(normal, -1);
    if (length(outward) <= 1e-6 && dot(normal, alongV) < 0) normal = scale(normal, -1);
    return normal;
  };
  const colour = (i, j) => colourOf?.(i, j) ?? out.tint ?? WHITE;

  const count = closed ? n : n - 1;
  for (let i = 0; i + 1 < m; i++) {
    let u = 0;
    for (let j = 0; j < count; j++) {
      const a = at(i, j);
      const b = at(i, j + 1);
      const c = at(i + 1, j + 1);
      const d = at(i + 1, j);
      const width = length(sub(b, a));
      const uv = [
        [u, along[i]],
        [u + width, along[i]],
        [u + width, along[i + 1]],
        [u, along[i + 1]],
      ];
      const normals = [normalAt(i, j), normalAt(i, j + 1), normalAt(i + 1, j + 1), normalAt(i + 1, j)];
      const colours = [colour(i, j), colour(i, j + 1), colour(i + 1, j + 1), colour(i + 1, j)];
      // Wind the face the way its own normal points — the smooth normals
      // decide, and a degenerate quad (a ring shrunk to its tip) is skipped.
      const face = cross(sub(b, a), sub(c, a));
      const mean = add(add(normals[0], normals[1]), add(normals[2], normals[3]));
      if (length(face) < 1e-12) {
        u += width;
        continue;
      }
      const flip = dot(face, mean) < 0;
      const saved = out.tint;
      const corners = flip ? [a, d, c, b] : [a, b, c, d];
      const map = flip ? [uv[0], uv[3], uv[2], uv[1]] : uv;
      const ns = flip ? [normals[0], normals[3], normals[2], normals[1]] : normals;
      const cs = flip ? [colours[0], colours[3], colours[2], colours[1]] : colours;
      // `quad` writes one tint for all four corners; the per-vertex colours
      // are patched in afterwards.
      const base = out.positions.length / 3;
      quad(out, corners[0], corners[1], corners[2], corners[3], map, ns);
      for (let k = 0; k < 4; k++) {
        out.colors[(base + k) * 4] = cs[k][0];
        out.colors[(base + k) * 4 + 1] = cs[k][1];
        out.colors[(base + k) * 4 + 2] = cs[k][2];
        out.colors[(base + k) * 4 + 3] = cs[k][3];
      }
      out.tint = saved;
      u += width;
    }
  }
  return out;
}

/** Closes a ring with a fan of triangles, the normal along `direction`. */
export function cap(out, ring, direction) {
  const n = ring.length;
  const c = [0, 0, 0];
  for (const p of ring) {
    c[0] += p[0] / n;
    c[1] += p[1] / n;
    c[2] += p[2] / n;
  }
  const d = normalise(direction);
  for (let j = 0; j < n; j++) {
    const a = ring[j];
    const b = ring[(j + 1) % n];
    const face = cross(sub(b, c), sub(a, c));
    if (dot(face, d) >= 0) tri(out, c, b, a);
    else tri(out, c, a, b);
  }
  return out;
}

/** A ring of `sides` points of radius `r` in the plane ⟂ `axis` through `centre`. */
function ring(centre, r, sides, axis = [0, 0, 1]) {
  const w = normalise(axis);
  const helper = Math.abs(w[1]) > 0.99 ? [0, 0, 1] : [0, 1, 0];
  const u = normalise(cross(helper, w));
  const v = cross(w, u);
  const points = [];
  for (let i = 0; i < sides; i++) {
    const t = (i / sides) * Math.PI * 2;
    points.push(add(add(centre, scale(u, Math.cos(t) * r)), scale(v, Math.sin(t) * r)));
  }
  return points;
}

/** A body of revolution about `axis` through `origin`: `profile` is `[[along, radius], …]`. */
export function revolve(out, origin, axis, profile, sides, { capStart = true, capEnd = true } = {}) {
  const w = normalise(axis);
  const rings = profile.map(([t, r]) => ring(add(origin, scale(w, t)), Math.max(r, 0.001), sides, w));
  loft(out, rings);
  if (capStart && profile[0][1] > 0.001) cap(out, rings[0], scale(w, -1));
  if (capEnd && profile[profile.length - 1][1] > 0.001) cap(out, rings[rings.length - 1], w);
  return out;
}

/** A sphere — a lamp, a sensor housing. */
export function sphere(out, centre, r, sides) {
  const profile = [];
  const rows = Math.max(3, Math.round(sides / 2));
  for (let i = 0; i <= rows; i++) {
    const t = (i / rows) * Math.PI;
    profile.push([-Math.cos(t) * r, Math.sin(t) * r]);
  }
  return revolve(out, centre, [0, 1, 0], profile, sides, { capStart: false, capEnd: false });
}

// ---------------------------------------------------------------------------
// The blade
// ---------------------------------------------------------------------------

/**
 * A blade section at `x` along the chord (0 = leading edge, 1 = trailing
 * edge) and `side` (+1 suction, −1 pressure): a NACA four-digit thickness
 * distribution with a little camber. Unit chord, thickness ratio `tc`.
 *
 * What comes back is `[chordwise, thicknesswise]` with the leading edge at
 * `+0.3` — the pitch axis is at thirty per cent of the chord, which is where
 * a blade's own axis runs — so a section turns about the right line when it
 * is twisted.
 */
function section(x, side, tc, camber) {
  const t = tc;
  const yt =
    5 * t * (0.2969 * Math.sqrt(x) - 0.126 * x - 0.3516 * x * x + 0.2843 * x ** 3 - 0.1036 * x ** 4);
  const p = 0.4;
  const yc =
    x < p
      ? (camber / (p * p)) * (2 * p * x - x * x)
      : (camber / ((1 - p) * (1 - p))) * (1 - 2 * p + 2 * p * x - x * x);
  return [0.3 - x, yc + side * yt];
}

/**
 * The chord [m] along the blade, `f` from the root (0) to the tip (1): a
 * cylinder at the root that a bolt circle fits, the widest point at a
 * quarter of the length, and a straight taper to the tip.
 */
function chordAt(blade, f) {
  return table(
    [
      [0, blade.root_m],
      [0.06, blade.root_m],
      [0.24, blade.max_m],
      [0.6, lerp(blade.max_m, blade.tip_m, 0.55)],
      [0.96, blade.tip_m],
      [1, blade.tip_m * 0.55],
    ],
    f,
  );
}

/** Thickness as a share of the chord: round at the root, thin at the tip. */
const thicknessAt = (f) =>
  table(
    [
      [0, 1],
      [0.1, 0.62],
      [0.2, 0.36],
      [0.4, 0.24],
      [0.7, 0.18],
      [1, 0.14],
    ],
    f,
  );

/** Twist [deg]: turned well into the wind at the root, flat at the tip. */
const twistAt = (f) =>
  table(
    [
      [0, 18],
      [0.12, 16],
      [0.3, 9],
      [0.6, 3.5],
      [0.9, 0],
      [1, -1],
    ],
    f,
  );

/** How far from the tip the day marking's bands lie [m], and their colours. */
function bandColour(fromTip) {
  if (fromTip < BAND_M) return RED;
  if (fromTip < 2 * BAND_M) return WHITE;
  if (fromTip < 3 * BAND_M) return RED;
  return null;
}

/**
 * One blade along +Y from the hub, coned upwind, in the rotor's frame (Z the
 * axis, the rotor turning clockwise seen from the wind, so the leading edge
 * of the blade at twelve o'clock faces +X).
 *
 * `marking` puts the AVV's three tip bands on: the stations are placed at the
 * band edges, so a band ends on a ring rather than fading across a face.
 */
function blade(out, spec, { detail, marking }) {
  const R = spec.rotor_m / 2;
  const r0 = spec.hub_diameter_m / 2 * 0.92;
  const span = R - r0;
  const points = BLADE_POINTS[detail];
  let stations = [...BLADE_STATIONS[detail]];
  if (marking) {
    for (const d of [BAND_M, 2 * BAND_M, 3 * BAND_M]) stations.push(1 - d / span);
    stations = [...new Set(stations)].sort((a, b) => a - b);
  }
  const cone = rad(CONE_DEG);
  const rings = [];
  const colours = [];
  for (const f of stations) {
    const r = r0 + f * span;
    const chord = chordAt(spec.blade, f);
    const tc = thicknessAt(f);
    // Round where a bolt circle is, an aerofoil where the wind works.
    const round = 1 - smoothstep(0.06, 0.22, f);
    const camber = 0.035 * (1 - round);
    const twist = rad(twistAt(f));
    const ring = [];
    const shade = [];
    for (let j = 0; j < points; j++) {
      // Round the section: upper surface leading edge → trailing edge, then
      // the lower surface back. `k` is the position on the surface, 0 = LE.
      const t = j / points;
      const upper = t < 0.5;
      const k = upper ? t * 2 : (1 - t) * 2;
      // Cosine spacing packs the points round the leading edge, where the
      // curvature is.
      const x = 0.5 - 0.5 * Math.cos(Math.PI * k);
      let [cx, cz] = section(x, upper ? 1 : -1, tc, camber);
      // The root cylinder: blend towards a circle about the pitch axis.
      const angle = Math.PI * 2 * t;
      const circle = [Math.cos(angle) * 0.5, Math.sin(angle) * 0.5];
      cx = lerp(cx, circle[0], round);
      cz = lerp(cz, circle[1], round);
      cx *= chord;
      cz *= chord;
      // Twist about the span axis: the leading edge turns upwind (−Z).
      const px = cx * Math.cos(twist) + cz * Math.sin(twist);
      const pz = -cx * Math.sin(twist) + cz * Math.cos(twist);
      // Cone: the blade leans upwind as it goes out.
      ring.push([px, r * Math.cos(cone), pz - r * Math.sin(cone)]);
      // The leading edge is sand-blasted matte and a shade darker along the
      // outer half; the rest of the blade keeps its gelcoat.
      const edge = Math.exp(-((k / 0.12) ** 2)) * smoothstep(0.35, 0.8, f);
      const dirt = 1 - edge * 0.14;
      shade.push([dirt, dirt, dirt * 0.985, 1]);
    }
    rings.push(ring);
    colours.push(shade);
  }
  // Marking bands by ring pair, the leading-edge dirt on top.
  const colourOf = (i, j) => {
    const base = colours[Math.min(i, colours.length - 1)][j % points];
    if (!marking) return base;
    const f = stations[Math.min(i, stations.length - 1)];
    const next = stations[Math.min(i + 1, stations.length - 1)];
    const midFromTip = (1 - (f + next) / 2) * span;
    const band = bandColour(midFromTip);
    if (!band) return base;
    return [band[0] * base[0], band[1] * base[1], band[2] * base[2], 1];
  };
  // The tip: the last ring shrunk onto its own centre closes the blade.
  const last = rings[rings.length - 1];
  const tipCentre = [0, 0, 0];
  for (const p of last) {
    tipCentre[0] += p[0] / points;
    tipCentre[1] += p[1] / points;
    tipCentre[2] += p[2] / points;
  }
  const tipRing = last.map((p) => add(tipCentre, scale(sub(p, tipCentre), 0.12)));
  rings.push(tipRing);
  colours.push(colours[colours.length - 1]);
  loft(out, rings, { colourOf });
  cap(out, tipRing, sub(tipCentre, [0, 0, 0]));
  // The root is bolted into the hub: close it, so a coarse spinner cannot
  // show the inside of a blade.
  cap(out, rings[0], [0, -1, 0]);
  return out;
}

/**
 * The rotor in its own frame: three blades a hundred and twenty degrees apart
 * and the spinner over the hub. The origin is the hub centre, the axis Z, the
 * wind from −Z.
 */
export function buildRotor(spec, { detail, marking, variant }) {
  const out = empty();
  const one = blade(empty(), spec, { detail, marking });
  append(out, one);
  append(out, rotateZ(merge([one]), (Math.PI * 2) / 3));
  append(out, rotateZ(merge([one]), (Math.PI * 4) / 3));

  const hub = spec.hub_diameter_m / 2;
  const sides = ROUND_SIDES[detail];
  if (variant === 'enercon') {
    // Enercon's spinner is the nose of the egg: a dome as wide as the shell
    // behind it, and no gap between the two.
    const r = spec.nacelle.diameter_m / 2;
    const profile = [
      [-r * 0.95, 0.001],
      [-r * 0.82, r * 0.42],
      [-r * 0.6, r * 0.72],
      [-r * 0.3, r * 0.92],
      [0, r],
      [hub * 0.9, r],
    ];
    revolve(out, [0, 0, 0], [0, 0, 1], profile, sides, { capStart: false, capEnd: true });
  } else if (detail < 3) {
    // An ogive nose over the hub, a shade longer than it is wide.
    const profile = [
      [-hub * 1.35, 0.001],
      [-hub * 1.1, hub * 0.42],
      [-hub * 0.75, hub * 0.78],
      [-hub * 0.3, hub * 0.97],
      [0, hub],
      [hub * 0.7, hub],
    ];
    revolve(out, [0, 0, 0], [0, 0, 1], profile, sides, { capStart: false, capEnd: true });
  } else {
    revolve(out, [0, 0, 0], [0, 0, 1], [[-hub, hub * 0.6], [hub * 0.6, hub * 0.9]], sides);
  }
  return out;
}

// ---------------------------------------------------------------------------
// The nacelle
// ---------------------------------------------------------------------------

/**
 * A rounded rectangle of `w` by `h` in the XY plane, `corner` the radius of
 * its corners, `segments` points on each corner — the section of the box
 * nacelle. `shrink` pulls it in towards the centre for a rounded end.
 */
function roundedRect(w, h, corner, segments, z, shrink = 1) {
  const points = [];
  const hw = (w / 2) * shrink;
  const hh = (h / 2) * shrink;
  const r = Math.min(corner * shrink, hw, hh);
  const corners = [
    [hw - r, hh - r, 0],
    [-(hw - r), hh - r, Math.PI / 2],
    [-(hw - r), -(hh - r), Math.PI],
    [hw - r, -(hh - r), (3 * Math.PI) / 2],
  ];
  for (const [cx, cy, start] of corners) {
    for (let i = 0; i <= segments; i++) {
      const a = start + (i / segments) * (Math.PI / 2);
      points.push([cx + Math.cos(a) * r, cy + Math.sin(a) * r, z]);
    }
  }
  return points;
}

/**
 * The nacelle in its own frame — the origin at hub height on the tower axis,
 * the rotor at −Z. What comes back is the shell, and separately what sits on
 * it: the yaw bearing down to the tower, the anemometer mast, the lamps.
 */
export function buildNacelle(spec, { detail, variant, marking }) {
  const shell = empty();
  const dark = empty();
  const lamps = empty();
  const hardware = empty();
  const n = spec.nacelle;
  const hub = spec.hub_diameter_m / 2;
  // The shell starts just behind the spinner and runs back.
  const front = -spec.overhang_m + hub * 0.75;

  if (variant === 'enercon') {
    // The egg: widest just behind the spinner, tapering to a rounded tail.
    const r = n.diameter_m / 2;
    const L = n.length_m;
    const sides = ROUND_SIDES[detail];
    const profile = [
      [front - 0.05, r],
      [front + L * 0.12, r],
      [front + L * 0.35, r * 0.93],
      [front + L * 0.6, r * 0.78],
      [front + L * 0.82, r * 0.55],
      [front + L * 0.95, r * 0.3],
      [front + L, 0.001],
    ];
    revolve(shell, [0, 0, 0], [0, 0, 1], profile, sides, { capStart: true, capEnd: false });
  } else {
    const segments = [4, 2, 1, 1][detail];
    const corner = detail < 2 ? Math.min(0.45, n.height_m * 0.16) : 0.05;
    const L = n.length_m;
    const rings = [roundedRect(n.width_m, n.height_m, corner, segments, front)];
    // The body, then a rounded tail over the last tenth.
    rings.push(roundedRect(n.width_m, n.height_m, corner, segments, front + L * 0.9));
    if (detail < 2) {
      rings.push(roundedRect(n.width_m, n.height_m, corner, segments, front + L * 0.97, 0.9));
      rings.push(roundedRect(n.width_m, n.height_m, corner, segments, front + L, 0.72));
    } else {
      rings.push(roundedRect(n.width_m, n.height_m, corner, segments, front + L, 0.85));
    }
    loft(shell, rings);
    cap(shell, rings[0], [0, 0, -1]);
    cap(shell, rings[rings.length - 1], [0, 0, 1]);
    if (detail < 2) {
      // The cooling louvres on the tail: the dark rectangle every box nacelle
      // has where the generator's air goes out.
      const z = front + L + 0.02;
      const w = n.width_m * 0.3;
      const h = n.height_m * 0.3;
      tint(dark, [0.25, 0.25, 0.25, 1]);
      quad(dark, [-w, -h, z], [w, -h, z], [w, h, z], [-w, h, z]);
    }
  }

  // The yaw bearing: a short drum from the nacelle floor down onto the tower.
  const floor = variant === 'enercon' ? -n.diameter_m / 2 + 0.2 : -n.height_m / 2 + 0.05;
  const bearing = spec.tower_top_m / 2 - 0.08;
  tube(hardware, [0, floor - 0.9, 0], [0, floor + 0.3, 0], bearing, bearing, TOWER_SIDES[detail], true);

  if (detail < 2) {
    // Anemometer and wind vane on the tail: two thin masts nobody sees but
    // everybody would miss.
    const top = variant === 'enercon' ? n.diameter_m * 0.36 : n.height_m / 2;
    const zTail = front + n.length_m * 0.86;
    member(hardware, [0.4, top, zTail], [0.4, top + 1.6, zTail], 0.06, 0.06, true);
    member(hardware, [-0.4, top, zTail], [-0.4, top + 1.3, zTail], 0.06, 0.06, true);
    member(hardware, [-0.4, top + 1.3, zTail], [-0.4, top + 1.3, zTail + 0.5], 0.04, 0.04, true);
  }

  if (marking) {
    // Feuer W, rot: two lamps at the tail corners, or one big one where the
    // machine is far enough away that two would be one pixel anyway.
    const top = variant === 'enercon' ? n.diameter_m * 0.34 : n.height_m / 2;
    const zTail = front + n.length_m * 0.78;
    const d = LAMP_M[detail];
    if (detail < 2) {
      for (const x of [-n.width_m * 0.3, n.width_m * 0.3]) {
        member(hardware, [x, top, zTail], [x, top + 0.45, zTail], 0.1, 0.1, true);
        sphere(lamps, [x, top + 0.45 + d / 2, zTail], d / 2, ROUND_SIDES[detail]);
      }
    } else {
      sphere(lamps, [0, top + d / 2, zTail], d / 2, ROUND_SIDES[detail]);
    }
  }
  return { shell, dark, hardware, lamps, floor };
}

// ---------------------------------------------------------------------------
// The tower
// ---------------------------------------------------------------------------

/**
 * The tubular steel tower from the ground to the yaw bearing, in the model's
 * root frame. Sections of it carry a tint: the day marking's red band, and on
 * an Enercon the green rings of the foot. `weather` puts the dirt at the foot
 * on afterwards.
 */
export function buildTower(spec, { detail, variant, marking, top }) {
  const out = empty();
  const sides = TOWER_SIDES[detail];
  const r0 = spec.tower_base_m / 2;
  const r1 = spec.tower_top_m / 2;
  const radius = (y) => lerp(r0, r1, y / top);

  // The tints, bottom up: `[from, to, colour]`.
  const bands = [];
  if (variant === 'enercon') {
    ENERCON_RINGS.forEach((colour, i) => {
      bands.push([i * ENERCON_RING_HEIGHT, (i + 1) * ENERCON_RING_HEIGHT, colour]);
    });
  }
  if (marking && top > TOWER_BAND_FROM + TOWER_BAND_HEIGHT + 5) {
    bands.push([TOWER_BAND_FROM, TOWER_BAND_FROM + TOWER_BAND_HEIGHT, RED]);
  }
  bands.sort((a, b) => a[0] - b[0]);
  const cuts = [0];
  for (const [from, to] of bands) cuts.push(from, to);
  cuts.push(top);
  const edges = [...new Set(cuts)].sort((a, b) => a - b);
  for (let i = 0; i + 1 < edges.length; i++) {
    const a = edges[i];
    const b = edges[i + 1];
    const mid = (a + b) / 2;
    const band = bands.find(([from, to]) => mid >= from && mid < to);
    tint(out, band ? band[2] : WHITE);
    tube(out, [0, a, 0], [0, b, 0], radius(a), radius(b), sides, false);
  }
  tint(out, WHITE);
  // The bottom is closed for the check, the top is closed under the bearing.
  cap(out, ring([0, 0, 0], r0, sides, [0, 1, 0]), [0, -1, 0]);
  cap(out, ring([0, top, 0], r1, sides, [0, 1, 0]), [0, 1, 0]);

  if (detail === 0) {
    // The flanges where the sections were bolted: every 20-odd metres a
    // hairline the paint does not hide, drawn as a ring a few centimetres
    // proud of the shell.
    const sections = Math.max(2, Math.round(top / 24));
    for (let i = 1; i < sections; i++) {
      const y = (top * i) / sections;
      tube(out, [0, y - 0.05, 0], [0, y + 0.05, 0], radius(y) + 0.035, radius(y) + 0.035, sides, true);
    }
    // The door at the foot, on the lee side, a step proud of the shell.
    const z = radius(1.5);
    tint(out, [0.42, 0.42, 0.44, 1]);
    quad(out, [-0.5, 0.35, z + 0.03], [0.5, 0.35, z + 0.03], [0.5, 2.5, z + 0.03], [-0.5, 2.5, z + 0.03]);
    tint(out, WHITE);
  }
  weather(out, { ground: 7.0, floor: 0.80, jitter: 0.0, seed: 3 });
  return out;
}

/**
 * A lattice tower: four battered legs with X bracing in panels, a galvanised
 * frame like the masts', and a short tube on top for the yaw bearing to sit
 * on — what a Fuhrländer or a 1990s Nordex stood on.
 */
export function buildLatticeTower(spec, { detail, top }) {
  const steel = empty();
  const shell = empty();
  const half0 = top / 11;
  const half1 = 0.9;
  const panelHeight = 4.5;
  const panels = Math.max(4, Math.round(top / panelHeight));
  const leg = [0.26, 0.3, 0.36, 0.5][detail];
  const brace = [0.12, 0.15, 0.2, 0.3][detail];
  const halfAt = (y) => lerp(half0, half1, y / top);
  const corners = [
    [1, 1],
    [-1, 1],
    [-1, -1],
    [1, -1],
  ];
  // Legs, one member per panel so the batter is a polyline.
  for (const [sx, sz] of corners) {
    for (let i = 0; i < panels; i++) {
      const y0 = (top * i) / panels;
      const y1 = (top * (i + 1)) / panels;
      member(steel, [sx * halfAt(y0), y0, sz * halfAt(y0)], [sx * halfAt(y1), y1, sz * halfAt(y1)], leg, leg, false);
    }
  }
  // Bracing: every panel at LOD0, every second at LOD1, every fourth at LOD2,
  // horizontals only at LOD3 — the same thinning the masts do.
  const every = [1, 2, 4, panels][detail];
  for (let i = 0; i < panels; i++) {
    const y0 = (top * i) / panels;
    const y1 = (top * (i + 1)) / panels;
    for (let f = 0; f < 4; f++) {
      const [ax, az] = corners[f];
      const [bx, bz] = corners[(f + 1) % 4];
      const a0 = [ax * halfAt(y0), y0, az * halfAt(y0)];
      const b0 = [bx * halfAt(y0), y0, bz * halfAt(y0)];
      const a1 = [ax * halfAt(y1), y1, az * halfAt(y1)];
      const b1 = [bx * halfAt(y1), y1, bz * halfAt(y1)];
      if (i % Math.max(1, Math.round(every / 2)) === 0 || detail === 0) {
        member(steel, a1, b1, brace, brace, false);
      }
      if (i % every === 0 && detail < 3) {
        member(steel, a0, b1, brace, brace, false);
        member(steel, b0, a1, brace, brace, false);
      }
    }
  }
  // The transition to the yaw bearing: a tube on the legs' head.
  const r = spec.tower_top_m / 2;
  tube(shell, [0, top - 0.6, 0], [0, top + 0.01, 0], half1 * 1.2, r, TOWER_SIDES[detail], true);
  weather(steel, { ground: 6.0, floor: 0.76, jitter: 0.05, seed: 5 });
  return { steel, shell, half0 };
}

/** The concrete under the tower: a disc the grass grows up to. */
export function buildFoundation(spec, { detail, variant, lattice }) {
  const out = empty();
  if (detail > 1) return out;
  const sides = TOWER_SIDES[detail];
  if (lattice) {
    for (const [sx, sz] of [[1, 1], [-1, 1], [-1, -1], [1, -1]]) {
      const x = sx * lattice;
      const z = sz * lattice;
      tube(out, [x, -0.4, z], [x, 0.3, z], 0.7, 0.6, Math.max(6, sides / 2), true);
    }
    return out;
  }
  const r = spec.tower_base_m / 2 + (variant === 'enercon' ? 1.6 : 1.2);
  tube(out, [0, -0.4, 0], [0, 0.25, 0], r, r - 0.15, sides, true);
  return out;
}

/**
 * The whole machine as parts by material, per node the glTF is built from.
 *
 * `top` is the height of the tower's head under the yaw bearing — the hub
 * height less the nacelle's half height and the bearing.
 */
export function buildTurbine(spec, { variant, detail }) {
  const marking = spec.marking;
  const n = spec.nacelle;
  const floor = variant === 'enercon' ? -n.diameter_m / 2 + 0.2 : -n.height_m / 2 + 0.05;
  const top = spec.hub_m + floor - 0.9;
  const lattice = variant === 'gitter';

  const tower = lattice ? buildLatticeTower(spec, { detail, top }) : null;
  const towerShell = lattice ? tower.shell : buildTower(spec, { detail, variant, marking, top });
  const foundation = buildFoundation(spec, { detail, variant, lattice: lattice ? tower.half0 : 0 });
  const nacelle = buildNacelle(spec, { detail, variant, marking });
  const rotor = buildRotor(spec, { detail, marking, variant });

  return {
    tower: { coating: towerShell, steel: lattice ? tower.steel : empty(), concrete: foundation },
    nacelle: { coating: merge([nacelle.shell, nacelle.hardware]), dark: nacelle.dark },
    lamps: nacelle.lamps,
    rotor,
    top,
  };
}

/**
 * How many triangles are wound the way their own normals point, and how many
 * against it — what back-face culling will show of a surface.
 *
 * The masts' `facing` judges a face against the centre of its piece, which is
 * right for a bar and wrong for a coned blade: at the tip the whole piece's
 * centre is behind the face whichever way the face points. A lofted surface
 * orients its normals off the ring it was built from, so the honest question
 * is whether each face's winding agrees with them.
 */
export function windingAgrees(geometry) {
  const { positions: p, normals: n, indices: idx } = geometry;
  let agree = 0;
  let against = 0;
  for (let t = 0; t < idx.length; t += 3) {
    const [i, j, k] = [idx[t], idx[t + 1], idx[t + 2]];
    const a = [p[i * 3], p[i * 3 + 1], p[i * 3 + 2]];
    const b = [p[j * 3], p[j * 3 + 1], p[j * 3 + 2]];
    const c = [p[k * 3], p[k * 3 + 1], p[k * 3 + 2]];
    const face = cross(sub(b, a), sub(c, a));
    if (length(face) < 1e-12) continue;
    const mean = [
      n[i * 3] + n[j * 3] + n[k * 3],
      n[i * 3 + 1] + n[j * 3 + 1] + n[k * 3 + 1],
      n[i * 3 + 2] + n[j * 3 + 2] + n[k * 3 + 2],
    ];
    if (dot(face, mean) >= 0) agree++;
    else against++;
  }
  return { agree, against };
}

/** The finest thing a level draws [m] — what the hand-over distances are cut on. */
export function finestMember(spec, detail) {
  // The blade's outer chord is what stops resolving first; below it the
  // aerofoil section is a line and a level with a section is wasted.
  return chordAt(spec.blade, 0.8) * [1.0, 1.0, 1.0, 1.0][detail];
}

export { chordAt, lerp };
