// The model audit's camera: puts every mast of the catalogue on one scratch
// line and works out where the simulator has to stand to photograph one of them.
//
//   node tools/pylons/shoot.mjs --write            # (re)write the audit line
//   node tools/pylons/shoot.mjs <object-id>        # whole mast
//   node tools/pylons/shoot.mjs <object-id> --detail   # the crossarms, close up
//   node tools/pylons/shoot.mjs <object-id> --foot     # the foot, close up
//
// It echoes the `--fly`/`--look` offsets, so the caller is one line of shell
// (`shoot.sh`). Scratch tooling: `mods/mastparade` is not shipped content.
//
// **The offsets are measured from the train's front, not from the start of the
// line**, because that is what `--fly` and `--look` are relative to — and the
// player train's front is a train's length along the track, not at zero. That
// cost an afternoon: a mast placed at "three hundred metres" and photographed
// from "two hundred and twenty" came out behind the camera, which looks exactly
// like a model that will not draw.
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '..', '..');
const lineFile = join(repo, 'mods', 'mastparade', 'lines', 'audit.ron');

/** The catalogue, ordered the way the parade is: by voltage, coarsest first. */
const ORDER = [
  'tonnenmast_380_trag',
  'tonnenmast_380_abspann',
  'kombimast_380_110_trag',
  'kombimast_380_110_abspann',
  'donaumast_380_trag',
  'donaumast_380_abspann',
  'kompaktmast_380_trag',
  'kompaktmast_380_abspann',
  'einebenenmast_380_trag',
  'einebenenmast_380_abspann',
  'portalmast_380_abspann',
  'tannenbaummast_220_trag',
  'tannenbaummast_220_abspann',
  'donaumast_220_trag',
  'donaumast_220_abspann',
  'donaumast_110_trag',
  'donaumast_110_abspann',
  'einebenenmast_110_trag',
  'einebenenmast_110_abspann',
  'bahnstrommast_110_zweiebenen_trag',
  'bahnstrommast_110_zweiebenen_abspann',
  'bahnstrommast_110_trag',
  'bahnstrommast_110_abspann',
  'stahlgittermast_20kv_trag',
  'stahlgittermast_20kv_abspann',
  'betonmast_20kv_einebene_trag',
  'betonmast_20kv_einebene_abspann',
  'betonmast_20kv_dreieck_trag',
  'masttrafo_20kv',
  'holzmast_nsp_trag',
  'holzmast_nsp_abspann',
  'fernmeldemast_bahn_trag',
  'fernmeldemast_bahn_abspann',
];

/** A field's width north of the line, and far enough apart to photograph one. */
const RIGHT = -70;
const SPACING = 150;
const FIRST = 150;
/** Half the simulator's vertical field of view. */
const HALF_FOV = Math.PI / 8;

// Degrees per metre on WGS84 at 52° N. The round 111 320 is a degree of
// latitude at the equator and is 0.3 % out here — half a metre over the length
// of this line, which is nothing, but the numbers are free.
const M_LAT = 1 / 111266;
const M_LON = 1 / 68685;
/**
 * Where the player train's front stands along the line [m].
 *
 * The train is placed from the start of the first edge and `--fly`/`--look`
 * measure from its **front vehicle**, so everything this file computes is
 * offset by the length of the consist. Measured off the render positions of a
 * mast at a known place; a different composition moves it, and the pictures say
 * so at once by framing the wrong thing.
 */
const TRAIN_HEAD = 190.6;
const forwardOf = (i) => FIRST + i * SPACING;

/** The model's own height [m], off the glTF's LOD0 position bounds. */
function heightOf(object) {
  const gltf = JSON.parse(
    readFileSync(join(repo, 'mods', 'pylons', 'assets', `${object}.gltf`), 'utf8'),
  );
  let top = 0;
  for (const primitive of gltf.meshes[0].primitives) {
    const max = gltf.accessors[primitive.attributes.POSITION].max;
    if (max && max[1] > top) top = max[1];
  }
  return top;
}

function writeLine() {
  const rows = ORDER.map((id, i) => {
    const s = forwardOf(i);
    return `        // ${String(i + 1).padStart(2)}  ${id}\n`
      + `        (object: "pylons:${id}", lat: ${(52 - RIGHT * M_LAT).toFixed(7)}, `
      + `lon: ${(10 + (TRAIN_HEAD + s) * M_LON).toFixed(7)}, yaw_deg: 0.0, scale: 1.0),`;
  }).join('\n');
  const end = TRAIN_HEAD + forwardOf(ORDER.length - 1) + 400;
  writeFileSync(
    lineFile,
    `// The whole pylon catalogue in one row, ${SPACING} m apart, for the model audit.
// Written by tools/pylons/shoot.mjs --write; scratch content, not shipped.
(
    name: "Mastaudit",
    year: Some(2026),
    fictional: true,
    nodes: [Buffer, Buffer],
    edges: [
        (
            from: 0,
            to: 1,
            start: Geo(point: (lat: 52.0, lon: 10.0, height: 100.0), heading_deg: 90.0),
            segments: [(len: ${end.toFixed(1)}, k0: 0.0, dk: 0.0)],
            grade: [(0.0, 0.0)],
            cant: [],
            speed: [(0.0, 120.0)],
            track_type: [],
            electrification: [(0.0, "ac-15kv")],
            formation: true,
        ),
    ],
    trees: [
${rows}
    ],
    anchor: Some((lat: 52.0, lon: ${(10 + (end / 2) * M_LON).toFixed(7)}, height: 100.0)),
    envelope: [
        (lat: 51.9930, lon: 9.9950),
        (lat: 51.9930, lon: ${(10 + (end + 400) * M_LON).toFixed(4)}),
        (lat: 52.0070, lon: ${(10 + (end + 400) * M_LON).toFixed(4)}),
        (lat: 52.0070, lon: 9.9950),
    ],
)
`,
  );
}

const args = process.argv.slice(2);
if (args.includes('--write') || args.length === 0) {
  writeLine();
  if (args.length && args[0] === '--write') {
    console.error(`${lineFile}: ${ORDER.length} masts`);
    process.exit(0);
  }
}

const id = args.find((a) => !a.startsWith('--'));
if (!id) throw new Error('usage: shoot.mjs <object-id> [--detail|--foot]');
const index = ORDER.indexOf(id);
if (index < 0) throw new Error(`${id} is not in the catalogue`);
writeLine();

const height = heightOf(id);
const forward = forwardOf(index);

let eye;
let look;
if (args.includes('--detail')) {
  // The crossarms, from close enough to read a bolt.
  const at = height * 0.82;
  eye = [RIGHT + 9, at, forward - 4];
  look = [RIGHT, at, forward];
} else if (args.includes('--foot')) {
  // Where the steel meets its concrete, which is where the dirt is.
  eye = [RIGHT + 7, 2.2, forward - 5];
  look = [RIGHT, 1.6, forward];
} else {
  // The whole thing, at about seventy per cent of the frame height. A tighter
  // fit reads as cut off: the camera sits at half the mast's height, so the
  // margin is split between top and bottom and half of a tenth is nothing.
  const distance = (height * 1.45) / 2 / Math.tan(HALF_FOV);
  eye = [RIGHT, height / 2, forward - distance];
  look = [RIGHT, height / 2, forward];
}

const f = (v) => v.map((n) => n.toFixed(1)).join(',');
console.log(`--fly ${f(eye)} --look ${f(look)}`);
console.error(`${id}: ${height.toFixed(1)} m, km 0.${forward}`);
