// Builds `mods/cars` — the road vehicles of a German module, and what the AI
// import places when it finds one in the aerial imagery.
//
//   cache/cars/manual/  ──build_cars.mjs──▶ mods/cars/assets/<id>.bin
//   (FBX + atlas)       │                                   <id>.gltf, <id>.jpg
//   cars.json ──────────┴──────────────────▶ mods/cars/objects/*.ron
//                                            mods/cars/mod.ron
//
//   node tools/cars/import_cars.mjs           # once: the zips out of ~/Downloads
//   node tools/cars/build_cars.mjs
//   node tools/cars/build_cars.mjs --report   # the budget, writes nothing
//   node tools/cars/build_cars.mjs --preview  # picture sheets into /tmp
//   node tools/cars/build_cars.mjs --only kompaktwagen
//
// Node 20 or newer, plus ImageMagick for the atlases (see `import_cars.mjs`).
// The FBX reader, the PNG reader, the cleaner and the decimator are in `lib/`;
// the glTF writer is `tools/pylons/lib`.
//
// **What arrives is a photograph of a car, and what a car park needs is a
// prop.** The models are generated: one nameless mesh, one baked atlas, eleven
// thousand triangles, and everything the generator thought it saw. Four things
// are done to every one of them, and each is a rule about geometry because
// there are no object names to go by:
//
//  1. **The plinth comes off** — the generator stands its subject on a slab,
//     and a car standing on one floats a few centimetres above the tarmac and
//     drags a grey rectangle around the car park with it.
//  2. **The interior goes** — seats, a floor, the inside of the shell. Found by
//     looking at the model from a hundred directions and keeping what was seen
//     from any of them (`lib/visible.mjs`), which is about a sixth of the mesh
//     and not one triangle of it is ever on screen.
//  3. **The windows become glass** — dark, smooth and untextured, instead of a
//     photograph of a window with somebody else's street reflected in it.
//     Found where the texture is dark, the surface steep and the height above
//     the waistline; shape alone cannot tell a window from the roof shoulder
//     above it, but the texture can.
//  4. **Four levels of detail**, since a car park holds two hundred of these
//     and the mesh that is right at ten metres is nonsense at three hundred.
//
// Axes are the game's (`MODS.md`, *Track objects*): +Y up, the model's front
// along −Z.

import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { readFbxScene } from './lib/fbx_scene.mjs';
import { decodePng, sample } from './lib/png.mjs';
import { encodePng } from '../trees/lib/png.mjs';
import {
  bodyBounds,
  bounds,
  drop,
  facesOf,
  facet,
  fixWinding,
  glasshouse,
  groundPlate,
  hidden,
  specks,
} from './lib/clean.mjs';
import { simplify, weld } from './lib/simplify.mjs';
import { renderSheet, renderOverview, renderHandovers } from './lib/preview.mjs';
import { BufferBuilder, writeGltf, writeBuffer } from '../pylons/lib/gltf.mjs';
import { SHIPPED, fbxIn, readableTexture, sourceDir } from './import_cars.mjs';
import { coverage, dilate } from './lib/atlas.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, '..', '..');
const modDir = join(root, 'mods', 'cars');
const assetDir = join(modDir, 'assets');
const objectDir = join(modDir, 'objects');

/**
 * How far a normal may turn before a decimated level treats the edge as a
 * crease.
 *
 * Sixty degrees, which is generous: a car body is one long smooth surface with
 * a handful of real creases in it, and after a level has been decimated its
 * neighbouring faces meet at angles the original never had. A threshold tight
 * enough to be right on the original is what makes the first decimated level
 * look like it is made of paper.
 */
const CREASE = Math.cos((60 * Math.PI) / 180);

/** Where the interior hunt looks from, and how finely. */
const LOOK = { directions: 128, resolution: 640, lowest: -28 };

const COPYRIGHT =
  'Vehicle meshes generated with a 3D tool; cleaned, fitted and cut into levels ' +
  'of detail by tools/cars/build_cars.mjs (Connected Rails, EUPL-1.2)';

// ---------------------------------------------------------------------------
// Reading and cleaning one vehicle
// ---------------------------------------------------------------------------

