"""Motion capture for the characters: reading BVH files and putting a recorded
person's motion onto the game rig.

The sources (`clips.json`) are recordings of real people on skeletons that are
not ours: other joint names, other proportions, and rest poses that are a
T-pose in one file and, in MotionBuilder's exports, a heap of pre-rotations
baked into the joint offsets in another. The retargeting therefore does not
trust any rest pose. For every bone it wants two anatomical directions — the
bone itself, and a twist reference that means the same thing on both skeletons
(the elbow's hinge axis for the arm, the knee's for the leg, the way the body
faces for the trunk) — and it reads those off the *motion*: where the bones
point over the frames, and about which axis the elbows and knees bend. That
gives each source joint a constant anatomical frame in its own local
coordinates (`SourceRig.calibrate`); the game rig's frames come out of its
rest pose the same way (`TargetRig`). A frame of animation then maps as
`R_target = R_source · C_source · C_targetᵀ` — the target joint turns so that
its anatomical frame coincides with the source's — and the local rotations
the glTF needs follow from the parents. Bone lengths stay the character's own;
only the pelvis travels, scaled by the ratio of the leg lengths.

`walk_cycle` finds one steady gait cycle in a recording of walking and its
natural pace, `idle_loop` a stretch of an idle that closes on itself, and
`seam` makes either a loop the eye cannot see the join of.

Conventions shared with `build_character.py`: Y up, quaternions (x, y, z, w),
matrices act on column vectors, the raw MakeHuman export faces +Z — which is
also where the sources' actors face when a clip is aligned.
"""

from __future__ import annotations

from dataclasses import dataclass, field

import numpy as np

import glb

UP = np.array([0.0, 1.0, 0.0])
FORWARD = np.array([0.0, 0.0, 1.0])

#: The game rig's joints that carry motion, with the child (or `None` for the
#: joint's own +Y) that says which way the bone points, and the kind of twist
#: reference the bone uses.
TARGET_BONES = {
    "pelvis": ("spine_01", "trunk"),
    "spine_01": ("spine_02", "trunk"),
    "spine_02": ("spine_03", "trunk"),
    "spine_03": ("neck_01", "trunk"),
    "neck_01": ("head", "trunk"),
    "head": (None, "trunk"),
    "clavicle_l": ("upperarm_l", "trunk"),
    "clavicle_r": ("upperarm_r", "trunk"),
    "upperarm_l": ("lowerarm_l", "arm_l"),
    "upperarm_r": ("lowerarm_r", "arm_r"),
    "lowerarm_l": ("hand_l", "arm_l"),
    "lowerarm_r": ("hand_r", "arm_r"),
    "hand_l": ("middle_01_l", "arm_l"),
    "hand_r": ("middle_01_r", "arm_r"),
    "thigh_l": ("calf_l", "leg_l"),
    "thigh_r": ("calf_r", "leg_r"),
    "calf_l": ("foot_l", "leg_l"),
    "calf_r": ("foot_r", "leg_r"),
    "foot_l": ("ball_l", "leg_l"),
    "foot_r": ("ball_r", "leg_r"),
    "ball_l": (None, "leg_l"),
    "ball_r": (None, "leg_r"),
}
UPPER_BODY = [
    "spine_01", "spine_02", "spine_03", "neck_01", "head", "clavicle_l", "clavicle_r",
    "upperarm_l", "upperarm_r", "lowerarm_l", "lowerarm_r", "hand_l", "hand_r",
]
#: A hinge is read off frames bent at least this much [deg].
HINGE_BENT = 20.0
#: A walk cycle is cut from a stretch walked at least this share as fast as
#: the fastest stretch of the recording, so a slow turn is not taken for the
#: walk.
BRISK_SHARE = 0.8


# ---------------------------------------------------------------------------
# BVH
# ---------------------------------------------------------------------------


@dataclass
class BvhJoint:
    name: str
    parent: int | None
    offset: np.ndarray
    channels: list[str]
    end_site: np.ndarray | None = None
    children: list[int] = field(default_factory=list)


