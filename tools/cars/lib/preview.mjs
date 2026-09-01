// `--preview`: what the build just made, as a picture.
//
// A car is judged by its outline and by where its windows sit, and neither is
// something a triangle count tells you about — a vehicle whose paint cluster
// was picked wrong passes every check in this pipeline and is obviously wrong
// at a glance. So the build can draw itself: an orthographic rasteriser with a
// depth buffer and one light, writing PNGs into /tmp.
//
// The sheet shows every level of detail side by side at the same scale, which
// is the one comparison that matters: a level that has lost its silhouette is
// a car that pops when it hands over.
//
// Nothing here ships. It is a pair of eyes for whoever edits `cars.json`.

import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { encodePng } from '../../trees/lib/png.mjs';
import { sample } from './png.mjs';

const BACKGROUND = [24, 26, 30, 255];
const LIGHT = [0.36, 0.80, 0.48];

/**
 * Draws one geometry into an RGBA canvas.
 *
 * `axis`: `side` looks along +X (the view from a platform), `front` along +Z,
 * `top` straight down — which is the one a car park is actually seen from.
 *
 * With an `image`, the texture is sampled **per pixel** through interpolated
 * coordinates, which is what a graphics card does. Sampling it once per vertex
 * and blending the three results between them — which this used to do — turns
 * a triangle whose corners land in three different places in the atlas into a
 * gentle gradient, when on screen it is a slice cut across a twentieth of the
 * whole image. That is the difference between a sheet that says the paint is
 * fine and a van that looks like crumpled foil.
 */
function raster(geometry, { rgba, width, height, metresPerPixel, originX, originY, axis, image }) {
  const depth = new Float32Array(width * height).fill(Infinity);
  const p = geometry.positions;
  const n = geometry.normals;
  const c = geometry.colors;
  const uv = geometry.uvs;
  // Which vertices take their colour from the atlas. The glass does not: it is
  // a flat dark material with no texture coordinates worth the name.
  const textured = geometry.textured;

  for (let t = 0; t < geometry.indices.length; t += 3) {
    const idx = [geometry.indices[t], geometry.indices[t + 1], geometry.indices[t + 2]];
    const pts = idx.map((i) => {
      const x = p[i * 3];
      const y = p[i * 3 + 1];
      const z = p[i * 3 + 2];
      const [u, v, d] =
        axis === 'front' ? [x, y, z] : axis === 'top' ? [x, -z, -y] : [z, y, -x];
      return { sx: originX + u / metresPerPixel, sy: originY - v / metresPerPixel, d };
    });
    const i0 = idx[0];
    // No absolute value here, deliberately. The lambert term used to be
    // `|n·l|`, which lights a face that has been turned inside out exactly as
    // brightly as one that has not — so a model whose normals were half
    // wrong drew clean sheet after clean sheet while the engine showed it in
    // shards. A surface facing away from the light gets the ambient term and
    // nothing else, the way it would on screen.
    const lambert =
      0.35 +
      0.65 *
        Math.max(0, n[i0 * 3] * LIGHT[0] + n[i0 * 3 + 1] * LIGHT[1] + n[i0 * 3 + 2] * LIGHT[2]);

    const minX = Math.max(0, Math.floor(Math.min(...pts.map((q) => q.sx))));
    const maxX = Math.min(width - 1, Math.ceil(Math.max(...pts.map((q) => q.sx))));
    const minY = Math.max(0, Math.floor(Math.min(...pts.map((q) => q.sy))));
    const maxY = Math.min(height - 1, Math.ceil(Math.max(...pts.map((q) => q.sy))));
    if (maxX < minX || maxY < minY) continue;

    const [a, b, cc] = pts;
    const area = (b.sx - a.sx) * (cc.sy - a.sy) - (cc.sx - a.sx) * (b.sy - a.sy);
    if (Math.abs(area) < 1e-9) continue;

    for (let y = minY; y <= maxY; y++) {
      for (let x = minX; x <= maxX; x++) {
        const px = x + 0.5;
        const py = y + 0.5;
        const w0 = ((b.sx - px) * (cc.sy - py) - (cc.sx - px) * (b.sy - py)) / area;
        const w1 = ((cc.sx - px) * (a.sy - py) - (a.sx - px) * (cc.sy - py)) / area;
        const w2 = 1 - w0 - w1;
        if (w0 < -1e-6 || w1 < -1e-6 || w2 < -1e-6) continue;
        const d = w0 * a.d + w1 * b.d + w2 * cc.d;
        const at = y * width + x;
        if (d >= depth[at]) continue;
        depth[at] = d;
        let tint;
        if (image && textured && textured[idx[0]] && uv) {
          const s = w0 * uv[idx[0] * 2] + w1 * uv[idx[1] * 2] + w2 * uv[idx[2] * 2];
          const r = w0 * uv[idx[0] * 2 + 1] + w1 * uv[idx[1] * 2 + 1] + w2 * uv[idx[2] * 2 + 1];
          tint = sample(image, s, r);
        } else {
          tint = [0, 1, 2].map(
            (k) => w0 * c[idx[0] * 4 + k] + w1 * c[idx[1] * 4 + k] + w2 * c[idx[2] * 4 + k],
          );
        }
        for (let k = 0; k < 3; k++) {
          rgba[at * 4 + k] = Math.round(Math.min(255, 255 * tint[k] * lambert));
        }
        rgba[at * 4 + 3] = 255;
      }
    }
  }
}