/** The FBX of a source directory, and the atlas beside it. */
function sourceOf(type) {
  const directory = join(sourceDir, `src-${type.source}`);
  const fbx = fbxIn(directory)[0];
  if (!fbx) {
    throw new Error(
      `${type.id}: no FBX in ${directory} — run: node tools/cars/import_cars.mjs`,
    );
  }
  const readable = readableTexture(type.source);
  return { fbx, texture: existsSync(readable) ? readable : null };
}

/**
 * One vehicle, read and cleaned.
 *
 * The order matters. The plinth goes first, because it is *visible* and the
 * hunt for what nobody can see would keep it; and the glass is classified
 * last, on what is left, because a face index means nothing after two rounds
 * of deletion.
 */
function readVehicle(type, { quiet = false } = {}) {
  const { fbx, texture } = sourceOf(type);
  const scene = readFbxScene(fbx);
  const source = scene.meshes[0];
  if (!source) throw new Error(`${type.id}: ${fbx} holds no mesh`);

  // The file's own normals are not read. They disagree with the surface they
  // sit on for about a tenth of the triangles of every one of these models,
  // and a normal that points into the panel it belongs to is a black shard on
  // the paint under any light. Everything below computes them from the
  // geometry instead, which is the one description of the shape that cannot be
  // wrong about itself.
  let mesh = {
    positions: source.positions.slice(),
    uvs: source.uvs.slice(),
    faces: facesOf(source.indices),
  };
  const before = mesh.faces.length;

  const plate = groundPlate(mesh.positions, mesh.faces);
  const plateFaces = plate.flags.reduce((sum, v) => sum + v, 0);
  mesh = drop(mesh, plate.flags);

  const hide = hidden(mesh.positions, mesh.faces, LOOK);
  const hiddenFaces = hide.flags.reduce((sum, v) => sum + v, 0);
  // Wound right before anything is thrown away: the pass that found what is
  // visible also knows which way each of those faces was looked at.
  const flipped = fixWinding(mesh.positions, mesh.faces, hide.towards);
  mesh = drop(mesh, hide.flags);

  // The litter goes last of the three, once the plinth and the interior are
  // out of the way and the surface area means what it says.
  const litter = specks(mesh.positions, mesh.faces);
  mesh = drop(mesh, litter.flags);

  // The atlas, read once, so a face can be asked what colour it is — and asked
  // about the whole of itself, not only its middle. A triangle laid across the
  // edge of a window is dark at one corner and white at the other two, and its
  // centre alone will happily claim it is glass. Four readings say so.
  const image = texture ? decodePng(texture) : null;
  const grey = (c) => 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
  const readingsOf = mesh.faces.map((face) => {
    if (!image) return [grey([0.72, 0.73, 0.75])];
    const middle = [0, 0];
    for (const i of face) {
      middle[0] += mesh.uvs[i * 2] / 3;
      middle[1] += mesh.uvs[i * 2 + 1] / 3;
    }
    const out = [grey(sample(image, middle[0], middle[1]))];
    for (const i of face) {
      // Pulled a little way in from the corner: a corner sits on the edge of
      // its island, where a nearest-neighbour read can fall off the side.
      const u = middle[0] + (mesh.uvs[i * 2] - middle[0]) * 0.85;
      const v = middle[1] + (mesh.uvs[i * 2 + 1] - middle[1]) * 0.85;
      out.push(grey(sample(image, u, v)));
    }
    return out;
  });
  const colourAt = (face) => {
    if (!image) return [0.72, 0.73, 0.75];
    let u = 0;
    let v = 0;
    for (const i of face) {
      u += mesh.uvs[i * 2] / 3;
      v += mesh.uvs[i * 2 + 1] / 3;
    }
    return sample(image, u, v);
  };
  const luminance = (f) => readingsOf[f].reduce((a, b) => a + b, 0) / readingsOf[f].length;
  const spread = (f) => Math.max(...readingsOf[f]) - Math.min(...readingsOf[f]);

  let glass = glasshouse(mesh.positions, mesh.faces, {
    waist: type.waist ?? 0.5,
    // Without an atlas there is nothing to ask but the shape, and shape alone
    // cannot tell a window from the roof shoulder above it. So the rule is
    // drawn tighter: only what stands nearly upright counts, which loses the
    // top of a windscreen and keeps the roof — the better way round of the two
    // mistakes it can make.
    steep: image ? (type.steep ?? 0.8) : (type.steep ?? 0.45),
    luminance: image ? luminance : null,
    spread: image ? spread : null,
  });
  // The classification has to earn its place, and what it has to be measured
  // against is **the car's own paint** — not the rest of the glasshouse.
  //
  // That distinction is the whole check. Asked whether what it picked out is
  // darker than everything else above the waistline, the answer on a black
  // saloon is yes: it picked out the black boot lid, and the teal photograph
  // of the windows next to it is lighter. The substitution then lays glass
  // over the bodywork in a jagged band and leaves the windows as they were,
  // which is precisely backwards. Asked instead whether it is darker than the
  // doors — which are paint on every car there has ever been — the same saloon
  // answers no, and is left alone.
  //
  // Left alone is a perfectly good outcome. These windows are opaque baked
  // photographs already; on a car whose paint is as dark as its glass there is
  // nothing to win and a band of glossy black to lose.
  if (image && type.glass !== false) {
    const box = bounds(mesh.positions);
    const line = box.min[1] + box.size[1] * (type.waist ?? 0.5);
    // Between a fifth of the way up and the waist: door skin and wings, above
    // the tyres and the shadow under the sill, below any window.
    const low = box.min[1] + box.size[1] * 0.2;
    let paintArea = 0;
    let paintSum = 0;
    let glassArea = 0;
    let glassSum = 0;
    mesh.faces.forEach((face, f) => {
      const { area, centre, normal } = facet(mesh.positions, face);
      if (glass[f]) {
        glassArea += area;
        glassSum += luminance(f) * area;
      } else if (centre[1] >= low && centre[1] <= line && Math.abs(normal[1]) <= 0.5) {
        paintArea += area;
        paintSum += luminance(f) * area;
      }
    });
    const darkerBy =
      paintArea && glassArea ? paintSum / paintArea - glassSum / glassArea : 0;
    // Four hundredths, which is the middle of a gap rather than a tuned
    // number: of these seven the two that come out wrong measure 0.025 and
    // 0.023 — each ends up with one window black and the one beside it as it
    // was baked — and the nearest one that comes out right measures 0.048.
    if (darkerBy < 0.04) {
      glass = new Uint8Array(mesh.faces.length);
      if (!quiet) {
        console.log(
          `  ${''.padEnd(15)} glass left as the atlas baked it — the windows are ` +
            `only ${darkerBy.toFixed(3)} darker than the paint, and the guess is ` +
            `not worth a black band down the side`,
        );
      }
    }
  }
  if (type.glass === false) glass = new Uint8Array(mesh.faces.length);
  mesh.groups = [...glass].map((g) => (g ? 1 : 0));

  fit(mesh, type);
  // Measured on the fitted mesh: before the fit it is still in a unit box, and
  // one of these eight is still lying across its own length.
  const shape = proportions(mesh, type);
  if (!quiet) {
    const glassFaces = mesh.groups.reduce((sum, g) => sum + g, 0);
    console.log(
      `  ${type.id.padEnd(15)} ${String(before).padStart(6)} tris → ` +
        `${String(mesh.faces.length).padStart(6)}   ` +
        `plinth ${String(plateFaces).padStart(4)} (${(plate.share * 100).toFixed(0)}% of area), ` +
        `inside ${String(hiddenFaces).padStart(5)}, specks ${String(litter.dropped).padStart(3)}, ` +
        `wound ${String(flipped).padStart(3)}, ` +
        `glass ${String(glassFaces).padStart(4)}` +
        `${image ? '' : '   [no atlas: glass by shape alone]'}`,
    );
    // Width and height are what the catalogue *expects*, not what it sets: the
    // scale comes from the length alone. Where the model disagrees by more
    // than a twentieth, the entry and the model are describing different
    // vehicles, and somebody has to choose which.
    const [narrow, wide] = shape.span;
    const wrongWidth =
      shape.built[0] < narrow * (1 - (type.tolerance ?? 0.05)) ? shape.built[0] / narrow - 1 :
      shape.built[0] > wide * (1 + (type.tolerance ?? 0.05)) ? shape.built[0] / wide - 1 : 0;
    const wrongHeight = shape.built[1] / shape.height - 1;
    // A vehicle may state a wider tolerance, and one of these does. That is an
    // exception with a reason written next to it in the catalogue, not a
    // loosened rule: everything else is still held to a twentieth, and a build
    // that warned every time would only teach whoever runs it to look away.
    const allowed = type.tolerance ?? 0.05;
    if (Math.abs(wrongWidth) > 0 || Math.abs(wrongHeight) > allowed) {
      console.log(
        `      ! built ${shape.built[2].toFixed(2)} × ${shape.built[0].toFixed(2)} × ` +
          `${shape.built[1].toFixed(2)} m, expected ${shape.length.toFixed(2)} × ` +
          `${narrow.toFixed(2)}–${wide.toFixed(2)} × ${shape.height.toFixed(2)}` +
          (wrongWidth ? `  width ${(wrongWidth * 100).toFixed(0)}%` : '') +
          (Math.abs(wrongHeight) > 0.05 ? `  height ${(wrongHeight * 100).toFixed(0)}%` : '') +
          ` — length_m ${shape.best.toFixed(2)} fits this model best`,
      );
    }
  }
  // What the windows this vehicle actually has look like, area-weighted. The
  // flat colour that replaces them is set from it — see `materials`.
  let tone = null;
  if (image) {
    const sum = [0, 0, 0];
    let area = 0;
    mesh.faces.forEach((face, f) => {
      if ((mesh.groups?.[f] ?? 0) !== 1) return;
      const { area: a } = facet(mesh.positions, face);
      const colour = colourAt(face);
      for (let c = 0; c < 3; c++) sum[c] += colour[c] * a;
      area += a;
    });
    if (area > 0) tone = sum.map((c) => c / area);
  }

  return { mesh, texture: texture ? type.source : null, tone };
}

