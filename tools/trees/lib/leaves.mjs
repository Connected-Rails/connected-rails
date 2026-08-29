// Cutting single leaves out of a scanned sheet, and stamping them onto a card.
//
// ambientCG's `LeafSet###` are photographs of loose leaves on black with an
// opacity map beside them — six oak leaves, a spray of ash leaflets, a handful
// of autumn beech. This module finds each leaf in such a sheet, cuts it out,
// and draws it rotated and scaled wherever `foliage.mjs` wants one. The
// arrangement stays the pipeline's own: which leaves sit on which shoot, at
// what angle and how big, is what tells an ash from an oak, and no sheet
// carries that.
//
// A leaf is found by flood-filling the opacity mask. Whatever is smaller than
// a thousandth of the sheet is dirt on the scanner bed, not a leaf.

import { readFileSync } from 'node:fs';

import { decodePng } from './png.mjs';
import { Surface } from './raster.mjs';

/**
 * Reads a colour map and an opacity map into one straight-alpha RGBA image.
 * ambientCG ships the two apart; the opacity map is greyscale.
 */
export function loadSheet(colorPath, opacityPath) {
  const color = decodePng(readFileSync(colorPath));
  const opacity = opacityPath ? decodePng(readFileSync(opacityPath)) : null;
  if (opacity && (opacity.width !== color.width || opacity.height !== color.height)) {
    throw new Error('colour and opacity maps differ in size');
  }
  const data = new Uint8Array(color.data);
  if (opacity) {
    for (let i = 0; i < data.length; i += 4) data[i + 3] = opacity.data[i];
  }
  return { width: color.width, height: color.height, data };
}

/**
 * The bounding boxes of the separate shapes on a sheet, in reading order.
 *
 * @param {{width:number,height:number,data:Uint8Array}} sheet
 * @param {object} options `{ threshold, minArea }` — `minArea` as a share of
 *   the sheet.
 */
export function segment(sheet, { threshold = 96, minArea = 0.001 } = {}) {
  const { width, height, data } = sheet;
  const seen = new Uint8Array(width * height);
  const boxes = [];
  const stack = [];
  const limit = minArea * width * height;
  for (let start = 0; start < width * height; start++) {
    if (seen[start] || data[start * 4 + 3] < threshold) continue;
    let minX = width;
    let maxX = 0;
    let minY = height;
    let maxY = 0;
    let area = 0;
    stack.push(start);
    seen[start] = 1;
    while (stack.length) {
      const at = stack.pop();
      const x = at % width;
      const y = (at - x) / width;
      area += 1;
      if (x < minX) minX = x;
      if (x > maxX) maxX = x;
      if (y < minY) minY = y;
      if (y > maxY) maxY = y;
      // Eight-connected: a leaf's serrated edge is a chain of diagonal texels.
      for (let dy = -1; dy <= 1; dy++) {
        for (let dx = -1; dx <= 1; dx++) {
          const nx = x + dx;
          const ny = y + dy;
          if (nx < 0 || ny < 0 || nx >= width || ny >= height) continue;
          const next = ny * width + nx;
          if (seen[next] || data[next * 4 + 3] < threshold) continue;
          seen[next] = 1;
          stack.push(next);
        }
      }
    }
    if (area >= limit) {
      boxes.push({ x: minX, y: minY, w: maxX - minX + 1, h: maxY - minY + 1, area });
    }
  }
  // Reading order, so an index in `species.json` means the same leaf whenever
  // the sheet is cut again.
  boxes.sort((a, b) => (Math.abs(a.y - b.y) > 40 ? a.y - b.y : a.x - b.x));
  return boxes;
}

/** Cuts a box out of a sheet into its own image. */
export function crop(sheet, box) {
  const data = new Uint8Array(box.w * box.h * 4);
  for (let y = 0; y < box.h; y++) {
    const from = ((box.y + y) * sheet.width + box.x) * 4;
    data.set(sheet.data.subarray(from, from + box.w * 4), y * box.w * 4);
  }
  return { width: box.w, height: box.h, data };
}

/**
 * Draws a cut-out leaf onto a surface.
 *
 * The contract is the one `foliage.mjs` uses for a painted blade: `at` is where
 * the stalk joins the shoot, `angle` is measured from straight down the card,
 * and `length` is how far the leaf reaches from there. The image's own long
 * axis runs bottom (stalk) to top (tip) unless `flip` says otherwise — a
 * scanner bed has no convention.
 *
 * `tint` multiplies the sampled colour, which is how one sheet of green leaves
 * serves a beech and a lime without either looking painted.
 */
