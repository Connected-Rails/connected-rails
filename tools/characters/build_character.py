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

CUTOUT_MIN_SHARE = 0.12  # of every triangle budget, so the hair outlives the body
SMALL_GROUP_SHARE = 0.25  # a group below this share of the budget is kept whole
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


# MakeHuman's walk cycles come out narrow and hunched on the game rig — knees together,
# short shuffling steps. The `walk` clip is therefore made here too: a plain in-place
# cycle out of the rest pose, one second long (two steps, the pace `CYCLE_PACE` of the
# game), so the feet cover the ground the person moves over. Angles in degrees; the
# signs are found per joint like the chair pose finds them.
WALK_CLIP = "walk"
WALK_FRAMES = 24
WALK_SECONDS = 1.0
WALK_HIP = 28.0  # thigh swing amplitude, forward and back
WALK_KNEE = 42.0  # knee flexion at the middle of the swing
WALK_KNEE_STANCE = 5.0  # a knee is never locked straight
WALK_ARM = 8.0  # upper arm swing, opposite to the leg on its side
WALK_ELBOW = 5.0  # constant elbow bend
# A relaxed arm hangs this far from the vertical, sideways and forwards [deg]; the
# rest pose's A-pose arms are turned in and back by whatever it takes to get there.
ARM_HANG_OUT = 6.0
ARM_HANG_FORWARD = 4.0
# The MakeHuman standing clips hold the arms out and bent; they get the relaxed arms
# of the rest pose instead, frame by frame, so hands hang beside the thighs.
RELAXED_ARM_CLIPS = ("idle", "idle2", "stand", "stand2", "stand3")
# The feet walk this far apart sideways [m]; the thighs are turned in or out from the
# rest pose's stance (MakeHuman's A-pose stands 0.3–0.4 m wide) to get there.
WALK_STEP_WIDTH = 0.15
WALK_LEAN = 3.0  # forward lean of the lower spine
WALK_BOB = 0.02  # vertical bob of the body [m], twice per cycle
# Sign probes per joint and world axis: the joint whose position moves the way named
# when the angle is positive — the rig's own axes are whatever MakeHuman made them.
WALK_PROBES = {
    ("thigh_l", "x"): ("calf_l", "z", "max"),  # knee forward
    ("thigh_r", "x"): ("calf_r", "z", "max"),
    ("calf_l", "x"): ("foot_l", "z", "min"),  # heel back: knee flexion
    ("calf_r", "x"): ("foot_r", "z", "min"),
    ("foot_l", "x"): ("ball_l", "y", "max"),  # toes up
    ("foot_r", "x"): ("ball_r", "y", "max"),
    ("thigh_l", "z"): ("calf_l", "|x|", "max"),  # knee outward
    ("thigh_r", "z"): ("calf_r", "|x|", "max"),
    ("upperarm_l", "x"): ("hand_l", "z", "max"),  # hand forward
    ("upperarm_r", "x"): ("hand_r", "z", "max"),
    ("lowerarm_l", "x"): ("hand_l", "z", "max"),  # elbow flexion brings the hand forward
    ("lowerarm_r", "x"): ("hand_r", "z", "max"),
    ("upperarm_l", "z"): ("hand_l", "|x|", "min"),  # arm in, towards the body
    ("upperarm_r", "z"): ("hand_r", "|x|", "min"),
    ("spine_01", "x"): ("head", "z", "max"),  # lean forward
}


def arm_hang(gltf, skeleton):
    """Per upper arm, the turns [deg] about the world Z (sideways, positive = in) and X
    (positive = forward) axes that let the rest pose's arm hang `ARM_HANG_OUT` from the
    vertical sideways and `ARM_HANG_FORWARD` forwards, measured shoulder to hand."""
    nodes = gltf["nodes"]
    by_name = {nodes[j].get("name"): j for j in skeleton.joints}
    world = glb.global_matrices(gltf, skeleton.parents, {})
    turns = {}
    for side in ("l", "r"):
        shoulder = world[by_name[f"upperarm_{side}"]][:3, 3]
        hand = world[by_name[f"hand_{side}"]][:3, 3]
        v = hand - shoulder
        down = max(-v[1], 1e-6)
        out = np.degrees(np.arctan2(abs(v[0]), down))
        forward = np.degrees(np.arctan2(v[2], down))  # the exporter's frame: +Z is the front
        turns[side] = (out - ARM_HANG_OUT, ARM_HANG_FORWARD - forward)
    return turns