/**
 * Turns the model round, scales it to the real vehicle and stands it on the
 * ground.
 *
 * One factor for all three axes. The models are of real cars and their
 * proportions are right; fitting each axis on its own would only stretch away
 * what makes them look like cars. The length is what the factor comes from —
 * it is the dimension a catalogue states without ambiguity, while the width of
 * a bounding box silently includes the wing mirrors.
 */
/**
 * How well a model's own proportions match the vehicle it is catalogued as,
 * and what length would fit it best.
 *
 * The scale comes from the length alone — one factor for all three axes,
 * because these are models of real cars and fitting each axis separately would
 * only stretch away what makes them look like cars. But that means the width
 * and the height are *predictions*, and a generated model does not always
 * agree with the vehicle it was generated from: one of these eight is
 * proportioned like a car three hundred millimetres shorter than the one it is
 * named after, and another like a van two thirds of a metre longer.
 *
 * So the build measures what came out and says so. `best` is the length that
 * minimises the relative error over all three dimensions at once — the number
 * to put in the catalogue when the warning fires.
 */
function proportions(mesh, type) {
  const box = bodyBounds(mesh.positions, mesh.faces);
  const mirrors = type.mirrors_m ?? type.width_m;
  // The width is checked against a *span*, not a figure. Whether a bounding
  // box has the wing mirrors in it depends on how the model happens to be
  // split — on one of these the mirrors are their own islands and fall out of
  // the body's box, on another they are part of the shell and stay in — and
  // both are correct. Anywhere between the body and the mirrors is right.
  const target = [(type.width_m + mirrors) / 2, type.height_m, type.length_m];
  const model = [box.size[0], box.size[1], box.size[2]];
  let numerator = 0;
  let denominator = 0;
  for (let c = 0; c < 3; c++) {
    numerator += model[c] / target[c];
    denominator += (model[c] / target[c]) ** 2;
  }
  return {
    built: model,
    span: [type.width_m, mirrors],
    height: type.height_m,
    length: type.length_m,
    best: denominator > 0 ? (numerator / denominator) * model[2] : type.length_m,
  };
}

