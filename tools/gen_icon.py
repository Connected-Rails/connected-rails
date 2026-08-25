#!/usr/bin/env python3
"""Bakes `crates/app-icon/icon.ico` out of `crates/app-icon/icon.png`.

`icon.png` is the master and the only file to edit — a square RGBA drawing. The
`.ico` beside it is what `build.rs` puts into the Windows executables, and Windows
picks one of its sizes per place: 16 for the list view, 32 for the task bar, 256 for
the large icon view. Letting it scale a single 256 down instead gives a smudge at 16.

The scaling runs on premultiplied alpha (`RGBa`). Straight RGBA would average the
colour of the fully transparent pixels into the outline, and since those carry white,
every edge would come out with a white fringe on a dark task bar.

    python tools/gen_icon.py
"""

import pathlib
from PIL import Image

SIZES = [16, 24, 32, 48, 64, 128, 256]
ICON = pathlib.Path(__file__).resolve().parent.parent / "crates" / "app-icon"


def main() -> None:
    master = Image.open(ICON / "icon.png").convert("RGBA")
    if master.width != master.height:
        raise SystemExit(f"icon.png has to be square, is {master.size}")

    premultiplied = master.convert("RGBa")
    frames = {
        size: premultiplied.resize((size, size), Image.LANCZOS).convert("RGBA")
        for size in SIZES
        if size <= master.width
    }
    largest = frames[max(frames)]
    largest.save(
        ICON / "icon.ico",
        sizes=[(s, s) for s in frames],
        append_images=[f for s, f in frames.items() if f is not largest],
    )
    print(f"icon.ico: {', '.join(str(s) for s in frames)}")


if __name__ == "__main__":
    main()
