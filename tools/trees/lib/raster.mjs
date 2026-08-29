// A tiny painting surface: RGBA float buffers, anti-aliased polygons and
// strokes, and the alpha dilation a cut-out texture needs.
//
// Node has no canvas, and pulling one in for a handful of leaf silhouettes
// would be a native dependency for the whole pipeline. Coverage is sampled on
// a 4 × 4 grid per pixel, which is smooth enough for foliage cards that end up
// twenty pixels tall on screen.

const SUB = 4;

export class Surface {
  constructor(width, height) {
    this.width = width;
    this.height = height;
    // Straight (non-premultiplied) RGBA; alpha accumulates by `over`.
    this.data = new Float32Array(width * height * 4);
  }

  /** Fills every texel with one colour. */
  clear(r, g, b, a) {
    for (let i = 0; i < this.data.length; i += 4) {
      this.data[i] = r;
      this.data[i + 1] = g;
      this.data[i + 2] = b;
      this.data[i + 3] = a;
    }
  }

  /** Source-over of a straight-alpha colour at one texel. */
  blend(x, y, r, g, b, a) {
    if (a <= 0 || x < 0 || y < 0 || x >= this.width || y >= this.height) return;
    const i = (y * this.width + x) * 4;
    const dst = this.data[i + 3];
    const out = a + dst * (1 - a);
    if (out <= 0) {
      this.data[i + 3] = 0;
      return;
    }
    const w = dst * (1 - a);
    this.data[i] = (this.data[i] * w + r * a) / out;
    this.data[i + 1] = (this.data[i + 1] * w + g * a) / out;
    this.data[i + 2] = (this.data[i + 2] * w + b * a) / out;
    this.data[i + 3] = out;
  }

  /** Reads a texel back as `[r, g, b, a]`. */
  at(x, y) {
    const i = (Math.min(Math.max(x, 0), this.width - 1) +
      Math.min(Math.max(y, 0), this.height - 1) * this.width) * 4;
    return [this.data[i], this.data[i + 1], this.data[i + 2], this.data[i + 3]];
  }

  /**
   * Fills a closed polygon (`[[x, y], …]`, pixel coordinates) with the colour
   * `shade(x, y)` returns — `[r, g, b]` or `[r, g, b, a]`.
   */
  fill(points, shade) {
    if (points.length < 3) return;
    let minX = Infinity;
    let maxX = -Infinity;
    let minY = Infinity;
    let maxY = -Infinity;
    for (const [x, y] of points) {
      if (x < minX) minX = x;
      if (x > maxX) maxX = x;
      if (y < minY) minY = y;
      if (y > maxY) maxY = y;
    }
    const x0 = Math.max(0, Math.floor(minX));
    const x1 = Math.min(this.width - 1, Math.ceil(maxX));
    const y0 = Math.max(0, Math.floor(minY));
    const y1 = Math.min(this.height - 1, Math.ceil(maxY));
    if (x1 < x0 || y1 < y0) return;

    const cover = new Float32Array((x1 - x0 + 1) * (y1 - y0 + 1));
    const spans = [];
    for (let py = y0; py <= y1; py++) {
      for (let s = 0; s < SUB; s++) {
        const sy = py + (s + 0.5) / SUB;
        spans.length = 0;
        for (let i = 0, j = points.length - 1; i < points.length; j = i++) {
          const [ax, ay] = points[j];
          const [bx, by] = points[i];
          if (ay === by) continue;
          if (sy < Math.min(ay, by) || sy >= Math.max(ay, by)) continue;
          spans.push(ax + ((sy - ay) / (by - ay)) * (bx - ax));
        }
        if (spans.length < 2) continue;
        spans.sort((a, b) => a - b);
        for (let k = 0; k + 1 < spans.length; k += 2) {
          const sa = spans[k];
          const sb = spans[k + 1];
          const from = Math.max(x0, Math.floor(sa));
          const to = Math.min(x1, Math.ceil(sb));
          for (let px = from; px <= to; px++) {
            // Horizontal coverage of this sub-scanline inside the pixel.
            const lo = Math.max(sa, px);
            const hi = Math.min(sb, px + 1);
            if (hi > lo) {
              cover[(py - y0) * (x1 - x0 + 1) + (px - x0)] += (hi - lo) / SUB;
            }
          }
        }
      }
    }

    for (let py = y0; py <= y1; py++) {
      for (let px = x0; px <= x1; px++) {
        const c = Math.min(1, cover[(py - y0) * (x1 - x0 + 1) + (px - x0)]);
        if (c <= 0.001) continue;
        const rgba = shade(px + 0.5, py + 0.5);
        if (!rgba) continue;
        this.blend(px, py, rgba[0], rgba[1], rgba[2], c * (rgba.length > 3 ? rgba[3] : 1));
      }
    }
  }

