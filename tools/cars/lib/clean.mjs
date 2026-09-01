// Turning a generated vehicle mesh into a prop a game can afford.
//
// The models this pipeline is fed are photogrammetry-style: one nameless mesh,
// one texture atlas, and everything the generator thought it saw. Three things
// are wrong with that for a car park, and all three have to be found by
// looking at the geometry, because there are no names to go by:
//
//  1. **A slab under the car.** The generator stands its subject on a plinth.
//     Left in, every car in the module floats a few centimetres up and drags a
//     grey rectangle around with it.
//  2. **An interior.** Seats, a floor, the inside of the shell. Never seen
//     through a dark window, paid for on every one of two hundred instances.
//  3. **Windows that are photographs of windows.** A reflection baked into the
//     texture reads as a smear at any distance, and it dates the whole car.
//     What reads as glass is a dark, smooth, untextured surface.
//
// Each step is separate and each can be looked at on its own — `--preview`
// draws what was thrown away next to what was kept, which is the only way to
// be sure a rule that works on a hatchback has not eaten a van's roof.

import { visibleFaces } from './visible.mjs';

/**
 * Which vertices are the same point.
 *
 * The meshes arrive with every triangle carrying its own three corners — FBX
 * splits attributes per polygon vertex and this pipeline keeps that — so no two
 * faces share an index and no question about topology can be asked until the
 * positions are welded. Everything here that grows a patch over shared edges
 * goes through this first.
 */
export function weldMap(positions, tolerance = 1e-5) {
  const map = new Map();
  const remap = new Array(positions.length / 3);
  for (let i = 0; i < positions.length / 3; i++) {
    const key =
      `${Math.round(positions[i * 3] / tolerance)},` +
      `${Math.round(positions[i * 3 + 1] / tolerance)},` +
      `${Math.round(positions[i * 3 + 2] / tolerance)}`;
    let at = map.get(key);
    if (at === undefined) {
      at = map.size;
      map.set(key, at);
    }
    remap[i] = at;
  }
  return remap;
}

/** Faces as index triples, from a flat index list. */
export function facesOf(indices) {
  const faces = [];
  for (let t = 0; t < indices.length; t += 3) {
    faces.push([indices[t], indices[t + 1], indices[t + 2]]);
  }
  return faces;
}

/** The axis-aligned box of a set of positions. */
export function bounds(positions) {
  const min = [Infinity, Infinity, Infinity];
  const max = [-Infinity, -Infinity, -Infinity];
  for (let i = 0; i < positions.length; i += 3) {
    for (let c = 0; c < 3; c++) {
      min[c] = Math.min(min[c], positions[i + c]);
      max[c] = Math.max(max[c], positions[i + c]);
    }
  }
  return { min, max, size: [0, 1, 2].map((c) => max[c] - min[c]) };
}

/** Area, unit normal and centroid of one face. */
export function facet(positions, [a, b, c]) {
  const ux = positions[b * 3] - positions[a * 3];
  const uy = positions[b * 3 + 1] - positions[a * 3 + 1];
  const uz = positions[b * 3 + 2] - positions[a * 3 + 2];
  const vx = positions[c * 3] - positions[a * 3];
  const vy = positions[c * 3 + 1] - positions[a * 3 + 1];
  const vz = positions[c * 3 + 2] - positions[a * 3 + 2];
  const n = [uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx];
  const length = Math.hypot(...n);
  const centre = [0, 1, 2].map(
    (k) => (positions[a * 3 + k] + positions[b * 3 + k] + positions[c * 3 + k]) / 3,
  );
  return {
    area: length / 2,
    normal: length > 1e-12 ? n.map((v) => v / length) : [0, 1, 0],
    centre,
  };
}

/**
 * The slab the generator stood the car on.
 *
 * Flat, horizontal, and in the bottom two per cent of the model — which is
 * *below the tyres*, because a plinth is what the tyres rest on. A car's own
 * floor pan is horizontal too but sits a hand's width up, and its wheels are
 * the lowest thing it has.
 *
 * Returns a flag per face. Where a model has no plinth this finds a handful of
 * tyre-bottom triangles and says so; the caller decides on the share.
 */
export function groundPlate(positions, faces, { band = 0.02, flat = 0.85 } = {}) {
  const box = bounds(positions);
  const height = box.size[1] || 1;
  const limit = box.min[1] + height * band;
  const flags = new Uint8Array(faces.length);
  let area = 0;
  let total = 0;
  faces.forEach((face, f) => {
    const { area: a, normal, centre } = facet(positions, face);
    total += a;
    if (centre[1] <= limit && Math.abs(normal[1]) >= flat) {
      flags[f] = 1;
      area += a;
    }
  });
  return { flags, share: total > 0 ? area / total : 0 };
}