def probe_signs(gltf, skeleton):
    """The sign that turns each joint of `WALK_PROBES` the named way, or `None` if a
    joint is missing from the rig."""
    nodes = gltf["nodes"]
    by_name = {nodes[j].get("name"): j for j in skeleton.joints}
    signs = {}
    for (joint, axis), (probe, coordinate, extreme) in WALK_PROBES.items():
        if joint not in by_name or probe not in by_name:
            return None
        index = by_name[joint]
        rest = glb.global_matrices(gltf, skeleton.parents, {})
        parent = skeleton.parents.get(index)
        above = matrix_to_quat(rest[parent]) if parent is not None else np.array(glb.IDENTITY_QUATERNION, dtype=np.float64)
        local = glb.node_trs(nodes[index])[1]
        best = None
        for sign in (1.0, -1.0):
            turn = quat_from_axis_angle(WORLD_AXES[axis], sign * 20.0)
            rotated = glb.quat_multiply(quat_conjugate(above), glb.quat_multiply(turn, glb.quat_multiply(above, local)))
            position = glb.global_matrices(gltf, skeleton.parents, {index: {"rotation": rotated}})[by_name[probe]][:3, 3]
            value = position["xyz".index(coordinate.strip("|"))]
            if coordinate.startswith("|"):
                value = abs(value)
            score = value if extreme == "max" else -value
            if best is None or score > best[0]:
                best = (score, sign)
        signs[(joint, axis)] = best[1]
    return signs


def walk_spread(gltf, skeleton):
    """Thigh abduction [deg] that brings the feet to `WALK_STEP_WIDTH` apart: the rest
    stance measured between the ankle joints, the change per degree from the leg's length."""
    nodes = gltf["nodes"]
    by_name = {nodes[j].get("name"): j for j in skeleton.joints}
    world = glb.global_matrices(gltf, skeleton.parents, {})
    at = lambda name: world[by_name[name]][:3, 3]
    stance = abs(at("foot_l")[0] - at("foot_r")[0])
    leg = np.linalg.norm(at("foot_l") - at("thigh_l"))
    if leg < 1e-3:
        return 0.0
    return float(np.degrees(np.arcsin(np.clip((WALK_STEP_WIDTH - stance) / (2.0 * leg), -0.5, 0.5))))


def walk_angles(phase, spread, hang):
    """Joint turns [deg] of the walk at `phase` (0 … 2π), keyed like `WALK_PROBES`;
    `spread` is the thigh abduction of `walk_spread`, `hang` the arm turns of `arm_hang`."""
    s, c = np.sin(phase), np.cos(phase)
    # Left leg: forward at phase π/2, back at 3π/2; the knee bends while the leg swings
    # forward (around phase 0) and stays a touch bent in stance.
    knee_l = WALK_KNEE_STANCE + WALK_KNEE * max(0.0, c) ** 1.5
    knee_r = WALK_KNEE_STANCE + WALK_KNEE * max(0.0, -c) ** 1.5
    hip_l, hip_r = WALK_HIP * s, -WALK_HIP * s
    return {
        ("thigh_l", "x"): hip_l,
        ("thigh_r", "x"): hip_r,
        ("calf_l", "x"): knee_l,
        ("calf_r", "x"): knee_r,
        # A level foot: undo the thigh and shin turns, plus a little toe lift in the swing.
        ("foot_l", "x"): -(hip_l - knee_l) + 6.0 * max(0.0, c),
        ("foot_r", "x"): -(hip_r - knee_r) + 6.0 * max(0.0, -c),
        ("thigh_l", "z"): spread,
        ("thigh_r", "z"): spread,
        ("upperarm_l", "x"): hang["l"][1] - WALK_ARM * s,
        ("upperarm_r", "x"): hang["r"][1] + WALK_ARM * s,
        ("upperarm_l", "z"): hang["l"][0],
        ("upperarm_r", "z"): hang["r"][0],
        ("lowerarm_l", "x"): WALK_ELBOW,
        ("lowerarm_r", "x"): WALK_ELBOW,
        ("spine_01", "x"): WALK_LEAN,
    }