function fit(mesh, type) {
  // Lay the vehicle along Z first. An exporter is free to have written it out
  // facing along X, and one of these eight is — which shows up not as a car
  // pointing the wrong way but as a car two and a half times too big, because
  // the scale is taken from the length and the length was measured across it.
  const laid = bodyBounds(mesh.positions, mesh.faces);
  if (laid.size[0] > laid.size[2]) {
    for (let i = 0; i < mesh.positions.length; i += 3) {
      const x = mesh.positions[i];
      mesh.positions[i] = -mesh.positions[i + 2];
      mesh.positions[i + 2] = x;
    }
  }
  if ((type.nose ?? '-z') === '+z') {
    // Half a turn about the up axis. A rotation, not a mirror, so the winding
    // stands.
    for (let i = 0; i < mesh.positions.length; i += 3) {
      mesh.positions[i] = -mesh.positions[i];
      mesh.positions[i + 2] = -mesh.positions[i + 2];
    }
  }
  const box = bodyBounds(mesh.positions, mesh.faces);
  const scale = type.length_m / (box.size[2] || 1);
  const centre = [0, 1, 2].map((c) => (box.min[c] + box.max[c]) / 2);
  for (let i = 0; i < mesh.positions.length; i += 3) {
    mesh.positions[i] = (mesh.positions[i] - centre[0]) * scale;
    // On the ground, not around its middle: a car stands on the tarmac.
    mesh.positions[i + 1] = (mesh.positions[i + 1] - box.min[1]) * scale;
    mesh.positions[i + 2] = (mesh.positions[i + 2] - centre[2]) * scale;
  }
}

