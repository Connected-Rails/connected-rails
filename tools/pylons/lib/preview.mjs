// `--preview`: what the kit just built, as a picture.
//
// A mast is judged by its outline, and an outline is not something a triangle
// count tells you about — a Donaumast whose upper crossarm ended up wider than
// its lower one passes every check in this pipeline and is wrong at a glance.
// So the build can draw itself: an orthographic rasteriser with a depth buffer
// and one light, about a hundred lines, writing PNGs into /tmp.
//
// Nothing here ships. It is a pair of eyes for whoever edits `pylons.json`.

import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { encodePng } from '../../trees/lib/png.mjs';
import { buildMast, finestMember } from './kit.mjs';

/**
 * Draws `geometry` orthographically into an RGBA canvas.
 *
 * `axis` picks the view: `'front'` looks along +Z (the line runs left to
 * right past the camera, so the crossarms are seen in full), `'side'` looks
 * along +X (down the line, the view a driver gets).
 */
function raster(geometry, { width, height, metresPerPixel, originX, originY, axis, rgba, colour }) {
  const depth = new Float32Array(width * height).fill(Infinity);
  const p = geometry.positions;
  const n = geometry.normals;
  const light = [0.42, 0.78, 0.46];

  for (let t = 0; t < geometry.indices.length; t += 3) {
    const idx = [geometry.indices[t], geometry.indices[t + 1], geometry.indices[t + 2]];
    const pts = idx.map((i) => {
      const x = p[i * 3];
      const y = p[i * 3 + 1];
      const z = p[i * 3 + 2];
      const u = axis === 'front' ? x : z;
      return {
        sx: originX + u / metresPerPixel,
        sy: originY - y / metresPerPixel,
        d: axis === 'front' ? z : -x,
      };
    });
    const nx = n[idx[0] * 3];
    const ny = n[idx[0] * 3 + 1];
    const nz = n[idx[0] * 3 + 2];
    const lambert = 0.4 + 0.6 * Math.abs(nx * light[0] + ny * light[1] + nz * light[2]);

    const minX = Math.max(0, Math.floor(Math.min(...pts.map((q) => q.sx))));
    const maxX = Math.min(width - 1, Math.ceil(Math.max(...pts.map((q) => q.sx))));
    const minY = Math.max(0, Math.floor(Math.min(...pts.map((q) => q.sy))));
    const maxY = Math.min(height - 1, Math.ceil(Math.max(...pts.map((q) => q.sy))));
    if (maxX < minX || maxY < minY) continue;

    const [a, b, c] = pts;
    const area = (b.sx - a.sx) * (c.sy - a.sy) - (c.sx - a.sx) * (b.sy - a.sy);
    if (Math.abs(area) < 1e-9) continue;

    for (let y = minY; y <= maxY; y++) {
      for (let x = minX; x <= maxX; x++) {
        const px = x + 0.5;
        const py = y + 0.5;
        const w0 = ((b.sx - a.sx) * (py - a.sy) - (px - a.sx) * (b.sy - a.sy)) / area;
        const w1 = ((px - a.sx) * (c.sy - a.sy) - (c.sx - a.sx) * (py - a.sy)) / area;
        const w2 = 1 - w0 - w1;
        if (w0 < 0 || w1 < 0 || w2 < 0) continue;
        const d = a.d * w2 + b.d * w1 + c.d * w0;
        const o = y * width + x;
        if (d >= depth[o]) continue;
        depth[o] = d;
        rgba[o * 4] = Math.round(colour[0] * lambert * 255);
        rgba[o * 4 + 1] = Math.round(colour[1] * lambert * 255);
        rgba[o * 4 + 2] = Math.round(colour[2] * lambert * 255);
        rgba[o * 4 + 3] = 255;
      }
    }
  }
}

/** A one-pixel scale rule every ten metres up the left edge. */
function scaleRule(rgba, width, height, originY, metresPerPixel) {
  for (let m = 0; m <= 100; m += 10) {
    const y = Math.round(originY - m / metresPerPixel);
    if (y < 0 || y >= height) continue;
    const len = m % 50 === 0 ? 14 : 7;
    for (let x = 0; x < len; x++) {
      const o = (y * width + x) * 4;
      rgba[o] = 90;
      rgba[o + 1] = 110;
      rgba[o + 2] = 120;
      rgba[o + 3] = 255;
    }
  }
}

/**
 * One sheet per type (front and side at the finest level, front at the coarsest
 * so the hand-over can be judged), plus `alle.png` with every type of the run
 * side by side at one scale — which is the picture that shows whether the
 * catalogue's heights agree with each other.
 */
