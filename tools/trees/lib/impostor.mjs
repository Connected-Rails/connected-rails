// Baking the coarsest level: a picture of the tree, drawn from its own mesh.
//
// Past six hundred metres a twenty-five metre tree is forty pixels tall and a
// meshed crown is pure waste, so the last level is a pair of crossed quads
// showing an image of the tree. That image is not painted by hand and not
// screenshotted from a viewer — it is the level-1 geometry rasterised
// orthographically here, sampling the very atlas the near levels sample. The
// silhouette, the colour and the density of the canopy therefore agree with
// what the tree looked like a metre before the switch.
//
// The shading is all but flat — a hint of the canopy's own occlusion, no sun
// direction, and nothing that the game's own lighting of the quads would then
// apply a second time. A wood lit twice reads as a dark band behind a bright
// one at exactly the distance the levels hand over.

import { Surface } from './raster.mjs';

/**
 * @param {object} geometry positions/normals/uvs/indices, in metres
 * @param {Surface} atlas the texture the geometry samples
 * @param {number} size edge length of the baked image [texels]
 * @returns {{surface: Surface, width: number, height: number, base: number}}
 *   the image plus the size of the tree it was drawn from [m]
 */
export function bakeImpostor(geometry, atlas, size) {
  const { positions, normals, uvs, indices } = geometry;
  let minX = Infinity;
  let maxX = -Infinity;
  let minY = Infinity;
  let maxY = -Infinity;
  let minZ = Infinity;
  let maxZ = -Infinity;
  for (let i = 0; i < positions.length; i += 3) {
    minX = Math.min(minX, positions[i]);
    maxX = Math.max(maxX, positions[i]);
    minY = Math.min(minY, positions[i + 1]);
    maxY = Math.max(maxY, positions[i + 1]);
    minZ = Math.min(minZ, positions[i + 2]);
    maxZ = Math.max(maxZ, positions[i + 2]);
  }
  // The quads are square on the trunk, so the picture has to be taken about
  // the same axis: half width to each side of x = 0.
  const half = Math.max(Math.abs(minX), Math.abs(maxX), Math.abs(minZ), Math.abs(maxZ));
  const width = half * 2;
  const height = maxY - minY;

  const surface = new Surface(size, size);
  const depth = new Float32Array(size * size).fill(Infinity);

  // World to image: x ∈ [-half, half] → [0, size], y ∈ [minY, maxY] → [size, 0].
  const px = (x) => ((x + half) / width) * size;
  const py = (y) => (1 - (y - minY) / height) * size;

  const tri = [0, 0, 0];
  for (let t = 0; t < indices.length; t += 3) {
    tri[0] = indices[t];
    tri[1] = indices[t + 1];
    tri[2] = indices[t + 2];
    const sx = [];
    const sy = [];
    const sz = [];
    for (let k = 0; k < 3; k++) {
      const i = tri[k] * 3;
      sx.push(px(positions[i]));
      sy.push(py(positions[i + 1]));
      // Depth towards the camera at −z; nearer is smaller.
      sz.push(-positions[i + 2]);
    }
    const area = (sx[1] - sx[0]) * (sy[2] - sy[0]) - (sx[2] - sx[0]) * (sy[1] - sy[0]);
    if (Math.abs(area) < 1e-9) continue;

    const x0 = Math.max(0, Math.floor(Math.min(sx[0], sx[1], sx[2])));
    const x1 = Math.min(size - 1, Math.ceil(Math.max(sx[0], sx[1], sx[2])));
    const y0 = Math.max(0, Math.floor(Math.min(sy[0], sy[1], sy[2])));
    const y1 = Math.min(size - 1, Math.ceil(Math.max(sy[0], sy[1], sy[2])));

    for (let y = y0; y <= y1; y++) {
      for (let x = x0; x <= x1; x++) {
        const cx = x + 0.5;
        const cy = y + 0.5;
        let w0 = ((sx[1] - cx) * (sy[2] - cy) - (sx[2] - cx) * (sy[1] - cy)) / area;
        let w1 = ((sx[2] - cx) * (sy[0] - cy) - (sx[0] - cx) * (sy[2] - cy)) / area;
        let w2 = 1 - w0 - w1;
        if (w0 < 0 || w1 < 0 || w2 < 0) continue;
        const z = w0 * sz[0] + w1 * sz[1] + w2 * sz[2];
        if (z >= depth[y * size + x]) continue;

        const u = w0 * uvs[tri[0] * 2] + w1 * uvs[tri[1] * 2] + w2 * uvs[tri[2] * 2];
        const v = w0 * uvs[tri[0] * 2 + 1] + w1 * uvs[tri[1] * 2 + 1] + w2 * uvs[tri[2] * 2 + 1];
        const texel = sample(atlas, u, v);
        // The near levels are alpha-masked, so the bake is too — a leaf's
        // quad must not print as a square.
        if (texel[3] < 0.5) continue;

        const ny =
          w0 * normals[tri[0] * 3 + 1] + w1 * normals[tri[1] * 3 + 1] + w2 * normals[tri[2] * 3 + 1];
        // **The picture carries the modelling.** The quads it goes on all have
        // one normal pointing straight up (see `crossQuads`), so the renderer
        // shades the whole billboard by one number — without any relief in the
        // texture a tree at dawn is a flat orange blob, sky light and nothing
        // else. So the bake keeps a real range: bright where the canopy faces
        // up, dark underneath, and darker again the further back in the crown a
        // texel sits, which is the ambient occlusion of the mass itself.
        const light = 0.74 + 0.26 * Math.max(0, ny);
        depth[y * size + x] = z;
        const i = (y * size + x) * 4;
        surface.data[i] = texel[0] * light;
        surface.data[i + 1] = texel[1] * light;
        surface.data[i + 2] = texel[2] * light;
        surface.data[i + 3] = 1;
      }
    }
  }

  // A canopy rasterised leaf by leaf is full of single-texel holes that read
  // as noise once the whole tree is forty pixels tall. Closing them keeps the
  // crown a mass; the dilation then carries its colour into the empty texels
  // so the silhouette does not darken when the GPU filters it.
  close(surface, size);
  shadeByDepth(surface, depth, size);
  surface.dilateAlpha(6);
  return { surface, width, height, base: minY };
}

