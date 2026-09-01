// Which faces of a mesh can be seen from outside it.
//
// A model generated from photographs — which is what these vehicles are —
// carries everything the generator thought it saw: a seat behind a windscreen,
// a floor under the doors, the far side of a wheel arch, the inside of the very
// shell it just built. None of it is ever seen in a car park, all of it is
// paid for on every one of two hundred instances, and none of it can be found
// by name because the whole car is one nameless mesh.
//
// It can be found by *looking*, though, which is the one thing this repository
// already has a tool for. The mesh is rasterised from a sphere of directions
// with a depth buffer that remembers which face won each pixel; a face that
// never wins a pixel from any direction is a face nobody will ever see.
//
// Two decisions worth stating:
//
// * **Both sides count.** A face seen from behind is still seen — an interior
//   panel visible through a window opening has to survive, or the hole it
//   leaves is worse than the panel was.
// * **Nothing is looked at from straight underneath.** A car stands on the
//   ground. Faces that only the tarmac could see are as invisible as the ones
//   inside the boot, and dropping them is most of what this saves.

/** Directions spread evenly over a sphere, cut off below the horizon. */
export function viewpoints(count = 96, lowest = -30) {
  const out = [];
  const golden = Math.PI * (3 - Math.sqrt(5));
  const sin = Math.sin((lowest * Math.PI) / 180);
  for (let i = 0; i < count; i++) {
    // Even in the sine of the elevation, so the sphere is covered evenly
    // rather than bunched at the pole.
    const y = sin + ((1 - sin) * i) / Math.max(1, count - 1);
    const radius = Math.sqrt(Math.max(0, 1 - y * y));
    const angle = golden * i;
    out.push([Math.cos(angle) * radius, y, Math.sin(angle) * radius]);
  }
  return out;
}

/** An orthonormal basis with `w` along the view direction. */
function basis(direction) {
  const w = direction;
  const up = Math.abs(w[1]) > 0.95 ? [0, 0, 1] : [0, 1, 0];
  let u = [
    up[1] * w[2] - up[2] * w[1],
    up[2] * w[0] - up[0] * w[2],
    up[0] * w[1] - up[1] * w[0],
  ];
  const length = Math.hypot(...u) || 1;
  u = u.map((c) => c / length);
  const v = [
    w[1] * u[2] - w[2] * u[1],
    w[2] * u[0] - w[0] * u[2],
    w[0] * u[1] - w[1] * u[0],
  ];
  return { u, v, w };
}

/**
 * A flag per face, and the directions each was seen from.
 *
 * `resolution` is the side of the depth buffer each direction is rasterised
 * into. It is the only quality knob that matters: too coarse and a slim face —
 * a wing mirror's edge, a number plate — never wins a pixel and is culled
 * although it is in plain sight.
 */
export function visibleFaces(positions, faces, { directions = 96, resolution = 512, lowest = -30 } = {}) {
  const seen = new Uint8Array(faces.length);
  // Where each face was looked at from, summed. A face's outward normal has to
  // point roughly back at whoever saw it, which is the one test that says
  // whether a triangle is wound the right way round — and it comes free with
  // the pass that is already looking from everywhere.
  const towards = new Float64Array(faces.length * 3);
  const min = [Infinity, Infinity, Infinity];
  const max = [-Infinity, -Infinity, -Infinity];
  for (let i = 0; i < positions.length; i += 3) {
    for (let c = 0; c < 3; c++) {
      min[c] = Math.min(min[c], positions[i + c]);
      max[c] = Math.max(max[c], positions[i + c]);
    }
  }
  const centre = [0, 1, 2].map((c) => (min[c] + max[c]) / 2);
  const radius = Math.hypot(...[0, 1, 2].map((c) => (max[c] - min[c]) / 2)) || 1;
  const scale = resolution / (2.05 * radius);

  const depth = new Float32Array(resolution * resolution);
  const owner = new Int32Array(resolution * resolution);

  for (const direction of viewpoints(directions, lowest)) {
    const { u, v, w } = basis(direction);
    depth.fill(Infinity);
    owner.fill(-1);
    for (let f = 0; f < faces.length; f++) {
      const face = faces[f];
      const points = [];
      for (const index of face) {
        const x = positions[index * 3] - centre[0];
        const y = positions[index * 3 + 1] - centre[1];
        const z = positions[index * 3 + 2] - centre[2];
        points.push({
          x: (x * u[0] + y * u[1] + z * u[2]) * scale + resolution / 2,
          y: (x * v[0] + y * v[1] + z * v[2]) * scale + resolution / 2,
          // Towards the eye: the smaller, the nearer.
          d: -(x * w[0] + y * w[1] + z * w[2]),
        });
      }
      const [a, b, c] = points;
      const area = (b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y);
      if (Math.abs(area) < 1e-9) continue;
      const minX = Math.max(0, Math.floor(Math.min(a.x, b.x, c.x)));
      const maxX = Math.min(resolution - 1, Math.ceil(Math.max(a.x, b.x, c.x)));
      const minY = Math.max(0, Math.floor(Math.min(a.y, b.y, c.y)));
      const maxY = Math.min(resolution - 1, Math.ceil(Math.max(a.y, b.y, c.y)));
      for (let y = minY; y <= maxY; y++) {
        for (let x = minX; x <= maxX; x++) {
          const px = x + 0.5;
          const py = y + 0.5;
          const w0 = ((b.x - px) * (c.y - py) - (c.x - px) * (b.y - py)) / area;
          const w1 = ((c.x - px) * (a.y - py) - (a.x - px) * (c.y - py)) / area;
          const w2 = 1 - w0 - w1;
          if (w0 < -1e-6 || w1 < -1e-6 || w2 < -1e-6) continue;
          const d = w0 * a.d + w1 * b.d + w2 * c.d;
          const at = y * resolution + x;
          if (d >= depth[at]) continue;
          depth[at] = d;
          owner[at] = f;
        }
      }
    }
    for (let i = 0; i < owner.length; i++) {
      const f = owner[i];
      if (f < 0) continue;
      seen[f] = 1;
      towards[f * 3] += direction[0];
      towards[f * 3 + 1] += direction[1];
      towards[f * 3 + 2] += direction[2];
    }
  }
  return { seen, towards };
}
