"""Procedural DB BR 101 model generator for the example mod.

Writes mods/example/assets/br101.gltf — created entirely from scratch in this
script (public dimensions of the class 101 only), no third-party assets, so
the file carries the project's licence.

Conventions per MODS.md: origin at vehicle centre on rail top, runs along
-Z/+Z, +Y up, units metres. Nodes: body_LOD0/body_LOD1, pant_front/pant_rear
with ts_* extras.

Run: python tools/gen_br101.py
"""

import base64
import json
import math
import struct
from pathlib import Path

OUT = Path(__file__).resolve().parents[1] / "mods" / "example" / "assets" / "br101.gltf"

# Class 101 key dimensions [m]: 19.10 over buffers, 2.95 wide, ~3.96 roof,
# bogie pivots +-4.85, axle base 2.65 per bogie, wheel dia 1.25.
HALF_W = 1.475     # body half width
ROOF_HALF_W = 1.02  # half width at roof top (chamfered shoulder)
Y_FLOOR = 1.05     # body underside above rail top
Y_SIDE_TOP = 3.42  # where the roof chamfer starts
Y_ROOF = 3.96      # roof top
Z_STRAIGHT = 6.9   # straight body section ends, nose taper begins
Z_NOSE = 9.0       # nose foot (front face bottom edge)
Z_BUFFER = 9.55    # buffer face (length over buffers 19.10)
NOSE_HALF_W = 1.18
NOSE_ROOF_HALF_W = 0.82
RAKE = 0.55        # front face set-back per metre of height (the raked BR101 face)
BOGIE_Z = 4.85
AXLE_HALF_BASE = 1.325
WHEEL_R = 0.625
RAIL_X = 1.435 / 2

# --- small vector / mesh helpers ---------------------------------------------


def sub(a, b):
    return (a[0] - b[0], a[1] - b[1], a[2] - b[2])


def cross(a, b):
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def dot(a, b):
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def normalize(v):
    l = math.sqrt(dot(v, v))
    return (v[0] / l, v[1] / l, v[2] / l)


class Prim:
    """One glTF primitive: flat-shaded triangles with per-face normals."""

    def __init__(self):
        self.pos, self.nrm, self.idx = [], [], []

    def tri(self, a, b, c):
        n = normalize(cross(sub(b, a), sub(c, a)))
        base = len(self.pos)
        self.pos += [a, b, c]
        self.nrm += [n, n, n]
        self.idx += [base, base + 1, base + 2]

    def quad(self, a, b, c, d):  # corners CCW seen from outside
        self.tri(a, b, c)
        self.tri(a, c, d)

    def oriented_quad(self, a, b, c, d, outward):
        """Quad whose winding is fixed up to face roughly along `outward`."""
        if dot(cross(sub(b, a), sub(c, a)), outward) < 0:
            a, b, c, d = d, c, b, a
        self.quad(a, b, c, d)

    def fan(self, pts):  # planar polygon, CCW seen from outside
        for i in range(1, len(pts) - 1):
            self.tri(pts[0], pts[i], pts[i + 1])

    def box(self, mn, mx):
        x0, y0, z0 = mn
        x1, y1, z1 = mx
        self.quad((x1, y0, z0), (x1, y1, z0), (x1, y1, z1), (x1, y0, z1))  # +X
        self.quad((x0, y0, z1), (x0, y1, z1), (x0, y1, z0), (x0, y0, z0))  # -X
        self.quad((x0, y1, z0), (x0, y1, z1), (x1, y1, z1), (x1, y1, z0))  # +Y
        self.quad((x0, y0, z0), (x1, y0, z0), (x1, y0, z1), (x0, y0, z1))  # -Y
        self.quad((x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1))  # +Z
        self.quad((x1, y0, z0), (x0, y0, z0), (x0, y1, z0), (x1, y1, z0))  # -Z

    def tube(self, loops, cap_start=True, cap_end=True):
        """Loft point loops (same length, CCW viewed from +Z, z increasing)."""
        for a, b in zip(loops, loops[1:]):
            for i in range(len(a)):
                j = (i + 1) % len(a)
                self.quad(a[i], a[j], b[j], b[i])
        if cap_start:
            self.fan(loops[0][::-1])
        if cap_end:
            self.fan(loops[-1])

    def cyl_x(self, cx, cy, cz, r, half_w, segs=16):
        """Cylinder along the X axis (a wheel)."""
        ring0, ring1 = [], []
        for i in range(segs):
            t = 2 * math.pi * i / segs
            y, z = cy + r * math.cos(t), cz + r * math.sin(t)
            ring0.append((cx - half_w, y, z))
            ring1.append((cx + half_w, y, z))
        for i in range(segs):
            j = (i + 1) % segs
            self.quad(ring0[i], ring0[j], ring1[j], ring1[i])
        self.fan(ring1)          # +X cap
        self.fan(ring0[::-1])    # -X cap


