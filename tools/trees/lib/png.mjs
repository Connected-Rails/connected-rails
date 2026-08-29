// Minimal PNG codec — RGBA8 out, anything non-interlaced in.
//
// The pipeline paints its own bark and puts the finished atlases on disk, and
// it reads the scanned leaf sheets of ambientCG back in to cut single leaves
// out of them. Node's zlib does the compression either way; the rest is the
// container and the scanline filters.

import { deflateSync, inflateSync } from 'node:zlib';

const CRC_TABLE = (() => {
  const table = new Int32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    table[n] = c;
  }
  return table;
})();

function crc32(buffer) {
  let c = -1;
  for (let i = 0; i < buffer.length; i++) c = CRC_TABLE[(c ^ buffer[i]) & 0xff] ^ (c >>> 8);
  return (c ^ -1) >>> 0;
}

function chunk(type, data) {
  const out = Buffer.alloc(12 + data.length);
  out.writeUInt32BE(data.length, 0);
  out.write(type, 4, 'ascii');
  data.copy(out, 8);
  out.writeUInt32BE(crc32(out.subarray(4, 8 + data.length)), 8 + data.length);
  return out;
}

/**
 * Encodes RGBA8 pixels as a PNG.
 *
 * Every scanline is filtered with Paeth (filter 4): bark and foliage are
 * smooth gradients with noise on top, where Paeth beats the alternatives by a
 * wide margin and the encoder stays a single pass.
 *
 * @param {Uint8Array} rgba `width * height * 4` bytes
 * @returns {Buffer}
 */
export function encodePng(rgba, width, height) {
  const stride = width * 4;
  const raw = Buffer.alloc((stride + 1) * height);
  for (let y = 0; y < height; y++) {
    const row = y * stride;
    const out = y * (stride + 1);
    raw[out] = 4;
    for (let x = 0; x < stride; x++) {
      const a = x >= 4 ? rgba[row + x - 4] : 0;
      const b = y > 0 ? rgba[row - stride + x] : 0;
      const c = x >= 4 && y > 0 ? rgba[row - stride + x - 4] : 0;
      const p = a + b - c;
      const pa = Math.abs(p - a);
      const pb = Math.abs(p - b);
      const pc = Math.abs(p - c);
      const pred = pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
      raw[out + 1 + x] = (rgba[row + x] - pred) & 0xff;
    }
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // colour type: RGBA
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', ihdr),
    chunk('IDAT', deflateSync(raw, { level: 9 })),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

/**
 * Decodes a PNG into straight RGBA8.
 *
 * Everything the scanned sheets come in is covered: 8 and 16 bits per channel,
 * greyscale, palette, RGB and RGBA, with or without an alpha channel. Adam7
 * interlacing is not — nothing that is downloaded uses it, and a silent wrong
 * answer would be worse than the throw.
 *
 * @param {Buffer} buffer
 * @returns {{ width: number, height: number, data: Uint8Array }}
 */
export function decodePng(buffer) {
  if (buffer.readUInt32BE(0) !== 0x89504e47) throw new Error('not a PNG');
  let offset = 8;
  let header = null;
  let palette = null;
  let transparency = null;
  const idat = [];
  while (offset < buffer.length) {
    const length = buffer.readUInt32BE(offset);
    const type = buffer.toString('ascii', offset + 4, offset + 8);
    const body = buffer.subarray(offset + 8, offset + 8 + length);
    if (type === 'IHDR') {
      header = {
        width: body.readUInt32BE(0),
        height: body.readUInt32BE(4),
        depth: body[8],
        colorType: body[9],
        interlace: body[12],
      };
      if (header.interlace) throw new Error('interlaced PNG is not supported');
    } else if (type === 'PLTE') {
      palette = Buffer.from(body);
    } else if (type === 'tRNS') {
      transparency = Buffer.from(body);
    } else if (type === 'IDAT') {
      idat.push(Buffer.from(body));
    } else if (type === 'IEND') {
      break;
    }
    offset += 12 + length;
  }
  if (!header) throw new Error('PNG without a header');

  const { width, height, depth, colorType } = header;
  const channels = { 0: 1, 2: 3, 3: 1, 4: 2, 6: 4 }[colorType];
  if (!channels) throw new Error(`unsupported PNG colour type ${colorType}`);
  const bytesPerSample = depth === 16 ? 2 : 1;
  if (depth !== 8 && depth !== 16) throw new Error(`unsupported PNG bit depth ${depth}`);
  const pixelBytes = channels * bytesPerSample;
  const stride = width * pixelBytes;

  const raw = inflateSync(Buffer.concat(idat));
  const lines = Buffer.alloc(height * stride);
  for (let y = 0; y < height; y++) {
    const filter = raw[y * (stride + 1)];
    const from = y * (stride + 1) + 1;
    const to = y * stride;
    for (let x = 0; x < stride; x++) {
      const value = raw[from + x];
      const a = x >= pixelBytes ? lines[to + x - pixelBytes] : 0;
      const b = y > 0 ? lines[to - stride + x] : 0;
      const c = x >= pixelBytes && y > 0 ? lines[to - stride + x - pixelBytes] : 0;
      let recon;
      switch (filter) {
        case 0: recon = value; break;
        case 1: recon = value + a; break;
        case 2: recon = value + b; break;
        case 3: recon = value + ((a + b) >> 1); break;
        case 4: {
          const p = a + b - c;
          const pa = Math.abs(p - a);
          const pb = Math.abs(p - b);
          const pc = Math.abs(p - c);
          recon = value + (pa <= pb && pa <= pc ? a : pb <= pc ? b : c);
          break;
        }
        default: throw new Error(`unknown PNG filter ${filter}`);
      }
      lines[to + x] = recon & 0xff;
    }
  }

  const data = new Uint8Array(width * height * 4);
  for (let i = 0; i < width * height; i++) {
    const at = (channel) => lines[i * pixelBytes + channel * bytesPerSample];
    let r;
    let g;
    let b;
    let a = 255;
    switch (colorType) {
      case 0: r = g = b = at(0); break;
      case 4: r = g = b = at(0); a = at(1); break;
      case 2: [r, g, b] = [at(0), at(1), at(2)]; break;
      case 6: [r, g, b, a] = [at(0), at(1), at(2), at(3)]; break;
      case 3: {
        const index = at(0);
        [r, g, b] = [palette[index * 3], palette[index * 3 + 1], palette[index * 3 + 2]];
        if (transparency && index < transparency.length) a = transparency[index];
        break;
      }
      default: throw new Error(`unsupported PNG colour type ${colorType}`);
    }
    data.set([r, g, b, a], i * 4);
  }
  return { width, height, data };
}