@dataclass
class Bvh:
    joints: list[BvhJoint]
    frame_time: float
    scale: float
    #: World rotations (frames, joints, 4) and positions (frames, joints, 3) [m].
    rotations: np.ndarray
    positions: np.ndarray

    @property
    def frames(self) -> int:
        return len(self.rotations)

    @property
    def fps(self) -> float:
        return 1.0 / self.frame_time

    def index(self, name: str) -> int:
        for i, joint in enumerate(self.joints):
            if joint.name == name:
                return i
        raise KeyError(name)

    def local_direction(self, index: int, towards: str | None = None) -> np.ndarray:
        """Unit vector of the bone in the joint's own frame: towards the joint
        `towards`, else the first child, else the end site. Read off the motion,
        so it holds for a joint further down the tree as well (MotionBuilder puts
        a fixed-angle joint between hips and spine)."""
        joint = self.joints[index]
        if towards is None and not joint.children and joint.end_site is not None:
            vector = joint.end_site
        else:
            other = self.index(towards) if towards is not None else joint.children[0]
            world = self.positions[:, other] - self.positions[:, index]
            vector = glb.quat_rotate(quat_conjugate(self.rotations[:, index]), world).mean(axis=0)
        norm = np.linalg.norm(vector)
        if norm < 1e-9:
            raise ValueError(f"{joint.name}: zero-length bone")
        return vector / norm

    def slice(self, start: int, stop: int) -> "Bvh":
        """The frames `start` … `stop` (exclusive) as a clip of their own."""
        return Bvh(self.joints, self.frame_time, self.scale, self.rotations[start:stop], self.positions[start:stop])


_AXIS = {"X": 0, "Y": 1, "Z": 2}


def axis_quaternion(axis: int, radians) -> np.ndarray:
    """Quaternions (n, 4) of rotations about a coordinate axis."""
    half = np.asarray(radians, dtype=np.float64).reshape(-1) * 0.5
    q = np.zeros((len(half), 4))
    q[:, axis] = np.sin(half)
    q[:, 3] = np.cos(half)
    return q


def read_bvh(path, scale: float = 0.01) -> Bvh:
    """Read a BVH file; `scale` is metres per file unit.

    The root's OFFSET is ignored: the specification adds it to the position
    channels, MotionBuilder's exports carry absolute positions in the channels
    and something else in the offset, and every source in `clips.json` is of
    the second kind or has a zero root offset.
    """
    with open(path, encoding="utf-8", errors="replace") as handle:
        tokens = handle.read().split()
    joints: list[BvhJoint] = []
    stack: list[int] = []
    pos = 0

    def take() -> str:
        nonlocal pos
        token = tokens[pos]
        pos += 1
        return token

    def take_vector() -> np.ndarray:
        return np.array([float(take()) for _ in range(3)])

    if take() != "HIERARCHY":
        raise ValueError(f"{path}: not a BVH file")
    while True:
        token = take()
        if token in ("ROOT", "JOINT"):
            name = take()
            if take() != "{":
                raise ValueError(f"{path}: '{{' expected after {name}")
            parent = stack[-1] if stack else None
            joints.append(BvhJoint(name, parent, np.zeros(3), []))
            index = len(joints) - 1
            if parent is not None:
                joints[parent].children.append(index)
            stack.append(index)
        elif token == "OFFSET":
            joints[stack[-1]].offset = take_vector()
        elif token == "CHANNELS":
            count = int(take())
            joints[stack[-1]].channels = [take() for _ in range(count)]
        elif token == "End":
            take()  # "Site"
            if take() != "{" or take() != "OFFSET":
                raise ValueError(f"{path}: malformed End Site")
            joints[stack[-1]].end_site = take_vector()
            if take() != "}":
                raise ValueError(f"{path}: '}}' expected after End Site")
        elif token == "}":
            stack.pop()
        elif token == "MOTION":
            break
        else:
            raise ValueError(f"{path}: unexpected token {token!r}")
    if take() != "Frames:":
        raise ValueError(f"{path}: 'Frames:' expected")
    frames = int(take())
    if take() != "Frame" or take() != "Time:":
        raise ValueError(f"{path}: 'Frame Time:' expected")
    frame_time = float(take())
    channel_count = sum(len(j.channels) for j in joints)
    values = np.array(tokens[pos : pos + frames * channel_count], dtype=np.float64)
    if len(values) != frames * channel_count:
        raise ValueError(f"{path}: {len(values)} motion values for {frames} frames of {channel_count} channels")
    motion = values.reshape(frames, channel_count)

    rotations = np.zeros((frames, len(joints), 4))
    positions = np.zeros((frames, len(joints), 3))
    column = 0
    identity = np.tile(glb.IDENTITY_QUATERNION, (frames, 1))
    for index, joint in enumerate(joints):
        local_rotation = identity.copy()
        translation = np.zeros((frames, 3)) if joint.parent is None else np.tile(joint.offset, (frames, 1))
        for channel in joint.channels:
            data = motion[:, column]
            column += 1
            if channel.endswith("position"):
                translation[:, _AXIS[channel[0]]] += data
            elif channel.endswith("rotation"):
                # Channels listed first are applied last: R = R_first · R_second · R_third.
                local_rotation = glb.quat_multiply(local_rotation, axis_quaternion(_AXIS[channel[0]], np.radians(data)))
            else:
                raise ValueError(f"{path}: unknown channel {channel!r}")
        if joint.parent is None:
            rotations[:, index] = local_rotation
            positions[:, index] = translation * scale
        else:
            parent_rotation = rotations[:, joint.parent]
            rotations[:, index] = glb.quat_multiply(parent_rotation, local_rotation)
            positions[:, index] = positions[:, joint.parent] + glb.quat_rotate(parent_rotation, translation) * scale
    return Bvh(joints, frame_time, scale, rotations, positions)


