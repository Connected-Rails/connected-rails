// Meshing the ez-tree skeleton, and the geometry arithmetic around it.
//
// ez-tree can mesh its own skeleton at several detail levels (`sectionStride`,
// `segmentFactor`, `leafStride`), but every branch it grew keeps at least one
// ring of three segments — a beech with three levels of branching is three
// hundred branches, so the coarsest level it offers still costs eleven hundred
// triangles before a single leaf. What a wood needs at six hundred metres is
// the trunk, the limbs and the canopy, and nothing of the twigs.
//
// So the pipeline meshes the skeleton itself. That is the same ring-and-quad
// construction as `Tree.#meshBranch`/`#meshLeaf` (MIT, Daniel Greenheck), with
// two additions:
//
//   * `minRadius` drops every branch thinner than a limit. The leaves stay
//     where they were, so the canopy keeps its shape while the twigs holding
//     it up disappear.
//   * the bark wraps exactly once around a branch, so `u` stays inside `[0, 1]`
//     and both materials fit in one atlas — one draw call per tree instead of
//     two.
//
// `verifyAgainstEzTree` checks the construction against the library's own
// output at full detail, so a change on either side is caught by the build.

/** Rotates `v` by a three.js Euler in XYZ order. */
export function applyEuler(v, e) {
  const c1 = Math.cos(e.x / 2);
  const c2 = Math.cos(e.y / 2);
  const c3 = Math.cos(e.z / 2);
  const s1 = Math.sin(e.x / 2);
  const s2 = Math.sin(e.y / 2);
  const s3 = Math.sin(e.z / 2);
  const qx = s1 * c2 * c3 + c1 * s2 * s3;
  const qy = c1 * s2 * c3 - s1 * c2 * s3;
  const qz = c1 * c2 * s3 + s1 * s2 * c3;
  const qw = c1 * c2 * c3 - s1 * s2 * s3;
  const ix = qw * v[0] + qy * v[2] - qz * v[1];
  const iy = qw * v[1] + qz * v[0] - qx * v[2];
  const iz = qw * v[2] + qx * v[1] - qy * v[0];
  const iw = -qx * v[0] - qy * v[1] - qz * v[2];
  return [
    ix * qw + iw * -qx + iy * -qz - iz * -qy,
    iy * qw + iw * -qy + iz * -qx - ix * -qz,
    iz * qw + iw * -qz + ix * -qy - iy * -qx,
  ];
}

/** Rotation about the Y axis, used for the second quad of a crossed leaf. */
function rotateY(v, angle) {
  const c = Math.cos(angle);
  const s = Math.sin(angle);
  return [v[0] * c + v[2] * s, v[1], -v[0] * s + v[2] * c];
}

/** An empty geometry in the pipeline's own layout. */
export function emptyGeometry() {
  return { positions: [], normals: [], uvs: [], indices: [] };
}

export function triangleCount(geometry) {
  return geometry.indices.length / 3;
}

/**
 * Meshes every branch of the skeleton into `out`.
 *
 * @param {object} skeleton `tree.skeleton`
 * @param {object} detail `{ sectionStride, segmentFactor, minRadius }`
 */
export function meshBranches(skeleton, detail = {}) {
  const sectionStride = Math.max(1, Math.floor(detail.sectionStride ?? 1));
  const segmentFactor = detail.segmentFactor ?? 1;
  // `minRadius` is a share of the trunk, not a length: a hazel's twigs are as
  // thick as a spruce's limbs, and a level that drops "everything under nine
  // centimetres" would leave one species bare and the next untouched.
  const trunk = skeleton.branches[0]?.baseRadius ?? 1;
  const minRadius = (detail.minRadius ?? 0) * trunk;
  const out = emptyGeometry();

  for (const branch of skeleton.branches) {
    if (branch.baseRadius < minRadius) continue;
    const segments = Math.max(3, Math.round(branch.segmentCount * segmentFactor));
    const sections = branch.sections;
    const sampled = [];
    for (let i = 0; i < sections.length; i += sectionStride) sampled.push(sections[i]);
    if ((sections.length - 1) % sectionStride !== 0) sampled.push(sections[sections.length - 1]);

    const offset = out.positions.length / 3;
    for (let k = 0; k < sampled.length; k++) {
      const section = sampled[k];
      for (let j = 0; j <= segments; j++) {
        const wrapped = j % segments;
        const angle = (2 * Math.PI * wrapped) / segments;
        const dir = [Math.cos(angle), 0, Math.sin(angle)];
        const local = [dir[0] * section.radius, 0, dir[2] * section.radius];
        const p = applyEuler(local, section.orientation);
        const n = applyEuler(dir, section.orientation);
        const len = Math.hypot(n[0], n[1], n[2]) || 1;
        out.positions.push(
          p[0] + section.origin.x,
          p[1] + section.origin.y,
          p[2] + section.origin.z,
        );
        out.normals.push(n[0] / len, n[1] / len, n[2] / len);
        // One wrap of the bark around the branch — `u` never leaves [0, 1],
        // which is what lets bark and foliage share one atlas. The duplicated
        // seam vertex at `j === segments` carries u = 1.
        out.uvs.push(j / segments, k % 2 === 0 ? 0 : 1);
      }
    }
    const n = segments + 1;
    for (let i = 0; i < sampled.length - 1; i++) {
      for (let j = 0; j < segments; j++) {
        const v1 = offset + i * n + j;
        const v2 = offset + i * n + j + 1;
        out.indices.push(v1, v1 + n, v2, v2, v1 + n, v2 + n);
      }
    }
  }
  return out;
}

