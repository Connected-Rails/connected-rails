#!/usr/bin/env python3
"""Fetches the motion capture recordings the characters' clips are made of.

`clips.json` names the sources — the 100STYLE dataset (CC BY 4.0) and the
Open Motion Project of ACCAD, The Ohio State University (CC BY 3.0) — with
the files each clip comes out of. This script downloads them into a cache
outside the repository (`~/.cache/connected-rails/mocap/<source>/`), unpacks
the archives, and leaves the rest to `build_character.py`. Nothing of the raw
recordings is checked in; the attribution the licences ask for is in
`THIRD_PARTY_LICENSES.md`.

Usage::

    python3 tools/characters/fetch_mocap.py            # everything clips.json needs
    python3 tools/characters/fetch_mocap.py --check    # only say what is missing

The 100STYLE files come one by one from the links on the dataset's page
(Google Drive); should those go away, the whole dataset is archived on Zenodo
(`archive_url` in `clips.json`) — unpack its BVH files into the cache folder
of the source by hand.
"""

from __future__ import annotations

import argparse
import json
import shutil
import sys
import urllib.request
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
DEFAULT_CACHE = Path.home() / ".cache/connected-rails/mocap"


def mocap_dir(cache: Path, source: str) -> Path:
    return cache / source


def missing_files(clips: dict, cache: Path) -> dict[str, list[str]]:
    """Per source, the files of `clips.json` that are not in the cache."""
    missing = {}
    for name, source in clips["sources"].items():
        folder = mocap_dir(cache, name)
        files = source["files"]
        names = list(files) if isinstance(files, dict) else list(files)
        absent = [f for f in names if not (folder / f).is_file() or (folder / f).stat().st_size == 0]
        if absent:
            missing[name] = absent
    return missing


def download(url: str, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    request = urllib.request.Request(url, headers={"User-Agent": "connected-rails/tools/characters"})
    with urllib.request.urlopen(request, timeout=600) as response, open(path, "wb") as out:
        shutil.copyfileobj(response, out)
    if path.stat().st_size < 1024:
        text = path.read_bytes()[:200]
        path.unlink()
        raise RuntimeError(f"{url}: {len(text)} bytes — not the file (an error page?)")


def fetch_source(name: str, source: dict, wanted: list[str], cache: Path) -> None:
    folder = mocap_dir(cache, name)
    folder.mkdir(parents=True, exist_ok=True)
    if "archive" in source:
        archive = folder / source["archive"]["file"]
        if not archive.is_file():
            print(f"{name}: downloading {source['archive']['url']}")
            download(source["archive"]["url"], archive)
        with zipfile.ZipFile(archive) as zipped:
            members = {Path(m).name: m for m in zipped.namelist()}
            for file in wanted:
                if file not in members:
                    raise RuntimeError(f"{archive.name}: {file} not in the archive")
                with zipped.open(members[file]) as inside, open(folder / file, "wb") as out:
                    shutil.copyfileobj(inside, out)
                print(f"{name}: {file} unpacked")
        return
    files = source["files"]
    for file in wanted:
        url = files[file] if isinstance(files, dict) else None
        if url is None:
            raise RuntimeError(f"{name}: no download link for {file}")
        print(f"{name}: downloading {file}")
        download(url, folder / file)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--clips", type=Path, default=HERE / "clips.json")
    parser.add_argument("--cache", type=Path, default=DEFAULT_CACHE)
    parser.add_argument("--check", action="store_true", help="list what is missing, download nothing")
    args = parser.parse_args()
    clips = json.loads(args.clips.read_text(encoding="utf-8"))
    missing = missing_files(clips, args.cache)
    if not missing:
        print(f"all {sum(len(s['files']) for s in clips['sources'].values())} files in {args.cache}")
        return 0
    if args.check:
        for name, files in missing.items():
            print(f"{name}: {len(files)} missing — {', '.join(files)}")
        return 1
    failures = 0
    for name, files in missing.items():
        source = clips["sources"][name]
        try:
            fetch_source(name, source, files, args.cache)
        except Exception as error:  # noqa: BLE001 — one source must not stop the others
            print(f"{name}: FAILED: {error}", file=sys.stderr)
            failures += 1
    still = missing_files(clips, args.cache)
    for name, files in still.items():
        print(f"{name}: still missing {', '.join(files)}", file=sys.stderr)
    for name, source in clips["sources"].items():
        print(f"{name}: {source['name']} — {source['license']} ({source['url']})")
    return 1 if failures or still else 0


if __name__ == "__main__":
    sys.exit(main())