export function renderSheet(types, directory) {
  mkdirSync(directory, { recursive: true });
  const structureColour = [0.72, 0.74, 0.76];
  const fittingColour = [0.6, 0.45, 0.36];

  for (const type of types) {
    const height = (type.height_m[0] + type.height_m[1]) / 2;
    const panels = [
      { detail: 0, axis: 'front' },
      { detail: 0, axis: 'side' },
      { detail: 2, axis: 'front' },
      { detail: 3, axis: 'front' },
    ];
    const panelWidth = 300;
    const panelHeight = 460;
    const width = panelWidth * panels.length;
    const rgba = new Uint8Array(width * panelHeight * 4).fill(0);
    for (let i = 3; i < rgba.length; i += 4) rgba[i] = 255;

    // Fit the tallest dimension into the panel it has: height against the
    // panel's height, crossarm width against its width.
    const widest = Math.max(...type.crossarms.map((a) => a.width_m));
    const mpp = Math.max((height * 1.12) / (panelHeight - 40), (widest * 1.12) / panelWidth);

    panels.forEach((panel, i) => {
      const { structure, fittings } = buildMast(type, { detail: panel.detail, height });
      const common = {
        width,
        height: panelHeight,
        metresPerPixel: mpp,
        originX: panelWidth * i + panelWidth / 2,
        originY: panelHeight - 24,
        axis: panel.axis,
        rgba,
      };
      raster(structure, { ...common, colour: structureColour });
      raster(fittings, { ...common, colour: fittingColour });
    });
    scaleRule(rgba, width, panelHeight, panelHeight - 24, mpp);
    writeFileSync(join(directory, `${type.id}.png`), encodePng(rgba, width, panelHeight));
  }

  // The line-up: everything at one metres-per-pixel, so the 80 m combination
  // mast and the 7.5 m telegraph pole are in the same picture at the same size.
  const tallest = Math.max(...types.map((t) => (t.height_m[0] + t.height_m[1]) / 2));
  const sheetHeight = 700;
  const mpp = (tallest * 1.1) / sheetHeight;
  const cell = 380;
  const sheetWidth = cell * types.length;
  const rgba = new Uint8Array(sheetWidth * sheetHeight * 4).fill(0);
  for (let i = 3; i < rgba.length; i += 4) rgba[i] = 255;
  types.forEach((type, i) => {
    const height = (type.height_m[0] + type.height_m[1]) / 2;
    const { structure, fittings } = buildMast(type, { detail: 0, height });
    const common = {
      width: sheetWidth,
      height: sheetHeight,
      metresPerPixel: mpp,
      originX: cell * i + cell / 2,
      originY: sheetHeight - 20,
      axis: 'front',
      rgba,
    };
    raster(structure, { ...common, colour: structureColour });
    raster(fittings, { ...common, colour: fittingColour });
  });
  scaleRule(rgba, sheetWidth, sheetHeight, sheetHeight - 20, mpp);
  writeFileSync(join(directory, 'alle.png'), encodePng(rgba, sheetWidth, sheetHeight));

  return types.length + 1;
}

/**
 * How much of the screen a mast covers, as a fraction of the panel it is drawn
 * in — measured by supersampling, so a member thinner than a pixel counts for
 * the fraction of a pixel it really is.
 *
 * This is the number the levels have to agree on. A coarse level with the same
 * outline but half the members is not the same picture: it is a *paler* mast,
 * and a hand-over that halves the ink is a mast that visibly thins out as the
 * train approaches it. The kit compensates by drawing what is left thicker
 * ([`MEMBER_SCALE`](./kit.mjs)), and this says whether the compensation is
 * right.
 */
export function ink(type, detail, metresPerPixel, { height, axis = 'front', supersample = 6 } = {}) {
  const h = height ?? (type.height_m[0] + type.height_m[1]) / 2;
  const mpp = metresPerPixel / supersample;
  const widest = Math.max(h, ...type.crossarms.map((a) => a.width_m));
  const width = Math.ceil((widest * 1.1) / mpp);
  const canvasHeight = Math.ceil((h * 1.1) / mpp);
  if (width * canvasHeight > 40e6) throw new Error('preview canvas too large');
  const rgba = new Uint8Array(width * canvasHeight * 4);
  const { structure, fittings } = buildMast(type, { detail, height: h });
  const common = {
    width,
    height: canvasHeight,
    metresPerPixel: mpp,
    originX: width / 2,
    originY: canvasHeight - 2,
    axis,
    rgba,
  };
  raster(structure, { ...common, colour: [1, 1, 1] });
  raster(fittings, { ...common, colour: [1, 1, 1] });
  let covered = 0;
  for (let i = 3; i < rgba.length; i += 4) if (rgba[i] !== 0) covered++;
  // Per whole pixel of the un-supersampled image, so the number is comparable
  // across levels and distances.
  return covered / (width * canvasHeight);
}

/**
 * `--ink`: every type, every level, at the distance the level hands over at —
 * and what that does to the mast's ink compared with the finest level at the
 * same distance.
 */
