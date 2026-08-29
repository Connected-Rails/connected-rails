#!/usr/bin/env python3
"""Bakes North Rhine-Westphalia's InVeKoS crop code list into `fields/src/crops/nw.csv`.

Every federal state numbers its crops itself, and the numbers are what the GSA
services hand out — `115` is winter wheat in NRW and means nothing anywhere else.
The import maps them onto the dozen render groups the simulator can actually tell
apart (`fields::CropClass`), and that mapping is a CSV rather than code so a
builder who disagrees can correct a row without a compiler (plan ch. 5).

The codes are read off the service itself instead of the PDF that documents it:
the Teilschlaege layer carries `CODE`, `CODE_TXT` and the InVeKoS group `USE_CODE`
on every feature, so a large sample without geometry yields the whole list plus
how much of the state each code actually covers. That count goes into the CSV as
a comment — it is what says which rows are worth arguing about.

Usage:

    curl -o nw_codes.json -G https://www.wfs.nrw.de/umwelt/lwk_eufoerderung \\
      --data-urlencode SERVICE=WFS --data-urlencode VERSION=2.0.0 \\
      --data-urlencode REQUEST=GetFeature \\
      --data-urlencode TYPENAMES=umwelt_lwk_eufoerderung:Beantragte_und_als_foerderfaehig_festgestellte_Teilschlaege_in_NRW \\
      --data-urlencode PROPERTYNAME=CODE,CODE_TXT,USE_CODE,USE_TXT \\
      --data-urlencode COUNT=5000 --data-urlencode OUTPUTFORMAT=GEOJSON
    python tools/gen_crop_codes.py nw_codes.json crates/fields/src/crops/nw.csv

`PROPERTYNAME` leaves the geometry out, which is what makes this cheap — but the
service then writes `{"type":"MultiPolygon",}` where the geometry would be, which
is not JSON. The reader below picks the properties out with a regular expression
instead of parsing, and takes any number of input files.

The data is dl-de/by-2-0, Landwirtschaftskammer Nordrhein-Westfalen; see
THIRD_PARTY_LICENSES.md.
"""

import collections
import re
import sys

FEATURE = re.compile(
    r'"CODE":(\d+),"CODE_TXT":"([^"]*)","USE_CODE":"([^"]*)","USE_TXT":"([^"]*)"'
)

# What a code becomes when nothing more specific matches, by InVeKoS group.
BY_GROUP = {
    "GT": "summer-cereal",  # cereals — the label decides winter or summer
    "OE": "rapeseed",  # oilseeds are rape unless named otherwise
    "HF": "potato",
    "EW": "legume",
    "AF": "grassland",  # fodder: grass and clover, bar the maize
    "GL": "grassland",
    "GM": "vegetable",
    "HP": "vegetable",  # herbs and medicinal plants: beds, like vegetables
    "ZP": "vegetable",  # ornamentals: the same beds
    "DA": "orchard",  # permanent crops are trees unless they are vines
    "EP": "maize",  # energy crops: miscanthus and sudan grass stand like maize
    "PA": "fallow",
    "SL": "fallow",
    "SF": "other",
}

# Rows where the group is not enough. Judgement calls, and the reason they are in
# a CSV: what matters is what a crop *looks like* from a train window, not what
# the ministry files it under.
BY_CODE = {
    81: "orchard",  # agroforestry strips — rows of trees in a field
    88: "fallow",
    92: "fallow",
    93: "fallow",
    150: "summer-cereal",  # cereal/legume mix, mostly cereal
    171: "maize",  # grain maize sits in the cereal group
    181: "summer-cereal",  # millet
    182: "summer-cereal",  # buckwheat
    183: "maize",  # sorghum stands as tall as maize
    187: "summer-cereal",  # quinoa
    189: "summer-cereal",  # chia
    320: "other",  # sunflowers
    330: "legume",  # soya
    341: "other",  # flax
    393: "other",  # camelina
    411: "maize",  # silage maize, filed under fodder
    413: "sugar-beet",  # fodder beet
    414: "sugar-beet",  # swede
    434: "grassland",
    480: "orchard",  # orchard meadow, grazed — the trees are what one sees
    492: "fallow",  # heath
    603: "sugar-beet",
    604: "other",  # jerusalem artichoke
    610: "vegetable",
    614: "rapeseed",  # brown mustard flowers like rape and is sown as widely
    619: "rapeseed",  # white mustard, the usual catch crop
    650: "vegetable",
    690: "vegetable",
    701: "other",  # hemp
    702: "grassland",  # turf
    704: "grassland",  # canary grass
    706: "other",  # poppy
    707: "vegetable",  # strawberries
    708: "other",
    709: "other",
    710: "other",
    718: "vegetable",
    720: "vegetable",
    842: "vineyard",
    851: "vegetable",  # rhubarb
    860: "vegetable",  # asparagus
    861: "vegetable",
    863: "vegetable",  # cut roses
    871: "fallow",
    910: "fallow",  # game cover
    911: "sugar-beet",  # beet seed multiplication
    912: "grassland",  # grass seed multiplication
    913: "fallow",
    914: "other",  # trial plots
    915: "fallow",  # field margins
    917: "maize",
    919: "maize",  # seed maize
    924: "fallow",
    956: "other",  # afforestation
    564: "other",  # afforestation
    972: "grassland",
    973: "other",
    994: "other",  # clamps
    995: "other",  # forest
    996: "other",
    999: "other",
}


def render_group(code: int, label: str, group: str) -> str:
    if code in BY_CODE:
        return BY_CODE[code]
    if group == "GT":
        # "Winterweichweizen", "Sommergerste" — the season is in the name, and
        # winter and summer cereal stand differently in every month but July.
        if label.lower().startswith("winter"):
            return "winter-cereal"
        return "summer-cereal"
    return BY_GROUP.get(group, "other")


def main(sources: list[str], target: str) -> None:
    seen = collections.Counter()
    labels: dict[int, tuple[str, str]] = {}
    for source in sources:
        with open(source, encoding="utf-8", errors="replace") as handle:
            for chunk in iter(lambda h=handle: h.read(1 << 22), ""):
                for match in FEATURE.finditer(chunk):
                    code = int(match.group(1))
                    seen[code] += 1
                    labels[code] = (match.group(2).strip(), match.group(3))

    rows = []
    for code in sorted(labels):
        label, group = labels[code]
        rows.append((code, render_group(code, label, group), label, group, seen[code]))

    with open(target, "w", encoding="utf-8", newline="\n") as handle:
        handle.write(
            "# North Rhine-Westphalia: InVeKoS crop code -> render group.\n"
            "# Read off the Teilschlaege WFS, see tools/gen_crop_codes.py.\n"
            "# Columns: code, render group, detail label (as the service writes it),\n"
            "# InVeKoS group, share of the sampled parcels in per mille.\n"
        )
        total = max(sum(seen.values()), 1)
        for code, group, label, use, count in rows:
            share = 1000.0 * count / total
            handle.write(f"{code},{group},{label},{use},{share:.2f}\n")
    print(f"{len(rows)} codes from {sum(seen.values())} parcels -> {target}")


if __name__ == "__main__":
    if len(sys.argv) < 3:
        raise SystemExit(__doc__)
    main(sys.argv[1:-1], sys.argv[-1])
