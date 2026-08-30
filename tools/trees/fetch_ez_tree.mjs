// Fetches and builds ez-tree, the procedural tree generator the models come
// out of (https://github.com/dgreenheck/ez-tree, MIT).
//
// The library is not vendored and not an npm dependency of this repository:
// the version published to npm predates the LOD API the pipeline needs, and a
// copy in `tools/` would be a fork nobody maintains. Instead the pinned commit
// is cloned into `~/.cache/connected-rails/ez-tree` — the same place the motion
// capture recordings live (tools/characters/fetch_mocap.py) — and built there
// with its own dependencies. Nothing of it is checked in.
//
// Run: node tools/trees/fetch_ez_tree.mjs

import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';

/** The commit the models were generated from. */
export const COMMIT = 'dcf309bd86bd521083d9c70f01f2de45fdc7c457';
export const REPOSITORY = 'https://github.com/dgreenheck/ez-tree.git';
export const CACHE = join(homedir(), '.cache', 'connected-rails', 'ez-tree');
/** What `build_trees.mjs` imports once this has run. */
export const LIBRARY = join(CACHE, 'build', 'ez-tree.es.js');

function run(command, args, cwd) {
  execFileSync(command, args, { cwd, stdio: 'inherit' });
}

export function ensureEzTree({ quiet = false } = {}) {
  if (existsSync(LIBRARY) && head() === COMMIT) {
    if (!quiet) console.log(`ez-tree ${COMMIT.slice(0, 8)} ready in ${CACHE}`);
    return LIBRARY;
  }
  mkdirSync(join(homedir(), '.cache', 'connected-rails'), { recursive: true });
  if (!existsSync(join(CACHE, '.git'))) {
    run('git', ['clone', REPOSITORY, CACHE]);
  }
  run('git', ['fetch', 'origin', COMMIT], CACHE);
  run('git', ['checkout', '--detach', COMMIT], CACHE);
  // `npm ci` needs the lock file the repository ships; `--ignore-scripts` is
  // deliberately *not* passed — esbuild, which vite builds with, installs its
  // binary in a postinstall step.
  run('npm', ['install', '--no-audit', '--no-fund'], CACHE);
  run('npm', ['run', 'build:lib'], CACHE);
  if (!existsSync(LIBRARY)) throw new Error(`ez-tree build produced no ${LIBRARY}`);
  console.log(`ez-tree ${COMMIT.slice(0, 8)} built in ${CACHE}`);
  return LIBRARY;
}

function head() {
  try {
    return execFileSync('git', ['rev-parse', 'HEAD'], { cwd: CACHE, encoding: 'utf8' }).trim();
  } catch {
    return null;
  }
}

if (import.meta.url === `file://${process.argv[1]}`) {
  ensureEzTree();
}