export function inkReport(types, bands) {
  const rows = [];
  for (const type of types) {
    const h = (type.height_m[0] + type.height_m[1]) / 2;
    const distances = bands(type);
    const line = [];
    for (let detail = 0; detail < 4; detail++) {
      // Measured where the level *starts*, which is where it has to stand in
      // for the one before it. A level whose band is empty — the cull caught up
      // with its hand-over, which is what happens to a 10 m pole's coarsest
      // level — is never drawn and is not worth matching.
      const at = detail === 0 ? distances[0] : distances[detail - 1];
      if (detail > 0 && distances[detail] <= at + 1) {
        line.push({ detail, at, fine: 0, coarse: 0, ratio: 1, unused: true });
        continue;
      }
      const mpp = at * METRES_PER_PIXEL_PER_METRE;
      const fine = ink(type, Math.max(0, detail - 1), mpp, { height: h });
      const coarse = ink(type, detail, mpp, { height: h });
      line.push({ detail, at, fine, coarse, ratio: fine > 0 ? coarse / fine : 1 });
    }
    rows.push({ type, h, finest: finestMember(type), line });
  }
  return rows;
}

/**
 * One pixel, in metres, per metre of distance — the reference the bands are
 * cut for: **1440 lines at the simulator's 45° vertical field of view**
 * (`2 * tan(fov / 2) / lines`). A 4K screen resolves finer and gets a slightly
 * conservative hand-over; a 1080p one a slightly late.
 */
export const METRES_PER_PIXEL_PER_METRE = (2 * Math.tan(Math.PI / 8)) / 1440;

/**
 * The hand-overs, drawn at the distance they happen at.
 *
 * `--ink` says the two levels carry the same amount of grey; this says whether
 * they carry it in the same *shape*. Each pair is rendered at the metres per
 * pixel of its own hand-over distance and then magnified with no filtering, so
 * what is on the sheet is what the screen gets — the fine level on the left of
 * each pair, the coarse one on the right. If the two halves of a pair look like
 * the same mast, the hand-over is invisible in the game.
 */
export function renderHandovers(types, directory, bands) {
  mkdirSync(directory, { recursive: true });
  const zoom = 5;
  const gap = 8;
  for (const type of types) {
    const height = (type.height_m[0] + type.height_m[1]) / 2;
    const distances = bands(type);
    const cells = [];
    for (let detail = 1; detail < 4; detail++) {
      const at = distances[detail - 1];
      if (distances[detail] <= at + 1) continue;
      const mpp = at * METRES_PER_PIXEL_PER_METRE;
      cells.push({ at, fine: detail - 1, coarse: detail, mpp });
    }
    if (cells.length === 0) continue;

    const widest = Math.max(height, ...type.crossarms.map((a) => a.width_m));
    const sizes = cells.map((c) => ({
      w: Math.ceil((widest * 1.1) / c.mpp),
      h: Math.ceil((height * 1.15) / c.mpp),
    }));
    const cellWidth = sizes.map((s) => s.w * 2 * zoom + gap);
    const width = cellWidth.reduce((a, b) => a + b + gap * 3, gap);
    const canvasHeight = Math.max(...sizes.map((s) => s.h)) * zoom + gap * 2;
    const rgba = new Uint8Array(width * canvasHeight * 4);
    for (let i = 3; i < rgba.length; i += 4) rgba[i] = 255;

    let x = gap;
    cells.forEach((cell, i) => {
      const { w, h } = sizes[i];
      for (const [k, detail] of [cell.fine, cell.coarse].entries()) {
        const small = new Uint8Array(w * h * 4);
        const { structure, fittings } = buildMast(type, { detail, height });
        const common = {
          width: w,
          height: h,
          metresPerPixel: cell.mpp,
          originX: w / 2,
          originY: h - 2,
          axis: 'front',
          rgba: small,
        };
        raster(structure, { ...common, colour: [0.78, 0.8, 0.82] });
        raster(fittings, { ...common, colour: [0.62, 0.47, 0.38] });
        // Nearest-neighbour up, so a half-covered pixel stays a half-covered
        // pixel instead of being smoothed into a lie.
        const originX = x + k * (w * zoom + gap);
        const originY = canvasHeight - gap - h * zoom;
        for (let sy = 0; sy < h * zoom; sy++) {
          for (let sx = 0; sx < w * zoom; sx++) {
            const src = (Math.floor(sy / zoom) * w + Math.floor(sx / zoom)) * 4;
            if (small[src + 3] === 0) continue;
            const dst = ((originY + sy) * width + originX + sx) * 4;
            if (dst < 0 || dst + 3 >= rgba.length) continue;
            rgba[dst] = small[src];
            rgba[dst + 1] = small[src + 1];
            rgba[dst + 2] = small[src + 2];
            rgba[dst + 3] = 255;
          }
        }
      }
      x += cellWidth[i] + gap * 3;
    });
    writeFileSync(
      join(directory, `${type.id}-handover.png`),
      encodePng(rgba, width, canvasHeight),
    );
  }
  return types.length;
}
