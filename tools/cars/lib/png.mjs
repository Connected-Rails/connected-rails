// Reading a PNG, far enough to sample a palette out of it.
//
// The kit's models are textured with one 512×512 sheet of colour swatches —
// no detail, no photographs, just the paint of every part laid out in cells.
// A texture like that is a lookup table wearing a costume, so this pipeline
// reads it once at build time and bakes the answer into the vertices
// (`build_cars.mjs`). The mod then ships no bitmap at all, which is the same
// bargain the roads and the track already take: nothing to load, nothing to
// filter, and two clients of a multiplayer run agree on the colour of a car
// without a byte crossing the network.
//
// `inflateSync` is Node's own, so this is a filter reverser and nothing more.

import { readFileSync } from 'node:fs';
import { inflateSync } from 'node:zlib';

const SIGNATURE = [137, 80, 78, 71, 13, 10, 26, 10];

/** Bytes per pixel of each colour type, at eight bits a channel. */
const CHANNELS = { 0: 1, 2: 3, 3: 1, 4: 2, 6: 4 };

/**
 * Decodes an eight-bit PNG to RGBA.
 *
 * Returns `{ width, height, pixels }`, `pixels` row by row from the top.
 * Interlaced, sixteen-bit and paletted images throw — the sheet this reads is
 * none of them, and a wrong guess would be a wrong colour on every car.
 */
export function decodePng(path) {
  const bytes = readFileSync(path);
  for (let i = 0; i < SIGNATURE.length; i++) {
    if (bytes[i] !== SIGNATURE[i]) throw new Error(`${path}: not a png`);
  }
  let offset = 8;
  let header = null;
  const data = [];
  while (offset + 8 <= bytes.length) {
    const length = bytes.readUInt32BE(offset);
    const kind = bytes.toString('ascii', offset + 4, offset + 8);
    const chunk = bytes.subarray(offset + 8, offset + 8 + length);
    if (kind === 'IHDR') {
      header = {
        width: chunk.readUInt32BE(0),
        height: chunk.readUInt32BE(4),
        depth: chunk[8],
        colour: chunk[9],
        interlace: chunk[12],
      };
    } else if (kind === 'IDAT') {
      data.push(chunk);
    } else if (kind === 'IEND') {
      break;
    }
    offset += 12 + length;
  }
  if (!header) throw new Error(`${path}: no header`);
  if (header.depth !== 8) throw new Error(`${path}: ${header.depth} bits a channel`);
  if (header.interlace) throw new Error(`${path}: interlaced`);
  const channels = CHANNELS[header.colour];
  if (!channels || header.colour === 3) {
    throw new Error(`${path}: colour type ${header.colour}`);
  }

  const raw = inflateSync(Buffer.concat(data));
  const { width, height } = header;
  const stride = width * channels;
  const out = Buffer.alloc(width * height * 4);
  let previous = Buffer.alloc(stride);
  let at = 0;
  for (let y = 0; y < height; y++) {
    const filter = raw[at++];
    const row = Buffer.from(raw.subarray(at, at + stride));
    at += stride;
    unfilter(filter, row, previous, channels);
    for (let x = 0; x < width; x++) {
      const from = x * channels;
      const to = (y * width + x) * 4;
      // Grey (0), grey+alpha (4), RGB (2), RGBA (6) — in that order the first
      // channel is either the grey or the red, which is what makes this one
      // expression rather than four.
      const grey = channels <= 2;
      out[to] = row[from];
      out[to + 1] = grey ? row[from] : row[from + 1];
      out[to + 2] = grey ? row[from] : row[from + 2];
      out[to + 3] = channels === 4 ? row[from + 3] : channels === 2 ? row[from + 1] : 255;
    }
    previous = row;
  }
  return { width, height, pixels: out };
}

