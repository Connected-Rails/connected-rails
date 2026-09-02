#!/usr/bin/env python3
"""Turn CC0 artist trees into the checked-in, game-ready tree mod.

This is deliberately an *importer*, not a tree generator.  Every trunk, branch,
needle and near leaf comes from a Mantissa or Poly Haven source. The only new geometry is the
standard crossed quad used for the two distance LODs.  Run through Blender:

    blender -b --python tools/trees/import_mantissa.py -- [--only fichte,...]

The source archives are not vendored (several GB).  See README.md for their
official URLs and the expected cache layout.
"""

from __future__ import annotations

import argparse
import gc
import hashlib
import json
import math
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
import tempfile
from typing import Iterable

import bpy
from mathutils import Vector


HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
MOD = ROOT / "mods" / "trees"
ASSETS = MOD / "assets"
OBJECTS = MOD / "objects"
TEXTURES = ASSETS / "textures"
IMPOSTORS = ASSETS / "impostors"
VARIANTS = ("a", "b", "c")

# The source meshes contain tens of thousands of real leaves.  These budgets
# retain a spatially distributed subset without enlarging it.  Farther out the
# whole-tree render replaces individual leaves, so a LOD switch can never turn
# them into the conspicuously huge cards the old procedural trees used.
LEAF_BUDGET = {
    # The distance image is now an exact projection of LOD0. Keep enough of
    # the artist-authored leaves for that shared silhouette to remain closed;
    # the old lower budgets only looked dense because the impostor secretly
    # rendered three times more leaves than the near mesh.
    "broadleaf": (144000, 48000),
    "shrub": (100000, 40000),
    # The spruce packs model every needle. Roughly one percent of the source
    # was visibly too sparse at close range even with its twigs present; three
    # percent retains a connected crown while the next LOD is still 8 tris.
    "conifer": (96000, 36000),
}
BRANCH_BUDGET = {
    "broadleaf": (12000, 3200),
    "shrub": (9000, 2500),
    # Mantissa's spruce needles sit on a separate, dense twig mesh. Keeping
    # enough of that authored mesh is essential: without it the individual
    # needles look like static falling strokes around otherwise bare branches.
    "conifer": (40000, 3200),
}

# Collapsing the open branch tubes of Poly Haven's pine below roughly 6,000
# source faces creates long, filled triangles across otherwise empty crown
# space. The artist LOD remains inexpensive at this floor and is also reused by
# the European larch catalogue entries.
PINE_MEDIUM_BRANCH_BUDGET = 6000

# A dense crown must never consume the complete branch-decimation budget.  A
# separately simplified structural trunk stays grounded and readable while the
# smaller boughs can still be reduced aggressively.
MEDIUM_TRUNK_BUDGET = 2400
NEAR_TRUNK_BUDGET = 6000

AUTUMN = {
    "rotbuche": (0.62, 0.24, 0.08, 1.0),
    "stieleiche": (0.66, 0.31, 0.07, 1.0),
    "hainbuche": (0.72, 0.43, 0.08, 1.0),
    "sandbirke": (0.91, 0.65, 0.12, 1.0),
    "schwarzerle": (0.70, 0.48, 0.10, 1.0),
    "bergahorn": (0.78, 0.28, 0.06, 1.0),
    "spitzahorn": (0.82, 0.18, 0.04, 1.0),
    "esche": (0.83, 0.61, 0.14, 1.0),
    "winterlinde": (0.87, 0.56, 0.10, 1.0),
    "zitterpappel": (0.88, 0.64, 0.16, 1.0),
    "pyramidenpappel": (0.82, 0.58, 0.12, 1.0),
    "silberweide": (0.80, 0.62, 0.18, 1.0),
    "salweide": (0.79, 0.55, 0.12, 1.0),
    "bergulme": (0.72, 0.42, 0.08, 1.0),
    "rosskastanie": (0.71, 0.34, 0.06, 1.0),
    "eberesche": (0.84, 0.34, 0.07, 1.0),
    "vogelkirsche": (0.81, 0.37, 0.08, 1.0),
    "robinie": (0.76, 0.55, 0.15, 1.0),
    "hasel": (0.73, 0.43, 0.09, 1.0),
    "weissdorn": (0.76, 0.38, 0.07, 1.0),
    "holunder": (0.68, 0.38, 0.08, 1.0),
    "schlehe": (0.73, 0.35, 0.08, 1.0),
    "laerche": (0.88, 0.54, 0.08, 1.0),
}


def parse_args() -> argparse.Namespace:
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    parser = argparse.ArgumentParser()
    parser.add_argument("--only", help="comma-separated species ids")
    parser.add_argument("--source-root", type=Path)
    parser.add_argument("--audit", action="store_true", help="verify checked-in output only")
    return parser.parse_args(argv)


def source_file(source_root: Path, family: str, index: int) -> Path:
    polyhaven = source_root.parent / "polyhaven"
    polyhaven_assets = {
        "polyhaven_fir": "fir_tree_01",
        "polyhaven_pine": "pine_tree_01",
        "polyhaven_sapling": "fir_sapling",
    }
    if family in polyhaven_assets:
        asset = polyhaven_assets[family]
        # The exchange glTF contains only the multi-million-polygon LOD0.
        # Poly Haven's authored clean LOD1 meshes live in the .blend source.
        return polyhaven / asset / f"{asset}_1k.blend"
    if family == "generic":
        return source_root / family / f"Mantissa_Generic_Tree_{index:03}.FBX"
    if family == "birch":
        return source_root / family / f"Mantissa_Birch_{index:03}.FBX"
    if family == "maple":
        return source_root / family / f"Mantissa_Japanese_Maple_{index:03}.FBX"
    if family == "spruce":
        return source_root / family / f"Mantissa_Free_Spruce_{index:03}.FBX"
    if family == "fir":
        return source_root / family / "fbx" / f"Fir.Tall.{index:02}.fbx"
    raise ValueError(f"unknown source family {family}")


def source_index(entry: dict, variant_index: int) -> int:
    if entry["source"].startswith("polyhaven_"):
        return (entry.get("offset", 0) + variant_index) % 3 + 1
    if entry["source"] == "generic":
        return (entry.get("offset", 0) + variant_index) % 10 + 1
    if entry["source"] == "spruce":
        return (entry.get("offset", 0) + variant_index) % 3 + 1
    return variant_index + 1


def wanted_tasks(catalogue: dict, only: set[str] | None) -> dict[Path, list[tuple[dict, int]]]:
    source_root = Path(os.path.expanduser(catalogue["source_root"]))
    tasks: dict[Path, list[tuple[dict, int]]] = {}
    for entry in catalogue["species"]:
        if only and entry["id"] not in only:
            continue
        for variant_index in range(3):
            path = source_file(source_root, entry["source"], source_index(entry, variant_index))
            tasks.setdefault(path, []).append((entry, variant_index))
    return tasks


def clean_generated() -> None:
    for directory in (ASSETS, OBJECTS):
        directory.mkdir(parents=True, exist_ok=True)
    TEXTURES.mkdir(parents=True, exist_ok=True)
    IMPOSTORS.mkdir(parents=True, exist_ok=True)
    for path in ASSETS.iterdir():
        if path.is_file() and path.suffix.lower() in {".gltf", ".bin", ".png", ".jpg"}:
            path.unlink()
    for path in OBJECTS.glob("*.ron"):
        path.unlink()
    if IMPOSTORS.exists():
        for path in IMPOSTORS.glob("*.png"):
            path.unlink()


def convert_image(source: Path, target: Path, size: str, quality: int = 88) -> None:
    target.parent.mkdir(parents=True, exist_ok=True)
    command = shutil.which("magick") or shutil.which("convert")
    if not command:
        raise RuntimeError("ImageMagick (magick or convert) is required")
    subprocess.run(
        [command, str(source), "-auto-orient", "-resize", f"{size}^", "-gravity", "center",
         "-extent", size, "-strip", "-quality", str(quality), str(target)],
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )


def combine_alpha(diffuse: Path, alpha: Path, target: Path, size: str = "512x512") -> None:
    """Make the RGBA texture required by glTF from Poly Haven's split maps."""
    command = shutil.which("magick") or shutil.which("convert")
    if not command:
        raise RuntimeError("ImageMagick (magick or convert) is required")
    subprocess.run(
        [command, str(diffuse), str(alpha), "-alpha", "off", "-compose", "CopyOpacity",
         "-composite", "-resize", size, "-strip", str(target)],
        check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )


def prepare_textures(source_root: Path) -> None:
    polyhaven = source_root.parent / "polyhaven"
    conversions = {
        "bark_broadleaf.jpg": (source_root / "maple/Textures/Bark_DIFF.jpg", "512x512"),
        "bark_birch.jpg": (source_root / "birch/Textures/Birch_Bark_2K_DIFF.jpg", "512x512"),
        "bark_cherry.jpg": (source_root / "cherry/Textures/Cherry_Bark_DIFF.png", "512x512"),
        "bark_conifer.jpg": (source_root / "fir/textures/Fir_Bark_3K_DIFF.tga", "512x512"),
        "foliage_broadleaf.jpg": (
            source_root / "birch/Textures/Birch_Leaf_Front_512_DIFF_01.jpg", "256x256"),
        "foliage_conifer.jpg": (source_root / "fir/textures/Pine_Needle_Alive_DIFF.jpg", "256x256"),
        "poly_fir_bark.jpg": (
            polyhaven / "fir_tree_01/textures/fir_tree_01_trunk_a_diff_1k.jpg", "512x512"),
        "poly_pine_bark.jpg": (
            polyhaven / "pine_tree_01/textures/pine_tree_01_trunk_a_diff_1k.jpg", "512x512"),
        "poly_sapling_bark.jpg": (
            polyhaven / "fir_sapling/textures/fir_sapling_branches_diff_1k.jpg", "512x512"),
    }
    for name, (source, size) in conversions.items():
        if not source.exists():
            raise FileNotFoundError(f"missing Mantissa texture: {source}")
        convert_image(source, TEXTURES / name, size)
    alpha_sources = {
        "poly_fir_foliage.png": (
            polyhaven / "fir_tree_01/textures/fir_tree_01_twig_diff_1k.jpg",
            polyhaven / "fir_tree_01/textures/fir_tree_01_twig_alpha_1k.png"),
        "poly_pine_foliage.png": (
            polyhaven / "pine_tree_01/textures/pine_tree_01_twig_diff_1k.jpg",
            polyhaven / "pine_tree_01/textures/pine_tree_01_twig_alpha_1k.png"),
        "poly_sapling_foliage.png": (
            polyhaven / "fir_sapling/textures/fir_sapling_twigs_diff_1k.jpg",
            polyhaven / "fir_sapling/textures/fir_sapling_twigs_alpha_1k.png"),
    }
    for name, (diffuse, alpha) in alpha_sources.items():
        if not diffuse.exists() or not alpha.exists():
            raise FileNotFoundError(f"missing Poly Haven foliage maps: {diffuse}, {alpha}")
        combine_alpha(diffuse, alpha, TEXTURES / name)


def material_name(obj: bpy.types.Object, polygon) -> str:
    if polygon.material_index >= len(obj.data.materials):
        return ""
    material = obj.data.materials[polygon.material_index]
    return material.name.lower() if material else ""


def is_foliage(name: str) -> bool:
    return "leaf" in name or "needle" in name or "twig" in name


def is_branch(name: str, object_name: str, include_small: bool = True) -> bool:
    text = name.lower()
    obj = object_name.lower()
    if "root" in text:
        return False
    if "twig" in text:
        return False
    if any(word in text for word in ("stem", "small")):
        return include_small
    if any(word in text for word in ("trunk", "branch", "bark")):
        return True
    return ("trunk" in obj or "branches" in obj) and "needle" not in obj


def clear_scene() -> None:
    for obj in list(bpy.data.objects):
        bpy.data.objects.remove(obj, do_unlink=True)
    for mesh in list(bpy.data.meshes):
        if mesh.users == 0:
            bpy.data.meshes.remove(mesh)


def import_source(path: Path) -> list[bpy.types.Object]:
    clear_scene()
    if path.suffix.lower() == ".blend":
        asset = path.stem.removesuffix("_1k")
        main_lod = re.compile(rf"^{re.escape(asset)}_[abc](?:_LOD1)?$")
        with bpy.data.libraries.load(str(path), link=False) as (available, loaded):
            loaded.objects = [name for name in available.objects if main_lod.match(name)]
        for obj in loaded.objects:
            if obj is not None:
                bpy.context.collection.objects.link(obj)
        # Library-appended objects can expose an identity matrix until the
        # dependency graph updates. Extracting wood before and foliage after
        # that implicit update shifted some crowns away from their trunks.
        bpy.context.view_layer.update()
    elif path.suffix.lower() in {".gltf", ".glb"}:
        bpy.ops.import_scene.gltf(filepath=str(path))
    else:
        bpy.ops.import_scene.fbx(filepath=str(path), use_anim=False)
    objects = [obj for obj in bpy.context.scene.objects if obj.type == "MESH" and len(obj.data.polygons) > 100]
    if not objects:
        raise RuntimeError(f"no tree mesh in {path}")
    return objects


def all_world_points(objects: list[bpy.types.Object]) -> list[Vector]:
    points = []
    for obj in objects:
        matrix = obj.matrix_world
        points.extend(matrix @ Vector(corner) for corner in obj.bound_box)
    return points


class AxisMap:
    def __init__(self, objects: list[bpy.types.Object], up_axis: int):
        points = all_world_points(objects)
        mins = [min(p[i] for p in points) for i in range(3)]
        maxs = [max(p[i] for p in points) for i in range(3)]
        dimensions = [maxs[i] - mins[i] for i in range(3)]
        self.up = up_axis
        self.horizontal = [i for i in range(3) if i != self.up]
        trunk_vertices = []
        for obj in objects:
            used = {index for polygon in obj.data.polygons
                    if is_branch(material_name(obj, polygon), obj.name)
                    for index in polygon.vertices}
            trunk_vertices.extend((obj.matrix_world @ obj.data.vertices[index].co)[self.up]
                                  for index in used)
        self.minimum = min(trunk_vertices) if trunk_vertices else mins[self.up]
        self.height = maxs[self.up] - self.minimum
        self.centres = [(mins[i] + maxs[i]) * 0.5 for i in self.horizontal]
        self.width = max(dimensions[i] for i in self.horizontal)

    def canonical(self, point: Vector) -> tuple[float, float, float]:
        # glTF is Y-up.  The sign on Z keeps winding consistent with the axis swap.
        x = point[self.horizontal[0]] - self.centres[0]
        z = -(point[self.horizontal[1]] - self.centres[1])
        if self.up == 1:
            # The fir source trees are intentionally very flat.  Forty-five
            # degrees makes both orthogonal impostor views representative and
            # avoids a green tree becoming a brown edge-on sliver.
            x, z = (x - z) / math.sqrt(2.0), (x + z) / math.sqrt(2.0)
        return (x, point[self.up] - self.minimum, z)

    def canonical_normal(self, normal: Vector) -> tuple[float, float, float]:
        x, y, z = normal[self.horizontal[0]], normal[self.up], -normal[self.horizontal[1]]
        if self.up == 1:
            x, z = (x - z) / math.sqrt(2.0), (x + z) / math.sqrt(2.0)
        return (x, y, z)


def polygon_uv(obj: bpy.types.Object, loop_index: int) -> tuple[float, float]:
    layer = obj.data.uv_layers.active
    if layer is None:
        return (0.0, 0.0)
    uv = layer.data[loop_index].uv
    return (float(uv.x), 1.0 - float(uv.y))


def build_branch_object(objects: list[bpy.types.Object], include_small: bool,
                        trunk_only: bool = False,
                        exclude_trunk: bool = False) -> bpy.types.Object:
    vertices: list[tuple[float, float, float]] = []
    faces: list[list[int]] = []
    face_uvs: list[list[tuple[float, float]]] = []
    for obj in objects:
        matrix = obj.matrix_world
        branch_polygons = []
        for polygon in obj.data.polygons:
            name = material_name(obj, polygon)
            if not is_branch(name, obj.name, include_small):
                continue
            is_trunk = "trunk" in name or "trunk" in obj.name.lower()
            if trunk_only and not is_trunk:
                continue
            if exclude_trunk and is_trunk:
                continue
            branch_polygons.append(polygon)
        if not branch_polygons:
            continue
        used = sorted({index for polygon in branch_polygons for index in polygon.vertices})
        remap = {old: new for new, old in enumerate(used)}
        offset = len(vertices)
        vertices.extend(tuple(matrix @ obj.data.vertices[index].co) for index in used)
        for polygon in branch_polygons:
            faces.append([offset + remap[i] for i in polygon.vertices])
            face_uvs.append([polygon_uv(obj, i) for i in polygon.loop_indices])
    if not faces:
        raise RuntimeError("source has no branch material")
    mesh = bpy.data.meshes.new("source_branches")
    mesh.from_pydata(vertices, [], faces)
    uv_layer = mesh.uv_layers.new(name="UVMap")
    for polygon, uvs in zip(mesh.polygons, face_uvs):
        for loop_index, uv in zip(polygon.loop_indices, uvs):
            uv_layer.data[loop_index].uv = (uv[0], 1.0 - uv[1])
        polygon.use_smooth = True
    obj = bpy.data.objects.new("source_branches", mesh)
    bpy.context.collection.objects.link(obj)
    return obj


