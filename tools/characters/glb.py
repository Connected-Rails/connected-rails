"""glTF 2.0 binary (GLB) helpers shared by the character tools.

Reading: `read_glb` splits a GLB into its JSON and BIN chunks, `read_accessor`
turns an accessor into a numpy array (strided buffer views included) and
`read_image` hands back the encoded bytes of an embedded image.

Writing: `BufferBuilder` collects arrays into one 4-byte aligned buffer and
produces the matching `bufferViews` and `accessors`; `write_glb` wraps the
JSON and the buffer into a file.

Skinning maths: node transforms are (translation, rotation, uniform scale)
triples in glTF's convention, quaternions as (x, y, z, w). Matrices are
row-major numpy arrays acting on column vectors, so `M @ v` applies M; glTF
stores matrices column-major, which `ibm_to_matrices` / `matrices_to_ibm`
convert. `global_matrices` walks the node tree, `skin_positions` is linear
blend skinning and `clip_poses` samples every channel of an animation on the
union of its keyframe times so a clip can be baked frame by frame.
"""

import json
import struct

import numpy as np

GLB_MAGIC = b"glTF"
CHUNK_JSON = b"JSON"
CHUNK_BIN = b"BIN\x00"

ARRAY_BUFFER = 34962
ELEMENT_ARRAY_BUFFER = 34963

COMPONENT_DTYPES = {
    5120: np.int8,
    5121: np.uint8,
    5122: np.int16,
    5123: np.uint16,
    5125: np.uint32,
    5126: np.float32,
}
COMPONENT_TYPES = {np.dtype(dtype): code for code, dtype in COMPONENT_DTYPES.items()}
TYPE_COMPONENTS = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4, "MAT2": 4, "MAT3": 9, "MAT4": 16}
COMPONENTS_TYPE = {1: "SCALAR", 2: "VEC2", 3: "VEC3", 4: "VEC4", 16: "MAT4"}

IDENTITY_QUATERNION = np.array([0.0, 0.0, 0.0, 1.0])


# ---------------------------------------------------------------------------
# Reading
# ---------------------------------------------------------------------------


def read_glb(path):
    """Return `(gltf, binary)`: the parsed JSON chunk and the BIN chunk bytes."""
    with open(path, "rb") as handle:
        data = handle.read()
    magic, version, length = struct.unpack_from("<4sII", data, 0)
    if magic != GLB_MAGIC or version != 2:
        raise ValueError(f"{path}: not a glTF 2.0 binary")
    gltf, binary = None, b""
    offset = 12
    while offset < length:
        chunk_length, chunk_type = struct.unpack_from("<I4s", data, offset)
        offset += 8
        chunk = data[offset : offset + chunk_length]
        offset += chunk_length
        if chunk_type == CHUNK_JSON:
            gltf = json.loads(chunk)
        elif chunk_type == CHUNK_BIN:
            binary = chunk
    if gltf is None:
        raise ValueError(f"{path}: no JSON chunk")
    return gltf, binary


def read_accessor(gltf, binary, index):
    """Accessor `index` as an array of shape (count, components), or (count,) for SCALAR.

    Interleaved (strided) buffer views are unpacked; the returned array is an
    owned, contiguous copy. `normalized` integer data is returned unscaled.
    """
    accessor = gltf["accessors"][index]
    if "bufferView" not in accessor:
        raise ValueError(f"accessor {index}: sparse or zero-filled accessors are not supported")
    view = gltf["bufferViews"][accessor["bufferView"]]
    dtype = np.dtype(COMPONENT_DTYPES[accessor["componentType"]])
    components = TYPE_COMPONENTS[accessor["type"]]
    count = accessor["count"]
    offset = view.get("byteOffset", 0) + accessor.get("byteOffset", 0)
    stride = view.get("byteStride", dtype.itemsize * components)
    strided = np.ndarray(
        (count, components), dtype, buffer=binary, offset=offset, strides=(stride, dtype.itemsize)
    )
    array = np.array(strided)
    return array[:, 0] if components == 1 else array