// ---------------------------------------------------------------------------
// Levels of detail
// ---------------------------------------------------------------------------

/**
 * A cleaned mesh as this pipeline's geometry, split by group.
 *
 * The normals are computed here, at every level, from the faces that meet at
 * each corner within the crease angle. The alternative — keeping what the file
 * brought — was tried and is what made these cars look shattered: a generated
 * mesh's normals are as approximate as everything else about it, and one in
 * ten of them points somewhere the surface does not.
 */
function toGeometry(mesh, group) {
  const out = { positions: [], normals: [], uvs: [], colors: [], indices: [], pieces: [] };
  const faceNormals = mesh.faces.map((face) => facet(mesh.positions, face));
  const around = new Map();
  mesh.faces.forEach((face, f) => {
    for (const v of face) {
      if (!around.has(v)) around.set(v, []);
      around.get(v).push(f);
    }
  });
  const seen = new Map();
  mesh.faces.forEach((face, f) => {
    if ((mesh.groups?.[f] ?? 0) !== group) return;
    const corners = [];
    for (const v of face) {
      // Averaged over the faces that meet within the crease angle, so a panel
      // is smooth and a shut line is not.
      const own = faceNormals[f].normal;
      const n = [0, 0, 0];
      for (const other of around.get(v)) {
        const m = faceNormals[other];
        const dot = m.normal[0] * own[0] + m.normal[1] * own[1] + m.normal[2] * own[2];
        if (dot < CREASE) continue;
        for (let c = 0; c < 3; c++) n[c] += m.normal[c] * m.area;
      }
      const length = Math.hypot(...n) || 1;
      const normal = n.map((c) => c / length);
      const u = mesh.uvs ? mesh.uvs[v * 2] : 0;
      const w = mesh.uvs ? mesh.uvs[v * 2 + 1] : 0;
      const vertex = [
        mesh.positions[v * 3],
        mesh.positions[v * 3 + 1],
        mesh.positions[v * 3 + 2],
        ...normal,
        u,
        w,
      ];
      const key = vertex.map((c) => Math.round(c * 8192)).join(',');
      let at = seen.get(key);
      if (at === undefined) {
        at = out.positions.length / 3;
        seen.set(key, at);
        out.positions.push(vertex[0], vertex[1], vertex[2]);
        out.normals.push(vertex[3], vertex[4], vertex[5]);
        out.uvs.push(u, w);
        out.colors.push(1, 1, 1, 1);
        out.pieces.push(0);
      }
      corners.push(at);
    }
    out.indices.push(...corners);
  });
  return out;
}

