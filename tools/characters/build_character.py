#!/usr/bin/env python3
"""Turns a MakeHuman2 glTF export into a game-ready character.

Input is the GLB MakeHuman2 writes — metres, Y up, face towards +Z, one
skinned mesh per asset, PNG textures, one animation per clip — plus the
side-car `<name>.meta.json` that says which node is which kind of asset and
which garments get a colour shift.

Pipeline:

1. Load every skinned primitive with its base-colour texture and class it as
   skin, clothes, shoes, hat, hair, eyes, eyebrows or eyelashes.
2. Bake the rest pose and every clip with linear blend skinning. The rest
   pose gives the height and the shoe test; the clips give the ground fix:
   the exporter scales the root joint's keyframes wrongly and leaves every
   clip about 0.7 m in the air, so the root translation channel of each clip
   is shifted by the constant that puts the lowest vertex of the whole clip
   on y = 0.
3. Turn the character round: the walker yaws the model about Y and expects
   the face towards −Z. The 180° turn is baked into positions, normals,
   inverse bind matrices, the skeleton root and its keyframes. The
   translation the exporter left on the mesh node is folded into the
   skeleton root as well, because glTF ignores a skinned mesh's own node
   transform.
4. Pack the textures: base colours are tinted (HSV shift from the meta
   file), shrunk to a per-class edge and packed into two atlases — JPEG for
   the opaque meshes (`body`), PNG with alpha for hair, eyes, eyebrows and
   eyelashes (`cutout`). Normal, occlusion and roughness maps are dropped.
5. Merge the meshes into those two groups and build four levels of detail
   with meshoptimizer, splitting each triangle budget between the groups.
6. Write one GLB: `character` → `Root` (the joints) and `char_LOD0..3`, two
   materials, one 4-byte aligned buffer. Optionally a stats file and a
   self-check that re-reads the file.

    build_character.py in.glb out.glb --meta in.meta.json --stats out.json --check
"""

import argparse
import ctypes
import io
import json
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from types import SimpleNamespace

import meshoptimizer
import numpy as np
from PIL import Image

import glb

GENERATOR = "connected-rails build_character.py"
JOINT_COUNT = 53  # MakeHuman "game_engine" rig
BODY, CUTOUT = "body", "cutout"
LOD_COUNT = 4

# Longest edge a texture of each class may keep in the atlas, gutter included.
CLASS_MAX_EDGE = {
    "skin": 1024,
    "clothes": 768,
    "shoes": 384,
    "hat": 384,
    "hair": 512,
    "eyes": 256,
    "eyebrows": 128,
    "eyelashes": 128,
}
GUTTER = 2  # texels of edge-extended padding on every side of an atlas region
ATLAS_SHRINK = 0.92  # scale applied to all regions when they do not fit
UV_OUTSIDE_WARN = 0.01  # share of UVs outside [0, 1] worth a warning
SHOES_BELOW = 0.35  # a garment whose top stays below this share of the height is footwear

CUTOUT_MIN_SHARE = 0.08  # of every triangle budget, so the hair outlives the body
SLOPPY_ABOVE = 1.15  # retry with the sloppy simplifier when over target by this factor
PRUNE_FROM_LOD = 2
PRUNE_OPTION = getattr(meshoptimizer, "SIMPLIFY_PRUNE", 0)  # older wrappers lack the flag
SIMPLIFY_WEIGHTS = np.array([0.5, 0.5, 0.5, 1.0, 1.0], dtype=np.float32)  # normal xyz, uv

TURN = (np.zeros(3), np.array([0.0, 1.0, 0.0, 0.0]), 1.0)  # 180° about Y as TRS
TURN_MATRIX = np.diag([-1.0, 1.0, -1.0, 1.0])

MATERIALS = {
    BODY: {
        "name": BODY,
        "pbrMetallicRoughness": {"metallicFactor": 0.0, "roughnessFactor": 0.85},
        "alphaMode": "OPAQUE",
        "doubleSided": False,
    },
    CUTOUT: {
        "name": CUTOUT,
        "pbrMetallicRoughness": {"metallicFactor": 0.0, "roughnessFactor": 0.9},
        "alphaMode": "MASK",
        "alphaCutoff": 0.5,
        "doubleSided": True,
    },
}
SAMPLER = {"magFilter": 9729, "minFilter": 9987, "wrapS": 33071, "wrapT": 33071}

WARNINGS = []


def declare_simplify_with_attributes():
    """Declare the C signature of meshopt_simplifyWithAttributes.

    The 0.2.30 wrapper calls it without `argtypes`, so ctypes cannot pass the
    `target_error` float. The order matches the wrapper's call.
    """
    function = meshoptimizer.simplifier.lib.meshopt_simplifyWithAttributes
    if function.argtypes is None:
        size_t, c_uint, c_float = ctypes.c_size_t, ctypes.c_uint, ctypes.c_float
        floats, uints = ctypes.POINTER(c_float), ctypes.POINTER(c_uint)
        function.argtypes = [
            uints,  # destination
            uints,  # indices
            size_t,  # index_count
            floats,  # vertex_positions
            size_t,  # vertex_count
            size_t,  # vertex_positions_stride
            floats,  # vertex_attributes
            size_t,  # vertex_attributes_stride
            floats,  # attribute_weights
            size_t,  # attribute_count
            ctypes.POINTER(ctypes.c_ubyte),  # vertex_lock
            size_t,  # target_index_count
            c_float,  # target_error
            c_uint,  # options
            floats,  # result_error
        ]
        function.restype = size_t


declare_simplify_with_attributes()


class BuildError(Exception):
    """Input that cannot be turned into a character."""


def warn(message):
    """Print a warning and remember it for the summary."""
    WARNINGS.append(message)
    print(f"warning: {message}", file=sys.stderr)


# ---------------------------------------------------------------------------
# Source data
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class TextureRef:
    """What a mesh is painted with: an embedded image times a colour, optionally tinted."""

    image: int | None
    colour: tuple
    tint: tuple | None  # (hue degrees, saturation factor, value factor)


@dataclass
class SourceMesh:
    """One primitive of the export with its vertex data."""

    name: str
    kind: str | None
    alpha_mode: str
    positions: np.ndarray
    normals: np.ndarray
    uvs: np.ndarray
    joints: np.ndarray
    weights: np.ndarray
    indices: np.ndarray
    texture: TextureRef

    @property
    def group(self):
        return BODY if self.alpha_mode == "OPAQUE" else CUTOUT