def body_profile(z, w, wr, rake_sign=0.0):
    """6-point body cross-section, CCW viewed from +Z.

    rake_sign: +1 leans the profile back for the +Z nose, -1 for the -Z nose.
    """
    pts = [(-w, Y_FLOOR), (w, Y_FLOOR), (w, Y_SIDE_TOP),
           (ROOF_HALF_W if w == HALF_W else wr, Y_ROOF),
           (-(ROOF_HALF_W if w == HALF_W else wr), Y_ROOF), (-w, Y_SIDE_TOP)]
    return [(x, y, z - rake_sign * RAKE * (y - Y_FLOOR)) for x, y in pts]


def front_plane_point(x, y, z_sign):
    """Point on the raked front face at height y, pushed 1 cm outward."""
    z = z_sign * (Z_NOSE - RAKE * (y - Y_FLOOR))
    n = normalize((0.0, RAKE, z_sign))
    return (x + 0.01 * n[0], y + 0.01 * n[1], z + 0.01 * n[2])


# --- build the model ----------------------------------------------------------

MATERIALS = [
    # baseColorFactor is linear RGB.
    ("red", dict(baseColorFactor=[0.55, 0.02, 0.02, 1.0], metallicFactor=0.1, roughnessFactor=0.45)),
    ("glass", dict(baseColorFactor=[0.01, 0.011, 0.013, 1.0], metallicFactor=0.2, roughnessFactor=0.15)),
    ("stripe", dict(baseColorFactor=[0.62, 0.63, 0.61, 1.0], metallicFactor=0.0, roughnessFactor=0.6)),
    ("roof", dict(baseColorFactor=[0.23, 0.24, 0.25, 1.0], metallicFactor=0.2, roughnessFactor=0.7)),
    ("frame", dict(baseColorFactor=[0.045, 0.045, 0.05, 1.0], metallicFactor=0.1, roughnessFactor=0.9)),
    ("wheel", dict(baseColorFactor=[0.08, 0.08, 0.09, 1.0], metallicFactor=0.4, roughnessFactor=0.6)),
    ("pant", dict(baseColorFactor=[0.12, 0.12, 0.13, 1.0], metallicFactor=0.6, roughnessFactor=0.5)),
    ("light", dict(baseColorFactor=[0.9, 0.88, 0.8, 1.0], metallicFactor=0.0, roughnessFactor=0.3)),
]
MAT = {name: i for i, (name, _) in enumerate(MATERIALS)}


def build_body_loft(prim):
    loops = [
        body_profile(-Z_NOSE, NOSE_HALF_W, NOSE_ROOF_HALF_W, rake_sign=-1.0),
        body_profile(-Z_STRAIGHT, HALF_W, ROOF_HALF_W),
        body_profile(Z_STRAIGHT, HALF_W, ROOF_HALF_W),
        body_profile(Z_NOSE, NOSE_HALF_W, NOSE_ROOF_HALF_W, rake_sign=1.0),
    ]
    prim.tube(loops)