/** Every level of one vehicle, as `{ body, glass }` geometries. */
function levelsOf(mesh, levels) {
  // The colour payload carries the group in its fifth slot, which is what the
  // decimator compares: the line between paint and glass is then a border it
  // holds on to rather than a difference it averages away.
  const colors = mesh.faces.map((_, f) => [1, 1, 1, 1, mesh.groups?.[f] ?? 0]);
  const welded = weld(mesh.positions, mesh.faces, colors, 1e-5, mesh.uvs);
  const source = welded.faces.length;

  return levels.map((level, index) => {
    let level_mesh;
    if (index === 0) {
      // Every triangle the cleaned model has.
      level_mesh = mesh;
    } else {
      const cut = simplify(welded, Math.max(24, Math.round(source * level.keep)));
      level_mesh = {
        positions: cut.positions,
        uvs: cut.uvs,
        faces: cut.faces,
        groups: cut.colors.map((c) => c[4]),
      };
    }
    return {
      name: `car_LOD${index}`,
      body: toGeometry(level_mesh, 0),
      glass: toGeometry(level_mesh, 1),
      tris: level_mesh.faces.length,
    };
  });
}

// ---------------------------------------------------------------------------
// The atlas
// ---------------------------------------------------------------------------

/**
 * The shipped atlas: the source image with its dead space flooded, block
 * compressed, with a mip chain.
 *
 * This cannot be done at import time, which is where it used to be, because it
 * needs the model: only the mesh's own texture coordinates say which part of
 * the image is the car and which part is the gap between two scraps of it. A
 * tyre and the dead space are the same colour, and no rule about brightness
 * can separate them.
 */
function writeAtlas(type, mesh, into) {
  const image = decodePng(readableTexture(type.source));
  const painted = mesh.faces.filter((_, f) => (mesh.groups?.[f] ?? 0) === 0);
  const mask = coverage(mesh.uvs, painted, image.width);
  const gaps = dilate(image.pixels, mask, image.width);

  const png = join(dirname(readableTexture(type.source)), `${type.source}-packed.png`);
  writeFileSync(png, encodePng(image.pixels, image.width, image.height));
  // DXT1 and a full mip chain. No alpha: a car's atlas is opaque, and the four
  // bits a pixel BC1 costs are what keeps seven vehicles under five megabytes.
  execFileSync('magick', [
    png,
    '-resize', `${SHIPPED}x${SHIPPED}`,
    '-define', 'dds:mipmaps=10',
    '-define', 'dds:compression=dxt1',
    into,
  ]);
  return { covered: 1 - gaps / mask.length, size: statSync(into).size };
}

// ---------------------------------------------------------------------------
// Writing the mod
// ---------------------------------------------------------------------------

/**
 * The two materials every vehicle has.
 *
 * The body wears the atlas it was baked with; `tint` multiplies it, which is
 * what gives a car park more than one colour out of one texture — the atlases
 * are near enough to grey that a multiply reads as paint, and the parts of
 * them that are already dark (tyres, grille, shut lines) stay dark whatever
 * the tint.
 *
 * The glass carries no texture at all. A baked window is a photograph of
 * somebody else's street; a dark, smooth, untextured surface is what reads as
 * glass under the game's own light, at every distance and every time of day.
 */
/** How much the atlas is dimmed. See `materials`. */
const BAKED_LIGHT = 0.82;

/**
 * The flat colour that stands in for a window.
 *
 * Taken from the windows it replaces, rather than fixed. A fixed near-black
 * was tried and the edge of every window came out serrated: wherever the
 * classification put the boundary a triangle to one side or the other of the
 * rubber — and on a mesh of this kind it always will, because the boundary
 * does not run along triangle edges — the step between a nearly black flat
 * surface and a mid-grey photograph of a reflection is plainly visible, and
 * reads as teeth. Matched to the photograph the step disappears, and a
 * triangle out of place stops mattering at all.
 *
 * Still pulled down and towards neutral: a window should be darker than what
 * it reflects, and a tinge of the sky that happened to be in the photograph is
 * not something to carry into the game.
 */
