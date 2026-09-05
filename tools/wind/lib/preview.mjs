// `--preview`: what the kit just built, as a picture.
//
// A turbine is judged by its outline — the taper of the tower, the bulge of a
// blade, whether an Enercon's egg reads as an egg — and none of that is in a
// triangle count. The masts' rasteriser draws it: an orthographic view with a
// depth buffer and one light, into PNGs under /tmp. Nothing here ships.

import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { encodePng } from '../../trees/lib/png.mjs';
import { raster, scaleRule } from '../../pylons/lib/preview.mjs';
import { merge } from '../../pylons/lib/geom.mjs';
import { TILT_DEG, buildTurbine, rotateZ, translate } from './kit.mjs';

const COATING = [0.86, 0.87, 0.88];
const CONCRETE = [0.58, 0.57, 0.54];
const STEEL = [0.66, 0.68, 0.7];
const LAMP = [0.9, 0.15, 0.1];

/**
 * The machine assembled into the root frame — nacelle and rotor moved to hub
 * height, the rotor tilted and turned to a phase where no blade hides behind
 * the tower — as one geometry per material, for drawing.
 */
export function assemble(spec, variant, detail, phase = 0.35) {
  const built = buildTurbine(spec, { variant, detail });
  const hub = spec.hub_m;
  const rotor = rotateZ(merge([built.rotor]), phase);
  // The tilt about X: the rotor's Z axis lifted at the front.
  const tilt = (TILT_DEG * Math.PI) / 180;
  for (let i = 0; i < rotor.positions.length; i += 3) {
    for (const array of [rotor.positions, rotor.normals]) {
      const y = array[i + 1];
      const z = array[i + 2];
      array[i + 1] = y * Math.cos(tilt) - z * Math.sin(tilt);
      array[i + 2] = y * Math.sin(tilt) + z * Math.cos(tilt);
    }
  }
  translate(rotor, [0, hub, -spec.overhang_m]);
  const nacelle = translate(merge([built.nacelle.coating]), [0, hub, 0]);
  const dark = translate(merge([built.nacelle.dark]), [0, hub, 0]);
  const lamps = translate(merge([built.lamps]), [0, hub, 0]);
  return {
    coating: merge([built.tower.coating, nacelle, rotor]),
    steel: built.tower.steel,
    concrete: built.tower.concrete,
    dark,
    lamps,
  };
}

// The rasteriser keeps a depth buffer per call, so a later part paints over an
// earlier one whatever its depth: the parts go in from the back — the louvres
// on the tail first, the shell that hides them over it, the lamps last.
function draw(parts, common) {
  raster(parts.dark, { ...common, colour: [0.2, 0.2, 0.2] });
  raster(parts.concrete, { ...common, colour: CONCRETE });
  raster(parts.steel, { ...common, colour: STEEL });
  raster(parts.coating, { ...common, colour: COATING });
  raster(parts.lamps, { ...common, colour: LAMP });
}

/**
 * One sheet per class and variant — front and side at the finest level, then
 * the coarser levels from the front — plus `alle.png` with every variant of
 * the run side by side at one scale.
 */