def build_lod0():
    prims = {name: Prim() for name in MAT}
    red, glass, stripe, roof, frame, wheel, light = (
        prims["red"], prims["glass"], prims["stripe"], prims["roof"], prims["frame"],
        prims["wheel"], prims["light"])

    build_body_loft(red)

    # Light grey frame stripe along the lower body edge (straight section).
    stripe.box((-HALF_W - 0.004, Y_FLOOR - 0.02, -Z_STRAIGHT + 0.02),
               (HALF_W + 0.004, Y_FLOOR + 0.23, Z_STRAIGHT - 0.02))

    # Grey roof well with a cable duct and the main-switch box, between the pantographs.
    roof.box((-0.98, Y_ROOF, -6.8), (0.98, Y_ROOF + 0.02, 6.8))
    roof.box((-0.22, Y_ROOF + 0.02, -4.4), (0.22, Y_ROOF + 0.12, 4.4))
    roof.box((-0.4, Y_ROOF + 0.02, 2.6), (0.4, Y_ROOF + 0.22, 3.6))

    for z_sign in (1.0, -1.0):
        # Windscreen band on the raked front face.
        ws = [front_plane_point(x, y, z_sign)
              for x, y in ((-0.92, 2.15), (0.92, 2.15), (0.92, 3.25), (-0.92, 3.25))]
        glass.oriented_quad(*ws, outward=(0, RAKE, z_sign))
        # Head lights: two low, one high above the windscreen.
        for lx, ly, s in ((-0.72, 1.55, 0.17), (0.72, 1.55, 0.17), (0.0, 3.5, 0.13)):
            pts = [front_plane_point(lx + dx, ly + dy, z_sign)
                   for dx, dy in ((-s, -s * 0.7), (s, -s * 0.7), (s, s * 0.7), (-s, s * 0.7))]
            light.oriented_quad(*pts, outward=(0, RAKE, z_sign))

        # Buffer beam, two buffers (drawn ~3 cm compressed per MODS.md) and coupler.
        z0, z1 = sorted((z_sign * 9.02, z_sign * 8.82))
        frame.box((-1.28, 0.70, z0), (1.28, 1.15, z1))
        for bx in (-0.875, 0.875):
            z0, z1 = sorted((z_sign * 9.02, z_sign * 9.45))
            frame.box((bx - 0.07, 0.98, z0), (bx + 0.07, 1.12, z1))
            z0, z1 = sorted((z_sign * 9.45, z_sign * (Z_BUFFER - 0.03)))
            frame.box((bx - 0.22, 0.86, z0), (bx + 0.22, 1.24, z1))
        z0, z1 = sorted((z_sign * 9.02, z_sign * 9.32))
        frame.box((-0.06, 0.90, z0), (0.06, 1.10, z1))

        # Cab side windows.
        for x_sign in (1.0, -1.0):
            x = x_sign * (HALF_W + 0.008)
            za, zb = sorted((z_sign * 6.0, z_sign * 6.75))
            glass.oriented_quad((x, 2.3, za), (x, 2.3, zb), (x, 3.1, zb), (x, 3.1, za),
                                outward=(x_sign, 0, 0))

    # Machine-room grilles high on the sides.
    for x_sign in (1.0, -1.0):
        x = x_sign * (HALF_W + 0.006)
        for zc in (-4.1, -2.5, 2.5, 4.1):
            roof.oriented_quad((x, 2.45, zc - 0.4), (x, 2.45, zc + 0.4),
                               (x, 3.15, zc + 0.4), (x, 3.15, zc - 0.4),
                               outward=(x_sign, 0, 0))

    # Transformer tank between the bogies.
    frame.box((-1.32, 0.60, -2.95), (1.32, 1.05, 2.95))

    # Bogies: frame plus four wheels each.
    for zc in (BOGIE_Z, -BOGIE_Z):
        frame.box((-1.08, 0.50, zc - 1.9), (1.08, 0.95, zc + 1.9))
        for za in (zc - AXLE_HALF_BASE, zc + AXLE_HALF_BASE):
            for x_sign in (1.0, -1.0):
                wheel.cyl_x(x_sign * RAIL_X, WHEEL_R, za, WHEEL_R, 0.0675)

    return prims


def build_lod1():
    prim = Prim()
    build_body_loft(prim)
    return {"red": prim}


def build_pantograph():
    """Lowered single-arm pantograph; node origin sits on the roof.

    The runtime raises it by rotating the whole node 45 deg about +X, which
    lifts the -Z knee end (ts_motion in the node extras).
    """
    p = Prim()
    for ix in (-0.42, 0.42):
        for iz in (-0.30, 0.30):
            p.box((ix - 0.04, 0.0, iz - 0.04), (ix + 0.04, 0.14, iz + 0.04))  # insulators
    for ix in (-0.42, 0.42):
        p.box((ix - 0.06, 0.14, -0.52), (ix + 0.06, 0.20, 0.52))  # base rails

    def arm_loop(z, yc, hw):
        return [(-hw, yc - 0.045, z), (hw, yc - 0.045, z), (hw, yc + 0.045, z), (-hw, yc + 0.045, z)]

    p.tube([arm_loop(-0.45, 0.24, 0.05), arm_loop(0.40, 0.30, 0.05)])   # lower arm
    p.tube([arm_loop(-0.35, 0.38, 0.04), arm_loop(0.40, 0.30, 0.04)])   # upper arm
    p.box((-0.9, 0.40, -0.42), (0.9, 0.45, -0.28))                       # contact strip
    return {"pant": p}


# --- glTF assembly ------------------------------------------------------------