def turned_pose(gltf, skeleton, signs, angles, base=None):
    """Rotation overrides of a pose: the turns in `angles` (keyed like `WALK_PROBES`)
    applied about world axes in hierarchy order on top of `base` (a pose's overrides,
    the rest pose by default), each on the joints already turned."""
    nodes = gltf["nodes"]
    by_name = {nodes[j].get("name"): j for j in skeleton.joints}
    overrides = {k: dict(v) for k, v in (base or {}).items()}
    # Parents before children, so a child's world turn sits on its parent's new frame.
    order = ["spine_01", "thigh_l", "thigh_r", "calf_l", "calf_r", "foot_l", "foot_r",
             "upperarm_l", "upperarm_r", "lowerarm_l", "lowerarm_r"]
    for joint in order:
        for axis in ("z", "x"):
            key = (joint, axis)
            if key not in angles:
                continue
            index = by_name[joint]
            world = glb.global_matrices(gltf, skeleton.parents, overrides)
            parent = skeleton.parents.get(index)
            above = matrix_to_quat(world[parent]) if parent is not None else np.array(glb.IDENTITY_QUATERNION, dtype=np.float64)
            local = overrides.get(index, {}).get("rotation", glb.node_trs(nodes[index])[1])
            turn = quat_from_axis_angle(WORLD_AXES[axis], signs[key] * angles[key])
            rotated = glb.quat_multiply(quat_conjugate(above), glb.quat_multiply(turn, glb.quat_multiply(above, local)))
            overrides[index] = {**overrides.get(index, {}), "rotation": rotated}
    return overrides


def walk_frame(gltf, skeleton, signs, phase, spread, hang):
    """Rotation overrides of one walk frame out of the rest pose."""
    return turned_pose(gltf, skeleton, signs, walk_angles(phase, spread, hang))


def relax_arms(gltf, binary, skeleton, signs):
    """Give the MakeHuman standing clips the relaxed arms of the rest pose: in every
    frame the arm joints take the rest pose's local rotations turned by `arm_hang`,
    so the hands hang beside the thighs however the torso sways. Returns the binary
    with the rewritten channels appended."""
    hang = arm_hang(gltf, skeleton)
    angles = {
        ("upperarm_l", "z"): hang["l"][0],
        ("upperarm_r", "z"): hang["r"][0],
        ("upperarm_l", "x"): hang["l"][1],
        ("upperarm_r", "x"): hang["r"][1],
        ("lowerarm_l", "x"): WALK_ELBOW,
        ("lowerarm_r", "x"): WALK_ELBOW,
    }
    relaxed = turned_pose(gltf, skeleton, signs, angles)
    nodes = gltf["nodes"]
    by_name = {nodes[j].get("name"): j for j in skeleton.joints}
    arm_joints = [by_name[n] for n in ("upperarm_l", "upperarm_r", "lowerarm_l", "lowerarm_r") if n in by_name]
    for animation in gltf.get("animations", []):
        if animation.get("name") not in RELAXED_ARM_CLIPS:
            continue
        for channel in animation["channels"]:
            target = channel["target"]
            node = target.get("node")
            if node not in arm_joints or target["path"] != "rotation":
                continue
            sampler = animation["samplers"][channel["sampler"]]
            count = gltf["accessors"][sampler["output"]]["count"]
            rotation = np.asarray(relaxed[node]["rotation"], dtype=np.float32)
            binary, output = append_accessor(gltf, binary, np.tile(rotation, (count, 1)), "VEC4")
            sampler["output"] = output
    return binary


