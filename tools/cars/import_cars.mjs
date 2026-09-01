// Unpacks the vehicle archives and gets their textures into a shape the build
// can read.
//
//   node tools/cars/import_cars.mjs [~/Downloads]
//
// Each archive holds one FBX and one baked colour atlas, usually 4096 square.
// Two things happen here and nowhere else:
//
//  * the zip is unpacked into `cache/cars/manual/src-<id>/`, git-ignored,
//    because five megabytes of source per car has no business in the history —
//    what ships is what the build makes of it;
//  * the atlas is written twice: once as a PNG the build can *read* (Node has
//    no JPEG decoder, and the build has to sample the texture to tell a window
//    from the roof above it), and once as the JPEG the mod actually ships,
//    scaled down.
//
// Four thousand pixels square is a photograph of a car; a car in a car park is
// two hundred pixels of screen. The shipped atlas is a quarter of that on a
// side, which is three per cent of the bytes and no visible difference at any
// distance this vehicle is ever seen from.
//
// `magick` (ImageMagick) does the scaling. It is the one tool beyond Node this
// pipeline needs, and it is needed for a reason no amount of JavaScript would
// avoid: the source is a JPEG.

import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { basename, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, '..', '..');
export const sourceDir = join(root, 'cache', 'cars', 'manual');
export const textureDir = join(root, 'cache', 'cars', 'tex');

/**
 * Side of the atlas the mod ships, and the one the build reads.
 *
 * A thousand and twenty-four, which is four times the texels an ordinary
 * atlas of a car would need. These are not ordinary atlases: they are
 * hundreds of small scraps of photograph packed together, so the resolution
 * that matters is the resolution of one scrap, not of the sheet. Five hundred
 * and twelve was tried and it is not enough — a door panel comes out sixty
 * texels across, which is soft at the distance a car park is walked past at.
 *
 * The image is written once, at the size it ships at, because the build both
 * *reads* it — to ask what colour a face is — and packs it. The block
 * compression and the mip chain happen in `build_cars.mjs`, which is the only
 * place that knows which parts of the atlas the model actually uses.
 */
export const SHIPPED = 1024;

/** `VW Golf 3D.zip` → `vw-golf`. */
export function idOf(name) {
  return basename(name, '.zip')
    .replace(/\s*3D\s*$/i, '')
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-|-$/g, '');
}

/** Every FBX under a source directory. */
export function fbxIn(directory) {
  const found = [];
  const walk = (at) => {
    for (const entry of readdirSync(at)) {
      const path = join(at, entry);
      if (statSync(path).isDirectory()) walk(path);
      else if (entry.toLowerCase().endsWith('.fbx')) found.push(path);
    }
  };
  if (existsSync(directory)) walk(directory);
  return found;
}

/** The colour atlas beside an FBX, whatever the exporter called its folder. */
export function textureIn(directory) {
  const found = [];
  const walk = (at) => {
    for (const entry of readdirSync(at)) {
      const path = join(at, entry);
      if (statSync(path).isDirectory()) walk(path);
      else if (/\.(jpe?g|png)$/i.test(entry)) found.push(path);
    }
  };
  if (existsSync(directory)) walk(directory);
  // A base colour if it says so, else the biggest file: an atlas is always the
  // biggest thing in one of these archives.
  const colour = found.filter((f) => /base_?colou?r|albedo|diffuse/i.test(f));
  const pick = (colour.length ? colour : found).sort(
    (a, b) => statSync(b).size - statSync(a).size,
  );
  return pick[0] ?? null;
}

/** Where the build reads a vehicle's texture. */
export function readableTexture(id) {
  return join(textureDir, `${id}-read.png`);
}

function magick(args) {
  try {
    execFileSync('magick', args, { stdio: 'pipe' });
  } catch (error) {
    throw new Error(
      `ImageMagick failed (${args.join(' ')}). It is needed to read and scale the ` +
        `texture atlases — install it, or convert them by hand into ${textureDir}.\n${error.message}`,
    );
  }
}

function main() {
  const from = process.argv[2] ?? join(process.env.HOME ?? '', 'Downloads');
  const archives = readdirSync(from)
    .filter((f) => f.toLowerCase().endsWith('.zip'))
    .map((f) => join(from, f));
  const catalogue = JSON.parse(readFileSync(join(here, 'cars.json'), 'utf8'));
  const wanted = new Set(catalogue.types.map((t) => t.source));

  mkdirSync(sourceDir, { recursive: true });
  mkdirSync(textureDir, { recursive: true });
  let taken = 0;
  for (const archive of archives) {
    const id = idOf(archive);
    if (!wanted.has(id)) continue;
    const directory = join(sourceDir, `src-${id}`);
    mkdirSync(directory, { recursive: true });
    execFileSync('unzip', ['-o', '-q', archive, '-d', directory], { stdio: 'inherit' });
    const fbx = fbxIn(directory)[0];
    const texture = textureIn(directory);
    if (!fbx) {
      console.log(`  ${id}: no FBX in the archive`);
      continue;
    }
    if (texture) {
      magick([
        texture,
        '-resize', `${SHIPPED}x${SHIPPED}`,
        '-depth', '8',
        `PNG24:${readableTexture(id)}`,
      ]);
    }
    const bytes = texture ? statSync(readableTexture(id)).size : 0;
    console.log(
      `  ${id.padEnd(18)} ${basename(fbx).padEnd(28)} ` +
        `${texture ? `atlas ${SHIPPED}² ${(bytes / 1024).toFixed(0)} KB` : 'no texture'}`,
    );
    taken++;
  }
  console.log(`${taken} of ${wanted.size} vehicles imported into ${sourceDir}`);
  const missing = [...wanted].filter((id) => !fbxIn(join(sourceDir, `src-${id}`)).length);
  if (missing.length) console.log(`still missing: ${missing.join(', ')}`);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
