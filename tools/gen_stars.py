#!/usr/bin/env python3
"""Bakes the naked-eye stars of the HYG database into `world-render/src/stars.bin`.

The night sky of the simulator is the real one: `sky.rs` draws one point sprite per
record, in J2000 equatorial coordinates, and turns it into the local sky with the
observer's latitude and the sidereal time. Everything down to magnitude 6.5 is in —
that is what an eye sees at a dark site, and about 8900 stars.

Record layout (little endian, 16 bytes), read back by `world_render::sky::stars`:

    f32 right ascension [rad]
    f32 declination     [rad]
    f32 apparent visual magnitude
    f32 colour index B-V

Usage:

    curl -L -o hyg.csv https://raw.githubusercontent.com/astronexus/HYG-Database/main/hyg/CURRENT/hygdata_v41.csv
    python tools/gen_stars.py hyg.csv crates/world-render/src/stars.bin

The catalogue is CC BY-SA 4.0, see THIRD_PARTY_LICENSES.md.
"""

import csv
import math
import struct
import sys

# Faintest star a dark-adapted eye picks out. Below this the sky is the Milky Way,
# which `sky.rs` draws procedurally instead — a catalogue that deep is a megabyte.
LIMIT_MAGNITUDE = 6.5

# The sun sits in the catalogue at RA/Dec 0 with magnitude -26.7; it has its own light.
SUN = "Sol"


def main(source: str, target: str) -> None:
    records = []
    with open(source, newline="", encoding="utf-8") as handle:
        for row in csv.DictReader(handle):
            if row.get("proper") == SUN:
                continue
            try:
                magnitude = float(row["mag"])
            except (KeyError, ValueError):
                continue
            if magnitude > LIMIT_MAGNITUDE:
                continue
            # `rarad`/`decrad` are the same position in radians; the hour/degree
            # columns exist for humans. Fall back for rows that lack them.
            try:
                ra = float(row["rarad"])
                dec = float(row["decrad"])
            except (KeyError, ValueError):
                ra = float(row["ra"]) * math.pi / 12.0
                dec = float(row["dec"]) * math.pi / 180.0
            try:
                colour = float(row["ci"])
            except (KeyError, ValueError):
                colour = 0.65  # sun-like, the commonest case
            records.append((ra, dec, magnitude, colour))

    records.sort(key=lambda r: r[2])
    with open(target, "wb") as out:
        for record in records:
            out.write(struct.pack("<ffff", *record))
    print(f"{len(records)} stars written to {target}")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    main(sys.argv[1], sys.argv[2])