/**
 * Everything nobody will ever see, by looking from every direction that a
 * viewer can stand in (`visible.mjs`).
 */
export function hidden(positions, faces, options = {}) {
  const { seen, towards } = visibleFaces(positions, faces, options);
  const flags = new Uint8Array(faces.length);
  for (let f = 0; f < faces.length; f++) flags[f] = seen[f] ? 0 : 1;
  return { flags, towards };
}

/**
 * Turns the triangles that were wound inside out the right way round.
 *
 * A generated mesh is not consistently wound: three per cent of these come
 * with their corners the other way about. With single-sided materials — which
 * is what a car wants, since nothing should ever see the inside of one — every
 * such triangle is a hole, and through the hole you see the far side of the
 * shell lit from behind. On screen that reads as a shard of the wrong shade
 * lying on the paint, scattered over the bonnet and the roof, and no amount of
 * looking at the texture explains it.
 *
 * The test is the one thing that is known for certain about a face that was
 * seen: it was seen *from* somewhere, so its outward side is the side facing
 * that somewhere. `towards` comes out of the visibility pass, which has
 * already looked from every direction there is.
 */
export function fixWinding(positions, faces, towards) {
  let flipped = 0;
  faces.forEach((face, f) => {
    const view = [towards[f * 3], towards[f * 3 + 1], towards[f * 3 + 2]];
    const length = Math.hypot(...view);
    // Never seen from anywhere: nothing to go on, and nothing that will be
    // looked at either.
    if (length < 1e-9) return;
    const { normal } = facet(positions, face);
    if (normal[0] * view[0] + normal[1] * view[1] + normal[2] * view[2] >= 0) return;
    const swap = face[1];
    face[1] = face[2];
    face[2] = swap;
    flipped++;
  });
  return flipped;
}

/**
 * The specks: parts of the mesh that are too small to be anything.
 *
 * A generated model comes with litter — a dozen triangles floating above the
 * roof, a shard beside a wheel arch, a fleck under the sill. It is a rounding
 * error in the surface area and it does real damage, because everything this
 * pipeline decides is measured off the bounding box: the scale comes from the
 * length, the ride height from the lowest point, and the waistline from the
 * height. A speck a hand's width above the roof made a Polo nineteen per cent
 * too tall — not visibly, but in every number derived from it.
 *
 * Islands are grown over shared edges, so the wheels, the mirrors and the
 * glass stay whole; only what is under `minShare` of the total surface goes.
 * The threshold is deliberately far below the smallest real part: a wing
 * mirror is a per cent of a car, a speck is a hundredth of one.
 */
export function islandsOf(positions, faces) {
  const same = weldMap(positions);
  const byEdge = new Map();
  faces.forEach((face, f) => {
    const w = face.map((v) => same[v]);
    for (const [a, b] of [
      [w[0], w[1]],
      [w[1], w[2]],
      [w[2], w[0]],
    ]) {
      const key = a < b ? `${a},${b}` : `${b},${a}`;
      if (!byEdge.has(key)) byEdge.set(key, []);
      byEdge.get(key).push(f);
    }
  });
  const island = new Int32Array(faces.length).fill(-1);
  const areas = [];
  let total = 0;
  for (let f = 0; f < faces.length; f++) {
    if (island[f] >= 0) continue;
    const id = areas.length;
    const stack = [f];
    island[f] = id;
    let area = 0;
    while (stack.length) {
      const current = stack.pop();
      area += facet(positions, faces[current]).area;
      const w = faces[current].map((v) => same[v]);
      for (const [a, b] of [
        [w[0], w[1]],
        [w[1], w[2]],
        [w[2], w[0]],
      ]) {
        const key = a < b ? `${a},${b}` : `${b},${a}`;
        for (const other of byEdge.get(key) ?? []) {
          if (island[other] < 0) {
            island[other] = id;
            stack.push(other);
          }
        }
      }
    }
    areas.push(area);
    total += area;
  }
  return { island, areas, total };
}

/** Faces belonging to an island under `minShare` of the surface. */
export function specks(positions, faces, { minShare = 0.0004 } = {}) {
  const { island, areas, total } = islandsOf(positions, faces);
  const flags = new Uint8Array(faces.length);
  let dropped = 0;
  for (let f = 0; f < faces.length; f++) {
    if (areas[island[f]] / total < minShare) {
      flags[f] = 1;
      dropped++;
    }
  }
  return { flags, dropped, islands: areas.length };
}

