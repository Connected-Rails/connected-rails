#!/usr/bin/env python3
"""Render one signal, or a complete review matrix, with the game renderer.

Examples:
  python tools/signals/preview.py form_hp_8m_gitter_2fl --aspect hp2 --view front
  python tools/signals/preview.py form_vr_4_87m_3begr --matrix --focus head
  python tools/signals/preview.py form_hp_8m_gitter_2fl --animation hp1:hp0 --focus head
  python tools/signals/preview.py form_hp_8m_gitter_2fl --matrix --accept-baseline
  python tools/signals/preview.py form_hp_8m_gitter_2fl --matrix --compare-baseline

The wrapper deliberately drives ``trainsim-signal-editor`` rather than Blender:
the screenshots therefore exercise the same glTF loader, PBR materials, lamp
visibility, motion bindings and LOD selection as the simulator.
"""

from __future__ import annotations

import argparse
import base64
import copy
from datetime import datetime, timezone
import hashlib
import json
import math
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
DEFAULT_OUTPUT = Path("/tmp/connected-rails-signal-preview")
MATRIX_VIEWS = (
    "front",
    "front-left",
    "left",
    "rear",
    "rear-right",
    "right",
)
ALL_VIEWS = (
    "front",
    "front-left",
    "front-right",
    "left",
    "rear",
    "rear-left",
    "rear-right",
    "right",
)
DEFAULT_ANIMATION_TIMES = (0.0, 0.30, 0.60, 0.75, 0.90, 1.20, 1.50, 1.80, 2.10, 2.50)


def single_output_path(requested: Path | None, default: Path) -> Path:
    """Resolve a single capture without mistaking a directory for a PNG.

    A suffixless ``--output`` is intentionally a directory, matching matrix
    and animation mode.  This makes repeated detail crops safe to type and
    prevents the renderer from being handed a directory-looking filename that
    image decoders later cannot recognise.
    """
    if requested is None:
        return default.resolve()
    expanded = requested.expanduser()
    if expanded.is_dir() or not expanded.suffix:
        return (expanded / default.name).resolve()
    if expanded.suffix.lower() != ".png":
        raise SystemExit(
            f"single preview output must be a .png file or a directory: {requested}"
        )
    return expanded.resolve()


def model_path(value: str) -> Path:
    candidate = Path(value)
    if candidate.is_file():
        return candidate.resolve()
    if candidate.suffix != ".ron":
        candidate = candidate.with_suffix(".ron")
    matches = sorted(ROOT.glob(f"mods/*/signal_models/{candidate.name}"))
    if len(matches) != 1:
        found = "none" if not matches else ", ".join(map(str, matches))
        raise SystemExit(f"signal model {value!r} is not unique: {found}")
    return matches[0].resolve()


def aspects_for(name: str) -> tuple[str, ...]:
    if "form_hp" in name:
        if "_gekuppelt" in name:
            return ("hp0", "hp2")
        return ("hp0", "hp1", "hp2") if "_2fl" in name or name == "form_hp" else ("hp0", "hp1")
    if "form_vr" in name:
        return ("vr0", "vr1", "vr2") if "_3begr" in name else ("vr0", "vr1")
    if "form_sh" in name or "formsperr" in name:
        return ("sh0", "sh1")
    return ("none",)


def animation_transition(value: str) -> tuple[str, str]:
    try:
        source, target = value.lower().split(":", 1)
    except ValueError as error:
        raise argparse.ArgumentTypeError("expected FROM:TO, for example hp1:hp0") from error
    if not source or not target:
        raise argparse.ArgumentTypeError("both FROM and TO aspects are required")
    return source, target


def animation_times(value: str) -> tuple[float, ...]:
    try:
        times = tuple(float(item) for item in value.split(","))
    except ValueError as error:
        raise argparse.ArgumentTypeError("times must be comma-separated seconds") from error
    if not times or any(not math.isfinite(item) or item < 0.0 for item in times):
        raise argparse.ArgumentTypeError("times must be finite and non-negative")
    if any(right <= left for left, right in zip(times, times[1:])):
        raise argparse.ArgumentTypeError("times must be strictly increasing")
    return times


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def gltf_external_dependencies(path: Path) -> list[Path]:
    """Resolve non-data buffers and images referenced by one glTF asset."""
    try:
        document = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return []
    result = []
    for entry in (*document.get("buffers", ()), *document.get("images", ())):
        uri = entry.get("uri", "")
        if not uri or uri.startswith("data:") or "://" in uri:
            continue
        dependency = (path.parent / uri).resolve()
        if dependency.is_file() and dependency not in result:
            result.append(dependency)
    return result


def dependencies(model: Path) -> list[Path]:
    text = model.read_text(encoding="utf-8")
    result = []
    for relative in re.findall(r'file:\s*"([^"]+)"', text):
        path = ROOT / "mods" / relative
        if path.is_file() and path.resolve() not in result:
            resolved = path.resolve()
            result.append(resolved)
            result.extend(
                dependency
                for dependency in gltf_external_dependencies(resolved)
                if dependency not in result
            )
    return result


def uri_bytes(path: Path, uri: str) -> bytes:
    if uri.startswith("data:"):
        return base64.b64decode(uri.split(",", 1)[1])
    return (path.parent / uri).resolve().read_bytes()


def canonical_hash(value: object) -> str:
    encoded = json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=True
    ).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