class Skeleton:
    """The one skin of a file: joint nodes, their root and the inverse bind matrices."""

    def __init__(self, gltf, binary):
        skins = gltf.get("skins", [])
        if len(skins) != 1:
            raise BuildError(f"expected one skin, found {len(skins)}")
        skin = skins[0]
        self.gltf = gltf
        self.joints = list(skin["joints"])
        self.parents = glb.node_parents(gltf)
        joint_set = set(self.joints)
        roots = [j for j in self.joints if self.parents.get(j) not in joint_set]
        if len(roots) != 1:
            raise BuildError(f"skeleton has {len(roots)} root joints, expected one")
        self.root = roots[0]
        self.ibm = glb.ibm_to_matrices(glb.read_accessor(gltf, binary, skin["inverseBindMatrices"]))

    def joint_matrices(self, pose):
        """`world @ inverse_bind` per joint for the pose (overrides of `global_matrices`)."""
        world = glb.global_matrices(self.gltf, self.parents, pose)
        return world[self.joints] @ self.ibm


class Baker:
    """Linear blend skinning of parts (anything with positions, joints, weights) against a skeleton."""

    def __init__(self, skeleton, parts):
        self.skeleton = skeleton
        self.positions = np.concatenate([p.positions for p in parts]).astype(np.float64)
        self.joints = np.concatenate([p.joints for p in parts]).astype(np.int64)
        self.weights = np.concatenate([p.weights for p in parts]).astype(np.float64)
        bounds = np.cumsum([0] + [len(p.positions) for p in parts])
        self.slices = [slice(int(a), int(b)) for a, b in zip(bounds[:-1], bounds[1:])]

    def pose(self, overrides):
        """World positions (n, 3) of all vertices in the given pose."""
        return glb.skin_positions(self.skeleton.joint_matrices(overrides), self.positions, self.joints, self.weights)


def load_meshes(gltf, binary, meta):
    """Every skinned primitive of the export, in the exporter's frame."""
    assets = meta.get("assets", {})
    tints = meta.get("tints", {})
    base = meta.get("base_node", "base")
    meshes = []
    for node in gltf["nodes"]:
        if "mesh" not in node:
            continue
        name = node.get("name", f"mesh{node['mesh']}")
        if node.get("skin") != 0:
            warn(f"{name}: not bound to the skin, dropped")
            continue
        kind = "skin" if name == base else assets.get(name)
        tint = tints.get(name)
        if tint is not None:
            tint = (float(tint.get("hue", 0.0)), float(tint.get("sat", 1.0)), float(tint.get("val", 1.0)))
        for primitive in gltf["meshes"][node["mesh"]]["primitives"]:
            meshes.append(load_primitive(gltf, binary, primitive, name, kind, tint))
    if not meshes:
        raise BuildError("no skinned meshes in the input")
    return meshes


def load_primitive(gltf, binary, primitive, name, kind, tint):
    """One triangle-list primitive as a SourceMesh."""
    if primitive.get("mode", 4) != 4:
        raise BuildError(f"{name}: only triangle lists are supported")
    attributes = primitive["attributes"]
    missing = [a for a in ("POSITION", "NORMAL", "TEXCOORD_0", "JOINTS_0", "WEIGHTS_0") if a not in attributes]
    if missing:
        raise BuildError(f"{name}: missing vertex attributes {', '.join(missing)}")

    def read(attribute, normalised=False):
        data = glb.read_accessor(gltf, binary, attributes[attribute])
        if normalised and data.dtype != np.float32:
            data = data.astype(np.float32) / np.iinfo(data.dtype).max
        return data

    positions = read("POSITION").astype(np.float32)
    if "indices" in primitive:
        indices = glb.read_accessor(gltf, binary, primitive["indices"]).astype(np.uint32)
    else:
        indices = np.arange(len(positions), dtype=np.uint32)
    material = gltf["materials"][primitive["material"]] if "material" in primitive else {}
    pbr = material.get("pbrMetallicRoughness", {})
    image = None
    if "baseColorTexture" in pbr:
        image = gltf["textures"][pbr["baseColorTexture"]["index"]]["source"]
    texture = TextureRef(image, tuple(float(c) for c in pbr.get("baseColorFactor", [1.0, 1.0, 1.0, 1.0])), tint)
    return SourceMesh(
        name=name,
        kind=kind,
        alpha_mode=material.get("alphaMode", "OPAQUE"),
        positions=positions,
        normals=read("NORMAL").astype(np.float32),
        uvs=read("TEXCOORD_0", normalised=True).astype(np.float32),
        joints=read("JOINTS_0").astype(np.uint8),
        weights=read("WEIGHTS_0", normalised=True).astype(np.float32),
        indices=indices,
        texture=texture,
    )


def classify(meshes, baker, rest, ground, height):
    """Settle the texture class of every mesh from the rest pose.

    Garments whose top stays low are shoes; nodes the meta file does not
    list, or lists with a type the class table lacks, are guessed from their
    alpha mode. Fails when the eyes do not face +Z, the MakeHuman convention
    the turn relies on.
    """
    for mesh, part in zip(meshes, baker.slices):
        top = rest[part][:, 1].max() - ground
        if mesh.kind not in CLASS_MAX_EDGE:
            reason = "not in the meta file" if mesh.kind is None else f"asset type {mesh.kind!r} unknown"
            mesh.kind = "clothes" if mesh.alpha_mode == "OPAQUE" else "hair"
            warn(f"{mesh.name}: {reason}, treated as {mesh.kind}")
        if mesh.kind == "clothes" and top < SHOES_BELOW * height:
            mesh.kind = "shoes"
        if mesh.kind == "eyes" and rest[part][:, 2].mean() <= 0:
            raise BuildError(f"{mesh.name}: the eyes look along -Z, but the input has to face +Z")


def ground_shifts(gltf, binary, baker):
    """Per clip, how far the root must move so the lowest vertex of the whole clip sits on y = 0."""
    shifts = []
    for animation in gltf.get("animations", []):
        _, poses = glb.clip_poses(gltf, binary, animation)
        shifts.append(-min(baker.pose(pose)[:, 1].min() for pose in poses))
    return shifts


def turn_meshes(meshes):
    """Bake the 180° turn into positions and normals and tidy the vertex data."""
    for mesh in meshes:
        mesh.positions[:, 0] *= -1
        mesh.positions[:, 2] *= -1
        mesh.normals[:, 0] *= -1
        mesh.normals[:, 2] *= -1
        mesh.normals /= np.maximum(np.linalg.norm(mesh.normals, axis=1, keepdims=True), 1e-12)
        total = mesh.weights.sum(axis=1, keepdims=True)
        mesh.weights = np.where(total > 0, mesh.weights / np.maximum(total, 1e-12), [1.0, 0.0, 0.0, 0.0]).astype(
            np.float32
        )


def root_prefix(gltf, skeleton):
    """TRS to put in front of the root joint: the turn and its former ancestors' transforms."""
    ancestors = []
    parent = skeleton.parents.get(skeleton.root)
    while parent is not None:
        ancestors.append(parent)
        parent = skeleton.parents.get(parent)
    prefix = (np.zeros(3), glb.IDENTITY_QUATERNION, 1.0)
    for index in reversed(ancestors):
        prefix = glb.trs_compose(prefix, glb.node_trs(gltf["nodes"][index]))
    return glb.trs_compose(TURN, prefix)