/**
 * One sheet per vehicle: every level of detail, in three views, at one scale.
 *
 * `models` is `[{ id, levels: [{ name, geometry }], length, image }]`, where
 * `image` is the vehicle's atlas — without one the vertex colours stand in.
 */
export function renderSheet(models, directory) {
  mkdirSync(directory, { recursive: true });
  for (const model of models) {
    const cell = 260;
    const views = ['side', 'front', 'top'];
    const width = cell * model.levels.length;
    const height = cell * views.length;
    const rgba = new Uint8Array(width * height * 4);
    for (let i = 0; i < rgba.length; i += 4) {
      rgba[i] = BACKGROUND[0];
      rgba[i + 1] = BACKGROUND[1];
      rgba[i + 2] = BACKGROUND[2];
      rgba[i + 3] = BACKGROUND[3];
    }
    // One scale for the whole sheet, so a level that shrank is visible as
    // having shrunk.
    const metresPerPixel = (model.length * 1.25) / cell;
    model.levels.forEach((level, column) => {
      views.forEach((axis, row) => {
        raster(level.geometry, {
          rgba,
          width,
          height,
          metresPerPixel,
          originX: column * cell + cell / 2,
          originY: row * cell + (axis === 'top' ? cell / 2 : cell * 0.78),
          axis,
          image: model.image,
        });
      });
    });
    writeFileSync(join(directory, `${model.id}.png`), encodePng(rgba, width, height));
  }
}

/**
 * Metres per pixel at one metre of distance.
 *
 * A 45° vertical field of view over a 1440-pixel window, which is what this
 * project's screenshots are taken at. Multiply by a distance and you have the
 * scale a model is actually seen at there.
 */
export const METRES_PER_PIXEL_PER_METRE = (2 * Math.tan(Math.PI / 8)) / 1440;

/**
 * The hand-over sheet: each pair of levels drawn at the scale of the distance
 * the first one hands over at, then magnified without filtering.
 *
 * This is the sheet that decides whether a level of detail is good enough. A
 * level judged at arm's length is judged at a magnification it will never be
 * seen at; judged at its own hand-over distance, the only question is whether
 * the two halves of a pair look alike — and if they do, the switch is
 * invisible in the game.
 */
