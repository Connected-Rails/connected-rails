// Filling the dead space in a photogrammetry atlas.
//
// These atlases are not a tidy layout of a few big islands. They are hundreds
// of small scraps — a wing here, a wheel arch there, half a door — packed
// tight, with the unused space between them left a flat dark blue. That is
// harmless as long as nothing ever reads it, and everything reads it:
//
// * **Downsampling** averages a block of texels into one, and a block that
//   straddles the edge of an island mixes white paint with the dead space.
// * **Mip levels** are the same thing again, five times over. By the third
//   level a small island is more gap than paint.
// * **Block compression** gives a 4×4 block four colours to share. A block
//   with paint on one side and dead space on the other spends two of them on
//   the boundary and banding is what comes out.
//
// On screen that is a white van with dark angular shards over the panels,
// following island edges — it looks like crumpled foil, and it looks like a
// texture bug because it is one.
//
// The fix is the standard one: no texel anywhere in the image may hold a
// colour that is not the colour of some part of the car. The dead space is
// flooded with the nearest island's colour, so every average, every mip and
// every block boundary mixes paint with paint.
//
// What makes it exact here is that the model says where its islands are. The
// dead space is not guessed at by colour — a tyre is as dark as the
// background — it is the part of the atlas that no triangle's texture
// coordinates cover.

/**
 * Which texels of the atlas the mesh actually reads.
 *
 * The UV triangles are rasterised at atlas resolution. `grow` widens each
 * island by that many texels first, which covers the half-texel a sampler
 * reaches past the edge and the rounding in between.
 */
export function coverage(uvs, faces, size, grow = 2) {
  const mask = new Uint8Array(size * size);
  const at = (x, y) => ((y % size) + size) % size * size + (((x % size) + size) % size);

  for (const face of faces) {
    const pts = face.map((v) => ({ x: uvs[v * 2] * size, y: uvs[v * 2 + 1] * size }));
    const [a, b, c] = pts;
    const area = (b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y);
    const minX = Math.floor(Math.min(a.x, b.x, c.x)) - 1;
    const maxX = Math.ceil(Math.max(a.x, b.x, c.x)) + 1;
    const minY = Math.floor(Math.min(a.y, b.y, c.y)) - 1;
    const maxY = Math.ceil(Math.max(a.y, b.y, c.y)) + 1;
    // A triangle thinner than a texel still reads texels. Degenerate ones fall
    // back to marking the box they sit in, which is at most a few texels.
    if (Math.abs(area) < 1e-9) {
      for (let y = minY; y <= maxY; y++) for (let x = minX; x <= maxX; x++) mask[at(x, y)] = 1;
      continue;
    }
    for (let y = minY; y <= maxY; y++) {
      for (let x = minX; x <= maxX; x++) {
        const px = x + 0.5;
        const py = y + 0.5;
        const w0 = ((b.x - px) * (c.y - py) - (c.x - px) * (b.y - py)) / area;
        const w1 = ((c.x - px) * (a.y - py) - (a.x - px) * (c.y - py)) / area;
        const w2 = 1 - w0 - w1;
        // A texel's worth of slack, so a triangle that grazes a texel centre
        // still claims it.
        if (w0 < -0.5 / size || w1 < -0.5 / size || w2 < -0.5 / size) continue;
        mask[at(x, y)] = 1;
      }
    }
  }

  for (let round = 0; round < grow; round++) {
    const wider = mask.slice();
    for (let y = 0; y < size; y++) {
      for (let x = 0; x < size; x++) {
        if (mask[at(x, y)]) continue;
        if (
          mask[at(x - 1, y)] || mask[at(x + 1, y)] ||
          mask[at(x, y - 1)] || mask[at(x, y + 1)]
        ) {
          wider[at(x, y)] = 1;
        }
      }
    }
    mask.set(wider);
  }
  return mask;
}

/**
 * Floods the colour of the nearest covered texel into everything else.
 *
 * A breadth-first sweep out of the islands, so every uncovered texel ends up
 * with the colour of the covered texel nearest to it. The atlas afterwards has
 * no dead space left to bleed: the gaps hold the paint of whatever is beside
 * them, which is exactly the colour that filtering there ought to produce.
 *
 * `pixels` is RGBA, modified in place. Returns how many texels were filled.
 */
export function dilate(pixels, mask, size) {
  const filled = mask.slice();
  let front = [];
  for (let i = 0; i < filled.length; i++) if (filled[i]) front.push(i);
  const started = front.length;

  while (front.length) {
    const next = [];
    for (const i of front) {
      const x = i % size;
      const y = (i / size) | 0;
      for (const [dx, dy] of [[1, 0], [-1, 0], [0, 1], [0, -1]]) {
        const nx = ((x + dx) % size + size) % size;
        const ny = ((y + dy) % size + size) % size;
        const j = ny * size + nx;
        if (filled[j]) continue;
        filled[j] = 1;
        pixels[j * 4] = pixels[i * 4];
        pixels[j * 4 + 1] = pixels[i * 4 + 1];
        pixels[j * 4 + 2] = pixels[i * 4 + 2];
        pixels[j * 4 + 3] = 255;
        next.push(j);
      }
    }
    front = next;
  }
  return filled.length - started;
}