_ACCESSOR_COMPONENT_BYTES = {
    5120: 1,  # BYTE
    5121: 1,  # UNSIGNED_BYTE
    5122: 2,  # SHORT
    5123: 2,  # UNSIGNED_SHORT
    5125: 4,  # UNSIGNED_INT
    5126: 4,  # FLOAT
}
_ACCESSOR_TYPE_COMPONENTS = {
    "SCALAR": 1,
    "VEC2": 2,
    "VEC3": 3,
    "VEC4": 4,
    "MAT2": 4,
    "MAT3": 9,
    "MAT4": 16,
}


def gltf_node_geometry_fingerprints(
    path: Path, document: dict[str, object] | None = None
) -> dict[str, str]:
    """Hash every named mesh node independently, including its placement.

    Whole-file hashes identify that *something* changed, but they cannot allow
    an intentional lamp edit while proving that a blade in the same buffer did
    not move.  These hashes use only the accessor byte ranges consumed by one
    mesh plus the node/ancestor transforms; material indices are deliberately
    excluded.
    """
    if document is None:
        document = json.loads(path.read_text(encoding="utf-8"))
    buffers = [uri_bytes(path, buffer["uri"]) for buffer in document.get("buffers", ())]
    accessors = document.get("accessors", ())
    buffer_views = document.get("bufferViews", ())
    accessor_cache: dict[int, dict[str, object]] = {}

    def accessor_signature(index: int) -> dict[str, object]:
        if index in accessor_cache:
            return accessor_cache[index]
        accessor = copy.deepcopy(accessors[index])
        if "sparse" in accessor:
            raise ValueError(f"sparse glTF accessors are not supported in {path}")
        view_index = accessor.get("bufferView")
        if view_index is None:
            payload = b""
            view_meta: dict[str, object] = {}
        else:
            view = buffer_views[view_index]
            component_bytes = _ACCESSOR_COMPONENT_BYTES[accessor["componentType"]]
            component_count = _ACCESSOR_TYPE_COMPONENTS[accessor["type"]]
            element_bytes = component_bytes * component_count
            stride = view.get("byteStride", element_bytes)
            start = view.get("byteOffset", 0) + accessor.get("byteOffset", 0)
            source = buffers[view["buffer"]]
            payload = b"".join(
                source[start + row * stride:start + row * stride + element_bytes]
                for row in range(accessor["count"])
            )
            view_meta = {
                key: view[key]
                for key in ("byteStride", "target")
                if key in view
            }
        signature = {
            "accessor": {
                key: value
                for key, value in accessor.items()
                if key not in {"bufferView", "byteOffset"}
            },
            "buffer_view": view_meta,
            "payload_sha256": hashlib.sha256(payload).hexdigest(),
        }
        accessor_cache[index] = signature
        return signature

    nodes = document.get("nodes", ())
    meshes = document.get("meshes", ())
    parents: dict[int, int] = {}
    for parent_index, node in enumerate(nodes):
        for child in node.get("children", ()):
            parents[child] = parent_index

    def transform_signature(index: int) -> dict[str, object]:
        node = nodes[index]
        return {
            key: copy.deepcopy(node[key])
            for key in ("matrix", "translation", "rotation", "scale", "weights")
            if key in node
        }

    result: dict[str, str] = {}
    duplicate_counts: dict[str, int] = {}
    for node_index, node in enumerate(nodes):
        mesh_index = node.get("mesh")
        if mesh_index is None:
            continue
        base_name = node.get("name") or f"node-{node_index}"
        occurrence = duplicate_counts.get(base_name, 0)
        duplicate_counts[base_name] = occurrence + 1
        name = base_name if occurrence == 0 else f"{base_name}#{occurrence + 1}"

        ancestry = []
        current: int | None = node_index
        while current is not None:
            ancestry.append({
                "name": nodes[current].get("name"),
                "transform": transform_signature(current),
            })
            current = parents.get(current)
        ancestry.reverse()

        mesh = meshes[mesh_index]
        primitive_signatures = []
        for primitive in mesh.get("primitives", ()):
            signature: dict[str, object] = {
                "mode": primitive.get("mode", 4),
                "attributes": {
                    semantic: accessor_signature(accessor_index)
                    for semantic, accessor_index in sorted(
                        primitive.get("attributes", {}).items()
                    )
                },
            }
            if "indices" in primitive:
                signature["indices"] = accessor_signature(primitive["indices"])
            if "targets" in primitive:
                signature["targets"] = [
                    {
                        semantic: accessor_signature(accessor_index)
                        for semantic, accessor_index in sorted(target.items())
                    }
                    for target in primitive["targets"]
                ]
            primitive_signatures.append(signature)
        result[name] = canonical_hash({
            "ancestry": ancestry,
            "mesh_name": mesh.get("name"),
            "mesh_weights": mesh.get("weights"),
            "primitives": primitive_signatures,
        })
    return result