export function stamp(surface, image, { at, angle, length, tint = [1, 1, 1], flip = false }) {
  const aspect = image.width / image.height;
  const width = length * aspect;
  const cos = Math.cos(angle);
  const sin = Math.sin(angle);
  // Card space: x across the leaf, y from stalk (0) to tip (1).
  const place = (x, y) => [
    at[0] + (x * cos + y * sin) * 1,
    at[1] + (-x * sin + y * cos) * 1,
  ];
  const corners = [
    place(-width / 2, 0),
    place(width / 2, 0),
    place(width / 2, length),
    place(-width / 2, length),
  ];
  let minX = Infinity;
  let maxX = -Infinity;
  let minY = Infinity;
  let maxY = -Infinity;
  for (const [x, y] of corners) {
    minX = Math.min(minX, x);
    maxX = Math.max(maxX, x);
    minY = Math.min(minY, y);
    maxY = Math.max(maxY, y);
  }
  const x0 = Math.max(0, Math.floor(minX));
  const x1 = Math.min(surface.width - 1, Math.ceil(maxX));
  const y0 = Math.max(0, Math.floor(minY));
  const y1 = Math.min(surface.height - 1, Math.ceil(maxY));

  for (let py = y0; py <= y1; py++) {
    for (let px = x0; px <= x1; px++) {
      // Card coordinates of this texel, by the inverse rotation.
      const dx = px + 0.5 - at[0];
      const dy = py + 0.5 - at[1];
      const lx = dx * cos - dy * sin;
      const ly = dx * sin + dy * cos;
      if (ly < 0 || ly > length) continue;
      const u = lx / width + 0.5;
      if (u < 0 || u > 1) continue;
      const v = flip ? ly / length : 1 - ly / length;
      const texel = sample(image, u, v);
      if (texel[3] <= 0.004) continue;
      surface.blend(px, py, texel[0] * tint[0], texel[1] * tint[1], texel[2] * tint[2], texel[3]);
    }
  }
}

/** Bilinear read of an RGBA8 image at `[0, 1]²`, returning 0…1 floats. */
function sample(image, u, v) {
  const x = Math.min(image.width - 1.001, Math.max(0, u * image.width - 0.5));
  const y = Math.min(image.height - 1.001, Math.max(0, v * image.height - 0.5));
  const x0 = Math.floor(x);
  const y0 = Math.floor(y);
  const fx = x - x0;
  const fy = y - y0;
  const out = [0, 0, 0, 0];
  for (let c = 0; c < 4; c++) {
    const at = (ix, iy) => image.data[(iy * image.width + ix) * 4 + c] / 255;
    const top = at(x0, y0) + (at(x0 + 1, y0) - at(x0, y0)) * fx;
    const bottom = at(x0, y0 + 1) + (at(x0 + 1, y0 + 1) - at(x0, y0 + 1)) * fx;
    out[c] = top + (bottom - top) * fy;
  }
  return out;
}

/**
 * A sheet of every cut-out leaf, numbered, for picking them by eye. The index
 * a leaf carries here is the one `species.json` names.
 */
export function contactSheet(entries, cell = 128, columns = 12) {
  const rows = entries.reduce((n, e) => n + Math.ceil(e.boxes.length / columns), 0);
  const surface = new Surface(cell * columns, cell * Math.max(1, rows));
  surface.clear(0.12, 0.12, 0.14, 1);
  let row = 0;
  for (const entry of entries) {
    entry.boxes.forEach((box, i) => {
      const leaf = crop(entry.sheet, box);
      const column = i % columns;
      const line = row + Math.floor(i / columns);
      const scale = Math.min((cell - 8) / leaf.width, (cell - 8) / leaf.height);
      const ox = column * cell + (cell - leaf.width * scale) / 2;
      const oy = line * cell + (cell - leaf.height * scale) / 2;
      for (let y = 0; y < leaf.height * scale; y++) {
        for (let x = 0; x < leaf.width * scale; x++) {
          const texel = sample(leaf, x / (leaf.width * scale), y / (leaf.height * scale));
          surface.blend(Math.round(ox + x), Math.round(oy + y), texel[0], texel[1], texel[2], texel[3]);
        }
      }
      // A tick mark every fifth leaf, so counting along a row is possible.
      if (i % 5 === 0) {
        for (let x = 0; x < 10; x++) surface.blend(column * cell + x, line * cell + 2, 1, 0.85, 0.2, 1);
      }
    });
    row += Math.ceil(entry.boxes.length / columns);
  }
  return surface;
}

/**
 * Resolves a catalogue `scan` block into the images it names, cutting the
 * sheet only once however many species draw from it.
 *
 * ```json
 * "scan": { "set": "LeafSet016", "leaves": [0, 1, 3] }
 * "scan": { "asset": "fir_tree_01", "map": "twig", "leaves": [1, 2, 3], "whole": true }
 * ```
 *
 * `leaves` are indices into the sheet's shapes in reading order; leaving it out
 * takes all of them. `whole` says the shape is already a spray or a compound
 * leaf, so the card stamps it once instead of composing one out of leaflets.
 */
const sheets = new Map();
export function loadScan(scan, { leafSetFiles, polyHavenFile }) {
  if (!scan) return null;
  const key = scan.set ?? `${scan.asset}/${scan.map}`;
  if (!sheets.has(key)) {
    const files = scan.set
      ? leafSetFiles(scan.set)
      : {
          color: polyHavenFile(scan.asset, `${scan.map}_diff`),
          opacity: polyHavenFile(scan.asset, `${scan.map}_alpha`),
        };
    const sheet = loadSheet(files.color, files.opacity);
    const boxes = segment(sheet, { minArea: scan.minArea ?? 0.001 });
    sheets.set(key, { sheet, boxes });
  }
  const { sheet, boxes } = sheets.get(key);
  const wanted = scan.leaves ?? boxes.map((_, i) => i);
  const images = wanted
    .filter((i) => i >= 0 && i < boxes.length)
    .map((i) => crop(sheet, boxes[i]));
  if (images.length === 0) {
    throw new Error(`${key}: none of the leaves ${JSON.stringify(wanted)} exist (${boxes.length} on the sheet)`);
  }
  return { images, whole: scan.whole === true, flip: scan.flip === true, tint: scan.tint };
}

/** How many separate shapes a sheet holds — for checking a catalogue entry. */
export function countShapes(scan, paths) {
  const resolved = loadScan({ ...scan, leaves: undefined }, paths);
  return resolved.images.length;
}
