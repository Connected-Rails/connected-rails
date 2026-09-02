#!/usr/bin/env python3
"""Compose repeatable hedge sections from the checked-in CC0 plant models.

The source meshes, textures and their four authored game LODs remain untouched.
This builder only instances them beneath one LOD parent per hedge section, so a
hedge is dense at arm's length and still uses the vegetation renderer's normal
distance selection.  No new third-party material is introduced.
"""

from __future__ import annotations

import argparse
import copy
import json
import math
import random
import re
import struct
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
ASSETS = ROOT / "mods/trees/assets"
OBJECTS = ROOT / "mods/trees/objects"
VARIANTS = ("a", "b", "c")


RECIPES = (
    {
        "id": "feldhecke",
        "name": "Gemischte Feldhecke",
        "length": 8.0,
        "width": 2.4,
        "heights": (2.4, 3.0, 3.6),
        "spacing": 0.82,
        "wild": True,
        "deciduous": True,
        "sources": ("hundsrose_a", "brombeere_a", "himbeere_a", "berberitze_a", "heckenkirsche_a"),
        "tags": ("hecke", "feldhecke", "stand-feld", "stand-boeschung", "stand-bahndamm"),
    },
    {
        "id": "weissdornhecke",
        "name": "Weißdornhecke",
        "length": 6.0,
        "width": 1.7,
        "heights": (1.6, 2.0, 2.5),
        "spacing": 0.64,
        "wild": False,
        "deciduous": True,
        "sources": ("hundsrose_a", "berberitze_a", "heckenkirsche_a"),
        "tags": ("hecke", "weissdorn", "stand-feld", "stand-boeschung", "stand-bahndamm"),
    },
    {
        "id": "ligusterhecke",
        "name": "Ligusterhecke",
        "length": 6.0,
        "width": 1.3,
        "heights": (1.3, 1.7, 2.1),
        "spacing": 0.58,
        "wild": False,
        "deciduous": True,
        "sources": ("himbeere_a", "himbeere_b", "himbeere_c"),
        "tags": ("hecke", "formhecke", "stand-stadt", "stand-park", "stand-feld"),
    },
    {
        "id": "hainbuchenhecke",
        "name": "Hainbuchenhecke",
        "length": 6.0,
        "width": 1.5,
        "heights": (1.6, 2.0, 2.5),
        "spacing": 0.62,
        "wild": False,
        "deciduous": True,
        "sources": ("berberitze_a", "heckenkirsche_a", "himbeere_b"),
        "tags": ("hecke", "formhecke", "stand-stadt", "stand-park", "stand-feld"),
    },
    {
        "id": "immergruene_hecke",
        "name": "Immergrüne Formhecke",
        "length": 6.0,
        "width": 1.4,
        "heights": (1.4, 1.8, 2.2),
        "spacing": 0.58,
        "wild": False,
        "deciduous": False,
        "sources": ("besenheide_c", "besenginster_c"),
        "tags": ("hecke", "formhecke", "immergruen", "stand-stadt", "stand-park"),
    },
)


def suffix(season: str) -> str:
    return "" if season == "summer" else "_herbst" if season == "autumn" else "_winter"


def source_path(stem: str, season: str) -> Path:
    return ASSETS / f"{stem}{suffix(season)}.gltf"


def mesh_bounds(model: dict, mesh_index: int = 0) -> tuple[list[float], list[float]]:
    accessors = [model["accessors"][part["attributes"]["POSITION"]]
                 for part in model["meshes"][mesh_index]["primitives"]]
    return (
        [min(accessor["min"][axis] for accessor in accessors) for axis in range(3)],
        [max(accessor["max"][axis] for accessor in accessors) for axis in range(3)],
    )