# ---------------------------------------------------------------------------
# Rotation helpers on arrays
# ---------------------------------------------------------------------------


def quat_conjugate(q) -> np.ndarray:
    return np.asarray(q, dtype=np.float64) * np.array([-1.0, -1.0, -1.0, 1.0])


def quat_normalize(q) -> np.ndarray:
    q = np.asarray(q, dtype=np.float64)
    return q / np.linalg.norm(q, axis=-1, keepdims=True)


def quat_from_matrix(m) -> np.ndarray:
    """Unit quaternions (..., 4) of rotation matrices (..., 3, 3)."""
    m = np.asarray(m, dtype=np.float64)
    shape = m.shape[:-2]
    m = m.reshape(-1, 3, 3)
    # Shepperd's method, all four branches evaluated and the best-conditioned one picked.
    t0 = 1.0 + m[:, 0, 0] + m[:, 1, 1] + m[:, 2, 2]
    t1 = 1.0 + m[:, 0, 0] - m[:, 1, 1] - m[:, 2, 2]
    t2 = 1.0 - m[:, 0, 0] + m[:, 1, 1] - m[:, 2, 2]
    t3 = 1.0 - m[:, 0, 0] - m[:, 1, 1] + m[:, 2, 2]
    q = np.empty((len(m), 4))
    choice = np.argmax(np.stack([t0, t1, t2, t3], axis=1), axis=1)
    for k, t in enumerate((t0, t1, t2, t3)):
        pick = choice == k
        if not pick.any():
            continue
        s = np.sqrt(np.maximum(t[pick], 1e-12)) * 2
        a = m[pick]
        if k == 0:
            q[pick] = np.stack([(a[:, 2, 1] - a[:, 1, 2]) / s, (a[:, 0, 2] - a[:, 2, 0]) / s, (a[:, 1, 0] - a[:, 0, 1]) / s, 0.25 * s], axis=1)
        elif k == 1:
            q[pick] = np.stack([0.25 * s, (a[:, 0, 1] + a[:, 1, 0]) / s, (a[:, 0, 2] + a[:, 2, 0]) / s, (a[:, 2, 1] - a[:, 1, 2]) / s], axis=1)
        elif k == 2:
            q[pick] = np.stack([(a[:, 0, 1] + a[:, 1, 0]) / s, 0.25 * s, (a[:, 1, 2] + a[:, 2, 1]) / s, (a[:, 0, 2] - a[:, 2, 0]) / s], axis=1)
        else:
            q[pick] = np.stack([(a[:, 0, 2] + a[:, 2, 0]) / s, (a[:, 1, 2] + a[:, 2, 1]) / s, 0.25 * s, (a[:, 1, 0] - a[:, 0, 1]) / s], axis=1)
    return quat_normalize(q).reshape(shape + (4,))


