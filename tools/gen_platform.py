"""Procedural platform model for the example mod, with walkways built in.

Writes ``mods/example/assets/platform.gltf`` — created entirely from scratch in
this script, no third-party assets, so the file carries the project's licence:
a 210 m platform, 6 m wide, 0.76 m above the rail head (the German standard
height), with a tactile strip along the edge, and the nodes the simulator reads
people off (MODS.md, *People*):

  wp_edge_0 … wp_edge_6    a footpath along the platform, 2 m in from the edge:
                           empty nodes in walking order; ``extras`` on the first
                           one say how many people are on it and how wide it is
  wa_middle_0 … wa_middle_3  the corners of the waiting area behind the strip:
                           empty nodes in ring order; ``extras`` on the first one
                           say how many people are about on it and what share
                           of them wander

Conventions per MODS.md: origin at the platform's start, on the rail head, at
the track-side edge; +Y up, and the model's front −Z (Bevy's convention, the
one every vehicle and character uses) along increasing arc length — so the
body runs from z = 0 to z = −210. Placed with ``lateral_offset: -1.65`` it is a
platform on the left of the line, the body extending away from the track along
−X. The route editor drops it at that offset out of ``objects/platform.ron``.

Run: python tools/gen_platform.py
"""

from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from gen_signal_parts import Prim, write_gltf  # noqa: E402

LENGTH = 210.0
WIDTH = 6.0
HEIGHT = 0.76
# The tactile strip runs a metre from the edge; people keep behind it.
STRIP_FROM_EDGE = 1.0
STRIP_WIDTH = 0.4

CONCRETE = (
    "concrete",
    dict(baseColorFactor=[0.62, 0.61, 0.58, 1.0], metallicFactor=0.0, roughnessFactor=0.9),
    None,
)
EDGE = (
    "edge",
    dict(baseColorFactor=[0.80, 0.80, 0.78, 1.0], metallicFactor=0.0, roughnessFactor=0.85),
    None,
)
STRIP = (
    "strip",
    dict(baseColorFactor=[0.85, 0.85, 0.82, 1.0], metallicFactor=0.0, roughnessFactor=0.8),
    None,
)
WALL = (
    "wall",
    dict(baseColorFactor=[0.45, 0.44, 0.42, 1.0], metallicFactor=0.0, roughnessFactor=0.95),
    None,
)


def build():
    body = {"concrete": Prim(), "edge": Prim(), "strip": Prim(), "wall": Prim()}
    # The body runs along −Z: from the origin at the platform's start to −LENGTH.
    z0, z1 = -LENGTH, 0.0
    # The slab: 5 cm of coping along the edge, the rest concrete.
    body["edge"].box((-0.60, HEIGHT - 0.05, z0), (0.0, HEIGHT, z1))
    body["concrete"].box((-WIDTH, HEIGHT - 0.05, z0), (-0.60, HEIGHT, z1))
    # The tactile strip sits a millimetre proud so it reads as a stripe.
    body["strip"].box(
        (-STRIP_FROM_EDGE - STRIP_WIDTH, HEIGHT, z0), (-STRIP_FROM_EDGE, HEIGHT + 0.002, z1)
    )
    # The platform wall below the slab, and the ends and back closed off.
    body["wall"].box((-WIDTH, 0.0, z0), (0.0, HEIGHT - 0.05, z1))
    return body


def empties(prefix, points, extras):
    """Empty nodes ``<prefix>_<i>`` at the points, the first carrying the extras."""
    nodes = []
    for i, (x, y, z) in enumerate(points):
        node = {"name": f"{prefix}_{i}", "translation": [x, y, z]}
        if i == 0 and extras:
            node["extras"] = extras
        nodes.append(node)
    return nodes


def main():
    body = build()
    # A footpath along the platform, 3.4 m in from the edge and a metre wide — behind
    # the lane the platform device's waiting crowd walks (3.8 m from the track axis,
    # see content::people), so the two never touch.
    x_path = -3.4
    path = [(x_path, HEIGHT, -z) for z in (5.0, 40.0, 75.0, 105.0, 135.0, 170.0, 205.0)]
    # The waiting area behind the path, up to 0.7 m from the back edge.
    area = [
        (-4.2, HEIGHT, -8.0),
        (-WIDTH + 0.7, HEIGHT, -8.0),
        (-WIDTH + 0.7, HEIGHT, -(LENGTH - 8.0)),
        (-4.2, HEIGHT, -(LENGTH - 8.0)),
    ]
    nodes = [{"name": "platform", "mesh": "platform"}]
    nodes += empties("wp_edge", path, {"people": 6, "width": 1.0})
    nodes += empties("wa_middle", area, {"people": 10, "walking_share": 0.4})
    write_gltf("platform.gltf", "platform", [CONCRETE, EDGE, STRIP, WALL], [("platform", body)], nodes)


if __name__ == "__main__":
    main()