# ---------------------------------------------------------------------------
# The chair pose
# ---------------------------------------------------------------------------

# MakeHuman's `sit01` sits on the floor with the legs stretched out; a passenger sits
# on a seat. The `sit` clip is therefore made here out of the rest pose: joints turned
# about world axes in the exporter's frame (+Z is the front there), in hierarchy
# order. The sign of a turn is not hard-coded — the joint axes of the rig are whatever
# MakeHuman made them — but picked by an objective on the joint positions it yields:
# the knee forward, the foot down, the hand forward, the hand towards the body.
CHAIR_POSE = [
    # joint, world axis, degrees, (joint whose position decides the sign, coordinate, wanted extreme)
    ("thigh_l", "x", 90.0, ("calf_l", "z", "max")),
    ("thigh_r", "x", 90.0, ("calf_r", "z", "max")),
    ("calf_l", "x", 85.0, ("foot_l", "y", "min")),
    ("calf_r", "x", 85.0, ("foot_r", "y", "min")),
    # Arms: a touch forward, well in from the A-pose and bent at the elbow — the hands
    # come to rest a few centimetres above the thighs, about 0.4 m apart.
    ("upperarm_l", "x", 5.0, ("hand_l", "z", "max")),
    ("upperarm_r", "x", 5.0, ("hand_r", "z", "max")),
    ("upperarm_l", "z", 40.0, ("hand_l", "|x|", "min")),
    ("upperarm_r", "z", 40.0, ("hand_r", "|x|", "min")),
    ("lowerarm_l", "x", 25.0, ("hand_l", "z", "max")),
    ("lowerarm_r", "x", 25.0, ("hand_r", "z", "max")),
]
SIT_CLIP = "sit"
WORLD_AXES = {"x": (1.0, 0.0, 0.0), "y": (0.0, 1.0, 0.0), "z": (0.0, 0.0, 1.0)}
# What the check expects of the seated pose, in metres above the floor / in front of the pelvis.
SEAT_HEIGHT = (0.35, 0.60)
KNEES_AHEAD = 0.25
HANDS_NEAR_SEAT = 0.35


def quat_from_axis_angle(axis, degrees):
    half = np.radians(degrees) / 2.0
    return np.array([*(np.asarray(axis, dtype=np.float64) * np.sin(half)), np.cos(half)])


def quat_conjugate(q):
    return np.array([-q[0], -q[1], -q[2], q[3]], dtype=np.float64)


def matrix_to_quat(matrix):
    """Rotation part of a column-vector matrix (unit scale) as (x, y, z, w)."""
    m = np.asarray(matrix, dtype=np.float64)[:3, :3]
    trace = np.trace(m)
    if trace > 0:
        s = np.sqrt(trace + 1.0) * 2
        q = [(m[2, 1] - m[1, 2]) / s, (m[0, 2] - m[2, 0]) / s, (m[1, 0] - m[0, 1]) / s, 0.25 * s]
    elif m[0, 0] > m[1, 1] and m[0, 0] > m[2, 2]:
        s = np.sqrt(1.0 + m[0, 0] - m[1, 1] - m[2, 2]) * 2
        q = [0.25 * s, (m[0, 1] + m[1, 0]) / s, (m[0, 2] + m[2, 0]) / s, (m[2, 1] - m[1, 2]) / s]
    elif m[1, 1] > m[2, 2]:
        s = np.sqrt(1.0 + m[1, 1] - m[0, 0] - m[2, 2]) * 2
        q = [(m[0, 1] + m[1, 0]) / s, 0.25 * s, (m[1, 2] + m[2, 1]) / s, (m[0, 2] - m[2, 0]) / s]
    else:
        s = np.sqrt(1.0 + m[2, 2] - m[0, 0] - m[1, 1]) * 2
        q = [(m[0, 2] + m[2, 0]) / s, (m[1, 2] + m[2, 1]) / s, 0.25 * s, (m[1, 0] - m[0, 1]) / s]
    q = np.array(q)
    return q / np.linalg.norm(q)


def append_accessor(gltf, binary, array, accessor_type, bounds=False):
    """Append float data to the document's buffer; returns `(binary, accessor index)`."""
    array = np.ascontiguousarray(array, dtype=np.float32)
    data = array.tobytes()
    pad = (-len(binary)) % 4
    offset = len(binary) + pad
    binary = binary + b"\0" * pad + data
    gltf["bufferViews"].append({"buffer": 0, "byteOffset": offset, "byteLength": len(data)})
    count = len(array) if accessor_type == "SCALAR" else array.shape[0]
    accessor = {"bufferView": len(gltf["bufferViews"]) - 1, "componentType": 5126, "count": int(count), "type": accessor_type}
    if bounds:
        flat = array.reshape(count, -1)
        accessor["min"] = [float(v) for v in flat.min(axis=0)]
        accessor["max"] = [float(v) for v in flat.max(axis=0)]
    gltf["accessors"].append(accessor)
    gltf["buffers"][0]["byteLength"] = len(binary)
    return binary, len(gltf["accessors"]) - 1


def chair_pose(gltf, skeleton):
    """Joint rotation overrides of the seated pose, or `None` when the rig lacks a joint."""
    nodes = gltf["nodes"]
    by_name = {nodes[j].get("name"): j for j in skeleton.joints}
    needed = {step[0] for step in CHAIR_POSE} | {step[3][0] for step in CHAIR_POSE}
    missing = sorted(needed - set(by_name))
    if missing:
        warn(f"chair pose: joints {missing} missing, MakeHuman's sit kept")
        return None
    overrides = {}
    for joint, axis, degrees, (probe, coordinate, extreme) in CHAIR_POSE:
        index = by_name[joint]
        world = glb.global_matrices(gltf, skeleton.parents, overrides)
        parent = skeleton.parents.get(index)
        above = matrix_to_quat(world[parent]) if parent is not None else np.array(glb.IDENTITY_QUATERNION, dtype=np.float64)
        local = overrides.get(index, {}).get("rotation", glb.node_trs(nodes[index])[1])
        candidates = []
        for sign in (1.0, -1.0):
            # A turn about a world axis through the joint: G' = R ⊗ G, so L' = G_p⁻¹ ⊗ R ⊗ G_p ⊗ L.
            turn = quat_from_axis_angle(WORLD_AXES[axis], sign * degrees)
            rotated = glb.quat_multiply(quat_conjugate(above), glb.quat_multiply(turn, glb.quat_multiply(above, local)))
            trial = {**overrides, index: {**overrides.get(index, {}), "rotation": rotated}}
            position = glb.global_matrices(gltf, skeleton.parents, trial)[by_name[probe]][:3, 3]
            value = position["xyz".index(coordinate.strip("|"))]
            if coordinate.startswith("|"):
                value = abs(value)
            candidates.append((value if extreme == "max" else -value, trial))
        overrides = max(candidates, key=lambda candidate: candidate[0])[1]
    return overrides