def gltf_node_shading_fingerprints(
    path: Path, document: dict[str, object] | None = None
) -> dict[str, str]:
    """Hash the material and texture response consumed by each named node.

    A whole-file shading hash cannot safely scope a material edit: changing a
    material shared by a mast and blade affects both even when only the mast
    was meant to change.  Per-node hashes deliberately follow each primitive's
    material through texture, sampler, image metadata and the actual image
    bytes, so a component guard catches that kind of collateral PBR change.
    """
    if document is None:
        document = json.loads(path.read_text(encoding="utf-8"))
    materials = document.get("materials", ())
    textures = document.get("textures", ())
    images = document.get("images", ())
    samplers = document.get("samplers", ())

    def texture_indices(value: object) -> set[int]:
        found: set[int] = set()
        if isinstance(value, dict):
            index = value.get("index")
            if isinstance(index, int):
                found.add(index)
            for child in value.values():
                found.update(texture_indices(child))
        elif isinstance(value, list):
            for child in value:
                found.update(texture_indices(child))
        return found

    material_cache: dict[int, dict[str, object]] = {}

    def material_signature(index: int) -> dict[str, object]:
        if index in material_cache:
            return material_cache[index]
        material = copy.deepcopy(materials[index])
        referenced = []
        for texture_index in sorted(texture_indices(material)):
            texture = copy.deepcopy(textures[texture_index])
            image_index = texture.get("source")
            sampler_index = texture.get("sampler")
            image = (
                copy.deepcopy(images[image_index])
                if isinstance(image_index, int) else None
            )
            referenced.append({
                "texture": texture,
                "sampler": (
                    copy.deepcopy(samplers[sampler_index])
                    if isinstance(sampler_index, int) else None
                ),
                "image": image,
                "image_sha256": (
                    hashlib.sha256(uri_bytes(path, image["uri"])).hexdigest()
                    if isinstance(image, dict) and image.get("uri") else None
                ),
            })
        signature = {"material": material, "textures": referenced}
        material_cache[index] = signature
        return signature

    nodes = document.get("nodes", ())
    meshes = document.get("meshes", ())
    result: dict[str, str] = {}
    duplicate_counts: dict[str, int] = {}
    for node_index, node in enumerate(nodes):
        mesh_index = node.get("mesh")
        if mesh_index is None:
            continue
        base_name = node.get("name") or f"node-{node_index}"
        occurrence = duplicate_counts.get(base_name, 0)
        duplicate_counts[base_name] = occurrence + 1
        name = base_name if occurrence == 0 else f"{base_name}#{occurrence + 1}"
        primitive_materials = []
        for primitive in meshes[mesh_index].get("primitives", ()):
            material_index = primitive.get("material")
            primitive_materials.append(
                material_signature(material_index)
                if isinstance(material_index, int)
                else {"material": "glTF default"}
            )
        result[name] = canonical_hash(primitive_materials)
    return result


def model_fingerprints(path: Path) -> dict[str, object]:
    """Separate global RON configuration from bindings owned by mesh nodes."""
    text = path.read_text(encoding="utf-8")
    bindings: dict[str, list[str]] = {}
    static_lines = []
    for line in text.splitlines():
        match = re.search(r'\bnode:\s*"([^"]+)"', line)
        normalized = " ".join(line.split())
        if match:
            bindings.setdefault(match.group(1), []).append(normalized)
        else:
            static_lines.append(normalized)
    return {
        "static_sha256": canonical_hash(static_lines),
        "node_binding_sha256": {
            node: canonical_hash(lines) for node, lines in sorted(bindings.items())
        },
    }


def gltf_fingerprints(path: Path) -> dict[str, object]:
    """Hash geometry and shading independently for regression attribution."""
    document = json.loads(path.read_text(encoding="utf-8"))
    geometry = {
        key: copy.deepcopy(document.get(key, []))
        for key in ("scene", "scenes", "nodes", "meshes", "accessors", "bufferViews")
    }
    for mesh in geometry["meshes"]:
        for primitive in mesh.get("primitives", ()):
            primitive.pop("material", None)
    geometry_buffers = [
        hashlib.sha256(uri_bytes(path, buffer["uri"])).hexdigest()
        for buffer in document.get("buffers", ())
    ]
    geometry["buffer_sha256"] = geometry_buffers

    shading = {
        key: copy.deepcopy(document.get(key, []))
        for key in ("materials", "samplers", "textures", "images", "extensionsUsed")
    }
    shading["image_sha256"] = [
        hashlib.sha256(uri_bytes(path, image["uri"])).hexdigest()
        for image in document.get("images", ())
    ]

    return {
        "geometry_sha256": canonical_hash(geometry),
        "shading_sha256": canonical_hash(shading),
        "node_geometry_sha256": gltf_node_geometry_fingerprints(path, document),
        "node_shading_sha256": gltf_node_shading_fingerprints(path, document),
    }


def input_fingerprint_manifest(model: Path) -> dict[str, object]:
    """Create the immutable-input part shared by renders and preflight guards."""
    manifest_files = list(dict.fromkeys([model, *dependencies(model)]))
    gltf_files = [path for path in manifest_files if path.suffix.lower() == ".gltf"]
    return {
        "model": str(model.relative_to(ROOT)),
        "files": {str(path.relative_to(ROOT)): sha256(path) for path in manifest_files},
        "model_fingerprints": model_fingerprints(model),
        "gltf_fingerprints": {
            str(path.relative_to(ROOT)): gltf_fingerprints(path)
            for path in gltf_files
        },
    }


