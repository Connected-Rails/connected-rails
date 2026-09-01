// What is actually inside a vehicle pack somebody downloaded.
//
//   node tools/cars/inspect_pack.mjs cache/cars/manual/<pack>
//   node tools/cars/inspect_pack.mjs cache/cars/manual/<pack>/scene.gltf
//
// A pack from an asset site is one glTF holding a dozen vehicles as named
// nodes, a buffer, and a directory of textures — and nothing about it is known
// until it is opened: what the nodes are called, whether the model is in metres
// or in centimetres, which way it faces, how many triangles it costs and how
// big its textures are. All of that has to go into `cars.json` before the build
// can do anything with it, and all of it is here in one listing.
//
// It writes nothing. It is the first thing to run on a new pack, and the thing
// to run again when one is updated and the node names have quietly changed.

import { existsSync, readdirSync, statSync } from 'node:fs';
import { join, extname } from 'node:path';

import { readGltf, partsOf, boundsOf } from './lib/glb.mjs';
import { decodePng } from './lib/png.mjs';

/** The glTF inside a directory, or the file itself. */
function find(target) {
  if (!existsSync(target)) throw new Error(`${target} is not there`);
  if (statSync(target).isFile()) return target;
  const walk = (directory) => {
    for (const entry of readdirSync(directory)) {
      const path = join(directory, entry);
      if (statSync(path).isDirectory()) {
        const found = walk(path);
        if (found) return found;
      } else if (['.gltf', '.glb'].includes(extname(entry).toLowerCase())) {
        return path;
      }
    }
    return null;
  };
  const found = walk(target);
  if (!found) throw new Error(`${target}: no .gltf or .glb in it`);
  return found;
}

/**
 * The scale the pack is modelled at.
 *
 * Guessed from the biggest thing in it: an asset site's converter leaves a
 * model in whatever unit it was authored in, and a pack in centimetres arrives
 * a hundred times too big. A car is between three and six metres long, so the
 * power of ten that puts the longest vehicle in that range is almost certainly
 * the one — and the build states it in `cars.json` rather than guessing again.
 */
function guessScale(longest) {
  for (const scale of [1, 0.01, 0.1, 10, 100, 0.001]) {
    const metres = longest * scale;
    if (metres > 2.5 && metres < 25) return scale;
  }
  return 1;
}

function main() {
  const target = process.argv[2];
  if (!target) {
    console.error('usage: node tools/cars/inspect_pack.mjs <directory or .gltf>');
    process.exit(1);
  }
  const path = find(target);
  const { json, bin, base } = readGltf(path);
  const parts = partsOf(json, bin, path);
  console.log(`${path}`);
  console.log(
    `  ${json.asset?.generator ?? 'unknown generator'} — ${parts.length} primitives, ` +
      `${(json.materials ?? []).length} materials, ${(json.images ?? []).length} images`,
  );

  // Grouped by the top-level node, which is how a pack names its vehicles.
  const byName = new Map();
  for (const part of parts) {
    const key = part.name;
    if (!byName.has(key)) byName.set(key, []);
    byName.get(key).push(part);
  }

  const whole = boundsOf(parts);
  const longest = Math.max(...whole.size);
  const scale = guessScale(Math.max(...[...byName.values()].map((p) => Math.max(...boundsOf(p).size))));
  console.log(
    `  everything together: ${whole.size.map((v) => v.toFixed(2)).join(' × ')} ` +
      `(longest ${longest.toFixed(2)}), so the unit looks like ${scale === 1 ? 'metres' : `${1 / scale} per metre`}`,
  );
  console.log('');
  console.log('  node                             tris     size in metres (x × y × z)   materials');
  for (const [name, group] of byName) {
    const box = boundsOf(group);
    const tris = group.reduce((sum, p) => sum + p.indices.length / 3, 0);
    const materials = [...new Set(group.map((p) => p.material))]
      .map((m) => json.materials?.[m]?.name ?? m)
      .join(',');
    console.log(
      `  ${name.slice(0, 30).padEnd(32)} ${String(tris).padStart(6)}   ` +
        `${box.size.map((v) => (v * scale).toFixed(2).padStart(6)).join(' × ')}   ${materials}`,
    );
  }

  if (json.images?.length) {
    console.log('');
    console.log('  images');
    for (const image of json.images) {
      if (!image.uri || image.uri.startsWith('data:')) {
        console.log(`  ${(image.name ?? 'embedded').padEnd(40)} (inside the file)`);
        continue;
      }
      const file = join(base ?? '.', decodeURIComponent(image.uri));
      let note = '';
      try {
        const bytes = statSync(file).size;
        note = `${(bytes / 1024).toFixed(0)} KB`;
        if (file.toLowerCase().endsWith('.png')) {
          const { width, height } = decodePng(file);
          note += `, ${width}×${height}`;
        }
      } catch {
        note = 'missing';
      }
      console.log(`  ${image.uri.slice(0, 40).padEnd(40)} ${note}`);
    }
  }
}

main();
