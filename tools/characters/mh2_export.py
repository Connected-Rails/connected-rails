#!/usr/bin/env python3
"""Headless MakeHuman 2 driver: turns the roster into raw character exports.

For every character in ``roster.json`` this script writes a MakeHuman model
file (``.mhm``), loads it into MakeHuman 2 without a window, attaches the
``game_engine`` rig, and exports a binary glTF with the animation clips the
game uses.  The result is *raw*: full-resolution PNG textures, one mesh per
garment, no levels of detail.  ``build_character.py`` turns it into the
game-ready file.

MakeHuman 2 is a Qt/OpenGL desktop program with no batch mode.  The trick
used here is to import its core modules directly, run Qt on the ``offscreen``
platform plug-in, and replace the two objects the core code talks to (the
OpenGL window and the middle column of the main window) by stubs that swallow
every call.  Nothing is rendered, only the numpy geometry is touched.

The asset packs in the user's MakeHuman folder are laid out the MakeHuman 1
way (``clothes/<asset>``); MakeHuman 2 expects ``clothes/hm08/<asset>``.  The
script therefore builds its own MakeHuman home directory (``--home``) with
symbolic links in the expected layout and points MakeHuman at it through
``MH_HOME_LOCATION``, so the user's own installation stays untouched.

Usage::

    ~/.local/share/makehuman2/venv/bin/python tools/characters/mh2_export.py \
        --roster tools/characters/roster.json --out build/characters/raw

The MakeHuman 2 virtual environment has to be used: PySide6 and numpy come
from there.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from dataclasses import dataclass, field
from pathlib import Path

# The animation clips every character ships, in the order they appear in the
# glTF: clip name → MakeHuman pose file (``poses/<file>.bvh``).  ``walk`` is
# gender-specific and filled in per character.
CLIPS = [
    ("idle", "idle1"),
    ("idle2", "idle2"),
    ("stand", "standing01"),
    ("stand2", "standing02"),
    ("stand3", "standing03"),
    ("sit", "sit01"),
]
WALK_CLIP = {"male": "walk_normal", "female": "walk_female"}

# Asset categories in the MakeHuman 1 pack layout → MakeHuman 2 category name.
# MakeHuman 2 wants ``<category>/<base mesh>/<asset>``; the pack is flat.
LINKED_CATEGORIES = {
    "clothes": "clothes",
    "hair": "hair",
    "eyebrows": "eyebrows",
    "eyelashes": "eyelashes",
    "skins": "skins",
    "proxymeshes": "proxy",
    "teeth": "teeth",
    "tongue": "tongue",
}
BASE_MESH = "hm08"
RIG = "game_engine.mhskel"
EYES_ASSET = "low-poly"


@dataclass
class Character:
    """One roster entry, as read from ``roster.json``."""

    id: str
    name: str
    gender: str
    roles: list[str]
    tags: list[str]
    modifiers: dict[str, float]
    skin: str
    eyes: str
    eyebrows: str | None
    eyelashes: str | None
    hair: str | None
    clothes: list[str]
    tints: dict[str, dict[str, float]] = field(default_factory=dict)

    @staticmethod
    def from_json(data: dict) -> "Character":
        return Character(
            id=data["id"],
            name=data["name"],
            gender=data["gender"],
            roles=list(data.get("roles", ["passenger"])),
            tags=list(data.get("tags", [])),
            modifiers=dict(data.get("modifiers", {})),
            skin=data["skin"],
            eyes=data.get("eyes", "brown"),
            eyebrows=data.get("eyebrows"),
            eyelashes=data.get("eyelashes"),
            hair=data.get("hair"),
            clothes=list(data.get("clothes", [])),
            tints=dict(data.get("tints", {})),
        )


def prepare_home(home: Path, assets: Path) -> None:
    """Builds the MakeHuman 2 home with the asset packs linked in hm08 layout.

    Idempotent: existing links are refreshed, nothing in ``assets`` is written.
    """
    data = home / "data"
    for src_name, category in LINKED_CATEGORIES.items():
        src = assets / src_name
        if not src.is_dir():
            continue
        dst = data / category / BASE_MESH
        dst.mkdir(parents=True, exist_ok=True)
        for entry in sorted(src.iterdir()):
            # The pack carries empty ``hm08``/``mh2bot`` folders of its own.
            if not entry.is_dir() or entry.name in (BASE_MESH, "mh2bot"):
                continue
            link = dst / entry.name
            if link.is_symlink() or link.exists():
                link.unlink()
            link.symlink_to(entry)
    for category, patterns in (("rigs", ("*.mhskel", "*.thumb")), ("poses", ("*.bvh", "*.meta", "*.thumb"))):
        src = assets / category
        dst = data / category / BASE_MESH
        dst.mkdir(parents=True, exist_ok=True)
        for pattern in patterns:
            for entry in sorted(src.glob(pattern)):
                link = dst / entry.name
                if link.is_symlink() or link.exists():
                    link.unlink()
                link.symlink_to(entry)
    (data / "models" / BASE_MESH).mkdir(parents=True, exist_ok=True)
    (data / "exports").mkdir(parents=True, exist_ok=True)


def asset_identity(mhclo: Path) -> tuple[str, str]:
    """Name and uuid out of a ``.mhclo``/``.proxy`` file — what an ``.mhm`` line
    refers to."""
    name = uuid = None
    with open(mhclo, encoding="utf-8", errors="ignore") as f:
        for line in f:
            words = line.split()
            if not words:
                continue
            if words[0] == "uuid":
                uuid = words[1]
            elif words[0] == "name":
                name = " ".join(words[1:])
    if name is None or uuid is None:
        raise ValueError(f"{mhclo}: no name/uuid")
    return name, uuid


def write_mhm(character: Character, assets: Path, path: Path) -> dict[str, str]:
    """Writes the MakeHuman model file and returns node name → asset type.

    MakeHuman names an exported mesh node after the asset's file stem, which is
    what the build step needs to know which mesh is hair and which is skin.
    """
    lines = ["# MakeHuman2 Model File — generated by tools/characters/mh2_export.py",
             "version v2.0.1", f"name {character.id}", "author Connected Rails"]
    lines.append("tags " + ";".join(character.tags))
    for key, value in character.modifiers.items():
        lines.append(f"modifier {key} {value:.6f}")
    lines.append(f"skinMaterial skins/{character.skin}/{character.skin}.mhmat")
    nodes: dict[str, str] = {}

    def attach(kind: str, folder: str, asset: str, extension: str = ".mhclo") -> None:
        file = assets / folder / asset / f"{asset}{extension}"
        name, uuid = asset_identity(file)
        lines.append(f"{kind} {name} {uuid}")
        nodes[asset] = kind

    eyes_name, eyes_uuid = asset_identity(assets / "eyes" / EYES_ASSET / f"{EYES_ASSET}.mhclo")
    lines.append(f"eyes {eyes_name} {eyes_uuid}")
    lines.append(f"material {eyes_name} {eyes_uuid} eyes/materials/{character.eyes}.mhmat")
    nodes[EYES_ASSET] = "eyes"
    if character.eyebrows:
        attach("eyebrows", "eyebrows", character.eyebrows)
    if character.eyelashes:
        attach("eyelashes", "eyelashes", character.eyelashes)
    if character.hair:
        attach("hair", "hair", character.hair)
    for garment in character.clothes:
        attach("clothes", "clothes", garment)
        if garment.startswith("fedora"):
            nodes[garment] = "hat"
    lines.append(f"skeleton {RIG}")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return nodes


class Headless:
    """MakeHuman 2 booted without a window."""

    def __init__(self, mh2: Path, home: Path, verbose: int = 1):
        os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
        os.environ["MH_HOME_LOCATION"] = str(home)
        os.chdir(mh2)
        sys.path.insert(0, str(mh2))
        from PySide6.QtWidgets import QApplication

        self.app = QApplication.instance() or QApplication([sys.argv[0]])
        from core.baseobj import baseClass
        from core.export_gltf import gltfExport
        from core.globenv import globalObjects, programInfo

        class Args:
            model = None
            version = False
            noskybox = True
            nomultisampling = True
            l = False
            base = BASE_MESH
            repository = True  # rebuild the asset cache — the links may be new
            admin = False

        Args.verbose = verbose
        self.env = programInfo(False, str(mh2), Args())
        if not self.env.environment():
            raise RuntimeError(self.env.last_error)
        self.glob = globalObjects(self.env)
        if not self.glob.readShaderInitJSON():
            raise RuntimeError(self.env.last_error)

        class Stub:
            """Swallows every OpenGL/GUI call the core code makes."""

            def __getattr__(self, _name):
                return Stub()

            def __call__(self, *_args, **_kwargs):
                return None

        self.glob.openGLWindow = Stub()
        self.glob.midColumn = Stub()
        self.env.basename = BASE_MESH
        self.base = baseClass(self.glob, BASE_MESH)
        if not self.base.prepareClass():
            raise RuntimeError(self.env.last_error)
        self._exporter = gltfExport

    def export(self, mhm: Path, clips: list[tuple[str, Path]], out: Path) -> dict:
        """Loads ``mhm``, exports ``out`` with ``clips`` and returns facts about it."""
        base = self.base
        ok, message = base.loadMHMFile(str(mhm))
        if not ok:
            raise RuntimeError(f"{mhm}: {message}")
        if base.skeleton is None:
            raise RuntimeError(f"{mhm}: rig {RIG} not found")
        for asset in base.attachedAssets:
            asset.obj.precalculateDimension()

        # What the GUI does when it enters pose mode, minus the drawing: keep a
        # copy of the rest-pose coordinates so a loaded pose can be undone.
        base.baseMesh.createWCopy()
        base.restPose()
        base.precalculateAssetsInRestPose()
        base.pose_skeleton.newGeometry()
        base.in_posemode = True

        exporter = self._exporter
        clip_frames: dict[str, int] = {}

        class MultiClipExport(exporter):
            """The stock exporter writes the one loaded pose; this one loads each
            clip in turn and keeps them all."""

            def addAnimations(self, skeleton, bvh, orig=True):
                animations = []
                for name, path in clips:
                    if not base.addPose(name, str(path)):
                        raise RuntimeError(f"pose {path}: {base.env.last_error}")
                    base.bvh.modCorrections()
                    exporter.addAnimations(self, skeleton, base.bvh, orig)
                    self.json["animations"][0]["name"] = name
                    clip_frames[name] = base.bvh.frameCount
                    animations.extend(self.json["animations"])
                    base.bvh.identFinal()
                self.json["animations"] = animations

        # ``addNodes`` needs a loaded pose to decide that animation is wanted;
        # the mesh itself has to be exported in the rest pose.
        if not base.addPose(clips[0][0], str(clips[0][1])):
            raise RuntimeError(f"pose {clips[0][1]}: {base.env.last_error}")
        base.baseMesh.resetFromCopy()
        base.restPose()
        base.updateAttachedAssets()

        gltf = MultiClipExport(
            self.glob, str(out.parent), "textures",
            includetextures=True, hiddenverts=False, onground=True,
            animation=True, saveprops=False, scale=0.1,
        )
        if not gltf.binSave(base, str(out)):
            raise RuntimeError(f"{out}: {self.env.last_error}")
        heights = base.baseMesh.gl_coord[1::3]
        return {
            "attached": [(a.type, a.name, os.path.basename(a.filename)) for a in base.attachedAssets],
            "height_m": float(heights.max() - heights.min()) * 0.1,
            "clips": clip_frames,
        }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--roster", type=Path, default=Path(__file__).with_name("roster.json"))
    parser.add_argument("--out", type=Path, required=True, help="folder for the raw .glb/.mhm/.meta.json files")
    parser.add_argument("--only", type=str, default="", help="comma-separated character ids to build")
    parser.add_argument("--mh2", type=Path, default=Path.home() / ".local/share/makehuman2")
    parser.add_argument("--assets", type=Path, default=Path.home() / "Documents/makehuman2/data",
                        help="MakeHuman asset packs (MakeHuman 1 layout)")
    parser.add_argument("--home", type=Path, default=Path.home() / ".cache/connected-rails/mh2home",
                        help="MakeHuman 2 home built for this pipeline")
    parser.add_argument("-v", "--verbose", type=int, default=1)
    args = parser.parse_args()

    roster = json.loads(args.roster.read_text(encoding="utf-8"))
    characters = [Character.from_json(c) for c in roster["characters"]]
    if args.only:
        wanted = set(args.only.split(","))
        characters = [c for c in characters if c.id in wanted]
        missing = wanted - {c.id for c in characters}
        if missing:
            print(f"unknown character ids: {sorted(missing)}", file=sys.stderr)
            return 2

    args.out.mkdir(parents=True, exist_ok=True)
    prepare_home(args.home, args.assets)
    mh = Headless(args.mh2, args.home, args.verbose)
    poses = args.assets / "poses"

    failures = 0
    for character in characters:
        mhm = args.out / f"{character.id}.mhm"
        nodes = write_mhm(character, args.assets, mhm)
        clips = [(name, poses / f"{file}.bvh") for name, file in CLIPS]
        clips.insert(1, ("walk", poses / f"{WALK_CLIP[character.gender]}.bvh"))
        out = args.out / f"{character.id}.glb"
        try:
            facts = mh.export(mhm, clips, out)
        except Exception as error:  # noqa: BLE001 — one broken character must not stop the batch
            print(f"{character.id}: FAILED: {error}", file=sys.stderr)
            failures += 1
            continue
        expected = 1 + sum(1 for x in (character.eyebrows, character.eyelashes, character.hair) if x) + len(character.clothes)
        if len(facts["attached"]) != expected:
            print(f"{character.id}: FAILED: {len(facts['attached'])} of {expected} assets attached: {facts['attached']}",
                  file=sys.stderr)
            failures += 1
            continue
        meta = {
            "id": character.id,
            "name": character.name,
            "gender": character.gender,
            "roles": character.roles,
            "tags": character.tags,
            "base_node": "base",
            "assets": nodes,
            "tints": character.tints,
            "height_m": facts["height_m"],
            "clips": facts["clips"],
            "source": {"skin": character.skin, "eyes": character.eyes, "eyebrows": character.eyebrows,
                       "eyelashes": character.eyelashes, "hair": character.hair, "clothes": character.clothes,
                       "rig": RIG, "poses": [file for _, file in CLIPS] + [WALK_CLIP[character.gender]]},
        }
        (args.out / f"{character.id}.meta.json").write_text(json.dumps(meta, indent=2) + "\n", encoding="utf-8")
        size = out.stat().st_size // 1024
        print(f"{character.id}: {size} KB raw, {facts['height_m']:.2f} m, {len(facts['attached'])} assets, "
              f"clips {', '.join(f'{k}:{v}' for k, v in facts['clips'].items())}")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