def read_image(gltf, binary, index):
    """Encoded bytes (PNG, JPEG) of image `index`, which has to live in a buffer view."""
    image = gltf["images"][index]
    if "bufferView" not in image:
        raise ValueError(f"image {index}: only images embedded in the buffer are supported")
    view = gltf["bufferViews"][image["bufferView"]]
    start = view.get("byteOffset", 0)
    return binary[start : start + view["byteLength"]]


def node_parents(gltf):
    """Map of node index → parent node index for every node that has a parent."""
    parents = {}
    for index, node in enumerate(gltf["nodes"]):
        for child in node.get("children", []):
            parents[child] = index
    return parents


# ---------------------------------------------------------------------------
# Writing
# ---------------------------------------------------------------------------


class BufferBuilder:
    """Collects the binary data of one glTF buffer.

    Every buffer view starts on a 4-byte boundary and accessors are tightly
    packed with `byteOffset` 0, so alignment holds for any component type.
    """

    def __init__(self):
        self.views = []
        self.accessors = []
        self._chunks = []
        self._length = 0

    def add_view(self, data, target=None):
        """Append raw bytes as a buffer view; returns its index."""
        data = bytes(data)
        view = {"buffer": 0, "byteOffset": self._length, "byteLength": len(data)}
        if target is not None:
            view["target"] = target
        padding = -len(data) % 4
        self._chunks.append(data + b"\0" * padding)
        self._length += len(data) + padding
        self.views.append(view)
        return len(self.views) - 1

    def add_accessor(self, array, target=None, bounds=False):
        """Append `array` of shape (count,) or (count, components) as an accessor.

        `bounds` adds `min`/`max` (required on POSITION and animation inputs).
        Returns the accessor index.
        """
        array = np.ascontiguousarray(array)
        components = 1 if array.ndim == 1 else array.shape[1]
        accessor = {
            "bufferView": self.add_view(array.tobytes(), target),
            "componentType": COMPONENT_TYPES[array.dtype],
            "count": len(array),
            "type": COMPONENTS_TYPE[components],
        }
        if bounds:
            flat = array.reshape(len(array), -1)
            accessor["min"] = flat.min(axis=0).tolist()
            accessor["max"] = flat.max(axis=0).tolist()
        self.accessors.append(accessor)
        return len(self.accessors) - 1

    def binary(self):
        """The buffer contents so far."""
        return b"".join(self._chunks)


def write_glb(path, gltf, binary):
    """Write `gltf` (JSON dict) and `binary` (buffer 0) as a GLB; returns the file size."""
    json_bytes = json.dumps(gltf, separators=(",", ":")).encode("utf-8")
    json_bytes += b" " * (-len(json_bytes) % 4)
    binary = bytes(binary) + b"\0" * (-len(binary) % 4)
    length = 12 + 8 + len(json_bytes) + 8 + len(binary)
    with open(path, "wb") as handle:
        handle.write(struct.pack("<4sII", GLB_MAGIC, 2, length))
        handle.write(struct.pack("<I4s", len(json_bytes), CHUNK_JSON))
        handle.write(json_bytes)
        handle.write(struct.pack("<I4s", len(binary), CHUNK_BIN))
        handle.write(binary)
    return length


# ---------------------------------------------------------------------------
# Transforms
# ---------------------------------------------------------------------------