def synthesize_walk(gltf, binary, skeleton):
    """Replace (or add) the `walk` clip by the procedural cycle; returns the binary."""
    signs = probe_signs(gltf, skeleton)
    if signs is None:
        warn("walk cycle: joints missing from the rig, MakeHuman's walk kept")
        return binary
    binary = relax_arms(gltf, binary, skeleton, signs)
    frames = WALK_FRAMES + 1  # the last keyframe repeats the first, so the loop is seamless
    times = np.arange(frames, dtype=np.float32) * (WALK_SECONDS / WALK_FRAMES)
    binary, time_accessor = append_accessor(gltf, binary, times, "SCALAR", bounds=True)
    spread = walk_spread(gltf, skeleton)
    hang = arm_hang(gltf, skeleton)
    poses = [
        walk_frame(gltf, skeleton, signs, 2.0 * np.pi * (f % WALK_FRAMES) / WALK_FRAMES, spread, hang)
        for f in range(frames)
    ]
    samplers, channels = [], []
    for joint in skeleton.joints:
        translation, rest_rotation, _ = glb.node_trs(gltf["nodes"][joint])
        rotations = np.array([p.get(joint, {}).get("rotation", rest_rotation) for p in poses], dtype=np.float64)
        # Keep neighbouring quaternions in one hemisphere, or the loop flips.
        for k in range(1, frames):
            if np.dot(rotations[k], rotations[k - 1]) < 0:
                rotations[k] = -rotations[k]
        translations = np.tile(np.asarray(translation, dtype=np.float64), (frames, 1))
        if joint == skeleton.root:
            bob = WALK_BOB * 0.5 * (np.cos(2.0 * 2.0 * np.pi * np.arange(frames) / WALK_FRAMES) - 1.0)
            translations[:, 1] += bob
        for path, values in (("translation", translations), ("rotation", rotations)):
            binary, output = append_accessor(gltf, binary, values, "VEC3" if path == "translation" else "VEC4")
            samplers.append({"input": time_accessor, "interpolation": "LINEAR", "output": output})
            channels.append({"sampler": len(samplers) - 1, "target": {"node": joint, "path": path}})
    clip = {"name": WALK_CLIP, "samplers": samplers, "channels": channels}
    animations = gltf.setdefault("animations", [])
    for number, animation in enumerate(animations):
        if animation.get("name") == WALK_CLIP:
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
# Subdivision of the garments
# ---------------------------------------------------------------------------

# MakeHuman's clothes are coarse: a suit is two thousand quads, and over the bust that
# is a handful of flat facets. Loop subdivision rounds them off; the finest level of
# detail is then simplified back to its budget, which keeps the rounded shape where
# the curvature is and throws the flat parts away again. Skin, hair, eyes and shoes
# are dense enough as they are.
SUBDIVIDE_KINDS = ("clothes", "hat")
SUBDIVIDE_LEVELS = 2
# Loop subdivision shrinks a surface a little towards its inside; the garment is pushed
# out along its normals by this much afterwards so the skin under it stays covered [m].
SUBDIVIDE_INFLATE = 0.002
WELD_PRECISION = 5  # decimals of a metre that make two vertices one point


def welded_ids(positions):
    """One id per point in space, so UV seams and hard edges do not split the topology."""
    _, inverse = np.unique(np.round(positions, WELD_PRECISION), axis=0, return_inverse=True)
    return inverse.reshape(-1)


def blend_skin(joints, weights, a, b):
    """Skinning of the point halfway between vertices `a` and `b`: the two weight sets
    added, the four strongest joints kept, renormalised."""
    count = len(a)
    bones = int(joints.max()) + 1 if len(joints) else 1
    dense = np.zeros((count, bones), dtype=np.float64)
    rows = np.arange(count)
    for end in (a, b):
        for k in range(4):
            np.add.at(dense, (rows, joints[end, k]), weights[end, k] * 0.5)
    top = np.argsort(-dense, axis=1)[:, :4]
    picked = np.take_along_axis(dense, top, axis=1)
    picked /= np.maximum(picked.sum(axis=1, keepdims=True), 1e-12)
    return top.astype(np.uint8), picked.astype(np.float32)