/** Reverses one row's filter, in place. */
function unfilter(filter, row, previous, channels) {
  const stride = row.length;
  switch (filter) {
    case 0:
      break;
    case 1:
      for (let i = channels; i < stride; i++) row[i] = (row[i] + row[i - channels]) & 0xff;
      break;
    case 2:
      for (let i = 0; i < stride; i++) row[i] = (row[i] + previous[i]) & 0xff;
      break;
    case 3:
      for (let i = 0; i < stride; i++) {
        const left = i >= channels ? row[i - channels] : 0;
        row[i] = (row[i] + ((left + previous[i]) >> 1)) & 0xff;
      }
      break;
    case 4:
      for (let i = 0; i < stride; i++) {
        const left = i >= channels ? row[i - channels] : 0;
        const up = previous[i];
        const corner = i >= channels ? previous[i - channels] : 0;
        row[i] = (row[i] + paeth(left, up, corner)) & 0xff;
      }
      break;
    default:
      throw new Error(`png filter ${filter}`);
  }
}

function paeth(a, b, c) {
  const p = a + b - c;
  const pa = Math.abs(p - a);
  const pb = Math.abs(p - b);
  const pc = Math.abs(p - c);
  if (pa <= pb && pa <= pc) return a;
  return pb <= pc ? b : c;
}

/**
 * The grid of swatches in a palette sheet, found by looking for the seams.
 *
 * The sheet is a table of colours and every cell holds a gradient, so a
 * colour cannot be its own identity: two shades of one swatch are one paint,
 * and two swatches can be closer to each other than the ends of a single
 * gradient are. What *is* an identity is the cell — and the cell boundaries
 * are the columns and rows where the picture jumps rather than slides.
 *
 * Found rather than assumed, because it is one scan of a 512-pixel image and
 * it survives the next version of the kit rearranging its sheet.
 */
export function paletteGrid(image, jump = 10) {
  const at = (x, y) => {
    const i = (y * image.width + x) * 4;
    return [image.pixels[i], image.pixels[i + 1], image.pixels[i + 2]];
  };
  const apart = (a, b) =>
    Math.max(Math.abs(a[0] - b[0]), Math.abs(a[1] - b[1]), Math.abs(a[2] - b[2]));
  const columns = [0];
  for (let x = 1; x < image.width; x++) {
    let most = 0;
    for (let y = 0; y < image.height; y += 7) most = Math.max(most, apart(at(x, y), at(x - 1, y)));
    if (most > jump) columns.push(x);
  }
  const rows = [0];
  for (let y = 1; y < image.height; y++) {
    let most = 0;
    for (let x = 0; x < image.width; x += 7) most = Math.max(most, apart(at(x, y), at(x, y - 1)));
    if (most > jump) rows.push(y);
  }
  return { columns, rows, width: image.width, height: image.height };
}

/** Which cell of the sheet a texture coordinate falls in. */
export function cellOf(grid, u, v) {
  const wrap = (t, n) => {
    const i = Math.floor(t * n);
    return ((i % n) + n) % n;
  };
  const x = wrap(u, grid.width);
  // glTF puts the origin of a texture coordinate at the *top* left, so v
  // counts down the image and there is no flip to undo. Getting this the
  // other way round is not a subtle mistake: on a sheet of swatches it reads
  // every model's paint out of the wrong row, and reads it consistently, so
  // the result looks deliberate.
  const y = wrap(v, grid.height);
  let column = 0;
  while (column + 1 < grid.columns.length && grid.columns[column + 1] <= x) column++;
  let row = 0;
  while (row + 1 < grid.rows.length && grid.rows[row + 1] <= y) row++;
  return row * grid.columns.length + column;
}

/**
 * Samples an image at a texture coordinate, nearest neighbour, wrapping.
 *
 * Nearest and not bilinear: every texel of a swatch sheet is a decision, and
 * a filtered read at the seam between two swatches is a colour that is in
 * neither of them.
 */
export function sample(image, u, v) {
  const wrap = (t, n) => {
    const i = Math.floor(t * n);
    return ((i % n) + n) % n;
  };
  const x = wrap(u, image.width);
  // v counts down the image — glTF's origin is the top left corner.
  const y = wrap(v, image.height);
  const at = (y * image.width + x) * 4;
  return [
    image.pixels[at] / 255,
    image.pixels[at + 1] / 255,
    image.pixels[at + 2] / 255,
    image.pixels[at + 3] / 255,
  ];
}