def placements(recipe: dict, variant_index: int) -> list[dict]:
    """Deterministic overlapping plants, with matching ends for repeated sections."""
    rng = random.Random(f"connected-rails:{recipe['id']}:{variant_index}")
    count = math.ceil(recipe["length"] / recipe["spacing"]) + 1
    step = recipe["length"] / (count - 1)
    source_kinds = recipe["sources"]
    result = []
    for index in range(count):
        # Cycle source species before adding the variant offset. A mixed hedge
        # consequently remains mixed even if its random stream changes later.
        source = source_kinds[(index + variant_index) % len(source_kinds)]
        individual = VARIANTS[(index * 2 + variant_index) % len(VARIANTS)]
        wild = recipe["wild"]
        height_factor = rng.uniform(0.82, 1.16) if wild else rng.uniform(0.96, 1.04)
        result.append({
            "stem": source if re.search(r"_[abc]$", source) else f"{source}_{individual}",
            "x": -recipe["length"] * 0.5 + index * step,
            "z": rng.uniform(-recipe["width"] * (0.22 if wild else 0.10),
                             recipe["width"] * (0.22 if wild else 0.10)),
            "height": recipe["heights"][variant_index] * height_factor,
            "along": step * (1.65 if wild else 1.45),
            "depth": recipe["width"] * rng.uniform(0.78, 1.04),
            "yaw": rng.uniform(-math.pi, math.pi) if wild else rng.uniform(-0.16, 0.16),
        })
    if not recipe["wild"]:
        # A second, shorter staggered row fills the natural bare foot of each
        # source shrub. It turns individually legible plants into a continuous
        # clipped volume without stretching leaves or adding a solid box.
        for index in range(count - 1):
            source = source_kinds[(index + variant_index + 1) % len(source_kinds)]
            individual = VARIANTS[(index + variant_index) % len(VARIANTS)]
            result.append({
                "stem": source if re.search(r"_[abc]$", source) else f"{source}_{individual}",
                "x": -recipe["length"] * 0.5 + (index + 0.5) * step,
                "z": rng.uniform(-recipe["width"] * 0.18, recipe["width"] * 0.18),
                "height": recipe["heights"][variant_index] * rng.uniform(0.55, 0.70),
                "along": step * 1.55,
                "depth": recipe["width"] * rng.uniform(0.82, 1.04),
                "yaw": rng.uniform(-0.22, 0.22),
            })
    return result


# Safe inset rectangles around seven individual leaves in the Poly Haven
# Searsia alpha atlas. Coordinates use glTF's top-left texture convention.
LEAF_UVS = (
    (0.049, 0.074, 0.199, 0.221),
    (0.219, 0.066, 0.338, 0.166),
    (0.279, 0.178, 0.443, 0.299),
    (0.021, 0.205, 0.178, 0.396),
    (0.178, 0.227, 0.320, 0.426),
    (0.361, 0.203, 0.498, 0.400),
    (0.498, 0.043, 0.600, 0.252),
)
# Distance levels use fewer cards, but progressively larger individual leaves.
# At their hand-over distance those leaves remain around a pixel wide while
# deliberately providing substantially more projected coverage than LOD0.
SHELL_COUNTS = (7200, 5600, 5500, 5400, 5300, 5200,
                5100, 5000, 4900, 4800, 4650, 4500)
# Keep roughly the successful LOD1 leaf size in screen pixels from the 250 m
# hand-over onward. These large world-space cards are only active
# once perspective has reduced them to the same few pixels as an LOD1 leaf.
SHELL_SCALES = (1.0, 2.0, 2.7, 3.6, 4.8, 6.3,
                8.2, 10.5, 12.8, 15.5, 18.5, 22.0)
# Mip filtering averages an increasingly large transparent area around each
# cut-out leaf. Lower thresholds compensate that loss without introducing a
# blended or whole-section billboard.
SHELL_ALPHA_CUTOFFS = (0.18, 0.08, 0.065, 0.055, 0.045, 0.038,
                       0.032, 0.027, 0.023, 0.020, 0.017, 0.015)
LOD_COUNT = len(SHELL_COUNTS)