def simplify_branch(source: bpy.types.Object, target_faces: int) -> dict:
    obj = source.copy()
    obj.data = source.data.copy()
    bpy.context.collection.objects.link(obj)
    count = len(obj.data.polygons)
    if count > target_faces:
        modifier = obj.modifiers.new("game_lod", "DECIMATE")
        modifier.ratio = max(0.001, target_faces / count)
        modifier.use_collapse_triangulate = True
        bpy.context.view_layer.objects.active = obj
        obj.select_set(True)
        bpy.ops.object.modifier_apply(modifier=modifier.name)
        obj.select_set(False)
    result = extract_mesh(obj)
    bpy.data.objects.remove(obj, do_unlink=True)
    return result


def merge_meshes(*meshes: dict) -> dict:
    """Join independently simplified wood parts without changing their surfaces."""
    result = {"positions": [], "normals": [], "uvs": [], "indices": []}
    for mesh in meshes:
        base = len(result["positions"])
        result["positions"].extend(mesh["positions"])
        result["normals"].extend(mesh["normals"])
        result["uvs"].extend(mesh["uvs"])
        result["indices"].extend(base + index for index in mesh["indices"])
    return result


def has_separate_trunk(objects: list[bpy.types.Object]) -> bool:
    trunk = False
    bough = False
    for obj in objects:
        for polygon in obj.data.polygons:
            name = material_name(obj, polygon)
            if not is_branch(name, obj.name, True):
                continue
            if "trunk" in name or "trunk" in obj.name.lower():
                trunk = True
            else:
                bough = True
    return trunk and bough


def extract_mesh(obj: bpy.types.Object, accepted: set[int] | None = None) -> dict:
    mesh = obj.data
    mesh.calc_loop_triangles()
    positions: list[tuple[float, float, float]] = []
    normals: list[tuple[float, float, float]] = []
    uvs: list[tuple[float, float]] = []
    indices: list[int] = []
    corner_normals = getattr(mesh, "corner_normals", None)
    for triangle in mesh.loop_triangles:
        if accepted is not None and triangle.polygon_index not in accepted:
            continue
        for loop_index in triangle.loops:
            loop = mesh.loops[loop_index]
            positions.append(tuple(mesh.vertices[loop.vertex_index].co))
            if corner_normals is not None:
                normals.append(tuple(corner_normals[loop_index].vector))
            else:
                normals.append(tuple(mesh.vertices[loop.vertex_index].normal))
            uvs.append(polygon_uv(obj, loop_index))
            indices.append(len(indices))
    return {"positions": positions, "normals": normals, "uvs": uvs, "indices": indices}


def foliage_groups(objects: list[bpy.types.Object]) -> list[dict]:
    groups: list[dict] = []
    for obj in objects:
        matrix = obj.matrix_world
        normal_matrix = matrix.to_3x3().inverted().transposed()
        by_material: dict[int, list] = {}
        for polygon in obj.data.polygons:
            if is_foliage(material_name(obj, polygon)):
                by_material.setdefault(polygon.material_index, []).append(polygon)
        for polygons in by_material.values():
            current = []
            current_max = -1
            for polygon in polygons:
                lo, hi = min(polygon.vertices), max(polygon.vertices)
                if current and lo > current_max:
                    groups.append(extract_group(obj, current, matrix, normal_matrix))
                    current = []
                    current_max = -1
                current.append(polygon)
                current_max = max(current_max, hi)
            if current:
                groups.append(extract_group(obj, current, matrix, normal_matrix))
    if not groups:
        raise RuntimeError("source has no leaf/needle material")
    return groups


def extract_group(obj, polygons, matrix, normal_matrix) -> dict:
    positions = []
    normals = []
    uvs = []
    indices = []
    for polygon in polygons:
        loops = list(polygon.loop_indices)
        for i in range(1, len(loops) - 1):
            for loop_index in (loops[0], loops[i], loops[i + 1]):
                loop = obj.data.loops[loop_index]
                positions.append(tuple(matrix @ obj.data.vertices[loop.vertex_index].co))
                normal = normal_matrix @ obj.data.vertices[loop.vertex_index].normal
                normals.append(tuple(normal.normalized()))
                uvs.append(polygon_uv(obj, loop_index))
                indices.append(len(indices))
    centre = tuple(sum(p[i] for p in positions) / len(positions) for i in range(3))
    digest = hashlib.blake2b(struct.pack("<3f", *centre), digest_size=8).digest()
    return {"positions": positions, "normals": normals, "uvs": uvs,
            "indices": indices, "centre": centre, "score": int.from_bytes(digest, "little")}