def synthesize_sit(gltf, binary, skeleton):
    """Replace (or add) the `sit` clip by the chair pose as a one-frame animation.

    Returns the binary with the clip's data appended; the document is edited in place.
    """
    overrides = chair_pose(gltf, skeleton)
    if overrides is None:
        return binary
    binary, times = append_accessor(gltf, binary, np.array([0.0]), "SCALAR", bounds=True)
    samplers, channels = [], []
    for joint in skeleton.joints:
        translation, rotation, _ = glb.node_trs(gltf["nodes"][joint])
        rotation = overrides.get(joint, {}).get("rotation", rotation)
        for path, value, kind in (("translation", translation, "VEC3"), ("rotation", rotation, "VEC4")):
            binary, output = append_accessor(gltf, binary, np.asarray(value)[None, :], kind)
            samplers.append({"input": times, "interpolation": "STEP", "output": output})
            channels.append({"sampler": len(samplers) - 1, "target": {"node": joint, "path": path}})
    clip = {"name": SIT_CLIP, "samplers": samplers, "channels": channels}
    animations = gltf.setdefault("animations", [])
    for number, animation in enumerate(animations):
        if animation.get("name") == SIT_CLIP:
            animations[number] = clip
            break
    else:
        animations.append(clip)
    return binary


def seated_geometry(gltf, binary, skeleton, baker):
    """Seat height, how far the knees are ahead and the hands' height of the `sit` clip [m], or `None`.

    Measured on the output frame (front towards −Z), above the clip's lowest vertex.
    """
    sit = next((a for a in gltf.get("animations", []) if a.get("name") == SIT_CLIP), None)
    if sit is None:
        return None
    by_name = {gltf["nodes"][j].get("name"): j for j in skeleton.joints}
    if not {"pelvis", "calf_l", "calf_r", "hand_l", "hand_r"} <= set(by_name):
        return None
    _, poses = glb.clip_poses(gltf, binary, sit)
    world = glb.global_matrices(gltf, skeleton.parents, poses[0])
    floor = baker.pose(poses[0])[:, 1].min()

    def at(name):
        return world[by_name[name]][:3, 3]

    pelvis = at("pelvis")
    knees = (at("calf_l") + at("calf_r")) / 2
    hands = (at("hand_l") + at("hand_r")) / 2
    return pelvis[1] - floor, pelvis[2] - knees[2], hands[1] - floor


# ---------------------------------------------------------------------------
# Textures
# ---------------------------------------------------------------------------


def rgb_to_hsv(rgb):
    """(..., 3) floats in [0, 1] → hue, saturation, value arrays."""
    r, g, b = rgb[..., 0], rgb[..., 1], rgb[..., 2]
    high = rgb.max(axis=-1)
    delta = high - rgb.min(axis=-1)
    safe = np.maximum(delta, 1e-12)
    hue = np.where(high == r, (g - b) / safe, np.where(high == g, 2.0 + (b - r) / safe, 4.0 + (r - g) / safe))
    hue = np.where(delta > 0, (hue / 6.0) % 1.0, 0.0)
    saturation = np.where(high > 0, delta / np.maximum(high, 1e-12), 0.0)
    return hue, saturation, high


def hsv_to_rgb(hue, saturation, value):
    """Inverse of `rgb_to_hsv`; returns (..., 3) floats in [0, 1]."""
    sector = np.floor(hue * 6.0)
    fraction = hue * 6.0 - sector
    sector = sector.astype(np.int64) % 6
    p = value * (1.0 - saturation)
    q = value * (1.0 - saturation * fraction)
    t = value * (1.0 - saturation * (1.0 - fraction))
    r = np.choose(sector, [value, q, p, p, t, value])
    g = np.choose(sector, [t, value, value, q, p, p])
    b = np.choose(sector, [p, p, t, value, value, q])
    return np.stack([r, g, b], axis=-1)


def apply_tint(pixels, hue, saturation, value):
    """HSV shift of RGBA pixels: `hue` in degrees is added, the others multiply. Alpha stays."""
    h, s, v = rgb_to_hsv(pixels[..., :3].astype(np.float32) / 255.0)
    h = (h + hue / 360.0) % 1.0
    s = np.clip(s * saturation, 0.0, 1.0)
    v = np.clip(v * value, 0.0, 1.0)
    out = pixels.copy()
    out[..., :3] = np.clip(np.round(hsv_to_rgb(h, s, v) * 255.0), 0, 255).astype(np.uint8)
    return out


def decode_texture(gltf, binary, ref):
    """RGBA pixels (h, w, 4) of a texture reference, colour factor and tint applied."""
    if ref.image is None:
        pixels = np.full((4, 4, 4), 255, dtype=np.uint8)
    else:
        image = Image.open(io.BytesIO(glb.read_image(gltf, binary, ref.image)))
        pixels = np.asarray(image.convert("RGBA"))
    if any(c != 1.0 for c in ref.colour):
        pixels = np.clip(np.round(pixels * np.array(ref.colour)), 0, 255).astype(np.uint8)
    if ref.tint is not None:
        pixels = apply_tint(pixels, *ref.tint)
    return pixels


def resize(pixels, width, height):
    """Lanczos downscale on premultiplied alpha, so transparent texels do not bleed their colour."""
    if (pixels.shape[1], pixels.shape[0]) == (width, height):
        return pixels
    image = Image.fromarray(pixels, "RGBA").convert("RGBa").resize((width, height), Image.LANCZOS)
    return np.asarray(image.convert("RGBA"))


def pack(sizes, width, height):
    """Guillotine packing of (w, h) rectangles into width × height.

    Largest area first, best short-side fit, split along the shorter leftover
    axis. Returns the (x, y) of every rectangle, or None when one does not fit.
    """
    free = [(0, 0, width, height)]
    placements = [None] * len(sizes)
    for index in sorted(range(len(sizes)), key=lambda i: -sizes[i][0] * sizes[i][1]):
        w, h = sizes[index]
        best = None
        for slot, (fx, fy, fw, fh) in enumerate(free):
            if w <= fw and h <= fh:
                score = min(fw - w, fh - h)
                if best is None or score < best[0]:
                    best = (score, slot)
        if best is None:
            return None
        fx, fy, fw, fh = free.pop(best[1])
        placements[index] = (fx, fy)
        if fw - w < fh - h:
            leftovers = [(fx + w, fy, fw - w, h), (fx, fy + h, fw, fh - h)]
        else:
            leftovers = [(fx + w, fy, fw - w, fh), (fx, fy + h, w, fh - h)]
        free.extend(rect for rect in leftovers if rect[2] > 0 and rect[3] > 0)
    return placements