def shell_colour(recipe: dict, season: str) -> tuple[float, float, float, float]:
    if season == "autumn":
        return {
            "feldhecke": (0.78, 0.48, 0.13, 1.0),
            "weissdornhecke": (0.72, 0.37, 0.10, 1.0),
            "ligusterhecke": (0.68, 0.62, 0.24, 1.0),
            "hainbuchenhecke": (0.78, 0.50, 0.14, 1.0),
        }.get(recipe["id"], (0.62, 0.88, 0.62, 1.0))
    if season == "winter":
        return ((0.60, 0.42, 0.22, 1.0) if recipe["deciduous"]
                else (0.50, 0.82, 0.56, 1.0))
    return (0.58, 0.96, 0.66, 1.0)


def shell_fraction(recipe: dict, season: str) -> float:
    if season != "winter":
        return 1.0
    return {
        "feldhecke": 0.14,
        "weissdornhecke": 0.06,
        "ligusterhecke": 0.48,
        "hainbuchenhecke": 0.40,
        "immergruene_hecke": 1.0,
    }[recipe["id"]]


def shell_mesh(recipe: dict, variant_index: int, level: int) -> dict:
    """A dense volume of small, individually cut-out artist leaf cards."""
    rng = random.Random(f"connected-rails:hedge-shell:{recipe['id']}:{variant_index}:{level}")
    length = recipe["length"]
    width = recipe["width"]
    height = recipe["heights"][variant_index]
    wild = recipe["wild"]
    positions = []
    normals = []
    uvs = []
    indices = []
    for card in range(SHELL_COUNTS[level]):
        face = rng.random()
        x = rng.uniform(-length * 0.5, length * 0.5)
        local_top = height
        if wild:
            local_top *= 0.82 + 0.12 * math.sin(x * 1.7 + variant_index) + rng.uniform(-0.08, 0.10)
        y = rng.uniform(0.10, max(0.12, local_top - 0.04))
        z = rng.uniform(-width * 0.48, width * 0.48)
        leaf_w = (rng.uniform(0.13, 0.24) * (1.18 if wild else 1.0)
                  * SHELL_SCALES[level])
        leaf_h = leaf_w * rng.uniform(0.48, 0.72)

        if face < 0.60:
            # The two long faces carry most of the visible foliage.
            sign = -1.0 if rng.random() < 0.5 else 1.0
            z = sign * width * rng.uniform(0.40, 0.53)
            normal = (rng.uniform(-0.18, 0.18), rng.uniform(-0.15, 0.20), sign)
            right = (1.0, 0.0, -normal[0] * sign)
            up = (0.0, 1.0, -normal[1] * sign)
        elif face < 0.74:
            # A leafy top keeps a formal hedge closed when seen from above.
            y = local_top + rng.uniform(-0.05, 0.04)
            normal = (rng.uniform(-0.12, 0.12), 1.0, rng.uniform(-0.12, 0.12))
            right = (1.0, -normal[0], 0.0)
            up = (0.0, -normal[2], 1.0)
        elif face < 0.82:
            sign = -1.0 if rng.random() < 0.5 else 1.0
            x = sign * length * rng.uniform(0.46, 0.51)
            normal = (sign, rng.uniform(-0.12, 0.16), rng.uniform(-0.18, 0.18))
            right = (-normal[2] * sign, 0.0, 1.0)
            up = (-normal[1] * sign, 1.0, 0.0)
        else:
            # Interior cards stop oblique views from opening into an empty box.
            angle = rng.uniform(-math.pi, math.pi)
            normal = (math.sin(angle), rng.uniform(-0.15, 0.20), math.cos(angle))
            right = (math.cos(angle), 0.0, -math.sin(angle))
            up = (0.0, 1.0, 0.0)

        base = len(positions)
        centre = (x, y, z)
        for sx, sy in ((-1, -1), (1, -1), (1, 1), (-1, 1)):
            positions.append(tuple(centre[i] + right[i] * sx * leaf_w * 0.5
                                   + up[i] * sy * leaf_h * 0.5 for i in range(3)))
            normals.append(normal)
        u0, v0, u1, v1 = LEAF_UVS[(card + variant_index) % len(LEAF_UVS)]
        uvs.extend(((u0, v1), (u1, v1), (u1, v0), (u0, v0)))
        indices.extend((base, base + 1, base + 2, base, base + 2, base + 3))
    return {"positions": positions, "normals": normals, "uvs": uvs, "indices": indices}