function glassTone(tone) {
  if (!tone) return [0.045, 0.05, 0.062];
  const grey = 0.2126 * tone[0] + 0.7152 * tone[1] + 0.0722 * tone[2];
  return tone.map((c) => (c * 0.35 + grey * 0.65) * 0.62 * BAKED_LIGHT);
}

const materials = (image, tint, tone) => [
  {
    name: 'lack',
    // Dimmed, and for a reason worth writing down: these atlases are baked
    // *with the light already in them*. Lit a second time by the game's own
    // sun they come out bleached — a car park of white blobs at noon. Taking
    // a fifth off the base colour puts them back where a photograph of a
    // silver car belongs, and costs nothing at dusk because the sun is what
    // multiplies it.
    color: (tint ?? [1, 1, 1]).map((c) => c * BAKED_LIGHT).concat(1),
    // Barely metallic: with no metallic-roughness map to vary it, anything
    // more turns the whole body into one mirror of the sky.
    metallic: 0.05,
    roughness: 0.45,
    ...(image !== null ? { maps: { colour: image } } : {}),
  },
  {
    name: 'glas',
    color: [...glassTone(tone), 1],
    metallic: 0.0,
    roughness: 0.06,
  },
];

function ron(type, paint, reach) {
  const file = paint ? `${type.id}_${paint.id}` : type.id;
  const name = paint ? `${type.name_de}, ${paint.name_de}` : type.name_de;
  const tags = paint ? [...type.tags, paint.id] : [...type.tags];
  return {
    file,
    text: `// ${name} — generated by tools/cars/build_cars.mjs from
// ${type.source}; edit tools/cars/cars.json, not this file.
(
    name: "${name}",
    model: "cars/assets/${file}.gltf",
    lateral_offset: 0.0,
    yaw_deg: 0.0,
    height: 0.0,
    lod_distances: [${reach.join(', ')}],
    // What the imagery detection measures a find against before it picks this
    // model for it. Width is over the mirrors: that is what touches the car in
    // the next bay.
    footprint: Some((length: ${type.length_m.toFixed(2)}, width: ${(type.mirrors_m ?? type.width_m).toFixed(2)})),
    tags: [${tags.map((t) => `"${t}"`).join(', ')}],
)
`,
  };
}

function manifest(types, objects) {
  return `// Generated by tools/cars/build_cars.mjs — see tools/cars/README.md.
(
    id: "cars",
    name: "Straßenfahrzeuge",
    version: "3.0.0",
    author: "Connected Rails",
    description: "Die Fahrzeuge, die neben einer deutschen Strecke stehen: ${types} Klassen vom Kleinwagen bis zum Kastenwagen in ${objects} Lackierungen, auf reale Maße gebracht, ohne Innenraum, mit dunklen Scheiben und vier Detailstufen. Sie sind das, was der Luftbild-Import setzt, wenn er ein Auto oder einen Transporter erkennt.",
    depends: [],
    enabled: true,
)
`;
}

