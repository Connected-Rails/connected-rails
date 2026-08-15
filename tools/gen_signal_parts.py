"""Procedural signal part models for the example mod.

Writes three glTF files below mods/example/assets/ — created entirely from
scratch in this script, no third-party assets, so the files carry the
project's licence:

  sig_mast.gltf        mast with base plate; mount point ``mp_schirm``
  sig_schirm_ks.gltf   Ks screen with lamp nodes lamp_red/green/yellow/zs1
                       and the mount point ``mp_top`` for a Zs3
  sig_zs3.gltf         Zs3 speed indicator board with the digit node ``zs3_4``

Conventions per MODS.md: origin at the part's attachment (the mast at its foot
on rail-top height), +Y up, the face towards the approaching driver looks
along +Z. Mount points are empty nodes named ``mp_*``; lamps are meshes the
app switches by visibility, so a lit lens sits proud of a dark housing that
stays visible.

Run: python tools/gen_signal_parts.py
"""

import base64
import json
import math
import struct
from pathlib import Path

ASSETS = Path(__file__).resolve().parents[1] / "mods" / "example" / "assets"

COPYRIGHT = (
    "TrainSim-DE project. Procedural model built from scratch in "
    "tools/gen_signal_parts.py, no third-party assets; licensed like the project."
)


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

    def box(self, mn, mx):
        x0, y0, z0 = mn
        x1, y1, z1 = mx
        self.quad((x1, y0, z0), (x1, y1, z0), (x1, y1, z1), (x1, y0, z1))  # +X
        self.quad((x0, y0, z1), (x0, y1, z1), (x0, y1, z0), (x0, y0, z0))  # -X
        self.quad((x0, y1, z0), (x0, y1, z1), (x1, y1, z1), (x1, y1, z0))  # +Y
        self.quad((x0, y0, z0), (x1, y0, z0), (x1, y0, z1), (x0, y0, z1))  # -Y
        self.quad((x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1))  # +Z
        self.quad((x1, y0, z0), (x0, y0, z0), (x0, y1, z0), (x1, y1, z0))  # -Z


def write_gltf(filename, scene_name, materials, mesh_specs, nodes):
    """materials: [(name, pbr dict, emissive or None)]; mesh_specs:
    [(mesh name, {material name: Prim})]; nodes reference meshes by name."""
    mat_index = {name: i for i, (name, _, _) in enumerate(materials)}
    blob = bytearray()
    buffer_views, accessors, meshes = [], [], []

    def accessor(data, comp_type, count, type_, target, minmax=None):
        while len(blob) % 4:
            blob.append(0)
        buffer_views.append(
            {"buffer": 0, "byteOffset": len(blob), "byteLength": len(data), "target": target}
        )
        blob.extend(data)
        acc = {
            "bufferView": len(buffer_views) - 1,
            "componentType": comp_type,
            "count": count,
            "type": type_,
        }
        if minmax:
            acc["min"], acc["max"] = minmax
        accessors.append(acc)
        return len(accessors) - 1

    mesh_index = {}
    for mesh_name, prims_by_mat in mesh_specs:
        gl_prims = []
        for mat_name, prim in prims_by_mat.items():
            if not prim.pos:
                continue
            flat_pos = [c for v in prim.pos for c in v]
            flat_nrm = [c for v in prim.nrm for c in v]
            mins = [min(p[i] for p in prim.pos) for i in range(3)]
            maxs = [max(p[i] for p in prim.pos) for i in range(3)]
            gl_prims.append(
                {
                    "attributes": {
                        "POSITION": accessor(
                            struct.pack(f"<{len(flat_pos)}f", *flat_pos),
                            5126, len(prim.pos), "VEC3", 34962, (mins, maxs),
                        ),
                        "NORMAL": accessor(
                            struct.pack(f"<{len(flat_nrm)}f", *flat_nrm),
                            5126, len(prim.nrm), "VEC3", 34962,
                        ),
                    },
                    "indices": accessor(
                        struct.pack(f"<{len(prim.idx)}I", *prim.idx),
                        5125, len(prim.idx), "SCALAR", 34963,
                    ),
                    "material": mat_index[mat_name],
                }
            )
        meshes.append({"name": mesh_name, "primitives": gl_prims})
        mesh_index[mesh_name] = len(meshes) - 1

    gl_nodes = []
    for node in nodes:
        out = dict(node)
        if "mesh" in out:
            out["mesh"] = mesh_index[out["mesh"]]
        gl_nodes.append(out)

    gl_materials = []
    for name, pbr, emissive in materials:
        mat = {"name": name, "pbrMetallicRoughness": pbr}
        if emissive:
            mat["emissiveFactor"] = emissive
        gl_materials.append(mat)

    gltf = {
        "asset": {
            "version": "2.0",
            "generator": "tools/gen_signal_parts.py (TrainSim-DE example mod)",
            "copyright": COPYRIGHT,
        },
        "scene": 0,
        "scenes": [{"name": scene_name, "nodes": list(range(len(gl_nodes)))}],
        "nodes": gl_nodes,
        "meshes": meshes,
        "materials": gl_materials,
        "accessors": accessors,
        "bufferViews": buffer_views,
        "buffers": [
            {
                "byteLength": len(blob),
                "uri": "data:application/octet-stream;base64,"
                + base64.b64encode(bytes(blob)).decode(),
            }
        ],
    }
    out = ASSETS / filename
    out.write_text(json.dumps(gltf, indent=1) + "\n", encoding="utf-8", newline="\n")
    print(f"wrote {out} ({out.stat().st_size // 1024} KiB)")