  /**
   * Strokes a polyline of `[[x, y], …]` with a width that may taper: `width`
   * is either a number or `(t) => number` over the run of the line.
   */
  stroke(points, width, shade) {
    const w = typeof width === 'function' ? width : () => width;
    for (let i = 0; i + 1 < points.length; i++) {
      const [ax, ay] = points[i];
      const [bx, by] = points[i + 1];
      const dx = bx - ax;
      const dy = by - ay;
      const len = Math.hypot(dx, dy) || 1;
      const nx = -dy / len;
      const ny = dx / len;
      const wa = w(i / Math.max(1, points.length - 1)) / 2;
      const wb = w((i + 1) / Math.max(1, points.length - 1)) / 2;
      this.fill(
        [
          [ax + nx * wa, ay + ny * wa],
          [bx + nx * wb, by + ny * wb],
          [bx - nx * wb, by - ny * wb],
          [ax - nx * wa, ay - ny * wa],
        ],
        shade,
      );
    }
  }

  /** Separable box blur, `radius` texels, wrapping or clamping at the edge. */
  blur(radius, wrap = false) {
    if (radius < 1) return;
    const { width, height, data } = this;
    const tmp = new Float32Array(data.length);
    const idx = (v, n) => (wrap ? ((v % n) + n) % n : Math.min(Math.max(v, 0), n - 1));
    for (let y = 0; y < height; y++) {
      for (let x = 0; x < width; x++) {
        for (let c = 0; c < 4; c++) {
          let sum = 0;
          for (let k = -radius; k <= radius; k++) sum += data[(y * width + idx(x + k, width)) * 4 + c];
          tmp[(y * width + x) * 4 + c] = sum / (radius * 2 + 1);
        }
      }
    }
    for (let y = 0; y < height; y++) {
      for (let x = 0; x < width; x++) {
        for (let c = 0; c < 4; c++) {
          let sum = 0;
          for (let k = -radius; k <= radius; k++) sum += tmp[(idx(y + k, height) * width + x) * 4 + c];
          data[(y * width + x) * 4 + c] = sum / (radius * 2 + 1);
        }
      }
    }
  }

  /**
   * Pushes the colour of the covered texels outwards into the transparent
   * ones, `passes` texels far. Without it a leaf's silhouette bleeds towards
   * whatever the empty texels happen to hold as soon as the GPU filters or
   * mips the texture, and a canopy grows a dark halo at a distance.
   */
  dilateAlpha(passes = 8) {
    const { width, height, data } = this;
    for (let pass = 0; pass < passes; pass++) {
      const copy = Float32Array.from(data);
      for (let y = 0; y < height; y++) {
        for (let x = 0; x < width; x++) {
          const i = (y * width + x) * 4;
          if (copy[i + 3] > 0.004) continue;
          let r = 0;
          let g = 0;
          let b = 0;
          let n = 0;
          for (let dy = -1; dy <= 1; dy++) {
            for (let dx = -1; dx <= 1; dx++) {
              const sx = x + dx;
              const sy = y + dy;
              if (sx < 0 || sy < 0 || sx >= width || sy >= height) continue;
              const j = (sy * width + sx) * 4;
              if (copy[j + 3] <= 0.004) continue;
              r += copy[j];
              g += copy[j + 1];
              b += copy[j + 2];
              n++;
            }
          }
          if (n === 0) continue;
          data[i] = r / n;
          data[i + 1] = g / n;
          data[i + 2] = b / n;
          // Alpha stays where it was — only the colour spreads.
        }
      }
    }
  }

  /** The surface as the RGBA8 bytes {@link encodePng} wants. */
  toRgba8() {
    const out = new Uint8Array(this.width * this.height * 4);
    for (let i = 0; i < out.length; i++) {
      out[i] = Math.round(Math.min(1, Math.max(0, this.data[i])) * 255);
    }
    return out;
  }
}

/** Linear interpolation between two `[r, g, b]` colours. */
export function mix(a, b, t) {
  const k = Math.min(1, Math.max(0, t));
  return [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k, a[2] + (b[2] - a[2]) * k];
}

/** `#rrggbb` or `0xrrggbb` to a linear-ish `[r, g, b]` in 0…1 (sRGB values). */
export function rgb(hex) {
  const v = typeof hex === 'string' ? parseInt(hex.replace('#', ''), 16) : hex;
  return [((v >> 16) & 0xff) / 255, ((v >> 8) & 0xff) / 255, (v & 0xff) / 255];
}

/** Multiplies a colour towards white — the pipeline's snow and haze. */
export function lighten(c, t) {
  return mix(c, [1, 1, 1], t);
}