@dataclass
class Region:
    """Where a texture's content sits in an atlas, in texels."""

    x: int
    y: int
    width: int
    height: int


def build_atlas(name, textures, size):
    """Pack `textures` ({ref: (pixels, max edge)}) into an atlas of `size`.

    Every texture is scaled down (never up) so its longer edge plus the
    gutter fits its class edge; when the set does not fit the atlas, all of
    them shrink together until it does. Returns the RGBA atlas and the
    regions per reference.
    """
    width, height = size
    refs = list(textures)
    base = []
    for ref in refs:
        pixels, edge = textures[ref]
        h, w = pixels.shape[:2]
        scale = min(1.0, (edge - 2 * GUTTER) / max(w, h))
        base.append((w * scale, h * scale))
    factor = 1.0
    while True:
        sizes = [(max(1, round(w * factor)), max(1, round(h * factor))) for w, h in base]
        placements = pack([(w + 2 * GUTTER, h + 2 * GUTTER) for w, h in sizes], width, height)
        if placements is not None:
            break
        factor *= ATLAS_SHRINK
        if factor < 0.2:
            raise BuildError(f"{name} atlas {width}x{height} cannot hold its {len(refs)} textures")
    if factor < 1.0:
        warn(f"{name} atlas {width}x{height} is too small for the class edges; textures scaled by {factor:.2f}")
    atlas = np.zeros((height, width, 4), dtype=np.uint8)
    covered = np.zeros((height, width), dtype=bool)
    regions = {}
    for ref, (w, h), (x, y) in zip(refs, sizes, placements):
        padded = np.pad(resize(textures[ref][0], w, h), ((GUTTER, GUTTER), (GUTTER, GUTTER), (0, 0)), mode="edge")
        atlas[y : y + h + 2 * GUTTER, x : x + w + 2 * GUTTER] = padded
        covered[y : y + h + 2 * GUTTER, x : x + w + 2 * GUTTER] = True
        regions[ref] = Region(x + GUTTER, y + GUTTER, w, h)
    if name == BODY:
        # JPEG rings on hard edges: fill the unused area with the mean colour.
        atlas[~covered, :3] = atlas[covered, :3].mean(axis=0).astype(np.uint8)
        atlas[..., 3] = 255
    return atlas, regions


def remap_uvs(mesh, region, size):
    """Move the mesh's UVs from its own texture into its atlas region."""
    uv = mesh.uvs.astype(np.float64)
    outside = ((uv < -1e-4) | (uv > 1.0 + 1e-4)).any(axis=1).mean()
    if outside > UV_OUTSIDE_WARN:
        warn(f"{mesh.name}: {outside:.1%} of the UVs lie outside [0, 1] and are clamped")
    uv = np.clip(uv, 0.0, 1.0)
    offset = np.array([region.x, region.y], dtype=np.float64)
    scale = np.array([region.width, region.height], dtype=np.float64)
    mesh.uvs = ((offset + uv * scale) / np.array(size, dtype=np.float64)).astype(np.float32)


def encode_atlas(name, pixels, jpeg_quality):
    """Encode an atlas as the file format its material uses; returns (bytes, MIME type)."""
    buffer = io.BytesIO()
    if name == BODY:
        Image.fromarray(np.ascontiguousarray(pixels[..., :3])).save(buffer, "JPEG", quality=jpeg_quality, optimize=True)
        return buffer.getvalue(), "image/jpeg"
    Image.fromarray(pixels).save(buffer, "PNG", optimize=True)
    return buffer.getvalue(), "image/png"


# ---------------------------------------------------------------------------
# Geometry
# ---------------------------------------------------------------------------


@dataclass
class Group:
    """The merged vertex data of one output material."""

    name: str
    positions: np.ndarray
    normals: np.ndarray
    uvs: np.ndarray
    joints: np.ndarray
    weights: np.ndarray
    indices: np.ndarray

    @property
    def triangles(self):
        return len(self.indices) // 3


def merge(meshes, name):
    """Concatenate the meshes of one group into a Group, or None when there are none."""
    if not meshes:
        return None
    offsets = np.cumsum([0] + [len(m.positions) for m in meshes[:-1]])
    return Group(
        name=name,
        positions=np.ascontiguousarray(np.concatenate([m.positions for m in meshes]), dtype=np.float32),
        normals=np.ascontiguousarray(np.concatenate([m.normals for m in meshes]), dtype=np.float32),
        uvs=np.ascontiguousarray(np.concatenate([m.uvs for m in meshes]), dtype=np.float32),
        joints=np.ascontiguousarray(np.concatenate([m.joints for m in meshes]), dtype=np.uint8),
        weights=np.ascontiguousarray(np.concatenate([m.weights for m in meshes]), dtype=np.float32),
        indices=np.ascontiguousarray(np.concatenate([m.indices + o for m, o in zip(meshes, offsets)]), dtype=np.uint32),
    )


def simplify(group, target_triangles, prune):
    """Index buffer of `group` reduced to about `target_triangles`.

    Edge collapse with normal and UV in the error metric first; when that
    stays well over target (small disconnected parts such as hair cards
    cannot collapse further) the sloppy vertex-clustering simplifier takes
    over. Both keep the original vertices, so skinning data stays valid.
    """
    target = target_triangles * 3
    if len(group.indices) <= target:
        return group.indices
    destination = np.empty(len(group.indices), dtype=np.uint32)
    attributes = np.ascontiguousarray(np.hstack([group.normals, group.uvs]), dtype=np.float32)
    count = meshoptimizer.simplify_with_attributes(
        destination,
        group.indices,
        group.positions,
        attributes,
        SIMPLIFY_WEIGHTS,
        target_index_count=target,
        target_error=1.0,
        options=PRUNE_OPTION if prune else 0,
    )
    result = destination[:count].copy()
    if count > target * SLOPPY_ABOVE:
        count = meshoptimizer.simplify_sloppy(
            destination, group.indices, group.positions, target_index_count=target, target_error=1.0
        )
        if count < len(result):
            result = destination[:count].copy()
    return result


def compact(group, indices):
    """A Group with only the vertices `indices` use, ordered for the vertex cache and fetch."""
    vertex_count = len(group.positions)
    cached = np.empty_like(indices)
    meshoptimizer.optimize_vertex_cache(cached, indices, vertex_count=vertex_count)
    remap = np.empty(vertex_count, dtype=np.uint32)
    unique = meshoptimizer.optimize_vertex_fetch_remap(remap, cached, vertex_count=vertex_count)
    used = remap != np.uint32(0xFFFFFFFF)
    order = np.empty(unique, dtype=np.int64)
    order[remap[used]] = np.flatnonzero(used)
    return Group(
        name=group.name,
        positions=group.positions[order],
        normals=group.normals[order],
        uvs=group.uvs[order],
        joints=group.joints[order],
        weights=group.weights[order],
        indices=remap[cached],
    )


