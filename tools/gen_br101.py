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
    """One glTF primitive: flat-shaded triangles with per-face normals.

    `textured=True` adds TEXCOORD_0 — needed only where a texture is mapped
    (the display screens); everything else stays untextured.
    """

    def __init__(self, textured=False):
        self.pos, self.nrm, self.idx = [], [], []
        self.uv = [] if textured else None

    def tri(self, a, b, c, uvs=None):
        n = normalize(cross(sub(b, a), sub(c, a)))
        base = len(self.pos)
        self.pos += [a, b, c]
        self.nrm += [n, n, n]
        if self.uv is not None:
            self.uv += list(uvs) if uvs else [(0.0, 0.0)] * 3
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


def build_cab_interior():
    """Cab 1 interior (the -Z end): floor, walls, desk, seat, instrument panel.

    Only what the driver sees from the eye point — the body loft is culled from
    inside, so the windscreen and side windows act as openings by themselves.
    """
    prims = {name: Prim() for name in ("roof", "frame", "wheel", "light")}
    wall, frame, dark, light = prims["roof"], prims["frame"], prims["wheel"], prims["light"]

    wall.box((-1.35, 1.28, -8.0), (1.35, 1.32, -5.95))     # floor
    wall.box((-1.35, 1.32, -6.0), (1.35, 3.3, -5.95))      # back wall
    wall.box((-1.35, 3.3, -8.0), (1.35, 3.36, -5.95))      # ceiling
    wall.box((-1.15, 1.32, -8.3), (1.15, 2.1, -8.2))       # front wall below screen
    wall.box((-1.15, 3.3, -8.45), (1.15, 3.36, -8.0))      # screen header
    for x_sign in (1.0, -1.0):                              # side walls, window cut-out
        x0, x1 = sorted((x_sign * 1.30, x_sign * 1.35))
        wall.box((x0, 1.32, -8.0), (x1, 3.3, -6.8))
        wall.box((x0, 1.32, -6.8), (x1, 2.3, -6.0))
    wall.box((-1.1, 1.9, -7.6), (1.1, 2.15, -7.0))         # desk top
    wall.box((-0.3, 1.32, -7.5), (0.3, 1.9, -7.1))         # desk pedestal
    dark.box((-0.75, 1.7, -6.4), (-0.35, 1.85, -6.0))      # seat
    dark.box((-0.75, 1.85, -6.1), (-0.35, 2.4, -6.0))      # backrest

    frame.box((-0.35, 2.15, -7.62), (0.35, 2.55, -7.56))   # instrument panel
    light.box((-0.26, 2.30, -7.559), (-0.14, 2.46, -7.554))  # speedometer dial
    dark.box((0.06, 2.34, -7.559), (0.14, 2.42, -7.554))   # 1000 Hz lamp housing
    # Target-distance column: a dark slot the bright bar node slides up in.
    dark.box((-0.025, 2.295, -7.559), (0.025, 2.465, -7.554))
    # Window of the four-digit distance counter above the lamp housing.
    dark.box((0.005, 2.468, -7.559), (0.215, 2.525, -7.554))
    # Housings of the three cab screens; the faces themselves are separate
    # UV-mapped quad nodes (`screen_mfa`, `screen_brake`, `screen_ebula`).
    dark.box((0.29, 2.17, -7.60), (0.71, 2.44, -7.578))
    dark.box((-0.96, 2.19, -7.60), (-0.69, 2.39, -7.578))
    dark.box((-0.68, 2.19, -7.60), (-0.38, 2.41, -7.578))
    return prims


def build_lever(height, knob):
    """Upright control lever; the node's base rotation leans it to its rest."""
    p = Prim()
    p.box((-0.025, 0.0, -0.025), (0.025, height, 0.025))
    return {"frame": p, "wheel": _knob(height, knob)}


def _knob(height, size):
    p = Prim()
    p.box((-size, height, -size), (size, height + 2 * size, size))
    return p


def build_switch():
    """Small toggle switch for the preparation row."""
    p = Prim()
    p.box((-0.012, 0.0, -0.012), (0.012, 0.07, 0.012))
    return {"wheel": p}


def build_button():
    """Push button (Sifa); pressed by translating the node downwards."""
    p = Prim()
    p.box((-0.04, 0.0, -0.04), (0.04, 0.035, 0.04))
    return {"red": p}