def quat_slerp(a, b, t) -> np.ndarray:
    """Spherical interpolation between quaternion arrays; `t` broadcasts over the leading axes."""
    a = np.asarray(a, dtype=np.float64)
    b = np.asarray(b, dtype=np.float64)
    t = np.asarray(t, dtype=np.float64)[..., None]
    dot = np.sum(a * b, axis=-1, keepdims=True)
    b = np.where(dot < 0, -b, b)
    dot = np.abs(dot)
    theta = np.arccos(np.clip(dot, -1.0, 1.0))
    sin_theta = np.sin(theta)
    near = sin_theta < 1e-6
    safe = np.where(near, 1.0, sin_theta)
    wa = np.where(near, 1.0 - t, np.sin((1.0 - t) * theta) / safe)
    wb = np.where(near, t, np.sin(t * theta) / safe)
    return quat_normalize(wa * a + wb * b)


def quat_angle(a, b) -> np.ndarray:
    """Angle [rad] between rotations, elementwise over leading axes."""
    dot = np.abs(np.sum(np.asarray(a) * np.asarray(b), axis=-1))
    return 2.0 * np.arccos(np.clip(dot, -1.0, 1.0))


def hemisphere(q) -> np.ndarray:
    """Flip quaternions along the first axis so neighbours are in one hemisphere."""
    q = np.array(q, dtype=np.float64)
    for k in range(1, len(q)):
        flip = np.sum(q[k] * q[k - 1], axis=-1) < 0
        q[k] = np.where(flip[..., None], -q[k], q[k])
    return q


def frame_from(direction, reference) -> np.ndarray:
    """Orthonormal 3×3 frame (columns) with `direction` first and `reference`, made
    perpendicular, second. The same two anatomical directions on two rigs give
    two frames whose difference is the retargeting rotation."""
    x = np.asarray(direction, dtype=np.float64)
    x = x / np.linalg.norm(x)
    y = np.asarray(reference, dtype=np.float64) - np.dot(reference, x) * x
    norm = np.linalg.norm(y)
    if norm < 1e-6:
        raise ValueError("twist reference is parallel to the bone")
    y = y / norm
    return np.stack([x, y, np.cross(x, y)], axis=1)


def smoothstep(t) -> np.ndarray:
    t = np.clip(np.asarray(t, dtype=np.float64), 0.0, 1.0)
    return t * t * (3.0 - 2.0 * t)


def moving_average(values, window: int) -> np.ndarray:
    """Centred moving average along the first axis, shortened at the edges."""
    values = np.asarray(values, dtype=np.float64)
    if window <= 1:
        return values
    out = np.empty_like(values)
    half = window // 2
    for k in range(len(values)):
        out[k] = values[max(0, k - half) : k + half + 1].mean(axis=0)
    return out


def unit(v) -> np.ndarray:
    v = np.asarray(v, dtype=np.float64)
    return v / np.maximum(np.linalg.norm(v, axis=-1, keepdims=True), 1e-12)


def yaw_to_forward(direction) -> float:
    """Angle [rad] about Y that turns the horizontal `direction` onto +Z: a turn
    by θ about +Y takes +Z to (sin θ, 0, cos θ), so a direction at azimuth φ
    needs −φ."""
    d = np.asarray(direction, dtype=np.float64)
    yaw = -float(np.arctan2(d[0], d[2]))
    turned = glb.quat_rotate(axis_quaternion(1, [yaw])[0], np.array([d[0], 0.0, d[2]]))
    if turned[2] <= 0.0 or abs(turned[0]) > 1e-6 * max(np.linalg.norm(d), 1.0):
        raise AssertionError(f"yaw {yaw} does not turn {d} onto +Z: {turned}")
    return yaw


# ---------------------------------------------------------------------------
# Rigs: anatomical frames per bone
# ---------------------------------------------------------------------------


def _hinge_axis(rotations, upper_dir, lower_dir) -> np.ndarray:
    """Local axis of a hinge (elbow, knee) in the upper bone's parent's… no: in
    each frame's world, then averaged in the frame of `rotations` — weighted by
    how bent the hinge is, which is where the axis can be seen."""
    cross = np.cross(upper_dir, lower_dir)
    sin_angle = np.linalg.norm(cross, axis=-1)
    weight = np.where(sin_angle >= np.sin(np.radians(HINGE_BENT)), sin_angle, 0.0)
    if weight.sum() < 1e-6:
        raise ValueError("hinge never bends in the calibration clips")
    axis_world = unit(cross) * weight[:, None]
    local = glb.quat_rotate(quat_conjugate(rotations), axis_world)
    return unit(local.sum(axis=0))