function build(catalogue, { write, preview, only }) {
  const types = catalogue.types.filter((t) => !only || only.includes(t.id));
  const levels = catalogue.levels;
  const paints = catalogue.paints ?? [];
  const rows = [];
  const sheets = [];
  let objects = 0;

  for (const type of types) {
    // A vehicle whose archive came without its atlas cannot be built. It is
    // not a lesser version of itself: it is a white shell with the glass
    // guessed from shape alone, and next to seven textured cars it reads as
    // broken — which is exactly what it is. Better to say so and leave it out
    // than to ship it and have somebody find it in a car park.
    if (!existsSync(readableTexture(type.source))) {
      console.log(
        `  ${type.id.padEnd(15)} skipped — no colour atlas in the archive for ` +
          `${type.source}. Export it again with its texture and re-run ` +
          `import_cars.mjs; nothing else has to change.`,
      );
      continue;
    }
    const { mesh, texture, tone } = readVehicle(type);
    const built = levelsOf(mesh, levels);

    const buffer = new BufferBuilder();
    const packed = built.map((level) => ({
      name: level.name,
      parts: [
        ...(level.body.indices.length ? [{ packed: buffer.add(level.body), material: 0 }] : []),
        ...(level.glass.indices.length ? [{ packed: buffer.add(level.glass), material: 1 }] : []),
      ],
    }));

    const wanted = paints.length && type.paints !== false ? paints : [null];
    objects += wanted.length;
    rows.push({
      id: type.id,
      source: type.source,
      tris: built.map((l) => l.tris),
      paints: wanted.length,
      texture: Boolean(texture),
    });
    if (preview) {
      sheets.push({
        id: type.id,
        length: type.length_m,
        levels: built.map((level, index) => ({
          name: level.name,
          reach: levels[index].reach,
          geometry: merge(level.body, level.glass),
        })),
      });
    }
    if (!write) continue;

    const bin = `${type.id}.bin`;
    writeBuffer(assetDir, bin, buffer.toBuffer());
    const atlas = texture ? `${type.id}.dds` : null;
    if (atlas) {
      const packedAtlas = writeAtlas(type, mesh, join(assetDir, atlas));
      console.log(
        `  ${''.padEnd(15)} atlas ${SHIPPED}²  ` +
          `${(packedAtlas.covered * 100).toFixed(0)}% of it is car, the rest flooded  ` +
          `${(packedAtlas.size / 1024).toFixed(0)} KB`,
      );
    }
    for (const paint of wanted) {
      const entry = ron(type, paint, levels.map((l) => l.reach));
      writeGltf({
        path: join(assetDir, `${entry.file}.gltf`),
        name: entry.file,
        buffer: bin,
        bufferLength: buffer.length,
        levels: packed,
        materials: materials(atlas ? 0 : null, paint?.rgb, tone),
        images: atlas ? [atlas] : [],
        copyright: COPYRIGHT,
        generator: 'tools/cars/build_cars.mjs (Connected Rails)',
      });
      writeFileSync(join(objectDir, `${entry.file}.ron`), entry.text, 'utf8');
    }
  }

  if (preview) {
    const directory = '/tmp/car-preview';
    renderSheet(sheets, directory);
    renderOverview(sheets, directory);
    renderHandovers(sheets, directory);
    console.log(`preview sheets in ${directory}`);
  }
  let total = 0;
  for (const row of rows) total += row.tris[0];
  console.log(
    `${rows.length} vehicles, ${objects} objects, ` +
      `${total} triangles at LOD0 (${Math.round(total / Math.max(1, rows.length))} each)`,
  );
  return { rows, objects };
}

/** Body and glass into one geometry — preview only. */
function merge(body, glass) {
  const out = { positions: [], normals: [], uvs: [], colors: [], indices: [] };
  for (const [part, tint] of [
    [body, [0.78, 0.79, 0.8]],
    [glass, [0.1, 0.12, 0.15]],
  ]) {
    const base = out.positions.length / 3;
    out.positions.push(...part.positions);
    out.normals.push(...part.normals);
    out.uvs.push(...part.uvs);
    for (let i = 0; i < part.positions.length / 3; i++) out.colors.push(...tint, 1);
    out.indices.push(...part.indices.map((i) => i + base));
  }
  return out;
}

function main() {
  const args = process.argv.slice(2);
  const report = args.includes('--report');
  const preview = args.includes('--preview');
  const write = !report;
  const onlyArg =
    args.find((a) => a.startsWith('--only='))?.slice(7) ??
    (args.includes('--only') ? args[args.indexOf('--only') + 1] : null);
  const only = onlyArg ? onlyArg.split(',').map((s) => s.trim()) : null;

  const catalogue = JSON.parse(readFileSync(join(here, 'cars.json'), 'utf8'));
  if (write) {
    // The set of vehicles changes with the catalogue, and a stale file from a
    // type that was renamed would go on being loaded and placed.
    if (!only) {
      for (const directory of [objectDir, assetDir]) {
        if (existsSync(directory)) rmSync(directory, { recursive: true });
      }
    }
    mkdirSync(assetDir, { recursive: true });
    mkdirSync(objectDir, { recursive: true });
  }
  const { rows, objects } = build(catalogue, { write, preview, only });
  if (write && !only) {
    writeFileSync(join(modDir, 'mod.ron'), manifest(rows.length, objects), 'utf8');
  }
}

main();