/**
 * The box of the vehicle, ignoring anything too small to be part of it.
 *
 * Every number this pipeline derives comes off the bounding box — the scale
 * from the length, the ride height from the lowest point, the waistline from
 * the height — so a stray fleck a hand's width above the roof does not merely
 * sit there: it makes the car nineteen per cent too tall in every calculation
 * that follows. Deleting the fleck is one answer and this is the better one,
 * because the size of a car should be decided by the parts of it that *are*
 * the car, whatever else is lying about.
 *
 * A per cent of the surface is the line. A wing mirror is above it, a badge
 * below, and neither should ever have a say in how long a car is.
 */
export function bodyBounds(positions, faces, { minShare = 0.01 } = {}) {
  const { island, areas, total } = islandsOf(positions, faces);
  const min = [Infinity, Infinity, Infinity];
  const max = [-Infinity, -Infinity, -Infinity];
  let counted = 0;
  faces.forEach((face, f) => {
    if (areas[island[f]] / total < minShare) return;
    counted++;
    for (const v of face) {
      for (let c = 0; c < 3; c++) {
        min[c] = Math.min(min[c], positions[v * 3 + c]);
        max[c] = Math.max(max[c], positions[v * 3 + c]);
      }
    }
  });
  // Nothing was big enough — a mesh of many equal pieces. Then everything has
  // a say, which is what the plain box does.
  if (!counted) return bounds(positions);
  return { min, max, size: [0, 1, 2].map((c) => max[c] - min[c]) };
}

/**
 * Otsu's threshold: the brightness that best splits a set in two.
 *
 * The glasshouse of a vehicle is two populations — paint and glass — and where
 * the line between them falls depends on the car: one atlas is a silver car in
 * daylight, the next a dark blue one in shade, and a fixed threshold that
 * suits the first classifies the whole of the second as window. Otsu finds the
 * split that leaves the least variance inside the two halves, which is exactly
 * the question being asked, and it asks it of every vehicle separately.
 *
 * Weighted by area, because a window is a few large faces and the trim around
 * it is many small ones; counting faces would let the trim outvote the glass.
 */
export function otsu(values, weights, bins = 64) {
  const histogram = new Float64Array(bins);
  let total = 0;
  for (let i = 0; i < values.length; i++) {
    const at = Math.min(bins - 1, Math.max(0, Math.floor(values[i] * bins)));
    histogram[at] += weights[i];
    total += weights[i];
  }
  if (total <= 0) return null;
  let sum = 0;
  for (let b = 0; b < bins; b++) sum += ((b + 0.5) / bins) * histogram[b];
  let belowWeight = 0;
  let belowSum = 0;
  let best = { variance: -1, threshold: null };
  for (let b = 0; b < bins; b++) {
    belowWeight += histogram[b];
    if (belowWeight <= 0) continue;
    const aboveWeight = total - belowWeight;
    if (aboveWeight <= 0) break;
    belowSum += ((b + 0.5) / bins) * histogram[b];
    const belowMean = belowSum / belowWeight;
    const aboveMean = (sum - belowSum) / aboveWeight;
    // Between-class variance: the bigger, the cleaner the split.
    const variance = belowWeight * aboveWeight * (belowMean - aboveMean) ** 2;
    if (variance > best.variance) {
      best = { variance, threshold: (b + 1) / bins, dark: belowMean, light: aboveMean };
    }
  }
  return best.threshold === null ? null : best;
}

/**
 * The glasshouse: which faces are windows.
 *
 * Found by shape, since there is nothing else to go by. A window is
 *
 *  * above the waistline — everything below it is bodywork,
 *  * not the roof: a face turned to the sky is painted, a window is steep,
 *  * and part of a large flat run rather than a lone triangle, which is what
 *    keeps the pillars and the mirrors out of it.
 *
 * `waist` is where the glasshouse starts, as a share of the vehicle's height.
 * It is a per-vehicle number in the catalogue because a van's waistline is
 * much lower down its body than a saloon's.
 */