def build_lods(groups, budgets):
    """Per level of detail, one compacted Group per input group within the split budget."""
    total = sum(g.triangles for g in groups)
    shares = {g.name: g.triangles / total for g in groups}
    if len(shares) == 2 and shares[CUTOUT] < CUTOUT_MIN_SHARE:
        shares[CUTOUT] = CUTOUT_MIN_SHARE
        shares[BODY] = 1.0 - CUTOUT_MIN_SHARE
    lods = []
    for level, budget in enumerate(budgets):
        primitives = []
        for group in groups:
            indices = simplify(group, max(1, round(budget * shares[group.name])), level >= PRUNE_FROM_LOD)
            if len(indices) == 0:
                warn(f"LOD{level}: the {group.name} group simplified away completely")
                continue
            primitives.append(compact(group, indices))
        lods.append(primitives)
    return lods


# ---------------------------------------------------------------------------
# Output
# ---------------------------------------------------------------------------


def build_nodes(gltf, skeleton, prefix, rest_shift, character_id):
    """The output node list and the map of old joint index → new index.

    `character` (0) holds the joint subtree (root first, depth-first) and the
    LOD mesh nodes. The root joint gets `prefix` in front of its rest
    transform and `rest_shift` added to y.
    """
    order = []
    stack = [skeleton.root]
    while stack:
        index = stack.pop()
        order.append(index)
        stack.extend(reversed(gltf["nodes"][index].get("children", [])))
    remap = {old: new for new, old in enumerate(order, start=1)}
    lod_start = 1 + len(order)
    nodes = [
        {
            "name": "character",
            "children": [1] + list(range(lod_start, lod_start + LOD_COUNT)),
            "extras": {"id": character_id, "generator": GENERATOR},
        }
    ]
    for old in order:
        source = gltf["nodes"][old]
        node = {"name": source.get("name", f"node{old}")}
        for key in ("translation", "rotation", "scale"):
            if key in source:
                node[key] = [float(v) for v in source[key]]
        if source.get("children"):
            node["children"] = [remap[child] for child in source["children"]]
        nodes.append(node)
    translation, rotation, scale = glb.trs_compose(prefix, glb.node_trs(gltf["nodes"][skeleton.root]))
    root = nodes[1]
    root["translation"] = (translation + [0.0, rest_shift, 0.0]).tolist()
    root["rotation"] = rotation.tolist()
    root.pop("scale", None)
    if scale != 1.0:
        root["scale"] = [scale] * 3
    for level in range(LOD_COUNT):
        nodes.append({"name": f"char_LOD{level}", "mesh": level, "skin": 0})
    return nodes, remap


def build_animations(gltf, binary, skeleton, remap, prefix, clip_shifts, builder):
    """Copy the clips into the builder; the root joint's channels get the prefix and the ground shift."""
    animations = []
    for number, (animation, shift) in enumerate(zip(gltf.get("animations", []), clip_shifts)):
        name = animation.get("name", f"clip{number}")
        inputs = {}
        samplers, channels = [], []
        root_translated = False
        for channel in animation["channels"]:
            target = channel["target"]
            node, path = target.get("node"), target["path"]
            if node not in remap or path == "weights":
                warn(f"clip {name}: {path} channel of node {node} dropped")
                continue
            sampler = animation["samplers"][channel["sampler"]]
            if sampler["input"] not in inputs:
                times = glb.read_accessor(gltf, binary, sampler["input"]).astype(np.float32)
                inputs[sampler["input"]] = (times, builder.add_accessor(times, bounds=True))
            values = glb.read_accessor(gltf, binary, sampler["output"]).astype(np.float64)
            if node == skeleton.root:
                if path == "translation":
                    values = glb.trs_compose(prefix, (values, glb.IDENTITY_QUATERNION, 1.0))[0] + [0.0, shift, 0.0]
                    root_translated = True
                elif path == "rotation":
                    values = glb.quat_multiply(prefix[1], values)
                elif path == "scale":
                    values = values * prefix[2]
            samplers.append(
                {
                    "input": inputs[sampler["input"]][1],
                    "interpolation": sampler.get("interpolation", "LINEAR"),
                    "output": builder.add_accessor(values.astype(np.float32)),
                }
            )
            channels.append({"sampler": len(samplers) - 1, "target": {"node": remap[node], "path": path}})
        if not root_translated:
            # The ground shift needs a translation channel on the root: a constant over the clip's span.
            times = np.unique(np.concatenate([t for t, _ in inputs.values()]))
            span = np.unique(np.array([times[0], times[-1]], dtype=np.float32))
            rest = glb.trs_compose(prefix, glb.node_trs(gltf["nodes"][skeleton.root]))[0] + [0.0, shift, 0.0]
            samplers.append(
                {
                    "input": builder.add_accessor(span, bounds=True),
                    "interpolation": "STEP",
                    "output": builder.add_accessor(np.tile(rest, (len(span), 1)).astype(np.float32)),
                }
            )
            channels.append({"sampler": len(samplers) - 1, "target": {"node": remap[skeleton.root], "path": "translation"}})
        animations.append({"name": name, "samplers": samplers, "channels": channels})
    return animations


def assemble(gltf, binary, skeleton, prefix, rest_shift, clip_shifts, lods, atlases, character_id, jpeg_quality):
    """The output glTF document and its buffer."""
    builder = glb.BufferBuilder()
    nodes, remap = build_nodes(gltf, skeleton, prefix, rest_shift, character_id)
    skin = {
        "name": "skeleton",
        "joints": [remap[j] for j in skeleton.joints],
        "skeleton": remap[skeleton.root],
        "inverseBindMatrices": builder.add_accessor(glb.matrices_to_ibm(skeleton.ibm @ TURN_MATRIX)),
    }
    images, textures, materials, material_index = [], [], [], {}
    for name, pixels in atlases.items():
        data, mime = encode_atlas(name, pixels, jpeg_quality)
        images.append({"name": name, "mimeType": mime, "bufferView": builder.add_view(data)})
        textures.append({"sampler": 0, "source": len(images) - 1})
        material = json.loads(json.dumps(MATERIALS[name]))
        material["pbrMetallicRoughness"]["baseColorTexture"] = {"index": len(textures) - 1}
        material_index[name] = len(materials)
        materials.append(material)
    meshes = []
    for level, groups in enumerate(lods):
        primitives = []
        for group in groups:
            index_type = np.uint16 if len(group.positions) <= 0xFFFF else np.uint32
            primitives.append(
                {
                    "attributes": {
                        "POSITION": builder.add_accessor(group.positions, glb.ARRAY_BUFFER, bounds=True),
                        "NORMAL": builder.add_accessor(group.normals, glb.ARRAY_BUFFER),
                        "TEXCOORD_0": builder.add_accessor(group.uvs, glb.ARRAY_BUFFER),
                        "JOINTS_0": builder.add_accessor(group.joints, glb.ARRAY_BUFFER),
                        "WEIGHTS_0": builder.add_accessor(group.weights, glb.ARRAY_BUFFER),
                    },
                    "indices": builder.add_accessor(group.indices.astype(index_type), glb.ELEMENT_ARRAY_BUFFER),
                    "material": material_index[group.name],
                    "mode": 4,
                }
            )
        meshes.append({"name": f"char_LOD{level}", "primitives": primitives})
    animations = build_animations(gltf, binary, skeleton, remap, prefix, clip_shifts, builder)
    buffer = builder.binary()
    document = {
        "asset": {"version": "2.0", "generator": GENERATOR},
        "scene": 0,
        "scenes": [{"name": character_id, "nodes": [0]}],
        "nodes": nodes,
        "meshes": meshes,
        "skins": [skin],
        "materials": materials,
        "textures": textures,
        "images": images,
        "samplers": [SAMPLER],
        "accessors": builder.accessors,
        "bufferViews": builder.views,
        "buffers": [{"byteLength": len(buffer)}],
    }
    if animations:
        document["animations"] = animations
    return document, buffer