class SourceRig:
    """A mocap skeleton with its joint map onto the game rig and, once
    calibrated, the anatomical frame of every mapped joint."""

    def __init__(self, joints: dict[str, str | list], scale: float = 0.01):
        # target joint → (source joint, direction child or None)
        self.map: dict[str, tuple[str, str | None]] = {}
        for target, source in joints.items():
            if isinstance(source, str):
                self.map[target] = (source, None)
            else:
                self.map[target] = (source[0], source[1] if len(source) > 1 else None)
        self.scale = scale
        self.frames: dict[str, np.ndarray] = {}
        self.leg_length: float | None = None

    def source_index(self, bvh: Bvh, target: str) -> int:
        return bvh.index(self.map[target][0])

    def bone_direction(self, bvh: Bvh, target: str) -> np.ndarray:
        """World unit vectors (frames, 3) of the source bone mapped to `target`."""
        source, towards = self.map[target]
        index = bvh.index(source)
        return glb.quat_rotate(bvh.rotations[:, index], bvh.local_direction(index, towards))

    def facing(self, bvh: Bvh, left: str, right: str) -> np.ndarray:
        """Horizontal unit vectors (frames, 3) the body faces: across the line
        between two bilateral joints, left to right, turned to the front."""
        across = bvh.positions[:, self.source_index(bvh, left)] - bvh.positions[:, self.source_index(bvh, right)]
        forward = np.cross(across, UP)
        forward[:, 1] = 0.0
        return unit(forward)

    def references(self, bvh: Bvh) -> dict[str, np.ndarray]:
        """World twist references (frames, 3) per kind for one clip."""
        refs = {"trunk": self.facing(bvh, "thigh_l", "thigh_r"), "chest": self.facing(bvh, "clavicle_l", "clavicle_r")}
        for side in ("l", "r"):
            refs[f"arm_{side}"] = (self.bone_direction(bvh, f"upperarm_{side}"), self.bone_direction(bvh, f"lowerarm_{side}"))
            refs[f"leg_{side}"] = (self.bone_direction(bvh, f"thigh_{side}"), self.bone_direction(bvh, f"calf_{side}"))
        return refs

    def calibrate(self, clips: list[Bvh]) -> None:
        """Read the anatomical frames off the motion in `clips` (a walk and a
        stand of the actor do; the hinges must bend somewhere in them)."""
        rotations: dict[str, list[np.ndarray]] = {t: [] for t in TARGET_BONES}
        world_refs: dict[str, list] = {t: [] for t in TARGET_BONES}
        for bvh in clips:
            refs = self.references(bvh)
            for target, (_, kind) in TARGET_BONES.items():
                index = self.source_index(bvh, target)
                rotations[target].append(bvh.rotations[:, index])
                if kind == "trunk":
                    facing = refs["chest"] if target in ("spine_03", "neck_01", "head", "clavicle_l", "clavicle_r") else refs["trunk"]
                    world_refs[target].append(facing)
                else:
                    world_refs[target].append(refs[kind])
        for target, (_, kind) in TARGET_BONES.items():
            source, towards = self.map[target]
            bvh = clips[0]
            direction = bvh.local_direction(bvh.index(source), towards)
            rotation = np.concatenate(rotations[target])
            if kind == "trunk":
                facing = np.concatenate(world_refs[target])
                reference = unit(glb.quat_rotate(quat_conjugate(rotation), facing).sum(axis=0))
            else:
                upper = np.concatenate([u for u, _ in world_refs[target]])
                lower = np.concatenate([l for _, l in world_refs[target]])
                reference = _hinge_axis(rotation, upper, lower)
            self.frames[target] = frame_from(direction, reference)
        bvh = clips[0]
        knee = bvh.joints[self.source_index(bvh, "calf_l")].offset
        ankle = bvh.joints[self.source_index(bvh, "foot_l")].offset
        self.leg_length = float((np.linalg.norm(knee) + np.linalg.norm(ankle)) * bvh.scale)


