// Grows `mods/trees` out of `species.json`.
//
//   species.json ──ez-tree──▶ skeleton ──▶ four levels of detail ──▶ .gltf/.bin
//               └──bark.mjs/foliage.mjs──▶ one atlas per species and season
//                                       └─impostor.mjs──▶ the coarsest level
//
// Per species: three differently seeded individuals, each with four levels
// (`crown_LOD0` … `crown_LOD3`), and up to three seasons — summer, autumn for
// whatever turns, winter bare or snowed. All of it lands under `mods/trees`,
// which the simulator and both editors read like any other mod.
//
// Run: node tools/trees/build_trees.mjs
//      node tools/trees/build_trees.mjs --only rotbuche,fichte
//      node tools/trees/build_trees.mjs --report   (triangle and file budget)
//      node tools/trees/build_trees.mjs --preview  (a sheet of every species
//                                                   into /tmp, to look at)

import { mkdirSync, readFileSync, rmSync, writeFileSync, existsSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { ensureEzTree, COMMIT } from './fetch_ez_tree.mjs';
import { ensureFoliage, leafSetFiles, polyHavenFile } from './fetch_foliage.mjs';
import { loadScan } from './lib/leaves.mjs';
import { encodePng } from './lib/png.mjs';
import { Surface, rgb } from './lib/raster.mjs';
import { barkTexture } from './lib/bark.mjs';
import { foliageCard } from './lib/foliage.mjs';
import { bakeImpostor } from './lib/impostor.mjs';
import { BufferBuilder, writeGltf, writeBuffer } from './lib/gltf.mjs';
import {
  bounds,
  crossQuads,
  meshBranches,
  meshLeaves,
  mergeGeometries,
  remapUv,
  scaleGeometry,
  triangleCount,
  verifyAgainstEzTree,
} from './lib/mesh.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, '..', '..');
const MOD = join(ROOT, 'mods', 'trees');
const ASSETS = join(MOD, 'assets');
const OBJECTS = join(MOD, 'objects');

/** The variants every species is built in — three individuals, one seed each. */
const VARIANTS = ['a', 'b', 'c'];

/**
 * The atlas: bark, two foliage cards and the impostor in one 1024 × 512 sheet,
 * so a tree is one material and one draw call per level instead of two.
 * Rectangles are inset by a texel, which keeps bilinear filtering from
 * reaching into the neighbour.
 */
const ATLAS = { width: 1024, height: 512 };
const CELLS = {
  bark: { x: 0, y: 0, w: 512, h: 512 },
  foliageA: { x: 512, y: 0, w: 256, h: 256 },
  foliageB: { x: 768, y: 0, w: 256, h: 256 },
  impostor: { x: 512, y: 256, w: 256, h: 256 },
};

/**
 * The four levels, coarsening outwards. `minRadius` is the share of the trunk
 * radius a branch has to reach to survive the level: at LOD1 the twigs go and
 * the canopy stays where they were, at LOD2 only the trunk and the limbs are
 * left and the crown is the cards alone. `leafScale` grows what survives, so
 * a thinned canopy keeps covering its own volume — but only so far: a card
 * blown up reads as a streak, and a wood of them is a heap of cardboard. Every
 * step towards thinning less and growing less costs triangles and buys back a
 * canopy that still looks like leaves rather than like smears.
 */
const LEVELS = [
  { name: 'crown_LOD0', sectionStride: 1, segmentFactor: 1, minRadius: 0, leafStride: 1, leafScale: 1 },
  {
    name: 'crown_LOD1',
    sectionStride: 2,
    segmentFactor: 0.7,
    minRadius: 0.08,
    leafStride: 2,
    leafScale: 1.25,
    billboard: 'single',
  },
  {
    name: 'crown_LOD2',
    sectionStride: 5,
    segmentFactor: 0.45,
    minRadius: 0.2,
    leafStride: 3,
    leafScale: 1.4,
    billboard: 'single',
  },
];

const COPYRIGHT =
  'Connected Rails contributors, EUPL-1.2. Generated with ez-tree ' +
  '(MIT, Daniel Greenheck).';

async function main() {
  const argv = process.argv.slice(2);
  const only = pick(argv, '--only');
  const reportOnly = argv.includes('--report');
  const preview = argv.includes('--preview');
  const library = ensureEzTree({ quiet: true });
  await ensureFoliage({ quiet: true });
  const { Tree } = await import(`file://${library}`);

  const catalogue = JSON.parse(readFileSync(join(HERE, 'species.json'), 'utf8'));
  const wanted = only ? new Set(only.split(',').map((s) => s.trim())) : null;
  const species = catalogue.species.filter((s) => !wanted || wanted.has(s.id));
  if (species.length === 0) throw new Error('no species selected');

  if (!reportOnly) {
    mkdirSync(ASSETS, { recursive: true });
    mkdirSync(OBJECTS, { recursive: true });
    if (!wanted) clean();
  }

  const report = [];
  for (const entry of species) {
    report.push(...buildSpecies(Tree, catalogue.bases, entry, { reportOnly: reportOnly || preview, preview }));
  }

  if (!reportOnly && !wanted) writeModManifest();
  summarise(report);
}

/** Removes what a previous run wrote, so a renamed species leaves no orphan. */
function clean() {
  for (const directory of [ASSETS, OBJECTS]) {
    if (!existsSync(directory)) continue;
    for (const name of readdirSync(directory)) {
      if (/\.(gltf|bin|png|ron)$/.test(name)) rmSync(join(directory, name));
    }
  }
}

function pick(argv, flag) {
  const i = argv.indexOf(flag);
  return i >= 0 ? argv[i + 1] : null;
}

// ---------------------------------------------------------------------------

function buildSpecies(Tree, bases, entry, { reportOnly, preview = false }) {
  const base = bases[entry.base ?? (entry.kind === 'conifer' ? 'conifer' : 'deciduous')];
  const options = deepMerge(structuredClone(base), entry.tree ?? {});
  const rows = [];

  // The seasons this species has a model for. Everything gets a winter one —
  // a bare crown for what drops its leaves, a snowed one for what does not.
  const seasons = ['summer'];
  if (entry.deciduous) seasons.push('autumn');
  seasons.push('winter');

  // The atlas is per species and season; three individuals of a beech have
  // the same bark and the same leaves.
  const atlases = {};
  for (const season of seasons) atlases[season] = buildAtlas(entry, season);

  const skeletons = VARIANTS.map((variant, i) => {
    const tree = new Tree();
    tree.options.copy(options);
    tree.options.seed = seedFor(entry.id, variant);
    tree.generate();
    if (i === 0) {
      const check = verifyAgainstEzTree(tree);
      if (check.error || check.worst > 1e-4) {
        throw new Error(`${entry.id}: meshing disagrees with ez-tree (${check.error ?? check.worst})`);
      }
    }
    return tree.skeleton;
  });

  // Metres per ez-tree unit, so the variants span the species' height range.
  const scales = skeletons.map((skeleton, i) => {
    const raw = meshBranches(skeleton, {});
    const leafy = meshLeaves(skeleton, {});
    const top = Math.max(bounds(raw).max[1], leafy.positions.length ? bounds(leafy).max[1] : 0);
    const target =
      entry.height[0] + ((entry.height[1] - entry.height[0]) * i) / Math.max(1, VARIANTS.length - 1);
    return { factor: target / top, height: target };
  });

  for (const season of seasons) {
    const atlas = atlases[season];
    const bare = season === 'winter' && entry.deciduous;

    for (let v = 0; v < VARIANTS.length; v++) {
      const variant = VARIANTS[v];
      const { factor, height } = scales[v];
      const levels = [];
      for (const level of LEVELS) {
        const branches = remapUv(
          scaleGeometry(meshBranches(skeletons[v], level), factor),
          atlas.uv.bark,
        );
        const parts = [branches];
        if (!bare) {
          parts.push(
            scaleGeometry(
              meshLeaves(skeletons[v], {
                ...level,
                rects: [atlas.uv.foliageA, atlas.uv.foliageB],
              }),
              factor,
            ),
          );
        }
        levels.push({ name: level.name, geometry: mergeGeometries(parts) });
      }

      // The coarsest level is the picture of the finest one, on crossed quads.
      // Level 1 would be cheaper to rasterise, but its cards are enlarged to
      // cover the canopy it thinned, and a silhouette taken off those is a
      // tree that grows a metre wider the moment it switches to its impostor.
      // The picture is taken from the first individual only — it goes into the
      // very atlas the geometry samples, so a second bake would photograph the
      // first — and the other two carry the same sheet at their own size.
      const box = bounds(levels[0].geometry);
      const size = {
        width: Math.max(Math.abs(box.min[0]), box.max[0], Math.abs(box.min[2]), box.max[2]) * 2,
        height: box.max[1] - box.min[1],
        base: box.min[1],
      };
      if (v === 0) {
        atlas.blitImpostor(bakeImpostor(levels[0].geometry, atlas.surface, CELLS.impostor.w).surface);
        const expected = height * (entry.crown ?? 0.6);
        if (size.width < expected * 0.5 || size.width > expected * 1.9) {
          console.warn(
            `${entry.id}: crown ${size.width.toFixed(1)} m, catalogue says ${expected.toFixed(1)} m`,
          );
        }
      }
      levels.push({
        name: 'crown_LOD3',
        geometry: crossQuads(size.width, size.height, size.base, atlas.uv.impostor),
      });
      // After the paste, so the sheet shows the impostor the last level uses.
      if (v === 0 && preview && season === 'summer') writePreview(entry, levels, atlas);

      rows.push({
        species: entry.id,
        variant,
        season,
        height,
        triangles: levels.map((l) => triangleCount(l.geometry)),
      });

      if (reportOnly) continue;

      const buffer = new BufferBuilder();
      const packed = levels.map((l) => ({
        name: l.name,
        packed: buffer.add(l.geometry),
        material: 0,
      }));
      const binary = buffer.toBuffer();
      const binName = `${entry.id}_${variant}${bare ? '_winter' : ''}.bin`;
      writeBuffer(ASSETS, binName, binary);
      writeGltf({
        path: join(ASSETS, `${entry.id}_${variant}${modelSuffix(season)}.gltf`),
        name: `${entry.id}_${variant}`,
        buffer: binName,
        bufferLength: binary.length,
        levels: packed,
        materials: [
          {
            name: `${entry.id}_${season}`,
            texture: 0,
            alphaCutoff: options.leaves.alphaTest ?? 0.5,
            roughnessFactor: 0.9,
          },
        ],
        textures: [atlas.file],
        copyright: COPYRIGHT,
      });
    }
    if (!reportOnly) {
      writeFileSync(join(ASSETS, atlas.file), encodePng(atlas.surface.toRgba8(), ATLAS.width, ATLAS.height));
    }
  }

  if (!reportOnly) {
    for (let v = 0; v < VARIANTS.length; v++) {
      writeObject(entry, VARIANTS[v], scales[v].height, seasons);
    }
  }
  return rows;
}

/**
 * What a season adds to a model's file name. The *buffer* is not named this
 * way: summer and autumn are the same wood under a different sheet of leaves,
 * and an evergreen's winter model is the same wood again — they all point at
 * `<species>_<variant>.bin`. Only a bare deciduous crown is different geometry
 * and gets a `_winter.bin` of its own.
 */
function modelSuffix(season) {
  if (season === 'autumn') return '_herbst';
  if (season === 'winter') return '_winter';
  return '';
}

/**
 * A sheet to look at while tuning the catalogue: the four levels of one
 * individual side by side, rasterised the same way the impostor is, plus the
 * atlas they sample. Written to /tmp, never to the repository.
 */
function writePreview(entry, levels, atlas) {
  const cell = 384;
  const sheet = new Surface(cell * levels.length, cell + 256);
  sheet.clear(0.62, 0.70, 0.82, 1);
  levels.forEach((level, i) => {
    const shot = bakeImpostor(level.geometry, atlas.surface, cell);
    for (let y = 0; y < cell; y++) {
      for (let x = 0; x < cell; x++) {
        const s = (y * cell + x) * 4;
        if (shot.surface.data[s + 3] < 0.5) continue;
        const d = (y * sheet.width + i * cell + x) * 4;
        sheet.data[d] = shot.surface.data[s];
        sheet.data[d + 1] = shot.surface.data[s + 1];
        sheet.data[d + 2] = shot.surface.data[s + 2];
      }
    }
  });
  for (let y = 0; y < 256; y++) {
    for (let x = 0; x < Math.min(sheet.width, atlas.surface.width); x++) {
      const s = ((y * 2) * atlas.surface.width + x) * 4;
      const d = ((cell + y) * sheet.width + x) * 4;
      const a = atlas.surface.data[s + 3];
      for (let c = 0; c < 3; c++) {
        sheet.data[d + c] = atlas.surface.data[s + c] * a + sheet.data[d + c] * (1 - a);
      }
    }
  }
  const out = join('/tmp', 'tree-preview');
  mkdirSync(out, { recursive: true });
  writeFileSync(join(out, `${entry.id}.png`), encodePng(sheet.toRgba8(), sheet.width, sheet.height));
}

// ---------------------------------------------------------------------------

/** Paints one species' sheet and hands back the UV rectangles into it. */
function buildAtlas(entry, season) {
  const surface = new Surface(ATLAS.width, ATLAS.height);
  // Whatever the rectangles do not cover is filled with the foliage colour,
  // so the coarsest mip levels — where the cells blur into each other — stay
  // the colour of a tree.
  const fill =
    season === 'winter' && entry.deciduous
      ? rgb(entry.bark.base ?? '#6a5f4e')
      : rgb(entry.foliage.base ?? '#3d6b2a');
  surface.clear(fill[0], fill[1], fill[2], 1);

  const bark = barkTexture(CELLS.bark.w, seasonalBark(entry.bark, season), `${entry.id}/bark`);
  blit(surface, bark, CELLS.bark.x, CELLS.bark.y);

  const foliageSeason = season === 'autumn' ? 'autumn' : season === 'winter' ? 'winter' : 'summer';
  if (!(season === 'winter' && entry.deciduous)) {
    // An autumn sheet where the catalogue names one; otherwise the summer
    // leaves, which winter only dulls. Tinting a green scan orange makes mud,
    // so a species that turns is given a turned sheet of its own.
    const scan = loadScan(
      (season === 'autumn' && entry.foliage.autumnScan) || entry.foliage.scan,
      { leafSetFiles, polyHavenFile },
    );
    for (const [cell, salt] of [
      [CELLS.foliageA, 'a'],
      [CELLS.foliageB, 'b'],
    ]) {
      const card = foliageCard(cell.w, entry.foliage, foliageSeason, `${entry.id}/leaf/${salt}`, scan);
      blit(surface, card, cell.x, cell.y);
    }
  }

  return {
    surface,
    file: `${entry.id}_${season}.png`,
    uv: {
      bark: uvOf(CELLS.bark),
      foliageA: uvOf(CELLS.foliageA),
      foliageB: uvOf(CELLS.foliageB),
      impostor: uvOf(CELLS.impostor),
    },
    blitImpostor(image) {
      blit(surface, image, CELLS.impostor.x, CELLS.impostor.y);
    },
  };
}

/** Winter bark is paler — frost, and the light off the snow underneath. */
function seasonalBark(bark, season) {
  if (season !== 'winter') return bark;
  const pale = (hex) => {
    const c = rgb(hex);
    return `#${c
      .map((v) => Math.round(Math.min(1, v * 0.82 + 0.16) * 255).toString(16).padStart(2, '0'))
      .join('')}`;
  };
  return { ...bark, base: pale(bark.base), dark: pale(bark.dark), light: pale(bark.light), moss: (bark.moss ?? 0) * 0.4 };
}

/** The UV rectangle of a cell, inset by a texel against filter bleed. */
function uvOf(cell) {
  const inset = 1;
  return {
    u: (cell.x + inset) / ATLAS.width,
    v: (cell.y + inset) / ATLAS.height,
    w: (cell.w - 2 * inset) / ATLAS.width,
    h: (cell.h - 2 * inset) / ATLAS.height,
  };
}

function blit(target, source, x, y) {
  for (let sy = 0; sy < source.height; sy++) {
    for (let sx = 0; sx < source.width; sx++) {
      const i = (sy * source.width + sx) * 4;
      const j = ((y + sy) * target.width + (x + sx)) * 4;
      target.data[j] = source.data[i];
      target.data[j + 1] = source.data[i + 1];
      target.data[j + 2] = source.data[i + 2];
      target.data[j + 3] = source.data[i + 3];
    }
  }
}

// ---------------------------------------------------------------------------

/** One `objects/*.ron` per individual — what the editors place. */
function writeObject(entry, variant, height, seasons) {
  const id = `${entry.id}_${variant}`;
  const label = `${entry.name} ${variant.toUpperCase()}`;
  const model = (suffix) => `"trees/assets/${id}${suffix}.gltf"`;
  const lines = [
    `// ${entry.name} (${entry.latin}), ${height.toFixed(0)} m — generated by`,
    '// tools/trees/build_trees.mjs; edit species.json, not this file.',
    '(',
    `    name: "${label}",`,
    `    model: ${model('')},`,
  ];
  if (seasons.includes('autumn')) lines.push(`    autumn_model: Some(${model('_herbst')}),`);
  if (seasons.includes('winter') && entry.deciduous) {
    lines.push(`    winter_model: Some(${model('_winter')}),`);
  }
  lines.push(`    lod_distances: [${lodDistances(entry, height).map((d) => d.toFixed(0)).join(', ')}],`);
  lines.push(`    tags: [${entry.tags.map((t) => `"${t}"`).join(', ')}],`);
  lines.push(')');
  writeFileSync(join(OBJECTS, `${id}.ron`), `${lines.join('\n')}\n`, 'utf8');
}

/**
 * Where the levels hand over for this species [m]. A level is worth its
 * triangles as long as the tree covers enough pixels, and that is a matter of
 * *how big it is*: a thirty metre spruce still reads as a tree where a two
 * metre sloe is a smudge. So the bands scale with the height, clamped so no
 * species falls outside what the streaming radius makes sensible.
 *
 * **The bands are in units of the plant's own height, and they are generous.**
 * What decides whether a level is enough is how many *pixels* the plant covers,
 * and that is `height / distance × (screen height / field of view)` — on a 1440
 * line screen at sixty degrees, about `height / distance × 1375`. Working back
 * from the size each level still holds up at:
 *
 * | hand-over | tree covers | at          |
 * | --------- | ----------- | ----------- |
 * | LOD0→LOD1 | ~340 px     | 4 × height  |
 * | LOD1→LOD2 | ~100 px     | 14 × height |
 * | LOD2→LOD3 | ~35 px      | 40 × height |
 *
 * For a thirty metre beech that is 120 m, 420 m and 1.2 km. The numbers before
 * this were less than half of it and it showed: a tree two hundred metres off
 * was already down to a fifth of its cards and a wood at that distance read as
 * streaks. Beware of judging this in a small window — the same level looks
 * finer at 1200 lines than at 1440, which is how the earlier numbers passed.
 *
 * `crown_LOD3`, two quads crossed at a right angle, has a seam where whichever
 * blade is edge-on cuts through the other, so it wants to stay small on screen:
 * thirty-five pixels is where that strip is under a pixel wide.
 */
function lodDistances(entry, height) {
  const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
  return [
    clamp(height * 4, 30, 150),
    clamp(height * 14, 90, 480),
    clamp(height * 40, 300, 1400),
    entry.cull ?? 2000,
  ];
}

function writeModManifest() {
  writeFileSync(
    join(MOD, 'mod.ron'),
    `// Generated by tools/trees/build_trees.mjs — see tools/trees/README.md.
(
    id: "trees",
    name: "Bäume Mitteleuropas",
    version: "1.0.0",
    author: "Connected Rails",
    description: "Nadel- und Laubbäume und Sträucher Mitteleuropas, mit ez-tree erzeugt: drei Individuen je Art, vier Detailstufen, Sommer-, Herbst- und Winterausführung.",
    depends: [],
    enabled: true,
)
`,
    'utf8',
  );
}

// ---------------------------------------------------------------------------

function deepMerge(target, source) {
  for (const key of Object.keys(source)) {
    const value = source[key];
    if (value && typeof value === 'object' && !Array.isArray(value)) {
      target[key] = deepMerge(target[key] ?? {}, value);
    } else {
      target[key] = value;
    }
  }
  return target;
}

function seedFor(id, variant) {
  let h = 0x811c9dc5;
  for (const ch of `${id}/${variant}`) {
    h ^= ch.charCodeAt(0);
    h = Math.imul(h, 0x01000193);
  }
  return (h >>> 0) % 100000;
}

function summarise(rows) {
  const summer = rows.filter((r) => r.season === 'summer');
  const columns = ['LOD0', 'LOD1', 'LOD2', 'LOD3'];
  console.log(`\nez-tree ${COMMIT.slice(0, 8)} — ${summer.length} models in summer dress\n`);
  console.log(`${'species'.padEnd(18)}${'m'.padStart(5)}  ${columns.map((c) => c.padStart(7)).join('')}`);
  const totals = [0, 0, 0, 0];
  for (const row of summer) {
    console.log(
      `${`${row.species}_${row.variant}`.padEnd(18)}${row.height.toFixed(0).padStart(5)}  ` +
        row.triangles.map((t) => String(t).padStart(7)).join(''),
    );
    row.triangles.forEach((t, i) => (totals[i] += t));
  }
  const mean = totals.map((t) => Math.round(t / Math.max(1, summer.length)));
  console.log(`${'mean'.padEnd(18)}${''.padStart(5)}  ${mean.map((t) => String(t).padStart(7)).join('')}`);
  console.log(`\n${rows.length} models written in total (all seasons).`);
}

await main();