def spatial_subset(groups: list[dict], triangle_budget: int, axis: AxisMap) -> list[dict]:
    average = sum(len(group["indices"]) // 3 for group in groups) / len(groups)
    count = max(1, round(triangle_budget / max(average, 1)))
    if len(groups) <= count:
        return groups
    cells: dict[tuple[int, int, int], dict] = {}
    for group in groups:
        x, y, z = axis.canonical(Vector(group["centre"]))
        key = (
            math.floor((x / max(axis.width, 1e-6) + 0.5) * 14),
            math.floor(y / max(axis.height, 1e-6) * 20),
            math.floor((z / max(axis.width, 1e-6) + 0.5) * 14),
        )
        if key not in cells or group["score"] < cells[key]["score"]:
            cells[key] = group
    selected = sorted(cells.values(), key=lambda g: g["score"])
    picked = {id(group) for group in selected[:count]}
    if len(picked) < count:
        for group in sorted(groups, key=lambda g: g["score"]):
            if id(group) not in picked:
                picked.add(id(group))
                if len(picked) == count:
                    break
    candidates = sorted((group for group in groups if id(group) in picked),
                        key=lambda group: group["score"])
    result = []
    triangles = 0
    for group in candidates:
        cost = len(group["indices"]) // 3
        if result and triangles + cost > triangle_budget:
            continue
        result.append(group)
        triangles += cost
    return result


def transform_mesh(data: dict, axis: AxisMap, height: float, crown: float,
                   leaf_groups: list[dict] | None = None, leaf_cap: float = 3.0,
                   leaf_scale: float = 1.0) -> dict:
    horizontal_scale = height * crown / max(axis.width, 1e-6)
    vertical_scale = height / max(axis.height, 1e-6)
    positions = []
    normals = []
    uvs = []
    indices = []
    if leaf_groups is None:
        source_positions = data["positions"]
        source_normals = data["normals"]
        source_uvs = data["uvs"]
        for point, normal, uv in zip(source_positions, source_normals, source_uvs):
            x, y, z = axis.canonical(Vector(point))
            positions.append((x * horizontal_scale, y * vertical_scale, z * horizontal_scale))
            nx, ny, nz = axis.canonical_normal(Vector(normal))
            transformed = Vector((nx / horizontal_scale, ny / vertical_scale, nz / horizontal_scale)).normalized()
            normals.append(tuple(transformed))
            uvs.append(uv)
        indices = list(data["indices"])
    else:
        # Positions follow the scaled source crown; leaf size itself is capped.
        # A six-metre source becoming a thirty-metre tree must not acquire
        # thirty-centimetre leaves.
        leaf_h = min(horizontal_scale, leaf_cap) * leaf_scale
        leaf_v = min(vertical_scale, leaf_cap) * leaf_scale
        for group in leaf_groups:
            centre = Vector(group["centre"])
            cx, cy, cz = axis.canonical(centre)
            target_centre = Vector((cx * horizontal_scale, cy * vertical_scale, cz * horizontal_scale))
            base = len(positions)
            for point, normal, uv in zip(group["positions"], group["normals"], group["uvs"]):
                px, py, pz = axis.canonical(Vector(point))
                dx, dy, dz = px - cx, py - cy, pz - cz
                positions.append(tuple(target_centre + Vector((dx * leaf_h, dy * leaf_v, dz * leaf_h))))
                nx, ny, nz = axis.canonical_normal(Vector(normal))
                normals.append(tuple(Vector((nx, ny, nz)).normalized()))
                uvs.append(uv)
            indices.extend(base + index for index in group["indices"])
    return {"positions": positions, "normals": normals, "uvs": uvs, "indices": indices}


def remove_collapse_slabs(mesh: dict, maximum_edge: float) -> dict:
    """Discard the rare giant face made by collapsing an open branch tube."""
    kept = []
    positions = mesh["positions"]
    for offset in range(0, len(mesh["indices"]), 3):
        triangle = mesh["indices"][offset:offset + 3]
        points = [Vector(positions[index]) for index in triangle]
        longest = max((points[0] - points[1]).length,
                      (points[1] - points[2]).length,
                      (points[2] - points[0]).length)
        if longest <= maximum_edge:
            kept.extend(triangle)
    return dict(mesh, indices=kept)


def maximum_triangle_area_ratio(mesh: dict) -> float:
    """Measure the broadest face against tree height to catch mesh explosions."""
    positions = mesh["positions"]
    if not positions:
        return 0.0
    height = max(point[1] for point in positions) - min(point[1] for point in positions)
    if height <= 1e-6:
        return 0.0
    maximum = 0.0
    for offset in range(0, len(mesh["indices"]), 3):
        points = [Vector(positions[mesh["indices"][offset + i]]) for i in range(3)]
        area = ((points[1] - points[0]).cross(points[2] - points[0])).length * 0.5
        maximum = max(maximum, area / (height * height))
    return maximum


def maximum_triangle_edge_ratio(mesh: dict) -> float:
    positions = mesh["positions"]
    if not positions:
        return 0.0
    height = max(point[1] for point in positions) - min(point[1] for point in positions)
    if height <= 1e-6:
        return 0.0
    maximum = 0.0
    for offset in range(0, len(mesh["indices"]), 3):
        points = [Vector(positions[mesh["indices"][offset + i]]) for i in range(3)]
        maximum = max(maximum,
                      (points[0] - points[1]).length / height,
                      (points[1] - points[2]).length / height,
                      (points[2] - points[0]).length / height)
    return maximum


def mesh_surface_area(mesh: dict) -> float:
    positions = mesh["positions"]
    area = 0.0
    for offset in range(0, len(mesh["indices"]), 3):
        points = [Vector(positions[mesh["indices"][offset + i]]) for i in range(3)]
        area += ((points[1] - points[0]).cross(points[2] - points[0])).length * 0.5
    return area


def mesh_height_band_areas(mesh: dict, minimum: float, maximum: float,
                           band_count: int = 10) -> list[float]:
    """Measure wood surface in fixed tree-height bands.

    Bounds deliberately come from LOD0.  Computing independent bounds for every
    LOD would hide exactly the failure this audit guards against: a decimator
    deleting the trunk base and making the reduced mesh start metres above the
    ground.
    """
    areas = [0.0] * band_count
    span = max(maximum - minimum, 1e-6)
    positions = mesh["positions"]
    for offset in range(0, len(mesh["indices"]), 3):
        points = [Vector(positions[mesh["indices"][offset + i]]) for i in range(3)]
        centre_y = sum(point.y for point in points) / 3.0
        band = max(0, min(band_count - 1,
                          math.floor((centre_y - minimum) / span * band_count)))
        areas[band] += ((points[1] - points[0]).cross(points[2] - points[0])).length * 0.5
    return areas


def audit_trunk_retention(near: dict, medium: dict, context: str) -> None:
    """Require LOD1 wood to retain the grounded, continuous LOD0 trunk."""
    near_y = [point[1] for point in near["positions"]]
    medium_y = [point[1] for point in medium["positions"]]
    minimum, maximum = min(near_y), max(near_y)
    height = max(maximum - minimum, 1e-6)
    if min(medium_y) > minimum + height * 0.02:
        raise RuntimeError(
            f"floating LOD1 trunk in {context}: starts "
            f"{min(medium_y) - minimum:.2f} m above LOD0")
    near_areas = mesh_height_band_areas(near, minimum, maximum)
    medium_areas = mesh_height_band_areas(medium, minimum, maximum)
    for band, (near_area, medium_area) in enumerate(zip(near_areas[:6], medium_areas[:6])):
        if near_area > 1e-6 and medium_area / near_area < 0.15:
            raise RuntimeError(
                f"missing LOD1 trunk in {context}: height band {band} retains only "
                f"{medium_area / near_area:.1%} of LOD0 wood area")


def medium_leaf_scale(near_groups: list[dict], medium_groups: list[dict]) -> float:
    """Retain crown coverage without creating conspicuously huge distance leaves."""
    near_triangles = sum(len(group["indices"]) // 3 for group in near_groups)
    medium_triangles = sum(len(group["indices"]) // 3 for group in medium_groups)
    # Projected coverage is approximately leaf count times linear scale squared.
    # Eighty percent keeps branch gaps readable without looking wintry. More
    # source leaves let the enlargement stay subtle at the LOD0 hand-over.
    coverage_scale = math.sqrt(0.80 * near_triangles / max(medium_triangles, 1))
    return min(1.55, max(1.0, coverage_scale))


class GltfBuffer:
    def __init__(self):
        self.data = bytearray()
        self.views = []
        self.accessors = []

    def append(self, payload: bytes, target: int | None = None) -> int:
        while len(self.data) % 4:
            self.data.append(0)
        offset = len(self.data)
        self.data.extend(payload)
        view = {"buffer": 0, "byteOffset": offset, "byteLength": len(payload)}
        if target is not None:
            view["target"] = target
        self.views.append(view)
        return len(self.views) - 1

    def vectors(self, values: list[tuple], dimensions: int) -> int:
        flat = [component for value in values for component in value]
        view = self.append(struct.pack(f"<{len(flat)}f", *flat), 34962)
        accessor = {"bufferView": view, "componentType": 5126, "count": len(values),
                    "type": "VEC3" if dimensions == 3 else "VEC2"}
        if dimensions == 3 and values:
            accessor["min"] = [min(v[i] for v in values) for i in range(3)]
            accessor["max"] = [max(v[i] for v in values) for i in range(3)]
        self.accessors.append(accessor)
        return len(self.accessors) - 1

    def indices(self, values: list[int]) -> int:
        component = 5123 if max(values, default=0) < 65536 else 5125
        code = "H" if component == 5123 else "I"
        view = self.append(struct.pack(f"<{len(values)}{code}", *values), 34963)
        self.accessors.append({"bufferView": view, "componentType": component,
                               "count": len(values), "type": "SCALAR",
                               "min": [min(values, default=0)], "max": [max(values, default=0)]})
        return len(self.accessors) - 1

    def primitive(self, mesh: dict, material: int) -> dict:
        mesh = deduplicate_mesh(mesh)
        return {
            "attributes": {
                "POSITION": self.vectors(mesh["positions"], 3),
                "NORMAL": self.vectors(mesh["normals"], 3),
                "TEXCOORD_0": self.vectors(mesh["uvs"], 2),
            },
            "indices": self.indices(mesh["indices"]),
            "material": material,
        }


def deduplicate_mesh(mesh: dict) -> dict:
    """Share equal corners before writing; source extraction is loop-oriented."""
    positions = []
    normals = []
    uvs = []
    indices = []
    seen = {}
    for old in mesh["indices"]:
        position = mesh["positions"][old]
        normal = mesh["normals"][old]
        uv = mesh["uvs"][old]
        key = (tuple(round(value, 6) for value in position),
               tuple(round(value, 5) for value in normal),
               tuple(round(value, 6) for value in uv))
        index = seen.get(key)
        if index is None:
            index = len(positions)
            seen[key] = index
            positions.append(position)
            normals.append(normal)
            uvs.append(uv)
        indices.append(index)
    return {"positions": positions, "normals": normals, "uvs": uvs, "indices": indices}


def billboard_mesh(width: float, height: float, planes: int = 2) -> dict:
    w = width * 0.55
    positions = []
    normals = []
    uvs = []
    indices = []
    for plane in range(planes):
        angle = plane * math.pi / planes
        x, z = math.cos(angle) * w, math.sin(angle) * w
        base = len(positions)
        positions.extend([(-x, 0, -z), (x, 0, z), (x, height, z), (-x, height, -z)])
        normal = (-math.sin(angle), 0, math.cos(angle))
        normals.extend([normal] * 4)
        u0, u1 = ((0, 0.5) if plane % 2 == 0 else (0.5, 1))
        uvs.extend([(u0, 1), (u1, 1), (u1, 0), (u0, 0)])
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3])
    return {"positions": positions, "normals": normals, "uvs": uvs, "indices": indices}


def image_and_texture(gltf: dict, uri: str) -> int:
    gltf.setdefault("images", []).append({"uri": uri})
    gltf.setdefault("textures", []).append({"source": len(gltf["images"]) - 1,
                                             "sampler": 0})
    return len(gltf["textures"]) - 1


def pbr_material(name: str, texture: int, colour: tuple[float, float, float, float],
                 roughness: float, double_sided: bool = False,
                 alpha_mode: str | None = None) -> dict:
    material = {
        "name": name,
        "pbrMetallicRoughness": {
            "baseColorFactor": list(colour),
            "baseColorTexture": {"index": texture},
            "metallicFactor": 0.0,
            "roughnessFactor": roughness,
        },
        "doubleSided": double_sided,
    }
    if alpha_mode == "MASK":
        material.update({"alphaMode": "MASK", "alphaCutoff": 0.18})
    elif alpha_mode == "BLEND":
        material["alphaMode"] = "BLEND"
    return material


def seasonal_colours(entry: dict) -> dict[str, tuple[float, float, float, float]]:
    digest = hashlib.blake2b(entry["id"].encode(), digest_size=2).digest()
    variation = (digest[0] / 255.0 - 0.5) * 0.10
    if entry["source"].startswith("polyhaven_"):
        summer = (0.58 + variation, 1.0, 0.74 + variation, 1.0)
    elif entry["kind"] == "conifer":
        summer = (0.58 + variation, 0.78 + variation, 0.56 + variation, 1.0)
    else:
        summer = (0.66 + variation, 0.88 + variation * 0.5, 0.60 + variation, 1.0)
    result = {"summer": summer}
    if entry["deciduous"]:
        result["autumn"] = AUTUMN[entry["id"]]
        result["winter"] = (0.0, 0.0, 0.0, 0.0)
    else:
        result["winter"] = tuple(min(1.0, c * 1.10) for c in summer[:3]) + (1.0,)
    return result


def bark_texture(entry: dict) -> str:
    if entry["source"] == "polyhaven_fir":
        return "textures/poly_fir_bark.jpg"
    if entry["source"] == "polyhaven_pine":
        return "textures/poly_pine_bark.jpg"
    if entry["source"] == "polyhaven_sapling":
        return "textures/poly_sapling_bark.jpg"
    if entry.get("bark") == "cherry":
        return "textures/bark_cherry.jpg"
    if entry.get("bark") == "birch":
        return "textures/bark_birch.jpg"
    if entry["kind"] == "conifer":
        return "textures/bark_conifer.jpg"
    return "textures/bark_broadleaf.jpg"


def foliage_texture(entry: dict) -> str:
    textures = {
        "polyhaven_fir": "textures/poly_fir_foliage.png",
        "polyhaven_pine": "textures/poly_pine_foliage.png",
        "polyhaven_sapling": "textures/poly_sapling_foliage.png",
    }
    return textures.get(
        entry["source"],
        "textures/foliage_conifer.jpg" if entry["kind"] == "conifer"
        else "textures/foliage_broadleaf.jpg",
    )


def write_gltf(entry: dict, variant: str, season: str,
               near: tuple[dict, dict], medium: tuple[dict, dict],
               buffer: GltfBuffer, rich_billboard: dict, far_billboard: dict,
               bin_name: str) -> None:
    suffix = "" if season == "summer" else "_herbst" if season == "autumn" else "_winter"
    name = f"{entry['id']}_{variant}{suffix}"
    gltf = {
        "asset": {"version": "2.0", "generator": "Connected Rails CC0 tree importer",
                  "copyright": "Mantissa and Poly Haven source models: CC0 1.0; optimisation: Connected Rails"},
        "scene": 0,
        "scenes": [{"nodes": [0, 1, 2, 3]}],
        "nodes": [],
        "meshes": [],
        "buffers": [{"uri": bin_name, "byteLength": len(buffer.data)}],
        "bufferViews": buffer.views,
        "accessors": buffer.accessors,
        "samplers": [{"magFilter": 9729, "minFilter": 9987, "wrapS": 10497, "wrapT": 10497}],
    }
    bark_tex = image_and_texture(gltf, bark_texture(entry))
    foliage_tex = image_and_texture(gltf, foliage_texture(entry))
    impostor_tex = image_and_texture(gltf, f"impostors/{entry['id']}_{variant}_{season}.png")
    colours = seasonal_colours(entry)
    gltf["materials"] = [
        pbr_material("bark", bark_tex, (0.82, 0.82, 0.82, 1.0), 0.91),
        pbr_material("foliage", foliage_tex, colours[season], 0.78, True,
                     "MASK" if entry["source"].startswith("polyhaven_") else None),
        pbr_material("whole_tree_impostor", impostor_tex, (1, 1, 1, 1), 0.92, True, "MASK"),
    ]
    bare = season == "winter" and entry["deciduous"]
    for level, geometry in enumerate((near, medium)):
        branch_primitive, leaf_primitive = geometry
        primitives = [dict(branch_primitive, material=0)]
        if not bare:
            primitives.append(dict(leaf_primitive, material=1))
        gltf["meshes"].append({"name": f"crown_LOD{level}", "primitives": primitives})
        gltf["nodes"].append({"name": f"crown_LOD{level}", "mesh": level})
    for level, billboard in ((2, rich_billboard), (3, far_billboard)):
        gltf["meshes"].append({"name": f"crown_LOD{level}",
                               "primitives": [dict(billboard, material=2)]})
        gltf["nodes"].append({"name": f"crown_LOD{level}", "mesh": len(gltf["meshes"]) - 1})
    (ASSETS / f"{name}.gltf").write_text(json.dumps(gltf, separators=(",", ":")), encoding="utf-8")


def blender_mesh(name: str, data: dict) -> bpy.types.Object:
    mesh = bpy.data.meshes.new(name)
    faces = [tuple(data["indices"][i:i + 3]) for i in range(0, len(data["indices"]), 3)]
    # glTF Y-up -> Blender Z-up.
    positions = [(x, -z, y) for x, y, z in data["positions"]]
    mesh.from_pydata(positions, [], faces)
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(obj)
    return obj


def render_material(name: str, colour: tuple[float, float, float, float],
                    texture: Path | None = None, alpha: bool = False) -> bpy.types.Material:
    material = bpy.data.materials.new(name)
    material.diffuse_color = colour
    material.use_nodes = True
    bsdf = material.node_tree.nodes.get("Principled BSDF")
    bsdf.inputs["Base Color"].default_value = colour
    bsdf.inputs["Roughness"].default_value = 0.82
    if texture is not None:
        image = bpy.data.images.load(str(texture), check_existing=True)
        image_node = material.node_tree.nodes.new("ShaderNodeTexImage")
        image_node.image = image
        mix = material.node_tree.nodes.new("ShaderNodeMixRGB")
        mix.blend_type = "MULTIPLY"
        mix.inputs[0].default_value = 1.0
        mix.inputs[2].default_value = colour
        material.node_tree.links.new(image_node.outputs["Color"], mix.inputs[1])
        material.node_tree.links.new(mix.outputs["Color"], bsdf.inputs["Base Color"])
        if alpha:
            material.node_tree.links.new(image_node.outputs["Alpha"], bsdf.inputs["Alpha"])
            material.surface_render_method = "DITHERED"
            emission = bsdf.inputs.get("Emission Color") or bsdf.inputs.get("Emission")
            strength = bsdf.inputs.get("Emission Strength")
            if emission is not None:
                material.node_tree.links.new(mix.outputs["Color"], emission)
            if strength is not None:
                strength.default_value = 1.0
    return material


def look_at(camera: bpy.types.Object, target: Vector) -> None:
    camera.rotation_euler = (target - camera.location).to_track_quat("-Z", "Y").to_euler()


def render_impostor(entry: dict, variant: str, season: str, branch: dict,
                    foliage: dict, height: float, width: float) -> None:
    for obj in list(bpy.context.scene.objects):
        bpy.data.objects.remove(obj, do_unlink=True)
    branch_obj = blender_mesh("bark", branch)
    bark_colour = ((1.0, 1.0, 1.0, 1.0) if entry["source"].startswith("polyhaven_") else
                   (0.52, 0.50, 0.45, 1.0) if entry.get("bark") == "birch" else
                   (0.30, 0.13, 0.08, 1.0) if entry.get("bark") == "cherry" else
                   (0.22, 0.15, 0.09, 1.0))
    branch_texture = ASSETS / bark_texture(entry) if entry["source"].startswith("polyhaven_") else None
    branch_obj.data.materials.append(render_material("bark", bark_colour, branch_texture))
    if entry["source"] == "fir" and season != "winter":
        branch_obj.hide_render = True
    if not (season == "winter" and entry["deciduous"]):
        leaf_obj = blender_mesh("foliage", foliage)
        colour = seasonal_colours(entry)[season]
        render_colour = (colour if entry["source"].startswith("polyhaven_") else
                         tuple(component * 0.48 for component in colour[:3]) + (1.0,))
        leaf_texture = ASSETS / foliage_texture(entry) if entry["source"].startswith("polyhaven_") else None
        leaf_obj.data.materials.append(render_material(
            "foliage", render_colour, leaf_texture,
            entry["source"].startswith("polyhaven_")))

    scene = bpy.context.scene
    scene.render.engine = "BLENDER_EEVEE"
    scene.render.film_transparent = True
    scene.render.resolution_x = 256
    scene.render.resolution_y = 256
    scene.render.resolution_percentage = 100
    scene.render.image_settings.file_format = "PNG"
    scene.render.image_settings.color_mode = "RGBA"
    scene.render.image_settings.color_depth = "8"
    scene.view_settings.look = "AgX - Medium High Contrast"

    camera_data = bpy.data.cameras.new("Camera")
    camera = bpy.data.objects.new("Camera", camera_data)
    scene.collection.objects.link(camera)
    scene.camera = camera
    camera_data.type = "ORTHO"
    camera_data.ortho_scale = max(height, width) * 1.08
    target = Vector((0, 0, height * 0.5))

    world = bpy.data.worlds.new("World")
    scene.world = world
    world.use_nodes = True
    world.node_tree.nodes["Background"].inputs["Color"].default_value = (0.75, 0.79, 0.84, 1)
    world.node_tree.nodes["Background"].inputs["Strength"].default_value = 0.28
    sun_data = bpy.data.lights.new("Sun", "SUN")
    sun_data.energy = 1.25
    sun = bpy.data.objects.new("Sun", sun_data)
    scene.collection.objects.link(sun)
    sun.rotation_euler = (math.radians(35), math.radians(-20), math.radians(-35))

    target_path = IMPOSTORS / f"{entry['id']}_{variant}_{season}.png"
    with tempfile.TemporaryDirectory(prefix="connected-rails-tree-") as temporary:
        front = Path(temporary) / "front.png"
        side = Path(temporary) / "side.png"
        camera.location = (0, -max(height, width) * 2.5, height * 0.5)
        look_at(camera, target)
        scene.render.filepath = str(front)
        bpy.ops.render.render(write_still=True)
        camera.location = (-max(height, width) * 2.5, 0, height * 0.5)
        look_at(camera, target)
        scene.render.filepath = str(side)
        bpy.ops.render.render(write_still=True)
        command = shutil.which("magick") or shutil.which("convert")
        subprocess.run([command, str(front), str(side), "+append", "-strip", str(target_path)],
                       check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def crown_extent(*meshes: dict) -> tuple[float, float]:
    """How wide the built crown actually is [m], broadest span first.

    The catalogue states the crown as a share of the height; what goes into the
    object file is what the finished mesh spans, because that is the number the
    imagery detection compares a crown measured off a photograph against
    (`crates/vision`). Measured on LOD0, wood and foliage together.
    """
    points = [point for mesh in meshes for point in mesh["positions"]]
    if not points:
        return (0.0, 0.0)
    x = max(point[0] for point in points) - min(point[0] for point in points)
    z = max(point[2] for point in points) - min(point[2] for point in points)
    return (round(max(x, z), 1), round(min(x, z), 1))


def write_object(entry: dict, variant_index: int, height: float,
                 crown: tuple[float, float]) -> None:
    variant = VARIANTS[variant_index]
    name = f"{entry['name']} {variant.upper()}"
    model = f"trees/assets/{entry['id']}_{variant}.gltf"
    autumn = (f'Some("trees/assets/{entry["id"]}_{variant}_herbst.gltf")'
              if entry["deciduous"] else "None")
    winter = f'Some("trees/assets/{entry["id"]}_{variant}_winter.gltf")'
    # LOD1 is reduced geometry from the same tree, so the first and most visible
    # hand-over cannot replace the crown with an impostor seen from another
    # direction. Whole-tree renders begin only once the tree is genuinely far.
    lod0 = max(45, round(height * 3.5))
    lod1 = max(400, round(height * 15.0))
    lod2 = max(800, round(height * 30.0))
    cull = entry["cull"]
    tags = ", ".join(json.dumps(tag, ensure_ascii=False) for tag in entry["tags"])
    text = f'''// {entry["name"]} ({entry["latin"]}), {height:g} m. CC0 artist source;
// imported and LOD-optimised by tools/trees/import_mantissa.py.
(
    name: {json.dumps(name, ensure_ascii=False)},
    model: {json.dumps(model)},
    autumn_model: {autumn},
    winter_model: {winter},
    lod_distances: [{lod0}, {lod1}, {lod2}, {cull}],
    footprint: Some((length: {crown[0]:g}, width: {crown[1]:g})),
    tags: [{tags}],
)
'''
    (OBJECTS / f"{entry['id']}_{variant}.ron").write_text(text, encoding="utf-8")


def build_tree(entry: dict, variant_index: int, axis: AxisMap, branches: list[dict],
               groups: list[dict]) -> dict:
    variant = VARIANTS[variant_index]
    height = entry["height"][0] + (entry["height"][1] - entry["height"][0]) * variant_index / 2
    branch = transform_mesh(branches[0], axis, height, entry["crown"])
    medium_branch = transform_mesh(branches[1], axis, height, entry["crown"])
    if entry["source"] in {"birch", "maple"}:
        branch = remove_collapse_slabs(branch, height * 0.12)
        medium_branch = remove_collapse_slabs(medium_branch, height * 0.12)
    audit_trunk_retention(branch, medium_branch, f"{entry['id']}_{variant}")
    for level, mesh in enumerate((branch, medium_branch)):
        area_ratio = maximum_triangle_area_ratio(mesh)
        if area_ratio > 0.008:
            raise RuntimeError(
                f"exploded branch triangle in {entry['id']}_{variant} LOD{level}: "
                f"area is {area_ratio:.3%} of squared tree height")
        if entry["source"] == "maple":
            edge_ratio = maximum_triangle_edge_ratio(mesh)
            limit = (0.04, 0.09)[level]
            if edge_ratio > limit:
                raise RuntimeError(
                    f"collapsed maple branch in {entry['id']}_{variant} LOD{level}: "
                    f"edge spans {edge_ratio:.1%} of tree height")
    subset = spatial_subset(groups, LEAF_BUDGET[entry["kind"]][0], axis)
    medium_subset = spatial_subset(groups, LEAF_BUDGET[entry["kind"]][1], axis)
    foliage = transform_mesh({}, axis, height, entry["crown"], subset,
                             8.0 if entry["source"] == "fir" else
                             8.0 if entry["kind"] == "conifer" else 4.0)
    medium_foliage = transform_mesh(
        {}, axis, height, entry["crown"], medium_subset,
        8.0 if entry["source"] == "fir" else
        8.0 if entry["kind"] == "conifer" else 4.0,
        medium_leaf_scale(subset, medium_subset),
    )
    foliage_area = mesh_surface_area(foliage)
    medium_foliage_area = mesh_surface_area(medium_foliage)
    if foliage_area > 0 and medium_foliage_area / foliage_area < 0.65:
        raise RuntimeError(
            f"winter-thin foliage in {entry['id']}_{variant} LOD1: "
            f"only {medium_foliage_area / foliage_area:.1%} of LOD0 leaf area")
    near_mesh = (branch, foliage)
    medium_mesh = (medium_branch, medium_foliage)

    # The impostor must be a projection of exactly LOD0. A denser render-only
    # selection made the first LOD transition look like a completely different
    # tree even though both came from the same artist source.
    render_foliage = foliage

    buffer = GltfBuffer()
    near_primitives = (buffer.primitive(branch, 0), buffer.primitive(foliage, 1))
    medium_primitives = (buffer.primitive(medium_branch, 0),
                         buffer.primitive(medium_foliage, 1))
    rich_billboard_mesh = billboard_mesh(height * entry["crown"], height, 4)
    far_billboard_mesh = billboard_mesh(height * entry["crown"], height, 2)
    rich_billboard = buffer.primitive(rich_billboard_mesh, 2)
    far_billboard = buffer.primitive(far_billboard_mesh, 2)
    bin_name = f"{entry['id']}_{variant}.bin"
    (ASSETS / bin_name).write_bytes(buffer.data)

    seasons = ["summer", "winter"]
    if entry["deciduous"]:
        seasons.insert(1, "autumn")
    for season in seasons:
        render_impostor(entry, variant, season, branch, render_foliage,
                        height, height * entry["crown"])
        write_gltf(entry, variant, season, near_primitives, medium_primitives,
                   buffer, rich_billboard,
                   far_billboard, bin_name)
    write_object(entry, variant_index, height, crown_extent(branch, foliage))
    triangles = [len(branch["indices"]) // 3 + len(foliage["indices"]) // 3,
                 len(medium_branch["indices"]) // 3 + len(medium_foliage["indices"]) // 3,
                 len(rich_billboard_mesh["indices"]) // 3,
                 len(far_billboard_mesh["indices"]) // 3]
    return {"id": f"{entry['id']}_{variant}", "triangles": triangles,
            "height": height}


def write_manifest() -> None:
    (MOD / "mod.ron").write_text('''// Artist-authored CC0 trees; see LICENSES.md and tools/trees/README.md.
(
    id: "trees",
    name: "Bäume Mitteleuropas",
    version: "2.0.0",
    author: "Mantissa / Poly Haven / Connected Rails",
    description: "28 mitteleuropäische Baumarten und Sträucher auf Basis CC0-lizenzierter Künstlerbäume: drei Individuen je Art, vier LOD-Stufen sowie Sommer-, Herbst- und Winterausführungen.",
    depends: [],
    enabled: true,
)
''', encoding="utf-8")


def audit(catalogue: dict, report: list[dict]) -> None:
    expected = len(catalogue["species"]) * 3
    if len(report) != expected:
        raise RuntimeError(f"built {len(report)} trees, expected {expected}")
    for row in report:
        model = json.loads((ASSETS / f"{row['id']}.gltf").read_text())
        names = [node["name"] for node in model["nodes"]]
        if names != ["crown_LOD0", "crown_LOD1", "crown_LOD2", "crown_LOD3"]:
            raise RuntimeError(f"bad LOD nodes in {row['id']}: {names}")
        audit_lod_identity(model, row["id"], "summer", row["id"])
        for material in model["materials"]:
            pbr = material.get("pbrMetallicRoughness", {})
            if pbr.get("metallicFactor") != 0.0 or "roughnessFactor" not in pbr:
                raise RuntimeError(f"non-PBR material in {row['id']}")
            if material.get("name") == "foliage" and material.get("alphaMode") == "BLEND":
                raise RuntimeError(f"depthless blended foliage in {row['id']}")
    means = [sum(row["triangles"]) for row in report]
    print(f"\n{len(report)} trees; four LODs each; mean triangle sum {sum(means)//len(means):,}")
    print(f"LOD0 max {max(row['triangles'][0] for row in report):,}; "
          f"LOD1 max {max(row['triangles'][1] for row in report):,}; "
          f"LOD2 max {max(row['triangles'][2] for row in report):,}; LOD3: 4")


def audit_lod_identity(model: dict, stem: str, season: str, context: str) -> None:
    """The billboard LODs must all show this model, never a sibling tree."""
    expected = f"impostors/{stem}_{season}.png"
    material = model["materials"][2]
    texture_index = material["pbrMetallicRoughness"]["baseColorTexture"]["index"]
    image_index = model["textures"][texture_index]["source"]
    actual = model["images"][image_index]["uri"]
    if actual != expected:
        raise RuntimeError(f"foreign LOD impostor in {context}: {actual}, expected {expected}")
    # LOD1 deliberately remains bark/foliage geometry. Only the genuinely
    # distant LOD2/3 levels may use the whole-tree render.
    if any(primitive.get("material") not in (0, 1)
           for primitive in model["meshes"][1]["primitives"]):
        raise RuntimeError(f"LOD1 is not reduced source geometry in {context}")
    near_min, near_max = mesh_bounds(model, 0)
    medium_min, medium_max = mesh_bounds(model, 1)
    for axis, label in enumerate(("width", "height", "depth")):
        near_extent = near_max[axis] - near_min[axis]
        medium_extent = medium_max[axis] - medium_min[axis]
        ratio = medium_extent / max(near_extent, 1e-6)
        if not 0.65 <= ratio <= 1.20:
            raise RuntimeError(
                f"LOD1 {label} changes silhouette in {context}: {ratio:.2f} of LOD0")
        near_centre = (near_min[axis] + near_max[axis]) * 0.5
        medium_centre = (medium_min[axis] + medium_max[axis]) * 0.5
        if abs(medium_centre - near_centre) > max(near_extent * 0.15, 0.1):
            raise RuntimeError(f"LOD1 {label} shifts away from LOD0 in {context}")
    for level, mesh in enumerate(model["meshes"][2:], 2):
        if any(primitive.get("material") != 2 for primitive in mesh["primitives"]):
            raise RuntimeError(f"LOD{level} does not use its whole-tree impostor in {context}")


def mesh_bounds(model: dict, mesh_index: int) -> tuple[list[float], list[float]]:
    accessors = [model["accessors"][primitive["attributes"]["POSITION"]]
                 for primitive in model["meshes"][mesh_index]["primitives"]]
    return ([min(accessor["min"][axis] for accessor in accessors) for axis in range(3)],
            [max(accessor["max"][axis] for accessor in accessors) for axis in range(3)])


def read_accessor(model: dict, binary: bytes, accessor_index: int) -> list:
    accessor = model["accessors"][accessor_index]
    view = model["bufferViews"][accessor["bufferView"]]
    component = accessor["componentType"]
    code, size = {5123: ("H", 2), 5125: ("I", 4), 5126: ("f", 4)}[component]
    dimensions = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}[accessor["type"]]
    stride = view.get("byteStride", size * dimensions)
    start = view.get("byteOffset", 0) + accessor.get("byteOffset", 0)
    values = []
    for index in range(accessor["count"]):
        value = struct.unpack_from(f"<{dimensions}{code}", binary, start + index * stride)
        values.append(value[0] if dimensions == 1 else value)
    return values


def audit_branch_mesh_quality(model: dict, binary: bytes, context: str) -> None:
    branches = []
    for level in (0, 1):
        primitive = next(part for part in model["meshes"][level]["primitives"]
                         if part.get("material") == 0)
        mesh = {
            "positions": read_accessor(model, binary, primitive["attributes"]["POSITION"]),
            "indices": read_accessor(model, binary, primitive["indices"]),
        }
        area_ratio = maximum_triangle_area_ratio(mesh)
        if area_ratio > 0.008:
            raise RuntimeError(
                f"exploded branch triangle in {context} LOD{level}: "
                f"area is {area_ratio:.3%} of squared tree height")
        if context.startswith(("bergahorn_", "spitzahorn_")):
            edge_ratio = maximum_triangle_edge_ratio(mesh)
            limit = (0.04, 0.09)[level]
            if edge_ratio > limit:
                raise RuntimeError(
                    f"collapsed maple branch in {context} LOD{level}: "
                    f"edge spans {edge_ratio:.1%} of tree height")
        branches.append(mesh)
    audit_trunk_retention(branches[0], branches[1], context)


def audit_existing(catalogue: dict) -> None:
    report = []
    for entry in catalogue["species"]:
        for variant in VARIANTS:
            stem = f"{entry['id']}_{variant}"
            object_path = OBJECTS / f"{stem}.ron"
            if not object_path.exists():
                raise RuntimeError(f"missing object: {object_path}")
            required = ["summer", "winter"] + (["autumn"] if entry["deciduous"] else [])
            triangles = None
            for season in required:
                suffix = "" if season == "summer" else "_herbst" if season == "autumn" else "_winter"
                path = ASSETS / f"{stem}{suffix}.gltf"
                if not path.exists():
                    raise RuntimeError(f"missing seasonal model: {path}")
                model = json.loads(path.read_text(encoding="utf-8"))
                names = [node.get("name") for node in model.get("nodes", [])]
                if names != ["crown_LOD0", "crown_LOD1", "crown_LOD2", "crown_LOD3"]:
                    raise RuntimeError(f"bad LOD nodes in {path.name}: {names}")
                audit_lod_identity(model, stem, season, path.name)
                for material in model.get("materials", []):
                    pbr = material.get("pbrMetallicRoughness", {})
                    if pbr.get("metallicFactor") != 0.0 or "roughnessFactor" not in pbr:
                        raise RuntimeError(f"non-PBR material in {path.name}")
                    if material.get("name") == "foliage" and material.get("alphaMode") == "BLEND":
                        raise RuntimeError(f"depthless blended foliage in {path.name}")
                for image in model.get("images", []):
                    if not (ASSETS / image["uri"]).exists():
                        raise RuntimeError(f"missing texture {image['uri']} for {path.name}")
                binary = ASSETS / model["buffers"][0]["uri"]
                if not binary.exists() or binary.stat().st_size != model["buffers"][0]["byteLength"]:
                    raise RuntimeError(f"bad buffer for {path.name}")
                if season == "summer":
                    audit_branch_mesh_quality(model, binary.read_bytes(), path.name)
                    triangles = [sum(model["accessors"][primitive["indices"]]["count"] // 3
                                     for primitive in mesh["primitives"])
                                 for mesh in model["meshes"]]
            report.append({"id": stem, "triangles": triangles})
    audit(catalogue, report)
    print(f"seasonal models: {len(list(ASSETS.glob('*.gltf')))}; "
          f"objects: {len(list(OBJECTS.glob('*.ron')))}; PBR audit: OK")


def contact_sheet() -> None:
    command = shutil.which("montage")
    if not command:
        return
    sources = sorted(IMPOSTORS.glob("*_summer.png"))
    if sources:
        target = ROOT / "screenshots" / "trees-mantissa-contact-sheet.png"
        target.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run([command, *map(str, sources), "-thumbnail", "160x80", "-tile", "6x",
                        "-geometry", "+4+4", str(target)], check=True)
        print(f"visual audit sheet: {target}")


def build_imported_tasks(objects: list[bpy.types.Object], source_tasks: list[tuple[dict, int]],
                         report: list[dict], up_axis: int = 2) -> None:
    """Extract one artist individual and build every catalogue mapping that uses it."""
    # If a source provides both meshes they are alternatives, not pieces of one
    # tree.  Most Poly Haven imports deliberately begin with the clean artist
    # LOD1, which is then reduced once more for the game's medium level.
    artist_medium_objects = [obj for obj in objects if obj.name.lower().endswith("_lod1")]
    non_medium_objects = [obj for obj in objects if not obj.name.lower().endswith("_lod1")]
    detailed_objects = non_medium_objects or objects
    medium_objects = artist_medium_objects if non_medium_objects else detailed_objects
    axis = AxisMap(detailed_objects, up_axis)
    # The maple pack contains nearly half a million disconnected microscopic
    # Branches_Small faces. A collapse decimator bridges their open tubes with
    # long triangular sails. The intact trunk, major branches and unchanged leaf
    # groups make the same silhouette without that unsafe geometry.
    include_small_branches = not any(entry["source"] == "maple"
                                     for entry, _ in source_tasks)
    detailed_branches = build_branch_object(detailed_objects, include_small_branches)
    branch_kinds = {entry["kind"] for entry, _ in source_tasks}
    maximum_budget = max(BRANCH_BUDGET[kind][0] for kind in branch_kinds)
    medium_budget = max(
        max(BRANCH_BUDGET[kind][1] for kind in branch_kinds),
        PINE_MEDIUM_BRANCH_BUDGET
        if any(entry["source"] == "polyhaven_pine" for entry, _ in source_tasks)
        else 0,
    )
    if not include_small_branches and has_separate_trunk(detailed_objects):
        detailed_trunk_source = build_branch_object(
            detailed_objects, include_small_branches, trunk_only=True)
        detailed_bough_source = build_branch_object(
            detailed_objects, include_small_branches, exclude_trunk=True)
        detailed_branch = merge_meshes(
            simplify_branch(detailed_trunk_source, NEAR_TRUNK_BUDGET),
            simplify_branch(detailed_bough_source, maximum_budget),
        )
    else:
        detailed_trunk_source = None
        detailed_bough_source = None
        detailed_branch = simplify_branch(detailed_branches, maximum_budget)
    if has_separate_trunk(medium_objects):
        medium_trunk_source = build_branch_object(
            medium_objects, include_small_branches, trunk_only=True)
        medium_bough_source = build_branch_object(
            medium_objects, include_small_branches, exclude_trunk=True)
        medium_branch = merge_meshes(
            simplify_branch(medium_trunk_source, MEDIUM_TRUNK_BUDGET),
            simplify_branch(medium_bough_source, medium_budget),
        )
    else:
        medium_trunk_source = None
        medium_bough_source = None
        medium_branch = simplify_branch(detailed_branches, medium_budget)
    branches = [detailed_branch, medium_branch]
    groups = foliage_groups(detailed_objects)
    # Raw source objects are huge; extracted arrays are plain Python and survive cleanup.
    clear_scene()
    for entry, variant_index in source_tasks:
        report.append(build_tree(entry, variant_index, axis, branches, groups))
        print(f"  {report[-1]['id']}: {report[-1]['triangles']}", flush=True)
    del objects, detailed_objects, medium_objects, artist_medium_objects
    del non_medium_objects, detailed_branches, detailed_branch, medium_branch
    del detailed_trunk_source, detailed_bough_source
    del medium_trunk_source, medium_bough_source, branches, groups
    gc.collect()


def main() -> None:
    args = parse_args()
    catalogue = json.loads((HERE / "catalogue.json").read_text(encoding="utf-8"))
    if args.audit:
        audit_existing(catalogue)
        contact_sheet()
        return
    if args.source_root:
        catalogue["source_root"] = str(args.source_root)
    only = set(args.only.split(",")) if args.only else None
    if only:
        unknown = only - {entry["id"] for entry in catalogue["species"]}
        if unknown:
            raise ValueError(f"unknown species: {', '.join(sorted(unknown))}")
    tasks = wanted_tasks(catalogue, only)
    missing = [path for path in tasks if not path.exists()]
    if missing:
        raise FileNotFoundError("missing Mantissa sources (see tools/trees/README.md):\n" +
                                "\n".join(map(str, missing)))

    full_build = only is None
    if full_build:
        clean_generated()
        write_manifest()
    else:
        ASSETS.mkdir(parents=True, exist_ok=True)
        OBJECTS.mkdir(parents=True, exist_ok=True)
        IMPOSTORS.mkdir(parents=True, exist_ok=True)
    prepare_textures(Path(os.path.expanduser(catalogue["source_root"])))
    report = []
    for number, (path, source_tasks) in enumerate(sorted(tasks.items()), 1):
        print(f"[{number}/{len(tasks)}] {path.name}: {len(source_tasks)} catalogue trees", flush=True)
        if "/polyhaven/" in path.as_posix():
            # Poly Haven stores its three genuinely different individuals in
            # one Blend file. Import one at a time so a catalogue variant can never
            # accidentally combine all three and change identity at its LOD.
            for source_variant, letter in enumerate(VARIANTS, 1):
                selected_tasks = [task for task in source_tasks
                                  if source_index(task[0], task[1]) == source_variant]
                if not selected_tasks:
                    continue
                imported = import_source(path)
                objects = [obj for obj in imported if f"_{letter}" in obj.name.lower()]
                if not objects:
                    raise RuntimeError(f"{path.name} has no variant {letter}")
                build_imported_tasks(objects, selected_tasks, report)
            continue
        objects = import_source(path)
        # The fir pack is authored Y-up; all other Mantissa packs are Z-up.
        # Do not guess from the longest bounding-box edge: old, wide-crowned
        # broadleaves can genuinely be wider than they are tall.
        build_imported_tasks(objects, source_tasks, report,
                             1 if "/fir/" in path.as_posix() else 2)

    if full_build:
        audit(catalogue, report)
        contact_sheet()


if __name__ == "__main__":
    main()