export function renderHandovers(models, directory, magnify = 3) {
  mkdirSync(directory, { recursive: true });
  for (const model of models) {
    const pairs = model.levels.length - 1;
    // Wide enough for the nearest hand-over, which is the one that matters and
    // the one whose vehicle is biggest on screen. A fixed cell would crop
    // exactly the pair worth looking at.
    const nearest = model.levels[0].reach * METRES_PER_PIXEL_PER_METRE;
    const cell = Math.min(340, Math.max(96, Math.ceil((model.length * 1.35) / nearest)));
    const width = cell * 2 * pairs;
    const height = cell * 2;
    const small = new Uint8Array(width * height * 4);
    for (let i = 0; i < small.length; i += 4) {
      small[i] = BACKGROUND[0];
      small[i + 1] = BACKGROUND[1];
      small[i + 2] = BACKGROUND[2];
      small[i + 3] = BACKGROUND[3];
    }
    for (let pair = 0; pair < pairs; pair++) {
      const distance = model.levels[pair].reach;
      const metresPerPixel = distance * METRES_PER_PIXEL_PER_METRE;
      for (const [which, level] of [model.levels[pair], model.levels[pair + 1]].entries()) {
        for (const [row, axis] of ['side', 'top'].entries()) {
          raster(level.geometry, {
            rgba: small,
            width,
            height,
            metresPerPixel,
            originX: (pair * 2 + which) * cell + cell / 2,
            originY: row * cell + (axis === 'top' ? cell / 2 : cell * 0.66),
            axis,
            image: model.image,
          });
        }
      }
    }
    // Magnified with no filter: what the screen gets is what is on the sheet.
    const big = new Uint8Array(width * magnify * height * magnify * 4);
    for (let y = 0; y < height * magnify; y++) {
      for (let x = 0; x < width * magnify; x++) {
        const from = (Math.floor(y / magnify) * width + Math.floor(x / magnify)) * 4;
        const to = (y * width * magnify + x) * 4;
        for (let c = 0; c < 4; c++) big[to + c] = small[from + c];
      }
    }
    writeFileSync(
      join(directory, `${model.id}-handover.png`),
      encodePng(big, width * magnify, height * magnify),
    );
  }
}

/** Every vehicle at its finest level, side by side, seen from above. */
export function renderOverview(models, directory) {
  mkdirSync(directory, { recursive: true });
  const cell = 300;
  const columns = Math.min(4, models.length);
  const rows = Math.ceil(models.length / columns);
  const width = cell * columns;
  const height = cell * rows;
  const rgba = new Uint8Array(width * height * 4);
  for (let i = 0; i < rgba.length; i += 4) {
    rgba[i] = BACKGROUND[0];
    rgba[i + 1] = BACKGROUND[1];
    rgba[i + 2] = BACKGROUND[2];
    rgba[i + 3] = BACKGROUND[3];
  }
  // The same metres per pixel for every vehicle: a lorry has to come out
  // longer than a hatchback on the sheet, or the sheet is lying.
  const longest = Math.max(...models.map((m) => m.length));
  const metresPerPixel = (longest * 1.1) / cell;
  models.forEach((model, i) => {
    const column = i % columns;
    const row = Math.floor(i / columns);
    raster(model.levels[0].geometry, {
      rgba,
      width,
      height,
      metresPerPixel,
      originX: column * cell + cell / 2,
      originY: row * cell + cell / 2,
      axis: 'top',
      image: model.image,
    });
  });
  writeFileSync(join(directory, 'alle.png'), encodePng(rgba, width, height));
}

/**
 * The finest level, close enough to see where the glass ends.
 *
 * `renderSheet` fits a whole vehicle into 260 pixels, which is the right scale
 * for a silhouette and useless for an edge: whether the dark glass stops at
 * the window rubber or reaches a triangle's width out over the pillar is a
 * decision about a few centimetres, and at that scale a few centimetres is one
 * pixel. This draws the same geometry around the middle of the glasshouse at
 * whatever span is asked for, so the edge is a hundred pixels instead.
 */
export function renderClose(models, directory, { metres = 2.4 } = {}) {
  mkdirSync(directory, { recursive: true });
  const cell = 620;
  const views = ['side', 'front', 'top'];
  for (const model of models) {
    const width = cell * views.length;
    const rgba = new Uint8Array(width * cell * 4);
    for (let i = 0; i < rgba.length; i += 4) {
      rgba[i] = BACKGROUND[0];
      rgba[i + 1] = BACKGROUND[1];
      rgba[i + 2] = BACKGROUND[2];
      rgba[i + 3] = BACKGROUND[3];
    }
    const at = model.centre ?? [0, 1, 0];
    views.forEach((axis, column) => {
      // The same projection the rasteriser uses, so the centre lands in the
      // middle of its cell whichever way the vehicle is being looked at.
      const [u, v] =
        axis === 'front' ? [at[0], at[1]] : axis === 'top' ? [at[0], -at[2]] : [at[2], at[1]];
      raster(model.levels[0].geometry, {
        rgba,
        width,
        height: cell,
        metresPerPixel: metres / cell,
        originX: column * cell + cell / 2 - u / (metres / cell),
        originY: cell / 2 + v / (metres / cell),
        axis,
        image: model.image,
      });
    });
    writeFileSync(join(directory, `${model.id}-close.png`), encodePng(rgba, width, cell));
  }
}