def quat_to_matrix(q):
    """Rotation matrices (..., 3, 3) of unit quaternions (..., 4) in x, y, z, w order."""
    q = np.asarray(q, dtype=np.float64)
    x, y, z, w = q[..., 0], q[..., 1], q[..., 2], q[..., 3]
    m = np.empty(q.shape[:-1] + (3, 3))
    m[..., 0, 0] = 1 - 2 * (y * y + z * z)
    m[..., 0, 1] = 2 * (x * y - z * w)
    m[..., 0, 2] = 2 * (x * z + y * w)
    m[..., 1, 0] = 2 * (x * y + z * w)
    m[..., 1, 1] = 1 - 2 * (x * x + z * z)
    m[..., 1, 2] = 2 * (y * z - x * w)
    m[..., 2, 0] = 2 * (x * z - y * w)
    m[..., 2, 1] = 2 * (y * z + x * w)
    m[..., 2, 2] = 1 - 2 * (x * x + y * y)
    return m


def quat_multiply(a, b):
    """Hamilton product a ⊗ b: rotate by b first, then by a (matches R(a) @ R(b))."""
    a = np.asarray(a, dtype=np.float64)
    b = np.asarray(b, dtype=np.float64)
    ax, ay, az, aw = a[..., 0], a[..., 1], a[..., 2], a[..., 3]
    bx, by, bz, bw = b[..., 0], b[..., 1], b[..., 2], b[..., 3]
    return np.stack(
        [
            aw * bx + ax * bw + ay * bz - az * by,
            aw * by - ax * bz + ay * bw + az * bx,
            aw * bz + ax * by - ay * bx + az * bw,
            aw * bw - ax * bx - ay * by - az * bz,
        ],
        axis=-1,
    )


def quat_rotate(q, v):
    """Rotate vectors v (..., 3) by quaternions q (..., 4), broadcasting over leading axes."""
    return np.einsum("...ij,...j->...i", quat_to_matrix(q), np.asarray(v, dtype=np.float64))


def trs_compose(a, b):
    """The transform "b, then a" as (translation, rotation, uniform scale) triples.

    Translations may carry leading axes (one triple per keyframe); the
    rotation and scale of `a` are applied to every translation of `b`.
    """
    ta, qa, sa = a
    tb, qb, sb = b
    return ta + quat_rotate(qa, np.asarray(tb, dtype=np.float64) * sa), quat_multiply(qa, qb), sa * sb


def node_trs(node):
    """(translation, rotation, uniform scale) of a node's own transform.

    Raises on a `matrix` or a non-uniform scale: neither can be folded into a
    joint that stays a TRS node.
    """
    if "matrix" in node:
        raise ValueError(f"node {node.get('name')!r}: matrix transforms cannot be folded into the skeleton")
    scale = np.asarray(node.get("scale", [1.0, 1.0, 1.0]), dtype=np.float64)
    if not np.allclose(scale, scale[0]):
        raise ValueError(f"node {node.get('name')!r}: non-uniform scale cannot be folded into the skeleton")
    translation = np.asarray(node.get("translation", [0.0, 0.0, 0.0]), dtype=np.float64)
    rotation = np.asarray(node.get("rotation", IDENTITY_QUATERNION), dtype=np.float64)
    return translation, rotation, float(scale[0])


def node_matrix(node, translation=None, rotation=None, scale=None):
    """Local 4×4 matrix of a node, with optional animated TRS overrides."""
    if "matrix" in node and translation is None and rotation is None and scale is None:
        return np.asarray(node["matrix"], dtype=np.float64).reshape(4, 4).T
    if translation is None:
        translation = node.get("translation", [0.0, 0.0, 0.0])
    if rotation is None:
        rotation = node.get("rotation", IDENTITY_QUATERNION)
    if scale is None:
        scale = node.get("scale", [1.0, 1.0, 1.0])
    m = np.eye(4)
    m[:3, :3] = quat_to_matrix(rotation) * np.asarray(scale, dtype=np.float64)
    m[:3, 3] = translation
    return m