class TargetRig:
    """The game rig out of a raw MakeHuman export: rest world transforms and the
    anatomical frame of every bone of `TARGET_BONES`."""

    def __init__(self, gltf, skeleton):
        self.gltf = gltf
        self.skeleton = skeleton
        nodes = gltf["nodes"]
        self.by_name = {nodes[j].get("name"): j for j in skeleton.joints}
        missing = [name for name in TARGET_BONES if name not in self.by_name]
        if missing:
            raise ValueError(f"rig lacks joints {missing}")
        self.world = glb.global_matrices(gltf, skeleton.parents, {})
        self.rest_rotation = {name: self.world[index][:3, :3].copy() for name, index in self.by_name.items()}
        self.position = {name: self.world[index][:3, 3].copy() for name, index in self.by_name.items()}
        directions = {}
        for name, (child, _) in TARGET_BONES.items():
            if child is None:
                directions[name] = unit(self.rest_rotation[name] @ UP)
            else:
                directions[name] = unit(self.position[child] - self.position[name])
        hinges = {}
        for side in ("l", "r"):
            hinges[f"arm_{side}"] = unit(np.cross(directions[f"upperarm_{side}"], directions[f"lowerarm_{side}"]))
            knee = np.cross(directions[f"thigh_{side}"], directions[f"calf_{side}"])
            if np.linalg.norm(knee) < np.sin(np.radians(2.0)):
                knee = np.array([1.0, 0.0, 0.0])  # a straight rest knee: it bends about the sideways axis
            hinges[f"leg_{side}"] = unit(knee)
        self.frames = {}
        for name, (_, kind) in TARGET_BONES.items():
            reference = FORWARD if kind == "trunk" else hinges[kind]
            self.frames[name] = self.rest_rotation[name].T @ frame_from(directions[name], reference)
        self.leg_length = float(
            np.linalg.norm(self.position["calf_l"] - self.position["thigh_l"])
            + np.linalg.norm(self.position["foot_l"] - self.position["calf_l"])
        )
        self.parent_name = {}
        for name, index in self.by_name.items():
            parent = skeleton.parents.get(index)
            self.parent_name[name] = nodes[parent].get("name") if parent is not None and parent in set(skeleton.joints) else None


# ---------------------------------------------------------------------------
# Motion on the target rig
# ---------------------------------------------------------------------------


@dataclass
class Motion:
    """A clip on the game rig: local rotations per animated joint, the pelvis'
    local translation, and the frame times."""

    times: np.ndarray
    rotations: dict[str, np.ndarray]
    pelvis_translation: np.ndarray
    #: Natural pace over the ground [m/s] for a walk, 0 otherwise.
    pace: float = 0.0

    @property
    def frames(self) -> int:
        return len(self.times)

    def copy(self) -> "Motion":
        return Motion(self.times.copy(), {k: v.copy() for k, v in self.rotations.items()}, self.pelvis_translation.copy(), self.pace)


def retarget(bvh: Bvh, source: SourceRig, target: TargetRig, yaw: float = 0.0, translation: str = "centre") -> Motion:
    """Put the frames of `bvh` onto the target rig.

    `yaw` turns the whole clip about Y first (so a walk goes towards +Z);
    `translation` says what the pelvis does: `"centre"` keeps its excursions
    about the clip's mean, `"in-place"` also takes the steady travel out — a
    walk cycle that stays where it is — and `"none"` pins it to the rest.
    """
    if not source.frames:
        raise ValueError("source rig is not calibrated")
    turn = axis_quaternion(1, [yaw])[0]
    world = {}
    for name in TARGET_BONES:
        index = source.source_index(bvh, name)
        rotation = glb.quat_to_matrix(glb.quat_multiply(turn, bvh.rotations[:, index]))
        world[name] = rotation @ (source.frames[name] @ target.frames[name].T)
    rotations = {}
    for name in TARGET_BONES:
        parent = target.parent_name[name]
        if parent in world:
            above = world[parent]
        elif parent is not None:
            above = target.rest_rotation[parent][None]
        else:
            above = np.eye(3)[None]
        local = np.transpose(above, (0, 2, 1)) @ world[name]
        rotations[name] = hemisphere(quat_from_matrix(local))
    # The pelvis travels as the actor's hips do, scaled to the character's legs.
    hips = glb.quat_rotate(turn, bvh.positions[:, source.source_index(bvh, "pelvis")])
    scale = target.leg_length / source.leg_length
    if translation == "none":
        excursion = np.zeros_like(hips)
    else:
        excursion = hips - hips.mean(axis=0)
        if translation == "in-place":
            t = np.arange(len(hips))[:, None]
            trend = (hips[-1] - hips[0]) / max(len(hips) - 1, 1)
            excursion = excursion - (t - (len(hips) - 1) / 2.0) * trend * np.array([1.0, 0.0, 1.0])
    pelvis_world = target.position["pelvis"] + scale * excursion
    root = target.parent_name["pelvis"]
    root_matrix = target.world[target.by_name[root]] if root is not None else np.eye(4)
    inverse = np.linalg.inv(root_matrix)
    homogeneous = np.concatenate([pelvis_world, np.ones((len(pelvis_world), 1))], axis=1)
    pelvis_local = (inverse @ homogeneous.T).T[:, :3]
    times = np.arange(bvh.frames) * bvh.frame_time
    return Motion(times, rotations, pelvis_local)