def build_needle(mat="frame", half_w=0.006, length=0.052):
    """Instrument needle pointing +Y from its pivot, proud of the dial.

    The second (target) needle is narrower and shorter than the default so the
    two never share a coplanar face when they point the same way.
    """
    p = Prim()
    p.box((-half_w, 0.0, 0.0), (half_w, length, 0.004))
    return {mat: p}


def oblique_box(prim, origin, axes, extents):
    """Box spanned by three orthonormal axes; extents = ((u0,u1),(v0,v1),(w0,w1))."""
    u, v, w = axes
    (u0, u1), (v0, v1), (w0, w1) = extents

    def pt(a, b, c):
        return tuple(origin[i] + a * u[i] + b * v[i] + c * w[i] for i in range(3))

    def neg(d):
        return (-d[0], -d[1], -d[2])

    faces = [
        ((pt(u1, v0, w0), pt(u1, v1, w0), pt(u1, v1, w1), pt(u1, v0, w1)), u),
        ((pt(u0, v0, w0), pt(u0, v1, w0), pt(u0, v1, w1), pt(u0, v0, w1)), neg(u)),
        ((pt(u0, v1, w0), pt(u1, v1, w0), pt(u1, v1, w1), pt(u0, v1, w1)), v),
        ((pt(u0, v0, w0), pt(u1, v0, w0), pt(u1, v0, w1), pt(u0, v0, w1)), neg(v)),
        ((pt(u0, v0, w1), pt(u1, v0, w1), pt(u1, v1, w1), pt(u0, v1, w1)), w),
        ((pt(u0, v0, w0), pt(u1, v0, w0), pt(u1, v1, w0), pt(u0, v1, w0)), neg(w)),
    ]
    for corners, outward in faces:
        prim.oriented_quad(*corners, outward=outward)


def build_wiper():
    """Wiper arm and blade lying on the raked -Z windscreen, pointing up along
    the glass from the pivot at the node origin. The runtime rotates the node
    about the glass normal, so the blade stays on the pane through the sweep.
    """
    p = Prim()
    # Frame of the -Z front face: u across, v up the glass, w outward normal.
    u = (1.0, 0.0, 0.0)
    v = normalize((0.0, 1.0, RAKE))
    w = normalize((0.0, RAKE, -1.0))
    # Arm above the blade; the glass quad sits 1 cm proud of the face plane.
    oblique_box(p, (0.0, 0.0, 0.0), (u, v, w), ((-0.012, 0.012), (-0.03, 0.58), (0.020, 0.034)))
    oblique_box(p, (0.0, 0.0, 0.0), (u, v, w), ((-0.016, 0.016), (0.17, 0.62), (0.012, 0.020)))
    return {"frame": p}


def build_gauge_bar():
    """Bright sliding indicator of the target-distance slot; the node origin is
    the bottom of the slot and the runtime translates it upward."""
    p = Prim()
    p.box((-0.015, 0.0, 0.0), (0.015, 0.02, 0.004))
    return {"light": p}


def build_screen(width, height):
    """Cab display face: a single +Z quad whose UVs map the render texture —
    u runs left→right (+X), v top→bottom (image row 0 is the top). Without
    TEXCOORD_0 the runtime's emissive texture would not apply and the screen
    would glow plain white. The dark housing behind it belongs to the interior
    mesh, which needs no UVs."""
    p = Prim(textured=True)
    w, h = width / 2, height / 2
    a, b, c, d = (-w, -h, 0.0), (w, -h, 0.0), (w, h, 0.0), (-w, h, 0.0)
    p.tri(a, b, c, [(0.0, 1.0), (1.0, 1.0), (1.0, 0.0)])
    p.tri(a, c, d, [(0.0, 1.0), (1.0, 0.0), (0.0, 0.0)])
    return {"glass": p}


# Standard 7-segment table: a top, b top-right, c bottom-right, d bottom,
# e bottom-left, f top-left, g middle.
SEVEN_SEG = [
    "abcdef", "bc", "abdeg", "abcdg", "bcfg",
    "acdfg", "acdefg", "abc", "abcdefg", "abcdfg",
]