def global_matrices(gltf, parents, overrides=None):
    """World matrices (nodes, 4, 4) of every node.

    `overrides` maps a node index to a dict with any of `translation`,
    `rotation`, `scale` — the pose of an animation frame.
    """
    overrides = overrides or {}
    nodes = gltf["nodes"]
    world = np.zeros((len(nodes), 4, 4))
    done = np.zeros(len(nodes), dtype=bool)

    def compute(index):
        if not done[index]:
            local = node_matrix(nodes[index], **overrides.get(index, {}))
            world[index] = compute(parents[index]) @ local if index in parents else local
            done[index] = True
        return world[index]

    for index in range(len(nodes)):
        compute(index)
    return world


def ibm_to_matrices(flat):
    """glTF inverse bind matrices (n, 16) column-major → row-major (n, 4, 4)."""
    return np.asarray(flat, dtype=np.float64).reshape(-1, 4, 4).transpose(0, 2, 1)


def matrices_to_ibm(matrices):
    """Row-major (n, 4, 4) → glTF (n, 16) float32 column-major."""
    return np.ascontiguousarray(np.asarray(matrices).transpose(0, 2, 1).reshape(-1, 16), dtype=np.float32)


def skin_positions(joint_matrices, positions, joints, weights):
    """Linear blend skinning: world positions (n, 3) of bind-pose `positions`.

    `joint_matrices` are `world[joint] @ inverse_bind[joint]` per skin joint,
    `joints` (n, 4) index into them, `weights` (n, 4) blend them.
    """
    positions = np.asarray(positions, dtype=np.float64)
    weights = np.asarray(weights, dtype=np.float64)
    joints = np.asarray(joints, dtype=np.int64)
    homogeneous = np.concatenate([positions, np.ones((len(positions), 1))], axis=1)
    out = np.zeros((len(positions), 3))
    for k in range(4):
        m = joint_matrices[joints[:, k], :3, :]
        out += weights[:, k : k + 1] * np.einsum("nij,nj->ni", m, homogeneous)
    return out


# ---------------------------------------------------------------------------
# Animation sampling
# ---------------------------------------------------------------------------


def _resample(keys, values, times, step, rotation):
    """Values of one channel at `times`; LINEAR (or STEP) between keyframes."""
    if rotation:
        # Keep neighbouring quaternions in the same hemisphere so interpolation
        # takes the short way round.
        dots = np.sum(values[1:] * values[:-1], axis=1)
        flips = np.cumprod(np.where(dots < 0, -1.0, 1.0))
        values = values.copy()
        values[1:] *= flips[:, None]
    if step:
        index = np.clip(np.searchsorted(keys, times, side="right") - 1, 0, len(keys) - 1)
        out = values[index]
    else:
        out = np.stack([np.interp(times, keys, values[:, c]) for c in range(values.shape[1])], axis=1)
    if rotation:
        out /= np.linalg.norm(out, axis=1, keepdims=True)
    return out


def clip_poses(gltf, binary, animation):
    """Sample every channel of `animation` on the union of its keyframe times.

    Returns `(times, poses)`: `poses[frame]` is the overrides dict that
    `global_matrices` takes. CUBICSPLINE samplers are not supported.
    """
    samplers = animation["samplers"]
    inputs = {}
    for sampler in samplers:
        if sampler["input"] not in inputs:
            inputs[sampler["input"]] = read_accessor(gltf, binary, sampler["input"]).astype(np.float64)
    times = np.unique(np.concatenate(list(inputs.values())))
    poses = [{} for _ in times]
    for channel in animation["channels"]:
        target = channel["target"]
        path = target["path"]
        if "node" not in target or path == "weights":
            continue
        sampler = samplers[channel["sampler"]]
        interpolation = sampler.get("interpolation", "LINEAR")
        if interpolation == "CUBICSPLINE":
            raise ValueError(f"animation {animation.get('name')!r}: CUBICSPLINE samplers are not supported")
        values = read_accessor(gltf, binary, sampler["output"]).astype(np.float64)
        sampled = _resample(inputs[sampler["input"]], values, times, interpolation == "STEP", path == "rotation")
        for frame, value in enumerate(sampled):
            poses[frame].setdefault(target["node"], {})[path] = value
    return times, poses