def loop_once(mesh):
    """One level of Loop subdivision of a SourceMesh, seams closed, borders fixed.

    Positions are smoothed on the welded topology and shared by every copy of a
    point, so a UV seam stays closed. Border vertices (an edge with one triangle)
    stay where they are and border edges split at their midpoints, so a neckline
    or a cuff still meets the skin exactly where the garment's author put it.
    UVs and skinning are averaged per render vertex; normals are recomputed.
    """
    P = mesh.positions.astype(np.float64)
    I = mesh.indices.reshape(-1, 3).astype(np.int64)
    n = len(P)
    weld = welded_ids(P)
    welded_count = int(weld.max()) + 1
    Pw = np.zeros((welded_count, 3))
    Pw[weld] = P  # any copy will do, they coincide

    # Welded edges with their multiplicity and the vertex opposite each occurrence.
    T = weld[I]
    sides = np.stack([T[:, [0, 1]], T[:, [1, 2]], T[:, [2, 0]]], axis=1).reshape(-1, 2)
    opposite = T[:, [2, 0, 1]].reshape(-1)
    edges = np.sort(sides, axis=1)
    unique_edges, edge_of_side, multiplicity = np.unique(edges, axis=0, return_inverse=True, return_counts=True)
    edge_of_side = edge_of_side.reshape(-1)
    opposite_sum = np.zeros((len(unique_edges), 3))
    np.add.at(opposite_sum, edge_of_side, Pw[opposite])
    border_edge = multiplicity == 1
    ends = Pw[unique_edges[:, 0]] + Pw[unique_edges[:, 1]]
    # Loop's edge rule: 3/8 of both ends and 1/8 of both opposite vertices — the
    # opposite share spread over however many triangles meet at a non-manifold edge.
    interior = ends * 0.375 + opposite_sum * (0.25 / np.maximum(multiplicity[:, None], 1))
    edge_point = np.where(border_edge[:, None], ends * 0.5, interior)

    # Vertex smoothing (Loop's beta by valence), border vertices fixed.
    valence = np.zeros(welded_count)
    np.add.at(valence, unique_edges[:, 0], 1)
    np.add.at(valence, unique_edges[:, 1], 1)
    neighbour_sum = np.zeros((welded_count, 3))
    np.add.at(neighbour_sum, unique_edges[:, 0], Pw[unique_edges[:, 1]])
    np.add.at(neighbour_sum, unique_edges[:, 1], Pw[unique_edges[:, 0]])
    on_border = np.zeros(welded_count, dtype=bool)
    on_border[unique_edges[border_edge].reshape(-1)] = True
    k = np.maximum(valence, 3)
    beta = (0.625 - (0.375 + 0.25 * np.cos(2.0 * np.pi / k)) ** 2) / k
    smoothed = (1.0 - k * beta)[:, None] * Pw + beta[:, None] * neighbour_sum
    smoothed[on_border | (valence < 3)] = Pw[on_border | (valence < 3)]

    # Render-level midpoints: one per unordered pair of render vertices, so a seam
    # keeps two midpoints with their own UVs but one shared position.
    render_sides = np.stack([I[:, [0, 1]], I[:, [1, 2]], I[:, [2, 0]]], axis=1).reshape(-1, 2)
    render_edges = np.sort(render_sides, axis=1)
    unique_render, mid_of_side = np.unique(render_edges, axis=0, return_inverse=True)
    mid_of_side = mid_of_side.reshape(-1)
    # Which welded edge each render edge lies on: any side that uses it.
    first_side = np.zeros(len(unique_render), dtype=np.int64)
    first_side[mid_of_side[::-1]] = np.arange(len(mid_of_side))[::-1]
    mid_positions = edge_point[edge_of_side[first_side]]
    a, b = unique_render[:, 0], unique_render[:, 1]
    mid_uvs = (mesh.uvs[a].astype(np.float64) + mesh.uvs[b]) * 0.5
    mid_joints, mid_weights = blend_skin(mesh.joints.astype(np.int64), mesh.weights.astype(np.float64), a, b)

    positions = np.concatenate([smoothed[weld], mid_positions])
    uvs = np.concatenate([mesh.uvs.astype(np.float64), mid_uvs]).astype(np.float32)
    joints = np.concatenate([mesh.joints, mid_joints]).astype(np.uint8)
    weights = np.concatenate([mesh.weights, mid_weights]).astype(np.float32)
    m = mid_of_side.reshape(-1, 3) + n  # midpoints of sides ab, bc, ca per triangle
    faces = np.concatenate(
        [
            np.stack([I[:, 0], m[:, 0], m[:, 2]], axis=1),
            np.stack([I[:, 1], m[:, 1], m[:, 0]], axis=1),
            np.stack([I[:, 2], m[:, 2], m[:, 1]], axis=1),
            m,
        ]
    )
    mesh.positions = positions.astype(np.float32)
    mesh.uvs = uvs
    mesh.joints = joints
    mesh.weights = weights
    mesh.indices = faces.reshape(-1).astype(np.uint32)
    mesh.normals = smooth_normals(mesh.positions, mesh.indices)
    return mesh