def statistics(character_id, meta, height, lods, atlases, document, binary, file_bytes, source_triangles):
    """The numbers `--stats` writes."""
    levels = []
    for groups in lods:
        level = {"tris": sum(g.triangles for g in groups), "verts": sum(len(g.positions) for g in groups)}
        for group in groups:
            level[group.name] = {"tris": group.triangles, "verts": len(group.positions)}
        levels.append(level)
    clips = []
    for animation in document.get("animations", []):
        times = np.unique(np.concatenate([glb.read_accessor(document, binary, s["input"]) for s in animation["samplers"]]))
        clips.append({"name": animation["name"], "frames": len(times), "seconds": round(float(times[-1]), 4)})
    return {
        "id": character_id,
        "gender": meta.get("gender"),
        "height_m": round(height, 4),
        "lods": levels,
        "atlases": {name: [pixels.shape[1], pixels.shape[0]] for name, pixels in atlases.items()},
        "clips": clips,
        "file_bytes": file_bytes,
        "source_triangles": source_triangles,
    }


# ---------------------------------------------------------------------------
# Self-check
# ---------------------------------------------------------------------------


def check(path):
    """Re-read `path` and verify what the game relies on; prints a report, returns True when all holds."""
    gltf, binary = glb.read_glb(path)
    report = []
    nodes = gltf["nodes"]
    by_name = {node.get("name"): node for node in nodes}
    lod_nodes = [by_name.get(f"char_LOD{level}") for level in range(LOD_COUNT)]
    present = [node for node in lod_nodes if node is not None and "mesh" in node]
    skins = gltf.get("skins", [])
    joints = skins[0]["joints"] if skins else []
    report.append(
        (
            len(present) == LOD_COUNT and {node.get("skin") for node in present} == {0} and len(skins) == 1
            and len(joints) == JOINT_COUNT,
            f"{len(present)} LOD nodes char_LOD0..{LOD_COUNT - 1}, {len(skins)} skin with {len(joints)} joints",
        )
    )
    if len(present) != LOD_COUNT or not skins:
        return finish_report(path, report)

    skeleton = Skeleton(gltf, binary)
    primitives = gltf["meshes"][present[0]["mesh"]]["primitives"]
    names = [gltf["materials"][p["material"]]["name"] for p in primitives]
    parts = [
        SimpleNamespace(
            positions=glb.read_accessor(gltf, binary, p["attributes"]["POSITION"]),
            joints=glb.read_accessor(gltf, binary, p["attributes"]["JOINTS_0"]),
            weights=glb.read_accessor(gltf, binary, p["attributes"]["WEIGHTS_0"]),
        )
        for p in primitives
    ]
    baker = Baker(skeleton, parts)
    rest = baker.pose({})
    ground, height = rest[:, 1].min(), rest[:, 1].max()
    report.append((1.4 <= height <= 2.1 and abs(ground) <= 0.01, f"rest pose: feet at y = {ground:+.4f}, height {height:.3f} m"))
    for animation in gltf.get("animations", []):
        _, poses = glb.clip_poses(gltf, binary, animation)
        low = min(baker.pose(pose)[:, 1].min() for pose in poses)
        report.append((abs(low) <= 0.01, f"clip {animation['name']}: lowest vertex y = {low:+.4f} over {len(poses)} frames"))
    seated = seated_geometry(gltf, binary, skeleton, baker)
    if seated is not None:
        seat, ahead, hands = seated
        report.append(
            (
                SEAT_HEIGHT[0] <= seat <= SEAT_HEIGHT[1] and ahead >= KNEES_AHEAD,
                f"clip sit: seat {seat:.2f} m up, knees {ahead:.2f} m ahead of the pelvis",
            )
        )
        report.append((abs(hands - seat) <= HANDS_NEAR_SEAT, f"clip sit: hands {hands:.2f} m up, near the thighs"))
    for name, part in zip(names, baker.slices):
        vertices = rest[part]
        if name == CUTOUT:
            head = vertices[vertices[:, 1] > 0.8 * height]
            front = head[:, 2].min() if len(head) else np.nan
            report.append((front < 0, f"cutout head region reaches z = {front:+.3f} (eyes towards -Z)"))
        if name == BODY:
            feet = vertices[vertices[:, 1] < 0.12]
            toes, heels = feet[:, 2].min(), feet[:, 2].max()
            report.append((toes < 0 and -toes > heels, f"feet span z {toes:+.3f}..{heels:+.3f} (toes towards -Z)"))
    triangles = []
    for node in present:
        mesh = gltf["meshes"][node["mesh"]]
        triangles.append(sum(gltf["accessors"][p["indices"]]["count"] // 3 for p in mesh["primitives"]))
    report.append(
        (
            all(a >= b for a, b in zip(triangles, triangles[1:])) and triangles[-1] <= 800,
            f"LOD triangles {' > '.join(str(t) for t in triangles)} (last <= 800)",
        )
    )
    views = gltf["bufferViews"]
    aligned = inside = True
    for accessor in gltf["accessors"]:
        view = views[accessor["bufferView"]]
        offset = view.get("byteOffset", 0) + accessor.get("byteOffset", 0)
        size = np.dtype(glb.COMPONENT_DTYPES[accessor["componentType"]]).itemsize
        aligned &= offset % size == 0 and offset % 4 == 0
        length = accessor["count"] * size * glb.TYPE_COMPONENTS[accessor["type"]]
        inside &= accessor.get("byteOffset", 0) + length <= view["byteLength"]
    buffer_length = gltf["buffers"][0]["byteLength"]
    for view in views:
        aligned &= view.get("byteOffset", 0) % 4 == 0
        inside &= view.get("byteOffset", 0) + view["byteLength"] <= buffer_length
    inside &= buffer_length <= len(binary)
    report.append((aligned, f"{len(gltf['accessors'])} accessors and {len(views)} buffer views 4-byte aligned"))
    report.append((inside, f"all buffer views inside the {buffer_length} byte buffer"))
    return finish_report(path, report)


def finish_report(path, report):
    """Print the check report; returns whether every item passed."""
    print(f"check {path}")
    for ok, text in report:
        print(f"  {'ok  ' if ok else 'FAIL'} {text}")
    passed = all(ok for ok, _ in report)
    print("check passed" if passed else "check FAILED")
    return passed


# ---------------------------------------------------------------------------
# Command line
# ---------------------------------------------------------------------------


def parse_size(text):
    """`WIDTHxHEIGHT` → (width, height)."""
    try:
        width, height = (int(v) for v in text.lower().split("x"))
    except ValueError:
        raise argparse.ArgumentTypeError(f"expected WIDTHxHEIGHT, got {text!r}") from None
    if width <= 0 or height <= 0:
        raise argparse.ArgumentTypeError(f"atlas size has to be positive, got {text!r}")
    return width, height


def parse_lods(text):
    """`14000,5000,1600,500` → four non-increasing triangle budgets."""
    try:
        budgets = [int(v) for v in text.split(",")]
    except ValueError:
        raise argparse.ArgumentTypeError(f"expected four triangle counts, got {text!r}") from None
    if len(budgets) != LOD_COUNT or budgets[-1] <= 0 or budgets != sorted(budgets, reverse=True):
        raise argparse.ArgumentTypeError(f"expected four non-increasing triangle counts, got {text!r}")
    return budgets


def parse_max_edges(text):
    """`skin=1024,hair=512` → per-class maximum edge overrides."""
    edges = {}
    for item in text.split(","):
        key, _, value = item.partition("=")
        if key not in CLASS_MAX_EDGE or not value.isdigit():
            raise argparse.ArgumentTypeError(f"expected CLASS=EDGE with a class of {', '.join(CLASS_MAX_EDGE)}, got {item!r}")
        edges[key] = int(value)
    return edges


def parse_args(argv):
    """The command line; `argv` None takes sys.argv."""
    parser = argparse.ArgumentParser(description="Post-process a MakeHuman2 GLB export into a game character.")
    parser.add_argument("input", help="GLB written by MakeHuman2")
    parser.add_argument("output", help="game-ready GLB to write")
    parser.add_argument("--meta", required=True, help="side-car meta JSON of the export")
    parser.add_argument("--stats", help="write build statistics to this JSON file")
    parser.add_argument("--body-atlas", type=parse_size, default=(2048, 1024), help="opaque atlas size, default 2048x1024")
    parser.add_argument("--cutout-atlas", type=parse_size, default=(1024, 512), help="alpha atlas size, default 1024x512")
    parser.add_argument("--lods", type=parse_lods, default=[14000, 5000, 1600, 500], help="triangle budgets per LOD")
    parser.add_argument(
        "--max-edges", type=parse_max_edges, default={}, help="per-class texture edge overrides, e.g. skin=1024,hair=512"
    )
    parser.add_argument("--jpeg-quality", type=int, default=85, help="quality of the body atlas, default 85")
    parser.add_argument("--check", action="store_true", help="re-read the output and verify it")
    return parser.parse_args(argv)


def build(args):
    """Run the pipeline; returns the process exit code."""
    started = time.time()
    meta = json.loads(Path(args.meta).read_text(encoding="utf-8"))
    character_id = meta.get("id", Path(args.output).stem)
    gltf, binary = glb.read_glb(args.input)
    skeleton = Skeleton(gltf, binary)
    if len(skeleton.joints) != JOINT_COUNT:
        warn(f"skeleton has {len(skeleton.joints)} joints, the game rig has {JOINT_COUNT}")
    binary = synthesize_sit(gltf, binary, skeleton)
    meshes = load_meshes(gltf, binary, meta)
    source_triangles = sum(len(m.indices) // 3 for m in meshes)

    baker = Baker(skeleton, meshes)
    rest = baker.pose({})
    ground = float(rest[:, 1].min())
    height = float(rest[:, 1].max() - ground)
    classify(meshes, baker, rest, ground, height)
    clip_shifts = ground_shifts(gltf, binary, baker)

    max_edges = {**CLASS_MAX_EDGE, **args.max_edges}
    decoded = {}
    atlases = {}
    for name, size in ((BODY, args.body_atlas), (CUTOUT, args.cutout_atlas)):
        members = [m for m in meshes if m.group == name]
        if not members:
            continue
        textures = {}
        for mesh in members:
            if mesh.texture not in decoded:
                decoded[mesh.texture] = decode_texture(gltf, binary, mesh.texture)
            edge = max(max_edges[mesh.kind], textures[mesh.texture][1] if mesh.texture in textures else 0)
            textures[mesh.texture] = (decoded[mesh.texture], edge)
        atlas, regions = build_atlas(name, textures, size)
        for mesh in members:
            remap_uvs(mesh, regions[mesh.texture], size)
        atlases[name] = atlas

    turn_meshes(meshes)
    groups = [g for g in (merge([m for m in meshes if m.group == n], n) for n in (BODY, CUTOUT)) if g is not None]
    lods = build_lods(groups, args.lods)
    prefix = root_prefix(gltf, skeleton)
    document, buffer = assemble(
        gltf, binary, skeleton, prefix, -ground, clip_shifts, lods, atlases, character_id, args.jpeg_quality
    )
    file_bytes = glb.write_glb(args.output, document, buffer)

    stats = statistics(character_id, meta, height, lods, atlases, document, buffer, file_bytes, source_triangles)
    if args.stats:
        Path(args.stats).write_text(json.dumps(stats, indent=2) + "\n", encoding="utf-8")
    lod_text = ", ".join(f"LOD{i} {level['tris']} tris / {level['verts']} verts" for i, level in enumerate(stats["lods"]))
    atlas_text = ", ".join(f"{name} {w}x{h}" for name, (w, h) in stats["atlases"].items())
    print(f"{args.output}: {file_bytes / 1024:.0f} KB, height {height:.3f} m, {source_triangles} source tris")
    print(f"  {lod_text}")
    print(f"  atlases {atlas_text}; clips {', '.join(c['name'] for c in stats['clips'])}")
    print(f"  built in {time.time() - started:.1f} s, {len(WARNINGS)} warning(s)")
    if args.check and not check(args.output):
        return 1
    return 0


def main(argv=None):
    """Entry point; returns the exit code."""
    args = parse_args(argv)
    try:
        return build(args)
    except BuildError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