GREY = ("grey", dict(baseColorFactor=[0.42, 0.44, 0.46, 1.0], metallicFactor=0.3, roughnessFactor=0.6), None)
BLACK = ("black", dict(baseColorFactor=[0.03, 0.03, 0.035, 1.0], metallicFactor=0.0, roughnessFactor=0.8), None)
WHITE = ("white", dict(baseColorFactor=[0.85, 0.86, 0.85, 1.0], metallicFactor=0.0, roughnessFactor=0.6), None)
# Lit lenses: emissiveFactor makes them glow without any light of their own.
LENS_RED = ("lens_red", dict(baseColorFactor=[0.8, 0.03, 0.03, 1.0], metallicFactor=0.0, roughnessFactor=0.3), [1.0, 0.05, 0.05])
LENS_GREEN = ("lens_green", dict(baseColorFactor=[0.05, 0.7, 0.2, 1.0], metallicFactor=0.0, roughnessFactor=0.3), [0.1, 1.0, 0.3])
LENS_YELLOW = ("lens_yellow", dict(baseColorFactor=[0.85, 0.6, 0.05, 1.0], metallicFactor=0.0, roughnessFactor=0.3), [1.0, 0.75, 0.1])
LENS_WHITE = ("lens_white", dict(baseColorFactor=[0.9, 0.9, 0.85, 1.0], metallicFactor=0.0, roughnessFactor=0.3), [1.0, 1.0, 0.9])

MAST_H = 3.8
SCHIRM_Y = 3.0  # mp_schirm height on the mast


def build_mast():
    mast = Prim()
    mast.box((-0.06, 0.0, -0.06), (0.06, MAST_H, 0.06))
    mast.box((-0.16, 0.0, -0.16), (0.16, 0.06, 0.16))  # base plate
    write_gltf(
        "sig_mast.gltf",
        "sig_mast",
        [GREY],
        [("mast", {"grey": mast})],
        [
            {"name": "mast", "mesh": "mast"},
            # The screen hangs in front of the mast, facing the driver (+Z).
            {"name": "mp_schirm", "translation": [0.0, SCHIRM_Y, 0.08]},
        ],
    )


# Lamp positions on the Ks screen (x, y): main lamp centre, green above,
# yellow lower left, the two white Zs1 lamps at the bottom.
LAMPS = {
    "lamp_red": ("lens_red", [(0.0, 0.10)]),
    "lamp_green": ("lens_green", [(0.0, 0.34)]),
    "lamp_yellow": ("lens_yellow", [(-0.20, -0.18)]),
    "lamp_zs1": ("lens_white", [(-0.14, -0.40), (0.14, -0.40)]),
}


def build_schirm():
    board = Prim()
    board.box((-0.36, -0.55, 0.0), (0.36, 0.55, 0.06))
    # Dark housings that stay visible while the lens itself is switched off.
    for _, (_, positions) in LAMPS.items():
        for x, y in positions:
            board.box((x - 0.075, y - 0.075, 0.06), (x + 0.075, y + 0.075, 0.085))
    mesh_specs = [("schirm", {"black": board})]
    nodes = [{"name": "schirm", "mesh": "schirm"}]
    for name, (lens, positions) in LAMPS.items():
        prim = Prim()
        for x, y in positions:
            prim.box((x - 0.055, y - 0.055, 0.085), (x + 0.055, y + 0.055, 0.105))
        mesh_specs.append((name, {lens: prim}))
        nodes.append({"name": name, "mesh": name})
    nodes.append({"name": "mp_top", "translation": [0.0, 0.65, 0.0]})
    write_gltf(
        "sig_schirm_ks.gltf",
        "sig_schirm_ks",
        [BLACK, LENS_RED, LENS_GREEN, LENS_YELLOW, LENS_WHITE],
        mesh_specs,
        nodes,
    )


def build_zs3():
    board = Prim()
    board.box((-0.30, 0.0, -0.02), (0.30, 0.72, 0.03))
    # The digit 4 out of three bars, lit white — one node the lamp image toggles.
    digit = Prim()
    digit.box((-0.13, 0.38, 0.03), (-0.08, 0.60, 0.05))  # upper left stroke
    digit.box((-0.13, 0.33, 0.03), (0.13, 0.38, 0.05))   # cross bar
    digit.box((0.08, 0.12, 0.03), (0.13, 0.60, 0.05))    # right stroke
    write_gltf(
        "sig_zs3.gltf",
        "sig_zs3",
        [BLACK, LENS_WHITE],
        [("board", {"black": board}), ("zs3_4", {"lens_white": digit})],
        [
            {"name": "board", "mesh": "board"},
            {"name": "zs3_4", "mesh": "zs3_4"},
        ],
    )


def check():
    # Outward normals on a convex solid — the writer relies on CCW winding.
    prim = Prim()
    prim.box((0, 0, 0), (1, 1, 1))
    for t in range(0, len(prim.idx), 3):
        a, b, c = (prim.pos[prim.idx[t + k]] for k in range(3))
        centroid = tuple((a[i] + b[i] + c[i]) / 3 - 0.5 for i in range(3))
        assert dot(prim.nrm[prim.idx[t]], centroid) > 0, "inward-facing triangle"


if __name__ == "__main__":
    check()
    build_mast()
    build_schirm()
    build_zs3()
