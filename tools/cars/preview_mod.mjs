// What the *built mod* looks like — not what the build thinks it built.
//
//   node tools/cars/preview_mod.mjs            # every vehicle in mods/cars
//   node tools/cars/preview_mod.mjs kleinwagen
//
// `build_cars.mjs --preview` draws the geometry as it is about to be written:
// good for a level of detail gone wrong, blind to everything that happens
// afterwards. This reads back what is actually on disk — the glTF, its own
// texture coordinates, and the block-compressed atlas beside it — and draws
// that. If a car is broken in the game, it is broken on this sheet.
//
// It exists because it settled an argument that screenshots could not: seven
// of eight vehicles were fine and the eighth was a white shell with black
// shards over it, and the difference was one missing texture. Framing a
// screenshot in the editor took ten attempts; this took one.
//
// ImageMagick converts the DDS, since Node cannot read block compression.

import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { partsOf, readGltf } from './lib/glb.mjs';
import { decodePng } from './lib/png.mjs';
import { renderSheet, renderOverview, renderClose } from './lib/preview.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const assetDir = join(here, '..', '..', 'mods', 'cars', 'assets');
const out = '/tmp/car-mod';

/** The atlas of a vehicle, converted to something Node can read. */
function atlasOf(id) {
  const dds = join(assetDir, `${id}.dds`);
  if (!existsSync(dds)) return null;
  mkdirSync(out, { recursive: true });
  const png = join(out, `${id}-atlas.png`);
  // `[0]` is the top mip level; without it ImageMagick writes the whole chain.
  execFileSync('magick', [`${dds}[0]`, '-depth', '8', `PNG24:${png}`]);
  return decodePng(png);
}

/** One level of one vehicle, textured as the material would texture it. */
function level(json, bin, node, image, file) {
  const one = { ...json, scene: 0, scenes: [{ nodes: [node] }] };
  const geometry = { positions: [], normals: [], uvs: [], colors: [], textured: [], indices: [] };
  for (const part of partsOf(one, bin, file)) {
    const base = geometry.positions.length / 3;
    geometry.positions.push(...part.positions);
    geometry.normals.push(...part.normals);
    geometry.uvs.push(...part.uvs);
    // Material 1 is the glass: dark, smooth and untextured by design.
    const glass = part.material === 1;
    for (let i = 0; i < part.positions.length / 3; i++) {
      // The paint is sampled per pixel by the rasteriser, through these very
      // coordinates. Only the glass and the untextured fallback carry a colour
      // of their own.
      geometry.textured.push(glass || !image ? 0 : 1);
      if (glass) geometry.colors.push(0.08, 0.09, 0.11, 1);
      else geometry.colors.push(0.82, 0.82, 0.84, 1);
    }
    geometry.indices.push(...part.indices.map((i) => i + base));
  }
  return geometry;
}

function main() {
  const only = process.argv.slice(2);
  const models = [];
  for (const file of readdirSync(assetDir).filter((f) => f.endsWith('.gltf')).sort()) {
    const id = file.replace('.gltf', '');
    if (only.length && !only.includes(id)) continue;
    const { json, bin } = readGltf(join(assetDir, file));
    const image = atlasOf(id);
    const scene = json.scenes[json.scene ?? 0];
    const levels = scene.nodes.map((node) => ({
      name: json.nodes[node].name ?? `LOD${node}`,
      geometry: level(json, bin, node, image, file),
    }));
    const tris = levels[0].geometry.indices.length / 3;
    console.log(
      `  ${id.padEnd(15)} ${String(tris).padStart(6)} tris at LOD0, ` +
        `${levels.length} levels, atlas ${image ? `${image.width}²` : 'MISSING'}`,
    );
    // Where the glass sits, so the close-up has something to aim at: the
    // middle of the windows is the one place on a car worth magnifying.
    const glass = levels[0].geometry;
    let centre = null;
    let count = 0;
    const seen = new Set();
    for (let i = 0; i < glass.indices.length; i++) {
      const v = glass.indices[i];
      if (glass.textured[v] || seen.has(v)) continue;
      seen.add(v);
      centre = centre ?? [0, 0, 0];
      for (let c = 0; c < 3; c++) centre[c] += glass.positions[v * 3 + c];
      count++;
    }
    if (count) centre = centre.map((c) => c / count);
    // Long enough that the sheet's scale suits a van as well as a hatchback.
    models.push({ id, length: 5.5, levels, image, centre });
  }
  renderSheet(models, out);
  renderOverview(models, out);
  renderClose(models, out);
  console.log(`sheets in ${out}`);
}

main();