def resample(motion: Motion, fps: float) -> Motion:
    """The motion at `fps`, keeping the span; rotations slerped, translations lerped."""
    duration = motion.times[-1]
    count = max(int(round(duration * fps)), 1)
    times = np.linspace(0.0, duration, count + 1)
    index = np.clip(np.searchsorted(motion.times, times, side="right") - 1, 0, motion.frames - 2)
    span = motion.times[index + 1] - motion.times[index]
    t = np.where(span > 0, (times - motion.times[index]) / np.where(span > 0, span, 1.0), 0.0)
    rotations = {name: quat_slerp(q[index], q[index + 1], t) for name, q in motion.rotations.items()}
    translation = motion.pelvis_translation
    pelvis = translation[index] + (translation[index + 1] - translation[index]) * t[:, None]
    return Motion(times, rotations, pelvis, motion.pace)


def seam(motion: Motion, blend_seconds: float) -> Motion:
    """Make the clip loop: the last frame is made equal to the first, the
    difference worked off over the last `blend_seconds` (the whole clip when
    that is longer) so no frame jumps."""
    n = motion.frames - 1
    if n < 1:
        return motion
    blend = min(blend_seconds, motion.times[-1])
    start = motion.times[-1] - blend
    weight = smoothstep((motion.times - start) / blend) if blend > 0 else np.where(motion.times >= start, 1.0, 0.0)
    out = motion.copy()
    for name, q in motion.rotations.items():
        delta = glb.quat_multiply(quat_conjugate(q[n]), q[0])  # right-multiplied: q[n] · delta = q[0]
        identity = np.tile(glb.IDENTITY_QUATERNION, (len(q), 1))
        step = quat_slerp(identity, np.tile(delta, (len(q), 1)), weight)
        out.rotations[name] = hemisphere(glb.quat_multiply(q, step))
    delta = motion.pelvis_translation[0] - motion.pelvis_translation[n]
    out.pelvis_translation = motion.pelvis_translation + weight[:, None] * delta
    return out


def seat(upper: Motion, chair: dict[str, np.ndarray], pelvis_translation: np.ndarray) -> Motion:
    """A seated clip: the upper body of `upper` over the legs of the chair pose
    (`chair` maps joint name → local rotation; joints not in it keep the rest,
    which the caller passes in the map as well)."""
    rotations = {}
    for name in TARGET_BONES:
        if name in UPPER_BODY:
            rotations[name] = upper.rotations[name].copy()
        else:
            rotations[name] = np.tile(chair[name], (upper.frames, 1))
    return Motion(upper.times.copy(), rotations, np.tile(pelvis_translation, (upper.frames, 1)))


# ---------------------------------------------------------------------------
# Finding the useful stretch of a recording
# ---------------------------------------------------------------------------


@dataclass
class Cycle:
    start: int
    stop: int  # inclusive: the frame that closes the cycle
    heading: np.ndarray
    pace: float