def verify_fingerprint_guard(
    current: dict[str, object], expected_path: Path, fingerprint: str
) -> None:
    """Fail closed when a supposedly untouched glTF domain has changed."""
    try:
        expected = json.loads(expected_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise SystemExit(f"cannot read guard manifest {expected_path}: {error}") from error
    if expected.get("model") != current.get("model"):
        raise SystemExit(
            f"guard manifest is for {expected.get('model')!r}, not {current.get('model')!r}"
        )
    expected_gltf = expected.get("gltf_fingerprints")
    current_gltf = current.get("gltf_fingerprints")
    if not isinstance(expected_gltf, dict) or not isinstance(current_gltf, dict):
        raise SystemExit(f"guard manifest lacks glTF fingerprints: {expected_path}")
    if set(expected_gltf) != set(current_gltf):
        raise SystemExit(
            f"guarded glTF dependency set changed: expected {sorted(expected_gltf)}, "
            f"got {sorted(current_gltf)}"
        )
    changed = []
    for name, values in current_gltf.items():
        expected_values = expected_gltf.get(name)
        if not isinstance(values, dict) or not isinstance(expected_values, dict):
            changed.append(name)
        elif values.get(fingerprint) != expected_values.get(fingerprint):
            changed.append(name)
    if changed:
        domain = "geometry" if fingerprint == "geometry_sha256" else "shading"
        raise SystemExit(
            f"protected {domain} changed in " + ", ".join(changed)
            + f" (guard: {expected_path})"
        )


def verify_unrelated_geometry_guard(
    current: dict[str, object], expected_path: Path, allowed_prefixes: tuple[str, ...]
) -> None:
    """Permit geometry changes only below explicitly named node prefixes."""
    if not allowed_prefixes:
        raise SystemExit("unrelated-geometry guard needs at least one allowed node prefix")
    try:
        expected = json.loads(expected_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise SystemExit(f"cannot read guard manifest {expected_path}: {error}") from error
    if expected.get("model") != current.get("model"):
        raise SystemExit(
            f"guard manifest is for {expected.get('model')!r}, not {current.get('model')!r}"
        )
    expected_gltf = expected.get("gltf_fingerprints")
    current_gltf = current.get("gltf_fingerprints")
    if not isinstance(expected_gltf, dict) or not isinstance(current_gltf, dict):
        raise SystemExit(f"guard manifest lacks glTF fingerprints: {expected_path}")
    if set(expected_gltf) != set(current_gltf):
        raise SystemExit("guarded glTF dependency set changed")

    def allowed(name: str) -> bool:
        return any(name.startswith(prefix) for prefix in allowed_prefixes)

    changes: list[str] = []
    for asset_name, current_values in current_gltf.items():
        expected_values = expected_gltf.get(asset_name)
        current_nodes = (
            current_values.get("node_geometry_sha256")
            if isinstance(current_values, dict) else None
        )
        expected_nodes = (
            expected_values.get("node_geometry_sha256")
            if isinstance(expected_values, dict) else None
        )
        if not isinstance(current_nodes, dict) or not isinstance(expected_nodes, dict):
            raise SystemExit(
                f"guard manifest lacks per-node geometry fingerprints: {expected_path}"
            )
        current_protected = {name: value for name, value in current_nodes.items()
                             if not allowed(name)}
        expected_protected = {name: value for name, value in expected_nodes.items()
                              if not allowed(name)}
        for node_name in sorted(set(current_protected) | set(expected_protected)):
            if current_protected.get(node_name) != expected_protected.get(node_name):
                changes.append(f"{asset_name}:{node_name}")
    if changes:
        preview = ", ".join(changes[:12])
        suffix = " …" if len(changes) > 12 else ""
        raise SystemExit(
            f"protected unrelated node geometry changed in {preview}{suffix} "
            f"(allowed prefixes: {', '.join(allowed_prefixes)}; guard: {expected_path})"
        )


def verify_component_guard(
    current: dict[str, object],
    expected_path: Path,
    allowed_prefixes: tuple[str, ...],
    allowed_domains: tuple[str, ...],
) -> None:
    """Fail before rendering if an edit escapes its component or intent.

    Domains are ``geometry``, ``shading`` and ``binding``.  A geometry pass can
    therefore move only the named mesh nodes while every PBR response and
    animation binding remains byte-for-byte equivalent to the before capture.
    Global model configuration is always protected.
    """
    valid_domains = {"geometry", "shading", "binding"}
    unknown = set(allowed_domains) - valid_domains
    if not allowed_prefixes:
        raise SystemExit("component guard needs at least one allowed node prefix")
    if unknown:
        raise SystemExit("unknown component-guard domain(s): " + ", ".join(sorted(unknown)))
    try:
        expected = json.loads(expected_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise SystemExit(f"cannot read guard manifest {expected_path}: {error}") from error
    if expected.get("model") != current.get("model"):
        raise SystemExit(
            f"guard manifest is for {expected.get('model')!r}, not {current.get('model')!r}"
        )

    expected_gltf = expected.get("gltf_fingerprints")
    current_gltf = current.get("gltf_fingerprints")
    if not isinstance(expected_gltf, dict) or not isinstance(current_gltf, dict):
        raise SystemExit(f"guard manifest lacks glTF fingerprints: {expected_path}")
    if set(expected_gltf) != set(current_gltf):
        raise SystemExit("guarded glTF dependency set changed")

    def allowed(name: str, domain: str) -> bool:
        return domain in allowed_domains and any(
            name.startswith(prefix) for prefix in allowed_prefixes
        )

    changes: list[str] = []
    for asset_name, current_values in current_gltf.items():
        expected_values = expected_gltf.get(asset_name)
        if not isinstance(current_values, dict) or not isinstance(expected_values, dict):
            raise SystemExit(f"malformed glTF fingerprints in {expected_path}")
        for domain, key in (
            ("geometry", "node_geometry_sha256"),
            ("shading", "node_shading_sha256"),
        ):
            current_nodes = current_values.get(key)
            expected_nodes = expected_values.get(key)
            if not isinstance(current_nodes, dict) or not isinstance(expected_nodes, dict):
                raise SystemExit(
                    f"guard manifest lacks per-node {domain} fingerprints: {expected_path}"
                )
            for node_name in sorted(set(current_nodes) | set(expected_nodes)):
                if allowed(node_name, domain):
                    continue
                if current_nodes.get(node_name) != expected_nodes.get(node_name):
                    changes.append(f"{domain}:{asset_name}:{node_name}")

    current_model = current.get("model_fingerprints")
    expected_model = expected.get("model_fingerprints")
    if not isinstance(current_model, dict) or not isinstance(expected_model, dict):
        raise SystemExit(f"guard manifest lacks model fingerprints: {expected_path}")
    if current_model.get("static_sha256") != expected_model.get("static_sha256"):
        changes.append("binding:model-static-configuration")
    current_bindings = current_model.get("node_binding_sha256")
    expected_bindings = expected_model.get("node_binding_sha256")
    if not isinstance(current_bindings, dict) or not isinstance(expected_bindings, dict):
        raise SystemExit(f"guard manifest lacks per-node model bindings: {expected_path}")
    for node_name in sorted(set(current_bindings) | set(expected_bindings)):
        if allowed(node_name, "binding"):
            continue
        if current_bindings.get(node_name) != expected_bindings.get(node_name):
            changes.append(f"binding:{node_name}")

    if changes:
        preview = ", ".join(changes[:16])
        suffix = " …" if len(changes) > 16 else ""
        raise SystemExit(
            f"component guard blocked collateral change(s): {preview}{suffix} "
            f"(allowed nodes: {', '.join(allowed_prefixes)}; "
            f"allowed domains: {', '.join(allowed_domains) or 'none'}; "
            f"guard: {expected_path})"
        )


def renderer_sources() -> list[Path]:
    """Files whose changes can make a reused preview executable misleading."""
    roots = (
        ROOT / "crates/signal-editor",
        ROOT / "crates/sim-core",
        ROOT / "crates/i18n",
        ROOT / "crates/app-icon",
    )
    sources = [ROOT / "Cargo.toml", ROOT / "Cargo.lock"]
    for root in roots:
        sources.extend(root.rglob("*.rs"))
        sources.extend(root.rglob("Cargo.toml"))
    return [path for path in sources if path.is_file()]


def run(command: list[str], *, quiet: bool = False) -> None:
    result = subprocess.run(
        command,
        cwd=ROOT,
        text=True,
        stdout=subprocess.DEVNULL if quiet else None,
        stderr=subprocess.PIPE if quiet else None,
    )
    if result.returncode:
        if quiet and result.stderr:
            print(result.stderr, file=sys.stderr)
        raise SystemExit(result.returncode)


def image_has_content(path: Path) -> bool:
    """A cold GPU occasionally returns only the clear colour; reject that."""
    if not path.is_file():
        return False
    result = subprocess.run(
        ["magick", str(path), "-colorspace", "Gray", "-format", "%[fx:standard_deviation]", "info:"],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    try:
        return result.returncode == 0 and float(result.stdout) > 0.002
    except ValueError:
        return False


def image_dimensions(path: Path) -> tuple[int, int] | None:
    result = subprocess.run(
        ["magick", "identify", "-format", "%wx%h", str(path)],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    match = re.fullmatch(r"([1-9][0-9]*)x([1-9][0-9]*)", result.stdout.strip())
    return (int(match.group(1)), int(match.group(2))) if result.returncode == 0 and match else None


def labelled_tile(source: Path, label: str, target: Path) -> None:
    run(
        [
            "magick",
            str(source),
            "-resize",
            "560x680",
            "-background",
            "#202226",
            "-fill",
            "#f2f2f2",
            "-gravity",
            "south",
            "-splice",
            "0x42",
            "-pointsize",
            "22",
            "-annotate",
            "+0+10",
            label,
            str(target),
        ],
        quiet=True,
    )


def contact_sheet(images: list[tuple[Path, str]], target: Path, columns: int = 3) -> None:
    # A caller commonly launches independent front/rear/model reviews in
    # parallel.  A fixed ``.tiles`` directory in their shared output parent
    # let those jobs overwrite and delete one another's intermediate PNGs,
    # occasionally yielding CRC-corrupt contact sheets.  Give every sheet an
    # isolated directory and let TemporaryDirectory clean it on errors too.
    with tempfile.TemporaryDirectory(
            prefix=f".{target.stem}-tiles-", dir=target.parent) as temporary:
        tiles = Path(temporary)
        labelled = []
        for index, (source, label) in enumerate(images):
            tile = tiles / f"{index:03}.png"
            labelled_tile(source, label, tile)
            labelled.append(tile)
        run(
            [
                "magick",
                "montage",
                *map(str, labelled),
                "-tile",
                f"{columns}x",
                "-geometry",
                "+10+10",
                "-background",
                "#15171a",
                str(target),
            ],
            quiet=True,
        )


def compare(current: Path, baseline: Path, difference: Path) -> float:
    result = subprocess.run(
        ["magick", "compare", "-metric", "RMSE", str(baseline), str(current), str(difference)],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    match = re.search(r"\(([-+0-9.eE]+)\)", result.stderr)
    if not match:
        raise SystemExit(f"could not read ImageMagick RMSE: {result.stderr.strip()}")
    return float(match.group(1))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("model", help="model stem or signal_models/*.ron")
    parser.add_argument("--aspect", default="auto")
    parser.add_argument("--view", choices=ALL_VIEWS, default="front")
    parser.add_argument(
        "--focus", choices=("full", "head", "detail", "base"), default="full"
    )
    parser.add_argument(
        "--target-node",
        help="frame a visible glTF node-name prefix while keeping the assembly rendered",
    )
    parser.add_argument(
        "--isolate-target",
        action="store_true",
        help="render only --target-node and its descendants (bounds still use the assembly)",
    )
    parser.add_argument("--lod", type=int, choices=(0, 1, 2), default=0)
    parser.add_argument(
        "--background",
        choices=("neutral", "light", "dark"),
        default="neutral",
        help="repeatable studio backdrop; light is useful for black rear mechanisms",
    )
    parser.add_argument("--window", help="WIDTHxHEIGHT; inferred from focus by default")
    parser.add_argument("--frames", type=int, default=35, help="GPU settle frames after framing")
    parser.add_argument(
        "--output",
        type=Path,
        help="PNG or directory (a suffixless path is always treated as a directory)",
    )
    parser.add_argument("--matrix", action="store_true", help="all valid aspects from six directions")
    parser.add_argument(
        "--animation",
        type=animation_transition,
        metavar="FROM:TO",
        help="render a deterministic film strip of one real aspect transition",
    )
    parser.add_argument(
        "--animation-times",
        type=animation_times,
        default=DEFAULT_ANIMATION_TIMES,
        metavar="T0,T1,...",
        help="sample times in seconds (default chosen to expose semaphore impact and bounce)",
    )
    parser.add_argument("--reference", type=Path, action="append", default=[])
    parser.add_argument("--generate", action="store_true", help="regenerate form-signal assets first")
    parser.add_argument("--no-build", action="store_true", help="reuse target/debug binary")
    parser.add_argument("--accept-baseline", action="store_true")
    parser.add_argument("--compare-baseline", action="store_true")
    parser.add_argument(
        "--protect-geometry",
        type=Path,
        metavar="MANIFEST",
        help="fail if mesh/node/buffer geometry differs from an earlier preview manifest",
    )
    parser.add_argument(
        "--protect-shading",
        type=Path,
        metavar="MANIFEST",
        help="fail if materials or referenced texture bytes differ from an earlier manifest",
    )
    parser.add_argument(
        "--protect-unrelated-geometry",
        type=Path,
        metavar="MANIFEST",
        help="allow named component edits but fail if any other glTF node geometry changes",
    )
    parser.add_argument(
        "--allow-geometry-node",
        action="append",
        default=[],
        metavar="PREFIX",
        help="node-name prefix allowed by --protect-unrelated-geometry (repeatable)",
    )
    parser.add_argument(
        "--protect-component",
        type=Path,
        metavar="MANIFEST",
        help=(
            "fail when geometry, PBR shading or motion bindings escape the "
            "explicit node/domain edit scope"
        ),
    )
    parser.add_argument(
        "--allow-node",
        action="append",
        default=[],
        metavar="PREFIX",
        help="node-name prefix owned by --protect-component (repeatable)",
    )
    parser.add_argument(
        "--allow-domain",
        action="append",
        default=[],
        choices=("geometry", "shading", "binding"),
        help=(
            "kind of change allowed on --allow-node; no value means all three "
            "domains (repeatable)"
        ),
    )
    parser.add_argument(
        "--approval-note",
        help="required human approval reference when accepting visual baselines",
    )
    parser.add_argument("--max-rmse", type=float, default=0.002)
    args = parser.parse_args()

    if args.accept_baseline and args.compare_baseline:
        parser.error("choose either --accept-baseline or --compare-baseline")
    if args.accept_baseline and not (args.approval_note or "").strip():
        parser.error("--accept-baseline requires --approval-note with the user's approval")
    if args.approval_note and not args.accept_baseline:
        parser.error("--approval-note is only valid with --accept-baseline")
    if args.max_rmse < 0.0 or not math.isfinite(args.max_rmse):
        parser.error("--max-rmse must be finite and non-negative")
    if args.animation and args.matrix:
        parser.error("--animation and --matrix are separate review modes")
    if args.animation and args.aspect != "auto":
        parser.error("the target aspect comes from --animation; do not also pass --aspect")
    if args.isolate_target and not args.target_node:
        parser.error("--isolate-target requires --target-node")
    allowed_geometry_nodes = tuple(args.allow_geometry_node)
    if args.protect_unrelated_geometry and not allowed_geometry_nodes and args.target_node:
        allowed_geometry_nodes = (args.target_node,)
    if args.protect_unrelated_geometry and not allowed_geometry_nodes:
        parser.error(
            "--protect-unrelated-geometry needs --allow-geometry-node or --target-node"
        )
    if allowed_geometry_nodes and not args.protect_unrelated_geometry:
        parser.error("--allow-geometry-node requires --protect-unrelated-geometry")
    allowed_component_nodes = tuple(args.allow_node)
    if args.protect_component and not allowed_component_nodes and args.target_node:
        allowed_component_nodes = (args.target_node,)
    if args.protect_component and not allowed_component_nodes:
        parser.error("--protect-component needs --allow-node or --target-node")
    if allowed_component_nodes and not args.protect_component:
        parser.error("--allow-node requires --protect-component")
    if args.allow_domain and not args.protect_component:
        parser.error("--allow-domain requires --protect-component")
    allowed_component_domains = tuple(dict.fromkeys(
        args.allow_domain or ("geometry", "shading", "binding")
    ))
    if shutil.which("magick") is None:
        raise SystemExit("ImageMagick 'magick' is required for render validation and contact sheets")
    missing_references = [path for path in args.reference if not path.is_file()]
    if missing_references:
        raise SystemExit(
            "missing reference image(s): " + ", ".join(map(str, missing_references))
        )
    missing_guards = [
        path for path in (
            args.protect_geometry,
            args.protect_shading,
            args.protect_unrelated_geometry,
            args.protect_component,
        )
        if path is not None and not path.is_file()
    ]
    if missing_guards:
        raise SystemExit("missing guard manifest(s): " + ", ".join(map(str, missing_guards)))
    if args.generate:
        run([sys.executable, "tools/gen_form_signals.py"])
    if not args.no_build:
        run(["cargo", "build", "-p", "signal-editor"])

    binary = ROOT / "target/debug/trainsim-signal-editor"
    if not binary.is_file():
        raise SystemExit(f"missing {binary}; omit --no-build once")
    if args.no_build:
        newer = [path for path in renderer_sources() if path.stat().st_mtime > binary.stat().st_mtime]
        if newer:
            examples = ", ".join(str(path.relative_to(ROOT)) for path in newer[:3])
            raise SystemExit(
                f"refusing stale --no-build preview; {examples} is newer than the renderer. "
                "Omit --no-build once."
            )
    model = model_path(args.model)
    stem = model.stem
    size = args.window or (
        "1600x1200" if args.focus == "detail"
        else "1200x900" if args.focus == "head"
        else "900x1200"
    )
    size_match = re.fullmatch(r"([1-9][0-9]*)x([1-9][0-9]*)", size)
    if not size_match:
        raise SystemExit("--window must be WIDTHxHEIGHT with positive integer pixels")
    expected_size = (int(size_match.group(1)), int(size_match.group(2)))
    valid_aspects = aspects_for(stem)
    if args.animation:
        source_aspect, target_aspect = args.animation
        invalid = [aspect for aspect in args.animation if aspect not in valid_aspects]
        if invalid:
            raise SystemExit(
                f"{', '.join(invalid)} is not valid for {stem}; expected {', '.join(valid_aspects)}"
            )
        selected_aspects = (target_aspect,)
        selected_views = (args.view,)
    else:
        selected_aspects = valid_aspects if args.matrix or args.aspect == "auto" else (args.aspect,)
        if not args.matrix and args.aspect == "auto":
            selected_aspects = valid_aspects[:1]
        selected_views = MATRIX_VIEWS if args.matrix else (args.view,)

    collection = args.matrix or bool(args.animation)
    target_tag = ""
    if args.target_node:
        target_tag = "-node" + re.sub(r"[^A-Za-z0-9_.-]+", "_", args.target_node)
    if args.isolate_target:
        target_tag += "-isolated"
    output: Path | None = None
    if collection:
        default_directory = DEFAULT_OUTPUT / stem
        if args.animation:
            default_directory /= f"motion-{source_aspect}-to-{target_aspect}"
        directory = (args.output or default_directory).resolve()
    else:
        default = DEFAULT_OUTPUT / stem / (
            f"{selected_aspects[0]}-{args.view}-{args.focus}{target_tag}-"
            f"lod{args.lod}-bg{args.background}.png"
        )
        output = single_output_path(args.output, default)
        directory = output.parent
    directory.mkdir(parents=True, exist_ok=True)

    render_specs: list[tuple[str, str, Path, str, list[str]]] = []
    if args.animation:
        for index, seconds in enumerate(args.animation_times):
            target = directory / (
                f"{source_aspect}-to-{target_aspect}-{index:02d}-t{seconds:05.2f}s-"
                f"{args.view}-{args.focus}{target_tag}-lod{args.lod}-"
                f"bg{args.background}.png"
            )
            label = (
                f"{source_aspect} → {target_aspect} · t={seconds:.2f}s · "
                f"{args.background}"
            )
            render_specs.append(
                (
                    target_aspect,
                    args.view,
                    target,
                    label,
                    ["--from-aspect", source_aspect, "--motion-time", f"{seconds:.6f}"],
                )
            )
    else:
        for aspect in selected_aspects:
            if aspect not in valid_aspects and aspect != "none":
                raise SystemExit(f"{aspect} is not valid for {stem}; expected {', '.join(valid_aspects)}")
            for view in selected_views:
                target = directory / (
                    f"{aspect}-{view}-{args.focus}{target_tag}-lod{args.lod}-"
                    f"bg{args.background}.png"
                )
                if not args.matrix:
                    assert output is not None
                    target = output
                render_specs.append(
                    (
                        aspect,
                        view,
                        target,
                        f"{aspect} · {view} · {args.focus} · LOD{args.lod} · {args.background}",
                        [],
                    )
                )

    rendered: list[tuple[Path, str]] = []
    capture_bounds: dict[str, dict[str, object]] = {}
    for aspect, view, target, label, extra in render_specs:
        bounds_target = target.with_suffix(".bounds.json")
        command = [
            str(binary),
            str(model),
            "--render-only",
            "--aspect",
            aspect,
            "--view",
            view,
            "--focus",
            args.focus,
            "--lod",
            str(args.lod),
            "--background",
            args.background,
            "--window",
            size,
            "--frames",
            str(args.frames),
            "--screenshot",
            str(target),
            "--bounds-json",
            str(bounds_target),
            *(["--target-node", args.target_node] if args.target_node else []),
            *(["--isolate-target"] if args.isolate_target else []),
            *extra,
        ]
        run(command, quiet=True)
        if not image_has_content(target):
            # Asset dependencies can be loaded while their textures are
            # still reaching the GPU. One longer retry is faster and much
            # safer than silently putting a blank tile in a review sheet.
            print(f"blank render detected, retrying {target.name}", file=sys.stderr)
            command[command.index("--frames") + 1] = str(max(args.frames * 2, 60))
            run(command, quiet=True)
            if not image_has_content(target):
                raise SystemExit(f"renderer produced a blank image twice: {target}")
        actual_size = image_dimensions(target)
        if actual_size != expected_size:
            raise SystemExit(
                f"renderer produced {actual_size} instead of requested {expected_size}: {target}"
            )
        try:
            bounds_data = json.loads(bounds_target.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            raise SystemExit(f"invalid or missing bounds sidecar {bounds_target}: {error}") from error
        if bounds_data.get("unit") != "metre" or any(
            len(bounds_data.get(key, ())) != 3
            for key in ("assembly_min", "assembly_max", "assembly_size")
        ):
            raise SystemExit(f"malformed bounds sidecar: {bounds_target}")
        motions = bounds_data.get("motions")
        if not isinstance(motions, list) or any(
            not isinstance(motion, dict)
            or not {"node", "lamp", "travel", "velocity", "effective_amount"}
            <= motion.keys()
            for motion in motions
        ):
            raise SystemExit(f"missing or malformed motion state in {bounds_target}")
        capture_bounds[str(target.relative_to(directory))] = {
            "bounds_file": str(bounds_target.relative_to(directory)),
            **bounds_data,
        }
        rendered.append((target, label))
        print(target)

    manifest = {
        "renderer": {
            "path": str(binary.relative_to(ROOT)),
            "sha256": sha256(binary),
        },
        **input_fingerprint_manifest(model),
        "aspect": list(selected_aspects),
        "views": list(selected_views),
        "focus": args.focus,
        "target_node": args.target_node,
        "isolate_target": args.isolate_target,
        "lod": args.lod,
        "background": args.background,
        "window": size,
        "references": [
            {"path": str(path.resolve()), "sha256": sha256(path.resolve())}
            for path in args.reference
        ],
        "captures": capture_bounds,
    }
    if args.animation:
        manifest["transition"] = [source_aspect, target_aspect]
        manifest["motion_times"] = list(args.animation_times)
    if args.protect_geometry:
        verify_fingerprint_guard(manifest, args.protect_geometry.resolve(), "geometry_sha256")
        manifest["geometry_guard"] = str(args.protect_geometry.resolve())
    if args.protect_shading:
        verify_fingerprint_guard(manifest, args.protect_shading.resolve(), "shading_sha256")
        manifest["shading_guard"] = str(args.protect_shading.resolve())
    if args.protect_unrelated_geometry:
        verify_unrelated_geometry_guard(
            manifest,
            args.protect_unrelated_geometry.resolve(),
            allowed_geometry_nodes,
        )
        manifest["unrelated_geometry_guard"] = str(
            args.protect_unrelated_geometry.resolve()
        )
        manifest["allowed_geometry_node_prefixes"] = list(allowed_geometry_nodes)
    if args.protect_component:
        verify_component_guard(
            manifest,
            args.protect_component.resolve(),
            allowed_component_nodes,
            allowed_component_domains,
        )
        manifest["component_guard"] = str(args.protect_component.resolve())
        manifest["allowed_component_node_prefixes"] = list(allowed_component_nodes)
        manifest["allowed_component_domains"] = list(allowed_component_domains)
    assert output is not None or collection
    manifest_target = directory / "manifest.json" if collection else output.with_suffix(".json")
    manifest_target.write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )

    if collection or args.reference:
        sheet_inputs = [(path.resolve(), f"reference · {path.name}") for path in args.reference]
        sheet_inputs.extend(rendered)
        sheet = (
            directory / ("animation-strip.png" if args.animation else "contact-sheet.png")
            if collection
            else output.with_name(f"{output.stem}-contact-sheet.png")
        )
        contact_sheet(sheet_inputs, sheet, columns=5 if args.animation else 3)
        print(sheet)

    baseline_dir = ROOT / "screenshots/signals/baseline" / stem
    if args.accept_baseline:
        baseline_dir.mkdir(parents=True, exist_ok=True)
        accepted_at = datetime.now(timezone.utc).isoformat()
        for current, _label in rendered:
            baseline = baseline_dir / current.name
            shutil.copy2(current, baseline)
            metadata = {
                "accepted_at": accepted_at,
                "approval_note": args.approval_note.strip(),
                "baseline_image_sha256": sha256(baseline),
                "source_manifest": manifest,
            }
            baseline.with_suffix(".baseline.json").write_text(
                json.dumps(metadata, indent=2, sort_keys=True) + "\n", encoding="utf-8"
            )
        print(f"accepted {len(rendered)} baseline images in {baseline_dir}")
    if args.compare_baseline:
        failed = []
        diff_dir = directory / "diff"
        diff_dir.mkdir(parents=True, exist_ok=True)
        for current, _label in rendered:
            baseline = baseline_dir / current.name
            if not baseline.is_file():
                failed.append((current.name, float("inf")))
                continue
            metadata_path = baseline.with_suffix(".baseline.json")
            try:
                metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
            except (OSError, json.JSONDecodeError):
                failed.append((current.name, float("inf")))
                continue
            if metadata.get("baseline_image_sha256") != sha256(baseline):
                failed.append((current.name, float("inf")))
                continue
            score = compare(current, baseline, diff_dir / current.name)
            print(f"RMSE {score:.6f}  {current.name}")
            if score > args.max_rmse:
                failed.append((current.name, score))
        if failed:
            names = ", ".join(f"{name} ({score:.6f})" for name, score in failed)
            raise SystemExit(f"visual regression threshold exceeded: {names}")


if __name__ == "__main__":
    main()
