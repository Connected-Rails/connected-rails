// Fetches the scanned leaf and bark sheets the foliage cards are cut out of.
//
// Two CC0 libraries, both public domain, neither redistributed:
//
//   * **ambientCG** (https://ambientcg.com, CC0 1.0) — `LeafSet###`, sheets of
//     single leaves photographed on black with an opacity map beside them.
//     `lib/leaves.mjs` cuts the individual leaves out of those.
//   * **Poly Haven** (https://polyhaven.com, CC0 1.0) — the twig atlases and
//     bark of its photoscanned fir and pine, which is what the conifers of the
//     catalogue carry. Poly Haven's own tree *models* are not used; only the
//     two texture sets, and only the colour and opacity maps of them.
//
// Everything lands in `~/.cache/connected-rails/foliage/`, beside ez-tree and
// the motion capture recordings. Nothing of the raw downloads is checked in —
// what ships is the atlas the build paints out of them, which is a derived work
// CC0 explicitly allows (`THIRD_PARTY_LICENSES.md` credits both anyway).
//
// Run: node tools/trees/fetch_foliage.mjs
//      node tools/trees/fetch_foliage.mjs --all   (every leaf set, for browsing)

import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';

export const CACHE = join(homedir(), '.cache', 'connected-rails', 'foliage');
const AGENT = 'connected-rails-tree-pipeline/1.0 (+https://github.com/Connected-Rails)';

/**
 * The ambientCG leaf sets the catalogue draws from. Every one of them is a
 * sheet of loose leaves; which of its leaves a species takes is decided in
 * `species.json` (`foliage.scan`).
 */
export const LEAF_SETS = [
  'LeafSet001', 'LeafSet002', 'LeafSet003', 'LeafSet004', 'LeafSet005',
  'LeafSet006', 'LeafSet007', 'LeafSet008', 'LeafSet009', 'LeafSet010',
  'LeafSet011', 'LeafSet012', 'LeafSet013', 'LeafSet014', 'LeafSet015',
  'LeafSet016', 'LeafSet017', 'LeafSet018', 'LeafSet019', 'LeafSet020',
  'LeafSet021', 'LeafSet022', 'LeafSet023', 'LeafSet024', 'LeafSet025',
  'LeafSet026', 'LeafSet027', 'LeafSet028', 'LeafSet029', 'LeafSet030',
];

/** Poly Haven texture sets: the map name to fetch, per asset. */
export const POLYHAVEN = {
  fir_tree_01: ['twig_diff', 'twig_alpha', 'bark_diff'],
  pine_tree_01: ['twig_diff', 'twig_alpha', 'bark_diff'],
};

/** Where a fetched ambientCG sheet's colour and opacity maps live. */
export function leafSetFiles(id) {
  const dir = join(CACHE, id);
  return {
    color: join(dir, `${id}_1K-PNG_Color.png`),
    opacity: join(dir, `${id}_1K-PNG_Opacity.png`),
  };
}

/** Where a fetched Poly Haven map lives. */
export function polyHavenFile(asset, map) {
  return join(CACHE, asset, `${map}.png`);
}

function download(url, target) {
  execFileSync('curl', ['-sSL', '--fail', '-A', AGENT, '-o', target, url], { stdio: 'inherit' });
}

function fetchLeafSet(id) {
  const files = leafSetFiles(id);
  if (existsSync(files.color) && existsSync(files.opacity)) return false;
  const dir = join(CACHE, id);
  mkdirSync(dir, { recursive: true });
  const zip = join(CACHE, `${id}.zip`);
  download(`https://ambientcg.com/get?file=${id}_1K-PNG.zip`, zip);
  execFileSync('unzip', ['-o', '-q', '-j', zip, '*_Color.png', '*_Opacity.png', '-d', dir]);
  rmSync(zip);
  // The normal, roughness and displacement maps come in the same archive and
  // are four megabytes each; nothing here samples them.
  for (const name of readdirSync(dir)) {
    if (!/_(Color|Opacity)\.png$/.test(name)) rmSync(join(dir, name));
  }
  return true;
}

async function fetchPolyHaven(asset, maps) {
  const dir = join(CACHE, asset);
  mkdirSync(dir, { recursive: true });
  const wanted = maps.filter((map) => !existsSync(polyHavenFile(asset, map)));
  if (wanted.length === 0) return false;
  const response = await fetch(`https://api.polyhaven.com/files/${asset}`, {
    headers: { 'user-agent': AGENT },
  });
  if (!response.ok) throw new Error(`poly haven ${asset}: ${response.status}`);
  const files = await response.json();
  for (const map of wanted) {
    const entry = files[map]?.['1k']?.png ?? files[map]?.['2k']?.png;
    if (!entry?.url) throw new Error(`poly haven ${asset}: no PNG for ${map}`);
    download(entry.url, polyHavenFile(asset, map));
  }
  return true;
}

export async function ensureFoliage({ all = false, quiet = false } = {}) {
  mkdirSync(CACHE, { recursive: true });
  let fetched = 0;
  for (const id of all ? LEAF_SETS : usedLeafSets()) {
    if (fetchLeafSet(id)) {
      fetched += 1;
      if (!quiet) console.log(`ambientCG ${id}`);
    }
  }
  for (const [asset, maps] of Object.entries(POLYHAVEN)) {
    if (await fetchPolyHaven(asset, maps)) {
      fetched += 1;
      if (!quiet) console.log(`Poly Haven ${asset}`);
    }
  }
  if (!quiet) {
    console.log(fetched ? `${fetched} sheets fetched into ${CACHE}` : `up to date in ${CACHE}`);
  }
  writeFileSync(
    join(CACHE, 'SOURCES.txt'),
    'ambientCG (https://ambientcg.com) and Poly Haven (https://polyhaven.com),\n' +
      'both CC0 1.0 Universal. Fetched by tools/trees/fetch_foliage.mjs; not\n' +
      'redistributed — see THIRD_PARTY_LICENSES.md.\n',
    'utf8',
  );
}

/** The sets `species.json` actually names, so a build fetches only those. */
function usedLeafSets() {
  let catalogue;
  try {
    catalogue = JSON.parse(readFileSync(new URL('./species.json', import.meta.url), 'utf8'));
  } catch {
    return LEAF_SETS;
  }
  const used = new Set();
  for (const entry of catalogue.species ?? []) {
    for (const scan of [entry.foliage?.scan, entry.foliage?.autumnScan]) {
      if (scan?.set) used.add(scan.set);
    }
  }
  return [...used].sort();
}

if (import.meta.url === `file://${process.argv[1]}`) {
  await ensureFoliage({ all: process.argv.includes('--all') });
}