/**
 * Meshes the leaves into their own geometry.
 *
 * `rects` are the atlas rectangles the cards sit in; the quads take them in
 * turn, so a canopy is built from two different sprays instead of one repeated
 * card — variation inside a single tree for the price of atlas space that was
 * lying idle anyway.
 *
 * @param {object} detail `{ leafStride, leafScale, billboard, rects }`
 */
export function meshLeaves(skeleton, detail = {}) {
  const stride = Math.max(1, Math.floor(detail.leafStride ?? 1));
  const scale = detail.leafScale ?? 1;
  const double = (detail.billboard ?? 'double') === 'double';
  const rects = detail.rects ?? [{ u: 0, v: 0, w: 1, h: 1 }];
  const out = emptyGeometry();

  // The centre of the canopy, for the normals below.
  const centre = [0, 0, 0];
  for (const leaf of skeleton.leaves) {
    centre[0] += leaf.origin.x;
    centre[1] += leaf.origin.y;
    centre[2] += leaf.origin.z;
  }
  if (skeleton.leaves.length) {
    for (let c = 0; c < 3; c++) centre[c] /= skeleton.leaves.length;
  }

  for (let i = 0; i < skeleton.leaves.length; i += stride) {
    const leaf = skeleton.leaves[i];
    const size = leaf.size * scale;
    for (const rotation of double ? [0, Math.PI / 2] : [0]) {
      const base = out.positions.length / 3;
      const corners = [
        [-size / 2, size, 0],
        [-size / 2, 0, 0],
        [size / 2, 0, 0],
        [size / 2, size, 0],
      ].map((c) => applyEuler(rotateY(c, rotation), leaf.orientation));
      const face = applyEuler([0, 0, 1], leaf.orientation);
      for (const c of corners) {
        const p = [c[0] + leaf.origin.x, c[1] + leaf.origin.y, c[2] + leaf.origin.z];
        out.positions.push(p[0], p[1], p[2]);
        // **The normal points out of the canopy, not out of the card.**
        //
        // A card's own facing is where ez-tree happened to put it, and two
        // cards beside each other can face opposite ways. Lit from the side —
        // a low sun, which is most of a working day — one of them is then
        // blown out and its neighbour is black, and a crown two hundred metres
        // off reads as a mosaic of hard bright and dark blocks rather than as
        // foliage. Taking the normal from the vertex's direction out of the
        // middle of the canopy instead makes the crown shade like the rounded
        // mass it is: bright where it faces the sun, dark where it turns away,
        // and smooth in between. It is the leaves' *arrangement* that carries
        // the shading, which is how a real canopy works too.
        const out3 = [p[0] - centre[0], p[1] - centre[1], p[2] - centre[2]];
        const radius = Math.hypot(out3[0], out3[1], out3[2]) || 1;
        // Mostly the canopy, a third of the card. All canopy and the sunlit
        // side of a crown becomes one flat tone — every card there shares a
        // normal, so a tree at dawn is a single orange blob. All card and
        // neighbouring leaves face opposite ways and the crown breaks into a
        // mosaic of blown-out and black. The mixture keeps the rounded mass and
        // gives each leaf enough of its own to read as a leaf. The upward bias
        // is the sky: a canopy's underside is never as dark as a sphere's.
        const n = [
          (out3[0] / radius) * 0.7 + face[0] * 0.35,
          (out3[1] / radius) * 0.7 + face[1] * 0.35 + 0.3,
          (out3[2] / radius) * 0.7 + face[2] * 0.35,
        ];
        const len = Math.hypot(n[0], n[1], n[2]) || 1;
        out.normals.push(n[0] / len, n[1] / len, n[2] / len);
      }
      const r = rects[Math.floor(i / stride) % rects.length];
      out.uvs.push(
        r.u, r.v + r.h,
        r.u, r.v,
        r.u + r.w, r.v,
        r.u + r.w, r.v + r.h,
      );
      out.indices.push(base, base + 1, base + 2, base, base + 2, base + 3);
    }
  }
  return out;
}

/** Multiplies every position by `factor` (the model's metres per ez-tree unit). */
export function scaleGeometry(geometry, factor) {
  for (let i = 0; i < geometry.positions.length; i++) geometry.positions[i] *= factor;
  return geometry;
}

