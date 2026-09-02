#!/usr/bin/env python3
"""Download the pinned Poly Haven source packages used by the vegetation importer.

Poly Haven exposes the canonical file URLs and MD5 sums through its public API.
Only the 1K Blend package and the files referenced by that package are cached;
the unmodified sources remain outside the repository.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import urllib.request


API = "https://api.polyhaven.com/files/{asset}"
DEFAULT_ROOT = Path("~/.cache/connected-rails/polyhaven").expanduser()
ASSETS = (
    "fir_tree_01",
    "pine_tree_01",
    "fir_sapling",
    "shrub_01",
    "searsia_lucida",
    "wild_rooibos_bush",
    "fern_02",
    "nettle_plant",
    "periwinkle_plant",
)


def fetch_json(url: str) -> dict:
    request = urllib.request.Request(url, headers={"User-Agent": "Connected-Rails-asset-importer/1.0"})
    with urllib.request.urlopen(request) as response:
        return json.load(response)


def digest(path: Path) -> str:
    value = hashlib.md5()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def download(url: str, target: Path, expected_md5: str) -> None:
    if target.exists() and digest(target) == expected_md5:
        print(f"  cached {target.name}")
        return
    target.parent.mkdir(parents=True, exist_ok=True)
    temporary = target.with_suffix(target.suffix + ".part")
    request = urllib.request.Request(url, headers={"User-Agent": "Connected-Rails-asset-importer/1.0"})
    with urllib.request.urlopen(request) as response, temporary.open("wb") as output:
        while chunk := response.read(1024 * 1024):
            output.write(chunk)
    actual = digest(temporary)
    if actual != expected_md5:
        temporary.unlink(missing_ok=True)
        raise RuntimeError(f"MD5 mismatch for {target}: {actual} != {expected_md5}")
    temporary.replace(target)
    print(f"  fetched {target.name}")


def package_files(asset: str, record: dict) -> list[tuple[Path, dict]]:
    blend = record["blend"]["1k"]["blend"]
    files = [(Path(f"{asset}_1k.blend"), blend)]
    files.extend((Path(relative), metadata) for relative, metadata in blend.get("include", {}).items())
    return files


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("assets", nargs="*", choices=ASSETS, help="default: every source asset")
    parser.add_argument("--root", type=Path, default=DEFAULT_ROOT)
    args = parser.parse_args()
    assets = args.assets or ASSETS
    for asset in assets:
        print(asset)
        record = fetch_json(API.format(asset=asset))
        for relative, metadata in package_files(asset, record):
            download(metadata["url"], args.root.expanduser() / asset / relative, metadata["md5"])


if __name__ == "__main__":
    main()
