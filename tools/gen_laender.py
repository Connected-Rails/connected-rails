#!/usr/bin/env python3
"""Bakes the German federal state boundaries into `fields/src/laender.bin`.

Which state a point lies in decides which InVeKoS service the field import asks,
which schema it gets back and which crop code list applies (`fields::land`). That
is a lookup against sixteen polygons, so the polygons ship with the program rather
than being fetched — the editor has to answer it before it has a network.

Source: VG2500 of the Bundesamt fuer Kartographie und Geodaesie, the 1:2 500 000
boundaries, over the BKG's open WFS. dl-de/by-2-0, see THIRD_PARTY_LICENSES.md.
Only the `gf = 9` polygons are taken — the land without the sea and the big lakes.

The rings are thinned to `TOLERANCE` degrees, about half a kilometre. A field
import queries a bounding box, not a point, and asks every state the box touches
(`Land::touching`), so a coarse boundary costs one extra request near a border
rather than a wrong answer.

Record layout (little endian), read back by `fields::land::LAENDER`:

    u8       number of states
    per state:
      u8     index into `Land::ALL`
      u16    number of rings
      per ring:
        u16  number of points
        f32  longitude, f32 latitude, repeated

Usage:

    curl -o vg2500_lan.json -G https://sgx.geodatenzentrum.de/wfs_vg2500 \\
      --data-urlencode SERVICE=WFS --data-urlencode VERSION=2.0.0 \\
      --data-urlencode REQUEST=GetFeature \\
      --data-urlencode TYPENAMES=vg2500:vg2500_lan \\
      --data-urlencode SRSNAME=urn:ogc:def:crs:OGC:1.3:CRS84 \\
      --data-urlencode OUTPUTFORMAT=application/json
    python tools/gen_laender.py vg2500_lan.json crates/fields/src/laender.bin
"""

import json
import struct
import sys

# Order of `Land::ALL` in `fields/src/land.rs` — the file stores the index.
ORDER = [
    "BW",
    "BY",
    "BE",
    "BB",
    "HB",
    "HH",
    "HE",
    "MV",
    "NI",
    "NW",
    "RP",
    "SL",
    "SN",
    "ST",
    "SH",
    "TH",
]

# The official name in the source, per code.
NAMES = {
    "Baden-Württemberg": "BW",
    "Bayern": "BY",
    "Berlin": "BE",
    "Brandenburg": "BB",
    "Bremen": "HB",
    "Hamburg": "HH",
    "Hessen": "HE",
    "Mecklenburg-Vorpommern": "MV",
    "Niedersachsen": "NI",
    "Nordrhein-Westfalen": "NW",
    "Rheinland-Pfalz": "RP",
    "Saarland": "SL",
    "Sachsen": "SN",
    "Sachsen-Anhalt": "ST",
    "Schleswig-Holstein": "SH",
    "Thüringen": "TH",
}

# Douglas-Peucker tolerance [deg]. 0.005 deg is ~ 350 m north-south; the boundary
# between two states is not a place a field lies exactly on.
TOLERANCE = 0.005

# Rings smaller than this are dropped [deg^2] — North Sea sandbanks, on which
# nothing is farmed and which would each cost a ring.
MIN_AREA = 1e-4


def simplify(points, tolerance):
    """Douglas-Peucker over a ring, keeping the first and last point."""
    if len(points) < 3:
        return points
    keep = [False] * len(points)
    keep[0] = keep[-1] = True
    stack = [(0, len(points) - 1)]
    while stack:
        lo, hi = stack.pop()
        if hi - lo < 2:
            continue
        ax, ay = points[lo]
        bx, by = points[hi]
        dx, dy = bx - ax, by - ay
        norm = (dx * dx + dy * dy) ** 0.5
        worst, index = 0.0, lo
        for i in range(lo + 1, hi):
            px, py = points[i]
            if norm < 1e-12:
                d = ((px - ax) ** 2 + (py - ay) ** 2) ** 0.5
            else:
                d = abs(dx * (ay - py) - (ax - px) * dy) / norm
            if d > worst:
                worst, index = d, i
        if worst > tolerance:
            keep[index] = True
            stack.append((lo, index))
            stack.append((index, hi))
    return [p for p, k in zip(points, keep) if k]


def area(ring):
    """Shoelace area of a ring [deg^2], sign dropped."""
    total = 0.0
    for i in range(len(ring)):
        x1, y1 = ring[i]
        x2, y2 = ring[(i + 1) % len(ring)]
        total += x1 * y2 - x2 * y1
    return abs(total) / 2.0


def rings_of(geometry):
    """Outer rings of a (Multi)Polygon; holes are dropped — a state has none that
    matter for 'which service answers here'."""
    if geometry["type"] == "Polygon":
        return [geometry["coordinates"][0]]
    return [polygon[0] for polygon in geometry["coordinates"]]


def main(source: str, target: str) -> None:
    with open(source, encoding="utf-8") as handle:
        collection = json.load(handle)

    states = {}
    for feature in collection["features"]:
        properties = feature["properties"]
        # gf 9 is the land area; gf 4 repeats the same state including its water.
        if properties.get("gf") != 9:
            continue
        code = NAMES.get(properties["gen"])
        if code is None:
            raise SystemExit(f"unknown state {properties['gen']!r}")
        rings = states.setdefault(code, [])
        for ring in rings_of(feature["geometry"]):
            thin = simplify([(float(x), float(y)) for x, y in ring], TOLERANCE)
            if len(thin) >= 4 and area(thin) >= MIN_AREA:
                rings.append(thin)

    missing = [code for code in ORDER if code not in states]
    if missing:
        raise SystemExit(f"no polygons for {', '.join(missing)}")

    out = bytearray()
    out += struct.pack("<B", len(ORDER))
    points = 0
    for index, code in enumerate(ORDER):
        rings = states[code]
        out += struct.pack("<BH", index, len(rings))
        for ring in rings:
            out += struct.pack("<H", len(ring))
            for x, y in ring:
                out += struct.pack("<ff", x, y)
            points += len(ring)
        print(f"{code}: {len(rings)} rings, {sum(len(r) for r in rings)} points")

    with open(target, "wb") as handle:
        handle.write(out)
    print(f"{points} points, {len(out)} bytes -> {target}")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    main(sys.argv[1], sys.argv[2])