def build_gltf():
    blob = bytearray()
    buffer_views, accessors, meshes = [], [], []

    def accessor(data, comp_type, count, type_, target, minmax=None):
        while len(blob) % 4:
            blob.append(0)
        buffer_views.append({"buffer": 0, "byteOffset": len(blob), "byteLength": len(data), "target": target})
        blob.extend(data)
        acc = {"bufferView": len(buffer_views) - 1, "componentType": comp_type, "count": count, "type": type_}
        if minmax:
            acc["min"], acc["max"] = minmax
        accessors.append(acc)
        return len(accessors) - 1

    def add_mesh(name, prims_by_mat):
        gl_prims = []
        for mat_name, prim in prims_by_mat.items():
            if not prim.pos:
                continue
            flat_pos = [c for v in prim.pos for c in v]
            flat_nrm = [c for v in prim.nrm for c in v]
            mins = [min(p[i] for p in prim.pos) for i in range(3)]
            maxs = [max(p[i] for p in prim.pos) for i in range(3)]
            gl_prims.append({
                "attributes": {
                    "POSITION": accessor(struct.pack(f"<{len(flat_pos)}f", *flat_pos),
                                         5126, len(prim.pos), "VEC3", 34962, (mins, maxs)),
                    "NORMAL": accessor(struct.pack(f"<{len(flat_nrm)}f", *flat_nrm),
                                       5126, len(prim.nrm), "VEC3", 34962),
                },
                "indices": accessor(struct.pack(f"<{len(prim.idx)}I", *prim.idx),
                                    5125, len(prim.idx), "SCALAR", 34963),
                "material": MAT[mat_name],
            })
        meshes.append({"name": name, "primitives": gl_prims})
        return len(meshes) - 1

    lod0 = add_mesh("br101_LOD0", build_lod0())
    lod1 = add_mesh("br101_LOD1", build_lod1())
    pant = add_mesh("pantograph", build_pantograph())

    pant_extras = {"ts_function": "pantograph", "ts_motion": "rotate", "ts_axis": "1 0 0", "ts_amount": 45}
    nodes = [
        {"name": "body_LOD0", "mesh": lod0},
        {"name": "body_LOD1", "mesh": lod1},
        {"name": "pant_front", "mesh": pant, "translation": [0.0, Y_ROOF + 0.02, -5.7], "extras": pant_extras},
        {"name": "pant_rear", "mesh": pant, "translation": [0.0, Y_ROOF + 0.02, 5.7],
         "rotation": [0.0, 1.0, 0.0, 0.0], "extras": pant_extras},
    ]

    return {
        "asset": {
            "version": "2.0",
            "generator": "tools/gen_br101.py (TrainSim-DE example mod)",
            "copyright": "TrainSim-DE project. Procedural model built from scratch in tools/gen_br101.py, no third-party assets; licensed like the project.",
        },
        "scene": 0,
        "scenes": [{"name": "br101", "nodes": list(range(len(nodes)))}],
        "nodes": nodes,
        "meshes": meshes,
        "materials": [dict(name=n, pbrMetallicRoughness=m) for n, m in MATERIALS],
        "accessors": accessors,
        "bufferViews": buffer_views,
        "buffers": [{
            "byteLength": len(blob),
            "uri": "data:application/octet-stream;base64," + base64.b64encode(bytes(blob)).decode(),
        }],
    }


# --- self-check ----------------------------------------------------------------


def check():
    # Convex solids must have outward normals.
    for prim, center in ((Prim(), (0.5, 0.5, 0.5)), (Prim(), (0.0, 0.0, 0.0))):
        if center[0]:
            prim.box((0, 0, 0), (1, 1, 1))
        else:
            prim.cyl_x(0, 0, 0, 1.0, 0.5)
        for t in range(0, len(prim.idx), 3):
            a, b, c = (prim.pos[prim.idx[t + k]] for k in range(3))
            centroid = tuple((a[i] + b[i] + c[i]) / 3 - center[i] for i in range(3))
            assert dot(prim.nrm[prim.idx[t]], centroid) > 0, "inward-facing triangle"

    # Model bounds: on the rails, inside the loading gauge, 19.10 m over buffers.
    prims = build_lod0()
    pts = [p for prim in prims.values() for p in prim.pos]
    assert all(abs(p[0]) <= 1.6 for p in pts)
    assert all(-0.01 <= p[1] <= Y_ROOF + 0.3 for p in pts)
    z_max = max(p[2] for p in pts)
    assert abs(z_max - (Z_BUFFER - 0.03)) < 1e-6, z_max
    assert min(p[1] for p in prims["wheel"].pos) < 1e-6  # wheels touch the rail
    tris = sum(len(prim.idx) for prim in prims.values()) // 3
    return tris


if __name__ == "__main__":
    tris = check()
    gltf = build_gltf()
    OUT.write_text(json.dumps(gltf, indent=1) + "\n", encoding="utf-8", newline="\n")
    print(f"wrote {OUT} ({OUT.stat().st_size // 1024} KiB, LOD0 {tris} tris)")