export function renderSheet(classes, directory) {
  mkdirSync(directory, { recursive: true });
  let sheets = 0;
  const line = [];
  for (const spec of classes) {
    for (const variant of spec.variants) {
      const panels = [
        { detail: 0, axis: 'front' },
        { detail: 0, axis: 'side' },
        { detail: 1, axis: 'front' },
        { detail: 2, axis: 'front' },
        { detail: 3, axis: 'front' },
      ];
      const panelWidth = 320;
      const panelHeight = 560;
      const width = panelWidth * panels.length;
      const rgba = new Uint8Array(width * panelHeight * 4).fill(0);
      for (let i = 3; i < rgba.length; i += 4) rgba[i] = 255;
      const tip = spec.hub_m + spec.rotor_m / 2;
      const mpp = Math.max((tip * 1.1) / (panelHeight - 40), (spec.rotor_m * 1.15) / panelWidth);
      panels.forEach((panel, i) => {
        const parts = assemble(spec, variant, panel.detail);
        draw(parts, {
          width,
          height: panelHeight,
          metresPerPixel: mpp,
          originX: panelWidth * i + panelWidth / 2,
          originY: panelHeight - 24,
          axis: panel.axis,
          rgba,
        });
      });
      scaleRule(rgba, width, panelHeight, panelHeight - 24, mpp);
      writeFileSync(join(directory, `${spec.id}_${variant}.png`), encodePng(rgba, width, panelHeight));
      sheets++;
      line.push({ spec, variant });

      // The head close up: nacelle, spinner and blade roots from the front and
      // the side at the finest level — the part a passenger looks at when the
      // train stops beside one.
      const closeWidth = 640;
      const closeHeight = 480;
      const close = new Uint8Array(closeWidth * 2 * closeHeight * 4).fill(0);
      for (let i = 3; i < close.length; i += 4) close[i] = 255;
      const span = Math.max(spec.nacelle.length_m * 2.4, spec.rotor_m * 0.3);
      const closeMpp = span / closeWidth;
      for (const [i, axis] of ['front', 'side'].entries()) {
        draw(assemble(spec, variant, 0, 0.35), {
          width: closeWidth * 2,
          height: closeHeight,
          metresPerPixel: closeMpp,
          originX: closeWidth * i + closeWidth / 2,
          originY: closeHeight / 2 + spec.hub_m / closeMpp,
          axis,
          rgba: close,
        });
      }
      writeFileSync(join(directory, `${spec.id}_${variant}-kopf.png`), encodePng(close, closeWidth * 2, closeHeight));
    }
  }

  const tallest = Math.max(...classes.map((c) => c.hub_m + c.rotor_m / 2));
  const sheetHeight = 900;
  const mpp = (tallest * 1.08) / sheetHeight;
  const cell = Math.ceil((Math.max(...classes.map((c) => c.rotor_m)) * 1.1) / mpp);
  const sheetWidth = cell * line.length;
  const rgba = new Uint8Array(sheetWidth * sheetHeight * 4).fill(0);
  for (let i = 3; i < rgba.length; i += 4) rgba[i] = 255;
  line.forEach(({ spec, variant }, i) => {
    const parts = assemble(spec, variant, 0);
    draw(parts, {
      width: sheetWidth,
      height: sheetHeight,
      metresPerPixel: mpp,
      originX: cell * i + cell / 2,
      originY: sheetHeight - 20,
      axis: 'front',
      rgba,
    });
  });
  scaleRule(rgba, sheetWidth, sheetHeight, sheetHeight - 20, mpp);
  writeFileSync(join(directory, 'alle.png'), encodePng(rgba, sheetWidth, sheetHeight));
  return sheets + 1;
}

/**
 * The hand-overs, drawn at the distance they happen at and magnified without
 * filtering — the fine level left of each pair, the coarse one right. If the
 * two halves look like the same machine, the hand-over is invisible in the
 * game.
 */
export function renderHandovers(classes, directory, bands, metresPerPixelPerMetre) {
  mkdirSync(directory, { recursive: true });
  const zoom = 4;
  const gap = 8;
  let sheets = 0;
  for (const spec of classes) {
    const variant = spec.variants[0];
    const distances = bands(spec);
    const cells = [];
    for (let detail = 1; detail < 4; detail++) {
      const at = distances[detail - 1];
      if (distances[detail] <= at + 1) continue;
      cells.push({ at, fine: detail - 1, coarse: detail, mpp: at * metresPerPixelPerMetre });
    }
    if (cells.length === 0) continue;
    const tip = spec.hub_m + spec.rotor_m / 2;
    const sizes = cells.map((c) => ({
      w: Math.ceil((spec.rotor_m * 1.15) / c.mpp),
      h: Math.ceil((tip * 1.1) / c.mpp),
    }));
    const cellWidth = sizes.map((s) => s.w * 2 * zoom + gap);
    const width = cellWidth.reduce((a, b) => a + b + gap * 3, gap);
    const canvasHeight = Math.max(...sizes.map((s) => s.h)) * zoom + gap * 2;
    const rgba = new Uint8Array(width * canvasHeight * 4);
    for (let i = 3; i < rgba.length; i += 4) rgba[i] = 255;
    let x = gap;
    cells.forEach((cellSpec, i) => {
      const { w, h } = sizes[i];
      for (const [k, detail] of [cellSpec.fine, cellSpec.coarse].entries()) {
        const small = new Uint8Array(w * h * 4);
        draw(assemble(spec, variant, detail), {
          width: w,
          height: h,
          metresPerPixel: cellSpec.mpp,
          originX: w / 2,
          originY: h - 2,
          axis: 'front',
          rgba: small,
        });
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
    writeFileSync(join(directory, `${spec.id}-handover.png`), encodePng(rgba, width, canvasHeight));
    sheets++;
  }
  return sheets;
}