export function glasshouse(
  positions,
  faces,
  { waist = 0.5, steep = 0.8, minPatch = 12, luminance = null, spread = null, dark = 0.42 } = {},
) {
  const box = bounds(positions);
  const height = box.size[1] || 1;
  const line = box.min[1] + height * waist;
  const candidate = new Uint8Array(faces.length);
  const info = faces.map((face) => facet(positions, face));
  // Where the texture can be asked, the line between paint and glass is found
  // in the vehicle's own glasshouse rather than assumed. Shape alone cannot
  // tell a window from the shoulder of the roof above it — same height, same
  // angle — but the texture can: one is a photograph of glass and the other a
  // photograph of paint.
  const upper = [];
  faces.forEach((face, f) => {
    const { normal, centre } = info[f];
    if (centre[1] < line) return;
    if (Math.abs(normal[1]) > steep) return;
    upper.push(f);
  });
  let limit = dark;
  let steepEnough = () => true;
  // How far the readings across one face may disagree before it is treated as
  // a face that lies across the edge of the window rather than inside it.
  // Unset until Otsu has said how far apart paint and glass are on this
  // vehicle: the answer is half that distance, so it is the same measurement
  // the threshold comes from and not a second number to tune.
  let mixed = Infinity;
  if (luminance && upper.length > 8) {
    const split = otsu(
      upper.map((f) => luminance(f)),
      upper.map((f) => info[f].area),
    );
    // Otsu answers even when there is nothing to split. Two tests on its
    // answer: it has to land where a window plausibly is, and the two halves
    // it found have to actually differ. A dark car photographed in shade has a
    // glasshouse of one brightness from roof to sill, and forcing a split on
    // it paints the roof black — so where the halves are close, the texture is
    // not asked and only what stands nearly upright counts as glass.
    const separated = split && split.light - split.dark >= 0.1;
    if (split && separated && split.threshold > 0.05 && split.threshold < 0.75) {
      limit = split.threshold;
      if (spread) mixed = (split.light - split.dark) / 2;
    } else {
      limit = 1;
      steepEnough = (f) => Math.abs(info[f].normal[1]) <= 0.45;
    }
  }
  for (const f of upper) {
    if (luminance && luminance(f) > limit) continue;
    // A triangle that straddles the rubber is part window and part pillar, and
    // it can only be given wholly to one of them. Given to the glass it is a
    // dark spike reaching out across the paint, which is what anyone looking
    // at the car sees first. Given to the paint it is a triangle's width of
    // the baked photograph left at the edge of the window — which is a
    // photograph of the seal, and looks like the seal.
    if (spread && spread(f) > mixed) continue;
    if (!steepEnough(f)) continue;
    candidate[f] = 1;
  }

  // Grown into patches over shared edges, so a run of window survives and a
  // stray triangle on a mirror housing does not.
  const same = weldMap(positions);
  const byEdge = new Map();
  faces.forEach((face, f) => {
    if (!candidate[f]) return;
    const w = face.map((v) => same[v]);
    for (const [a, b] of [
      [w[0], w[1]],
      [w[1], w[2]],
      [w[2], w[0]],
    ]) {
      const key = a < b ? `${a},${b}` : `${b},${a}`;
      if (!byEdge.has(key)) byEdge.set(key, []);
      byEdge.get(key).push(f);
    }
  });
  const patch = new Int32Array(faces.length).fill(-1);
  const sizes = [];
  for (let f = 0; f < faces.length; f++) {
    if (!candidate[f] || patch[f] >= 0) continue;
    const id = sizes.length;
    const stack = [f];
    let count = 0;
    patch[f] = id;
    while (stack.length) {
      const current = stack.pop();
      count++;
      const w = faces[current].map((v) => same[v]);
      for (const [a, b] of [
        [w[0], w[1]],
        [w[1], w[2]],
        [w[2], w[0]],
      ]) {
        const key = a < b ? `${a},${b}` : `${b},${a}`;
        for (const other of byEdge.get(key) ?? []) {
          if (patch[other] < 0 && candidate[other]) {
            patch[other] = id;
            stack.push(other);
          }
        }
      }
    }
    sizes.push(count);
  }
  const flags = new Uint8Array(faces.length);
  for (let f = 0; f < faces.length; f++) {
    if (patch[f] >= 0 && sizes[patch[f]] >= minPatch) flags[f] = 1;
  }
  return flags;
}

/** A mesh with the flagged faces taken out, and its vertices compacted. */
export function drop(mesh, flags) {
  const keep = [];
  const keepGroup = [];
  mesh.faces.forEach((face, f) => {
    if (flags[f]) return;
    keep.push(face);
    if (mesh.groups) keepGroup.push(mesh.groups[f]);
  });
  return compact({ ...mesh, faces: keep, groups: mesh.groups ? keepGroup : undefined });
}

/** Renumbers the vertices so only the used ones remain. */
export function compact(mesh) {
  const used = new Map();
  const positions = [];
  const normals = mesh.normals ? [] : null;
  const uvs = mesh.uvs ? [] : null;
  const faces = mesh.faces.map((face) =>
    face.map((v) => {
      let at = used.get(v);
      if (at === undefined) {
        at = positions.length / 3;
        used.set(v, at);
        positions.push(mesh.positions[v * 3], mesh.positions[v * 3 + 1], mesh.positions[v * 3 + 2]);
        if (normals) normals.push(mesh.normals[v * 3], mesh.normals[v * 3 + 1], mesh.normals[v * 3 + 2]);
        if (uvs) uvs.push(mesh.uvs[v * 2], mesh.uvs[v * 2 + 1]);
      }
      return at;
    }),
  );
  return { ...mesh, positions, normals, uvs, faces };
}