def build_digit(digit):
    """7-segment glyph for the distance counter, ~0.025 m tall.

    The verticals stop short of the horizontals — no two segments overlap, so
    no coplanar faces, and the gaps read as a real segmented display.
    """
    w, h, t, depth = 0.016, 0.025, 0.004, 0.003
    x0, x1, mid = -w / 2, w / 2, h / 2
    seg = {
        "a": ((x0, h - t), (x1, h)),
        "d": ((x0, 0.0), (x1, t)),
        "g": ((x0, mid - t / 2), (x1, mid + t / 2)),
        "f": ((x0, mid + t / 2), (x0 + t, h - t)),
        "b": ((x1 - t, mid + t / 2), (x1, h - t)),
        "e": ((x0, t), (x0 + t, mid - t / 2)),
        "c": ((x1 - t, t), (x1, mid - t / 2)),
    }
    p = Prim()
    for s in SEVEN_SEG[digit]:
        (sx0, sy0), (sx1, sy1) = seg[s]
        p.box((sx0, sy0, 0.0), (sx1, sy1, depth))
    return {"light": p}


def build_lamp_cap():
    """Light cap of an indicator lamp; shown/hidden by the lamp function."""
    p = Prim()
    p.box((-0.018, -0.018, 0.0), (0.018, 0.018, 0.006))
    return {"light": p}


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
            attributes = {
                "POSITION": accessor(struct.pack(f"<{len(flat_pos)}f", *flat_pos),
                                     5126, len(prim.pos), "VEC3", 34962, (mins, maxs)),
                "NORMAL": accessor(struct.pack(f"<{len(flat_nrm)}f", *flat_nrm),
                                   5126, len(prim.nrm), "VEC3", 34962),
            }
            if prim.uv is not None:
                flat_uv = [c for v in prim.uv for c in v]
                attributes["TEXCOORD_0"] = accessor(
                    struct.pack(f"<{len(flat_uv)}f", *flat_uv),
                    5126, len(prim.uv), "VEC2", 34962)
            gl_prims.append({
                "attributes": attributes,
                "indices": accessor(struct.pack(f"<{len(prim.idx)}I", *prim.idx),
                                    5125, len(prim.idx), "SCALAR", 34963),
                "material": MAT[mat_name],
            })
        meshes.append({"name": name, "primitives": gl_prims})
        return len(meshes) - 1

    lod0 = add_mesh("br101_LOD0", build_lod0())
    lod1 = add_mesh("br101_LOD1", build_lod1())
    pant = add_mesh("pantograph", build_pantograph())
    cab = add_mesh("cab_interior", build_cab_interior())
    lever = add_mesh("lever", build_lever(0.24, 0.045))
    lever_small = add_mesh("lever_small", build_lever(0.14, 0.03))
    switch = add_mesh("switch", build_switch())
    button = add_mesh("button", build_button())
    needle = add_mesh("needle", build_needle())
    needle_red = add_mesh("needle_red", build_needle("red", 0.005, 0.046))
    lamp_cap = add_mesh("lamp_cap", build_lamp_cap())
    wiper = add_mesh("wiper", build_wiper())
    dist_bar = add_mesh("distance_bar", build_gauge_bar())
    screen_large = add_mesh("screen_large", build_screen(0.40, 0.25))
    screen_small = add_mesh("screen_small", build_screen(0.25, 0.18))
    screen_medium = add_mesh("screen_medium", build_screen(0.28, 0.20))
    digits = [add_mesh(f"digit_{d}", build_digit(d)) for d in range(10)]

    def lean_x(deg):
        """Node base rotation about +X — the rest pose the motion adds to."""
        half = math.radians(deg) / 2
        return [math.sin(half), 0.0, 0.0, math.cos(half)]

    pant_extras = {"ts_function": "pantograph", "ts_motion": "rotate", "ts_axis": "1 0 0", "ts_amount": 45}
    gauge_extras = {"ts_function": "gauge:speed", "ts_motion": "rotate", "ts_axis": "0 0 1", "ts_amount": -240}
    nodes = [
        {"name": "body_LOD0", "mesh": lod0},
        {"name": "body_LOD1", "mesh": lod1},
        {"name": "pant_front", "mesh": pant, "translation": [0.0, Y_ROOF + 0.02, -5.7], "extras": pant_extras},
        {"name": "pant_rear", "mesh": pant, "translation": [0.0, Y_ROOF + 0.02, 5.7],
         "rotation": [0.0, 1.0, 0.0, 0.0], "extras": pant_extras},
        # Cab 1 (the -Z end): interior shell plus the interactive controls the
        # vehicle file binds in its `cab:` section. Levers rest leaned back so
        # the motion sweeps them through upright at mid-travel.
        {"name": "cab_LOD0", "mesh": cab},
        {"name": "cab_throttle", "mesh": lever,
         "translation": [-0.85, 2.15, -7.3], "rotation": lean_x(-30)},
        {"name": "cab_brake", "mesh": lever,
         "translation": [-0.15, 2.15, -7.3], "rotation": lean_x(-30)},
        {"name": "cab_reverser", "mesh": lever_small,
         "translation": [-0.55, 2.15, -7.15], "rotation": lean_x(-20)},
        {"name": "cab_sifa", "mesh": button, "translation": [-0.55, 2.15, -6.95]},
        {"name": "cab_battery", "mesh": switch,
         "translation": [0.45, 2.15, -7.3], "rotation": lean_x(-25)},
        {"name": "cab_pantograph", "mesh": switch,
         "translation": [0.6, 2.15, -7.3], "rotation": lean_x(-25)},
        {"name": "cab_main_switch", "mesh": switch,
         "translation": [0.75, 2.15, -7.3], "rotation": lean_x(-25)},
        {"name": "cab_compressor", "mesh": switch,
         "translation": [0.9, 2.15, -7.3], "rotation": lean_x(-25)},
        {"name": "gauge_speed", "mesh": needle,
         "translation": [-0.2, 2.38, -7.552], "extras": gauge_extras},
        # Second needle on the same dial: AFB/LZB target speed. Sits 0.0015 m
        # behind gauge_speed and 0.0005 m proud of the dial — no coplanar faces.
        {"name": "gauge_v_soll", "mesh": needle_red,
         "translation": [-0.2, 2.38, -7.5535]},
        {"name": "lamp_1000hz", "mesh": lamp_cap, "translation": [0.1, 2.38, -7.553]},
        # Wiper pivot at the bottom of the -Z windscreen, on the front face
        # plane z = -(Z_NOSE - RAKE*(y - Y_FLOOR)); the vehicle file rotates it
        # about the glass normal.
        {"name": "wiper_left", "mesh": wiper,
         "translation": [-0.45, 2.2, -(Z_NOSE - RAKE * (2.2 - Y_FLOOR))]},
        {"name": "cab_wipers", "mesh": switch,
         "translation": [0.25, 2.15, -7.3], "rotation": lean_x(-25)},
        # Bottom of the target-distance slot; the runtime translates the bar up.
        {"name": "gauge_distance_bar", "mesh": dist_bar,
         "translation": [0.0, 2.30, -7.5535]},
        # Cab displays: the MFA screen on the right of the desk with its two
        # softkeys in front of it, the small brake screen on the far left, and
        # the EBuLa screen between the brake screen and the instrument panel.
        {"name": "screen_mfa", "mesh": screen_large,
         "translation": [0.5, 2.305, -7.575]},
        {"name": "screen_brake", "mesh": screen_small,
         "translation": [-0.825, 2.29, -7.575]},
        {"name": "screen_ebula", "mesh": screen_medium,
         "translation": [-0.53, 2.30, -7.575]},
        {"name": "cab_disp_1", "mesh": button, "translation": [0.42, 2.15, -7.45]},
        {"name": "cab_disp_2", "mesh": button, "translation": [0.58, 2.15, -7.45]},
        # EBuLa softkeys; the HTML page sees them as onButton indices 3 and 4.
        {"name": "cab_disp_3", "mesh": button, "translation": [-0.61, 2.15, -7.45]},
        {"name": "cab_disp_4", "mesh": button, "translation": [-0.45, 2.15, -7.45]},
    ]

    # Distance counter: four digit places (place 0 = ones, rightmost = +X seen
    # by the driver looking towards -Z). Each place is a parent node whose ten
    # children "0"-"9" carry one glyph each; the app shows the matching child.
    # Children are indices into the nodes array and stay out of the scene roots.
    roots = list(range(len(nodes)))
    for place in range(4):
        parent = {
            "name": f"digit_dist_{place}",
            "translation": [0.185 - 0.05 * place, 2.475, -7.5535],
            "children": [],
        }
        nodes.append(parent)
        roots.append(len(nodes) - 1)
        for d in range(10):
            parent["children"].append(len(nodes))
            nodes.append({"name": str(d), "mesh": digits[d]})

    return {
        "asset": {
            "version": "2.0",
            "generator": "tools/gen_br101.py (Connected Rails example mod)",
            "copyright": "Connected Rails project. Procedural model built from scratch in tools/gen_br101.py, no third-party assets; licensed like the project.",
        },
        "scene": 0,
        "scenes": [{"name": "br101", "nodes": roots}],
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