def smooth_normals(positions, indices):
    """Area-weighted vertex normals over the welded topology, one normal per point."""
    P = positions.astype(np.float64)
    I = indices.reshape(-1, 3).astype(np.int64)
    weld = welded_ids(P)
    face = np.cross(P[I[:, 1]] - P[I[:, 0]], P[I[:, 2]] - P[I[:, 0]])
    acc = np.zeros((int(weld.max()) + 1, 3))
    for k in range(3):
        np.add.at(acc, weld[I[:, k]], face)
    acc /= np.maximum(np.linalg.norm(acc, axis=1, keepdims=True), 1e-12)
    return acc[weld].astype(np.float32)


def subdivide_garments(meshes, levels, inflate):
    """Loop-subdivide the garments `levels` times and push them out by `inflate` [m]."""
    for mesh in meshes:
        if mesh.kind not in SUBDIVIDE_KINDS or levels <= 0:
            continue
        before = len(mesh.indices) // 3
        for _ in range(levels):
            loop_once(mesh)
        if inflate:
            mesh.positions = (mesh.positions + mesh.normals * inflate).astype(np.float32)
        mesh.name = mesh.name  # unchanged; the log line below is the only trace
        print(f"  {mesh.name}: subdivided {before} -> {len(mesh.indices) // 3} triangles")


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
        # A group small against the budget keeps every triangle it has; the others
        # share what is left by size. The subdivided garments make the body group
        # huge, and the hair should not pay for that with its strands.
        whole = [g for g in groups if g.triangles <= budget * SMALL_GROUP_SHARE]
        targets = {g.name: g.triangles for g in whole}
        remaining = max(1, budget - sum(targets.values()))
        rest = [g for g in groups if g not in whole]
        rest_total = sum(g.triangles for g in rest) or 1
        for group in rest:
            share = shares[group.name] if len(rest) == len(groups) else group.triangles / rest_total
            targets[group.name] = min(group.triangles, max(1, round(remaining * share)))
        primitives = []
        for group in groups:
            indices = simplify(group, targets[group.name], level >= PRUNE_FROM_LOD)
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
    """`30000,6000,1600,500` → four non-increasing triangle budgets."""
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
    parser.add_argument("--lods", type=parse_lods, default=[30000, 6000, 1600, 500], help="triangle budgets per LOD")
    parser.add_argument(
        "--subdivide",
        type=int,
        default=SUBDIVIDE_LEVELS,
        help=f"Loop subdivision levels of the garments before the finest level of detail, default {SUBDIVIDE_LEVELS}; 0 = off",
    )
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
    binary = synthesize_walk(gltf, binary, skeleton)
    meshes = load_meshes(gltf, binary, meta)
    source_triangles = sum(len(m.indices) // 3 for m in meshes)

    baker = Baker(skeleton, meshes)
    rest = baker.pose({})
    ground = float(rest[:, 1].min())
    height = float(rest[:, 1].max() - ground)
    classify(meshes, baker, rest, ground, height)
    clip_shifts = ground_shifts(gltf, binary, baker)
    subdivide_garments(meshes, args.subdivide, SUBDIVIDE_INFLATE)

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