/**
 * Darkens what sits further back in the crown — the depth buffer read as an
 * occlusion term. A canopy is a mass, and the near face of it is a good deal
 * brighter than the leaves two metres behind; without this the billboard is
 * evenly bright over its whole silhouette and reads as cardboard.
 */
function shadeByDepth(surface, depth, size) {
  let near = Infinity;
  let far = -Infinity;
  for (const z of depth) {
    if (!Number.isFinite(z)) continue;
    if (z < near) near = z;
    if (z > far) far = z;
  }
  const span = far - near;
  if (!(span > 0)) return;
  for (let i = 0; i < size * size; i++) {
    const z = depth[i];
    if (!Number.isFinite(z)) continue;
    const back = (z - near) / span;
    const shade = 1 - back * 0.14;
    surface.data[i * 4] *= shade;
    surface.data[i * 4 + 1] *= shade;
    surface.data[i * 4 + 2] *= shade;
  }
}

/** Nearest-texel read of a surface at `[0, 1]²`. */
function sample(surface, u, v) {
  const x = Math.min(surface.width - 1, Math.max(0, Math.round(u * surface.width - 0.5)));
  const y = Math.min(surface.height - 1, Math.max(0, Math.round(v * surface.height - 0.5)));
  const i = (y * surface.width + x) * 4;
  return [surface.data[i], surface.data[i + 1], surface.data[i + 2], surface.data[i + 3]];
}

/** Fills a transparent texel that has covered neighbours on both sides. */
function close(surface, size) {
  const copy = Float32Array.from(surface.data);
  const alpha = (x, y) =>
    x < 0 || y < 0 || x >= size || y >= size ? 0 : copy[(y * size + x) * 4 + 3];
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const i = (y * size + x) * 4;
      if (copy[i + 3] > 0.5) continue;
      let n = 0;
      let r = 0;
      let g = 0;
      let b = 0;
      for (let dy = -1; dy <= 1; dy++) {
        for (let dx = -1; dx <= 1; dx++) {
          if (dx === 0 && dy === 0) continue;
          if (alpha(x + dx, y + dy) < 0.5) continue;
          const j = ((y + dy) * size + (x + dx)) * 4;
          r += copy[j];
          g += copy[j + 1];
          b += copy[j + 2];
          n++;
        }
      }
      if (n < 5) continue;
      surface.data[i] = r / n;
      surface.data[i + 1] = g / n;
      surface.data[i + 2] = b / n;
      surface.data[i + 3] = 1;
    }
  }
}