/** Maps the `[0, 1]²` UVs into an atlas rectangle `{ u, v, w, h }`. */
export function remapUv(geometry, rect) {
  for (let i = 0; i < geometry.uvs.length; i += 2) {
    geometry.uvs[i] = rect.u + geometry.uvs[i] * rect.w;
    geometry.uvs[i + 1] = rect.v + geometry.uvs[i + 1] * rect.h;
  }
  return geometry;
}

/** Concatenates geometries into one, offsetting the indices. */
export function mergeGeometries(parts) {
  const out = emptyGeometry();
  for (const part of parts) {
    const offset = out.positions.length / 3;
    // Element by element: spreading a canopy's hundred thousand floats into
    // `push` overflows the argument stack.
    for (const v of part.positions) out.positions.push(v);
    for (const v of part.normals) out.normals.push(v);
    for (const v of part.uvs) out.uvs.push(v);
    for (const index of part.indices) out.indices.push(index + offset);
  }
  return out;
}

/** Axis-aligned bounds as `{ min: [x, y, z], max: [x, y, z] }`. */
export function bounds(geometry) {
  const min = [Infinity, Infinity, Infinity];
  const max = [-Infinity, -Infinity, -Infinity];
  for (let i = 0; i < geometry.positions.length; i += 3) {
    for (let c = 0; c < 3; c++) {
      const v = geometry.positions[i + c];
      if (v < min[c]) min[c] = v;
      if (v > max[c]) max[c] = v;
    }
  }
  return { min, max };
}

/**
 * The crossed quads of the coarsest level: `blades` planes through the trunk,
 * evenly turned about the up axis, all sampling the same impostor rectangle.
 * Four triangles where a meshed tree costs three hundred.
 *
 * Two is the least that works. **One is not**: a single fixed quad vanishes the
 * moment the camera looks along it, and on a railway the angle to a given tree
 * sweeps through every value as the train passes it — a wood built of single
 * quads flickers out of existence and back. The price of the pair is a seam:
 * whichever blade is edge-on is drawn as a narrow strip *through* the other,
 * showing the tree's picture squeezed into a few pixels, which reads as a slice
 * through the crown. Nothing removes it short of turning the quad to face the
 * camera in a shader, so the level is simply held far enough away that the
 * strip is under a pixel wide (see `lodDistances` in build_trees.mjs).
 */
export function crossQuads(width, height, yBase, rect, blades = 2) {
  const out = emptyGeometry();
  for (let b = 0; b < blades; b++) {
    const angle = (Math.PI * b) / blades;
    const dx = Math.cos(angle) * width * 0.5;
    const dz = Math.sin(angle) * width * 0.5;
    const base = out.positions.length / 3;
    const corners = [
      [-dx, yBase + height, -dz],
      [-dx, yBase, -dz],
      [dx, yBase, dz],
      [dx, yBase + height, dz],
    ];
    // **One normal for the whole blade: the direction the blade faces.**
    //
    // A blade is a flat quad, and a flat quad has one normal. Giving each
    // corner its own — outwards from the trunk, which is the obvious thing —
    // lights the left half of the blade differently from the right and splits
    // every tree down the middle with a dead straight vertical line. That was
    // the seam, and it was never the crossing of the two blades.
    //
    // Straight up instead is worse in another way: a billboard with an up
    // normal has no front and no back, so it does not darken when the sun is
    // behind it. It simply drinks the sky, and at dawn a backlit wood glows
    // orange. The face direction keeps the front and the back — the renderer
    // flips it for whichever side is seen, so a tree between the camera and a
    // low sun goes dark, as it should.
    //
    // No upward component, deliberately. `doubleSided` negates the whole
    // normal on the far side, and an upward tilt would come back as a downward
    // one — the same wood lit from below when seen from the other side. What
    // the tree loses in overhead light it gets back from the picture, which is
    // baked with its own relief (`lib/impostor.mjs`).
    const normal = [-Math.sin(angle), 0, Math.cos(angle)];
    for (const c of corners) {
      out.positions.push(c[0], c[1], c[2]);
      out.normals.push(normal[0], normal[1], normal[2]);
    }
    out.uvs.push(
      rect.u, rect.v,
      rect.u, rect.v + rect.h,
      rect.u + rect.w, rect.v + rect.h,
      rect.u + rect.w, rect.v,
    );
    out.indices.push(base, base + 1, base + 2, base, base + 2, base + 3);
  }
  return out;
}

/**
 * Compares the pipeline's own meshing against ez-tree's at full detail.
 * Returns the largest positional difference [ez-tree units]; the caller fails
 * the build if it is anything but rounding.
 */
export function verifyAgainstEzTree(tree) {
  const reference = tree.createGeometry({});
  const mine = meshBranches(tree.skeleton, {});
  const theirs = reference.branches.attributes.position.array;
  if (theirs.length !== mine.positions.length) {
    return { error: `vertex count ${mine.positions.length / 3} vs ${theirs.length / 3}` };
  }
  let worst = 0;
  for (let i = 0; i < theirs.length; i++) {
    worst = Math.max(worst, Math.abs(theirs[i] - mine.positions[i]));
  }
  return { worst };
}