class Composer:
    def __init__(self, recipe: dict, variant_index: int, season: str):
        self.recipe = recipe
        self.variant_index = variant_index
        self.season = season
        self.model = {
            "asset": {
                "version": "2.0",
                "generator": "Connected Rails CC0 hedge composer",
                "copyright": "Mantissa and Poly Haven source models: CC0 1.0; hedge composition: Connected Rails",
            },
            "scene": 0,
            "scenes": [{"nodes": list(range(LOD_COUNT))}],
            "nodes": [{"name": f"hedge_LOD{level}", "children": []}
                      for level in range(LOD_COUNT)],
            "meshes": [],
            "buffers": [],
            "bufferViews": [],
            "accessors": [],
            "images": [],
            "samplers": [],
            "textures": [],
            "materials": [],
        }
        self.imported: dict[str, tuple[list[int], tuple[list[float], list[float]]]] = {}

    def add_shell(self) -> None:
        """Add the continuous leaf volume around the source branches."""
        variant = VARIANTS[self.variant_index]
        binary_name = f"{self.recipe['id']}_{variant}_hedge.bin"
        binary = bytearray()
        buffer_index = len(self.model["buffers"])

        def view(payload: bytes, target: int) -> int:
            while len(binary) % 4:
                binary.append(0)
            offset = len(binary)
            binary.extend(payload)
            self.model["bufferViews"].append({
                "buffer": buffer_index,
                "byteOffset": offset,
                "byteLength": len(payload),
                "target": target,
            })
            return len(self.model["bufferViews"]) - 1

        def vectors(values: list[tuple], dimensions: int) -> int:
            flat = [component for value in values for component in value]
            accessor = {
                "bufferView": view(struct.pack(f"<{len(flat)}f", *flat), 34962),
                "componentType": 5126,
                "count": len(values),
                "type": "VEC3" if dimensions == 3 else "VEC2",
            }
            if dimensions == 3:
                accessor["min"] = [min(value[axis] for value in values) for axis in range(3)]
                accessor["max"] = [max(value[axis] for value in values) for axis in range(3)]
            self.model["accessors"].append(accessor)
            return len(self.model["accessors"]) - 1

        def triangles(values: list[int], fraction: float) -> int:
            accessor = {
                "bufferView": view(struct.pack(f"<{len(values)}H", *values), 34963),
                "componentType": 5123,
                "count": max(6, int(len(values) * fraction) // 6 * 6),
                "type": "SCALAR",
                "min": [0],
                "max": [max(values)],
            }
            self.model["accessors"].append(accessor)
            return len(self.model["accessors"]) - 1

        sampler = len(self.model["samplers"])
        self.model["samplers"].append({
            "magFilter": 9729, "minFilter": 9987, "wrapS": 10497, "wrapT": 10497,
        })
        image = len(self.model["images"])
        self.model["images"].append({"uri": "textures/poly_searsia_lucida.png"})
        texture = len(self.model["textures"])
        self.model["textures"].append({"source": image, "sampler": sampler})
        fraction = shell_fraction(self.recipe, self.season)
        for level in range(LOD_COUNT):
            material = len(self.model["materials"])
            self.model["materials"].append({
                "name": f"{self.recipe['id']}_individual_leaves_LOD{level}",
                "pbrMetallicRoughness": {
                    "baseColorFactor": list(shell_colour(self.recipe, self.season)),
                    "baseColorTexture": {"index": texture},
                    "metallicFactor": 0.0,
                    "roughnessFactor": 0.82,
                },
                "doubleSided": True,
                "alphaMode": "MASK",
                "alphaCutoff": SHELL_ALPHA_CUTOFFS[level],
            })
            mesh = shell_mesh(self.recipe, self.variant_index, level)
            primitive = {
                "attributes": {
                    "POSITION": vectors(mesh["positions"], 3),
                    "NORMAL": vectors(mesh["normals"], 3),
                    "TEXCOORD_0": vectors(mesh["uvs"], 2),
                },
                "indices": triangles(mesh["indices"], fraction),
                "material": material,
            }
            mesh_index = len(self.model["meshes"])
            self.model["meshes"].append({
                "name": f"{self.recipe['id']}_leaf_volume_LOD{level}",
                "primitives": [primitive],
            })
            node_index = len(self.model["nodes"])
            self.model["nodes"].append({
                "name": f"leaf_volume_LOD{level}",
                "mesh": mesh_index,
            })
            self.model["nodes"][level]["children"].append(node_index)

        self.model["buffers"].append({"uri": binary_name, "byteLength": len(binary)})
        (ASSETS / binary_name).write_bytes(binary)

    def import_stem(self, stem: str) -> tuple[list[int], tuple[list[float], list[float]]]:
        cached = self.imported.get(stem)
        if cached is not None:
            return cached
        source = json.loads(source_path(stem, self.season).read_text(encoding="utf-8"))

        buffer_offset = len(self.model["buffers"])
        view_offset = len(self.model["bufferViews"])
        accessor_offset = len(self.model["accessors"])
        image_offset = len(self.model["images"])
        sampler_offset = len(self.model["samplers"])
        texture_offset = len(self.model["textures"])
        material_offset = len(self.model["materials"])
        mesh_offset = len(self.model["meshes"])

        self.model["buffers"].extend(copy.deepcopy(source.get("buffers", [])))
        for view in copy.deepcopy(source.get("bufferViews", [])):
            view["buffer"] += buffer_offset
            self.model["bufferViews"].append(view)
        for accessor in copy.deepcopy(source.get("accessors", [])):
            accessor["bufferView"] += view_offset
            self.model["accessors"].append(accessor)
        self.model["images"].extend(copy.deepcopy(source.get("images", [])))
        self.model["samplers"].extend(copy.deepcopy(source.get("samplers", [])))
        for texture in copy.deepcopy(source.get("textures", [])):
            texture["source"] += image_offset
            if "sampler" in texture:
                texture["sampler"] += sampler_offset
            self.model["textures"].append(texture)
        for material in copy.deepcopy(source.get("materials", [])):
            pbr = material.get("pbrMetallicRoughness", {})
            if "baseColorTexture" in pbr:
                pbr["baseColorTexture"]["index"] += texture_offset
            if "normalTexture" in material:
                material["normalTexture"]["index"] += texture_offset
            if "occlusionTexture" in material:
                material["occlusionTexture"]["index"] += texture_offset
            if "emissiveTexture" in material:
                material["emissiveTexture"]["index"] += texture_offset
            material["name"] = f"{stem}_{material.get('name', 'material')}"
            self.model["materials"].append(material)
        for mesh in copy.deepcopy(source["meshes"]):
            for primitive in mesh["primitives"]:
                primitive["indices"] += accessor_offset
                primitive["attributes"] = {
                    name: index + accessor_offset for name, index in primitive["attributes"].items()
                }
                if "material" in primitive:
                    primitive["material"] += material_offset
            mesh["name"] = f"{stem}_{mesh.get('name', 'mesh')}"
            self.model["meshes"].append(mesh)

        result = ([mesh_offset + level for level in range(4)], mesh_bounds(source))
        self.imported[stem] = result
        return result

    def compose(self) -> dict:
        plants = placements(self.recipe, self.variant_index)
        # Give every source shrub a deterministic, spatially mixed priority.
        # Keeping successively shorter prefixes of this order makes every far
        # LOD a strict subset of the previous one without stripping the hedge
        # progressively from one end.
        priority_order = sorted(
            range(len(plants)),
            key=lambda index: random.Random(
                f"connected-rails:hedge-source-priority:{self.recipe['id']}:"
                f"{self.variant_index}:{index}"
            ).random(),
        )
        priority = {plant_index: rank for rank, plant_index in enumerate(priority_order)}
        source_fractions = (1.0, 1.0, 0.60, 0.60, 0.42, 0.42,
                            0.30, 0.30, 0.22, 0.22, 0.16, 0.12)
        source_counts = tuple(
            max(1, math.ceil(len(plants) * fraction)) for fraction in source_fractions
        )
        for plant_index, plant in enumerate(plants):
            meshes, (lo, hi) = self.import_stem(plant["stem"])
            extent = [max(hi[axis] - lo[axis], 1e-5) for axis in range(3)]
            scale = [plant["along"] / extent[0], plant["height"] / extent[1], plant["depth"] / extent[2]]
            angle = plant["yaw"]
            rotation = [0.0, math.sin(angle * 0.5), 0.0, math.cos(angle * 0.5)]
            translation = [plant["x"], -lo[1] * scale[1], plant["z"]]
            # Every level stays real 3D. Far levels retain a thinning subset of
            # the artist's reduced mesh instead of switching to its whole-plant
            # impostors; the enlarged individual leaf cards close the gaps.
            source_levels = (1,) * LOD_COUNT
            for level, source_level in enumerate(source_levels):
                if priority[plant_index] >= source_counts[level]:
                    continue
                node = {
                    "name": f"{plant['stem']}_part_{len(self.model['nodes'])}_LOD{level}",
                    "mesh": meshes[source_level],
                    "translation": translation,
                    "rotation": rotation,
                    "scale": scale,
                }
                index = len(self.model["nodes"])
                self.model["nodes"].append(node)
                self.model["nodes"][level]["children"].append(index)
        self.add_shell()
        return self.model


def object_text(recipe: dict, variant_index: int) -> str:
    variant = VARIANTS[variant_index]
    stem = f"{recipe['id']}_{variant}"
    height = recipe["heights"][variant_index]
    autumn = f'Some("trees/assets/{stem}_herbst.gltf")' if recipe["deciduous"] else "None"
    tags = ", ".join(json.dumps(tag, ensure_ascii=False) for tag in recipe["tags"])
    return f'''// {recipe["name"]}, {recipe["length"]:g} m section, {height:g} m high.
// Composed from the CC0 artist vegetation documented in mods/trees/LICENSES.md.
(
    name: {json.dumps(recipe["name"] + " " + variant.upper(), ensure_ascii=False)},
    model: "trees/assets/{stem}.gltf",
    autumn_model: {autumn},
    winter_model: Some("trees/assets/{stem}_winter.gltf"),
    lod_distances: [60, 120, 180, 250, 325, 400, 500, 600, 700, 800, 900, 1000],
    footprint: Some((length: {recipe["length"]:g}, width: {recipe["width"]:g})),
    tags: [{tags}],
)
'''


def expected() -> list[tuple[dict, int, str, Path]]:
    rows = []
    for recipe in RECIPES:
        seasons = ("summer", "autumn", "winter") if recipe["deciduous"] else ("summer", "winter")
        for variant_index, variant in enumerate(VARIANTS):
            stem = f"{recipe['id']}_{variant}"
            for season in seasons:
                rows.append((recipe, variant_index, season, ASSETS / f"{stem}{suffix(season)}.gltf"))
    return rows


def build() -> None:
    ASSETS.mkdir(parents=True, exist_ok=True)
    OBJECTS.mkdir(parents=True, exist_ok=True)
    for recipe in RECIPES:
        for variant_index, variant in enumerate(VARIANTS):
            stem = f"{recipe['id']}_{variant}"
            seasons = ("summer", "autumn", "winter") if recipe["deciduous"] else ("summer", "winter")
            for season in seasons:
                target = ASSETS / f"{stem}{suffix(season)}.gltf"
                target.write_text(json.dumps(
                    Composer(recipe, variant_index, season).compose(), separators=(",", ":")),
                    encoding="utf-8",
                )
            (OBJECTS / f"{stem}.ron").write_text(object_text(recipe, variant_index), encoding="utf-8")
    audit()


def audit() -> None:
    projected_coverage = [count * scale * scale
                          for count, scale in zip(SHELL_COUNTS, SHELL_SCALES)]
    if min(projected_coverage[1:]) < projected_coverage[0] * 0.8:
        raise RuntimeError(f"thin hedge distance LOD: projected coverage {projected_coverage}")
    models = 0
    maximum_parts = 0
    maximum_triangles = [0] * LOD_COUNT
    for recipe, variant_index, season, path in expected():
        if not path.exists():
            raise RuntimeError(f"missing hedge model: {path}")
        model = json.loads(path.read_text(encoding="utf-8"))
        roots = model["nodes"][:LOD_COUNT]
        if [node.get("name") for node in roots] != [f"hedge_LOD{i}"
                                                     for i in range(LOD_COUNT)]:
            raise RuntimeError(f"bad hedge LOD roots in {path.name}")
        counts = [len(node.get("children", [])) for node in roots]
        if (counts[0] != counts[1] or counts[-1] < 2
                or any(a < b for a, b in zip(counts, counts[1:]))):
            raise RuntimeError(f"incomplete hedge LODs in {path.name}: {counts}")
        for node in model["nodes"][LOD_COUNT:]:
            if ("mesh" not in node
                    or ("scale" in node and not all(value > 0 for value in node["scale"]))):
                raise RuntimeError(f"bad hedge part in {path.name}")
        triangles = []
        for root in roots:
            total = 0
            for child in root["children"]:
                mesh = model["meshes"][model["nodes"][child]["mesh"]]
                total += sum(model["accessors"][part["indices"]]["count"] // 3
                             for part in mesh["primitives"])
            triangles.append(total)
        # Different source shrubs have very different reduced-mesh weights.
        # A stable spatial subset may therefore fluctuate slightly even while
        # the overall budgets fall; reject only a material (>15%) increase.
        if any(b > a * 1.15 for a, b in zip(triangles, triangles[1:])):
            raise RuntimeError(f"hedge LOD grows with distance in {path.name}: {triangles}")
        if triangles[0] > 150_000:
            raise RuntimeError(f"overweight hedge in {path.name}: {triangles[0]} triangles")
        for material in model["materials"]:
            pbr = material.get("pbrMetallicRoughness", {})
            if pbr.get("metallicFactor") != 0.0 or "roughnessFactor" not in pbr:
                raise RuntimeError(f"non-PBR hedge material in {path.name}")
            if material.get("alphaMode") == "BLEND":
                raise RuntimeError(f"blended hedge foliage in {path.name}")
        shell_cutoffs = [material.get("alphaCutoff") for material in model["materials"]
                         if "_individual_leaves_LOD" in material.get("name", "")]
        if shell_cutoffs != list(SHELL_ALPHA_CUTOFFS):
            raise RuntimeError(f"bad hedge alpha coverage in {path.name}: {shell_cutoffs}")
        for resource in model.get("buffers", []):
            uri = resource["uri"]
            if not uri.startswith("data:"):
                actual_size = (path.parent / uri).stat().st_size
                if actual_size != resource["byteLength"]:
                    raise RuntimeError(
                        f"buffer length mismatch for {path.name}: {uri} "
                        f"declares {resource['byteLength']}, has {actual_size}"
                    )
        for resource in model.get("buffers", []) + model.get("images", []):
            uri = resource["uri"]
            if not uri.startswith("data:") and not (path.parent / uri).exists():
                raise RuntimeError(f"missing {uri} for {path.name}")
        maximum_parts = max(maximum_parts, *counts)
        maximum_triangles = [max(old, new) for old, new in zip(maximum_triangles, triangles)]
        models += 1
    objects = sum((OBJECTS / f"{recipe['id']}_{variant}.ron").exists()
                  for recipe in RECIPES for variant in VARIANTS)
    if objects != len(RECIPES) * len(VARIANTS):
        raise RuntimeError(f"built {objects} hedge objects, expected {len(RECIPES) * len(VARIANTS)}")
    coverage_percent = [round(value / projected_coverage[0] * 100)
                        for value in projected_coverage]
    print(f"hedges: {objects} objects, {models} seasonal models, up to {maximum_parts} parts/LOD; "
          f"triangle maxima {maximum_triangles}; projected coverage {coverage_percent}%; audit: OK")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--audit", action="store_true")
    args = parser.parse_args()
    audit() if args.audit else build()


if __name__ == "__main__":
    main()