def walk_cycle(bvh: Bvh, source: SourceRig, window_seconds: float = 3.0) -> Cycle:
    """One gait cycle — left heel strike to left heel strike — out of the
    steadiest straight stretch of brisk walking in the recording, with the
    direction walked and the pace over the ground [m/s, source scale]."""
    fps = bvh.fps
    hips = bvh.positions[:, source.source_index(bvh, "pelvis")]
    velocity = np.gradient(moving_average(hips, max(int(fps * 0.25), 1)), axis=0) * fps
    velocity[:, 1] = 0.0
    speed = np.linalg.norm(velocity, axis=1)
    heading = np.arctan2(velocity[:, 0], velocity[:, 2])
    window = max(int(window_seconds * fps), 2)
    margin = int(fps)
    windows = []
    for start in range(margin, max(bvh.frames - window - margin, margin) + 1, max(int(fps * 0.25), 1)):
        stop = start + window
        if stop > bvh.frames:
            break
        s = speed[start:stop]
        if s.mean() < 0.3:
            continue
        h = np.unwrap(heading[start:stop])
        # Straight and steady: the spread of the heading counts double, a curve
        # walked in this window would put the cut cycle off its own line.
        windows.append((2.0 * np.std(h) + 0.5 * np.std(s) / s.mean(), s.mean(), start, stop))
    if not windows:
        raise ValueError("no stretch of steady walking found")
    brisk = BRISK_SHARE * max(w[1] for w in windows)
    _, _, start, stop = min((w for w in windows if w[1] >= brisk), key=lambda w: w[0])
    direction = unit(velocity[start:stop].mean(axis=0))
    # Heel strikes: the left foot at its most forward relative to the hips.
    ankle = bvh.positions[:, source.source_index(bvh, "foot_l")]
    forward = np.sum((ankle - hips)[start:stop] * direction, axis=1)
    forward = moving_average(forward, max(int(fps * 0.05), 1))
    strikes = [k for k in range(1, len(forward) - 1) if forward[k] >= forward[k - 1] and forward[k] > forward[k + 1]]
    strikes = [k for k in strikes if forward[k] > np.percentile(forward, 60)]
    if len(strikes) < 2:
        raise ValueError("fewer than two heel strikes in the steady stretch")
    periods = np.diff(strikes)
    median = np.median(periods)
    middle = len(forward) / 2
    choice = min(
        (k for k in range(len(periods)) if abs(periods[k] - median) <= 0.15 * median),
        key=lambda k: abs(strikes[k] + periods[k] / 2 - middle),
        default=int(np.argmin(np.abs(periods - median))),
    )
    a, b = start + strikes[choice], start + strikes[choice + 1]
    # The cycle's own line, not the window's mean: an actor who curves through the
    # window has the two a good many degrees apart, and the in-place clip must step
    # along the way this very cycle went.
    displacement = (hips[b] - hips[a]) * np.array([1.0, 0.0, 1.0])
    travelled = float(np.linalg.norm(displacement))
    if travelled < 1e-6:
        raise ValueError("the cycle goes nowhere")
    return Cycle(a, b, displacement / travelled, travelled / ((b - a) / fps))


def idle_loop(bvh: Bvh, source: SourceRig, min_seconds: float, max_seconds: float, start: int = 0, stop: int | None = None) -> tuple[int, int]:
    """The stretch of `bvh` between `min_seconds` and `max_seconds` long whose
    first and last frames are most alike in pose and motion — the loop the
    seam has the least to hide. Returns `(first, last)`, last inclusive."""
    fps = bvh.fps
    stop = bvh.frames if stop is None else stop
    lo, hi = int(min_seconds * fps), int(max_seconds * fps)
    if stop - start <= lo:
        return start, stop - 1
    features = []
    for name in TARGET_BONES:
        index = source.source_index(bvh, name)
        joint = bvh.joints[index]
        parent = joint.parent
        local = glb.quat_multiply(quat_conjugate(bvh.rotations[:, parent]), bvh.rotations[:, index]) if parent is not None else bvh.rotations[:, index]
        features.append(hemisphere(local))
    pose = np.concatenate(features, axis=1)  # (frames, 4 * bones)
    velocity = np.gradient(moving_average(pose, max(int(fps * 0.1), 1)), axis=0) * fps
    best = None
    step = max(int(fps * 0.1), 1)
    for first in range(start, stop - lo, step):
        last_lo, last_hi = first + lo, min(first + hi, stop - 1)
        if last_hi <= last_lo:
            break
        candidates = np.arange(last_lo, last_hi + 1)
        pose_distance = np.sum((pose[candidates] - pose[first]) ** 2, axis=1)
        motion_distance = np.sum((velocity[candidates] - velocity[first]) ** 2, axis=1) * 0.05
        distance = pose_distance + motion_distance
        k = int(np.argmin(distance))
        if best is None or distance[k] < best[0]:
            best = (distance[k], first, int(candidates[k]))
    return best[1], best[2]


def mean_facing(bvh: Bvh, source: SourceRig, start: int, stop: int) -> np.ndarray:
    """The horizontal direction the actor faces on average over `start` … `stop`."""
    facing = source.facing(bvh.slice(start, stop), "thigh_l", "thigh_r")
    return unit(facing.mean(axis=0))
